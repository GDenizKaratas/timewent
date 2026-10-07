//! `docs/export-example.json` is the documented example of `timewent.report.v3` (DESIGN §10):
//! the `today` report of the `project_research_session` fixture, English, Europe/Istanbul.
//! This test keeps the committed file identical to what the code produces; a second test holds
//! the size budget. To regenerate after an intended change:
//! `cargo test -p timewent-core --test report_example -- --ignored`.

use std::fs;
use std::path::PathBuf;

use timewent_core::{json_text, report, Config, Lang, Range, ReportInput, Sample, SessionMeta};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Vec<Sample> {
    fs::read_to_string(root().join(format!("fixtures/{name}.jsonl")))
        .expect("fixture")
        .lines()
        .map(|l| serde_json::from_str(l).expect("sample"))
        .collect()
}

/// `today` report text over these sessions (each a run of samples), Istanbul time.
fn today(sessions: &[Vec<Sample>]) -> String {
    let samples: Vec<Sample> = sessions.iter().flatten().cloned().collect();
    let metas: Vec<SessionMeta> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| SessionMeta {
            id: i as i64 + 1,
            started_at_ms: s.first().map_or(0, |x| x.ts_ms),
            ended_at_ms: s.last().map(|x| x.ts_ms + 1000),
        })
        .collect();
    let first = samples.first().map_or(0, |s| s.ts_ms);
    let end = samples.last().map_or(0, |s| s.ts_ms + 1000);
    let istanbul = |_: i64| 3 * 3600;
    let midnight = first - (first + 3 * 3_600_000).rem_euclid(86_400_000);
    let r = report(&ReportInput {
        range: &Range::Today,
        span: (midnight, end),
        samples: &samples,
        sessions: &metas,
        config: &Config::default(),
        lang: Lang::En,
        timezone: "Europe/Istanbul",
        offset_at: &istanbul,
        generated_at_ms: end + 60_000,
    });
    json_text(&r).expect("json")
}

fn example() -> String {
    today(&[fixture("project_research_session")])
}

#[test]
fn committed_example_matches_the_code() {
    let committed = fs::read_to_string(root().join("docs/export-example.json"))
        .expect("docs/export-example.json");
    assert!(
        committed == example(),
        "docs/export-example.json is stale; regenerate with --ignored"
    );
}

#[test]
fn a_busy_today_report_stays_under_8_kb() {
    // Every fixture as its own session, one after another on the same day, 10 minutes apart.
    let names = [
        "coding_youtube_docs_chatgpt",
        "walk_away_and_return",
        "laptop_sleep_gap",
        "watching_video_no_input",
        "project_research_session",
        "activity_coding",
        "music_behind_code",
        "passthrough_not_credited",
    ];
    let mut sessions: Vec<Vec<Sample>> = Vec::new();
    let mut next_start = fixture(names[0])[0].ts_ms;
    for name in names {
        let f = fixture(name);
        let shift = next_start - f[0].ts_ms;
        let moved: Vec<Sample> = f
            .into_iter()
            .map(|s| Sample {
                ts_ms: s.ts_ms + shift,
                ..s
            })
            .collect();
        next_start = moved.last().map_or(next_start, |s| s.ts_ms) + 600_000;
        sessions.push(moved);
    }
    let text = today(&sessions);
    if let Ok(out) = std::env::var("TIMEWENT_BUDGET_OUT") {
        fs::write(out, &text).expect("write");
    }
    assert!(text.len() < 8 * 1024, "{} bytes", text.len());
}

#[test]
#[ignore = "writes docs/export-example.json; run explicitly to regenerate"]
fn regenerate_example() {
    fs::write(root().join("docs/export-example.json"), example()).expect("write");
}
