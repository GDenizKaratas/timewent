//! What the user was looking at, derived from one sample: `derive_context` (PLAN §3.3).

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::sample::Sample;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Code,
    Ai,
    Docs,
    Web,
    Media,
    Social,
    Comms,
    Design,
    Notes,
    App,
}

impl Category {
    /// The serde name (`"code"`, `"ai"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Code => "code",
            Category::Ai => "ai",
            Category::Docs => "docs",
            Category::Web => "web",
            Category::Media => "media",
            Category::Social => "social",
            Category::Comms => "comms",
            Category::Design => "design",
            Category::Notes => "notes",
            Category::App => "app",
        }
    }
}

/// The grouping identity of a sample. Samples with equal `key` belong to the same activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub key: String,
    pub label: String,
    /// File (editors) or page / host (browsers).
    pub detail: Option<String>,
    pub category: Category,
}

const CODE_EDITORS: &[&str] = &[
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "com.exafunction.windsurf",
];

/// Window-title suffixes editors append; they carry no information about the work.
const EDITOR_APP_NAMES: &[&str] = &[
    "Visual Studio Code",
    "Visual Studio Code - Insiders",
    "Cursor",
    "Windsurf",
];

const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "company.thebrowser.Browser", // Arc
    "com.brave.Browser",
    "com.apple.Safari",
    "com.microsoft.edgemac",
    "org.mozilla.firefox",
];

/// Rules, first match wins: code editor → browser (labels, then docs, then host) → any other app.
pub(crate) fn is_browser(bundle_id: &str) -> bool {
    BROWSERS.contains(&bundle_id)
}

pub fn derive_context(sample: &Sample, config: &Config) -> Context {
    let bundle = sample.bundle_id.as_str();
    let title = clean_title(sample.window_title.as_deref());
    if CODE_EDITORS.contains(&bundle) {
        return editor_context(&sample.app_name, title);
    }
    if BROWSERS.contains(&bundle) {
        if let Some(host) = sample.url.as_deref().and_then(web_host) {
            return browser_context(host, title, config);
        }
        return app_context(&sample.app_name, title, Category::Web);
    }
    let category = config
        .app_categories
        .get(bundle)
        .copied()
        .unwrap_or(Category::App);
    app_context(&sample.app_name, title, category)
}

/// Whitespace-only titles carry nothing; treat them as absent.
fn clean_title(title: Option<&str>) -> Option<&str> {
    title.map(str::trim).filter(|t| !t.is_empty())
}

fn editor_context(app_name: &str, title: Option<&str>) -> Context {
    let tokens = title.map(editor_title_tokens).unwrap_or_default();
    let (label, detail) = match tokens.as_slice() {
        // No project in the title (none, or Accessibility off): the editor app itself — an
        // app row of kind code, never a project (§11 decision #2, §23.2b).
        [] => return app_context(app_name, None, Category::Code),
        [project] => (project.to_string(), None),
        [file, project, ..] => (project.to_string(), Some(file.to_string())),
    };
    Context {
        key: format!("code:{label}"),
        label,
        detail,
        category: Category::Code,
    }
}

/// `"● file — project — Visual Studio Code"` → `["file", "project"]`.
fn editor_title_tokens(title: &str) -> Vec<&str> {
    let title = strip_dirty_markers(title);
    let sep = if title.contains(" — ") {
        " — "
    } else {
        " - "
    };
    let mut tokens: Vec<&str> = title
        .split(sep)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    loop {
        let n = tokens.len();
        if n >= 1 && EDITOR_APP_NAMES.contains(&tokens[n - 1]) {
            tokens.pop();
        } else if n >= 2 && is_split_app_name(tokens[n - 2], tokens[n - 1], sep) {
            // "Visual Studio Code - Insiders" itself contains " - ", so a hyphen-separated
            // title splits it into two tokens.
            tokens.truncate(n - 2);
        } else {
            return tokens;
        }
    }
}

fn is_split_app_name(a: &str, b: &str, sep: &str) -> bool {
    let joined = format!("{a}{sep}{b}");
    EDITOR_APP_NAMES.contains(&joined.as_str())
}

