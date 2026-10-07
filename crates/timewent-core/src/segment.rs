//! Samples → explainable time segments: `segment` (DESIGN §6).

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::activity::{prepare, Member};
use crate::attribute::{self, Match, Projects};
use crate::config::Config;
use crate::context::{Category, Context};
use crate::presence::classify_presence;
use crate::run::{absorb_transients, group_runs};
use crate::sample::Sample;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub key: String,
    pub label: String,
    /// `None` for away and gap segments.
    pub category: Option<Category>,
    pub kind: SegmentKind,
    pub active_ms: i64,
    pub passive_ms: i64,
    /// Files / pages of the host context, sorted by `ms` desc then `detail` asc.
    pub details: Vec<DetailTime>,
    /// Absorbed transient runs, chronological.
    pub interruptions: Vec<Interruption>,
    pub evidence: Vec<Evidence>,
    /// The project this time was for (DESIGN §7.1); `None` = its own context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Activity segments: time per member app / site, ms desc (DESIGN §7.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MemberTime>,
    /// Background audio during this segment (DESIGN §8.4), ms desc. Never part of in-use time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listening: Vec<ListenTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenTime {
    /// The source: a site label (`YouTube`) or the app (`Spotify`).
    pub label: String,
    pub title: Option<String>,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberTime {
    /// Bundle id or domain.
    pub key: String,
    pub label: String,
    pub ms: i64,
    /// The member's own category (picks the activity row's category).
    #[serde(skip)]
    pub category: Option<Category>,
}

impl Segment {
    /// The summary row this segment rolls into: its project's code key, else its own key.
    pub fn row_key(&self) -> String {
        match &self.project {
            Some(p) => format!("{CODE_KEY_PREFIX}{p}"),
            None => self.key.clone(),
        }
    }

    pub fn ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    Focus,
    Glance,
    Away,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailTime {
    pub detail: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interruption {
    pub key: String,
    pub label: String,
    pub start_ms: i64,
    pub ms: i64,
}

/// Why a segment looks the way it does. Self-contained: carries the thresholds that applied,
/// so `explain` needs no config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evidence {
    AbsorbedTransient {
        label: String,
        ms: i64,
        threshold_s: i64,
    },
    /// A run of a passthrough app (`Config::passthrough_bundle_ids`), absorbed whatever its
    /// length.
    AbsorbedPassthrough {
        label: String,
        ms: i64,
    },
    Glance {
        ms: i64,
        threshold_s: i64,
    },
    /// A passthrough run with no host beside it (block edge, away, gap).
    PassthroughGlance {
        ms: i64,
    },
    /// A pass-through run of glance length or more: in use, but credited to no activity
    /// (DESIGN §6.4).
    PassthroughNotCredited {
        label: String,
        ms: i64,
    },
    IdleAway {
        idle_run_s: i64,
        threshold_s: i64,
    },
    Locked,
    PassiveTime {
        ms: i64,
        threshold_s: i64,
    },
    /// Passive time while media/a call held the display awake: kept passive, never away.
    MediaPassive {
        ms: i64,
    },
    Gap {
        ms: i64,
    },
    /// Directly tied to a project: a repo URL, a window title, or localhost beside code.
    ProjectMatch {
        project: String,
        via: String,
    },
    /// AI / docs time between two blocks of one project, `window_s` apart.
    SupportFor {
        project: String,
        label: String,
        window_s: i64,
    },
    /// A member of an activity you defined.
    UserActivity {
        activity: String,
        member: String,
    },
}

pub(crate) const AWAY_KEY: &str = "away";
/// Context keys of code-editor projects: `code:{project}`.
pub const CODE_KEY_PREFIX: &str = "code:";
/// Key prefix of a passthrough run that had no host: `pass:{bundle_id}`. It tiles the
/// timeline and counts as in-use time, but `summarize` never makes it a row.
pub const PASSTHROUGH_KEY_PREFIX: &str = "pass:";
const GAP_KEY: &str = "gap";

/// Deterministic segmentation of samples sorted by `ts_ms`.
///
/// Each sample stands for the time until the next sample, or `poll_interval_ms` when it is
/// the last one before a gap or the end. Segments therefore tile the tracked time exactly.
pub fn segment(samples: &[Sample], config: &Config) -> Vec<Segment> {
    let (contexts, members): (Vec<Context>, Vec<Option<Member>>) =
        samples.iter().map(|s| prepare(s, config)).unzip();
    let projects = Projects::known(samples, &contexts, &members);
    let matches: Vec<Match> = samples
        .iter()
        .zip(&contexts)
        .zip(&members)
        .map(|((s, c), m)| projects.match_of(s, c, m.as_ref(), config))
        .collect();
    segment_prepared(samples, &contexts, &members, &matches, &projects, config)
}

/// `segment` for a stream that only grows (a live session): the per-sample work — context
/// derivation and project matching — is done once per sample. `segments()` equals
/// `segment(samples(), config)` exactly (same code path; property-tested).
#[derive(Debug, Clone)]
pub struct Segmenter {
    config: Config,
    samples: Vec<Sample>,
    contexts: Vec<Context>,
    members: Vec<Option<Member>>,
    matches: Vec<Match>,
    projects: Projects,
}

impl Segmenter {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            samples: Vec::new(),
            contexts: Vec::new(),
            members: Vec::new(),
            matches: Vec::new(),
            projects: Projects::default(),
        }
    }

    /// Appends a sample (`ts_ms` not before the last one).
    pub fn push(&mut self, sample: Sample) {
        let (ctx, member) = prepare(&sample, &self.config);
        let new_project = self.projects.learn(&sample, &ctx, member.as_ref());
        self.samples.push(sample);
        self.contexts.push(ctx);
        self.members.push(member);
        if new_project {
            // Rare (a project seen for the first time): earlier titles may now match it.
            self.matches = self
                .samples
                .iter()
                .zip(&self.contexts)
                .zip(&self.members)
                .map(|((s, c), m)| self.projects.match_of(s, c, m.as_ref(), &self.config))
                .collect();
        } else if let (Some(s), Some(c), Some(m)) = (
            self.samples.last(),
            self.contexts.last(),
            self.members.last(),
        ) {
            self.matches
                .push(self.projects.match_of(s, c, m.as_ref(), &self.config));
        }
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn segments(&self) -> Vec<Segment> {
        segment_prepared(
            &self.samples,
            &self.contexts,
            &self.members,
            &self.matches,
            &self.projects,
            &self.config,
        )
    }
}

