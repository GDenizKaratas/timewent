//! Project attribution (DESIGN §7.1): which project a segment was *for*, not which app it was.
//!
//! - known projects: labels of code-editor contexts seen in the range (case-insensitive);
//! - direct match, per sample while segmenting: a code repo URL or a window-title token;
//! - after segmentation: `localhost` joins the nearest code block of its gap-free block, and
//!   an AI / docs segment between two segments of the same project P (no away / gap between,
//!   ≤ `support_window_s` apart) counts as support for P. Plain web is never support.

use std::collections::BTreeMap;

use crate::activity::Member;
use crate::config::Config;
use crate::context::{web_host, Category, Context};
use crate::sample::Sample;
use crate::segment::{Evidence, Segment, SegmentKind, CODE_KEY_PREFIX};

/// Context key of a local dev server (`localhost` in the default labels).
const LOCALHOST_KEY: &str = "web:localhost";

/// Projects known in a range, keyed by [`normalize`]d name → the editor's spelling.
#[derive(Debug, Clone, Default)]
pub(crate) struct Projects {
    by_name: BTreeMap<String, String>,
    /// Longest names first, so `bank-agent-lab` wins over a project called `bank`.
    by_length: Vec<(String, String)>,
}

/// One sample's direct match: `(project, via)`.
pub(crate) type Match = Option<(String, String)>;

/// Case-insensitive; `-` and `_` are the same character (`bank_agent_lab` = `bank-agent-lab`).
fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| if c == '_' { '-' } else { c })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Characters that can be part of a project name inside a title or path.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

impl Projects {
    /// Labels of code-editor contexts. An untitled editor falls back to its app name — that is
    /// not a project.
    pub(crate) fn known(
        samples: &[Sample],
        contexts: &[Context],
        members: &[Option<Member>],
    ) -> Projects {
        let mut out = Projects::default();
        for ((s, c), m) in samples.iter().zip(contexts).zip(members) {
            out.learn(s, c, m.as_ref());
        }
        out
    }

    /// Records the project of a code context; `true` if it was not known yet. Activity
    /// members define no projects: your grouping beats inference (DESIGN §7.3).
    pub(crate) fn learn(
        &mut self,
        sample: &Sample,
        ctx: &Context,
        member: Option<&Member>,
    ) -> bool {
        if member.is_some() || !ctx.key.starts_with(CODE_KEY_PREFIX) || ctx.label == sample.app_name
        {
            return false;
        }
        let norm = normalize(&ctx.label);
        if self.by_name.contains_key(&norm) {
            return false;
        }
        self.by_name.insert(norm, ctx.label.clone());
        let mut v: Vec<(String, String)> = self
            .by_name
            .iter()
            .map(|(n, p)| (n.clone(), p.clone()))
            .collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        self.by_length = v;
        true
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// What `group_runs` needs per sample: direct matches of non-code samples.
    pub(crate) fn match_of(
        &self,
        sample: &Sample,
        ctx: &Context,
        member: Option<&Member>,
        config: &Config,
    ) -> Match {
        if member.is_some()
            || !config.attribute_projects
            || self.is_empty()
            || ctx.key.starts_with(CODE_KEY_PREFIX)
        {
            return None;
        }
        self.direct_match(sample)
    }

    /// The project a code context belongs to (its own label, if known).
    pub(crate) fn of_code(&self, ctx: &Context) -> Option<String> {
        if !ctx.key.starts_with(CODE_KEY_PREFIX) {
            return None;
        }
        self.by_name.get(&normalize(&ctx.label)).cloned()
    }

    /// Direct evidence in one non-code sample: `(project, via)`. A repo URL is authoritative —
    /// a repo page of an unknown project is not re-matched by its title.
    pub(crate) fn direct_match(&self, sample: &Sample) -> Option<(String, String)> {
        if let Some(url) = sample.url.as_deref() {
            if let Some((repo, via)) = repo_of(url) {
                return self
                    .by_name
                    .get(&normalize(&repo))
                    .map(|p| (p.clone(), via));
            }
        }
        let title = normalize(sample.window_title.as_deref()?);
        self.by_length
            .iter()
            .find(|(norm, _)| contains_name(&title, norm))
            .map(|(_, p)| (p.clone(), crate::explain::VIA_TITLE.to_string()))
    }
}

/// `needle` occurs in `hay` as a whole name (not inside a longer name).
fn contains_name(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    hay.match_indices(needle).any(|(i, m)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + m.len()..].chars().next();
        !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char)
    })
}

