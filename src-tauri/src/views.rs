//! Core results → IPC shapes (PLAN §6). Pure: samples and config in, DTOs out — no store,
//! no clock, no locks, so every view the ui shows is unit-tested here.

use timewent_core::{
    activity_member, classify_presence, derive_context, explain, one_liner, segment, summarize,
    Config, Lang, Presence, Range, Sample, Segment, SegmentKind, SessionMeta, ACTIVITY_KEY_PREFIX,
};
use timewent_probe::Permissions;

use crate::dto::{Current, InterruptionDto, PermissionsDto, SegmentDto, Status, View};

/// The pill's "now" for a session's samples (oldest first), derived from the same
/// segments as the view so its numbers always agree with it.
///
/// - context: the last sample that is neither a passthrough app nor away — while you look
///   at timewent or the screen is locked, it still names what you were doing;
/// - presence / `since_ms`: of the latest sample, and where that stretch began (a gap ends
///   a stretch);
/// - `context_ms`: that context's row in the session summary.
pub fn build_current(samples: &[Sample], config: &Config) -> Option<Current> {
    build_current_from(samples, &segment(samples, config), config)
}

/// [`build_current`] with the samples' segments already at hand (the live `Segmenter`).
pub fn build_current_from(
    samples: &[Sample],
    segments: &[Segment],
    config: &Config,
) -> Option<Current> {
    let last = samples.len().checked_sub(1)?;
    let presence = classify_presence(samples, config);
    let real = |i: &usize| !config.is_passthrough(&samples[*i].bundle_id);
    let ctx_at = (0..=last)
        .rev()
        .filter(real)
        .find(|&i| presence[i] != Presence::Away)
        .or_else(|| (0..=last).rev().find(real))?;
    let ctx = derive_context(&samples[ctx_at], config);
    // Inside an activity (§13.1) the pill shows the activity and the member app; the
    // grouping key is the activity's.
    let member = activity_member(&samples[ctx_at], config);
    let group_key = member.as_ref().map_or_else(
        || ctx.key.clone(),
        |m| format!("{ACTIVITY_KEY_PREFIX}{}", m.activity),
    );

    let now = presence[last];
    let gap_ms = i64::from(config.gap_after_s) * 1000;
    let since_ms = (now != Presence::Active).then(|| {
        let mut i = last;
        while i > 0 && presence[i - 1] == now && samples[i].ts_ms - samples[i - 1].ts_ms <= gap_ms {
            i -= 1;
        }
        samples[i].ts_ms
    });

    // The segment hosting that sample decides the row: its project's, if it has one. A
    // transient absorbed into another host keeps its own key.
    let ts = samples[ctx_at].ts_ms;
    let host = segments
        .iter()
        .find(|s| s.kind != SegmentKind::Gap && s.start_ms <= ts && ts < s.end_ms)
        .filter(|s| s.key == group_key);
    let key = host.map_or_else(|| group_key.clone(), Segment::row_key);
    let (label, detail, project) = match member {
        Some(m) => {
            let detail = match ctx.detail {
                Some(d) => format!("{} · {d}", ctx.label),
                None => ctx.label,
            };
            (m.label, Some(detail), Some(m.activity))
        }
        None => (ctx.label, ctx.detail, host.and_then(|s| s.project.clone())),
    };
    let context_ms = summarize(segments)
        .rows
        .into_iter()
        .find(|r| r.key == key)
        .map_or(0, |r| r.ms);

    Some(Current {
        key,
        label,
        detail,
        category: ctx.category,
        presence: now,
        context_ms,
        since_ms,
        project,
    })
}

/// `open` is the session being tracked, if any. Without one, everything but permissions is
/// empty (PLAN §6 contract note), whatever `current` still holds.
pub fn build_status(
    open: Option<&SessionMeta>,
    current: Option<Current>,
    permissions: Permissions,
    auto: bool,
    now_ms: i64,
) -> Status {
    let permissions = PermissionsDto {
        accessibility: permissions.accessibility,
    };
    match open {
        Some(s) => Status {
            tracking: true,
            session_id: Some(s.id),
            started_at_ms: Some(s.started_at_ms),
            // A wall-clock step backwards must not show a negative timer.
            elapsed_ms: (now_ms - s.started_at_ms).max(0),
            current,
            permissions,
            auto,
        },
        None => Status {
            tracking: false,
            session_id: None,
            started_at_ms: None,
            elapsed_ms: 0,
            current: None,
            permissions,
            auto,
        },
    }
}