/// Editors prefix unsaved files with `● ` (VS Code), `• ` or `* `.
fn strip_dirty_markers(mut title: &str) -> &str {
    while let Some(rest) = ["● ", "• ", "* "]
        .iter()
        .find_map(|marker| title.strip_prefix(marker))
    {
        title = rest.trim_start();
    }
    title
}

/// Lowercased host without leading `www.`, only for real web pages (http/https with a host).
/// Browser-internal pages (`chrome://`, `about:`, `file://`) fall back to the app context.
/// `https://www.YouTube.com/x` → `youtube.com`: lowercase, without `www.`; http(s) only.
pub fn web_host(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_lowercase();
    let host = host.strip_prefix("www.").map(String::from).unwrap_or(host);
    Some(host)
}

fn browser_context(host: String, title: Option<&str>, config: &Config) -> Context {
    // Explicit labels win over the docs heuristics (e.g. docs.google.com is an editor).
    if let Some(rule) = config.labels.get(&host) {
        return Context {
            key: format!("web:{host}"),
            label: rule.label.clone(),
            detail: title.map(String::from),
            category: rule.category,
        };
    }
    if is_docs_host(&host, config) {
        return Context {
            key: "web:docs".into(),
            label: "docs".into(),
            detail: Some(host),
            category: Category::Docs,
        };
    }
    Context {
        key: format!("web:{host}"),
        label: host,
        detail: title.map(String::from),
        category: Category::Web,
    }
}

fn is_docs_host(host: &str, config: &Config) -> bool {
    config.docs_domains.iter().any(|d| d == host)
        || host.starts_with("docs.")
        || host.ends_with(".readthedocs.io")
}

