//! The tracking loop (PLAN §6, §11.2).
//!
//! [`Tracker`] samples and stores; [`Driver::step`] is one iteration of the loop — a tick
//! while a session is open, an idle check while auto mode waits for input — and returns
//! when to run next. Both are driven directly by tests with a synthetic clock.
//! [`TrackerThread`] only adds timing: absolute deadlines, prompt stop, and it exists only
//! while there is something to do (zero wakeups when idle, PLAN §1).

use std::io;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use timewent_core::{Sample, SessionMeta};
use timewent_probe::{InputNow, Probe};
use timewent_store::SessionId;

use crate::auto::{input_resumed, AutoCfg, AutoTrack, Decision, Then, WAIT_MS};
use crate::shared::{lock, read, SharedConfig, SharedCounter, SharedStore};

/// The open session, if any. Written by the engine (manual start/stop) and the driver
/// (auto splits / resumes).
pub type SharedOpen = Arc<Mutex<Option<SessionMeta>>>;
/// Auto mode settings; `None` = manual.
pub type SharedAuto = Arc<Mutex<Option<AutoCfg>>>;
/// Called after the driver opened or closed a session (tray / ui refresh).
pub type OnChange = Arc<dyn Fn() + Send + Sync>;
pub type InputFn = Box<dyn Fn() -> InputNow + Send>;

pub struct Tracker {
    probe: Box<dyn Probe + Send>,
    store: SharedStore,
    config: SharedConfig,
    appended: SharedCounter,
}

impl Tracker {
    pub fn new(
        probe: Box<dyn Probe + Send>,
        store: SharedStore,
        config: SharedConfig,
        appended: SharedCounter,
    ) -> Self {
        Self {
            probe,
            store,
            config,
            appended,
        }
    }

    /// No lock held: a url lookup can take ~130ms.
    pub fn sample(&mut self, now_ms: i64) -> Sample {
        self.probe.sample(now_ms)
    }

    /// Every sample is stored as observed, passthrough apps included; what it means is the
    /// derivation's job. `appended` counts successes so readers know derived state is stale.
    pub fn append(
        &self,
        session_id: SessionId,
        sample: &Sample,
    ) -> Result<(), timewent_store::Error> {
        lock(&self.store).append(session_id, sample)?;
        self.appended.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn tick(
        &mut self,
        now_ms: i64,
        session_id: SessionId,
    ) -> Result<(), timewent_store::Error> {
        let sample = self.sample(now_ms);
        self.append(session_id, &sample)
    }

    /// The period to wait before the next tick, as currently configured — read fresh each
    /// time, so a settings change applies from the next tick on.
    pub fn interval_ms(&self) -> u32 {
        read(&self.config).poll_interval_ms
    }
}

/// One loop's state: the tracker plus what auto mode needs.
pub struct Driver {
    pub tracker: Tracker,
    pub open: SharedOpen,
    pub auto: SharedAuto,
    pub input: InputFn,
    pub on_change: OnChange,
    track: AutoTrack,
}

impl Driver {
    pub fn new(
        tracker: Tracker,
        open: SharedOpen,
        auto: SharedAuto,
        input: InputFn,
        on_change: OnChange,
    ) -> Self {
        Self {
            tracker,
            open,
            auto,
            input,
            on_change,
            track: AutoTrack::default(),
        }
    }

    /// One iteration at wall time `now_ms`. Returns ms until the next one (0 = at once), or
    /// `None` when there is nothing left to do (manual mode, no session): the thread exits.
    pub fn step(&mut self, now_ms: i64) -> Option<u64> {
        let auto = *lock(&self.auto);
        let open = lock(&self.open).clone();
        let interval = u64::from(self.tracker.interval_ms());
        match (open, auto) {
            (None, None) => None,
            (None, Some(_)) => {
                let input = (self.input)();
                if !input_resumed(input.idle_s, input.locked) {
                    return Some(WAIT_MS);
                }
                self.open_session(now_ms);
                Some(0)
            }
            (Some(meta), auto) => {
                let sample = self.tracker.sample(now_ms);
                let decision = match auto {
                    Some(a) => {
                        let config = read(&self.tracker.config).clone();
                        self.track
                            .on_sample(&sample, meta.started_at_ms, a, &config)
                    }
                    None => Decision::Keep,
                };
                match decision {
                    Decision::Keep => {
                        self.store(meta.id, &sample);
                        Some(interval)
                    }
                    Decision::Split { end_ms, then } => {
                        self.close_session(&meta, end_ms);
                        if then == Then::StartWith {
                            if let Some(id) = self.open_session(sample.ts_ms) {
                                self.store(id, &sample);
                            }
                            Some(interval)
                        } else {
                            Some(WAIT_MS)
                        }
                    }
                }
            }
        }
    }

