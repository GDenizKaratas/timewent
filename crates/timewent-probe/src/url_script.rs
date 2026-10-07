//! The AppleScript that asks a browser for its active url, and how to read the answer.

/// Chromium browsers share Chrome's scripting dictionary.
const CHROMIUM: &[&str] = &[
    "com.google.Chrome",
    "company.thebrowser.Browser", // Arc
    "com.brave.Browser",
    "com.microsoft.edgemac",
];
const SAFARI: &str = "com.apple.Safari";

/// `None` for anything that is not a scriptable browser (Firefox has no url in AppleScript).
///
/// The script addresses the app by bundle id (names differ by locale/edition), checks
/// `is running` first so a browser that just quit is never relaunched, and returns `""` when
/// there is no window rather than erroring (an error would trigger the failure backoff).
pub fn script_for(bundle_id: &str) -> Option<String> {
    let url_of_front = if CHROMIUM.contains(&bundle_id) {
        "if (count of windows) > 0 then return URL of active tab of front window"
    } else if bundle_id == SAFARI {
        "if (count of documents) > 0 then return URL of front document"
    } else {
        return None;
    };
    Some(format!(
        "if application id \"{bundle_id}\" is running then\n\
         \ttell application id \"{bundle_id}\"\n\
         \t\t{url_of_front}\n\
         \tend tell\n\
         end if\n\
         return \"\"\n"
    ))
}

/// `osascript` stdout → url. Blank output and AppleScript's `missing value` mean "no url".
pub fn parse_output(stdout: &str) -> Option<String> {
    let url = stdout.trim();
    if url.is_empty() || url == "missing value" {
        None
    } else {
        Some(url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_browsers_ask_for_the_active_tab_by_bundle_id() {
        for id in CHROMIUM {
            let s = script_for(id).expect("chromium script");
            assert!(
                s.contains(&format!("application id \"{id}\" is running")),
                "{s}"
            );
            assert!(s.contains("URL of active tab of front window"), "{s}");
        }
    }

    #[test]
    fn safari_asks_for_the_front_document() {
        let s = script_for("com.apple.Safari").expect("safari script");
        assert!(s.contains("URL of front document"));
        assert!(!s.contains("active tab"));
    }

    #[test]
    fn non_scriptable_apps_have_no_script() {
        assert_eq!(script_for("org.mozilla.firefox"), None);
        assert_eq!(script_for("com.microsoft.VSCode"), None);
        assert_eq!(script_for(""), None);
    }

    #[test]
    fn script_never_errors_on_no_window_or_quit_browser() {
        let s = script_for("com.google.Chrome").expect("script");
        assert!(s.contains("count of windows"));
        assert!(s.trim_end().ends_with("return \"\""));
    }

    #[test]
    fn output_is_trimmed() {
        assert_eq!(
            parse_output("https://docs.rs/serde\n").as_deref(),
            Some("https://docs.rs/serde")
        );
        assert_eq!(
            parse_output("  https://a.b/c  \r\n").as_deref(),
            Some("https://a.b/c")
        );
    }

    #[test]
    fn blank_or_missing_value_output_is_no_url() {
        assert_eq!(parse_output(""), None);
        assert_eq!(parse_output("\n"), None);
        assert_eq!(parse_output("missing value\n"), None);
    }
}