/// `github.com/{owner}/{repo}/…` or `gitlab.com/{group…}/{repo}[/-/…]` → (repo, display).
fn repo_of(url: &str) -> Option<(String, String)> {
    let host = web_host(url)?;
    let path = url::Url::parse(url).ok()?;
    let segs: Vec<&str> = path.path_segments()?.filter(|s| !s.is_empty()).collect();
    let repo_path: Vec<&str> = match host.as_str() {
        "github.com" if segs.len() >= 2 => segs[..2].to_vec(),
        "gitlab.com" => {
            let end = segs.iter().position(|s| *s == "-").unwrap_or(segs.len());
            if end < 2 {
                return None;
            }
            segs[..end].to_vec()
        }
        _ => return None,
    };
    let repo = repo_path.last()?.trim_end_matches(".git").to_string();
    Some((repo, format!("{host}/{}", repo_path.join("/"))))
}

/// Segment-level rules that need neighbours: localhost, then support sandwiches.
pub(crate) fn resolve(segments: &mut [Segment], config: &Config) {
    if !config.attribute_projects {
        return;
    }
    attribute_localhost(segments);
    attribute_support(segments, config);
}

fn is_barrier(s: &Segment) -> bool {
    matches!(s.kind, SegmentKind::Away | SegmentKind::Gap)
}

fn attribute_localhost(segments: &mut [Segment]) {
    for i in 0..segments.len() {
        if segments[i].key != LOCALHOST_KEY || segments[i].project.is_some() {
            continue;
        }
        let code = |s: &Segment| s.key.starts_with(CODE_KEY_PREFIX) && s.project.is_some();
        // Nearest code block within the gap-free block, by time; ties go to the earlier one.
        let before = segments[..i]
            .iter()
            .rev()
            .take_while(|s| s.kind != SegmentKind::Gap)
            .find(|s| code(s))
            .map(|s| (segments[i].start_ms - s.end_ms, s.project.clone()));
        let after = segments[i + 1..]
            .iter()
            .take_while(|s| s.kind != SegmentKind::Gap)
            .find(|s| code(s))
            .map(|s| (s.start_ms - segments[i].end_ms, s.project.clone()));
        let nearest = match (before, after) {
            (Some(b), Some(a)) => Some(if a.0 < b.0 { a } else { b }),
            (b, a) => b.or(a),
        };
        if let Some((_, Some(project))) = nearest {
            let seg = &mut segments[i];
            seg.evidence.push(Evidence::ProjectMatch {
                project: project.clone(),
                via: crate::explain::VIA_LOCALHOST.into(),
            });
            seg.project = Some(project);
        }
    }
}

