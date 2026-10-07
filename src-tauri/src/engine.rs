//! Sessions, views, export and config over the shared state — everything the IPC commands
//! and the tray do, without any Tauri types, so it is tested with a fake probe and an
//! in-memory store.

use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use timewent_core::{
    seen_sources, Config, Lang, Range, Sample, SeenSources, Segmenter, SessionMeta, SourceCount,
};
use timewent_probe::{Permissions, Probe};
use timewent_store::Store;

use crate::auto::AutoCfg;
use crate::config_file;
use crate::dto::{Current, SessionOverview, Status, View};
use crate::error::{Error, Result};
use crate::shared::{lock, read, write, SharedConfig, SharedCounter, SharedStore};
use crate::tracker::{Driver, OnChange, SharedAuto, SharedOpen, Tracker, TrackerThread};
use crate::views::{build_current_from, build_status, build_view};

pub type ProbeFactory = Box<dyn Fn() -> Box<dyn Probe + Send> + Send + Sync>;

/// Where "now", "today" and "is anyone here" come from; real in the app, fixed in tests.
#[derive(Clone, Copy)]
pub struct Clock {
    pub now_ms: fn() -> i64,
    pub day_start_ms: fn(i64) -> i64,
    pub week_start_ms: fn(i64) -> i64,
    /// The cheap idle/lock read used while auto mode waits for input.
    pub input_now: fn() -> timewent_probe::InputNow,
    /// Seconds east of UTC at an instant (DST-aware) — for the report's local times.
    pub offset_at: fn(i64) -> i32,
    /// IANA time zone name for the report.
    pub timezone: fn() -> String,
}

pub struct Engine {
    store: SharedStore,
    config: SharedConfig,
    /// Bumped by the tracker on every append and here on every config change: together with
    /// the session id they say whether the cached `current` is still true.
    appended: SharedCounter,
    config_gen: AtomicU64,
    current: Mutex<Option<(CurrentKey, Option<Current>)>>,
    /// The open session's samples, segmented incrementally (§11.5): each status costs the
    /// new samples' derivation plus one segmentation pass, not a reload of the session.
    live: Mutex<Option<Live>>,
    /// Closed sessions' `(in_use_ms, top_label)` by (session id, config hash) — §18.
    overviews: Mutex<HashMap<(i64, u64), OverviewNumbers>>,
    /// The session being recorded, if any (manual or auto).
    open: SharedOpen,
    /// Auto mode (§11.2); `None` = manual.
    auto: SharedAuto,
    /// Manual stop while auto is on: auto waits for a manual start (or a toggle / relaunch).
    paused: AtomicBool,
    /// The loop thread, when there is anything to do. Its mutex also serializes start/stop.
    driver: Mutex<Option<TrackerThread>>,
    on_change: Mutex<OnChange>,
    make_probe: ProbeFactory,
    config_path: Option<PathBuf>,
    clock: Clock,
}

struct Live {
    session_id: i64,
    config_gen: u64,
    segmenter: Segmenter,
}

/// `(in_use_ms, top_label)` of one session.
type OverviewNumbers = (i64, Option<String>);

/// (session, appends so far, config generation).
type CurrentKey = (i64, u64, u64);

