//! The export: `timewent.report.v3` (PLAN §21.2, §23.3). A compact narrative a person or
//! any LLM can read without a manual: totals, where time went (top 10), and per session the
//! blocks that mattered (≥ 60 s) with the short visits summed up. Local times, whole seconds,
//! plain names; `why` only where it changes the meaning. Pure: the time zone comes in as a name
//! plus an offset function, the clock as `generated_at_ms`.

use std::collections::BTreeMap;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::activity::ACTIVITY_KEY_PREFIX;
use crate::config::Config;
use crate::explain::line;
use crate::lang::Lang;
use crate::range::{Range, SessionMeta};
use crate::sample::Sample;
use crate::segment::{segment, Evidence, Segment, SegmentKind, PASSTHROUGH_KEY_PREFIX};
use crate::summary::{summarize, RowKind, Summary};

pub const REPORT_SCHEMA: &str = "timewent.report.v3";

/// Blocks are at least this long; everything shorter is a short visit (§23.3).
pub const BLOCK_MIN_S: i64 = 60;
/// `where` lists this many rows; the rest is `other_s`.
pub const WHERE_TOP: usize = 10;

/// What a report is built from.
pub struct ReportInput<'a> {
    pub range: &'a Range,
    /// The span the range covers, unix ms: session start → end (or now), local midnight → now,
    /// Monday → now, first → last sample.
    pub span: (i64, i64),
    /// The range's samples, oldest first.
    pub samples: &'a [Sample],
    /// Sessions overlapping the range (any order); an open one has no end.
    pub sessions: &'a [SessionMeta],
    pub config: &'a Config,
    /// Language of the `why` sentences (everything else is English).
    pub lang: Lang,
    /// IANA name, e.g. `Europe/Istanbul`.
    pub timezone: &'a str,
    /// Seconds east of UTC at a given unix ms (DST-aware in the app, fixed in tests).
    pub offset_at: &'a dyn Fn(i64) -> i32,
    pub generated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub about: String,
    pub range: RangeOut,
    pub totals: Totals,
    #[serde(rename = "where")]
    pub where_: Vec<WhereRow>,
    /// Seconds of the rows after the top ten.
    pub other_s: i64,
    pub listening: Vec<ListenOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<SessionOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<Day>>,
    pub rules: Rules,
    pub generated: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RangeOut {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub from: String,
    pub to: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Totals {
    pub in_use_s: i64,
    pub active_s: i64,
    pub reading_s: i64,
    pub away_s: i64,
    pub not_tracked_s: i64,
    pub longest_focus_s: i64,
    pub switches: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WhereRow {
    pub name: String,
    pub kind: &'static str,
    pub category: &'static str,
    pub seconds: i64,
    pub share: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<Ordered>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_details: Option<Vec<NameSeconds>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NameSeconds {
    pub name: String,
    pub seconds: i64,
}

/// A JSON object whose keys keep this order (largest first).
#[derive(Debug, Clone, PartialEq)]
pub struct Ordered(pub Vec<(String, i64)>);

impl Serialize for Ordered {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListenOut {
    pub title: Option<String>,
    pub source: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionOut {
    pub from: String,
    pub to: String,
    pub in_use_s: i64,
    pub blocks: Vec<Block>,
    pub short_visits: ShortVisits,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    pub from: String,
    pub to: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: &'static str,
    pub seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShortVisits {
    pub count: usize,
    pub seconds: i64,
    pub top: Vec<NameSeconds>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Day {
    pub date: String,
    pub in_use_s: i64,
    pub top: Vec<NameSeconds>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rules {
    pub passive_after_s: u32,
    pub away_after_s: u32,
    pub transient_max_s: u32,
    pub glance_max_s: u32,
}

/// Whole seconds, rounded half up (all inputs are non-negative).
fn secs(ms: i64) -> i64 {
    (ms.max(0) + 500) / 1000
}

fn share2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn report(input: &ReportInput) -> Report {
    let config = input.config;
    let segments = segment(input.samples, config);
    let summary = summarize(&segments);
    let offset = |ms: i64| (input.offset_at)(ms);
    let not_tracked_ms: i64 = segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Gap)
        .map(|s| s.ms())
        .sum();
    let multi_day = matches!(input.range, Range::Week | Range::All);

    let (from, to) = input.span;
    let range = RangeOut {
        kind: match input.range {
            Range::Session { .. } => "session",
            Range::Today => "today",
            Range::Week => "week",
            Range::All => "all",
        },
        date: (!multi_day).then(|| local_date(from, offset(from))),
        from: if multi_day {
            local_date(from, offset(from))
        } else {
            local_time(from, offset(from))
        },
        to: if multi_day {
            local_date(to, offset(to))
        } else {
            local_time(to, offset(to))
        },
        timezone: input.timezone.to_string(),
    };

    let (where_, other_s) = where_rows(&summary);
    let sessions = (!multi_day).then(|| sessions(input, &offset));
    let days = multi_day.then(|| days(input.samples, config, &offset));

    Report {
        schema: REPORT_SCHEMA,
        about: about(config),
        range,
        totals: Totals {
            in_use_s: secs(summary.active_ms + summary.passive_ms),
            active_s: secs(summary.active_ms),
            reading_s: secs(summary.passive_ms),
            away_s: secs(summary.away_ms),
            not_tracked_s: secs(not_tracked_ms),
            longest_focus_s: secs(summary.longest_focus_ms),
            switches: summary.switches,
        },
        where_,
        other_s,
        listening: summary
            .listening
            .iter()
            .filter(|l| secs(l.ms) > 0)
            .map(|l| ListenOut {
                title: l.title.clone(),
                source: l.label.clone(),
                seconds: secs(l.ms),
            })
            .collect(),
        sessions,
        days,
        rules: Rules {
            passive_after_s: config.passive_after_s,
            away_after_s: config.away_after_s,
            transient_max_s: config.transient_max_s,
            glance_max_s: config.glance_max_s,
        },
        generated: local_iso(input.generated_at_ms, offset(input.generated_at_ms)),
    }
}

/// ≤ 450 characters: the terms a reader needs, nothing else.
fn about(c: &Config) -> String {
    format!(
        "timewent report: where time went on this Mac, from what was on screen. in_use = \
         active + reading (idle {p}-{a}s or media). away = idle {a}s+ or locked. where = \
         projects (code + its research), your activities, apps, sites; top {WHERE_TOP}, rest in \
         other_s. sessions: blocks of {BLOCK_MIN_S}s+ in order (same name merged), shorter \
         visits summed in short_visits; gaps between sessions were not tracked. listening = \
         audio behind work, not in in_use. Local times, seconds.",
        p = c.passive_after_s,
        a = c.away_after_s,
    )
}

fn where_rows(summary: &Summary) -> (Vec<WhereRow>, i64) {
    let rows: Vec<WhereRow> = summary
        .rows
        .iter()
        .filter(|r| secs(r.ms) > 0)
        .map(|r| {
            let kind = match r.kind {
                RowKind::Project => "project",
                RowKind::Activity => "activity",
                RowKind::Context if r.key.starts_with("web:") => "site",
                RowKind::Context if r.key.starts_with(ACTIVITY_KEY_PREFIX) => "activity",
                RowKind::Context => "app",
            };
            let breakdown = (!r.breakdown.is_empty()).then(|| {
                Ordered(
                    r.breakdown
                        .iter()
                        .filter(|b| secs(b.ms) > 0)
                        .map(|b| (b.label.clone(), secs(b.ms)))
                        .collect(),
                )
            });
            let top: Vec<NameSeconds> = r
                .details
                .iter()
                .filter(|d| secs(d.ms) > 0)
                .take(5)
                .map(|d| NameSeconds {
                    name: d.detail.clone(),
                    seconds: secs(d.ms),
                })
                .collect();
            WhereRow {
                name: r.label.clone(),
                kind,
                category: r.category.as_str(),
                seconds: secs(r.ms),
                share: share2(r.share),
                breakdown,
                top_details: (!top.is_empty()).then_some(top),
            }
        })
        .collect();
    let other_s = rows.iter().skip(WHERE_TOP).map(|r| r.seconds).sum();
    (rows.into_iter().take(WHERE_TOP).collect(), other_s)
}

/// Evidence worth a sentence in the report (§23.3); the rest is mechanics.
fn meaningful(e: &Evidence) -> bool {
    matches!(
        e,
        Evidence::SupportFor { .. }
            | Evidence::ProjectMatch { .. }
            | Evidence::UserActivity { .. }
            | Evidence::MediaPassive { .. }
            | Evidence::PassthroughNotCredited { .. }
    )
}

/// One entry per session overlapping the range, oldest first.
fn sessions(input: &ReportInput, offset: &dyn Fn(i64) -> i32) -> Vec<SessionOut> {
    let (span_from, span_to) = input.span;
    let mut metas: Vec<&SessionMeta> = input.sessions.iter().collect();
    metas.sort_by_key(|m| (m.started_at_ms, m.id));
    metas
        .into_iter()
        .filter_map(|m| {
            let end = m.ended_at_ms.unwrap_or(span_to);
            let from = m.started_at_ms.max(span_from);
            let to = end.min(span_to);
            let samples: Vec<Sample> = input
                .samples
                .iter()
                .filter(|s| s.ts_ms >= m.started_at_ms && m.ended_at_ms.is_none_or(|e| s.ts_ms < e))
                .cloned()
                .collect();
            if samples.is_empty() && to <= from {
                return None;
            }
            let segs = segment(&samples, input.config);
            let summary = summarize(&segs);
            let (blocks, short_visits) = blocks(&segs, input, offset);
            Some(SessionOut {
                from: local_time(from, offset(from)),
                to: local_time(to, offset(to)),
                in_use_s: secs(summary.active_ms + summary.passive_ms),
                blocks,
                short_visits,
            })
        })
        .collect()
}

/// A block before merging: its own seconds, and the detail of its longest part.
struct Draft {
    from_ms: i64,
    to_ms: i64,
    name: String,
    kind: &'static str,
    ms: i64,
    detail: Option<(String, i64)>,
    why: Vec<String>,
}

fn blocks(
    segs: &[Segment],
    input: &ReportInput,
    offset: &dyn Fn(i64) -> i32,
) -> (Vec<Block>, ShortVisits) {
    let block_ms = BLOCK_MIN_S * 1000;
    let away_ms = i64::from(input.config.away_after_s) * 1000;
    let mut drafts: Vec<Draft> = Vec::new();
    let mut short: BTreeMap<String, i64> = BTreeMap::new();
    let mut short_count = 0;
    for s in segs {
        let block_kind = match s.kind {
            SegmentKind::Gap => continue,
            SegmentKind::Away if s.ms() >= away_ms => Some("away"),
            SegmentKind::Away => continue, // short away: in totals.away_s, not a visit
            _ if s.key.starts_with(PASSTHROUGH_KEY_PREFIX) && s.ms() >= block_ms => Some("focus"),
            SegmentKind::Focus if s.ms() >= block_ms => Some("focus"),
            _ => None,
        };
        let Some(kind) = block_kind else {
            short_count += 1;
            *short.entry(s.label.clone()).or_insert(0) += s.ms();
            continue;
        };
        let why: Vec<String> = s
            .evidence
            .iter()
            .filter(|e| meaningful(e))
            .map(|e| line(e, input.lang))
            .collect();
        let detail = s.details.first().map(|d| (d.detail.clone(), s.ms()));
        let draft = Draft {
            from_ms: s.start_ms,
            to_ms: s.end_ms,
            name: s.label.clone(),
            kind,
            ms: s.ms(),
            detail,
            why,
        };
        match drafts.last_mut() {
            Some(prev) if prev.name == draft.name && prev.kind == draft.kind => {
                prev.to_ms = draft.to_ms;
                prev.ms += draft.ms;
                if draft.detail.as_ref().map(|d| d.1) > prev.detail.as_ref().map(|d| d.1) {
                    prev.detail = draft.detail;
                }
                for w in draft.why {
                    if !prev.why.contains(&w) {
                        prev.why.push(w);
                    }
                }
            }
            _ => drafts.push(draft),
        }
    }
    let blocks = drafts
        .into_iter()
        .map(|d| Block {
            from: local_time(d.from_ms, offset(d.from_ms)),
            to: local_time(d.to_ms, offset(d.to_ms)),
            name: d.name,
            detail: d.detail.map(|(name, _)| name),
            kind: d.kind,
            seconds: secs(d.ms),
            why: (!d.why.is_empty()).then_some(d.why),
        })
        .collect();
    let mut top: Vec<NameSeconds> = short
        .iter()
        .map(|(name, ms)| NameSeconds {
            name: name.clone(),
            seconds: secs(*ms),
        })
        .collect();
    top.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.name.cmp(&b.name)));
    top.truncate(5);
    let short_visits = ShortVisits {
        count: short_count,
        seconds: secs(short.values().sum()),
        top,
    };
    (blocks, short_visits)
}

/// Per local date with tracked time, oldest first: in-use and the top three rows that day.
fn days(samples: &[Sample], config: &Config, offset: &dyn Fn(i64) -> i32) -> Vec<Day> {
    let mut by_date: BTreeMap<String, Vec<Sample>> = BTreeMap::new();
    for s in samples {
        by_date
            .entry(local_date(s.ts_ms, offset(s.ts_ms)))
            .or_default()
            .push(s.clone());
    }
    by_date
        .into_iter()
        .filter_map(|(date, day)| {
            let summary = summarize(&segment(&day, config));
            let in_use_s = secs(summary.active_ms + summary.passive_ms);
            (in_use_s > 0).then(|| Day {
                date,
                in_use_s,
                top: summary
                    .rows
                    .iter()
                    .filter(|r| !r.key.starts_with(PASSTHROUGH_KEY_PREFIX) && secs(r.ms) > 0)
                    .take(3)
                    .map(|r| NameSeconds {
                        name: r.label.clone(),
                        seconds: secs(r.ms),
                    })
                    .collect(),
            })
        })
        .collect()
}

// ── local time, pure ───────────────────────────────────────────

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// (y, m, d, h, min, s) of a unix ms at `offset_s` east of UTC.
fn parts(ms: i64, offset_s: i32) -> (i64, u32, u32, u32, u32, u32) {
    let local_s = ms.div_euclid(1000) + i64::from(offset_s);
    let (days, sod) = (local_s.div_euclid(86_400), local_s.rem_euclid(86_400));
    let (y, m, d) = civil(days);
    (
        y,
        m,
        d,
        (sod / 3600) as u32,
        (sod % 3600 / 60) as u32,
        (sod % 60) as u32,
    )
}

pub fn local_date(ms: i64, offset_s: i32) -> String {
    let (y, m, d, ..) = parts(ms, offset_s);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn local_time(ms: i64, offset_s: i32) -> String {
    let (.., h, mi, s) = parts(ms, offset_s);
    format!("{h:02}:{mi:02}:{s:02}")
}

/// ISO-8601 local time with offset: `2026-10-07T00:57:12+03:00`.
pub fn local_iso(ms: i64, offset_s: i32) -> String {
    let sign = if offset_s < 0 { '-' } else { '+' };
    let off = offset_s.unsigned_abs();
    format!(
        "{}T{}{sign}{:02}:{:02}",
        local_date(ms, offset_s),
        local_time(ms, offset_s),
        off / 3600,
        off % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Activity;
    use crate::testkit::spans::*;

    /// 2026-10-06T08:00:00Z = 11:00 in Istanbul.
    const T0: i64 = 1_791_273_600_000;
    const ISTANBUL: i32 = 3 * 3600;

    fn at(spans: &[(Option<Sample>, i64)]) -> Vec<Sample> {
        timeline(spans)
            .into_iter()
            .map(|s| Sample {
                ts_ms: s.ts_ms + T0,
                ..s
            })
            .collect()
    }

    fn one_session(samples: &[Sample]) -> Vec<SessionMeta> {
        vec![SessionMeta {
            id: 1,
            started_at_ms: samples.first().map_or(T0, |s| s.ts_ms),
            ended_at_ms: samples.last().map(|s| s.ts_ms + 1000),
        }]
    }

    fn build(
        range: &Range,
        samples: &[Sample],
        sessions: &[SessionMeta],
        config: &Config,
        lang: Lang,
    ) -> Report {
        let off = |_: i64| ISTANBUL;
        let end = samples.last().map_or(T0, |s| s.ts_ms + 1000);
        report(&ReportInput {
            range,
            span: (samples.first().map_or(T0, |s| s.ts_ms), end),
            samples,
            sessions,
            config,
            lang,
            timezone: "Europe/Istanbul",
            offset_at: &off,
            generated_at_ms: end + 60_000,
        })
    }

    fn me() -> Sample {
        Sample {
            bundle_id: "dev.timewent.app".into(),
            app_name: "timewent".into(),
            window_title: None,
            ..code("x")
        }
    }

    #[test]
    fn local_time_formatting_handles_offsets_and_date_rollover() {
        assert_eq!(local_iso(T0, ISTANBUL), "2026-10-06T11:00:00+03:00");
        assert_eq!(
            local_iso(T0, -(9 * 3600 + 1800)),
            "2026-10-05T22:30:00-09:30"
        );
        assert_eq!(local_date(T0 + 14 * 3_600_000, ISTANBUL), "2026-10-07");
        assert_eq!(local_date(951_782_400_000, 0), "2000-02-29");
    }

    #[test]
    fn seconds_round_half_up_and_shares_have_two_decimals() {
        assert_eq!((secs(499), secs(500), secs(95_000)), (0, 1, 95));
        assert_eq!(share2(0.0169), 0.02);
    }

    #[test]
    fn a_session_is_told_as_blocks_with_the_short_visits_summed() {
        let samples = at(&[
            (Some(code("p")), 120),
            (Some(youtube()), 15),    // short visit
            (Some(code("p")), 90),    // merges with the first block
            (Some(chatgpt("x")), 70), // research for p, between two p blocks
            (Some(code("p")), 30),    // short: under 60s
        ]);
        let r = build(
            &Range::Session { id: 1 },
            &samples,
            &one_session(&samples),
            &Config::default(),
            Lang::En,
        );
        assert_eq!(r.schema, "timewent.report.v3");
        let s = &r.sessions.as_ref().expect("sessions")[0];
        assert_eq!(
            (s.from.as_str(), s.to.as_str(), s.in_use_s),
            ("11:00:00", "11:05:25", 325)
        );
        let blocks: Vec<(&str, &str, &str, i64)> = s
            .blocks
            .iter()
            .map(|b| (b.from.as_str(), b.to.as_str(), b.name.as_str(), b.seconds))
            .collect();
        assert_eq!(
            blocks,
            [
                ("11:00:00", "11:03:45", "p", 210),
                ("11:03:45", "11:04:55", "ChatGPT", 70)
            ],
            "p's two blocks merge across the YouTube visit; seconds are their own, not the span"
        );
        assert_eq!(s.blocks[0].why, None, "plain code needs no explanation");
        assert_eq!(
            s.blocks[1].why.as_deref(),
            Some(
                &[
                    "ChatGPT counted as research for p (between two p blocks, 1m10s apart)"
                        .to_string()
                ][..]
            )
        );
        assert_eq!(
            s.short_visits,
            ShortVisits {
                count: 2,
                seconds: 45,
                top: vec![
                    NameSeconds {
                        name: "p".into(),
                        seconds: 30
                    },
                    NameSeconds {
                        name: "YouTube".into(),
                        seconds: 15
                    },
                ]
            }
        );
    }

    #[test]
    fn mechanics_never_get_a_why_but_a_long_uncredited_look_does() {
        let samples = at(&[
            (Some(code("p")), 60),
            (Some(youtube()), 2), // absorbed transient: no why
            (Some(code("p")), 60),
            (Some(me()), 95), // not credited: why
            (Some(code("q")), 70),
        ]);
        let r = build(
            &Range::Today,
            &samples,
            &one_session(&samples),
            &Config::default(),
            Lang::Tr,
        );
        let s = &r.sessions.expect("sessions")[0];
        let named: Vec<(&str, Option<usize>)> = s
            .blocks
            .iter()
            .map(|b| (b.name.as_str(), b.why.as_ref().map(Vec::len)))
            .collect();
        assert_eq!(named, [("p", None), ("timewent", Some(1)), ("q", None)]);
        assert_eq!(
            s.blocks[1].why.as_deref(),
            Some(&["timewent 1dk 35sn — ara uygulama, hiçbir etkinliğe yazılmadı".to_string()][..])
        );
    }

    #[test]
    fn long_away_is_a_block_short_away_and_gaps_are_not_listed() {
        let samples = at(&[
            (Some(code("p")), 60),
            (Some(away(code("p"))), 200), // locked 200s ≥ 180: a block
            (Some(code("p")), 60),
            (Some(away(code("p"))), 20), // short away: dropped
            (Some(code("p")), 60),
            (None, 900), // a gap: never an entry
            (Some(code("p")), 60),
        ]);
        let r = build(
            &Range::Today,
            &samples,
            &one_session(&samples),
            &Config::default(),
            Lang::En,
        );
        let s = &r.sessions.expect("sessions")[0];
        let kinds: Vec<(&str, &str, i64)> = s
            .blocks
            .iter()
            .map(|b| (b.name.as_str(), b.kind, b.seconds))
            .collect();
        assert_eq!(
            kinds,
            [
                ("p", "focus", 60),
                ("away", "away", 200),
                ("p", "focus", 180)
            ]
        );
        assert_eq!(s.short_visits.count, 0);
        assert_eq!(
            r.totals.not_tracked_s, 900,
            "the gap is a total, not an entry"
        );
    }

    #[test]
    fn a_today_report_has_one_entry_per_session() {
        let mut samples = at(&[(Some(code("p")), 90)]);
        let later: Vec<Sample> = at(&[(Some(youtube()), 120)])
            .into_iter()
            .map(|s| Sample {
                ts_ms: s.ts_ms + 3_600_000,
                ..s
            })
            .collect();
        samples.extend(later.clone());
        let sessions = vec![
            SessionMeta {
                id: 1,
                started_at_ms: T0,
                ended_at_ms: Some(T0 + 90_000),
            },
            SessionMeta {
                id: 2,
                started_at_ms: T0 + 3_600_000,
                ended_at_ms: None,
            },
        ];
        let r = build(
            &Range::Today,
            &samples,
            &sessions,
            &Config::default(),
            Lang::En,
        );
        let s = r.sessions.expect("sessions");
        let got: Vec<(&str, &str, i64)> = s
            .iter()
            .map(|x| (x.from.as_str(), x.to.as_str(), x.in_use_s))
            .collect();
        assert_eq!(
            got,
            [("11:00:00", "11:01:30", 90), ("12:00:00", "12:02:00", 120)]
        );
        assert!(r.days.is_none());
    }

    #[test]
    fn where_lists_the_top_ten_and_sums_the_rest() {
        let mut spans: Vec<(Option<Sample>, i64)> = Vec::new();
        for i in 0..13 {
            spans.push((
                Some(Sample {
                    app_name: format!("App{i:02}"),
                    bundle_id: format!("x.app{i}"),
                    window_title: None,
                    ..code("x")
                }),
                20 + i,
            ));
        }
        let samples = at(&spans);
        let r = build(
            &Range::Today,
            &samples,
            &one_session(&samples),
            &Config::default(),
            Lang::En,
        );
        assert_eq!(r.where_.len(), 10);
        assert_eq!(r.where_[0].name, "App12");
        assert_eq!(r.other_s, 20 + 21 + 22, "App00..App02");
        let json = serde_json::to_value(&r).expect("json");
        assert_eq!(json["other_s"], 63);
    }

    #[test]
    fn no_internal_keys_raw_ms_or_timeline_leak() {
        let samples = at(&[
            (Some(code("p")), 30),
            (Some(me()), 20),
            (Some(away(code("p"))), 10),
            (None, 600),
            (Some(code("p")), 30),
        ]);
        let r = build(
            &Range::Today,
            &samples,
            &one_session(&samples),
            &Config::default(),
            Lang::En,
        );
        let json = serde_json::to_string(&r).expect("json");
        for leak in [
            "pass:",
            "app:",
            "code:",
            "web:",
            "act:",
            "_ms",
            "\"evidence\"",
            "\"config\"",
            "\"timeline\"",
            "not_tracked\"",
            "glance ",
            "absorbed ",
        ] {
            assert!(!json.contains(leak), "{leak} in {json}");
        }
    }

    #[test]
    fn about_stays_short() {
        assert!(about(&Config::default()).chars().count() <= 450);
    }

    #[test]
    fn week_and_all_list_days_instead_of_sessions() {
        let mut samples = at(&[(Some(code("p")), 60)]);
        samples.extend((0..30).map(|i| Sample {
            ts_ms: T0 + 15 * 3_600_000 + 1_800_000 + i * 1000,
            ..youtube()
        }));
        let r = build(&Range::Week, &samples, &[], &Config::default(), Lang::En);
        assert!(r.sessions.is_none());
        let days = r.days.expect("days");
        let got: Vec<(&str, i64)> = days.iter().map(|d| (d.date.as_str(), d.in_use_s)).collect();
        assert_eq!(got, [("2026-10-06", 60), ("2026-10-07", 30)]);
    }

    #[test]
    fn activity_blocks_say_they_are_yours_and_breakdown_is_by_member() {
        let config = Config {
            activities: vec![Activity {
                name: "coding".into(),
                apps: vec!["com.microsoft.VSCode".into(), "com.apple.Terminal".into()],
                domains: vec![],
            }],
            ..Config::default()
        };
        let samples = at(&[(Some(code("p")), 40), (Some(terminal("~/src — zsh")), 30)]);
        let r = build(
            &Range::Session { id: 1 },
            &samples,
            &one_session(&samples),
            &config,
            Lang::En,
        );
        assert_eq!(
            (r.where_[0].name.as_str(), r.where_[0].kind),
            ("coding", "activity")
        );
        assert_eq!(
            r.where_[0].breakdown,
            Some(Ordered(vec![("Code".into(), 40), ("Terminal".into(), 30)]))
        );
        let b = &r.sessions.expect("sessions")[0].blocks[0];
        assert_eq!(b.name, "coding");
        assert_eq!(
            b.why.as_deref(),
            Some(
                &[
                    "Code counted as coding — your activity".to_string(),
                    "Terminal counted as coding — your activity".to_string()
                ][..]
            )
        );
    }
}
