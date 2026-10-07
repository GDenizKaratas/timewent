//! Small JSON settings files in the app data dir (`config.json`, `prefs.json`). A missing
//! file means defaults; a broken one is logged and ignored (defaults) — never fatal, and never
//! overwritten until the user saves.

use std::fs;
use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use timewent_core::Config;

/// The analysis config, validated by core's rules.
/// The analysis config, validated by core's rules. Built-in labels and app categories added
/// since it was saved are merged in; the user's own entries are never overwritten.
pub fn load(path: &Path) -> Config {
    let mut c: Config = load_or_default(path, |c: &Config| {
        c.validate().map_err(|errs| errs.join("; "))
    });
    c.merge_new_defaults();
    c
}

pub fn load_or_default<T: DeserializeOwned + Default>(
    path: &Path,
    validate: impl Fn(&T) -> Result<(), String>,
) -> T {
    match try_load(path, validate) {
        Ok(Some(v)) => v,
        Ok(None) => T::default(),
        Err(msg) => {
            eprintln!(
                "timewent: ignoring {}: {msg}; using defaults",
                path.display()
            );
            T::default()
        }
    }
}

fn try_load<T: DeserializeOwned>(
    path: &Path,
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<Option<T>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let value: T = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    validate(&value)?;
    Ok(Some(value))
}

/// Written to a sibling temp file and renamed into place, so a crash mid-write cannot leave
/// a truncated file behind.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json + "\n")?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = dir();
        assert_eq!(load(&d.path().join("config.json")), Config::default());
    }

    #[test]
    fn saved_config_loads_back() {
        let d = dir();
        let path = d.path().join("config.json");
        let c = Config {
            away_after_s: 300,
            ..Config::default()
        };
        save(&path, &c).expect("save");
        assert_eq!(load(&path), c);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn unparseable_file_gives_defaults_and_is_left_alone() {
        let d = dir();
        let path = d.path().join("config.json");
        fs::write(&path, "{ not json").expect("write");
        assert_eq!(load(&path), Config::default());
        assert_eq!(fs::read_to_string(&path).expect("read"), "{ not json");
    }

    #[test]
    fn file_failing_validation_gives_defaults() {
        let d = dir();
        let path = d.path().join("config.json");
        fs::write(&path, r#"{"passive_after_s": 500, "away_after_s": 100}"#).expect("write");
        assert_eq!(load(&path), Config::default());
    }

    #[test]
    fn a_config_saved_by_an_older_version_gains_new_defaults_but_keeps_edits() {
        let d = dir();
        let path = d.path().join("config.json");
        fs::write(
            &path,
            r#"{"labels": {"youtube.com": {"label": "Tube", "category": "web"}},
                "app_categories": {}}"#,
        )
        .expect("write");
        let c = load(&path);
        assert_eq!(
            c.labels["youtube.com"].label, "Tube",
            "the user's edit stays"
        );
        assert_eq!(
            c.labels["netflix.com"].label, "Netflix",
            "new default merged in"
        );
        assert_eq!(
            c.app_categories.get("com.spotify.client"),
            Some(&timewent_core::Category::Media)
        );
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let d = dir();
        let path = d.path().join("config.json");
        fs::write(&path, r#"{"glance_max_s": 20}"#).expect("write");
        assert_eq!(
            load(&path),
            Config {
                glance_max_s: 20,
                ..Config::default()
            }
        );
    }
}