pub fn build_view(range: Range, samples: &[Sample], config: &Config, lang: Lang) -> View {
    let segments = segment(samples, config);
    let summary = summarize(&segments);
    let one_liner = one_liner(&summary, lang);
    View {
        range,
        total_ms: summary.total_ms,
        active_ms: summary.active_ms,
        passive_ms: summary.passive_ms,
        away_ms: summary.away_ms,
        longest_focus_ms: summary.longest_focus_ms,
        switches: summary.switches,
        categories: summary.categories,
        listening: summary.listening,
        not_shown: summary.not_shown,
        one_liner,
        rows: summary.rows,
        segments: segments.iter().map(|s| segment_dto(s, lang)).collect(),
    }
}

pub fn segment_dto(seg: &Segment, lang: Lang) -> SegmentDto {
    SegmentDto {
        start_ms: seg.start_ms,
        end_ms: seg.end_ms,
        key: seg.key.clone(),
        label: seg.label.clone(),
        category: seg.category,
        kind: seg.kind,
        active_ms: seg.active_ms,
        passive_ms: seg.passive_ms,
        details: seg.details.clone(),
        interruptions: seg
            .interruptions
            .iter()
            .map(|i| InterruptionDto {
                label: i.label.clone(),
                ms: i.ms,
            })
            .collect(),
        explain: explain(seg, lang),
        project: seg.project.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{code, idle, me, run, web};
    use timewent_core::{Category, SegmentKind};

    const T0: i64 = 1_700_000_000_000;

    fn granted() -> Permissions {
        Permissions {
            accessibility: true,
        }
    }

    fn session(id: i64, start: i64, end: Option<i64>) -> SessionMeta {
        SessionMeta {
            id,
            started_at_ms: start,
            ended_at_ms: end,
        }
    }

    /// 20s coding, a 2s YouTube peek, 20s more coding, then 15s on ChatGPT.
    fn coding_with_peek() -> Vec<Sample> {
        let mut s = run(T0, 20, |t| code(t, "timewent", "lib.rs"));
        s.extend(run(T0 + 20_000, 2, |t| {
            web(t, "https://www.youtube.com/watch?v=x", "cats")
        }));
        s.extend(run(T0 + 22_000, 20, |t| code(t, "timewent", "views.rs")));
        s.extend(run(T0 + 42_000, 15, |t| {
            web(t, "https://chatgpt.com/c/1", "ChatGPT")
        }));
        s
    }

    fn cfg() -> Config {
        Config::default()
    }

    fn coding(start: i64, n: usize) -> Vec<Sample> {
        run(start, n, |t| code(t, "proj", "lib.rs"))
    }

    /// Same app, no input: idle climbs from `from_s` by 1s per sample.
    fn quiet(start: i64, n: usize, from_s: f64) -> Vec<Sample> {
        run(start, n, |t| {
            idle(
                code(t, "proj", "lib.rs"),
                from_s + ((t - start) / 1000) as f64,
            )
        })
    }

    #[test]
    fn idle_status_has_no_session_and_no_current() {
        let stale = build_current(&coding(T0, 5), &cfg());
        let st = build_status(None, stale, granted(), false, T0);
        assert!(!st.tracking);
        assert_eq!(st.session_id, None);
        assert_eq!(st.started_at_ms, None);
        assert_eq!(st.elapsed_ms, 0);
        assert_eq!(st.current, None);
        assert!(st.permissions.accessibility);
    }

    #[test]
    fn tracking_status_reports_session_elapsed_and_current() {
        let cur = build_current(&coding(T0, 5), &cfg());
        let st = build_status(
            Some(&session(7, T0, None)),
            cur.clone(),
            Permissions {
                accessibility: false,
            },
            true,
            T0 + 61_000,
        );
        assert!(st.auto);
        assert!(st.tracking);
        assert_eq!(st.session_id, Some(7));
        assert_eq!(st.started_at_ms, Some(T0));
        assert_eq!(st.elapsed_ms, 61_000);
        assert_eq!(st.current, cur);
        assert!(st.current.is_some());
        assert!(!st.permissions.accessibility);
    }

    #[test]
    fn elapsed_is_never_negative_when_the_clock_steps_back() {
        let st = build_status(
            Some(&session(1, T0, None)),
            None,
            granted(),
            false,
            T0 - 5_000,
        );
        assert_eq!(st.elapsed_ms, 0);
    }

    #[test]
    fn status_serializes_to_the_contract_shape() {
        let cur = build_current(&coding(T0, 20), &cfg());
        let st = build_status(
            Some(&session(3, T0, None)),
            cur,
            granted(),
            false,
            T0 + 20_000,
        );
        assert_eq!(
            serde_json::to_value(&st).expect("json"),
            serde_json::json!({
                "tracking": true, "session_id": 3, "started_at_ms": T0, "elapsed_ms": 20_000,
                "current": {"key": "code:proj", "label": "proj", "detail": "lib.rs",
                            "category": "code", "presence": "active",
                            "context_ms": 20_000, "since_ms": null, "project": "proj"},
                "permissions": {"accessibility": true}, "auto": false
            })
        );
        let idle = build_status(None, None, granted(), false, T0);
        assert_eq!(
            serde_json::to_value(&idle).expect("json"),
            serde_json::json!({
                "tracking": false, "session_id": null, "started_at_ms": null, "elapsed_ms": 0,
                "current": null, "permissions": {"accessibility": true}, "auto": false
            })
        );
    }

    #[test]
    fn no_samples_means_no_current() {
        assert_eq!(build_current(&[], &cfg()), None);
    }

    #[test]
    fn current_is_the_latest_context_with_its_session_time() {
        let mut s = coding(T0, 30);
        s.extend(run(T0 + 30_000, 20, |t| {
            web(t, "https://claude.ai/chat", "Claude")
        }));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.key, "web:claude.ai");
        assert_eq!(c.label, "Claude");
        assert_eq!(c.detail.as_deref(), Some("Claude"));
        assert_eq!(c.category, Category::Ai);
        assert_eq!(c.presence, Presence::Active);
        assert_eq!(c.context_ms, 20_000);
        assert_eq!(c.since_ms, None);
    }

    #[test]
    fn context_time_is_the_same_number_the_view_shows() {
        let s = coding_with_peek();
        let mut s2 = s.clone();
        s2.extend(run(T0 + 57_000, 20, |t| code(t, "timewent", "lib.rs")));
        let c = build_current(&s2, &cfg()).expect("current");
        let v = build_view(Range::Session { id: 1 }, &s2, &cfg(), Lang::En);
        let row = v.rows.iter().find(|r| r.key == c.key).expect("row");
        assert_eq!(c.context_ms, row.ms);
        assert_eq!(
            c.context_ms, 77_000,
            "40s + 2s absorbed peek + 15s ChatGPT research between the blocks + 20s back"
        );
    }

    #[test]
    fn on_attributed_research_the_pill_names_the_project_and_its_total() {
        let mut s = coding(T0, 60);
        s.extend(run(T0 + 60_000, 30, |t| {
            web(t, "https://github.com/acme/proj/pull/1", "PR")
        }));
        s.extend(coding(T0 + 90_000, 60));
        s.extend(run(T0 + 150_000, 20, |t| {
            web(t, "https://chatgpt.com/c/1", "how to proj")
        }));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.label, "ChatGPT");
        assert_eq!(c.project.as_deref(), Some("proj"));
        assert_eq!(c.key, "code:proj", "the row it rolls into");
        assert_eq!(c.context_ms, 170_000);
        let v = build_view(Range::Session { id: 1 }, &s, &cfg(), Lang::En);
        assert_eq!(v.rows[0].ms, c.context_ms);
    }

    #[test]
    fn inside_an_activity_the_pill_names_it_and_its_total() {
        let config = Config {
            activities: vec![timewent_core::Activity {
                name: "coding".into(),
                apps: vec![
                    "com.microsoft.VSCode".into(),
                    "com.googlecode.iterm2".into(),
                ],
                domains: vec![],
            }],
            ..cfg()
        };
        let mut s = coding(T0, 60);
        s.extend(run(T0 + 60_000, 30, |t| {
            let mut x = code(t, "x", "y");
            x.app_name = "iTerm2".into();
            x.bundle_id = "com.googlecode.iterm2".into();
            x.window_title = Some("~/src/proj — zsh".into());
            x
        }));
        let c = build_current(&s, &config).expect("current");
        assert_eq!(c.key, "act:coding");
        assert_eq!(c.project.as_deref(), Some("coding"));
        assert_eq!(c.label, "iTerm2", "the member app");
        assert_eq!(c.detail.as_deref(), Some("iTerm2 · ~/src/proj — zsh"));
        assert_eq!(c.context_ms, 90_000, "the activity row's total");
    }

    #[test]
    fn an_unattributed_context_has_no_project() {
        let mut s = coding(T0, 60);
        s.extend(run(T0 + 60_000, 20, |t| {
            web(t, "https://www.youtube.com/watch?v=x", "cats")
        }));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!((c.key.as_str(), c.project), ("web:youtube.com", None));
        assert_eq!(c.context_ms, 20_000);
    }

    #[test]
    fn view_carries_kinds_of_time_and_background_listening() {
        let spotify = timewent_core::Audio {
            bundle_id: "com.spotify.client".into(),
            app: "Spotify".into(),
            title: Some("lofi — ChilledCow".into()),
            host: None,
        };
        let mut s = run(T0, 60, |t| {
            let mut x = code(t, "proj", "lib.rs");
            x.audio = Some(spotify.clone());
            x
        });
        s.extend(run(T0 + 60_000, 20, |t| {
            web(t, "https://www.youtube.com/watch?v=x", "cats")
        }));
        let v = build_view(Range::Today, &s, &cfg(), Lang::En);
        assert_eq!(v.total_ms, 80_000, "listening is not in-use time");
        let json = serde_json::to_value(&v).expect("json");
        assert_eq!(
            json["categories"],
            serde_json::json!([
                {"category": "code", "ms": 60000, "share": 0.75},
                {"category": "media", "ms": 20000, "share": 0.25}
            ])
        );
        assert_eq!(
            json["listening"],
            serde_json::json!([{"label": "Spotify", "title": "lofi — ChilledCow", "ms": 60000}])
        );
    }

    #[test]
    fn view_text_follows_the_language() {
        let mut s = coding(T0, 120);
        s.extend(run(T0 + 120_000, 2, |t| {
            web(t, "https://www.youtube.com/watch?v=x", "cats")
        }));
        s.extend(coding(T0 + 122_000, 60));
        let v = build_view(Range::Today, &s, &cfg(), Lang::Tr);
        assert_eq!(v.one_liner, "3dk kullanımda · proj 3dk · en uzun odak 3dk");
        assert_eq!(
            v.segments[0].explain,
            ["YouTube (2sn) bu etkinliğe katıldı — 3sn eşiğinin altında"]
        );
        let en = build_view(Range::Today, &s, &cfg(), Lang::En);
        assert_eq!(
            en.segments[0].explain,
            ["absorbed YouTube (2s) — under transient threshold 3s"]
        );
    }

    #[test]
    fn view_carries_focus_stats_and_the_one_liner() {
        let mut s = coding(T0, 120);
        s.extend(run(T0 + 120_000, 30, |t| {
            web(t, "https://www.youtube.com/watch?v=x", "cats")
        }));
        let v = build_view(Range::Today, &s, &cfg(), Lang::En);
        assert_eq!((v.longest_focus_ms, v.switches), (120_000, 1));
        assert_eq!(v.one_liner, "2m in use · proj 2m · longest focus 2m");
        let json = serde_json::to_value(&v).expect("json");
        assert_eq!(json["rows"][0]["kind"], "project");
        assert_eq!(
            json["rows"][0]["breakdown"],
            serde_json::json!([{"key": "code", "label": "code", "ms": 120000}])
        );
        assert_eq!(json["rows"][1]["kind"], "context");
    }

    #[test]
    fn passthrough_apps_are_skipped_for_the_context() {
        let mut s = coding(T0, 20);
        s.extend(run(T0 + 20_000, 5, me));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.key, "code:proj");
        assert_eq!(c.presence, Presence::Active);
        assert_eq!(
            c.context_ms, 25_000,
            "the passthrough run is absorbed into it"
        );
        assert_eq!(build_current(&run(T0, 5, me), &cfg()), None);
    }

    #[test]
    fn passive_stretch_reports_when_it_started() {
        let mut s = coding(T0, 20);
        s.extend(quiet(T0 + 20_000, 30, 46.0));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.presence, Presence::Passive);
        assert_eq!(c.since_ms, Some(T0 + 20_000));
        assert_eq!(c.context_ms, 50_000);
    }

    #[test]
    fn away_keeps_the_last_real_context_and_reports_since() {
        let mut s = coding(T0, 20);
        // Idle passes 180s: the whole quiet stretch is away (retroactive rule).
        s.extend(quiet(T0 + 20_000, 200, 45.0));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.presence, Presence::Away);
        assert_eq!(c.since_ms, Some(T0 + 20_000));
        assert_eq!(c.key, "code:proj");
        assert_eq!(c.context_ms, 20_000);
    }

    #[test]
    fn locked_screen_is_away_since_the_lock_not_a_loginwindow_context() {
        let mut s = coding(T0, 20);
        s.extend(run(T0 + 20_000, 5, |t| {
            let mut l = code(t, "x", "y");
            l.app_name = "loginwindow".into();
            l.bundle_id = "com.apple.loginwindow".into();
            l.window_title = None;
            l.locked = true;
            l
        }));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!((c.key.as_str(), c.presence), ("code:proj", Presence::Away));
        assert_eq!(c.since_ms, Some(T0 + 20_000));
    }

    #[test]
    fn since_does_not_reach_back_across_a_gap() {
        let mut s = quiet(T0, 10, 50.0);
        s.extend(quiet(T0 + 600_000, 10, 50.0));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(c.presence, Presence::Passive);
        assert_eq!(c.since_ms, Some(T0 + 600_000));
    }

    #[test]
    fn watching_media_is_passive_not_away() {
        let mut s = coding(T0, 10);
        s.extend(run(T0 + 10_000, 300, |t| {
            let mut w = web(t, "https://www.youtube.com/watch?v=x", "cats");
            w.idle = crate::testkit::idle_all(((t - T0) / 1000) as f64);
            w.media_active = true;
            w
        }));
        let c = build_current(&s, &cfg()).expect("current");
        assert_eq!(
            (c.label.as_str(), c.presence),
            ("YouTube", Presence::Passive)
        );
        assert_eq!(c.since_ms, Some(T0 + 45_000));
    }

    #[test]
    fn session_view_totals_rows_and_absorbed_peek() {
        let v = build_view(
            Range::Session { id: 1 },
            &coding_with_peek(),
            &Config::default(),
            Lang::En,
        );
        assert_eq!(v.range, Range::Session { id: 1 });
        assert_eq!(v.total_ms, 57_000);
        assert_eq!(v.active_ms, 57_000);
        assert_eq!((v.passive_ms, v.away_ms), (0, 0));

        let rows: Vec<(&str, i64)> = v.rows.iter().map(|r| (r.label.as_str(), r.ms)).collect();
        assert_eq!(rows, [("timewent", 42_000), ("ChatGPT", 15_000)]);

        assert_eq!(v.segments.len(), 2);
        let coding = &v.segments[0];
        assert_eq!(coding.kind, SegmentKind::Focus);
        assert_eq!(coding.category, Some(Category::Code));
        assert_eq!(coding.interruptions.len(), 1);
        assert_eq!(coding.interruptions[0].label, "YouTube");
        assert_eq!(coding.interruptions[0].ms, 2_000);
        assert_eq!(
            coding.explain,
            ["absorbed YouTube (2s) — under transient threshold 3s"]
        );
        let files: Vec<&str> = coding.details.iter().map(|d| d.detail.as_str()).collect();
        assert_eq!(files, ["lib.rs", "views.rs"]);
    }

    #[test]
    fn a_quick_look_at_timewent_is_folded_into_the_work_around_it() {
        let mut s = run(T0, 20, |t| code(t, "timewent", "lib.rs"));
        s.extend(run(T0 + 20_000, 6, me));
        s.extend(run(T0 + 26_000, 20, |t| code(t, "timewent", "lib.rs")));
        let v = build_view(Range::Session { id: 1 }, &s, &Config::default(), Lang::En);
        assert_eq!(v.rows.len(), 1);
        assert_eq!(v.rows[0].ms, 46_000);
        assert_eq!(v.segments.len(), 1);
        assert_eq!(
            v.segments[0].explain,
            ["absorbed timewent (6s) — passthrough app, never counted as a context"]
        );
        assert!(v.not_shown.is_empty());
    }

    #[test]
    fn a_long_look_at_timewent_is_in_use_but_not_shown() {
        let mut s = run(T0, 20, |t| code(t, "timewent", "lib.rs"));
        s.extend(run(T0 + 20_000, 12, me));
        s.extend(run(T0 + 32_000, 20, |t| code(t, "timewent", "lib.rs")));
        let v = build_view(Range::Session { id: 1 }, &s, &Config::default(), Lang::En);
        assert_eq!((v.total_ms, v.rows[0].ms), (52_000, 40_000));
        assert_eq!(
            v.segments[1].explain,
            ["timewent 12s — passthrough, not credited to any activity"]
        );
        assert_eq!(
            serde_json::to_value(&v.not_shown).expect("json"),
            serde_json::json!([{"label": "timewent", "ms": 12000}])
        );
        assert_eq!((v.longest_focus_ms, v.switches), (40_000, 0));
    }

    #[test]
    fn row_shares_are_fractions_of_non_away_time() {
        let v = build_view(
            Range::Today,
            &coding_with_peek(),
            &Config::default(),
            Lang::En,
        );
        let sum: f64 = v.rows.iter().map(|r| r.share).sum();
        assert!((sum - 1.0).abs() < 1e-9, "shares sum to {sum}");
        assert!((v.rows[0].share - 42.0 / 57.0).abs() < 1e-9);
    }

    #[test]
    fn today_view_spans_sessions_with_a_gap_between_them() {
        let mut s = run(T0, 10, |t| code(t, "a", "x.rs"));
        s.extend(run(T0 + 600_000, 10, |t| code(t, "b", "y.rs")));
        let v = build_view(Range::Today, &s, &Config::default(), Lang::En);
        assert_eq!(v.range, Range::Today);
        assert_eq!(v.total_ms, 20_000, "gaps are not counted");
        let kinds: Vec<SegmentKind> = v.segments.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [SegmentKind::Focus, SegmentKind::Gap, SegmentKind::Focus]
        );
        let gap = &v.segments[1];
        assert_eq!((gap.label.as_str(), gap.category), ("gap", None));
        assert_eq!(gap.end_ms - gap.start_ms, 600_000 - 10_000);
        assert!(gap.explain[0].starts_with("gap: no samples for 9m50s"));
    }

    #[test]
    fn away_segment_is_labelled_away_with_null_category_and_explained() {
        let mut s = run(T0, 30, |t| code(t, "a", "x.rs"));
        // Idle climbs past the 180s away threshold while the editor stays frontmost.
        s.extend(run(T0 + 30_000, 20, |t| {
            idle(
                code(t, "a", "x.rs"),
                170.0 + ((t - T0 - 30_000) / 1000) as f64,
            )
        }));
        s.extend(run(T0 + 50_000, 30, |t| code(t, "a", "x.rs")));
        let v = build_view(Range::Session { id: 1 }, &s, &Config::default(), Lang::En);

        let away = v
            .segments
            .iter()
            .find(|s| s.kind == SegmentKind::Away)
            .expect("an away segment");
        assert_eq!((away.label.as_str(), away.category), ("away", None));
        assert_eq!(away.end_ms - away.start_ms, 20_000);
        assert_eq!(v.away_ms, 20_000);
        assert!(away.explain.iter().any(|l| l.starts_with("away: no input")));
        assert!(v.rows.iter().all(|r| r.label != "away"));

        let json = serde_json::to_value(away).expect("json");
        assert_eq!(json["kind"], "away");
        assert_eq!(json["category"], serde_json::Value::Null);
    }

    #[test]
    fn view_serializes_to_the_contract_shape() {
        let v = build_view(
            Range::Session { id: 9 },
            &coding_with_peek(),
            &Config::default(),
            Lang::En,
        );
        let json = serde_json::to_value(&v).expect("json");
        assert_eq!(
            json["range"],
            serde_json::json!({"kind": "session", "id": 9})
        );
        let mut keys: Vec<&String> = json.as_object().expect("obj").keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "active_ms",
                "away_ms",
                "categories",
                "listening",
                "longest_focus_ms",
                "not_shown",
                "one_liner",
                "passive_ms",
                "range",
                "rows",
                "segments",
                "switches",
                "total_ms"
            ]
        );
        assert_eq!(
            json["segments"][0]["interruptions"][0],
            serde_json::json!({"label": "YouTube", "ms": 2000})
        );
        assert_eq!(json["rows"][1]["category"], "ai");
        assert_eq!(json["segments"][0]["kind"], "focus");
    }

    #[test]
    fn empty_range_gives_an_empty_view() {
        let v = build_view(
            Range::Session { id: 404 },
            &[],
            &Config::default(),
            Lang::En,
        );
        assert_eq!(v.total_ms, 0);
        assert!(v.rows.is_empty() && v.segments.is_empty());
    }
}
