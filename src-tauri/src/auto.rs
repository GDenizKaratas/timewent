//! Auto mode decisions (DESIGN §11.7), pure. Tracking runs whenever the app runs; a session
//! ends at the start of a long away stretch or a sleep gap, and the next one starts on input.

use timewent_core::{Config, Sample};

/// While waiting for input, the idle check runs this often (no full sampling).
pub const WAIT_MS: u64 = 5_000;
/// Input newer than this (one wait period) means "someone is back".
const RESUMED_S: f64 = WAIT_MS as f64 / 1000.0;
/// Lower bound for `auto_split_after_s`: shorter would split ordinary reading pauses.
pub const MIN_SPLIT_S: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCfg {
    pub split_after_s: u32,
}

/// What follows a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    /// The sample that revealed the split already shows input: it opens the next session.
    StartWith,
    /// Wait for input (idle check every [`WAIT_MS`]).
    Wait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Keep,
    /// End the open session at `end_ms`.
    Split {
        end_ms: i64,
        then: Then,
    },
}

/// Memory across ticks of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AutoTrack {
    prev_ts: Option<i64>,
    lock_since: Option<i64>,
}

/// Someone is at the keyboard right now.
pub fn input_resumed(idle_s: f64, locked: bool) -> bool {
    !locked && idle_s < RESUMED_S
}

impl AutoTrack {
    /// Called with every sample of an open session (before it is stored).
    pub fn on_sample(
        &mut self,
        s: &Sample,
        session_start_ms: i64,
        auto: AutoCfg,
        config: &Config,
    ) -> Decision {
        let split_ms = i64::from(auto.split_after_s) * 1000;
        let prev = self.prev_ts.replace(s.ts_ms);
        // Slept (or the tracker stalled): the session ends where its samples end.
        if let Some(p) = prev.filter(|p| s.ts_ms - *p > i64::from(config.gap_after_s) * 1000) {
            *self = AutoTrack::default();
            let then = if input_resumed(s.idle.min_s(), s.locked) {
                Then::StartWith
            } else {
                Then::Wait
            };
            return Decision::Split {
                end_ms: (p + i64::from(config.poll_interval_ms)).max(session_start_ms),
                then,
            };
        }
        if s.locked {
            let since = *self.lock_since.get_or_insert(s.ts_ms);
            if s.ts_ms - since >= split_ms {
                *self = AutoTrack::default();
                return Decision::Split {
                    end_ms: since.max(session_start_ms),
                    then: Then::Wait,
                };
            }
            return Decision::Keep;
        }
        self.lock_since = None;
        // Media playing / a call is never away (DESIGN §4).
        let idle_ms = (s.idle.min_s() * 1000.0) as i64;
        if !s.media_active && idle_ms >= split_ms {
            *self = AutoTrack::default();
            return Decision::Split {
                end_ms: (s.ts_ms - idle_ms).max(session_start_ms),
                then: Then::Wait,
            };
        }
        Decision::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{code, idle};

    const AUTO: AutoCfg = AutoCfg {
        split_after_s: 1800,
    };
    const START: i64 = 1_000_000;

    fn run(track: &mut AutoTrack, samples: &[Sample]) -> Vec<Decision> {
        samples
            .iter()
            .map(|s| track.on_sample(s, START, AUTO, &Config::default()))
            .collect()
    }

    #[test]
    fn active_work_keeps_the_session() {
        let mut t = AutoTrack::default();
        let samples: Vec<Sample> = (0..100).map(|i| code(START + i * 1000, "p", "f")).collect();
        assert!(run(&mut t, &samples).iter().all(|d| *d == Decision::Keep));
    }

    #[test]
    fn a_long_idle_stretch_splits_at_the_last_input() {
        let mut t = AutoTrack::default();
        let now = START + 5_000_000;
        let d = run(&mut t, &[idle(code(now, "p", "f"), 1800.0)]);
        assert_eq!(
            d,
            [Decision::Split {
                end_ms: now - 1_800_000,
                then: Then::Wait
            }]
        );
        let d = run(
            &mut AutoTrack::default(),
            &[idle(code(now, "p", "f"), 1799.9)],
        );
        assert_eq!(d, [Decision::Keep]);
    }

    #[test]
    fn the_end_never_precedes_the_session_start() {
        let mut t = AutoTrack::default();
        let d = run(&mut t, &[idle(code(START + 10_000, "p", "f"), 4000.0)]);
        assert_eq!(
            d,
            [Decision::Split {
                end_ms: START,
                then: Then::Wait
            }]
        );
    }

    #[test]
    fn media_is_never_away() {
        let mut s = idle(code(START + 5_000_000, "p", "f"), 7200.0);
        s.media_active = true;
        assert_eq!(run(&mut AutoTrack::default(), &[s]), [Decision::Keep]);
    }

    #[test]
    fn a_locked_screen_splits_from_the_moment_it_locked() {
        let mut t = AutoTrack::default();
        let lock_at = START + 60_000;
        let locked: Vec<Sample> = (0..=1800)
            .map(|i| {
                let mut s = code(lock_at + i * 1000, "p", "f");
                s.locked = true;
                s
            })
            .collect();
        let d = run(&mut t, &locked);
        assert!(d[..1800].iter().all(|d| *d == Decision::Keep));
        assert_eq!(
            d[1800],
            Decision::Split {
                end_ms: lock_at,
                then: Then::Wait
            }
        );
    }

    #[test]
    fn unlocking_resets_the_lock_clock() {
        let mut t = AutoTrack::default();
        // Locked at START, unlocked a second later, locked again 1799s after that.
        let samples: Vec<Sample> = (0..=1801)
            .map(|i| {
                let mut s = code(START + i * 1000, "p", "f");
                s.locked = i == 0 || i == 1801;
                s
            })
            .collect();
        assert!(run(&mut t, &samples).iter().all(|d| *d == Decision::Keep));
    }

    #[test]
    fn a_sleep_gap_ends_the_session_at_its_last_sample() {
        let mut t = AutoTrack::default();
        let before = code(START + 10_000, "p", "f");
        let woke_typing = code(START + 3_600_000, "p", "f");
        let d = run(&mut t, &[before, woke_typing]);
        assert_eq!(
            d[1],
            Decision::Split {
                end_ms: START + 11_000,
                then: Then::StartWith
            }
        );
        let mut t = AutoTrack::default();
        let mut woke_locked = code(START + 3_600_000, "p", "f");
        woke_locked.locked = true;
        let d = run(&mut t, &[code(START + 10_000, "p", "f"), woke_locked]);
        assert_eq!(
            d[1],
            Decision::Split {
                end_ms: START + 11_000,
                then: Then::Wait
            }
        );
    }

    #[test]
    fn waiting_resumes_on_fresh_unlocked_input_only() {
        assert!(input_resumed(0.3, false));
        assert!(input_resumed(4.9, false));
        assert!(!input_resumed(5.0, false));
        assert!(!input_resumed(0.1, true));
    }
}
