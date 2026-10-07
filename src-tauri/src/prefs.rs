//! App preferences: how the window behaves, not how time is analysed (that is core
//! `Config`). Persisted as `prefs.json` next to `config.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::auto::{AutoCfg, MIN_SPLIT_S};
use crate::config_file;
use crate::language::LanguagePref;
use crate::shortcut::{parse_peek, DEFAULT_PEEK};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Keep the pill above other windows.
    pub always_on_top: bool,
    /// Global shortcut that peeks (PLAN §10.4), Tauri accelerator format.
    pub peek_shortcut: String,
    /// Track whenever the app runs; split sessions at long away stretches (§11.2).
    pub auto_track: bool,
    /// Away this long (seconds) ends an auto session. At least [`MIN_SPLIT_S`].
    pub auto_split_after_s: u32,
    /// `system` follows macOS's preferred languages (§13.3).
    pub language: LanguagePref,
    /// Start with the Mac, hidden as the pill (§22).
    pub launch_at_login: bool,
}

impl Prefs {
    /// Auto mode settings, if on.
    pub fn auto(&self) -> Option<AutoCfg> {
        self.auto_track.then_some(AutoCfg {
            split_after_s: self.auto_split_after_s,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        parse_peek(&self.peek_shortcut)?;
        if self.auto_split_after_s < MIN_SPLIT_S {
            return Err(format!(
                "auto_split_after_s ({}) must be >= {MIN_SPLIT_S}",
                self.auto_split_after_s
            ));
        }
        Ok(())
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            always_on_top: true,
            peek_shortcut: DEFAULT_PEEK.to_string(),
            auto_track: false,
            auto_split_after_s: 1800,
            language: LanguagePref::System,
            launch_at_login: true,
        }
    }
}

/// A file with an unusable shortcut keeps its other prefs and falls back to the default key.
pub fn load(path: &Path) -> Prefs {
    let mut prefs: Prefs = config_file::load_or_default(path, |_: &Prefs| Ok(()));
    if let Err(e) = parse_peek(&prefs.peek_shortcut) {
        eprintln!("timewent: {e}; using {DEFAULT_PEEK}");
        prefs.peek_shortcut = DEFAULT_PEEK.to_string();
    }
    if prefs.auto_split_after_s < MIN_SPLIT_S {
        prefs.auto_split_after_s = Prefs::default().auto_split_after_s;
    }
    prefs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_the_window_on_top() {
        assert!(Prefs::default().always_on_top);
        assert_eq!(
            serde_json::to_value(Prefs::default()).expect("json"),
            serde_json::json!({
                "always_on_top": true, "peek_shortcut": "Alt+Shift+Space",
                                "auto_track": false, "auto_split_after_s": 1800, "language": "system",
                "launch_at_login": true
            })
        );
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prefs.json");
        assert_eq!(load(&path), Prefs::default());
        std::fs::write(&path, "nope").expect("write");
        assert_eq!(load(&path), Prefs::default());
    }

    #[test]
    fn saved_prefs_load_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prefs.json");
        let p = Prefs {
            always_on_top: false,
            peek_shortcut: "Cmd+Shift+T".into(),
            auto_track: true,
            auto_split_after_s: 900,
            language: LanguagePref::Tr,
            launch_at_login: false,
        };
        config_file::save(&path, &p).expect("save");
        assert_eq!(load(&path), p);
    }

    #[test]
    fn unusable_shortcut_in_file_falls_back_to_default_keeping_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prefs.json");
        std::fs::write(
            &path,
            r#"{"always_on_top": false, "peek_shortcut": "Shift+A"}"#,
        )
        .expect("write");
        assert_eq!(
            load(&path),
            Prefs {
                always_on_top: false,
                peek_shortcut: DEFAULT_PEEK.into(),
                ..Prefs::default()
            }
        );
    }

    #[test]
    fn prefs_from_before_peek_existed_get_the_default_key() {
        let p: Prefs = serde_json::from_str(r#"{"always_on_top": false}"#).expect("parse");
        assert_eq!(p.peek_shortcut, DEFAULT_PEEK);
    }

    #[test]
    fn auto_is_off_by_default_and_validated() {
        let p = Prefs::default();
        assert_eq!(p.auto(), None);
        let on = Prefs {
            auto_track: true,
            ..Prefs::default()
        };
        assert_eq!(
            on.auto(),
            Some(AutoCfg {
                split_after_s: 1800
            })
        );
        let short = Prefs {
            auto_split_after_s: 299,
            ..Prefs::default()
        };
        assert_eq!(
            short.validate(),
            Err("auto_split_after_s (299) must be >= 300".into())
        );
        assert_eq!(Prefs::default().validate(), Ok(()));
    }

    #[test]
    fn empty_object_fills_defaults() {
        let p: Prefs = serde_json::from_str("{}").expect("parse");
        assert_eq!(p, Prefs::default());
    }
}