impl Engine {
    /// Closes a session a crash left open (at its last sample), then idles: no thread runs
    /// until [`start`](Self::start).
    pub fn new(
        mut store: Store,
        config: Config,
        config_path: Option<PathBuf>,
        make_probe: ProbeFactory,
        clock: Clock,
    ) -> Result<Self> {
        if let Some(s) = store.close_dangling(config.poll_interval_ms)? {
            eprintln!(
                "timewent: closed session {} left open by a previous run",
                s.id
            );
        }
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            config: Arc::new(RwLock::new(config)),
            appended: SharedCounter::default(),
            config_gen: AtomicU64::new(0),
            current: Mutex::new(None),
            live: Mutex::new(None),
            overviews: Mutex::new(HashMap::new()),
            open: Arc::default(),
            auto: Arc::default(),
            paused: AtomicBool::new(false),
            driver: Mutex::new(None),
            on_change: Mutex::new(Arc::new(|| {})),
            make_probe,
            config_path,
            clock,
        })
    }

    /// Called whenever the loop itself opens or closes a session (auto mode).
    pub fn set_on_change(&self, f: OnChange) {
        *lock(&self.on_change) = f;
    }

    /// (Re)starts the loop thread; the first step runs at once.
    fn spawn_driver(&self, slot: &mut Option<TrackerThread>) -> Result<()> {
        if let Some(t) = slot.take() {
            t.stop();
        }
        let tracker = Tracker::new(
            (self.make_probe)(),
            self.store.clone(),
            self.config.clone(),
            self.appended.clone(),
        );
        let input = self.clock.input_now;
        let driver = Driver::new(
            tracker,
            self.open.clone(),
            self.auto.clone(),
            Box::new(input),
            lock(&self.on_change).clone(),
        );
        *slot = Some(TrackerThread::spawn(driver, self.clock.now_ms)?);
        Ok(())
    }

    fn alive(slot: &Option<TrackerThread>) -> bool {
        slot.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Starts a session now (a manual start also ends an auto pause). Already tracking → the
    /// open session.
    pub fn start(&self) -> Result<SessionMeta> {
        let mut driver = lock(&self.driver);
        self.paused.store(false, Ordering::Release);
        if let Some(meta) = lock(&self.open).clone() {
            if !Self::alive(&driver) {
                self.spawn_driver(&mut driver)?;
            }
            return Ok(meta);
        }
        let now = (self.clock.now_ms)();
        let id = lock(&self.store).start_session(now)?;
        let meta = SessionMeta {
            id,
            started_at_ms: now,
            ended_at_ms: None,
        };
        *lock(&self.open) = Some(meta.clone());
        // Restart even a waiting auto loop, so the first sample is taken now, not ≤5s later.
        if let Err(e) = self.spawn_driver(&mut driver) {
            *lock(&self.open) = None;
            lock(&self.store).end_session(id, now)?;
            return Err(e);
        }
        Ok(meta)
    }

    /// Stops the loop first, then ends the session — so no sample lands after its end. In
    /// auto mode this pauses auto until the next manual start. Not tracking → `None`.
    pub fn stop(&self) -> Result<Option<SessionMeta>> {
        let mut driver = lock(&self.driver);
        if let Some(t) = driver.take() {
            t.stop();
        }
        if lock(&self.auto).is_some() {
            self.paused.store(true, Ordering::Release);
        }
        let Some(meta) = lock(&self.open).take() else {
            return Ok(None);
        };
        *lock(&self.current) = None;
        *lock(&self.live) = None;
        let end = (self.clock.now_ms)().max(meta.started_at_ms);
        lock(&self.store).end_session(meta.id, end)?;
        Ok(Some(SessionMeta {
            ended_at_ms: Some(end),
            ..meta
        }))
    }

    /// Turns auto mode on (`Some`) or off. Toggling clears a manual pause. Turning it off
    /// keeps an open session running as a manual one.
    pub fn set_auto(&self, cfg: Option<AutoCfg>) -> Result<()> {
        let mut driver = lock(&self.driver);
        let was_on = lock(&self.auto).is_some();
        *lock(&self.auto) = cfg;
        if was_on != cfg.is_some() {
            self.paused.store(false, Ordering::Release);
        }
        let open = lock(&self.open).is_some();
        match cfg {
            Some(_) if !self.paused.load(Ordering::Acquire) && !Self::alive(&driver) => {
                self.spawn_driver(&mut driver)?;
            }
            None if !open => {
                if let Some(t) = driver.take() {
                    t.stop();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Auto mode on and not paused by a manual stop.
    pub fn is_auto(&self) -> bool {
        lock(&self.auto).is_some() && !self.paused.load(Ordering::Acquire)
    }

    pub fn is_tracking(&self) -> bool {
        lock(&self.open).is_some()
    }

    pub fn status(&self, permissions: Permissions) -> Status {
        let open = lock(&self.open).clone();
        let current = open.as_ref().and_then(|s| match self.current(s.id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("timewent: status: {e}");
                None
            }
        });
        build_status(
            open.as_ref(),
            current,
            permissions,
            self.is_auto(),
            (self.clock.now_ms)(),
        )
    }

    /// The ui polls status every second; the session is re-derived only when a sample was
    /// appended or the config changed since the last call.
    fn current(&self, session_id: i64) -> Result<Option<Current>> {
        let key = (
            session_id,
            self.appended.load(Ordering::Acquire),
            self.config_gen.load(Ordering::Acquire),
        );
        if let Some((k, c)) = lock(&self.current).as_ref() {
            if *k == key {
                return Ok(c.clone());
            }
        }
        let current = self.with_live(session_id, |seg, segments| {
            build_current_from(seg.samples(), segments, seg.config())
        })?;
        *lock(&self.current) = Some((key, current.clone()));
        Ok(current)
    }

    /// Brings the open session's live `Segmenter` up to date (new samples only) and hands its
    /// segments to `f`.
    fn with_live<T>(
        &self,
        session_id: i64,
        f: impl FnOnce(&Segmenter, &[timewent_core::Segment]) -> T,
    ) -> Result<T> {
        let config_gen = self.config_gen.load(Ordering::Acquire);
        let mut live = lock(&self.live);
        let fresh = live
            .as_ref()
            .is_some_and(|l| l.session_id == session_id && l.config_gen == config_gen);
        if !fresh {
            *live = None;
        }
        let l = live.get_or_insert_with(|| Live {
            session_id,
            config_gen,
            segmenter: Segmenter::new(self.config()),
        });
        let after = l.segmenter.samples().last().map_or(i64::MIN, |s| s.ts_ms);
        for s in lock(&self.store).samples_after(session_id, after)? {
            l.segmenter.push(s);
        }
        let segments = l.segmenter.segments();
        Ok(f(&l.segmenter, &segments))
    }

    /// Past sessions for review (PLAN §18), newest first. Closed sessions never change, so
    /// their numbers are cached per (session, config hash); the open one is computed live.
    pub fn sessions_overview(&self, limit: u32, offset: u32) -> Result<Vec<SessionOverview>> {
        let metas = lock(&self.store).sessions_page(limit, offset)?;
        let config = self.config();
        let hash = config_hash(&config);
        let mut out = Vec::with_capacity(metas.len());
        for meta in metas {
            // Look up first, in its own statement: the guard must be gone before an insert.
            let cached = lock(&self.overviews).get(&(meta.id, hash)).cloned();
            let (in_use_ms, top_label) = if meta.ended_at_ms.is_none() {
                self.with_live(meta.id, |_, segments| overview_numbers(segments))?
            } else if let Some(hit) = cached {
                hit
            } else {
                let samples = lock(&self.store).samples(meta.id)?;
                let numbers = overview_numbers(&timewent_core::segment(&samples, &config));
                lock(&self.overviews).insert((meta.id, hash), numbers.clone());
                numbers
            };
            out.push(SessionOverview {
                id: meta.id,
                started_at_ms: meta.started_at_ms,
                ended_at_ms: meta.ended_at_ms,
                in_use_ms,
                top_label,
            });
        }
        Ok(out)
    }

    /// Deletes a past session and its raw samples for good. The open session is refused.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        if lock(&self.open).as_ref().is_some_and(|m| m.id == id) {
            return Err(timewent_store::Error::SessionOpen(id).into());
        }
        lock(&self.store).delete_session(id)?;
        lock(&self.overviews).retain(|(sid, _), _| *sid != id);
        Ok(())
    }

    pub fn view(&self, range: Range, lang: Lang) -> Result<View> {
        let samples = self.samples(&range)?;
        let config = self.config();
        Ok(build_view(range, &samples, &config, lang))
    }

    /// Newest first.
    pub fn sessions(&self, limit: u32) -> Result<Vec<SessionMeta>> {
        Ok(lock(&self.store).sessions(limit)?)
    }

    /// Writes the export document to `path` (absolute, `.json`) and returns it.
    pub fn export(&self, range: Range, path: &Path, lang: Lang) -> Result<PathBuf> {
        // The path comes over IPC: refuse anything that is not plainly an export file, so a
        // compromised page could not overwrite e.g. a shell profile.
        let is_json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        if !path.is_absolute() || !is_json {
            return Err(Error::ExportPath(path.display().to_string()));
        }
        fs::write(path, self.export_text(range, lang)?)?;
        Ok(path.to_path_buf())
    }

    /// The export document as text — exactly what [`export`](Self::export) writes to a file:
    /// pretty-printed `timewent.report.v2` (PLAN §21.2) plus a final newline. Used for both
    /// "copy as json" and the file export, so the two can never differ.
    pub fn export_text(&self, range: Range, lang: Lang) -> Result<String> {
        let now = (self.clock.now_ms)();
        let samples = self.samples(&range)?;
        let config = self.config();
        let span = match &range {
            Range::Session { id } => {
                let meta = self.sessions(u32::MAX)?.into_iter().find(|s| s.id == *id);
                meta.map_or((now, now), |m| {
                    (m.started_at_ms, m.ended_at_ms.unwrap_or(now))
                })
            }
            Range::All => (
                samples.first().map_or(now, |s| s.ts_ms),
                samples
                    .last()
                    .map_or(now, |s| s.ts_ms + i64::from(config.poll_interval_ms)),
            ),
            other => self
                .bounds(other, now)
                .map_or((now, now), |(from, _)| (from, now)),
        };
        let timezone = (self.clock.timezone)();
        let offset_at = self.clock.offset_at;
        let report = timewent_core::report(&timewent_core::ReportInput {
            range: &range,
            span,
            samples: &samples,
            config: &config,
            lang,
            timezone: &timezone,
            offset_at: &offset_at,
            generated_at_ms: now,
        });
        Ok(serde_json::to_string_pretty(&report)? + "\n")
    }

    /// Apps and sites used in the last 30 days (§13.1), for picking activity members.
    pub fn seen_sources(&self) -> Result<SeenSources> {
        const WINDOW_MS: i64 = 30 * 86_400_000;
        let from = (self.clock.now_ms)() - WINDOW_MS;
        let counts: Vec<SourceCount> = lock(&self.store)
            .context_counts(from)?
            .into_iter()
            .map(|c| SourceCount {
                bundle_id: c.bundle_id,
                app_name: c.app_name,
                url: c.url,
                samples: c.samples,
                last_ts_ms: c.last_ts_ms,
            })
            .collect();
        Ok(seen_sources(&counts, &self.config()))
    }

    pub fn config(&self) -> Config {
        read(&self.config).clone()
    }

    /// Validates (core rules), persists, then swaps it in; the tracker reads it on its next
    /// tick and every view re-derives with it.
    pub fn set_config(&self, config: Config) -> Result<Config> {
        config.validate().map_err(Error::InvalidConfig)?;
        if let Some(path) = &self.config_path {
            config_file::save(path, &config)?;
        }
        *write(&self.config) = config.clone();
        self.config_gen.fetch_add(1, Ordering::Release);
        Ok(config)
    }

    /// `[from, to)` for calendar ranges; `None` for a session.
    fn bounds(&self, range: &Range, now: i64) -> Option<(i64, i64)> {
        match range {
            Range::Session { .. } => None,
            Range::Today => Some(((self.clock.day_start_ms)(now), now + 1)),
            Range::Week => Some(((self.clock.week_start_ms)(now), now + 1)),
            Range::All => Some((i64::MIN, i64::MAX)),
        }
    }

    /// The store lock is held only to load; derivation happens after it is released.
    fn samples(&self, range: &Range) -> Result<Vec<Sample>> {
        let bounds = self.bounds(range, (self.clock.now_ms)());
        let store = lock(&self.store);
        Ok(match (range, bounds) {
            (_, Some((from, to))) => store.samples_between(from, to)?,
            (Range::Session { id }, None) => store.samples(*id)?,
            (_, None) => Vec::new(),
        })
    }
}

/// In-use time and the top row's label (pass-through segments never make a row).
fn overview_numbers(segments: &[timewent_core::Segment]) -> OverviewNumbers {
    let summary = timewent_core::summarize(segments);
    let top = summary
        .rows
        .iter()
        .find(|r| !r.key.starts_with(timewent_core::PASSTHROUGH_KEY_PREFIX))
        .map(|r| r.label.clone());
    (summary.active_ms + summary.passive_ms, top)
}

/// Any config change gives a different key, so cached overviews re-derive.
fn config_hash(config: &Config) -> u64 {
    let mut h = DefaultHasher::new();
    serde_json::to_string(config)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use timewent_probe::{FakeProbe, WhenExhausted};

    use super::*;
    use crate::testkit::code;

    fn real_clock() -> Clock {
        Clock {
            now_ms: crate::clock::now_ms,
            day_start_ms: |now| now - 3_600_000,
            week_start_ms: |now| now - 7_200_000,
            input_now: || timewent_probe::InputNow {
                idle_s: 1e9,
                locked: false,
            },
            offset_at: |_| 3 * 3600,
            timezone: || "Europe/Istanbul".into(),
        }
    }

    fn fake_probe() -> ProbeFactory {
        Box::new(|| {
            let p = FakeProbe::new(vec![code(0, "timewent", "lib.rs")], WhenExhausted::Cycle);
            Box::new(p.expect("script")) as Box<dyn Probe + Send>
        })
    }

    fn fast() -> Config {
        Config {
            poll_interval_ms: 250,
            gap_after_s: 1,
            ..Config::default()
        }
    }

    fn engine_with(store: Store, config: Config, path: Option<PathBuf>) -> Engine {
        Engine::new(store, config, path, fake_probe(), real_clock()).expect("engine")
    }

    fn engine() -> Engine {
        engine_with(Store::open_in_memory().expect("store"), fast(), None)
    }

    const NO_AX: Permissions = Permissions {
        accessibility: false,
    };

    #[test]
    fn idle_engine_reports_not_tracking() {
        let e = engine();
        assert!(!e.is_tracking());
        let st = e.status(NO_AX);
        assert!(!st.tracking && st.current.is_none() && st.session_id.is_none());
    }

    #[test]
    fn session_records_samples_and_stop_closes_it() {
        let e = engine();
        let meta = e.start().expect("start");
        assert!(e.is_tracking());
        std::thread::sleep(Duration::from_millis(650));

        let st = e.status(NO_AX);
        assert_eq!(st.session_id, Some(meta.id));
        assert!(st.elapsed_ms >= 600);
        assert_eq!(
            st.current.as_ref().map(|c| c.label.as_str()),
            Some("timewent")
        );

        let ended = e.stop().expect("stop").expect("was tracking");
        assert!(ended.ended_at_ms.is_some());
        assert_eq!(e.status(NO_AX).current, None);
        assert_eq!(
            e.sessions(10).expect("sessions"),
            std::slice::from_ref(&ended)
        );

        let v = e
            .view(Range::Session { id: meta.id }, Lang::En)
            .expect("view");
        assert!(
            v.total_ms >= 750,
            "≥3 samples at 250ms, got {}ms",
            v.total_ms
        );
        assert_eq!(v.rows[0].label, "timewent");

        let today = e.view(Range::Today, Lang::En).expect("today");
        assert_eq!(today.total_ms, v.total_ms);
        let week = e.view(Range::Week, Lang::En).expect("week");
        assert_eq!((week.range, week.total_ms), (Range::Week, v.total_ms));
    }

    #[test]
    fn live_status_equals_a_full_derivation_across_appends_and_config_changes() {
        let mut store = Store::open_in_memory().expect("store");
        let id = store.start_session(0).expect("session");
        let script: Vec<Sample> = (0..40)
            .map(|i| match i / 10 {
                1 => crate::testkit::web(0, "https://chatgpt.com/c/1", "x"),
                3 => crate::testkit::web(0, "https://www.youtube.com/watch?v=1", "y"),
                _ => code(0, "proj", "lib.rs"),
            })
            .collect();
        let e = engine_with(store, Config::default(), None);
        // Drive the engine's internals as the tracker would, without a thread.
        for (i, s) in script.iter().enumerate() {
            let s = Sample {
                ts_ms: i as i64 * 1000,
                ..s.clone()
            };
            lock(&e.store).append(id, &s).expect("append");
            e.appended.fetch_add(1, Ordering::Release);
            if i == 25 {
                e.set_config(Config {
                    support_window_s: 1,
                    ..Config::default()
                })
                .expect("config");
            }
            let full = crate::views::build_current(
                &lock(&e.store).samples(id).expect("samples"),
                &e.config(),
            );
            assert_eq!(e.current(id).expect("current"), full, "after sample {i}");
        }
    }

    fn someone_here() -> timewent_probe::InputNow {
        timewent_probe::InputNow {
            idle_s: 0.2,
            locked: false,
        }
    }

    fn auto_engine(input_now: fn() -> timewent_probe::InputNow) -> Engine {
        let clock = Clock {
            input_now,
            ..real_clock()
        };
        Engine::new(
            Store::open_in_memory().expect("store"),
            fast(),
            None,
            fake_probe(),
            clock,
        )
        .expect("engine")
    }

    const AUTO: Option<AutoCfg> = Some(AutoCfg {
        split_after_s: 1800,
    });

    fn wait_for(cond: impl Fn() -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(2) {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn auto_mode_starts_a_session_when_someone_is_here() {
        let e = auto_engine(someone_here);
        let changes = Arc::new(AtomicU64::new(0));
        let c = changes.clone();
        e.set_on_change(Arc::new(move || {
            c.fetch_add(1, Ordering::AcqRel);
        }));
        e.set_auto(AUTO).expect("auto");
        assert!(wait_for(|| e.is_tracking()), "auto start");
        let st = e.status(NO_AX);
        assert!(st.tracking && st.auto);
        assert!(
            changes.load(Ordering::Acquire) >= 1,
            "the tray hears about it"
        );
        assert!(wait_for(|| e.status(NO_AX).current.is_some()), "it records");
        e.stop().expect("stop");
    }

    #[test]
    fn auto_mode_waits_while_nobody_is_here_and_off_ends_the_wait() {
        let e = auto_engine(real_clock().input_now); // idle for ages
        e.set_auto(AUTO).expect("auto");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!e.is_tracking());
        assert!(e.status(NO_AX).auto);
        assert!(Engine::alive(&lock(&e.driver)), "waiting loop runs");
        e.set_auto(None).expect("off");
        assert!(lock(&e.driver).is_none(), "nothing to do: no thread");
        assert!(!e.status(NO_AX).auto);
    }

    #[test]
    fn manual_stop_pauses_auto_until_start_or_toggle() {
        let e = auto_engine(someone_here);
        e.set_auto(AUTO).expect("auto");
        assert!(wait_for(|| e.is_tracking()));
        e.stop().expect("stop");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!e.is_tracking(), "a manual stop wins over input");
        assert!(!e.status(NO_AX).auto, "paused");
        assert!(lock(&e.driver).is_none());

        e.start().expect("manual start");
        assert!(
            e.is_tracking() && e.status(NO_AX).auto,
            "start resumes auto"
        );
        e.stop().expect("stop");
        e.set_auto(None).expect("off");
        e.set_auto(AUTO).expect("on again");
        assert!(wait_for(|| e.is_tracking()), "toggling clears the pause");
        e.stop().expect("stop");
    }

    #[test]
    fn turning_auto_off_keeps_the_open_session_as_manual() {
        let e = auto_engine(someone_here);
        e.set_auto(AUTO).expect("auto");
        assert!(wait_for(|| e.is_tracking()));
        e.set_auto(None).expect("off");
        assert!(e.is_tracking() && !e.status(NO_AX).auto);
        e.stop().expect("stop");
        assert!(!e.is_tracking());
    }

    #[test]
    fn stop_is_prompt_even_with_a_long_interval() {
        let e = engine_with(
            Store::open_in_memory().expect("store"),
            Config {
                poll_interval_ms: 60_000,
                gap_after_s: 120,
                ..Config::default()
            },
            None,
        );
        e.start().expect("start");
        std::thread::sleep(Duration::from_millis(50));
        let t = Instant::now();
        e.stop().expect("stop");
        assert!(t.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn start_while_tracking_returns_the_running_session() {
        let e = engine();
        let a = e.start().expect("start");
        let b = e.start().expect("start again");
        assert_eq!(a, b);
        e.stop().expect("stop");
        assert_eq!(e.stop().expect("stop again"), None);
    }

    #[test]
    fn new_closes_a_session_left_open_by_a_crash() {
        let mut store = Store::open_in_memory().expect("store");
        let id = store.start_session(1_000).expect("session");
        store.append(id, &code(2_000, "p", "f")).expect("append");
        let e = engine_with(store, Config::default(), None);
        assert!(!e.is_tracking());
        let s = e.sessions(1).expect("sessions");
        assert_eq!(s[0].ended_at_ms, Some(3_000));
    }

    #[test]
    fn invalid_config_is_rejected_and_the_old_one_kept() {
        let e = engine();
        let bad = Config {
            transient_max_s: 30,
            ..Config::default()
        };
        let err = e.set_config(bad).expect_err("invalid").to_string();
        assert!(
            err.contains("transient_max_s (30) must be < glance_max_s (10)"),
            "{err}"
        );
        assert_eq!(e.config(), fast());
    }

    #[test]
    fn valid_config_is_persisted_and_applied() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        let e = engine_with(
            Store::open_in_memory().expect("store"),
            Config::default(),
            Some(path.clone()),
        );
        let next = Config {
            away_after_s: 600,
            ..Config::default()
        };
        assert_eq!(e.set_config(next.clone()).expect("set"), next);
        assert_eq!(e.config(), next);
        assert_eq!(config_file::load(&path), next);
    }

    #[test]
    fn export_writes_the_document_for_a_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.json");
        let e = engine();
        let meta = e.start().expect("start");
        std::thread::sleep(Duration::from_millis(300));
        e.stop().expect("stop");

        let written = e
            .export(Range::Session { id: meta.id }, &path, Lang::En)
            .expect("export");
        assert_eq!(written, path);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["schema"], "timewent.report.v2");
        assert_eq!(v["range"]["kind"], "session");
        assert_eq!(v["range"]["timezone"], "Europe/Istanbul");
        assert!(v["totals"]["in_use_s"].as_i64().is_some_and(|s| s >= 1));
        assert!(v["timeline"].is_array() && v.get("days").is_none());
    }

    #[test]
    fn ranges_resolve_against_the_clock_bounds() {
        // Samples 90 min ago (this week, not today) and 10 min ago (today).
        let now = crate::clock::now_ms();
        let mut store = Store::open_in_memory().expect("store");
        let old = store.start_session(now - 5_400_000).expect("session");
        store
            .append(old, &code(now - 5_400_000, "a", "x"))
            .expect("append");
        store.end_session(old, now - 5_399_000).expect("end");
        let recent = store.start_session(now - 600_000).expect("session");
        store
            .append(recent, &code(now - 600_000, "b", "y"))
            .expect("append");
        store.end_session(recent, now - 599_000).expect("end");
        let e = engine_with(store, Config::default(), None);

        assert_eq!(
            e.view(Range::Today, Lang::En).expect("today").total_ms,
            1_000
        );
        assert_eq!(e.view(Range::Week, Lang::En).expect("week").total_ms, 2_000);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("week.json");
        e.export(Range::Week, &path, Lang::En).expect("export");
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["range"]["kind"], "week");
        assert_eq!(v["totals"]["in_use_s"], 2, "both sessions are this week");
        assert!(v["days"].is_array() && v.get("timeline").is_none());
        e.export(Range::Today, &path, Lang::En).expect("export");
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["totals"]["in_use_s"], 1, "only the recent one is today");
    }

    #[test]
    fn seen_sources_cover_the_last_30_days_only() {
        let now = crate::clock::now_ms();
        let mut store = Store::open_in_memory().expect("store");
        let old = store.start_session(now - 40 * 86_400_000).expect("session");
        store
            .append(old, &code(now - 40 * 86_400_000, "a", "x"))
            .expect("append");
        store
            .end_session(old, now - 40 * 86_400_000 + 1)
            .expect("end");
        let id = store.start_session(now - 60_000).expect("session");
        for i in 0..3 {
            store
                .append(
                    id,
                    &crate::testkit::web(now - 60_000 + i * 1000, "https://github.com/a", "PR"),
                )
                .expect("append");
        }
        store.end_session(id, now - 50_000).expect("end");
        let e = engine_with(store, Config::default(), None);
        let s = e.seen_sources().expect("sources");
        let apps: Vec<(&str, i64)> = s.apps.iter().map(|a| (a.name.as_str(), a.ms)).collect();
        assert_eq!(apps, [("Google Chrome", 3_000)], "VS Code was 40 days ago");
        assert_eq!(s.domains[0].label, "GitHub");
    }

    /// Two closed sessions (code, then YouTube) and the clock's "now" after both.
    fn two_past_sessions() -> (Engine, i64, i64) {
        let now = crate::clock::now_ms();
        let mut store = Store::open_in_memory().expect("store");
        let a = store.start_session(now - 7_200_000).expect("a");
        for i in 0..30 {
            store
                .append(a, &code(now - 7_200_000 + i * 1000, "proj", "lib.rs"))
                .expect("append");
        }
        store.end_session(a, now - 7_100_000).expect("end");
        let b = store.start_session(now - 600_000).expect("b");
        for i in 0..20 {
            store
                .append(
                    b,
                    &crate::testkit::web(now - 600_000 + i * 1000, "https://youtube.com/w", "v"),
                )
                .expect("append");
        }
        store.end_session(b, now - 500_000).expect("end");
        (engine_with(store, Config::default(), None), a, b)
    }

    #[test]
    fn sessions_overview_lists_newest_first_with_in_use_and_top_label() {
        let (e, a, b) = two_past_sessions();
        let o = e.sessions_overview(20, 0).expect("overview");
        let got: Vec<(i64, i64, Option<&str>)> = o
            .iter()
            .map(|s| (s.id, s.in_use_ms, s.top_label.as_deref()))
            .collect();
        assert_eq!(
            got,
            [(b, 20_000, Some("YouTube")), (a, 30_000, Some("proj"))]
        );
        assert_eq!(e.sessions_overview(1, 1).expect("page")[0].id, a);
        assert!(serde_json::to_value(&o[0]).expect("json")["ended_at_ms"].is_i64());
    }

    #[test]
    fn a_config_change_invalidates_cached_overviews() {
        let (e, _, b) = two_past_sessions();
        assert_eq!(
            e.sessions_overview(1, 0).expect("o")[0]
                .top_label
                .as_deref(),
            Some("YouTube")
        );
        let mut c = e.config();
        c.labels.get_mut("youtube.com").expect("label").label = "Tube".into();
        e.set_config(c).expect("config");
        let o = e.sessions_overview(1, 0).expect("o");
        assert_eq!((o[0].id, o[0].top_label.as_deref()), (b, Some("Tube")));
    }

    #[test]
    fn a_session_of_only_timewent_has_no_top_label() {
        let now = crate::clock::now_ms();
        let mut store = Store::open_in_memory().expect("store");
        let id = store.start_session(now - 60_000).expect("s");
        for i in 0..10 {
            store
                .append(id, &crate::testkit::me(now - 60_000 + i * 1000))
                .expect("append");
        }
        store.end_session(id, now - 50_000).expect("end");
        let e = engine_with(store, Config::default(), None);
        let o = e.sessions_overview(5, 0).expect("o");
        assert_eq!((o[0].in_use_ms, o[0].top_label.clone()), (10_000, None));
    }

    #[test]
    fn the_open_session_is_computed_live_and_cannot_be_deleted() {
        let e = engine();
        let meta = e.start().expect("start");
        std::thread::sleep(Duration::from_millis(600));
        let o = e.sessions_overview(5, 0).expect("o");
        assert_eq!((o[0].id, o[0].ended_at_ms), (meta.id, None));
        assert!(o[0].in_use_ms >= 500);
        assert_eq!(o[0].top_label.as_deref(), Some("timewent"));
        let err = e.delete_session(meta.id).expect_err("open");
        assert!(err.to_string().contains("still being recorded"), "{err}");
        e.stop().expect("stop");
    }

    #[test]
    fn a_deleted_session_is_gone_from_lists_views_and_exports() {
        let (e, a, b) = two_past_sessions();
        e.sessions_overview(20, 0).expect("warm the cache");
        e.delete_session(a).expect("delete");
        let ids: Vec<i64> = e
            .sessions_overview(20, 0)
            .expect("o")
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, [b]);
        assert_eq!(
            e.view(Range::Session { id: a }, Lang::En)
                .expect("v")
                .total_ms,
            0
        );
        assert_eq!(e.view(Range::All, Lang::En).expect("all").total_ms, 20_000);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("all.json");
        e.export(Range::All, &path, Lang::En).expect("export");
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["totals"]["in_use_s"], 20, "only b is left");
        assert!(matches!(
            e.delete_session(a),
            Err(Error::Store(timewent_store::Error::NoSuchSession(_)))
        ));
    }

    #[test]
    fn range_all_covers_every_session_ever_recorded() {
        let (e, a, b) = two_past_sessions();
        // "today" starts an hour ago in this test clock: only b; "all" has both.
        assert_eq!(
            e.view(Range::Today, Lang::En).expect("today").total_ms,
            20_000
        );
        let all = e.view(Range::All, Lang::En).expect("all");
        assert_eq!((all.range, all.total_ms), (Range::All, 50_000));
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("all.json");
        e.export(Range::All, &path, Lang::En).expect("export");
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["range"]["kind"], "all");
        assert_eq!(v["totals"]["in_use_s"], 50, "a and b");
        let names: Vec<&str> = v["where"]
            .as_array()
            .expect("where")
            .iter()
            .filter_map(|r| r["name"].as_str())
            .collect();
        assert_eq!(names, ["proj", "YouTube"]);
        let _ = (a, b);
    }

    #[test]
    fn copied_json_is_byte_identical_to_the_exported_file() {
        // A fixed clock: generated_at_ms is the same in both documents.
        let clock = Clock {
            now_ms: || 1_800_000_000_000,
            ..real_clock()
        };
        let mut store = Store::open_in_memory().expect("store");
        let id = store.start_session(1_800_000_000_000 - 60_000).expect("s");
        for i in 0..30 {
            store
                .append(
                    id,
                    &code(1_800_000_000_000 - 60_000 + i * 1000, "proj", "lib.rs"),
                )
                .expect("append");
        }
        store
            .end_session(id, 1_800_000_000_000 - 30_000)
            .expect("end");
        let e = Engine::new(store, Config::default(), None, fake_probe(), clock).expect("engine");
        let dir = tempfile::tempdir().expect("tempdir");
        for range in [Range::Session { id }, Range::Today, Range::Week, Range::All] {
            let path = dir.path().join("out.json");
            e.export(range.clone(), &path, Lang::En).expect("export");
            let file = fs::read(&path).expect("read");
            let text = e.export_text(range.clone(), Lang::En).expect("text");
            assert_eq!(text.as_bytes(), file.as_slice(), "{range:?}");
        }
        let text = e
            .export_text(Range::Session { id }, Lang::En)
            .expect("text");
        assert!(text.starts_with("{\n  \"schema\": \"timewent.report.v2\""));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn export_refuses_paths_that_are_not_absolute_json_files() {
        let e = engine();
        for p in ["/tmp/.zshrc", "relative.json", "/tmp/notes.txt"] {
            assert!(
                matches!(
                    e.export(Range::Today, Path::new(p), Lang::En),
                    Err(Error::ExportPath(_))
                ),
                "{p}"
            );
        }
    }

    /// Cost of what a status poll re-derives, on a full 8-hour session in a file store:
    /// a full derivation vs the live (incremental) path for one new sample.
    /// Opt-in: `cargo test --release -p timewent-app eight_hour -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn eight_hour_session_status_cost() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = Store::open(dir.path().join("t.db")).expect("store");
        let id = store.start_session(0).expect("session");
        let files = ["lib.rs", "views.rs", "engine.rs", "tracker.rs"];
        let pages = [
            ("https://docs.rs/serde/latest/serde/", "serde - Rust"),
            ("https://chatgpt.com/c/1", "ChatGPT"),
            ("https://github.com/x/timewent/pull/1", "PR #1"),
        ];
        let n_samples = 28_800i64;
        for i in 0..n_samples {
            let s = if (i / 300) % 3 == 0 {
                let (url, title) = pages[(i as usize / 900) % pages.len()];
                crate::testkit::web(i * 1000, url, title)
            } else {
                code(
                    i * 1000,
                    "timewent",
                    files[(i as usize / 120) % files.len()],
                )
            };
            store.append(id, &s).expect("append");
        }
        let config = Config::default();

        let t = Instant::now();
        let samples = store.samples(id).expect("samples");
        let full = crate::views::build_current(&samples, &config);
        println!("8h full (load + segment): {:?}", t.elapsed());

        let mut seg = Segmenter::new(config.clone());
        for s in samples {
            seg.push(s);
        }
        let heap: usize = seg
            .samples()
            .iter()
            .map(|s| {
                std::mem::size_of::<Sample>()
                    + s.app_name.capacity()
                    + s.bundle_id.capacity()
                    + s.window_title.as_ref().map_or(0, String::capacity)
                    + s.url.as_ref().map_or(0, String::capacity)
            })
            .sum();
        println!(
            "  live cache: ~{} KiB of samples (contexts on top)",
            heap / 1024
        );
        let rounds = 20;
        let t = Instant::now();
        let mut last = None;
        for k in 0..rounds {
            let ts = (n_samples + k) * 1000;
            let s = code(ts, "timewent", "lib.rs");
            store.append(id, &s).expect("append");
            let after = seg.samples().last().map_or(i64::MIN, |s| s.ts_ms);
            for s in store.samples_after(id, after).expect("after") {
                seg.push(s);
            }
            let segments = seg.segments();
            last = build_current_from(seg.samples(), &segments, &config);
        }
        println!("8h live, per new sample: {:?}", t.elapsed() / rounds as u32);
        assert!(full.is_some() && last.is_some());
    }

    /// Size of the today report on a COPY of a real database (never the original).
    /// `TIMEWENT_SNAPSHOT=/path/copy.db cargo test -p timewent-app real_today -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_today_report_size() {
        let Ok(path) = std::env::var("TIMEWENT_SNAPSHOT") else {
            return;
        };
        let store = Store::open(&path).expect("snapshot");
        let clock = Clock {
            now_ms: crate::clock::now_ms,
            day_start_ms: crate::clock::local_midnight_ms,
            week_start_ms: crate::clock::local_week_start_ms,
            input_now: timewent_probe::input_now,
            offset_at: crate::clock::local_offset_s,
            timezone: crate::clock::timezone_name,
        };
        let e = Engine::new(store, Config::default(), None, fake_probe(), clock).expect("engine");
        let text = e.export_text(Range::Today, Lang::Tr).expect("report");
        let schema = text
            .lines()
            .find(|l| l.contains("\"schema\""))
            .unwrap_or_default()
            .trim()
            .to_string();
        println!("real today report: {} bytes ({schema})", text.len());
        if let Ok(out) = std::env::var("TIMEWENT_SNAPSHOT_OUT") {
            std::fs::write(out, &text).expect("write");
        }
    }
}