fn segment_prepared(
    samples: &[Sample],
    contexts: &[Context],
    members: &[Option<Member>],
    matches: &[Match],
    projects: &Projects,
    config: &Config,
) -> Vec<Segment> {
    let presence = classify_presence(samples, config);
    let mut out = Vec::new();
    let mut block_start = 0;
    for i in 1..=samples.len() {
        let block_ends =
            i == samples.len() || config.is_gap(samples[i - 1].ts_ms, samples[i].ts_ms);
        if !block_ends {
            continue;
        }
        let block = block_start..i;
        let runs = group_runs(
            &samples[block.clone()],
            &contexts[block.clone()],
            &members[block.clone()],
            &matches[block.clone()],
            &presence[block],
            projects,
            config,
        );
        out.extend(
            absorb_transients(runs, config)
                .into_iter()
                .map(|r| r.finish(config)),
        );
        if i < samples.len() {
            out.extend(gap_segment(
                samples[i - 1].ts_ms + config.poll_ms(),
                samples[i].ts_ms,
            ));
        }
        block_start = i;
    }
    attribute::resolve(&mut out, config);
    crate::listening::assign(samples, config, &mut out);
    out
}

/// Missing time between two blocks. Nothing to emit if the poll interval already covers it.
fn gap_segment(start_ms: i64, end_ms: i64) -> Option<Segment> {
    (end_ms > start_ms).then(|| Segment {
        start_ms,
        end_ms,
        key: GAP_KEY.into(),
        label: GAP_KEY.into(),
        category: None,
        kind: SegmentKind::Gap,
        active_ms: 0,
        passive_ms: 0,
        details: Vec::new(),
        interruptions: Vec::new(),
        evidence: vec![Evidence::Gap {
            ms: end_ms - start_ms,
        }],
        project: None,
        members: Vec::new(),
        listening: Vec::new(),
    })
}