fn app_context(app_name: &str, title: Option<&str>, category: Category) -> Context {
    Context {
        key: format!("app:{app_name}"),
        label: app_name.to_string(),
        detail: title.map(String::from),
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::Idle;

    const IDLE: Idle = Idle {
        keyboard_s: 0.0,
        mouse_s: 0.0,
        click_s: 0.0,
        scroll_s: 0.0,
    };

    fn sample(app: &str, bundle: &str, title: Option<&str>, url: Option<&str>) -> Sample {
        Sample {
            ts_ms: 0,
            app_name: app.into(),
            bundle_id: bundle.into(),
            window_title: title.map(String::from),
            url: url.map(String::from),
            idle: IDLE,
            locked: false,
            media_active: false,
            audio: None,
        }
    }

    fn vscode(title: Option<&str>) -> Context {
        derive_context(
            &sample("Code", "com.microsoft.VSCode", title, None),
            &Config::default(),
        )
    }

    fn chrome(url: Option<&str>, title: Option<&str>) -> Context {
        derive_context(
            &sample("Google Chrome", "com.google.Chrome", title, url),
            &Config::default(),
        )
    }

    fn ctx(key: &str, label: &str, detail: Option<&str>, category: Category) -> Context {
        Context {
            key: key.into(),
            label: label.into(),
            detail: detail.map(String::from),
            category,
        }
    }

    // --- code editors ---

    #[test]
    fn apps_take_their_category_from_app_categories() {
        let c = Config::default();
        let ctx = derive_context(
            &sample("iTerm2", "com.googlecode.iterm2", Some("~/src — zsh"), None),
            &c,
        );
        assert_eq!(
            (ctx.key.as_str(), ctx.category),
            ("app:iTerm2", Category::Code)
        );
        let slack = derive_context(
            &sample("Slack", "com.tinyspeck.slackmacgap", None, None),
            &c,
        );
        assert_eq!(slack.category, Category::Comms);
        let finder = derive_context(&sample("Finder", "com.apple.finder", None, None), &c);
        assert_eq!(finder.category, Category::App);
        // Editors stay code and browsers stay label-driven whatever the map says.
        let mut odd = Config::default();
        odd.app_categories
            .insert("com.microsoft.VSCode".into(), Category::Notes);
        odd.app_categories
            .insert("com.google.Chrome".into(), Category::Notes);
        let code = derive_context(
            &sample("Code", "com.microsoft.VSCode", Some("a.rs — p"), None),
            &odd,
        );
        assert_eq!(code.category, Category::Code);
        let yt = derive_context(
            &sample(
                "Google Chrome",
                "com.google.Chrome",
                Some("v"),
                Some("https://youtube.com/"),
            ),
            &odd,
        );
        assert_eq!(yt.category, Category::Media);
    }

    #[test]
    fn vscode_file_dash_project_gives_project_label_and_file_detail() {
        assert_eq!(
            vscode(Some("risk_engine.py — bank-agent-lab")),
            ctx(
                "code:bank-agent-lab",
                "bank-agent-lab",
                Some("risk_engine.py"),
                Category::Code
            )
        );
    }

    #[test]
    fn vscode_dirty_marker_is_stripped_from_file() {
        assert_eq!(
            vscode(Some("● risk_engine.py — bank-agent-lab")).detail,
            Some("risk_engine.py".into())
        );
        assert_eq!(
            vscode(Some("• main.rs — timewent")).detail,
            Some("main.rs".into())
        );
        assert_eq!(
            vscode(Some("* main.rs — timewent")).detail,
            Some("main.rs".into())
        );
    }

    #[test]
    fn vscode_trailing_app_name_token_is_dropped() {
        assert_eq!(
            vscode(Some("main.rs — timewent — Visual Studio Code")),
            ctx("code:timewent", "timewent", Some("main.rs"), Category::Code)
        );
    }

    #[test]
    fn vscode_insiders_app_name_is_dropped_even_with_hyphen_separator() {
        assert_eq!(
            vscode(Some("main.rs - timewent - Visual Studio Code - Insiders")),
            ctx("code:timewent", "timewent", Some("main.rs"), Category::Code)
        );
    }

    #[test]
    fn editor_title_falls_back_to_hyphen_separator() {
        assert_eq!(
            vscode(Some("lib.rs - timewent")),
            ctx("code:timewent", "timewent", Some("lib.rs"), Category::Code)
        );
    }

    #[test]
    fn vscode_single_token_title_becomes_label_without_detail() {
        assert_eq!(
            vscode(Some("timewent — Visual Studio Code")),
            ctx("code:timewent", "timewent", None, Category::Code)
        );
    }

    #[test]
    fn vscode_without_a_project_is_an_app_not_a_project() {
        // §11 decision #2 / §23.2b: no parsed project → the editor app itself (kind code).
        assert_eq!(vscode(None), ctx("app:Code", "Code", None, Category::Code));
        assert_eq!(
            vscode(Some("Visual Studio Code")),
            ctx("app:Code", "Code", None, Category::Code)
        );
        assert_eq!(
            vscode(Some("   ")),
            ctx("app:Code", "Code", None, Category::Code)
        );
    }

    #[test]
    fn cursor_is_recognised_as_code_editor_by_bundle_id() {
        let s = sample(
            "Cursor",
            "com.todesktop.230313mzl4w4u92",
            Some("app.ts — web — Cursor"),
            None,
        );
        assert_eq!(
            derive_context(&s, &Config::default()),
            ctx("code:web", "web", Some("app.ts"), Category::Code)
        );
    }

    #[test]
    fn windsurf_and_insiders_bundles_are_code_editors() {
        for bundle in ["com.exafunction.windsurf", "com.microsoft.VSCodeInsiders"] {
            let s = sample("X", bundle, Some("a.rs — proj"), None);
            assert_eq!(derive_context(&s, &Config::default()).key, "code:proj");
        }
    }

    // --- browsers ---

    #[test]
    fn browser_url_keys_by_host_without_www_and_title_as_detail() {
        assert_eq!(
            chrome(
                Some("https://www.Example.com/a/b?q=1"),
                Some("Example Domain")
            ),
            ctx(
                "web:example.com",
                "example.com",
                Some("Example Domain"),
                Category::Web
            )
        );
    }

    #[test]
    fn labelled_host_uses_label_and_category_from_map() {
        assert_eq!(
            chrome(Some("https://chatgpt.com/c/123"), Some("Rust lifetimes")),
            ctx(
                "web:chatgpt.com",
                "ChatGPT",
                Some("Rust lifetimes"),
                Category::Ai
            )
        );
        assert_eq!(
            chrome(Some("https://www.youtube.com/watch?v=x"), Some("cat video")).label,
            "YouTube"
        );
        assert_eq!(
            chrome(Some("http://localhost:5173/"), None),
            ctx("web:localhost", "localhost", None, Category::Code)
        );
    }

    #[test]
    fn docs_domain_exact_match_groups_under_docs_with_host_detail() {
        assert_eq!(
            chrome(
                Some("https://docs.rs/serde/latest/serde/"),
                Some("serde - Rust")
            ),
            ctx("web:docs", "docs", Some("docs.rs"), Category::Docs)
        );
        assert_eq!(
            chrome(Some("https://stackoverflow.com/questions/1"), Some("q")),
            ctx(
                "web:docs",
                "docs",
                Some("stackoverflow.com"),
                Category::Docs
            )
        );
    }

    #[test]
    fn any_docs_subdomain_is_docs() {
        assert_eq!(
            chrome(Some("https://docs.github.com/en"), Some("GitHub Docs")),
            ctx("web:docs", "docs", Some("docs.github.com"), Category::Docs)
        );
    }

    #[test]
    fn explicit_label_wins_over_docs_rules() {
        assert_eq!(
            chrome(
                Some("https://docs.google.com/document/d/1"),
                Some("Q4 plan")
            ),
            ctx(
                "web:docs.google.com",
                "Google Docs",
                Some("Q4 plan"),
                Category::Web
            )
        );
        assert_eq!(
            chrome(Some("https://docs.rs/serde"), Some("serde")).key,
            "web:docs"
        );
    }

    #[test]
    fn user_label_for_a_docs_domain_overrides_docs_grouping() {
        let mut config = Config::default();
        config.labels.insert(
            "stackoverflow.com".into(),
            crate::config::LabelRule {
                label: "SO".into(),
                category: Category::Web,
            },
        );
        let s = sample(
            "Google Chrome",
            "com.google.Chrome",
            None,
            Some("https://stackoverflow.com/q/1"),
        );
        assert_eq!(derive_context(&s, &config).label, "SO");
    }

    #[test]
    fn readthedocs_suffix_is_docs() {
        assert_eq!(
            chrome(Some("https://pytest.readthedocs.io/en/latest/"), None),
            ctx(
                "web:docs",
                "docs",
                Some("pytest.readthedocs.io"),
                Category::Docs
            )
        );
    }

    #[test]
    fn docs_match_is_exact_not_substring() {
        assert_eq!(
            chrome(Some("https://notdocs.rs/"), None).key,
            "web:notdocs.rs"
        );
        assert_eq!(
            chrome(Some("https://ru.stackoverflow.com/"), None).key,
            "web:ru.stackoverflow.com"
        );
    }

    #[test]
    fn browser_without_url_is_keyed_by_app() {
        assert_eq!(
            chrome(None, Some("New Tab")),
            ctx(
                "app:Google Chrome",
                "Google Chrome",
                Some("New Tab"),
                Category::Web
            )
        );
    }

    #[test]
    fn browser_internal_or_hostless_url_is_treated_as_no_url() {
        for url in [
            "chrome://newtab/",
            "about:blank",
            "file:///Users/me/a.html",
            "not a url",
        ] {
            assert_eq!(
                chrome(Some(url), Some("t")).key,
                "app:Google Chrome",
                "{url}"
            );
        }
    }

    #[test]
    fn all_listed_browsers_are_recognised() {
        for bundle in [
            "com.google.Chrome",
            "company.thebrowser.Browser",
            "com.brave.Browser",
            "com.apple.Safari",
            "com.microsoft.edgemac",
            "org.mozilla.firefox",
        ] {
            let s = sample("B", bundle, None, Some("https://github.com/x"));
            assert_eq!(derive_context(&s, &Config::default()).label, "GitHub");
        }
    }

    // --- everything else ---

    #[test]
    fn unknown_app_is_keyed_by_app_name_with_title_detail() {
        let s = sample("Finder", "com.apple.finder", Some("Downloads"), None);
        assert_eq!(
            derive_context(&s, &Config::default()),
            ctx("app:Finder", "Finder", Some("Downloads"), Category::App)
        );
    }

    #[test]
    fn unknown_app_ignores_url_field() {
        let s = sample(
            "Preview",
            "com.apple.Preview",
            None,
            Some("https://github.com"),
        );
        assert_eq!(
            derive_context(&s, &Config::default()),
            ctx("app:Preview", "Preview", None, Category::App)
        );
    }
}
