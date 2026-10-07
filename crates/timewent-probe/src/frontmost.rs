//! Pure decisions about the frontmost app: identity, title, lock state.

/// Frontmost while the screen is locked or the screensaver runs.
const LOCK_SCREEN_BUNDLES: &[&str] = &["com.apple.loginwindow", "com.apple.ScreenSaver.Engine"];

/// Locked if the session says so, or if the lock screen / screensaver is frontmost (the
/// session flag can lag, and the screensaver does not set it until the password grace ends).
pub fn is_locked(session_says_locked: bool, frontmost_bundle_id: &str) -> bool {
    session_says_locked || LOCK_SCREEN_BUNDLES.contains(&frontmost_bundle_id)
}

/// `(app_name, bundle_id)` for a `Sample`. Apps without an Info.plist have no bundle id; apps
/// without a localized name fall back to the bundle id; nothing frontmost → `unknown`.
pub fn app_identity(name: Option<String>, bundle_id: Option<String>) -> (String, String) {
    let name = name.filter(|n| !n.is_empty());
    let bundle_id = bundle_id.unwrap_or_default();
    let app_name = name.unwrap_or_else(|| {
        if bundle_id.is_empty() {
            "unknown".to_string()
        } else {
            bundle_id.clone()
        }
    });
    (app_name, bundle_id)
}

/// AX reports untitled windows as `""`; that carries nothing, so it is recorded as `None`.
pub fn normalize_title(title: Option<String>) -> Option<String> {
    title.filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lock_flag_means_locked() {
        assert!(is_locked(true, "com.microsoft.VSCode"));
        assert!(!is_locked(false, "com.microsoft.VSCode"));
    }

    #[test]
    fn loginwindow_or_screensaver_frontmost_means_locked() {
        assert!(is_locked(false, "com.apple.loginwindow"));
        assert!(is_locked(false, "com.apple.ScreenSaver.Engine"));
        assert!(!is_locked(false, "com.apple.finder"));
    }

    #[test]
    fn identity_prefers_the_localized_name() {
        assert_eq!(
            app_identity(Some("Code".into()), Some("com.microsoft.VSCode".into())),
            ("Code".into(), "com.microsoft.VSCode".into())
        );
    }

    #[test]
    fn identity_falls_back_to_bundle_id_then_unknown() {
        assert_eq!(
            app_identity(None, Some("x.y".into())),
            ("x.y".into(), "x.y".into())
        );
        assert_eq!(
            app_identity(Some(String::new()), Some("x.y".into())),
            ("x.y".into(), "x.y".into())
        );
        assert_eq!(
            app_identity(Some("tool".into()), None),
            ("tool".into(), String::new())
        );
        assert_eq!(app_identity(None, None), ("unknown".into(), String::new()));
    }

    #[test]
    fn empty_title_is_none_other_titles_are_kept_verbatim() {
        assert_eq!(normalize_title(Some(String::new())), None);
        assert_eq!(normalize_title(None), None);
        assert_eq!(
            normalize_title(Some("main.rs — timewent".into())).as_deref(),
            Some("main.rs — timewent")
        );
        assert_eq!(normalize_title(Some(" x ".into())).as_deref(), Some(" x "));
    }
}