/// Most time first; ties alphabetical so output is deterministic.
pub(crate) fn sorted_details(per_detail: BTreeMap<String, i64>) -> Vec<DetailTime> {
    let mut details: Vec<DetailTime> = per_detail
        .into_iter()
        .map(|(detail, ms)| DetailTime { detail, ms })
        .collect();
    details.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.detail.cmp(&b.detail)));
    details
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{idles, sample, seq};

    fn cfg() -> Config {
        Config::default()
    }

    /// `(key, kind, start_ms, end_ms)` per segment: the shape of a timeline.
    /// Shadows `super::segment` so every test also checks the timeline invariant.
    fn segment(samples: &[Sample], config: &Config) -> Vec<Segment> {
        let segs = super::segment(samples, config);
        assert_chronological_tiling(&segs);
        segs
    }

    /// Segments are non-empty, sorted, and each starts exactly where the previous ended.
    fn assert_chronological_tiling(segs: &[Segment]) {
        for s in segs {
            assert!(s.start_ms < s.end_ms, "empty or inverted segment: {s:?}");
        }
        for pair in segs.windows(2) {
            assert_eq!(
                pair[0].end_ms, pair[1].start_ms,
                "hole or overlap between {:?} and {:?}",
                pair[0].key, pair[1].key
            );
        }
    }

    /// `n` locked samples at the login window, 1s apart.
    fn locked(start_ms: i64, n: usize) -> Vec<Sample> {
        let mut out = seq(start_ms, &[("loginwindow", n)]);
        for s in &mut out {
            s.locked = true;
        }
        out
    }

    fn shape(segments: &[Segment]) -> Vec<(&str, SegmentKind, i64, i64)> {
        segments
            .iter()
            .map(|s| (s.key.as_str(), s.kind, s.start_ms, s.end_ms))
            .collect()
    }

    fn editor(ts_ms: i64, file: &str) -> Sample {
        Sample {
            bundle_id: "com.microsoft.VSCode".into(),
            app_name: "Code".into(),
            window_title: Some(format!("{file} — proj")),
            ..sample(ts_ms, "Code", 0.0)
        }
    }

    fn absorbed(label: &str, ms: i64) -> Evidence {
        Evidence::AbsorbedTransient {
            label: label.into(),
            ms,
            threshold_s: 3,
        }
    }

    use SegmentKind::*;

    #[test]
    fn empty_input_gives_no_segments() {
        assert!(segment(&[], &cfg()).is_empty());
    }

    #[test]
    fn consecutive_contexts_become_focus_segments() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 15)]), &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("app:B", Focus, 20_000, 35_000)
            ]
        );
        assert_eq!(segs[0].label, "A");
        assert_eq!(segs[0].category, Some(Category::App));
        assert_eq!(segs[0].active_ms, 20_000);
        assert_eq!(segs[0].passive_ms, 0);
        assert!(segs[0].evidence.is_empty());
        assert!(segs[0].interruptions.is_empty());
    }

    #[test]
    fn end_ms_is_last_sample_ts_plus_poll_interval() {
        let segs = segment(&seq(1_000_000, &[("A", 12)]), &cfg());
        assert_eq!(segs[0].end_ms, 1_011_000 + 1000);

        let config = Config {
            poll_interval_ms: 2000,
            ..cfg()
        };
        let samples: Vec<Sample> = (0..10).map(|i| sample(i * 2000, "A", 0.0)).collect();
        let segs = segment(&samples, &config);
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 18_000 + 2000)]);
        assert_eq!(segs[0].active_ms, 20_000);
    }

    #[test]
    fn transient_between_same_context_merges_into_one_segment() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 2), ("A", 20)]), &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 42_000)]);
        assert_eq!(
            segs[0].interruptions,
            vec![Interruption {
                key: "app:B".into(),
                label: "B".into(),
                start_ms: 20_000,
                ms: 2_000
            }]
        );
        assert_eq!(segs[0].evidence, vec![absorbed("B", 2_000)]);
    }

    #[test]
    fn absorbed_transient_time_counts_toward_host() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 2), ("A", 20)]), &cfg());
        assert_eq!(segs[0].ms(), 42_000);
        assert_eq!(segs[0].active_ms, 42_000);
    }

    #[test]
    fn transient_between_different_contexts_attaches_to_previous() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 2), ("C", 20)]), &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 22_000),
                ("app:C", Focus, 22_000, 42_000)
            ]
        );
        assert_eq!(segs[0].interruptions[0].key, "app:B");
        assert_eq!(segs[0].evidence, vec![absorbed("B", 2_000)]);
        assert_eq!(segs[0].active_ms, 22_000);
    }

    #[test]
    fn transient_first_run_attaches_to_next() {
        let segs = segment(&seq(0, &[("B", 2), ("A", 20)]), &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 22_000)]);
        assert_eq!(segs[0].interruptions[0].start_ms, 0);
        assert_eq!(segs[0].evidence, vec![absorbed("B", 2_000)]);
    }

    #[test]
    fn consecutive_transients_are_all_absorbed() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 1), ("C", 1), ("A", 20)]), &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 42_000)]);
        let keys: Vec<&str> = segs[0]
            .interruptions
            .iter()
            .map(|i| i.key.as_str())
            .collect();
        assert_eq!(keys, vec!["app:B", "app:C"]);
    }

    #[test]
    fn glance_between_same_context_stays_separate() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 3), ("A", 20)]), &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("app:B", Glance, 20_000, 23_000),
                ("app:A", Focus, 23_000, 43_000),
            ]
        );
    }

    #[test]
    fn glance_segment_carries_glance_evidence() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 9), ("C", 20)]), &cfg());
        assert_eq!(
            segs[1].evidence,
            vec![Evidence::Glance {
                ms: 9_000,
                threshold_s: 10
            }]
        );
        assert!(segs[0].evidence.is_empty());
    }

    #[test]
    fn lone_transient_glance_also_carries_glance_evidence() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            segs[2].evidence,
            vec![Evidence::Glance {
                ms: 2_000,
                threshold_s: 10
            }]
        );
    }

    #[test]
    fn run_just_under_glance_max_is_glance_and_at_glance_max_is_focus() {
        let segs = segment(&seq(0, &[("A", 20), ("B", 9), ("C", 10)]), &cfg());
        assert_eq!(segs[1].kind, Glance);
        assert_eq!(segs[2].kind, Focus);
    }

    #[test]
    fn transient_alone_with_no_host_stays_as_glance() {
        let segs = segment(&seq(0, &[("B", 2)]), &cfg());
        assert_eq!(shape(&segs), vec![("app:B", Glance, 0, 2_000)]);
    }

    #[test]
    fn away_run_becomes_away_segment_with_idle_evidence() {
        let mut samples = seq(0, &[("A", 10)]);
        samples.extend(idles(10_000, "A", &[180.0, 190.0, 200.0, 212.7]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![("app:A", Focus, 0, 10_000), ("away", Away, 10_000, 14_000)]
        );
        let away = &segs[1];
        assert_eq!(away.label, "away");
        assert_eq!(away.category, None);
        assert_eq!((away.active_ms, away.passive_ms), (0, 0));
        assert_eq!(
            away.evidence,
            vec![Evidence::IdleAway {
                idle_run_s: 212,
                threshold_s: 180
            }]
        );
    }

    #[test]
    fn reading_that_ends_in_walking_off_is_away_from_the_grace_on() {
        // 44s of active, then idle crosses 45 and keeps growing past 180.
        let idle: Vec<f64> = (0..200).map(f64::from).collect();
        let segs = segment(&idles(0, "A", &idle), &cfg());
        assert_eq!(
            shape(&segs),
            vec![("app:A", Focus, 0, 45_000), ("away", Away, 45_000, 200_000)]
        );
        assert_eq!(segs[0].passive_ms, 0);
    }

    #[test]
    fn locked_run_has_locked_evidence() {
        let mut samples = seq(0, &[("A", 10), ("loginwindow", 4)]);
        for s in &mut samples[10..] {
            s.locked = true;
        }
        let segs = segment(&samples, &cfg());
        assert_eq!(segs[1].kind, Away);
        assert_eq!(segs[1].evidence, vec![Evidence::Locked]);
    }

    #[test]
    fn reading_then_locking_reports_lock_not_idle_threshold() {
        let mut idle = vec![0.0; 10];
        idle.extend([50.0, 51.0, 52.0]);
        let mut samples = idles(0, "A", &idle);
        samples[12].locked = true;
        let segs = segment(&samples, &cfg());
        assert_eq!(shape(&segs)[1], ("away", Away, 10_000, 13_000));
        assert_eq!(segs[1].evidence, vec![Evidence::Locked]);
    }

    #[test]
    fn timestamp_jump_over_gap_after_emits_gap_segment() {
        let mut samples = seq(0, &[("A", 12)]);
        samples.extend(seq(60_000, &[("A", 12)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 12_000),
                ("gap", Gap, 12_000, 60_000),
                ("app:A", Focus, 60_000, 72_000),
            ]
        );
        let gap = &segs[1];
        assert_eq!(gap.category, None);
        assert_eq!((gap.active_ms, gap.passive_ms), (0, 0));
        assert_eq!(gap.evidence, vec![Evidence::Gap { ms: 48_000 }]);
    }

    #[test]
    fn jump_within_gap_after_is_attributed_to_the_run() {
        let mut samples = seq(0, &[("A", 3)]);
        samples.extend(seq(7_000, &[("A", 3)])); // 5s jump: not a gap
        let segs = segment(&samples, &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 10_000)]);
        assert_eq!(segs[0].active_ms, 10_000);
    }

    #[test]
    fn transient_is_not_absorbed_across_a_gap() {
        let mut samples = seq(0, &[("A", 20), ("B", 2)]);
        samples.extend(seq(100_000, &[("C", 2), ("A", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 22_000),
                ("gap", Gap, 22_000, 100_000),
                ("app:A", Focus, 100_000, 122_000),
            ]
        );
        assert_eq!(segs[0].interruptions[0].key, "app:B");
        assert_eq!(segs[2].interruptions[0].key, "app:C");
    }

    #[test]
    fn segments_tile_time_without_holes_when_sampling_jitters() {
        let ts = [0, 1_010, 1_990, 3_005, 4_020, 4_990, 6_000, 7_015, 8_000];
        let samples: Vec<Sample> = ts
            .iter()
            .enumerate()
            .map(|(i, t)| sample(*t, if i < 4 { "A" } else { "B" }, 0.0))
            .collect();
        let segs = segment(&samples, &cfg());
        assert_eq!(segs[0].end_ms, segs[1].start_ms);
        assert_eq!(segs[1].end_ms, 8_000 + 1000);
        let total: i64 = segs.iter().map(|s| s.active_ms + s.passive_ms).sum();
        assert_eq!(total, 9_000);
    }

    #[test]
    fn details_are_aggregated_per_file_sorted_by_time_desc() {
        let mut samples = Vec::new();
        let files = [("a.rs", 5), ("b.rs", 8), ("a.rs", 4), ("c.rs", 8)];
        let mut ts = 0;
        for (file, n) in files {
            for _ in 0..n {
                samples.push(editor(ts, file));
                ts += 1000;
            }
        }
        let segs = segment(&samples, &cfg());
        assert_eq!(segs.len(), 1);
        let details: Vec<(&str, i64)> = segs[0]
            .details
            .iter()
            .map(|d| (d.detail.as_str(), d.ms))
            .collect();
        assert_eq!(
            details,
            vec![("a.rs", 9_000), ("b.rs", 8_000), ("c.rs", 8_000)]
        );
    }

    #[test]
    fn absorbed_transient_details_do_not_leak_into_host_details() {
        let mut samples: Vec<Sample> = (0..10).map(|i| editor(i * 1000, "a.rs")).collect();
        let mut yt = sample(10_000, "Google Chrome", 0.0);
        yt.bundle_id = "com.google.Chrome".into();
        yt.url = Some("https://youtube.com/watch".into());
        yt.window_title = Some("video".into());
        samples.push(yt);
        samples.extend((11..20).map(|i| editor(i * 1000, "a.rs")));
        let segs = segment(&samples, &cfg());
        assert_eq!(segs.len(), 1);
        assert_eq!(
            segs[0].details,
            vec![DetailTime {
                detail: "a.rs".into(),
                ms: 19_000
            }]
        );
        assert_eq!(segs[0].interruptions[0].label, "YouTube");
    }

    #[test]
    fn active_and_passive_ms_split_the_segment_duration() {
        let mut idle = vec![0.0; 10];
        idle.extend((45..65).map(f64::from)); // 20s reading
        idle.extend([0.0; 5]);
        let segs = segment(&idles(0, "A", &idle), &cfg());
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].active_ms, 15_000);
        assert_eq!(segs[0].passive_ms, 20_000);
        assert_eq!(segs[0].active_ms + segs[0].passive_ms, segs[0].ms());
        assert_eq!(
            segs[0].evidence,
            vec![Evidence::PassiveTime {
                ms: 20_000,
                threshold_s: 45
            }]
        );
    }

    #[test]
    fn transient_right_after_away_joins_next_host_not_away() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2), ("C", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("away", Away, 20_000, 30_000),
                ("app:C", Focus, 30_000, 52_000),
            ]
        );
        assert!(segs[1].interruptions.is_empty());
        assert_eq!(segs[2].interruptions[0].key, "app:B");
        assert_eq!(segs[2].evidence, vec![absorbed("B", 2_000)]);
    }

    #[test]
    fn transient_between_away_and_block_end_kept_as_glance() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("away", Away, 20_000, 30_000),
                ("app:B", Glance, 30_000, 32_000),
            ]
        );
    }

    #[test]
    fn transient_between_away_and_gap_kept_as_glance() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2)]));
        samples.extend(seq(100_000, &[("C", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("away", Away, 20_000, 30_000),
                ("app:B", Glance, 30_000, 32_000),
                ("gap", Gap, 32_000, 100_000),
                ("app:C", Focus, 100_000, 120_000),
            ]
        );
    }

    #[test]
    fn transient_between_two_away_runs_kept_as_glance() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2)]));
        samples.extend(locked(32_000, 10));
        samples.extend(seq(42_000, &[("C", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("away", Away, 20_000, 30_000),
                ("app:B", Glance, 30_000, 32_000),
                ("away", Away, 32_000, 42_000),
                ("app:C", Focus, 42_000, 62_000),
            ]
        );
    }

    #[test]
    fn away_never_gains_active_ms_from_absorption() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(seq(30_000, &[("B", 2)]));
        samples.extend(locked(32_000, 10));
        samples.extend(seq(42_000, &[("D", 1), ("C", 20)]));
        let segs = segment(&samples, &cfg());
        for away in segs.iter().filter(|s| s.kind == Away) {
            assert_eq!((away.active_ms, away.passive_ms), (0, 0));
            assert!(away.interruptions.is_empty());
        }
        let summary = crate::summary::summarize(&segs);
        assert_eq!(summary.away_ms, 20_000, "only the 20 locked samples");
        assert_eq!(summary.active_ms, 20_000 + 2_000 + 1_000 + 20_000);
        assert_eq!(summary.total_ms, 63_000);
    }

    /// `n` samples with timewent itself frontmost (a default passthrough app), 1s apart.
    fn me(start_ms: i64, n: usize) -> Vec<Sample> {
        (0..n)
            .map(|i| Sample {
                bundle_id: "dev.timewent.app".into(),
                window_title: Some("timewent — see where your time went".into()),
                ..sample(start_ms + 1000 * i as i64, "timewent", 0.0)
            })
            .collect()
    }

    fn absorbed_passthrough(ms: i64) -> Evidence {
        Evidence::AbsorbedPassthrough {
            label: "timewent".into(),
            ms,
        }
    }

    #[test]
    fn launcher_and_system_ui_are_passthrough_by_default() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend((0..8).map(|i| Sample {
            bundle_id: "com.raycast.macos".into(),
            ..sample(20_000 + i * 1000, "Raycast", 0.0)
        }));
        samples.extend(seq(28_000, &[("B", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 28_000),
                ("app:B", Focus, 28_000, 48_000)
            ]
        );
        assert_eq!(
            segs[0].evidence,
            vec![Evidence::AbsorbedPassthrough {
                label: "Raycast".into(),
                ms: 8_000
            }]
        );
    }

    #[test]
    fn short_passthrough_between_same_context_merges() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(me(20_000, 9));
        samples.extend(seq(29_000, &[("A", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 49_000)]);
        assert_eq!(segs[0].active_ms, 49_000);
        assert_eq!(
            segs[0].interruptions,
            vec![Interruption {
                key: "pass:dev.timewent.app".into(),
                label: "timewent".into(),
                start_ms: 20_000,
                ms: 9_000
            }]
        );
        assert_eq!(segs[0].evidence, vec![absorbed_passthrough(9_000)]);
        let summary = crate::summary::summarize(&segs);
        assert!(summary.rows.iter().all(|r| r.label != "timewent"));
    }

    fn not_credited(ms: i64) -> Evidence {
        Evidence::PassthroughNotCredited {
            label: "timewent".into(),
            ms,
        }
    }

    #[test]
    fn passthrough_of_glance_length_or_more_is_never_credited_to_a_neighbour() {
        // DESIGN §6.4, from a real export: 95s of timewent was credited to PDFgear.
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(me(20_000, 10));
        samples.extend(seq(30_000, &[("B", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("pass:dev.timewent.app", Glance, 20_000, 30_000),
                ("app:B", Focus, 30_000, 50_000),
            ]
        );
        assert_eq!(segs[1].evidence, vec![not_credited(10_000)]);
        assert!(segs[0].evidence.is_empty() && segs[2].evidence.is_empty());
        let summary = crate::summary::summarize(&segs);
        assert_eq!(summary.total_ms, 50_000, "still in use");
        let rows: Vec<(&str, i64)> = summary
            .rows
            .iter()
            .map(|r| (r.label.as_str(), r.ms))
            .collect();
        assert_eq!(rows, [("A", 20_000), ("B", 20_000)], "never a row");
    }

    #[test]
    fn long_passthrough_between_one_context_does_not_merge_it() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(me(20_000, 60));
        samples.extend(seq(80_000, &[("A", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("pass:dev.timewent.app", Glance, 20_000, 80_000),
                ("app:A", Focus, 80_000, 100_000),
            ]
        );
    }

    #[test]
    fn short_passthrough_at_block_start_joins_the_next_host() {
        // Pressing start inside timewent: the first seconds belong to what comes next.
        let mut samples = me(0, 5);
        samples.extend(seq(5_000, &[("A", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 25_000)]);
        assert_eq!(segs[0].evidence, vec![absorbed_passthrough(5_000)]);
    }

    #[test]
    fn a_long_passthrough_run_hosts_nothing() {
        // A 2s transient right after 30s of timewent waits for the next real host.
        let mut samples = me(0, 30);
        samples.extend(seq(30_000, &[("B", 2), ("C", 20)]));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("pass:dev.timewent.app", Glance, 0, 30_000),
                ("app:C", Focus, 30_000, 52_000),
            ]
        );
        assert_eq!(segs[1].interruptions[0].key, "app:B");
    }

    #[test]
    fn the_user_case_settings_then_95s_of_timewent_then_pdfgear() {
        let settings = |start: i64, n: usize| -> Vec<Sample> {
            (0..n)
                .map(|i| Sample {
                    bundle_id: "com.apple.systempreferences".into(),
                    ..sample(start + 1000 * i as i64, "Sistem Ayarları", 0.0)
                })
                .collect()
        };
        let pdfgear = |start: i64, n: usize| -> Vec<Sample> {
            (0..n)
                .map(|i| Sample {
                    bundle_id: "com.pdfgear.mac".into(),
                    ..sample(start + 1000 * i as i64, "PDFgear", 0.0)
                })
                .collect()
        };
        let mut samples = settings(0, 40);
        samples.extend(me(40_000, 95));
        samples.extend(pdfgear(135_000, 30));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:Sistem Ayarları", Focus, 0, 40_000),
                ("pass:dev.timewent.app", Glance, 40_000, 135_000),
                ("app:PDFgear", Focus, 135_000, 165_000),
            ]
        );
        let summary = crate::summary::summarize(&segs);
        let pdf = summary
            .rows
            .iter()
            .find(|r| r.label == "PDFgear")
            .expect("row");
        assert_eq!(pdf.ms, 30_000, "PDFgear is not credited the 95s");
        assert_eq!(
            summary.not_shown,
            [crate::summary::NotShown {
                label: "timewent".into(),
                ms: 95_000
            }]
        );
    }

    #[test]
    fn passthrough_run_details_do_not_leak_into_the_host() {
        let mut samples = vec![editor(0, "a.rs")];
        samples.extend((1..20).map(|i| editor(i * 1000, "a.rs")));
        samples.extend(me(20_000, 5));
        let segs = segment(&samples, &cfg());
        assert_eq!(segs.len(), 1);
        let details: Vec<&str> = segs[0].details.iter().map(|d| d.detail.as_str()).collect();
        assert_eq!(details, vec!["a.rs"]);
    }

    #[test]
    fn passthrough_run_never_joins_away() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(locked(20_000, 10));
        samples.extend(me(30_000, 8));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("away", Away, 20_000, 30_000),
                ("pass:dev.timewent.app", Glance, 30_000, 38_000),
            ]
        );
        assert_eq!(
            segs[2].evidence,
            vec![Evidence::PassthroughGlance { ms: 8_000 }]
        );
    }

    #[test]
    fn passthrough_run_is_not_absorbed_across_a_gap() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(me(100_000, 15));
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("gap", Gap, 20_000, 100_000),
                ("pass:dev.timewent.app", Glance, 100_000, 115_000),
            ]
        );
    }

    #[test]
    fn count_self_makes_timewent_an_ordinary_context() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend(me(20_000, 12));
        let config = Config {
            count_self: true,
            ..cfg()
        };
        let segs = segment(&samples, &config);
        assert_eq!(
            shape(&segs),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("app:timewent", Focus, 20_000, 32_000)
            ]
        );
    }

    #[test]
    fn count_self_leaves_other_passthrough_apps_alone() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend((0..8).map(|i| Sample {
            bundle_id: "com.apple.Spotlight".into(),
            ..sample(20_000 + i * 1000, "Spotlight", 0.0)
        }));
        let config = Config {
            count_self: true,
            ..cfg()
        };
        assert_eq!(
            shape(&segment(&samples, &config)),
            vec![("app:A", Focus, 0, 28_000)]
        );
    }

    #[test]
    fn passthrough_bundle_ids_are_configurable() {
        let mut samples = seq(0, &[("A", 20)]);
        samples.extend((0..12).map(|i| Sample {
            bundle_id: "com.apple.Spotlight".into(),
            ..sample(20_000 + i * 1000, "Spotlight", 0.0)
        }));
        let config = Config {
            passthrough_bundle_ids: vec![],
            ..cfg()
        };
        assert_eq!(
            shape(&segment(&samples, &config)),
            vec![
                ("app:A", Focus, 0, 20_000),
                ("app:Spotlight", Focus, 20_000, 32_000)
            ]
        );
    }

    #[test]
    fn session_of_only_timewent_has_no_rows_but_counts_as_in_use() {
        let samples = me(0, 30);
        let segs = segment(&samples, &cfg());
        assert_eq!(
            shape(&segs),
            vec![("pass:dev.timewent.app", Glance, 0, 30_000)]
        );
        let summary = crate::summary::summarize(&segs);
        assert!(summary.rows.is_empty(), "{:?}", summary.rows);
        assert_eq!((summary.total_ms, summary.active_ms), (30_000, 30_000));

        // Same raw samples, recomputed with the toggle on: now it is a row.
        let config = Config {
            count_self: true,
            ..cfg()
        };
        let summary = crate::summary::summarize(&segment(&samples, &config));
        let rows: Vec<(&str, i64)> = summary
            .rows
            .iter()
            .map(|r| (r.label.as_str(), r.ms))
            .collect();
        assert_eq!(rows, vec![("timewent", 30_000)]);
    }

    #[test]
    fn watching_with_no_input_is_passive_with_media_evidence() {
        let mut samples = seq(0, &[("A", 20)]);
        // 300s on A with no input while a video plays; idle climbs from 1s to 300s.
        samples.extend((0..300).map(|i| Sample {
            media_active: true,
            ..sample(20_000 + i * 1000, "A", (i + 1) as f64)
        }));
        let segs = segment(&samples, &cfg());
        assert_eq!(shape(&segs), vec![("app:A", Focus, 0, 320_000)]);
        // idle < 45s is still active: 20s typing + 44 quiet seconds.
        assert_eq!(segs[0].active_ms, 64_000);
        assert_eq!(segs[0].passive_ms, 256_000);
        assert_eq!(
            segs[0].evidence,
            vec![
                Evidence::PassiveTime {
                    ms: 256_000,
                    threshold_s: 45
                },
                Evidence::MediaPassive { ms: 256_000 },
            ]
        );
        assert_eq!(crate::summary::summarize(&segs).away_ms, 0);
    }

    #[test]
    fn evidence_serializes_with_snake_case_type_tag() {
        let json = serde_json::to_string(&absorbed("B", 1)).expect("serialize");
        assert_eq!(
            json,
            r#"{"type":"absorbed_transient","label":"B","ms":1,"threshold_s":3}"#
        );
        let json = serde_json::to_string(&absorbed_passthrough(5)).expect("serialize");
        assert_eq!(
            json,
            r#"{"type":"absorbed_passthrough","label":"timewent","ms":5}"#
        );
        let json = serde_json::to_string(&Evidence::Locked).expect("serialize");
        assert_eq!(json, r#"{"type":"locked"}"#);
        assert_eq!(
            serde_json::to_string(&Glance).expect("serialize"),
            r#""glance""#
        );
    }
}