    fn store(&self, session_id: SessionId, sample: &Sample) {
        if let Err(e) = self.tracker.append(session_id, sample) {
            eprintln!("timewent: tick failed: {e}");
        }
    }

    fn open_session(&mut self, ts_ms: i64) -> Option<SessionId> {
        self.track = AutoTrack::default();
        match lock(&self.tracker.store).start_session(ts_ms) {
            Ok(id) => {
                *lock(&self.open) = Some(SessionMeta {
                    id,
                    started_at_ms: ts_ms,
                    ended_at_ms: None,
                });
                (self.on_change)();
                Some(id)
            }
            Err(e) => {
                eprintln!("timewent: auto start failed: {e}");
                None
            }
        }
    }

    fn close_session(&mut self, meta: &SessionMeta, end_ms: i64) {
        if let Err(e) = lock(&self.tracker.store).end_session(meta.id, end_ms) {
            eprintln!("timewent: auto split failed: {e}");
        }
        *lock(&self.open) = None;
        (self.on_change)();
    }
}

/// When to tick next, as ms offsets from the loop's (monotonic) start.
///
/// Ticks sit on the grid `prev + interval`, so slow ticks do not drift the schedule. Running
/// late by up to one interval fires immediately (one catch-up tick); later than that, the
/// missed ticks are dropped and the loop resumes at the first grid point not in the past —
/// it never bursts samples to make up for lost time.
pub fn next_deadline(prev_deadline: u64, interval_ms: u64, now: u64) -> u64 {
    let interval = interval_ms.max(1);
    let next = prev_deadline.saturating_add(interval);
    if now <= next.saturating_add(interval) {
        return next;
    }
    let missed = (now - next).div_ceil(interval);
    next.saturating_add(missed.saturating_mul(interval))
}

/// The thread running a [`Driver`]. Dropping it without [`stop`](Self::stop) also ends the
/// loop (the channel disconnects), but does not wait for it.
pub struct TrackerThread {
    stop: Sender<()>,
    handle: JoinHandle<()>,
}

impl TrackerThread {
    /// First step happens immediately. `wall_ms` stamps samples (unix ms); scheduling uses
    /// the monotonic clock, which on macOS pauses during sleep — waking resumes the grid
    /// instead of bursting, and the wall-clock jump shows up as a gap.
    pub fn spawn(mut driver: Driver, wall_ms: fn() -> i64) -> io::Result<Self> {
        let (stop, stop_rx) = mpsc::channel::<()>();
        let handle = thread::Builder::new()
            .name("timewent-tracker".into())
            .spawn(move || {
                let start = Instant::now();
                let since_start = || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                let mut deadline: u64 = 0;
                loop {
                    let wait = deadline.saturating_sub(since_start());
                    let stopped = if wait > 0 {
                        !matches!(
                            stop_rx.recv_timeout(Duration::from_millis(wait)),
                            Err(RecvTimeoutError::Timeout)
                        )
                    } else {
                        !matches!(stop_rx.try_recv(), Err(TryRecvError::Empty))
                    };
                    if stopped {
                        return;
                    }
                    let Some(next_in) = driver.step(wall_ms()) else {
                        return;
                    };
                    let now = since_start();
                    deadline = if next_in == 0 {
                        now
                    } else {
                        next_deadline(deadline, next_in, now)
                    };
                }
            })?;
        Ok(Self { stop, handle })
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Wakes the loop and waits for it: at most one in-flight tick (bounded by the probe's
    /// own timeouts), never a full interval.
    pub fn stop(self) {
        // A send error means the loop is already gone; joining is still right.
        let _ = self.stop.send(());
        if self.handle.join().is_err() {
            eprintln!("timewent: tracker thread panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::{Arc, Mutex, RwLock};

    use timewent_core::{Config, Range};
    use timewent_probe::{FakeProbe, WhenExhausted};
    use timewent_store::Store;

    use super::*;
    use crate::testkit::{code, idle, me, web};
    use crate::views::{build_current, build_view};

    const T0: i64 = 1_700_000_000_000;

    struct Rig {
        tracker: Tracker,
        store: SharedStore,
        config: SharedConfig,
        appended: SharedCounter,
        session: SessionId,
    }

    fn rig(script: Vec<Sample>, config: Config) -> Rig {
        let mut store = Store::open_in_memory().expect("store");
        let session = store.start_session(T0).expect("session");
        let store = Arc::new(Mutex::new(store));
        let config = Arc::new(RwLock::new(config));
        let appended = SharedCounter::default();
        let probe = FakeProbe::new(script, WhenExhausted::HoldLast).expect("script");
        Rig {
            tracker: Tracker::new(
                Box::new(probe),
                store.clone(),
                config.clone(),
                appended.clone(),
            ),
            store,
            config,
            appended,
            session,
        }
    }

    fn script() -> Vec<Sample> {
        let mut s: Vec<_> = (0..10).map(|_| code(0, "timewent", "lib.rs")).collect();
        s.extend((0..5).map(|_| web(0, "https://github.com/x/y", "PR")));
        s
    }

    fn stored(r: &Rig) -> Vec<Sample> {
        lock(&r.store).samples(r.session).expect("samples")
    }

    #[test]
    fn ticks_append_samples_that_add_up_in_the_view() {
        let mut r = rig(script(), Config::default());
        for k in 0..15 {
            r.tracker.tick(T0 + k * 1000, r.session).expect("tick");
        }
        let samples = stored(&r);
        assert_eq!(samples.len(), 15);
        assert_eq!(
            samples[3].ts_ms,
            T0 + 3_000,
            "stamped with the tick's clock"
        );
        let v = build_view(
            Range::Session { id: r.session },
            &samples,
            &Config::default(),
            timewent_core::Lang::En,
        );
        assert_eq!(v.total_ms, 15_000);
        assert_eq!(v.rows[0].label, "timewent");
    }

    #[test]
    fn current_follows_the_latest_tick() {
        let mut r = rig(script(), Config::default());
        r.tracker.tick(T0, r.session).expect("tick");
        let now = build_current(&stored(&r), &Config::default()).expect("current");
        assert_eq!((now.label.as_str(), now.context_ms), ("timewent", 1_000));
        for k in 1..15 {
            r.tracker.tick(T0 + k * 1000, r.session).expect("tick");
        }
        let now = build_current(&stored(&r), &Config::default()).expect("current");
        assert_eq!(now.label, "GitHub");
    }

    #[test]
    fn passthrough_samples_are_stored_raw() {
        let mut r = rig(
            vec![me(0), code(0, "proj", "a.rs"), me(0)],
            Config::default(),
        );
        for k in 0..3 {
            r.tracker.tick(T0 + k * 1000, r.session).expect("tick");
        }
        let samples = stored(&r);
        assert_eq!(samples.len(), 3, "raw data is never filtered");
        assert_eq!(samples[2].bundle_id, "dev.timewent.app");
    }

    #[test]
    fn successful_appends_are_counted() {
        let mut r = rig(script(), Config::default());
        r.tracker.tick(T0, r.session).expect("tick");
        r.tracker.tick(T0 + 1000, r.session).expect("tick");
        assert_eq!(r.appended.load(Ordering::Acquire), 2);
        assert!(r.tracker.tick(T0 + 1000, r.session).is_err());
        assert_eq!(r.appended.load(Ordering::Acquire), 2);
    }

    #[test]
    fn config_change_applies_from_the_next_tick() {
        let r = rig(script(), Config::default());
        assert_eq!(r.tracker.interval_ms(), 1000);
        crate::shared::write(&r.config).poll_interval_ms = 2000;
        assert_eq!(r.tracker.interval_ms(), 2000);
    }

    // ── driver ────────────────────────────────────────────────

    struct Drive {
        driver: Driver,
        store: SharedStore,
        open: SharedOpen,
        changes: Arc<AtomicU32>,
        idle_ms: Arc<AtomicU64>,
    }

    /// A driver with no session open; `auto` decides manual vs auto mode.
    fn drive(script: Vec<Sample>, auto: Option<AutoCfg>) -> Drive {
        let r = rig(script, Config::default());
        lock(&r.store).end_session(r.session, T0).expect("end");
        let open: SharedOpen = Arc::default();
        let changes = Arc::new(AtomicU32::new(0));
        let idle_ms = Arc::new(AtomicU64::new(60_000));
        let c = changes.clone();
        let i = idle_ms.clone();
        Drive {
            driver: Driver::new(
                r.tracker,
                open.clone(),
                Arc::new(Mutex::new(auto)),
                Box::new(move || InputNow {
                    idle_s: i.load(Ordering::Acquire) as f64 / 1000.0,
                    locked: false,
                }),
                Arc::new(move || {
                    c.fetch_add(1, Ordering::AcqRel);
                }),
            ),
            store: r.store,
            open,
            changes,
            idle_ms,
        }
    }

    const AUTO: Option<AutoCfg> = Some(AutoCfg {
        split_after_s: 1800,
    });

    fn sessions(d: &Drive) -> Vec<SessionMeta> {
        let mut s = lock(&d.store).sessions(10).expect("sessions");
        s.reverse();
        s
    }

    #[test]
    fn manual_mode_without_a_session_has_nothing_to_do() {
        let mut d = drive(script(), None);
        assert_eq!(d.driver.step(T0), None);
    }

    #[test]
    fn manual_mode_ticks_the_open_session() {
        let mut d = drive(script(), None);
        let id = lock(&d.store).start_session(T0 + 1).expect("start");
        *lock(&d.open) = Some(SessionMeta {
            id,
            started_at_ms: T0 + 1,
            ended_at_ms: None,
        });
        assert_eq!(d.driver.step(T0 + 1000), Some(1000));
        assert_eq!(lock(&d.store).samples(id).expect("samples").len(), 1);
    }

    #[test]
    fn auto_waits_with_cheap_checks_until_input_then_opens_a_session() {
        let mut d = drive(script(), AUTO);
        assert_eq!(
            d.driver.step(T0 + 10_000),
            Some(WAIT_MS),
            "nobody here: wait"
        );
        assert!(lock(&d.open).is_none());
        assert_eq!(d.changes.load(Ordering::Acquire), 0);

        d.idle_ms.store(800, Ordering::Release);
        assert_eq!(
            d.driver.step(T0 + 15_000),
            Some(0),
            "input: start and tick at once"
        );
        let meta = lock(&d.open).clone().expect("open");
        assert_eq!(meta.started_at_ms, T0 + 15_000);
        assert_eq!(d.changes.load(Ordering::Acquire), 1);

        assert_eq!(d.driver.step(T0 + 15_000), Some(1000));
        assert_eq!(lock(&d.store).samples(meta.id).expect("samples").len(), 1);
    }

    #[test]
    fn auto_splits_a_long_away_stretch_then_resumes_on_input() {
        let walk_off = idle(code(0, "p", "f"), 1800.0);
        let mut d = drive(vec![code(0, "p", "f"), walk_off, code(0, "p", "f")], AUTO);
        d.idle_ms.store(0, Ordering::Release);
        let t = T0 + 100_000;
        assert_eq!(d.driver.step(t), Some(0));
        assert_eq!(d.driver.step(t), Some(1000)); // working
                                                  // Next sample shows 30 minutes without input: the session ends at the last input.
        let later = t + 1_801_000;
        assert_eq!(d.driver.step(later), Some(WAIT_MS));
        assert!(lock(&d.open).is_none());
        let s = sessions(&d);
        assert_eq!(
            s.last().and_then(|m| m.ended_at_ms),
            Some(later - 1_800_000)
        );
        // Back at the keyboard: a new session.
        assert_eq!(d.driver.step(later + 5_000), Some(0));
        assert_eq!(
            sessions(&d).len(),
            3,
            "the rig's, the split one, the new one"
        );
        assert_eq!(d.changes.load(Ordering::Acquire), 3);
    }

    #[test]
    fn auto_carries_the_first_sample_after_sleep_into_a_new_session() {
        let mut d = drive(vec![code(0, "p", "f")], AUTO);
        d.idle_ms.store(0, Ordering::Release);
        assert_eq!(d.driver.step(T0), Some(0));
        assert_eq!(d.driver.step(T0), Some(1000));
        // Lid closed for an hour; the first sample after waking shows typing.
        assert_eq!(d.driver.step(T0 + 3_600_000), Some(1000));
        let s = sessions(&d);
        assert_eq!(
            s[1].ended_at_ms,
            Some(T0 + 1000),
            "ends at its last sample + poll"
        );
        let new = lock(&d.open).clone().expect("new session");
        assert_eq!(new.started_at_ms, T0 + 3_600_000);
        assert_eq!(lock(&d.store).samples(new.id).expect("samples").len(), 1);
    }

    #[test]
    fn turning_auto_off_while_waiting_ends_the_loop() {
        let mut d = drive(script(), AUTO);
        assert_eq!(d.driver.step(T0), Some(WAIT_MS));
        *lock(&d.driver.auto) = None;
        assert_eq!(d.driver.step(T0 + 5_000), None);
    }

    #[test]
    fn on_time_ticks_follow_the_grid() {
        assert_eq!(next_deadline(0, 1000, 2), 1000);
        assert_eq!(next_deadline(1000, 1000, 1001), 2000);
        assert_eq!(next_deadline(5000, 1000, 6000), 6000);
    }

    #[test]
    fn slow_tick_does_not_drift_the_schedule() {
        // A 130ms url lookup: the next deadline stays on the grid.
        assert_eq!(next_deadline(1000, 1000, 1130), 2000);
    }

    #[test]
    fn late_by_at_most_one_interval_fires_immediately_once() {
        // Deadline 2000 already passed at 2500: return it, the loop ticks without waiting.
        assert_eq!(next_deadline(1000, 1000, 2500), 2000);
        assert_eq!(next_deadline(1000, 1000, 3000), 2000);
    }

    #[test]
    fn late_by_more_than_an_interval_skips_ahead_without_bursting() {
        // Stalled until 7300: deadlines 2000..7000 are dropped; next is 8000 on the grid.
        assert_eq!(next_deadline(1000, 1000, 7300), 8000);
        assert_eq!(next_deadline(1000, 1000, 3001), 4000);
        // Exactly on a grid point: that one is still in the future-or-now.
        assert_eq!(next_deadline(1000, 1000, 6000), 6000);
    }

    #[test]
    fn interval_change_rebases_on_the_last_deadline() {
        assert_eq!(next_deadline(3000, 250, 3010), 3250);
        assert_eq!(next_deadline(3000, 5000, 3010), 8000);
    }

    #[test]
    fn zero_interval_cannot_spin() {
        assert_eq!(next_deadline(10, 0, 10), 11);
    }

    #[test]
    fn thread_ticks_and_stops_promptly() {
        let config = Config {
            poll_interval_ms: 250,
            gap_after_s: 1,
            ..Config::default()
        };
        let r = rig(script(), config);
        fn wall() -> i64 {
            crate::clock::now_ms()
        }
        let open: SharedOpen = Arc::new(Mutex::new(Some(SessionMeta {
            id: r.session,
            started_at_ms: T0,
            ended_at_ms: None,
        })));
        let driver = Driver::new(
            r.tracker,
            open,
            Arc::default(),
            Box::new(|| InputNow {
                idle_s: 0.0,
                locked: false,
            }),
            Arc::new(|| {}),
        );
        let t = TrackerThread::spawn(driver, wall).expect("spawn");
        std::thread::sleep(Duration::from_millis(600));
        let before = Instant::now();
        t.stop();
        assert!(
            before.elapsed() < Duration::from_millis(200),
            "stop waited for a tick"
        );
        let n = lock(&r.store).samples(r.session).expect("samples").len();
        assert!(
            (2..=4).contains(&n),
            "expected ~3 ticks in 600ms at 250ms, got {n}"
        );
    }
}
