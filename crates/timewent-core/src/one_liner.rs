//! One copyable line per summary (PLAN §11.4); also the seed for a future local-LLM
//! explanation, so it states facts only.

use crate::lang::Lang;
use crate::summary::Summary;

/// `1:11 in use · bank-agent-lab 1:05 · away 13m · longest focus 47m`
/// (tr: `1:11 kullanımda · bank-agent-lab 1:05 · uzakta 13dk · en uzun odak 47dk`).
/// In use = active + passive (the header's number); parts that are zero are left out.
pub fn one_liner(summary: &Summary, lang: Lang) -> String {
    let in_use = summary.active_ms + summary.passive_ms;
    if in_use == 0 && summary.away_ms == 0 {
        return match lang {
            Lang::En => "nothing tracked yet",
            Lang::Tr => "henüz kayıt yok",
        }
        .into();
    }
    let t = |ms| short(ms, lang);
    let mut parts = vec![match lang {
        Lang::En => format!("{} in use", t(in_use)),
        Lang::Tr => format!("{} kullanımda", t(in_use)),
    }];
    if let Some(top) = summary.rows.first() {
        parts.push(format!("{} {}", top.label, t(top.ms)));
    }
    if summary.away_ms > 0 {
        parts.push(match lang {
            Lang::En => format!("away {}", t(summary.away_ms)),
            Lang::Tr => format!("uzakta {}", t(summary.away_ms)),
        });
    }
    if summary.longest_focus_ms > 0 {
        parts.push(match lang {
            Lang::En => format!("longest focus {}", t(summary.longest_focus_ms)),
            Lang::Tr => format!("en uzun odak {}", t(summary.longest_focus_ms)),
        });
    }
    parts.join(" · ")
}

/// `1:11` from an hour on, `13m` / `13dk` from a minute on, else `42s` / `42sn`. Floors.
fn short(ms: i64, lang: Lang) -> String {
    let secs = ms.max(0) / 1000;
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    let (min, sec) = match lang {
        Lang::En => ("m", "s"),
        Lang::Tr => ("dk", "sn"),
    };
    if h > 0 {
        format!("{h}:{m:02}")
    } else if m > 0 {
        format!("{m}{min}")
    } else {
        format!("{secs}{sec}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Category;
    use crate::summary::{Row, RowKind, Summary};

    fn summary(
        active: i64,
        passive: i64,
        away: i64,
        longest: i64,
        rows: &[(&str, i64)],
    ) -> Summary {
        Summary {
            total_ms: active + passive + away,
            active_ms: active,
            passive_ms: passive,
            away_ms: away,
            rows: rows
                .iter()
                .map(|(label, ms)| Row {
                    key: format!("code:{label}"),
                    label: (*label).into(),
                    category: Category::Code,
                    ms: *ms,
                    share: 0.0,
                    details: vec![],
                    kind: RowKind::Project,
                    breakdown: vec![],
                })
                .collect(),
            longest_focus_ms: longest,
            switches: 0,
            categories: vec![],
            listening: vec![],
            not_shown: vec![],
        }
    }

    const M: i64 = 60_000;

    #[test]
    fn plan_example() {
        let s = summary(
            65 * M,
            6 * M,
            13 * M,
            47 * M,
            &[("bank-agent-lab", 65 * M), ("ChatGPT", 6 * M)],
        );
        assert_eq!(
            one_liner(&s, Lang::En),
            "1:11 in use · bank-agent-lab 1:05 · away 13m · longest focus 47m"
        );
    }

    #[test]
    fn zero_parts_are_left_out() {
        let s = summary(42 * M, 0, 0, 0, &[("docs", 42 * M)]);
        assert_eq!(one_liner(&s, Lang::En), "42m in use · docs 42m");
    }

    #[test]
    fn short_times_are_seconds_and_durations_floor() {
        let s = summary(
            59 * M + 59_999,
            0,
            42_500,
            59 * M + 59_999,
            &[("p", 59 * M + 59_999)],
        );
        assert_eq!(
            one_liner(&s, Lang::En),
            "59m in use · p 59m · away 42s · longest focus 59m"
        );
        let s = summary(2 * 3_600_000 + 5 * M, 0, 0, 0, &[]);
        assert_eq!(one_liner(&s, Lang::En), "2:05 in use");
    }

    #[test]
    fn nothing_tracked() {
        assert_eq!(
            one_liner(&summary(0, 0, 0, 0, &[]), Lang::En),
            "nothing tracked yet"
        );
        // Only away: nothing was in use, but the away time is still worth saying.
        assert_eq!(
            one_liner(&summary(0, 0, 5 * M, 0, &[]), Lang::En),
            "0s in use · away 5m"
        );
    }

    #[test]
    fn turkish_one_liner() {
        let s = summary(65 * M, 6 * M, 13 * M, 47 * M, &[("bank-agent-lab", 65 * M)]);
        assert_eq!(
            one_liner(&s, Lang::Tr),
            "1:11 kullanımda · bank-agent-lab 1:05 · uzakta 13dk · en uzun odak 47dk"
        );
        assert_eq!(
            one_liner(&summary(0, 0, 0, 0, &[]), Lang::Tr),
            "henüz kayıt yok"
        );
        assert_eq!(
            one_liner(&summary(42_000, 0, 0, 0, &[]), Lang::Tr),
            "42sn kullanımda"
        );
    }
}
