//! Is media keeping the display awake? Decided from the system-wide power-assertion levels
//! (DESIGN §4): video players and call apps hold `PreventUserIdleDisplaySleep` while they
//! play. Only the aggregate level is read — never which app, never what is playing.

/// Assertion types that keep the display on. `NoDisplaySleepAssertion` is the legacy name
/// some older apps still use for the same thing.
pub const DISPLAY_ASSERTIONS: &[&str] = &["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"];

/// `level(type)` is the aggregate level of an assertion type (0 = nobody holds it), or `None`
/// if the system did not report that type.
pub fn display_kept_awake(level: impl Fn(&str) -> Option<i64>) -> bool {
    DISPLAY_ASSERTIONS
        .iter()
        .any(|t| level(t).is_some_and(|l| l > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_display_assertion_means_media() {
        assert!(display_kept_awake(
            |t| (t == "PreventUserIdleDisplaySleep").then_some(255)
        ));
    }

    #[test]
    fn legacy_name_counts_too() {
        assert!(display_kept_awake(
            |t| (t == "NoDisplaySleepAssertion").then_some(255)
        ));
    }

    #[test]
    fn system_sleep_assertions_alone_are_not_media() {
        // e.g. a download or `caffeinate -i` keeps the machine awake, not the display.
        let levels = |t: &str| match t {
            "PreventUserIdleSystemSleep" => Some(255),
            "PreventUserIdleDisplaySleep" => Some(0),
            _ => None,
        };
        assert!(!display_kept_awake(levels));
    }

    #[test]
    fn nothing_reported_means_no_media() {
        assert!(!display_kept_awake(|_| None));
    }
}
