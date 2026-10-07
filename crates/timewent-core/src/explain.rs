//! "Why?" for a segment, as terminal-comment style lines: `explain` (DESIGN §9, DESIGN §9).

use crate::lang::Lang;
use crate::segment::{Evidence, Segment};

/// `ProjectMatch.via` for a window-title match (translated; a repo URL via is data).
pub const VIA_TITLE: &str = "window title";
/// `ProjectMatch.via` for localhost attributed to its neighbouring code block.
pub const VIA_LOCALHOST: &str = "localhost next to its code block";

/// One line per evidence item, in evidence order.
pub fn explain(segment: &Segment, lang: Lang) -> Vec<String> {
    segment.evidence.iter().map(|e| line(e, lang)).collect()
}

pub(crate) fn line(evidence: &Evidence, lang: Lang) -> String {
    let d = |ms: i64| fmt_duration(ms, lang);
    match (lang, evidence) {
        (
            Lang::En,
            Evidence::AbsorbedTransient {
                label,
                ms,
                threshold_s,
            },
        ) => format!(
            "absorbed {label} ({}) — under transient threshold {threshold_s}s",
            d(*ms)
        ),
        (
            Lang::Tr,
            Evidence::AbsorbedTransient {
                label,
                ms,
                threshold_s,
            },
        ) => format!(
            "{label} ({}) bu etkinliğe katıldı — {threshold_s}sn eşiğinin altında",
            d(*ms)
        ),
        (Lang::En, Evidence::AbsorbedPassthrough { label, ms }) => format!(
            "absorbed {label} ({}) — passthrough app, never counted as a context",
            d(*ms)
        ),
        (Lang::Tr, Evidence::AbsorbedPassthrough { label, ms }) => format!(
            "{label} ({}) bu etkinliğe katıldı — ara uygulama, ayrı sayılmaz",
            d(*ms)
        ),
        (Lang::En, Evidence::PassthroughGlance { ms }) => format!(
            "glance {} — passthrough app with no activity beside it to fold into",
            d(*ms)
        ),
        (Lang::Tr, Evidence::PassthroughGlance { ms }) => {
            format!("kısa bakış {} — katılacağı bir etkinlik yoktu", d(*ms))
        }
        (Lang::En, Evidence::PassthroughNotCredited { label, ms }) => format!(
            "{label} {} — passthrough, not credited to any activity",
            d(*ms)
        ),
        (Lang::Tr, Evidence::PassthroughNotCredited { label, ms }) => format!(
            "{label} {} — ara uygulama, hiçbir etkinliğe yazılmadı",
            d(*ms)
        ),
        (Lang::En, Evidence::Glance { ms, threshold_s }) => format!(
            "glance {} — under {threshold_s}s, kept separate, not a focus block",
            d(*ms)
        ),
        (Lang::Tr, Evidence::Glance { ms, threshold_s }) => format!(
            "kısa bakış {} — {threshold_s}sn altı, ayrı tutuldu, odak sayılmaz",
            d(*ms)
        ),
        (
            Lang::En,
            Evidence::IdleAway {
                idle_run_s,
                threshold_s,
            },
        ) => format!("away: no input for {idle_run_s}s (threshold {threshold_s}s)"),
        (
            Lang::Tr,
            Evidence::IdleAway {
                idle_run_s,
                threshold_s,
            },
        ) => format!("uzakta: {idle_run_s}sn boyunca giriş yok (eşik {threshold_s}sn)"),
        (Lang::En, Evidence::Locked) => "away: screen locked".to_string(),
        (Lang::Tr, Evidence::Locked) => "uzakta: ekran kilitli".to_string(),
        (Lang::En, Evidence::PassiveTime { ms, threshold_s }) => format!(
            "passive {} — idle ≥ {threshold_s}s, kept as reading",
            d(*ms)
        ),
        (Lang::Tr, Evidence::PassiveTime { ms, threshold_s }) => {
            format!("pasif {} — {threshold_s}sn+ boşta, okuma sayıldı", d(*ms))
        }
        (Lang::En, Evidence::MediaPassive { ms }) => format!(
            "kept as passive {} — media/meeting kept the display awake",
            d(*ms)
        ),
        (Lang::Tr, Evidence::MediaPassive { ms }) => format!(
            "{} pasif sayıldı — video/toplantı ekranı açık tuttu",
            d(*ms)
        ),
        (Lang::En, Evidence::ProjectMatch { project, via }) => {
            format!("{via} matched project {project}")
        }
        (Lang::Tr, Evidence::ProjectMatch { project, via }) => {
            let via = match via.as_str() {
                VIA_TITLE => "pencere başlığı",
                VIA_LOCALHOST => "kod bloğunun yanındaki localhost",
                data => data,
            };
            format!("{via} → {project} projesiyle eşleşti")
        }
        (
            Lang::En,
            Evidence::SupportFor {
                project,
                label,
                window_s,
            },
        ) => format!(
            "{label} counted as research for {project} (between two {project} blocks, {} apart)",
            d(window_s * 1000)
        ),
        (
            Lang::Tr,
            Evidence::SupportFor {
                project,
                label,
                window_s,
            },
        ) => format!(
            "{label}, {project} için araştırma sayıldı (iki {project} bloğu arasında, {} arayla)",
            d(window_s * 1000)
        ),
        (Lang::En, Evidence::UserActivity { activity, member }) => {
            format!("{member} counted as {activity} — your activity")
        }
        (Lang::Tr, Evidence::UserActivity { activity, member }) => {
            format!("{member}, {activity} etkinliğine sayıldı — senin tanımın")
        }
        (Lang::En, Evidence::Gap { ms }) => format!(
            "gap: no samples for {} — sleep, shutdown or tracker not running",
            d(*ms)
        ),
        (Lang::Tr, Evidence::Gap { ms }) => format!(
            "kaydedilmedi: {} — uyku, kapanma ya da takip kapalı",
            d(*ms)
        ),
    }
}

