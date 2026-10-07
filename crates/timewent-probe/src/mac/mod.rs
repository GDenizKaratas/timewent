//! The real probe. Thin: reads raw values from the OS and hands them to the pure modules.

mod ax;
mod cg;
mod power;
mod workspace;

use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};
use timewent_core::{Audio, Sample};

use crate::audio::{media_hosts, pick, AudioCache, Holder};

use crate::frontmost::{app_identity, is_locked, normalize_title};
use crate::osascript::osascript;
use crate::url_cache::{QueryFailed, UrlCache};
use crate::url_script::parse_output;
use crate::Probe;

pub(crate) use ax::{accessibility_trusted, request_accessibility};

/// Samples the frontmost app, window title (Accessibility), browser url (AppleScript), input
/// idleness and lock state.
///
/// `NSWorkspace.frontmostApplication` only updates while the **main** run loop runs. In the
/// app, Tauri runs it and `MacProbe` may live on any thread; a CLI must pump it itself
/// (see [`pump_main_run_loop`]).
#[derive(Debug)]
pub struct MacProbe {
    urls: UrlCache,
    audio: AudioCache,
    /// Hosts whose tabs count as media (from the default labels).
    media_hosts: Vec<String>,
    /// Monotonic base for url backoff, immune to wall-clock jumps.
    started: Instant,
}

impl MacProbe {
    pub fn new() -> Self {
        Self {
            urls: UrlCache::new(),
            audio: AudioCache::new(),
            media_hosts: media_hosts(&timewent_core::Config::default()),
            started: Instant::now(),
        }
    }

    fn sample_now(&mut self, now_ms: i64) -> Sample {
        let front = workspace::frontmost();
        let pid = front.as_ref().map(|f| f.pid);
        let (app_name, bundle_id) = match front {
            Some(f) => app_identity(f.name, f.bundle_id),
            None => app_identity(None, None),
        };
        let locked = is_locked(cg::session_locked(), &bundle_id);
        // While locked the frontmost window is not what the user is doing; reading its title or
        // url would cost work and record something nobody looked at.
        let mono_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let audio = if locked {
            None
        } else {
            self.audio_now(&bundle_id, mono_ms)
        };
        let (window_title, url) = if locked {
            (None, None)
        } else {
            let title = pid
                .filter(|p| *p > 0)
                .and_then(ax::focused_window_title)
                .and_then(|t| normalize_title(Some(t)));
            let url = self
                .urls
                .resolve(&bundle_id, title.as_deref(), mono_ms, query_url);
            (title, url)
        };
        Sample {
            ts_ms: now_ms,
            app_name,
            bundle_id,
            window_title,
            url,
            idle: cg::idle(),
            locked,
            // Nothing is read while locked; presence ignores media then anyway.
            media_active: !locked && power::media_active(),
            audio,
        }
    }

    /// The audio source of this sample (decision Q4), with its title when it can be told.
    fn audio_now(&mut self, frontmost_bundle: &str, mono_ms: u64) -> Option<Audio> {
        let own = i32::try_from(std::process::id()).unwrap_or(-1);
        let holders: Vec<Holder> = power::audio_holders()
            .into_iter()
            .filter(|(pid, _)| *pid != own)
            .filter_map(|(pid, started_s)| {
                let (app, bundle_id) = workspace::app_of_pid(pid)?;
                Some(Holder {
                    pid,
                    bundle_id,
                    app,
                    started_s,
                })
            })
            .collect();
        let holder = pick(&holders, frontmost_bundle)?;
        let playing = self
            .audio
            .resolve(holder, &self.media_hosts, mono_ms, |script| {
                osascript(script).map_err(QueryFailed::from)
            });
        Some(Audio {
            bundle_id: holder.bundle_id.clone(),
            app: holder.app.clone(),
            title: playing.title,
            host: playing.host,
        })
    }
}

impl Default for MacProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe for MacProbe {
    fn sample(&mut self, now_ms: i64) -> Sample {
        // Cocoa getters may autorelease; on a plain background thread nothing would drain them.
        autoreleasepool(|_| self.sample_now(now_ms))
    }
}

pub(crate) fn input_now() -> crate::InputNow {
    autoreleasepool(|_| crate::InputNow {
        idle_s: cg::idle().min_s(),
        locked: cg::session_locked(),
    })
}

fn query_url(script: &str) -> Result<Option<String>, QueryFailed> {
    osascript(script)
        .map(|out| parse_output(&out))
        .map_err(QueryFailed::from)
}

/// Runs the current thread's run loop for `duration` (call it on the main thread). For CLIs:
/// it both waits and lets AppKit deliver the frontmost-app updates `MacProbe` reads.
pub fn pump_main_run_loop(duration: Duration) {
    let deadline = Instant::now() + duration;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        // SAFETY: reading an immutable framework constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        let result = CFRunLoop::run_in_mode(mode, left.as_secs_f64(), false);
        if result == CFRunLoopRunResult::Finished {
            // No sources registered: nothing to deliver, so just wait out the rest.
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_probe_can_move_to_the_tracking_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<MacProbe>();
    }

    /// Cost of the power-assertion read that runs on every sample. Opt-in: real OS.
    #[test]
    #[ignore]
    fn media_active_is_cheap() {
        let n = 200;
        let t = Instant::now();
        let mut held = 0;
        for _ in 0..n {
            held += usize::from(power::media_active());
        }
        let per = t.elapsed() / n;
        println!("media_active: {per:?} per call, held {held}/{n}");
        assert!(per < Duration::from_millis(5), "{per:?}");
    }

    /// Cost of one auto-mode idle check (runs every 5s while waiting). Opt-in: real OS.
    #[test]
    #[ignore]
    fn input_now_is_cheap() {
        let n = 200;
        let t = Instant::now();
        for _ in 0..n {
            assert!(input_now().idle_s >= 0.0);
        }
        let per = t.elapsed() / n;
        println!("input_now: {per:?} per check");
        assert!(per < Duration::from_millis(5), "{per:?}");
    }

    /// Touches the real OS (and may run osascript if a browser is frontmost): opt-in only.
    #[test]
    #[ignore]
    fn real_sample_is_plausible() {
        let s = MacProbe::new().sample(1_000);
        assert_eq!(s.ts_ms, 1_000);
        assert!(!s.app_name.is_empty());
        assert!(s.idle.min_s() >= 0.0);
    }
}
