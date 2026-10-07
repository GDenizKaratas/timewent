//! "Apps and sites you used" (DESIGN §7.3, decision A6): what the activity editor offers as
//! members, so nobody types bundle ids. Pure: per-context sample counts in, sorted lists out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::context::{is_browser, web_host};

/// How many samples one context got (the store aggregates these from raw samples).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCount {
    pub bundle_id: String,
    pub app_name: String,
    pub url: Option<String>,
    pub samples: i64,
    pub last_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SeenSources {
    pub apps: Vec<SeenApp>,
    pub domains: Vec<SeenDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenApp {
    pub bundle_id: String,
    /// The app's most recent name.
    pub name: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenDomain {
    /// Host, lowercase, without `www.`.
    pub domain: String,
    /// The configured label (e.g. `GitHub`), else the domain.
    pub label: String,
    pub ms: i64,
}

/// At most this many apps and this many domains.
pub const MAX_SOURCES: usize = 50;

/// Time = samples × poll interval. Pass-through apps (timewent itself unless `count_self`)
/// are left out; domains come from browser samples with a URL. Sorted by time, then name.
pub fn seen_sources(counts: &[SourceCount], config: &Config) -> SeenSources {
    let poll = i64::from(config.poll_interval_ms);
    // bundle → (name, last_ts, ms)
    let mut apps: BTreeMap<&str, (&str, i64, i64)> = BTreeMap::new();
    let mut domains: BTreeMap<String, i64> = BTreeMap::new();
    for c in counts {
        if config.is_passthrough(&c.bundle_id) {
            continue;
        }
        let ms = c.samples * poll;
        let e = apps
            .entry(c.bundle_id.as_str())
            .or_insert((c.app_name.as_str(), c.last_ts_ms, 0));
        if c.last_ts_ms > e.1 {
            (e.0, e.1) = (c.app_name.as_str(), c.last_ts_ms);
        }
        e.2 += ms;
        if is_browser(&c.bundle_id) {
            if let Some(host) = c.url.as_deref().and_then(web_host) {
                *domains.entry(host).or_insert(0) += ms;
            }
        }
    }
    let mut apps: Vec<SeenApp> = apps
        .into_iter()
        .map(|(bundle_id, (name, _, ms))| SeenApp {
            bundle_id: bundle_id.into(),
            name: name.into(),
            ms,
        })
        .collect();
    apps.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.name.cmp(&b.name)));
    apps.truncate(MAX_SOURCES);
    let mut domains: Vec<SeenDomain> = domains
        .into_iter()
        .map(|(domain, ms)| SeenDomain {
            label: config
                .labels
                .get(&domain)
                .map_or_else(|| domain.clone(), |r| r.label.clone()),
            domain,
            ms,
        })
        .collect();
    domains.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.domain.cmp(&b.domain)));
    domains.truncate(MAX_SOURCES);
    SeenSources { apps, domains }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(bundle: &str, app: &str, url: Option<&str>, samples: i64, last: i64) -> SourceCount {
        SourceCount {
            bundle_id: bundle.into(),
            app_name: app.into(),
            url: url.map(String::from),
            samples,
            last_ts_ms: last,
        }
    }

    const CHROME: &str = "com.google.Chrome";

    #[test]
    fn apps_and_sites_sorted_by_time_with_labels() {
        let counts = [
            count("com.microsoft.VSCode", "Code", None, 600, 10),
            count(
                CHROME,
                "Google Chrome",
                Some("https://www.github.com/a"),
                100,
                5,
            ),
            count(CHROME, "Google Chrome", Some("https://github.com/b"), 50, 6),
            count(
                CHROME,
                "Google Chrome",
                Some("https://example.org/"),
                300,
                7,
            ),
            count("com.googlecode.iterm2", "iTerm2", None, 150, 8),
        ];
        let s = seen_sources(&counts, &Config::default());
        let apps: Vec<(&str, &str, i64)> = s
            .apps
            .iter()
            .map(|a| (a.bundle_id.as_str(), a.name.as_str(), a.ms))
            .collect();
        assert_eq!(
            apps,
            [
                ("com.microsoft.VSCode", "Code", 600_000),
                (CHROME, "Google Chrome", 450_000),
                ("com.googlecode.iterm2", "iTerm2", 150_000),
            ]
        );
        let domains: Vec<(&str, &str, i64)> = s
            .domains
            .iter()
            .map(|d| (d.domain.as_str(), d.label.as_str(), d.ms))
            .collect();
        assert_eq!(
            domains,
            [
                ("example.org", "example.org", 300_000),
                ("github.com", "GitHub", 150_000)
            ]
        );
    }

    #[test]
    fn passthrough_and_self_are_left_out_unless_counted() {
        let counts = [
            count("dev.timewent.app", "timewent", None, 100, 1),
            count("com.raycast.macos", "Raycast", None, 100, 1),
            count(
                "com.apple.Terminal",
                "Terminal",
                Some("https://not-a-browser.com"),
                10,
                1,
            ),
        ];
        let s = seen_sources(&counts, &Config::default());
        let names: Vec<&str> = s.apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Terminal"]);
        assert!(s.domains.is_empty(), "only browsers contribute sites");
        let counted = Config {
            count_self: true,
            ..Config::default()
        };
        assert!(seen_sources(&counts, &counted)
            .apps
            .iter()
            .any(|a| a.name == "timewent"));
    }

    #[test]
    fn an_app_shows_its_most_recent_name() {
        let counts = [
            count("x.y", "Old Name", None, 10, 100),
            count("x.y", "New Name", None, 10, 200),
        ];
        let s = seen_sources(&counts, &Config::default());
        assert_eq!(
            (s.apps[0].name.as_str(), s.apps[0].ms),
            ("New Name", 20_000)
        );
    }

    #[test]
    fn lists_are_capped() {
        let counts: Vec<SourceCount> = (0..80)
            .map(|i| count(&format!("app.{i}"), &format!("App {i}"), None, i + 1, 1))
            .collect();
        let s = seen_sources(&counts, &Config::default());
        assert_eq!(s.apps.len(), MAX_SOURCES);
        assert_eq!(s.apps[0].name, "App 79");
    }

    #[test]
    fn shape_is_the_contract() {
        let s = seen_sources(&[count("a.b", "A", None, 1, 1)], &Config::default());
        assert_eq!(
            serde_json::to_value(&s).expect("json"),
            serde_json::json!({"apps": [{"bundle_id": "a.b", "name": "A", "ms": 1000}], "domains": []})
        );
    }
}
