//! Every threshold the derivation uses. Changing config re-derives all views from raw samples.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::Category;

/// Thresholds and mappings for `f(samples, config)`.
///
/// `#[serde(default)]`: a partial config (e.g. from an older settings file) fills in defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Probe period; also the nominal duration one sample represents.
    pub poll_interval_ms: u32,
    /// Idle ≥ this → passive (reading/thinking).
    pub passive_after_s: u32,
    /// Idle ≥ this → away.
    pub away_after_s: u32,
    /// Timestamp jump between consecutive samples > this → gap (sleep/crash).
    pub gap_after_s: u32,
    /// Run shorter than this → transient (absorbed, never its own segment).
    pub transient_max_s: u32,
    /// `transient_max_s ≤ run < glance_max_s` → glance segment.
    pub glance_max_s: u32,
    /// Host (lowercase, no `www.`) → display label and category.
    pub labels: BTreeMap<String, LabelRule>,
    /// Hosts grouped under the `docs` label (in addition to `docs.*` and `*.readthedocs.io`).
    pub docs_domains: Vec<String>,
    /// Apps you pass through on the way to work — launchers, system UI. Their runs are never
    /// a context: always absorbed into a neighbour (DESIGN §5.5).
    pub passthrough_bundle_ids: Vec<String>,
    /// Count time spent looking at timewent itself as an ordinary app context. Off: timewent
    /// is passthrough, whatever `passthrough_bundle_ids` says (DESIGN §5.5, self-visibility).
    pub count_self: bool,
    /// Roll AI / docs / repo / matching-title time up into the project it served (DESIGN §7.1).
    /// Off: every context is its own row, as before.
    pub attribute_projects: bool,
    /// Max time between two blocks of one project for what lies between to count as support.
    pub support_window_s: u32,
    /// Names you give to sets of apps / sites (DESIGN §7.3). First match in list order wins.
    pub activities: Vec<Activity>,
    /// What kind of time an app is (DESIGN §5.4), by bundle id. Editors are always code and
    /// browsers follow `labels`; any other app not listed is `app`.
    pub app_categories: BTreeMap<String, Category>,
}

/// A user-defined activity: `coding = { apps: [VS Code, iTerm2] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub name: String,
    /// Bundle ids, matched exactly.
    #[serde(default)]
    pub apps: Vec<String>,
    /// Hosts; a page matches if its host is one of these or a subdomain of one.
    #[serde(default)]
    pub domains: Vec<String>,
}

/// timewent's own bundle id; governed by `Config::count_self`.
pub const SELF_BUNDLE_ID: &str = "dev.timewent.app";

/// How a known host is displayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelRule {
    pub label: String,
    pub category: Category,
}