/// English: `1.7s` under 10s (truncated to tenths), `42s`, `1m40s`, `1h02m03s`.
/// Turkish: `1,7sn`, `42sn`, `1dk 40sn`, `1sa 2dk 3sn` — spaced, unpadded, zero parts left out.
/// Integer arithmetic only, so output never depends on float rounding.
pub(crate) fn fmt_duration(ms: i64, lang: Lang) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let tenths = (secs < 10 && ms % 1000 != 0 && h == 0 && m == 0).then_some(ms % 1000 / 100);
    match lang {
        Lang::En => {
            if h > 0 {
                format!("{h}h{m:02}m{s:02}s")
            } else if m > 0 {
                format!("{m}m{s:02}s")
            } else if let Some(t) = tenths {
                format!("{secs}.{t}s")
            } else {
                format!("{secs}s")
            }
        }
        Lang::Tr => {
            if let Some(t) = tenths {
                return format!("{secs},{t}sn");
            }
            let parts: Vec<String> = [(h, "sa"), (m, "dk"), (s, "sn")]
                .into_iter()
                .filter(|(v, _)| *v > 0)
                .map(|(v, unit)| format!("{v}{unit}"))
                .collect();
            if parts.is_empty() {
                "0sn".into()
            } else {
                parts.join(" ")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::SegmentKind;

    fn seg_with(evidence: Vec<Evidence>) -> Segment {
        Segment {
            start_ms: 0,
            end_ms: 1000,
            key: "k".into(),
            label: "k".into(),
            category: None,
            kind: SegmentKind::Focus,
            active_ms: 0,
            passive_ms: 0,
            details: vec![],
            interruptions: vec![],
            evidence,
            project: None,
            members: vec![],
            listening: vec![],
        }
    }

    fn line(e: Evidence) -> String {
        let lines = explain(&seg_with(vec![e]), Lang::En);
        assert_eq!(lines.len(), 1);
        lines[0].clone()
    }

    #[test]
    fn absorbed_transient_names_label_duration_and_threshold() {
        assert_eq!(
            line(Evidence::AbsorbedTransient {
                label: "youtube.com".into(),
                ms: 1_700,
                threshold_s: 3
            }),
            "absorbed youtube.com (1.7s) — under transient threshold 3s"
        );
    }

    #[test]
    fn absorbed_passthrough_names_app_and_rule() {
        assert_eq!(
            line(Evidence::AbsorbedPassthrough {
                label: "timewent".into(),
                ms: 12_000
            }),
            "absorbed timewent (12s) — passthrough app, never counted as a context"
        );
    }

    #[test]
    fn passthrough_glance_says_there_was_nothing_to_fold_it_into() {
        assert_eq!(
            line(Evidence::PassthroughGlance { ms: 15_000 }),
            "glance 15s — passthrough app with no activity beside it to fold into"
        );
    }

    #[test]
    fn media_passive_says_the_display_was_kept_awake() {
        assert_eq!(
            line(Evidence::MediaPassive { ms: 360_000 }),
            "kept as passive 6m00s — media/meeting kept the display awake"
        );
    }

    #[test]
    fn passthrough_not_credited_names_app_and_time() {
        assert_eq!(
            line(Evidence::PassthroughNotCredited {
                label: "timewent".into(),
                ms: 95_000
            }),
            "timewent 1m35s — passthrough, not credited to any activity"
        );
    }

    #[test]
    fn idle_away_states_idle_run_and_threshold() {
        assert_eq!(
            line(Evidence::IdleAway {
                idle_run_s: 212,
                threshold_s: 180
            }),
            "away: no input for 212s (threshold 180s)"
        );
    }

    #[test]
    fn passive_time_uses_minutes_and_seconds() {
        assert_eq!(
            line(Evidence::PassiveTime {
                ms: 100_000,
                threshold_s: 45
            }),
            "passive 1m40s — idle ≥ 45s, kept as reading"
        );
    }

    #[test]
    fn glance_states_duration_and_threshold() {
        assert_eq!(
            line(Evidence::Glance {
                ms: 9_000,
                threshold_s: 10
            }),
            "glance 9s — under 10s, kept separate, not a focus block"
        );
    }

    #[test]
    fn locked_says_screen_locked() {
        assert_eq!(line(Evidence::Locked), "away: screen locked");
    }

    #[test]
    fn gap_states_missing_duration() {
        assert_eq!(
            line(Evidence::Gap { ms: 3_723_000 }),
            "gap: no samples for 1h02m03s — sleep, shutdown or tracker not running"
        );
    }

    #[test]
    fn lines_follow_evidence_order() {
        let lines = explain(
            &seg_with(vec![
                Evidence::AbsorbedTransient {
                    label: "B".into(),
                    ms: 2_000,
                    threshold_s: 3,
                },
                Evidence::PassiveTime {
                    ms: 59_000,
                    threshold_s: 45,
                },
            ]),
            Lang::En,
        );
        assert_eq!(
            lines,
            vec![
                "absorbed B (2s) — under transient threshold 3s",
                "passive 59s — idle ≥ 45s, kept as reading",
            ]
        );
    }

    #[test]
    fn segment_without_evidence_has_no_lines() {
        assert!(explain(&seg_with(vec![]), Lang::En).is_empty());
    }

    #[test]
    fn durations_format_compactly() {
        assert_eq!(fmt_duration(0, Lang::En), "0s");
        assert_eq!(fmt_duration(1_750, Lang::En), "1.7s");
        assert_eq!(fmt_duration(9_999, Lang::En), "9.9s");
        assert_eq!(fmt_duration(12_500, Lang::En), "12s");
        assert_eq!(fmt_duration(59_999, Lang::En), "59s");
        assert_eq!(fmt_duration(60_000, Lang::En), "1m00s");
        assert_eq!(fmt_duration(65_000, Lang::En), "1m05s");
        assert_eq!(fmt_duration(3_600_000, Lang::En), "1h00m00s");
    }

    mod turkish {
        use super::*;

        fn tr(e: Evidence) -> String {
            let lines = explain(&seg_with(vec![e]), Lang::Tr);
            assert_eq!(lines.len(), 1);
            lines[0].clone()
        }

        #[test]
        fn every_evidence_line_has_turkish_copy() {
            let cases = [
                (
                    Evidence::AbsorbedTransient {
                        label: "youtube.com".into(),
                        ms: 1_700,
                        threshold_s: 3,
                    },
                    "youtube.com (1,7sn) bu etkinliğe katıldı — 3sn eşiğinin altında",
                ),
                (
                    Evidence::AbsorbedPassthrough {
                        label: "timewent".into(),
                        ms: 12_000,
                    },
                    "timewent (12sn) bu etkinliğe katıldı — ara uygulama, ayrı sayılmaz",
                ),
                (
                    Evidence::Glance {
                        ms: 9_000,
                        threshold_s: 10,
                    },
                    "kısa bakış 9sn — 10sn altı, ayrı tutuldu, odak sayılmaz",
                ),
                (
                    Evidence::PassthroughGlance { ms: 15_000 },
                    "kısa bakış 15sn — katılacağı bir etkinlik yoktu",
                ),
                (
                    Evidence::IdleAway {
                        idle_run_s: 212,
                        threshold_s: 180,
                    },
                    "uzakta: 212sn boyunca giriş yok (eşik 180sn)",
                ),
                (
                    Evidence::PassthroughNotCredited {
                        label: "timewent".into(),
                        ms: 95_000,
                    },
                    "timewent 1dk 35sn — ara uygulama, hiçbir etkinliğe yazılmadı",
                ),
                (Evidence::Locked, "uzakta: ekran kilitli"),
                (
                    Evidence::PassiveTime {
                        ms: 100_000,
                        threshold_s: 45,
                    },
                    "pasif 1dk 40sn — 45sn+ boşta, okuma sayıldı",
                ),
                (
                    Evidence::MediaPassive { ms: 360_000 },
                    "6dk pasif sayıldı — video/toplantı ekranı açık tuttu",
                ),
                (
                    Evidence::Gap { ms: 3_723_000 },
                    "kaydedilmedi: 1sa 2dk 3sn — uyku, kapanma ya da takip kapalı",
                ),
                (
                    Evidence::ProjectMatch {
                        project: "bank-agent-lab".into(),
                        via: "github.com/acme/bank-agent-lab".into(),
                    },
                    "github.com/acme/bank-agent-lab → bank-agent-lab projesiyle eşleşti",
                ),
                (
                    Evidence::ProjectMatch {
                        project: "x".into(),
                        via: VIA_TITLE.into(),
                    },
                    "pencere başlığı → x projesiyle eşleşti",
                ),
                (
                    Evidence::ProjectMatch {
                        project: "x".into(),
                        via: VIA_LOCALHOST.into(),
                    },
                    "kod bloğunun yanındaki localhost → x projesiyle eşleşti",
                ),
                (
                    Evidence::SupportFor {
                        project: "p".into(),
                        label: "docs".into(),
                        window_s: 360,
                    },
                    "docs, p için araştırma sayıldı (iki p bloğu arasında, 6dk arayla)",
                ),
            ];
            for (e, want) in cases {
                assert_eq!(tr(e), want);
            }
        }

        #[test]
        fn english_via_phrases_are_unchanged() {
            let en = explain(
                &seg_with(vec![Evidence::ProjectMatch {
                    project: "x".into(),
                    via: VIA_TITLE.into(),
                }]),
                Lang::En,
            );
            assert_eq!(en, ["window title matched project x"]);
        }

        #[test]
        fn turkish_durations_use_sa_dk_sn_and_a_decimal_comma() {
            assert_eq!(fmt_duration(0, Lang::Tr), "0sn");
            assert_eq!(fmt_duration(1_700, Lang::Tr), "1,7sn");
            assert_eq!(fmt_duration(42_000, Lang::Tr), "42sn");
            assert_eq!(fmt_duration(100_000, Lang::Tr), "1dk 40sn");
            assert_eq!(fmt_duration(3_723_000, Lang::Tr), "1sa 2dk 3sn");
            // Zero parts are left out rather than padded.
            assert_eq!(fmt_duration(360_000, Lang::Tr), "6dk");
            assert_eq!(fmt_duration(3_603_000, Lang::Tr), "1sa 3sn");
            assert_eq!(fmt_duration(7_200_000, Lang::Tr), "2sa");
        }
    }
}
