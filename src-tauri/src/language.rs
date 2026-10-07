//! Which language the app speaks (PLAN §13.3): the user's pref, or macOS's preferred
//! languages when the pref is `system`.

use serde::{Deserialize, Serialize};
use timewent_core::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePref {
    #[default]
    System,
    En,
    Tr,
}

/// `preferred`: macOS preferred languages, most preferred first (`["tr-TR", "en-US"]`).
/// System → the first entry decides: Turkish if it starts with `tr`, else English.
pub fn resolve(pref: LanguagePref, preferred: &[String]) -> Lang {
    match pref {
        LanguagePref::En => Lang::En,
        LanguagePref::Tr => Lang::Tr,
        LanguagePref::System => match preferred.first() {
            Some(l) if l.to_lowercase().starts_with("tr") => Lang::Tr,
            _ => Lang::En,
        },
    }
}

/// `NSLocale.preferredLanguages`.
#[cfg(target_os = "macos")]
pub fn system_preferred() -> Vec<String> {
    use objc2_foundation::NSLocale;
    NSLocale::preferredLanguages()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn system_preferred() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn explicit_choice_wins() {
        assert_eq!(resolve(LanguagePref::Tr, &langs(&["en-US"])), Lang::Tr);
        assert_eq!(resolve(LanguagePref::En, &langs(&["tr-TR"])), Lang::En);
    }

    #[test]
    fn system_follows_the_first_preferred_language() {
        assert_eq!(
            resolve(LanguagePref::System, &langs(&["tr-TR", "en-US"])),
            Lang::Tr
        );
        assert_eq!(resolve(LanguagePref::System, &langs(&["tr"])), Lang::Tr);
        assert_eq!(
            resolve(LanguagePref::System, &langs(&["en-GB", "tr-TR"])),
            Lang::En
        );
        assert_eq!(resolve(LanguagePref::System, &langs(&["de-DE"])), Lang::En);
        assert_eq!(resolve(LanguagePref::System, &[]), Lang::En);
    }

    #[test]
    fn pref_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(LanguagePref::System).expect("json"),
            serde_json::json!("system")
        );
        let tr: LanguagePref = serde_json::from_str("\"tr\"").expect("parse");
        assert_eq!(tr, LanguagePref::Tr);
    }

    /// Reads the real macOS setting. Opt-in.
    #[test]
    #[ignore]
    fn system_preferred_is_readable() {
        let l = system_preferred();
        println!("preferred languages: {l:?}");
        assert!(!l.is_empty());
    }
}
