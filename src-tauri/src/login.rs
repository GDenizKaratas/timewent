//! Open at login (DESIGN §11.8). Pure decisions; `lib.rs` performs them with
//! `tauri-plugin-autostart` (a LaunchAgent that runs the bundle's executable with `--login`).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The argument the LaunchAgent passes, so a login launch can be told apart.
pub const LOGIN_ARG: &str = "--login";

/// Started by the login item (not by the user).
pub fn launched_at_login(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|a| a == LOGIN_ARG)
}

/// Only a built app bundle registers: a `cargo run` / `tauri dev` binary must never become the
/// thing that starts at login.
pub fn in_app_bundle(exe: &Path) -> bool {
    exe.to_string_lossy().contains(".app/Contents/MacOS/")
}

/// First time this version sees the setting: no prefs file yet, or one written before
/// `launch_at_login` existed. Then the default (on) is applied once by registering.
pub fn first_time(prefs_json: Option<&str>) -> bool {
    match prefs_json {
        None => true,
        Some(text) => serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v.get("launch_at_login").cloned())
            .is_none(),
    }
}

/// The window's last top-left, logical points, top-left origin (Tauri's coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowPos {
    pub x: f64,
    pub y: f64,
}

/// A screen's work area in the same coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The saved position, if the pill would still be on a screen there (a monitor may be gone).
pub fn restore(saved: Option<WindowPos>, areas: &[Area], width: f64) -> Option<WindowPos> {
    let p = saved?;
    areas
        .iter()
        .any(|a| p.x + width > a.x && p.x < a.x + a.w && p.y >= a.y && p.y + 56.0 <= a.y + a.h)
        .then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_launches_carry_the_login_argument() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(launched_at_login(args(&["/x/timewent", "--login"])));
        assert!(!launched_at_login(args(&["/x/timewent"])));
    }

    #[test]
    fn only_a_bundle_registers() {
        assert!(in_app_bundle(Path::new(
            "/Users/me/timewent/target/release/bundle/macos/timewent.app/Contents/MacOS/timewent"
        )));
        assert!(!in_app_bundle(Path::new(
            "/Users/me/timewent/target/debug/timewent"
        )));
    }

    #[test]
    fn first_time_is_no_file_or_a_file_without_the_setting() {
        assert!(first_time(None));
        assert!(first_time(Some(r#"{"always_on_top": true}"#)));
        assert!(first_time(Some("not json")));
        assert!(!first_time(Some(r#"{"launch_at_login": false}"#)));
        assert!(!first_time(Some(r#"{"launch_at_login": true}"#)));
    }

    const MAIN: Area = Area {
        x: 0.0,
        y: 38.0,
        w: 1512.0,
        h: 906.0,
    };

    #[test]
    fn a_saved_position_on_a_screen_is_reused() {
        let p = WindowPos { x: 961.0, y: 87.0 };
        assert_eq!(restore(Some(p), &[MAIN], 340.0), Some(p));
    }

    #[test]
    fn a_position_on_a_missing_monitor_or_off_screen_is_dropped() {
        assert_eq!(
            restore(Some(WindowPos { x: 2000.0, y: 87.0 }), &[MAIN], 340.0),
            None
        );
        assert_eq!(
            restore(
                Some(WindowPos {
                    x: 900.0,
                    y: 1468.0
                }),
                &[MAIN],
                340.0
            ),
            None
        );
        assert_eq!(
            restore(Some(WindowPos { x: 900.0, y: 10.0 }), &[MAIN], 340.0),
            None
        );
        assert_eq!(restore(None, &[MAIN], 340.0), None);
        let second = Area {
            x: 1512.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        assert_eq!(
            restore(
                Some(WindowPos { x: 2000.0, y: 87.0 }),
                &[MAIN, second],
                340.0
            ),
            Some(WindowPos { x: 2000.0, y: 87.0 })
        );
    }
}
