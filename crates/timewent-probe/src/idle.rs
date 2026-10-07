//! Input idleness from per-event-type "seconds since last event" readings.
//!
//! Only *time since* an event type is read (CGEventSource), never events themselves: no event
//! tap, no Input Monitoring permission, structurally unable to see keys (PLAN §1.3).

use timewent_core::Idle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleKind {
    Keyboard,
    Mouse,
    Click,
    Scroll,
}

/// CGEventType codes read each poll, and the `Idle` field each one feeds (minimum wins).
/// Modifier presses and drags/secondary buttons count, so e.g. a long drag is not "idle mouse".
pub const IDLE_EVENT_TYPES: &[(u32, IdleKind)] = &[
    (10, IdleKind::Keyboard), // keyDown
    (12, IdleKind::Keyboard), // flagsChanged (modifier keys)
    (5, IdleKind::Mouse),     // mouseMoved
    (6, IdleKind::Mouse),     // leftMouseDragged
    (7, IdleKind::Mouse),     // rightMouseDragged
    (27, IdleKind::Mouse),    // otherMouseDragged
    (1, IdleKind::Click),     // leftMouseDown
    (3, IdleKind::Click),     // rightMouseDown
    (25, IdleKind::Click),    // otherMouseDown
    (22, IdleKind::Scroll),   // scrollWheel
];

/// Upper bound for a reading (~31 years): keeps JSON finite whatever the OS returns.
pub const MAX_IDLE_S: f64 = 1e9;

/// Folds readings into `Idle`, each field the minimum of its event types, quantized to whole
/// milliseconds so a sample round-trips through the store unchanged.
pub fn fold_idle(readings: impl IntoIterator<Item = (IdleKind, f64)>) -> Idle {
    let mut idle = Idle {
        keyboard_s: MAX_IDLE_S,
        mouse_s: MAX_IDLE_S,
        click_s: MAX_IDLE_S,
        scroll_s: MAX_IDLE_S,
    };
    for (kind, secs) in readings {
        let secs = quantize_s(secs);
        let field = match kind {
            IdleKind::Keyboard => &mut idle.keyboard_s,
            IdleKind::Mouse => &mut idle.mouse_s,
            IdleKind::Click => &mut idle.click_s,
            IdleKind::Scroll => &mut idle.scroll_s,
        };
        *field = field.min(secs);
    }
    idle
}

/// Clamps to `[0, MAX_IDLE_S]` (NaN → 0) and rounds to the nearest millisecond.
pub fn quantize_s(secs: f64) -> f64 {
    if secs.is_nan() {
        return 0.0;
    }
    let ms = (secs.clamp(0.0, MAX_IDLE_S) * 1000.0).round();
    ms / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use IdleKind::*;

    #[test]
    fn each_field_is_the_minimum_of_its_event_types() {
        let idle = fold_idle([
            (Keyboard, 30.0),
            (Keyboard, 2.0),
            (Mouse, 9.0),
            (Mouse, 4.0),
            (Click, 50.0),
            (Click, 70.0),
            (Scroll, 0.5),
        ]);
        assert_eq!(
            idle,
            Idle {
                keyboard_s: 2.0,
                mouse_s: 4.0,
                click_s: 50.0,
                scroll_s: 0.5
            }
        );
    }

    #[test]
    fn a_field_without_readings_is_maximally_idle() {
        let idle = fold_idle([(Keyboard, 1.0)]);
        assert_eq!(idle.mouse_s, MAX_IDLE_S);
        assert_eq!(idle.min_s(), 1.0);
    }

    #[test]
    fn readings_are_quantized_to_milliseconds() {
        assert_eq!(quantize_s(1.234_56), 1.235);
        assert_eq!(quantize_s(0.000_4), 0.0);
        let idle = fold_idle([(Scroll, 12.345_678)]);
        assert_eq!(idle.scroll_s, 12.346);
    }

    #[test]
    fn nonsense_readings_are_clamped() {
        assert_eq!(quantize_s(f64::NAN), 0.0);
        assert_eq!(quantize_s(-1.0), 0.0);
        assert_eq!(quantize_s(f64::INFINITY), MAX_IDLE_S);
        assert_eq!(quantize_s(1e300), MAX_IDLE_S);
    }

    #[test]
    fn every_idle_kind_is_read_and_codes_are_unique() {
        for kind in [Keyboard, Mouse, Click, Scroll] {
            assert!(IDLE_EVENT_TYPES.iter().any(|(_, k)| *k == kind), "{kind:?}");
        }
        let mut codes: Vec<u32> = IDLE_EVENT_TYPES.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), IDLE_EVENT_TYPES.len());
    }

    #[test]
    fn spec_event_types_feed_the_spec_fields() {
        // PLAN §5: keyDown, mouseMoved, leftMouseDown, scrollWheel.
        for (code, kind) in [(10, Keyboard), (5, Mouse), (1, Click), (22, Scroll)] {
            assert!(IDLE_EVENT_TYPES.contains(&(code, kind)), "{code}");
        }
    }
}
