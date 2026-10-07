//! User-defined activities (DESIGN §7.3): a name you give to a set of apps and sites. Matched
//! per sample, before project attribution; members are never attributed to a project.

use crate::config::{Activity, Config};
use crate::context::{derive_context, is_browser, web_host, Category, Context};
use crate::sample::Sample;

/// Context keys of activities: `act:{name}`.
pub const ACTIVITY_KEY_PREFIX: &str = "act:";

/// The member of an activity a sample counts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub activity: String,
    /// Bundle id (app member) or the configured domain (site member).
    pub key: String,
    /// App name, or the site's configured label (else the domain).
    pub label: String,
    /// The sample's own category (an activity's category is its largest member's).
    pub category: Category,
}

fn norm_host(h: &str) -> String {
    let h = h.trim().to_lowercase();
    h.strip_prefix("www.").map(String::from).unwrap_or(h)
}

/// The activity this sample belongs to, if any: an app member by exact bundle id first, then
/// a site member (host equal to, or a subdomain of, a listed domain). First activity wins.
pub fn activity_member(sample: &Sample, config: &Config) -> Option<Member> {
    if config.activities.is_empty() {
        return None;
    }
    let host = is_browser(&sample.bundle_id)
        .then(|| sample.url.as_deref().and_then(web_host))
        .flatten();
    config
        .activities
        .iter()
        .find_map(|a| member_of(a, sample, host.as_deref(), config))
}

fn member_of(a: &Activity, sample: &Sample, host: Option<&str>, config: &Config) -> Option<Member> {
    let category = || derive_context(sample, config).category;
    if a.apps.iter().any(|b| b == &sample.bundle_id) {
        return Some(Member {
            activity: a.name.clone(),
            key: sample.bundle_id.clone(),
            label: sample.app_name.clone(),
            category: category(),
        });
    }
    let host = host?;
    let domain = a
        .domains
        .iter()
        .map(|d| norm_host(d))
        .find(|d| !d.is_empty() && (host == d || host.ends_with(&format!(".{d}"))))?;
    let label = config
        .labels
        .get(&domain)
        .map_or_else(|| domain.clone(), |r| r.label.clone());
    Some(Member {
        activity: a.name.clone(),
        key: domain,
        label,
        category: category(),
    })
}

