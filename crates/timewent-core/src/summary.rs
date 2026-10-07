//! Where the time went, per context: `summarize` (PLAN §3.6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::activity::ACTIVITY_KEY_PREFIX;
use crate::context::Category;
use crate::segment::{sorted_details, DetailTime, Segment, SegmentKind, PASSTHROUGH_KEY_PREFIX};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Everything except gaps. Always `active_ms + passive_ms + away_ms`.
    pub total_ms: i64,
    pub active_ms: i64,
    pub passive_ms: i64,
    pub away_ms: i64,
    /// Per key over focus + glance segments, sorted by `ms` desc then `key` asc. Hostless
    /// passthrough segments (`pass:` keys) count in the totals but never get a row.
    pub rows: Vec<Row>,
    /// Longest stretch of adjacent segments rolling into one row (§11.3); absorbed
    /// transients and pass-through segments don't break it, away / gap / another row do.
    #[serde(default)]
    pub longest_focus_ms: i64,
    /// Boundaries between such stretches that are not separated by away or gap.
    #[serde(default)]
    pub switches: u32,
    /// What kind of time (§14.1): in-use time per segment category (activity members by
    /// their own), ms desc. Share = same denominator as rows. Pass-through time is left out.
    #[serde(default)]
    pub categories: Vec<CategoryShare>,
    /// Background audio (§14.2), per source and title, ms desc. Not part of any total.
    #[serde(default)]
    pub listening: Vec<Listening>,
    /// In-use time that is no row: pass-through apps not credited to anything (§21.1),
    /// per app, ms desc — "not shown: timewent 1m35s".
    #[serde(default)]
    pub not_shown: Vec<NotShown>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotShown {
    pub label: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryShare {
    pub category: Category,
    pub ms: i64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listening {
    pub label: String,
    pub title: Option<String>,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub category: Category,
    pub ms: i64,
    /// Fraction of non-away time, `0.0..=1.0`.
    pub share: f64,
    /// Sorted by `ms` desc then `detail` asc.
    pub details: Vec<DetailTime>,
    #[serde(default)]
    pub kind: RowKind,
    /// Project rows: time per category (key = label = `code` / `ai` / …). Activity rows: time
    /// per member (key = bundle id or domain, label = app / site name). Ms desc. Empty for
    /// context rows.
    #[serde(default)]
    pub breakdown: Vec<BreakdownItem>,
}

/// A project (§11.1: its code plus everything attributed to it) or a plain context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Project,
    /// A group you defined (§13.1).
    Activity,
    #[default]
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakdownItem {
    pub key: String,
    pub label: String,
    pub ms: i64,
}

/// `(longest stretch ms, switches)` over the timeline.
fn focus(segments: &[Segment]) -> (i64, u32) {
    let (mut longest, mut switches) = (0, 0);
    let mut stretch: Option<(String, i64)> = None;
    for seg in segments {
        match seg.kind {
            SegmentKind::Away | SegmentKind::Gap => {
                if let Some((_, ms)) = stretch.take() {
                    longest = longest.max(ms);
                }
            }
            _ if seg.key.starts_with(PASSTHROUGH_KEY_PREFIX) => {}
            SegmentKind::Focus | SegmentKind::Glance => {
                let key = seg.row_key();
                match &mut stretch {
                    Some((k, ms)) if *k == key => *ms += seg.ms(),
                    Some((_, ms)) => {
                        longest = longest.max(*ms);
                        switches += 1;
                        stretch = Some((key, seg.ms()));
                    }
                    None => stretch = Some((key, seg.ms())),
                }
            }
        }
    }
    if let Some((_, ms)) = stretch {
        longest = longest.max(ms);
    }
    (longest, switches)
}

struct Agg {
    row: Row,
    details: BTreeMap<String, i64>,
    by_category: BTreeMap<Category, i64>,
    /// Activity rows: member key → (label, category, ms).
    members: BTreeMap<String, (String, Category, i64)>,
}

pub fn summarize(segments: &[Segment]) -> Summary {
    let mut summary = Summary {
        total_ms: 0,
        active_ms: 0,
        passive_ms: 0,
        away_ms: 0,
        rows: Vec::new(),
        longest_focus_ms: 0,
        switches: 0,
        categories: Vec::new(),
        listening: Vec::new(),
        not_shown: Vec::new(),
    };
    let mut hidden: BTreeMap<String, i64> = BTreeMap::new();
    let mut kinds: BTreeMap<Category, i64> = BTreeMap::new();
    let mut listened: BTreeMap<(String, Option<String>), i64> = BTreeMap::new();
    // Keyed by row key; BTreeMap keeps aggregation order deterministic.
    let mut rows: BTreeMap<String, Agg> = BTreeMap::new();

    for seg in segments {
        for l in &seg.listening {
            *listened
                .entry((l.label.clone(), l.title.clone()))
                .or_insert(0) += l.ms;
        }
        match seg.kind {
            SegmentKind::Gap => continue,
            SegmentKind::Away => summary.away_ms += seg.ms(),
            SegmentKind::Focus | SegmentKind::Glance => {
                summary.active_ms += seg.active_ms;
                summary.passive_ms += seg.passive_ms;
                if seg.key.starts_with(PASSTHROUGH_KEY_PREFIX) {
                    // In-use time, but never a place your time "went" (PLAN §10, §21.1).
                    *hidden.entry(seg.label.clone()).or_insert(0) += seg.ms();
                    summary.total_ms += seg.ms();
                    continue;
                }
                // Focus/glance segments always carry a category.
                let category = seg.category.unwrap_or(Category::App);
                let mut rest = seg.ms();
                for m in &seg.members {
                    *kinds.entry(m.category.unwrap_or(category)).or_insert(0) += m.ms;
                    rest -= m.ms;
                }
                if rest > 0 {
                    *kinds.entry(category).or_insert(0) += rest;
                }
                let agg = rows.entry(seg.row_key()).or_insert_with(|| {
                    let (label, kind, row_category) = match &seg.project {
                        Some(p) => (p.clone(), RowKind::Project, Category::Code),
                        None if seg.key.starts_with(ACTIVITY_KEY_PREFIX) => {
                            (seg.label.clone(), RowKind::Activity, category)
                        }
                        None => (seg.label.clone(), RowKind::Context, category),
                    };
                    Agg {
                        row: Row {
                            key: seg.row_key(),
                            label,
                            category: row_category,
                            ms: 0,
                            share: 0.0,
                            details: Vec::new(),
                            kind,
                            breakdown: Vec::new(),
                        },
                        details: BTreeMap::new(),
                        by_category: BTreeMap::new(),
                        members: BTreeMap::new(),
                    }
                });
                agg.row.ms += seg.ms();
                *agg.by_category.entry(category).or_insert(0) += seg.ms();
                for m in &seg.members {
                    let e = agg.members.entry(m.key.clone()).or_insert((
                        m.label.clone(),
                        m.category.unwrap_or(category),
                        0,
                    ));
                    e.2 += m.ms;
                }
                for d in &seg.details {
                    *agg.details.entry(d.detail.clone()).or_insert(0) += d.ms;
                }
            }
        }
        summary.total_ms += seg.ms();
    }

    (summary.longest_focus_ms, summary.switches) = focus(segments);
    let present_ms = summary.total_ms - summary.away_ms;
    summary.rows = rows
        .into_values()
        .map(|agg| {
            let mut row = agg.row;
            row.share = if present_ms > 0 {
                row.ms as f64 / present_ms as f64
            } else {
                0.0
            };
            row.details = sorted_details(agg.details);
            match row.kind {
                RowKind::Project => {
                    let mut b: Vec<(Category, i64)> = agg.by_category.into_iter().collect();
                    b.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
                    row.breakdown = b
                        .into_iter()
                        .map(|(c, ms)| BreakdownItem {
                            key: c.as_str().into(),
                            label: c.as_str().into(),
                            ms,
                        })
                        .collect();
                }
                RowKind::Activity => {
                    let mut m: Vec<(String, (String, Category, i64))> =
                        agg.members.into_iter().collect();
                    m.sort_by(|x, y| y.1 .2.cmp(&x.1 .2).then_with(|| x.1 .0.cmp(&y.1 .0)));
                    if let Some((_, (_, c, _))) = m.first() {
                        row.category = *c;
                    }
                    row.breakdown = m
                        .into_iter()
                        .map(|(key, (label, _, ms))| BreakdownItem { key, label, ms })
                        .collect();
                }
                RowKind::Context => {}
            }
            row
        })
        .collect();
    summary
        .rows
        .sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.key.cmp(&b.key)));
    let mut kinds: Vec<(Category, i64)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    summary.categories = kinds
        .into_iter()
        .map(|(category, ms)| CategoryShare {
            category,
            ms,
            share: if present_ms > 0 {
                ms as f64 / present_ms as f64
            } else {
                0.0
            },
        })
        .collect();
    let mut listened: Vec<Listening> = listened
        .into_iter()
        .map(|((label, title), ms)| Listening { label, title, ms })
        .collect();
    listened.sort_by(|a, b| {
        b.ms.cmp(&a.ms)
            .then_with(|| (&a.label, &a.title).cmp(&(&b.label, &b.title)))
    });
    summary.listening = listened;
    let mut hidden: Vec<NotShown> = hidden
        .into_iter()
        .map(|(label, ms)| NotShown { label, ms })
        .collect();
    hidden.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.label.cmp(&b.label)));
    summary.not_shown = hidden;
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    mod categories {
        use crate::config::{Activity, Config};
        use crate::context::Category;
        use crate::sample::Sample;
        use crate::segment::segment;
        use crate::summary::{summarize, CategoryShare};
        use crate::testkit::spans::*;

        fn cats(spans: &[(Option<Sample>, i64)], config: &Config) -> Vec<(Category, i64)> {
            summarize(&segment(&timeline(spans), config))
                .categories
                .iter()
                .map(|c| (c.category, c.ms))
                .collect()
        }

        #[test]
        fn each_segment_counts_as_its_own_kind_even_when_it_served_a_project() {
            let spans = [
                (Some(code("p")), 120),
                (Some(chatgpt("x")), 60),
                (Some(code("p")), 60),
                (Some(youtube()), 30),
            ];
            assert_eq!(
                cats(&spans, &Config::default()),
                [
                    (Category::Code, 180_000),
                    (Category::Ai, 60_000),
                    (Category::Media, 30_000)
                ]
            );
        }

        #[test]
        fn shares_use_the_rows_denominator_and_away_is_left_out() {
            let s = summarize(&segment(
                &timeline(&[
                    (Some(code("p")), 30),
                    (Some(away(code("p"))), 30),
                    (Some(youtube()), 10),
                ]),
                &Config::default(),
            ));
            assert_eq!(
                s.categories,
                [
                    CategoryShare {
                        category: Category::Code,
                        ms: 30_000,
                        share: 0.75
                    },
                    CategoryShare {
                        category: Category::Media,
                        ms: 10_000,
                        share: 0.25
                    },
                ]
            );
        }

        #[test]
        fn activity_members_keep_their_own_kinds() {
            let config = Config {
                activities: vec![Activity {
                    name: "flow".into(),
                    apps: vec!["com.microsoft.VSCode".into()],
                    domains: vec!["youtube.com".into()],
                }],
                ..Config::default()
            };
            let spans = [
                (Some(code("p")), 60),
                (Some(youtube()), 20),
                (Some(code("p")), 60),
            ];
            assert_eq!(
                cats(&spans, &config),
                [(Category::Code, 120_000), (Category::Media, 20_000)]
            );
        }

        #[test]
        fn pass_through_time_is_not_a_kind() {
            let me = Sample {
                bundle_id: "dev.timewent.app".into(),
                app_name: "timewent".into(),
                ..code("x")
            };
            assert!(cats(&[(Some(me), 30)], &Config::default()).is_empty());
        }
    }

    mod focus {
        use crate::config::Config;
        use crate::sample::Sample;
        use crate::segment::segment;
        use crate::summary::summarize;
        use crate::testkit::spans::*;

        fn summary(spans: &[(Option<Sample>, i64)]) -> crate::summary::Summary {
            summarize(&segment(&timeline(spans), &Config::default()))
        }

        #[test]
        fn one_project_across_ai_support_is_one_focus_stretch() {
            // P 60s → ChatGPT 60s (support for P) → P 60s → YouTube 9s (glance) → P 30s →
            // locked 20s → Q 40s.
            let s = summary(&[
                (Some(code("p")), 60),
                (Some(chatgpt("x")), 60),
                (Some(code("p")), 60),
                (Some(youtube()), 9),
                (Some(code("p")), 30),
                (Some(away(code("p"))), 20),
                (Some(code("q")), 40),
            ]);
            // Stretches: [P 180s] [YouTube 9s] [P 30s] | away | [Q 40s].
            assert_eq!(s.longest_focus_ms, 180_000);
            // P→YouTube, YouTube→P; P→Q is separated by away, not a switch.
            assert_eq!(s.switches, 2);
        }

        #[test]
        fn absorbed_transients_do_not_break_a_stretch() {
            let s = summary(&[
                (Some(code("p")), 60),
                (Some(youtube()), 2),
                (Some(code("p")), 60),
            ]);
            assert_eq!((s.longest_focus_ms, s.switches), (122_000, 0));
        }

        #[test]
        fn a_gap_ends_a_stretch_without_a_switch() {
            let s = summary(&[(Some(code("p")), 60), (None, 600), (Some(code("q")), 90)]);
            assert_eq!((s.longest_focus_ms, s.switches), (90_000, 0));
        }

        #[test]
        fn passthrough_segments_are_transparent() {
            let me = Sample {
                bundle_id: "dev.timewent.app".into(),
                app_name: "timewent".into(),
                ..code("x")
            };
            // timewent alone at the start (hostless pass:), then code.
            let s = summary(&[(Some(me), 5), (Some(code("p")), 60)]);
            assert_eq!(s.switches, 0);
            assert_eq!(
                s.longest_focus_ms, 65_000,
                "the pass-through run was absorbed"
            );
        }

        #[test]
        fn passthrough_between_one_app_keeps_the_stretch_but_adds_no_time() {
            // §21 clarification: PDFgear 30s + timewent 95s + PDFgear 40s → one 70s stretch.
            let pdf = Sample {
                bundle_id: "com.pdfgear.mac".into(),
                app_name: "PDFgear".into(),
                window_title: None,
                ..code("x")
            };
            let me = Sample {
                bundle_id: "dev.timewent.app".into(),
                app_name: "timewent".into(),
                window_title: None,
                ..code("x")
            };
            let s = summary(&[(Some(pdf.clone()), 30), (Some(me), 95), (Some(pdf), 40)]);
            assert_eq!((s.longest_focus_ms, s.switches), (70_000, 0));
            assert_eq!(s.total_ms, 165_000, "the 95s is still in use");
        }

        #[test]
        fn nothing_tracked_has_no_focus() {
            let s = summarize(&[]);
            assert_eq!((s.longest_focus_ms, s.switches), (0, 0));
        }
    }
    use crate::config::Config;
    use crate::segment::segment;
    use crate::testkit::{idles, seq};

    fn summary_of(samples: &[crate::sample::Sample]) -> Summary {
        summarize(&segment(samples, &Config::default()))
    }

    fn row_keys(s: &Summary) -> Vec<(&str, i64)> {
        s.rows.iter().map(|r| (r.key.as_str(), r.ms)).collect()
    }

    #[test]
    fn empty_segments_give_zero_summary() {
        let s = summarize(&[]);
        assert_eq!(
            s,
            Summary {
                total_ms: 0,
                active_ms: 0,
                passive_ms: 0,
                away_ms: 0,
                rows: vec![],
                longest_focus_ms: 0,
                switches: 0,
                categories: vec![],
                listening: vec![],
                not_shown: vec![],
            }
        );
    }

    #[test]
    fn totals_exclude_gaps() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(seq(3_600_000, &[("A", 20)]));
        let s = summary_of(&samples);
        assert_eq!(s.total_ms, 40_000);
        assert_eq!(s.active_ms, 40_000);
        assert_eq!(row_keys(&s), vec![("app:A", 40_000)]);
    }

    #[test]
    fn away_time_is_counted_separately_and_has_no_row() {
        let mut samples = seq(0, &[("A", 30)]);
        samples.extend(idles(30_000, "A", &[200.0; 60]));
        samples.extend(seq(90_000, &[("B", 10)]));
        let s = summary_of(&samples);
        assert_eq!(s.away_ms, 60_000);
        assert_eq!(s.total_ms, 100_000);
        assert_eq!(s.active_ms + s.passive_ms + s.away_ms, s.total_ms);
        assert_eq!(row_keys(&s), vec![("app:A", 30_000), ("app:B", 10_000)]);
    }

    #[test]
    fn share_is_fraction_of_non_away_time() {
        let mut samples = seq(0, &[("A", 30)]);
        samples.extend(idles(30_000, "A", &[200.0; 60]));
        samples.extend(seq(90_000, &[("B", 10)]));
        let s = summary_of(&samples);
        assert_eq!(s.rows[0].share, 0.75);
        assert_eq!(s.rows[1].share, 0.25);
    }

    #[test]
    fn rows_sorted_by_ms_desc_then_key_asc() {
        let s = summary_of(&seq(0, &[("C", 20), ("B", 30), ("A", 20)]));
        assert_eq!(
            row_keys(&s),
            vec![("app:B", 30_000), ("app:A", 20_000), ("app:C", 20_000)]
        );
    }

    #[test]
    fn glance_rows_are_included_and_same_key_segments_aggregate() {
        let s = summary_of(&seq(0, &[("A", 20), ("B", 5), ("A", 20)]));
        assert_eq!(row_keys(&s), vec![("app:A", 40_000), ("app:B", 5_000)]);
        assert_eq!(s.rows[1].label, "B");
        assert_eq!(s.rows[1].category, Category::App);
    }

    #[test]
    fn passive_time_is_reported_separately_from_active() {
        let mut idle = vec![0.0; 10];
        idle.extend((45..75).map(f64::from));
        idle.extend([0.0; 10]);
        let s = summary_of(&idles(0, "A", &idle));
        assert_eq!((s.active_ms, s.passive_ms, s.away_ms), (20_000, 30_000, 0));
        assert_eq!(s.total_ms, 50_000);
    }

    #[test]
    fn row_details_merge_across_segments_sorted_desc() {
        let mut segs = segment(
            &seq(0, &[("A", 20), ("B", 5), ("A", 20)]),
            &Config::default(),
        );
        segs[0].details = vec![
            DetailTime {
                detail: "x".into(),
                ms: 15_000,
            },
            DetailTime {
                detail: "y".into(),
                ms: 5_000,
            },
        ];
        segs[2].details = vec![DetailTime {
            detail: "y".into(),
            ms: 20_000,
        }];
        let s = summarize(&segs);
        assert_eq!(
            s.rows[0].details,
            vec![
                DetailTime {
                    detail: "y".into(),
                    ms: 25_000
                },
                DetailTime {
                    detail: "x".into(),
                    ms: 15_000
                },
            ]
        );
    }

    #[test]
    fn all_away_gives_zero_share_rows_and_no_nan() {
        let s = summary_of(&idles(0, "A", &[300.0; 20]));
        assert_eq!(s.away_ms, 20_000);
        assert!(s.rows.is_empty());
    }
}