fn attribute_support(segments: &mut [Segment], config: &Config) {
    // Anchors are fixed before any support is assigned: support never chains.
    let anchors: Vec<Option<String>> = segments.iter().map(|s| s.project.clone()).collect();
    let window_ms = i64::from(config.support_window_s) * 1000;
    for i in 0..segments.len() {
        let s = &segments[i];
        let supportable = matches!(s.category, Some(Category::Ai | Category::Docs))
            && !s.key.starts_with(crate::activity::ACTIVITY_KEY_PREFIX)
            && matches!(s.kind, SegmentKind::Focus | SegmentKind::Glance)
            && s.project.is_none();
        if !supportable {
            continue;
        }
        let left = (0..i)
            .rev()
            .take_while(|&j| !is_barrier(&segments[j]))
            .find(|&j| anchors[j].is_some());
        let right = (i + 1..segments.len())
            .take_while(|&j| !is_barrier(&segments[j]))
            .find(|&j| anchors[j].is_some());
        let (Some(l), Some(r)) = (left, right) else {
            continue;
        };
        let span_ms = segments[r].start_ms - segments[l].end_ms;
        if anchors[l] != anchors[r] || span_ms > window_ms {
            continue;
        }
        let Some(project) = anchors[l].clone() else {
            continue;
        };
        let seg = &mut segments[i];
        seg.evidence.push(Evidence::SupportFor {
            project: project.clone(),
            label: seg.label.clone(),
            window_s: span_ms / 1000,
        });
        seg.project = Some(project);
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::context::Category;
    use crate::sample::Sample;
    use crate::segment::{segment, Evidence, Segment, SegmentKind};
    use crate::summary::{summarize, RowKind};
    use crate::testkit::spans::*;

    fn segs(spans: &[(Option<Sample>, i64)]) -> Vec<Segment> {
        segment(&timeline(spans), &Config::default())
    }

    /// `(label, project)` per non-gap segment.
    fn projects(segs: &[Segment]) -> Vec<(&str, Option<&str>)> {
        segs.iter()
            .filter(|s| s.kind != SegmentKind::Gap)
            .map(|s| (s.label.as_str(), s.project.as_deref()))
            .collect()
    }

    fn matched(project: &str, via: &str) -> Evidence {
        Evidence::ProjectMatch {
            project: project.into(),
            via: via.into(),
        }
    }

    #[test]
    fn a_code_block_belongs_to_its_own_project_without_evidence() {
        let s = segs(&[(Some(code("bank-agent-lab")), 20)]);
        assert_eq!(projects(&s), [("bank-agent-lab", Some("bank-agent-lab"))]);
        assert!(s[0].evidence.is_empty());
    }

    #[test]
    fn repo_url_of_a_known_project_matches() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 20),
            (
                Some(browser(
                    "https://github.com/acme/bank-agent-lab/pull/7",
                    "Fix #7",
                )),
                20,
            ),
        ]);
        assert_eq!(s[1].project.as_deref(), Some("bank-agent-lab"));
        assert_eq!(
            s[1].evidence,
            [matched("bank-agent-lab", "github.com/acme/bank-agent-lab")]
        );
    }

    #[test]
    fn repo_of_an_unknown_project_stays_its_own_context() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 20),
            (
                Some(browser("https://github.com/acme/other-repo", "other")),
                20,
            ),
        ]);
        assert_eq!(s[1].project, None);
        assert!(s[1].evidence.is_empty());
    }

    #[test]
    fn gitlab_nested_group_repo_matches() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 20),
            (
                Some(browser(
                    "https://gitlab.com/acme/team/bank-agent-lab/-/merge_requests/3",
                    "MR",
                )),
                20,
            ),
        ]);
        assert_eq!(
            s[1].evidence,
            [matched(
                "bank-agent-lab",
                "gitlab.com/acme/team/bank-agent-lab"
            )]
        );
    }

    #[test]
    fn whole_title_token_matches_case_insensitively() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 20),
            (Some(chatgpt("Refactor Bank-Agent-Lab risk model")), 20),
            (Some(terminal("~/src/bank-agent-labs — zsh")), 20),
        ]);
        assert_eq!(
            projects(&s),
            [
                ("bank-agent-lab", Some("bank-agent-lab")),
                ("ChatGPT", Some("bank-agent-lab")),
                ("Terminal", None),
            ]
        );
        assert_eq!(s[1].evidence, [matched("bank-agent-lab", "window title")]);
    }

    #[test]
    fn a_title_match_must_cover_half_the_segment() {
        let mut spans = vec![(Some(code("p1")), 20)];
        spans.push((Some(chatgpt("about p1")), 4));
        spans.push((Some(browser("https://chatgpt.com/c/2", "dinner ideas")), 0));
        let mut t = timeline(&spans);
        // Same ChatGPT context key, different titles: 4s about p1, 16s about dinner.
        for i in 0..16 {
            t.push(Sample {
                ts_ms: (24 + i) * 1000,
                ..chatgpt("dinner ideas")
            });
        }
        let s = segment(&t, &Config::default());
        assert_eq!(s[1].label, "ChatGPT");
        assert_eq!(s[1].project, None);
    }

    #[test]
    fn editor_without_a_title_is_not_a_project() {
        let untitled = Sample {
            window_title: None,
            ..code("x")
        };
        let s = segs(&[
            (Some(untitled), 20),
            (Some(chatgpt("Code review tips")), 20),
        ]);
        assert_eq!(s[0].project, None);
        assert_eq!(s[1].project, None);
    }

    #[test]
    fn an_untitled_editor_is_an_app_row_and_never_an_anchor() {
        // From a real export: VS Code without window titles (Accessibility off) became a
        // project row named "Code".
        let untitled = Sample {
            window_title: None,
            ..code("x")
        };
        let s = segs(&[
            (Some(untitled.clone()), 60),
            (Some(chatgpt("x")), 60),
            (Some(untitled), 60),
        ]);
        assert!(
            s.iter().all(|s| s.project.is_none()),
            "ChatGPT is no research for 'Code'"
        );
        let rows = summarize(&s).rows;
        let code_row = rows.iter().find(|r| r.label == "Code").expect("row");
        assert_eq!(
            (code_row.key.as_str(), code_row.kind),
            ("app:Code", RowKind::Context)
        );
        assert_eq!(code_row.category, Category::Code);
    }

    #[test]
    fn localhost_goes_to_the_nearest_code_block_of_its_block() {
        let s = segs(&[
            (Some(code("api")), 20),
            (Some(youtube()), 30),
            (Some(browser("http://localhost:3000/", "dev")), 20),
            (Some(code("web")), 20),
        ]);
        assert_eq!(s[2].label, "localhost");
        assert_eq!(s[2].project.as_deref(), Some("web"));
        assert_eq!(
            s[2].evidence,
            [matched("web", "localhost next to its code block")]
        );
    }

    #[test]
    fn localhost_never_reaches_across_a_gap() {
        let s = segs(&[
            (Some(code("api")), 20),
            (None, 600),
            (Some(browser("http://localhost:3000/", "dev")), 20),
        ]);
        assert_eq!(projects(&s)[1], ("localhost", None));
    }

    #[test]
    fn ai_between_two_blocks_of_one_project_is_support() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 60),
            (Some(chatgpt("Rust lifetimes explained")), 300),
            (Some(code("bank-agent-lab")), 60),
        ]);
        assert_eq!(s[1].project.as_deref(), Some("bank-agent-lab"));
        assert_eq!(
            s[1].evidence,
            [Evidence::SupportFor {
                project: "bank-agent-lab".into(),
                label: "ChatGPT".into(),
                window_s: 300,
            }]
        );
    }

    #[test]
    fn support_spans_unattributed_neighbours_but_web_never_counts() {
        let s = segs(&[
            (Some(code("p")), 60),
            (Some(docs()), 60),
            (Some(youtube()), 30),
            (Some(code("p")), 60),
        ]);
        assert_eq!(
            projects(&s),
            [
                ("p", Some("p")),
                ("docs", Some("p")),
                ("YouTube", None),
                ("p", Some("p")),
            ]
        );
    }

    #[test]
    fn support_needs_the_same_project_on_both_sides() {
        let s = segs(&[
            (Some(code("a")), 60),
            (Some(chatgpt("x")), 60),
            (Some(code("b")), 60),
        ]);
        assert_eq!(s[1].project, None);
    }

    #[test]
    fn support_window_is_inclusive_and_configurable() {
        let spans = |gap_s| {
            [
                (Some(code("p")), 20),
                (Some(chatgpt("x")), gap_s),
                (Some(code("p")), 20),
            ]
        };
        assert_eq!(segs(&spans(600))[1].project.as_deref(), Some("p"));
        assert_eq!(segs(&spans(601))[1].project, None);
        let tight = Config {
            support_window_s: 60,
            ..Config::default()
        };
        assert_eq!(segment(&timeline(&spans(61)), &tight)[1].project, None);
    }

    #[test]
    fn away_or_gap_between_breaks_support() {
        let s = segs(&[
            (Some(code("p")), 60),
            (Some(chatgpt("x")), 60),
            (Some(away(code("p"))), 30),
            (Some(code("p")), 60),
        ]);
        assert_eq!(projects(&s)[1], ("ChatGPT", None));
        let s = segs(&[
            (Some(code("p")), 60),
            (Some(chatgpt("x")), 60),
            (None, 60),
            (Some(code("p")), 60),
        ]);
        assert_eq!(projects(&s)[1], ("ChatGPT", None));
    }

    #[test]
    fn direct_matches_anchor_support_too() {
        let s = segs(&[
            (Some(code("p")), 60),
            (Some(docs()), 60),
            (Some(browser("https://github.com/o/p", "p")), 60),
        ]);
        assert_eq!(projects(&s)[1], ("docs", Some("p")));
    }

    #[test]
    fn attribution_off_restores_plain_contexts() {
        let off = Config {
            attribute_projects: false,
            ..Config::default()
        };
        let t = timeline(&[
            (Some(code("p")), 60),
            (Some(chatgpt("p plans")), 60),
            (Some(code("p")), 60),
        ]);
        let s = segment(&t, &off);
        assert!(s.iter().all(|s| s.project.is_none()));
        assert!(s.iter().all(|s| s.evidence.is_empty()));
        let rows = summarize(&s).rows;
        assert!(rows.iter().all(|r| r.kind == RowKind::Context));
    }

    #[test]
    fn explain_lines_say_why() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 60),
            (Some(docs()), 360),
            (
                Some(browser("https://github.com/x/bank-agent-lab", "PR")),
                60,
            ),
        ]);
        assert_eq!(
            crate::explain::explain(&s[1], crate::lang::Lang::En),
            ["docs counted as research for bank-agent-lab (between two bank-agent-lab blocks, 6m00s apart)"]
        );
        assert_eq!(
            crate::explain::explain(&s[2], crate::lang::Lang::En),
            ["github.com/x/bank-agent-lab matched project bank-agent-lab"]
        );
    }

    #[test]
    fn project_row_rolls_up_with_a_category_breakdown() {
        let s = segs(&[
            (Some(code("bank-agent-lab")), 120),
            (Some(chatgpt("x")), 60),
            (Some(docs()), 40),
            (
                Some(browser("https://github.com/x/bank-agent-lab", "PR")),
                30,
            ),
            (Some(code("bank-agent-lab")), 60),
            (Some(youtube()), 15),
            (Some(code("bank-agent-lab")), 20),
        ]);
        let summary = summarize(&s);
        let rows: Vec<(&str, &str, RowKind, i64)> = summary
            .rows
            .iter()
            .map(|r| (r.key.as_str(), r.label.as_str(), r.kind, r.ms))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "code:bank-agent-lab",
                    "bank-agent-lab",
                    RowKind::Project,
                    330_000
                ),
                ("web:youtube.com", "YouTube", RowKind::Context, 15_000),
            ]
        );
        let p = &summary.rows[0];
        assert_eq!(p.category, Category::Code);
        let breakdown: Vec<(&str, &str, i64)> = p
            .breakdown
            .iter()
            .map(|b| (b.key.as_str(), b.label.as_str(), b.ms))
            .collect();
        assert_eq!(
            breakdown,
            [
                ("code", "code", 200_000),
                ("ai", "ai", 60_000),
                ("docs", "docs", 40_000),
                ("web", "web", 30_000),
            ]
        );
        assert!(summary.rows[1].breakdown.is_empty());
        assert!((p.share - 330.0 / 345.0).abs() < 1e-12);
    }
}