/// The per-sample context segmentation groups by: the activity's when the sample is a
/// member (the detail keeps the original label and detail visible), else `derive_context`'s.
pub(crate) fn prepare(sample: &Sample, config: &Config) -> (Context, Option<Member>) {
    let ctx = derive_context(sample, config);
    match activity_member(sample, config) {
        None => (ctx, None),
        Some(m) => {
            let detail = match ctx.detail {
                Some(d) => format!("{} · {d}", ctx.label),
                None => ctx.label,
            };
            (
                Context {
                    key: format!("{ACTIVITY_KEY_PREFIX}{}", m.activity),
                    label: m.activity.clone(),
                    detail: Some(detail),
                    category: m.category,
                },
                Some(m),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Activity, Config};
    use crate::context::Category;
    use crate::explain::explain;
    use crate::lang::Lang;
    use crate::sample::Sample;
    use crate::segment::{segment, Evidence, SegmentKind};
    use crate::summary::{summarize, RowKind};
    use crate::testkit::spans::*;

    fn iterm(title: &str) -> Sample {
        Sample {
            app_name: "iTerm2".into(),
            bundle_id: "com.googlecode.iterm2".into(),
            window_title: Some(title.into()),
            url: None,
            ..code("x")
        }
    }

    fn coding() -> Config {
        Config {
            activities: vec![Activity {
                name: "coding".into(),
                apps: vec![
                    "com.microsoft.VSCode".into(),
                    "com.googlecode.iterm2".into(),
                ],
                domains: vec!["github.com".into()],
            }],
            ..Config::default()
        }
    }

    fn segs(spans: &[(Option<Sample>, i64)], config: &Config) -> Vec<crate::segment::Segment> {
        segment(&timeline(spans), config)
    }

    #[test]
    fn no_activities_changes_nothing() {
        let s = segs(
            &[(Some(code("p")), 20), (Some(iterm("zsh")), 20)],
            &Config::default(),
        );
        let keys: Vec<&str> = s.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["code:p", "app:iTerm2"]);
    }

    #[test]
    fn alternating_members_form_one_activity_segment() {
        let s = segs(
            &[
                (Some(code("bank-agent-lab")), 20),
                (Some(iterm("~/src/x — zsh")), 2),
                (Some(code("bank-agent-lab")), 20),
            ],
            &coding(),
        );
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].key.as_str(), s[0].label.as_str()),
            ("act:coding", "coding")
        );
        assert_eq!(s[0].kind, SegmentKind::Focus);
        assert!(s[0].interruptions.is_empty(), "a member is not a transient");
        assert_eq!(
            s[0].category,
            Some(Category::Code),
            "largest member's category"
        );
        assert_eq!(s[0].project, None);
        let details: Vec<(&str, i64)> = s[0]
            .details
            .iter()
            .map(|d| (d.detail.as_str(), d.ms))
            .collect();
        assert_eq!(
            details,
            [
                ("bank-agent-lab · main.rs", 40_000),
                ("iTerm2 · ~/src/x — zsh", 2_000)
            ]
        );
        assert_eq!(
            s[0].evidence,
            [
                Evidence::UserActivity {
                    activity: "coding".into(),
                    member: "Code".into()
                },
                Evidence::UserActivity {
                    activity: "coding".into(),
                    member: "iTerm2".into()
                },
            ]
        );
        assert_eq!(
            explain(&s[0], Lang::En),
            [
                "Code counted as coding — your activity",
                "iTerm2 counted as coding — your activity"
            ]
        );
        assert_eq!(
            explain(&s[0], Lang::Tr)[1],
            "iTerm2, coding etkinliğine sayıldı — senin tanımın"
        );
    }

    #[test]
    fn domains_match_exactly_or_as_a_subdomain() {
        let s = segs(
            &[
                (Some(browser("https://www.github.com/a/b", "PR")), 20),
                (Some(browser("https://docs.github.com/x", "docs")), 20),
                (Some(browser("https://notgithub.com/x", "other")), 20),
            ],
            &coding(),
        );
        let keys: Vec<&str> = s.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["act:coding", "web:notgithub.com"]);
        assert_eq!(
            s[0].members
                .iter()
                .map(|m| (m.key.as_str(), m.label.as_str(), m.ms))
                .collect::<Vec<_>>(),
            [("github.com", "GitHub", 40_000)]
        );
    }

    #[test]
    fn first_matching_activity_wins_and_apps_before_domains() {
        let mut c = coding();
        c.activities.insert(
            0,
            Activity {
                name: "review".into(),
                apps: vec![],
                domains: vec!["github.com".into()],
            },
        );
        let s = segs(
            &[
                (Some(browser("https://github.com/a/b", "PR")), 20),
                (Some(code("p")), 20),
            ],
            &c,
        );
        let keys: Vec<&str> = s.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["act:review", "act:coding"]);
    }

    #[test]
    fn away_stays_away_inside_an_activity() {
        let s = segs(
            &[
                (Some(code("p")), 20),
                (Some(away(code("p"))), 20),
                (Some(code("p")), 20),
            ],
            &coding(),
        );
        let kinds: Vec<SegmentKind> = s.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [SegmentKind::Focus, SegmentKind::Away, SegmentKind::Focus]
        );
    }

    #[test]
    fn members_are_never_attributed_nor_anchors() {
        // VS Code is a member: bank-agent-lab is not a known project, so the ChatGPT chat that
        // names it and sits between two coding blocks stays its own context.
        let s = segs(
            &[
                (Some(code("bank-agent-lab")), 60),
                (Some(chatgpt("bank-agent-lab risk model")), 60),
                (Some(code("bank-agent-lab")), 60),
            ],
            &coding(),
        );
        assert!(s.iter().all(|s| s.project.is_none()));
        assert_eq!(s[1].key, "web:chatgpt.com");
        assert!(s[1].evidence.is_empty());
    }

    #[test]
    fn a_non_member_editor_still_defines_projects() {
        let cursor = Sample {
            app_name: "Cursor".into(),
            bundle_id: "com.todesktop.230313mzl4w4u92".into(),
            ..code("p")
        };
        let s = segs(
            &[
                (Some(cursor.clone()), 60),
                (Some(chatgpt("x")), 60),
                (Some(cursor), 60),
            ],
            &coding(),
        );
        assert_eq!(s[1].project.as_deref(), Some("p"));
    }

    #[test]
    fn activity_row_has_member_breakdown() {
        let s = segs(
            &[
                (Some(code("p")), 100),
                (Some(iterm("zsh")), 40),
                (Some(youtube()), 30),
                (Some(code("p")), 20),
            ],
            &coding(),
        );
        let summary = summarize(&s);
        let row = &summary.rows[0];
        assert_eq!(
            (
                row.key.as_str(),
                row.label.as_str(),
                row.kind,
                row.category,
                row.ms
            ),
            (
                "act:coding",
                "coding",
                RowKind::Activity,
                Category::Code,
                160_000
            )
        );
        let b: Vec<(&str, &str, i64)> = row
            .breakdown
            .iter()
            .map(|b| (b.key.as_str(), b.label.as_str(), b.ms))
            .collect();
        assert_eq!(
            b,
            [
                ("com.microsoft.VSCode", "Code", 120_000),
                ("com.googlecode.iterm2", "iTerm2", 40_000)
            ]
        );
        assert_eq!(summary.rows[1].kind, RowKind::Context);
        assert!(summary.rows[1].breakdown.is_empty());
    }

    #[test]
    fn activities_load_from_json_and_default_to_none() {
        assert!(Config::default().activities.is_empty());
        let c: Config = serde_json::from_str(
            r#"{"activities":[{"name":"coding","apps":["com.googlecode.iterm2"]}]}"#,
        )
        .expect("parse");
        assert_eq!(c.activities[0].domains, Vec::<String>::new());
    }
}
