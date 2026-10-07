//! Was the user there? `classify_presence` (DESIGN §4).

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::sample::Sample;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    Active,
    Passive,
    Away,
}

/// One presence per sample, same order. Expects samples sorted by `ts_ms`.
pub fn classify_presence(samples: &[Sample], config: &Config) -> Vec<Presence> {
    let mut out: Vec<Presence> = samples.iter().map(|s| instant(s, config)).collect();

    // Retroactive rule: reading that ends in walking off was walking off all along (after the
    // passive grace). A run of non-active samples ends at an active sample or a gap.
    let mut run_start = 0;
    for i in 0..samples.len() {
        if i > 0 && config.is_gap(samples[i - 1].ts_ms, samples[i].ts_ms) {
            settle_run(&mut out[run_start..i], &samples[run_start..i]);
            run_start = i;
        }
        if out[i] == Presence::Active {
            settle_run(&mut out[run_start..i], &samples[run_start..i]);
            run_start = i + 1;
        }
    }
    settle_run(&mut out[run_start..], &samples[run_start..]);
    out
}

/// `run` holds only non-active samples. Samples held passive by media are exempt: someone
/// was watching or in a call then, whatever happened afterwards.
fn settle_run(run: &mut [Presence], samples: &[Sample]) {
    if run.contains(&Presence::Away) {
        run.iter_mut()
            .zip(samples)
            .filter(|(p, s)| **p == Presence::Passive && !held_by_media(s))
            .for_each(|(p, _)| *p = Presence::Away);
    }
}

fn held_by_media(sample: &Sample) -> bool {
    sample.media_active && !sample.locked
}

/// Presence of one sample in isolation, before the retroactive rule.
fn instant(sample: &Sample, config: &Config) -> Presence {
    let idle = sample.idle.min_s();
    if sample.locked {
        Presence::Away
    } else if idle < f64::from(config.passive_after_s) {
        Presence::Active
    } else if idle < f64::from(config.away_after_s) || held_by_media(sample) {
        // Media playing or a call: idle input never means away (DESIGN §4).
        Presence::Passive
    } else {
        Presence::Away
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{idles, sample};
    use Presence::*;

    fn one(idle_s: f64) -> Presence {
        classify_presence(&[sample(0, "A", idle_s)], &Config::default())[0]
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(classify_presence(&[], &Config::default()).is_empty());
    }

    #[test]
    fn idle_just_under_passive_threshold_is_active() {
        assert_eq!(one(0.0), Active);
        assert_eq!(one(44.9), Active);
    }

    #[test]
    fn idle_at_passive_threshold_is_passive() {
        assert_eq!(one(45.0), Passive);
        assert_eq!(one(179.9), Passive);
    }

    #[test]
    fn idle_at_away_threshold_is_away() {
        assert_eq!(one(180.0), Away);
    }

    #[test]
    fn locked_is_away_even_with_recent_input() {
        let mut s = sample(0, "A", 0.0);
        s.locked = true;
        assert_eq!(classify_presence(&[s], &Config::default()), vec![Away]);
    }

    #[test]
    fn scroll_only_input_keeps_sample_active() {
        let mut s = sample(0, "A", 300.0);
        s.idle.scroll_s = 2.0;
        assert_eq!(classify_presence(&[s], &Config::default()), vec![Active]);
    }

    #[test]
    fn passive_run_that_reaches_away_becomes_away_retroactively() {
        let samples = idles(0, "A", &[10.0, 45.0, 100.0, 179.0, 180.0, 181.0]);
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Active, Away, Away, Away, Away, Away]
        );
    }

    #[test]
    fn passive_run_ending_in_input_stays_passive() {
        let samples = idles(0, "A", &[10.0, 45.0, 100.0, 179.0, 0.5]);
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Active, Passive, Passive, Passive, Active]
        );
    }

    #[test]
    fn active_sample_separates_passive_from_later_away() {
        let samples = idles(0, "A", &[50.0, 60.0, 1.0, 200.0]);
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Passive, Passive, Active, Away]
        );
    }

    fn media(mut samples: Vec<Sample>, on: &[bool]) -> Vec<Sample> {
        for (s, m) in samples.iter_mut().zip(on) {
            s.media_active = *m;
        }
        samples
    }

    #[test]
    fn media_keeps_long_idle_passive_not_away() {
        let s = media(idles(0, "A", &[10.0, 50.0, 200.0, 600.0]), &[true; 4]);
        assert_eq!(
            classify_presence(&s, &Config::default()),
            vec![Active, Passive, Passive, Passive]
        );
    }

    #[test]
    fn locked_is_away_even_while_media_plays() {
        let mut s = media(idles(0, "A", &[5.0]), &[true]);
        s[0].locked = true;
        assert_eq!(classify_presence(&s, &Config::default()), vec![Away]);
    }

    #[test]
    fn media_stretch_is_not_turned_away_retroactively() {
        // A video ends, then nobody comes back: the watching stays passive, the rest is away.
        let s = media(
            idles(0, "A", &[10.0, 50.0, 200.0, 400.0, 500.0, 600.0]),
            &[false, true, true, true, false, false],
        );
        assert_eq!(
            classify_presence(&s, &Config::default()),
            vec![Active, Passive, Passive, Passive, Away, Away]
        );
    }

    #[test]
    fn plain_reading_in_the_same_run_still_turns_away() {
        let s = media(idles(0, "A", &[50.0, 60.0, 300.0]), &[false, true, false]);
        assert_eq!(
            classify_presence(&s, &Config::default()),
            vec![Away, Passive, Away]
        );
    }

    #[test]
    fn locking_after_reading_turns_the_reading_into_away() {
        let mut samples = idles(0, "A", &[50.0, 51.0, 52.0]);
        samples[2].locked = true;
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Away, Away, Away]
        );
    }

    #[test]
    fn gap_breaks_the_retroactive_run() {
        let samples = vec![
            sample(0, "A", 50.0),
            sample(1_000, "A", 51.0),
            sample(60_000, "A", 200.0), // after sleep
        ];
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Passive, Passive, Away]
        );
    }

    #[test]
    fn jump_of_exactly_gap_after_does_not_break_the_run() {
        let samples = vec![sample(0, "A", 50.0), sample(5_000, "A", 200.0)];
        assert_eq!(
            classify_presence(&samples, &Config::default()),
            vec![Away, Away]
        );
    }
}