impl Default for Config {
    fn default() -> Self {
        let labels = [
            ("chatgpt.com", "ChatGPT", Category::Ai),
            ("chat.openai.com", "ChatGPT", Category::Ai),
            ("claude.ai", "Claude", Category::Ai),
            ("gemini.google.com", "Gemini", Category::Ai),
            ("docs.google.com", "Google Docs", Category::Web),
            ("github.com", "GitHub", Category::Web),
            ("google.com", "Google", Category::Web),
            ("localhost", "localhost", Category::Code),
            // DESIGN §5.4: what kind of time a site is.
            ("youtube.com", "YouTube", Category::Media),
            ("music.youtube.com", "YouTube Music", Category::Media),
            ("netflix.com", "Netflix", Category::Media),
            ("twitch.tv", "Twitch", Category::Media),
            ("open.spotify.com", "Spotify", Category::Media),
            ("soundcloud.com", "SoundCloud", Category::Media),
            ("x.com", "X", Category::Social),
            ("twitter.com", "X", Category::Social),
            ("instagram.com", "Instagram", Category::Social),
            ("reddit.com", "Reddit", Category::Social),
            ("facebook.com", "Facebook", Category::Social),
            ("linkedin.com", "LinkedIn", Category::Social),
            ("mail.google.com", "Gmail", Category::Comms),
            ("outlook.live.com", "Outlook", Category::Comms),
            ("app.slack.com", "Slack", Category::Comms),
            ("web.whatsapp.com", "WhatsApp", Category::Comms),
            ("discord.com", "Discord", Category::Comms),
            ("meet.google.com", "Google Meet", Category::Comms),
            ("zoom.us", "Zoom", Category::Comms),
            ("figma.com", "Figma", Category::Design),
            ("notion.so", "Notion", Category::Notes),
        ]
        .into_iter()
        .map(|(host, label, category)| {
            (
                host.to_string(),
                LabelRule {
                    label: label.to_string(),
                    category,
                },
            )
        })
        .collect();

        let docs_domains = [
            "developer.mozilla.org",
            "docs.rs",
            "doc.rust-lang.org",
            "stackoverflow.com",
            "developer.apple.com",
            "v2.tauri.app",
            "docs.python.org",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        Config {
            poll_interval_ms: 1000,
            passive_after_s: 45,
            away_after_s: 180,
            gap_after_s: 5,
            transient_max_s: 3,
            glance_max_s: 10,
            labels,
            docs_domains,
            passthrough_bundle_ids: DEFAULT_PASSTHROUGH.iter().map(|s| s.to_string()).collect(),
            count_self: false,
            attribute_projects: true,
            support_window_s: 600,
            activities: Vec::new(),
            app_categories: default_app_categories(),
        }
    }
}

/// DESIGN §5.4 defaults: bundle id → category for well-known apps.
const DEFAULT_APP_CATEGORIES: &[(&str, Category)] = &[
    ("com.apple.Terminal", Category::Code),
    ("com.googlecode.iterm2", Category::Code),
    ("com.mitchellh.ghostty", Category::Code),
    ("dev.warp.Warp-Stable", Category::Code),
    ("com.apple.dt.Xcode", Category::Code),
    ("com.jetbrains.intellij", Category::Code),
    ("com.jetbrains.intellij.ce", Category::Code),
    ("com.jetbrains.pycharm", Category::Code),
    ("com.jetbrains.pycharm.ce", Category::Code),
    ("com.jetbrains.WebStorm", Category::Code),
    ("com.jetbrains.goland", Category::Code),
    ("com.jetbrains.rustrover", Category::Code),
    ("com.jetbrains.CLion", Category::Code),
    ("com.jetbrains.rider", Category::Code),
    ("com.jetbrains.datagrip", Category::Code),
    ("com.jetbrains.PhpStorm", Category::Code),
    ("com.jetbrains.rubymine", Category::Code),
    ("com.google.android.studio", Category::Code),
    ("com.postmanlabs.mac", Category::Code),
    ("com.tinyapp.TablePlus", Category::Code),
    ("com.tinyspeck.slackmacgap", Category::Comms),
    ("com.apple.mail", Category::Comms),
    ("com.apple.MobileSMS", Category::Comms),
    ("net.whatsapp.WhatsApp", Category::Comms),
    ("desktop.WhatsApp", Category::Comms),
    ("com.hnc.Discord", Category::Comms),
    ("us.zoom.xos", Category::Comms),
    ("com.microsoft.teams2", Category::Comms),
    ("com.microsoft.teams", Category::Comms),
    ("com.apple.FaceTime", Category::Comms),
    ("com.spotify.client", Category::Media),
    ("com.apple.Music", Category::Media),
    ("com.apple.podcasts", Category::Media),
    ("com.figma.Desktop", Category::Design),
    ("notion.id", Category::Notes),
    ("md.obsidian", Category::Notes),
    ("com.apple.Notes", Category::Notes),
];

fn default_app_categories() -> BTreeMap<String, Category> {
    DEFAULT_APP_CATEGORIES
        .iter()
        .map(|(b, c)| (b.to_string(), *c))
        .collect()
}

/// DESIGN §5.5 defaults: launchers and system UI that briefly take focus. (timewent itself is
/// governed by `count_self`.)
const DEFAULT_PASSTHROUGH: &[&str] = &[
    "com.apple.Spotlight",
    "com.raycast.macos",
    "com.runningwithcrayons.Alfred",
    "com.apple.dock",
    "com.apple.controlcenter",
    "com.apple.notificationcenterui",
    "com.apple.screencaptureui",
    "com.apple.SecurityAgent",
    "com.apple.UserNotificationCenter",
    "com.apple.systemuiserver",
    "com.apple.WindowManager",
    // The Wi-Fi login sheet: a moment on the way online, never "time spent" (DESIGN §5.5).
    "com.apple.CaptiveNetworkAssistant",
];

/// Below this the probe (and osascript url lookups) would dominate the machine (DESIGN §1).
const MIN_POLL_INTERVAL_MS: u32 = 250;

impl Config {
    /// Checks the thresholds are mutually consistent. Every violation is reported, one
    /// human-readable line each, so a settings form can show them all at once.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.passive_after_s >= self.away_after_s {
            errors.push(format!(
                "passive_after_s ({}) must be < away_after_s ({})",
                self.passive_after_s, self.away_after_s
            ));
        }
        if self.transient_max_s >= self.glance_max_s {
            errors.push(format!(
                "transient_max_s ({}) must be < glance_max_s ({})",
                self.transient_max_s, self.glance_max_s
            ));
        }
        // Otherwise every ordinary step between two samples would count as a gap.
        if u64::from(self.gap_after_s) * 1000 <= u64::from(self.poll_interval_ms) {
            errors.push(format!(
                "gap_after_s ({}s) must exceed poll_interval_ms ({}ms)",
                self.gap_after_s, self.poll_interval_ms
            ));
        }
        if self.poll_interval_ms < MIN_POLL_INTERVAL_MS {
            errors.push(format!(
                "poll_interval_ms ({}) must be >= {MIN_POLL_INTERVAL_MS}",
                self.poll_interval_ms
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (i, a) in self.activities.iter().enumerate() {
            let name = a.name.trim();
            if name.is_empty() {
                errors.push(format!("activity {} needs a name", i + 1));
            } else if !seen.insert(name.to_lowercase()) {
                errors.push(format!("activity name \"{name}\" is used twice"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Adds built-in labels and app categories this config does not have yet (a config saved
    /// by an older version). Entries it has — including the user's edits — are kept.
    pub fn merge_new_defaults(&mut self) {
        let d = Config::default();
        for (host, rule) in d.labels {
            self.labels.entry(host).or_insert(rule);
        }
        for (bundle, cat) in d.app_categories {
            self.app_categories.entry(bundle).or_insert(cat);
        }
        for bundle in d.passthrough_bundle_ids {
            if !self.passthrough_bundle_ids.contains(&bundle) {
                self.passthrough_bundle_ids.push(bundle);
            }
        }
    }

    /// Whether runs of this app are passthrough (never a context of their own).
    pub fn is_passthrough(&self, bundle_id: &str) -> bool {
        if bundle_id == SELF_BUNDLE_ID {
            return !self.count_self;
        }
        self.passthrough_bundle_ids.iter().any(|b| b == bundle_id)
    }

    pub(crate) fn poll_ms(&self) -> i64 {
        i64::from(self.poll_interval_ms)
    }

    pub(crate) fn transient_ms(&self) -> i64 {
        i64::from(self.transient_max_s) * 1000
    }

    pub(crate) fn glance_ms(&self) -> i64 {
        i64::from(self.glance_max_s) * 1000
    }

    /// A jump strictly greater than `gap_after_s` between consecutive samples is a gap.
    pub(crate) fn is_gap(&self, prev_ts_ms: i64, next_ts_ms: i64) -> bool {
        next_ts_ms - prev_ts_ms > i64::from(self.gap_after_s) * 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(label: &str, category: Category) -> LabelRule {
        LabelRule {
            label: label.into(),
            category,
        }
    }

    #[test]
    fn default_thresholds_match_plan() {
        let c = Config::default();
        assert_eq!(c.poll_interval_ms, 1000);
        assert_eq!(c.passive_after_s, 45);
        assert_eq!(c.away_after_s, 180);
        assert_eq!(c.gap_after_s, 5);
        assert_eq!(c.transient_max_s, 3);
        assert_eq!(c.glance_max_s, 10);
    }

    #[test]
    fn default_labels_match_plan() {
        let c = Config::default();
        let expected: BTreeMap<String, LabelRule> = [
            ("chatgpt.com", rule("ChatGPT", Category::Ai)),
            ("chat.openai.com", rule("ChatGPT", Category::Ai)),
            ("claude.ai", rule("Claude", Category::Ai)),
            ("gemini.google.com", rule("Gemini", Category::Ai)),
            ("docs.google.com", rule("Google Docs", Category::Web)),
            ("github.com", rule("GitHub", Category::Web)),
            ("google.com", rule("Google", Category::Web)),
            ("localhost", rule("localhost", Category::Code)),
            // DESIGN §5.4
            ("youtube.com", rule("YouTube", Category::Media)),
            ("music.youtube.com", rule("YouTube Music", Category::Media)),
            ("netflix.com", rule("Netflix", Category::Media)),
            ("twitch.tv", rule("Twitch", Category::Media)),
            ("open.spotify.com", rule("Spotify", Category::Media)),
            ("soundcloud.com", rule("SoundCloud", Category::Media)),
            ("x.com", rule("X", Category::Social)),
            ("twitter.com", rule("X", Category::Social)),
            ("instagram.com", rule("Instagram", Category::Social)),
            ("reddit.com", rule("Reddit", Category::Social)),
            ("facebook.com", rule("Facebook", Category::Social)),
            ("linkedin.com", rule("LinkedIn", Category::Social)),
            ("mail.google.com", rule("Gmail", Category::Comms)),
            ("outlook.live.com", rule("Outlook", Category::Comms)),
            ("app.slack.com", rule("Slack", Category::Comms)),
            ("web.whatsapp.com", rule("WhatsApp", Category::Comms)),
            ("discord.com", rule("Discord", Category::Comms)),
            ("meet.google.com", rule("Google Meet", Category::Comms)),
            ("zoom.us", rule("Zoom", Category::Comms)),
            ("figma.com", rule("Figma", Category::Design)),
            ("notion.so", rule("Notion", Category::Notes)),
        ]
        .into_iter()
        .map(|(host, r)| (host.to_string(), r))
        .collect();
        assert_eq!(c.labels, expected);
    }

    #[test]
    fn default_app_categories_match_plan() {
        let c = Config::default();
        let cat = |b: &str| c.app_categories.get(b).copied();
        assert_eq!(cat("com.googlecode.iterm2"), Some(Category::Code));
        assert_eq!(cat("com.apple.Terminal"), Some(Category::Code));
        assert_eq!(cat("com.jetbrains.pycharm"), Some(Category::Code));
        assert_eq!(cat("com.tinyspeck.slackmacgap"), Some(Category::Comms));
        assert_eq!(cat("us.zoom.xos"), Some(Category::Comms));
        assert_eq!(cat("com.spotify.client"), Some(Category::Media));
        assert_eq!(cat("com.apple.Music"), Some(Category::Media));
        assert_eq!(cat("com.figma.Desktop"), Some(Category::Design));
        assert_eq!(cat("md.obsidian"), Some(Category::Notes));
        assert_eq!(cat("com.apple.finder"), None);
    }

    #[test]
    fn new_defaults_merge_into_a_saved_config_without_overwriting_it() {
        let mut saved = Config::default();
        saved.labels.clear();
        saved.labels.insert(
            "youtube.com".into(),
            rule("Tube", Category::Web), // the user's own edit
        );
        saved.app_categories.clear();
        saved
            .app_categories
            .insert("com.spotify.client".into(), Category::App);
        saved.merge_new_defaults();
        assert_eq!(saved.labels["youtube.com"], rule("Tube", Category::Web));
        assert_eq!(
            saved.labels["netflix.com"],
            rule("Netflix", Category::Media)
        );
        assert_eq!(saved.app_categories["com.spotify.client"], Category::App);
        assert_eq!(saved.app_categories["com.apple.Music"], Category::Media);
    }

    #[test]
    fn default_docs_domains_match_plan() {
        let c = Config::default();
        assert_eq!(
            c.docs_domains,
            vec![
                "developer.mozilla.org",
                "docs.rs",
                "doc.rust-lang.org",
                "stackoverflow.com",
                "developer.apple.com",
                "v2.tauri.app",
                "docs.python.org",
            ]
        );
    }

    #[test]
    fn default_passthrough_matches_plan() {
        let c = Config::default();
        assert_eq!(c.passthrough_bundle_ids.len(), 12);
        assert_eq!(c.passthrough_bundle_ids[0], "com.apple.Spotlight");
        assert!(!c.count_self);
        assert!(c.is_passthrough("com.apple.Spotlight"));
        assert!(c.is_passthrough("com.apple.WindowManager"));
        assert!(
            c.is_passthrough("dev.timewent.app"),
            "self is passthrough unless counted"
        );
        assert!(!Config::default().is_passthrough("com.microsoft.VSCode"));
    }

    #[test]
    fn attribution_defaults_match_plan() {
        let c = Config::default();
        assert!(c.attribute_projects);
        assert_eq!(c.support_window_s, 600);
        let old: Config = serde_json::from_str(r#"{"glance_max_s": 10}"#).expect("parse");
        assert!(old.attribute_projects);
        assert_eq!(old.support_window_s, 600);
    }

    #[test]
    fn captive_network_assistant_is_passthrough_and_merges_into_saved_configs() {
        assert!(Config::default().is_passthrough("com.apple.CaptiveNetworkAssistant"));
        let mut saved = Config {
            passthrough_bundle_ids: vec!["com.apple.Spotlight".into(), "my.own.launcher".into()],
            ..Config::default()
        };
        saved.merge_new_defaults();
        assert!(saved.is_passthrough("com.apple.CaptiveNetworkAssistant"));
        assert!(
            saved.is_passthrough("my.own.launcher"),
            "the user's entries stay"
        );
        assert_eq!(
            saved.passthrough_bundle_ids[..2],
            ["com.apple.Spotlight", "my.own.launcher"]
        );
        let n = saved.passthrough_bundle_ids.len();
        saved.merge_new_defaults();
        assert_eq!(saved.passthrough_bundle_ids.len(), n, "idempotent");
    }

    #[test]
    fn count_self_decides_for_timewent_alone() {
        let c = Config {
            count_self: true,
            ..Config::default()
        };
        assert!(!c.is_passthrough("dev.timewent.app"));
        assert!(c.is_passthrough("com.raycast.macos"));
        // Listing self explicitly does not override the toggle.
        let c = Config {
            count_self: true,
            passthrough_bundle_ids: vec!["dev.timewent.app".into()],
            ..Config::default()
        };
        assert!(!c.is_passthrough("dev.timewent.app"));
    }

    #[test]
    fn config_saved_before_passthrough_existed_still_loads() {
        let old = r#"{"poll_interval_ms":1000,"passive_after_s":45,"away_after_s":180,
            "gap_after_s":5,"transient_max_s":3,"glance_max_s":10,"labels":{},"docs_domains":[]}"#;
        let c: Config = serde_json::from_str(old).expect("parse");
        assert_eq!(
            c.passthrough_bundle_ids,
            Config::default().passthrough_bundle_ids
        );
        assert!(!c.count_self);
    }

    #[test]
    fn partial_config_json_fills_missing_fields_with_defaults() {
        let c: Config = serde_json::from_str(r#"{"away_after_s": 300}"#).expect("parse");
        assert_eq!(c.away_after_s, 300);
        assert_eq!(c.passive_after_s, 45);
        assert_eq!(c.labels, Config::default().labels);
    }

    #[test]
    fn config_round_trips_through_json() {
        let c = Config::default();
        let json = serde_json::to_string(&c).expect("serialize");
        assert!(json.contains(r#""chatgpt.com":{"label":"ChatGPT","category":"ai"}"#));
        let back: Config = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, c);
    }

    fn messages(c: &Config) -> Vec<String> {
        c.validate().expect_err("should be invalid")
    }

    #[test]
    fn default_config_is_valid() {
        assert_eq!(Config::default().validate(), Ok(()));
    }

    #[test]
    fn passive_must_come_before_away() {
        let c = Config {
            passive_after_s: 180,
            away_after_s: 180,
            ..Config::default()
        };
        assert_eq!(
            messages(&c),
            ["passive_after_s (180) must be < away_after_s (180)"]
        );
    }

    #[test]
    fn transient_must_be_shorter_than_glance() {
        let c = Config {
            transient_max_s: 10,
            glance_max_s: 10,
            ..Config::default()
        };
        assert_eq!(
            messages(&c),
            ["transient_max_s (10) must be < glance_max_s (10)"]
        );
    }

    #[test]
    fn gap_must_exceed_one_poll_interval() {
        let c = Config {
            gap_after_s: 2,
            poll_interval_ms: 2_000,
            ..Config::default()
        };
        assert_eq!(
            messages(&c),
            ["gap_after_s (2s) must exceed poll_interval_ms (2000ms)"]
        );
        let ok = Config {
            gap_after_s: 2,
            poll_interval_ms: 1_999,
            ..Config::default()
        };
        assert_eq!(ok.validate(), Ok(()));
    }

    #[test]
    fn poll_interval_has_a_floor_of_250ms() {
        let c = Config {
            poll_interval_ms: 249,
            ..Config::default()
        };
        assert_eq!(messages(&c), ["poll_interval_ms (249) must be >= 250"]);
        let ok = Config {
            poll_interval_ms: 250,
            ..Config::default()
        };
        assert_eq!(ok.validate(), Ok(()));
    }

    #[test]
    fn every_violation_is_reported() {
        let c = Config {
            poll_interval_ms: 100,
            passive_after_s: 300,
            away_after_s: 200,
            gap_after_s: 0,
            transient_max_s: 20,
            glance_max_s: 10,
            ..Config::default()
        };
        assert_eq!(messages(&c).len(), 4);
    }

    #[test]
    fn activity_names_must_be_present_and_unique() {
        let act = |name: &str| Activity {
            name: name.into(),
            apps: vec!["a.b".into()],
            domains: vec![],
        };
        let c = Config {
            activities: vec![act("coding"), act(" "), act("Coding")],
            ..Config::default()
        };
        assert_eq!(
            messages(&c),
            [
                "activity 2 needs a name",
                "activity name \"Coding\" is used twice"
            ]
        );
        let ok = Config {
            activities: vec![act("coding"), act("writing")],
            ..Config::default()
        };
        assert_eq!(ok.validate(), Ok(()));
    }

    #[test]
    fn huge_gap_does_not_overflow() {
        let c = Config {
            gap_after_s: u32::MAX,
            ..Config::default()
        };
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn jump_of_exactly_gap_after_is_not_a_gap() {
        let c = Config::default();
        assert!(!c.is_gap(0, 5_000));
        assert!(c.is_gap(0, 5_001));
    }
}
