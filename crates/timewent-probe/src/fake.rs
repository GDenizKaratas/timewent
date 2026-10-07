//! A scripted probe for tests and for running the app without macOS APIs.

use timewent_core::Sample;

use crate::Probe;

/// What a `FakeProbe` does once every scripted sample has been returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenExhausted {
    /// Start again from the first sample.
    Cycle,
    /// Keep returning the last sample.
    HoldLast,
}

/// Returns scripted samples in order, stamped with the `now_ms` it is asked at (the caller's
/// clock is the source of truth, exactly as with the real probe).
#[derive(Debug, Clone)]
pub struct FakeProbe {
    script: Vec<Sample>,
    next: usize,
    when_exhausted: WhenExhausted,
}

impl FakeProbe {
    /// `None` for an empty script: there would be nothing to return.
    pub fn new(script: Vec<Sample>, when_exhausted: WhenExhausted) -> Option<Self> {
        if script.is_empty() {
            return None;
        }
        Some(Self {
            script,
            next: 0,
            when_exhausted,
        })
    }
}

impl Probe for FakeProbe {
    fn sample(&mut self, now_ms: i64) -> Sample {
        let i = match self.when_exhausted {
            WhenExhausted::Cycle => self.next % self.script.len(),
            WhenExhausted::HoldLast => self.next.min(self.script.len() - 1),
        };
        self.next = self.next.saturating_add(1);
        Sample {
            ts_ms: now_ms,
            ..self.script[i].clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timewent_core::Idle;

    fn app(name: &str) -> Sample {
        Sample {
            ts_ms: 0,
            app_name: name.into(),
            bundle_id: format!("test.{name}"),
            window_title: None,
            url: None,
            idle: Idle {
                keyboard_s: 1.0,
                mouse_s: 1.0,
                click_s: 1.0,
                scroll_s: 1.0,
            },
            locked: false,
            media_active: false,
            audio: None,
        }
    }

    fn names(p: &mut FakeProbe, n: i64) -> Vec<String> {
        (0..n).map(|t| p.sample(t * 1000).app_name).collect()
    }

    #[test]
    fn cycles_through_the_script() {
        let mut p =
            FakeProbe::new(vec![app("A"), app("B")], WhenExhausted::Cycle).expect("non-empty");
        assert_eq!(names(&mut p, 5), ["A", "B", "A", "B", "A"]);
    }

    #[test]
    fn holds_the_last_sample() {
        let mut p =
            FakeProbe::new(vec![app("A"), app("B")], WhenExhausted::HoldLast).expect("non-empty");
        assert_eq!(names(&mut p, 4), ["A", "B", "B", "B"]);
    }

    #[test]
    fn stamps_the_requested_time() {
        let mut p = FakeProbe::new(vec![app("A")], WhenExhausted::Cycle).expect("non-empty");
        assert_eq!(p.sample(42_000).ts_ms, 42_000);
        let s = p.sample(43_000);
        assert_eq!(
            s,
            Sample {
                ts_ms: 43_000,
                ..app("A")
            }
        );
    }

    #[test]
    fn empty_script_is_rejected() {
        assert!(FakeProbe::new(vec![], WhenExhausted::Cycle).is_none());
    }

    #[test]
    fn usable_as_a_boxed_probe() {
        let fake = FakeProbe::new(vec![app("A")], WhenExhausted::Cycle).expect("non-empty");
        let mut probe: Box<dyn Probe + Send> = Box::new(fake);
        assert_eq!(probe.sample(1).app_name, "A");
    }
}
