//! Golden tests: recorded-format sample streams in `fixtures/*.jsonl` (one `Sample` per line,
//! exactly `serde_json::to_string(&Sample)`) → `segment` + `summarize` → compared with the
//! hand-computed `fixtures/<name>.expected.json`.
//!
//! The synthetic fixtures are produced by `synth` below. To regenerate them after changing a
//! scenario: `cargo test -p timewent-core --test golden -- --ignored`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use timewent_core::{
    explain, segment, summarize, Activity, Config, Lang, Sample, SegmentKind, Summary,
};

const SYNTHETIC: &[&str] = &[
    "coding_youtube_docs_chatgpt",
    "walk_away_and_return",
    "laptop_sleep_gap",
    "watching_video_no_input",
    "project_research_session",
    "activity_coding",
    "music_behind_code",
    "passthrough_not_credited",
];

/// Scenarios run with defaults, except where they exercise a config (§13.1 activities).
fn config_for(name: &str) -> Config {
    match name {
        "activity_coding" => Config {
            activities: vec![Activity {
                name: "coding".into(),
                apps: vec![
                    "com.microsoft.VSCode".into(),
                    "com.googlecode.iterm2".into(),
                ],
                domains: vec![],
            }],
            ..Config::default()
        },
        _ => Config::default(),
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn read_jsonl(name: &str) -> (Vec<String>, Vec<Sample>) {
    let path = fixtures_dir().join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let samples = lines
        .iter()
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}:{}: {e}", i + 1)))
        .collect();
    (lines, samples)
}

/// What a fixture must produce. Per segment: its shape plus the explain lines, so evidence
/// is pinned too (details are pinned through the summary rows).
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Expected {
    summary: Summary,
    segments: Vec<SegmentShape>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SegmentShape {
    key: String,
    kind: SegmentKind,
    start_ms: i64,
    end_ms: i64,
    active_ms: i64,
    passive_ms: i64,
    explain: Vec<String>,
}

/// Shares are compared at 4 decimals so expected files stay hand-writable.
fn round_shares(mut s: Summary) -> Summary {
    for row in &mut s.rows {
        row.share = (row.share * 10_000.0).round() / 10_000.0;
    }
    for c in &mut s.categories {
        c.share = (c.share * 10_000.0).round() / 10_000.0;
    }
    s
}

fn actual(samples: &[Sample], config: &Config) -> Expected {
    let segments = segment(samples, config);
    Expected {
        summary: round_shares(summarize(&segments)),
        segments: segments
            .iter()
            .map(|s| SegmentShape {
                key: s.key.clone(),
                kind: s.kind,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                active_ms: s.active_ms,
                passive_ms: s.passive_ms,
                explain: explain(s, Lang::En),
            })
            .collect(),
    }
}

fn check_golden(name: &str) {
    let (_, samples) = read_jsonl(name);
    let path = fixtures_dir().join(format!("{name}.expected.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let expected: Expected = serde_json::from_str(&text).expect("expected.json parses");
    let got = actual(&samples, &config_for(name));
    assert_eq!(
        got,
        expected,
        "{name}: actual output was\n{}",
        serde_json::to_string_pretty(&got).unwrap_or_default()
    );
}

#[test]
fn golden_coding_youtube_docs_chatgpt() {
    check_golden("coding_youtube_docs_chatgpt");
}

#[test]
fn golden_walk_away_and_return() {
    check_golden("walk_away_and_return");
}

#[test]
fn golden_laptop_sleep_gap() {
    check_golden("laptop_sleep_gap");
}

#[test]
fn golden_watching_video_no_input() {
    check_golden("watching_video_no_input");
}

#[test]
fn golden_project_research_session() {
    check_golden("project_research_session");
}

#[test]
fn golden_activity_coding() {
    check_golden("activity_coding");
}

#[test]
fn golden_music_behind_code() {
    check_golden("music_behind_code");
}

#[test]
fn golden_passthrough_not_credited() {
    check_golden("passthrough_not_credited");
}

#[test]
fn fixture_lines_are_exactly_serialized_samples() {
    for name in SYNTHETIC {
        let (lines, samples) = read_jsonl(name);
        for (line, sample) in lines.iter().zip(&samples) {
            assert_eq!(&serde_json::to_string(sample).expect("serialize"), line);
        }
    }
}

#[test]
fn synthetic_fixtures_match_their_generator() {
    for name in SYNTHETIC {
        let (_, samples) = read_jsonl(name);
        assert!(
            samples == synth::scenario(name),
            "{name}: regenerate fixtures"
        );
    }
}

#[test]
#[ignore = "writes fixtures/*.jsonl; run explicitly to regenerate"]
fn regenerate_synthetic_fixtures() {
    for name in SYNTHETIC {
        let mut out = String::new();
        for s in synth::scenario(name) {
            out.push_str(&serde_json::to_string(&s).expect("serialize"));
            out.push('\n');
        }
        fs::write(fixtures_dir().join(format!("{name}.jsonl")), out).expect("write fixture");
    }
}

/// Deterministic, readable scenario builder: one sample per second, idle derived from when
/// each input type last fired.
mod synth {
    use timewent_core::{Audio, Idle, Sample};

    /// 2026-10-06T08:00:00Z.
    pub const T0_MS: i64 = 1_791_273_600_000;

    #[derive(Clone, Copy)]
    pub enum Ev {
        Key,
        Mouse,
        Click,
        Scroll,
    }

    #[derive(Clone)]
    pub struct Win {
        app: &'static str,
        bundle: &'static str,
        title: Option<&'static str>,
        url: Option<&'static str>,
        locked: bool,
        /// A `PreventUserIdleDisplaySleep` assertion is held (video, call).
        media: bool,
        /// What holds audio output while this window is frontmost.
        audio: Option<Audio>,
    }

    fn vscode(title: &'static str) -> Win {
        Win {
            app: "Code",
            bundle: "com.microsoft.VSCode",
            title: Some(title),
            url: None,
            locked: false,
            media: false,
            audio: None,
        }
    }

    fn chrome(title: &'static str, url: &'static str) -> Win {
        Win {
            app: "Google Chrome",
            bundle: "com.google.Chrome",
            title: Some(title),
            url: Some(url),
            locked: false,
            media: false,
            audio: None,
        }
    }

    /// A tab playing video: the browser holds a display-sleep assertion.
    fn playing(win: Win) -> Win {
        Win { media: true, ..win }
    }

    fn app(app: &'static str, bundle: &'static str, title: &'static str) -> Win {
        Win {
            app,
            bundle,
            title: Some(title),
            url: None,
            locked: false,
            media: false,
            audio: None,
        }
    }

    fn with_audio(win: Win, audio: Audio) -> Win {
        Win {
            audio: Some(audio),
            ..win
        }
    }

    fn loginwindow() -> Win {
        Win {
            app: "loginwindow",
            bundle: "com.apple.loginwindow",
            title: None,
            url: None,
            locked: true,
            media: false,
            audio: None,
        }
    }

    struct Gen {
        t_s: i64,
        /// Second at which keyboard / mouse / click / scroll last fired.
        last_s: [i64; 4],
        out: Vec<Sample>,
    }

    impl Gen {
        fn new() -> Gen {
            Gen {
                t_s: 0,
                last_s: [-3, -10, -20, -60],
                out: Vec::new(),
            }
        }

        /// `secs` samples in `win`; `events(i)` lists the inputs during second `i`.
        fn span(&mut self, win: &Win, secs: i64, events: impl Fn(i64) -> Vec<Ev>) {
            for i in 0..secs {
                for ev in events(i) {
                    self.last_s[ev as usize] = self.t_s;
                }
                // Events land 400ms before the poll; integer ms keeps the JSON tidy.
                let idle = |k: usize| ((self.t_s - self.last_s[k]) * 1000 + 400) as f64 / 1000.0;
                self.out.push(Sample {
                    ts_ms: T0_MS + self.t_s * 1000,
                    app_name: win.app.into(),
                    bundle_id: win.bundle.into(),
                    window_title: win.title.map(String::from),
                    url: win.url.map(String::from),
                    idle: Idle {
                        keyboard_s: idle(0),
                        mouse_s: idle(1),
                        click_s: idle(2),
                        scroll_s: idle(3),
                    },
                    locked: win.locked,
                    media_active: win.media,
                    audio: win.audio.clone(),
                });
                self.t_s += 1;
            }
        }

        /// No samples at all for `secs` (lid closed).
        fn sleep(&mut self, secs: i64) {
            self.t_s += secs;
        }
    }

    fn typing(i: i64) -> Vec<Ev> {
        if i % 7 == 0 {
            vec![Ev::Key, Ev::Mouse]
        } else {
            vec![Ev::Key]
        }
    }

    fn mousing(_: i64) -> Vec<Ev> {
        vec![Ev::Mouse, Ev::Click]
    }

    fn nothing(_: i64) -> Vec<Ev> {
        vec![]
    }

    /// Scrolls at the given seconds only.
    fn scrolls_at(at: &'static [i64]) -> impl Fn(i64) -> Vec<Ev> {
        move |i| {
            if at.contains(&i) {
                vec![Ev::Scroll]
            } else {
                vec![]
            }
        }
    }

    pub fn scenario(name: &str) -> Vec<Sample> {
        let mut g = Gen::new();
        match name {
            // Code 120s → 2s YouTube (absorbed) → code 60s → docs.rs 140s read with long
            // scroll pauses (30s passive) → stackoverflow 40s → ChatGPT 90s.
            "coding_youtube_docs_chatgpt" => {
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 120, typing);
                g.span(
                    &chrome(
                        "cat video - YouTube",
                        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                    ),
                    2,
                    mousing,
                );
                g.span(
                    &vscode("● models.py — bank-agent-lab — Visual Studio Code"),
                    60,
                    typing,
                );
                g.span(
                    &chrome(
                        "serde_json - Rust",
                        "https://docs.rs/serde_json/latest/serde_json/",
                    ),
                    140,
                    scrolls_at(&[0, 70, 120]),
                );
                g.span(
                    &chrome(
                        "borrow checker - Stack Overflow",
                        "https://stackoverflow.com/questions/1",
                    ),
                    40,
                    |i| {
                        if i == 0 {
                            vec![Ev::Click, Ev::Scroll]
                        } else {
                            vec![]
                        }
                    },
                );
                g.span(
                    &chrome("Rust lifetimes explained", "https://chatgpt.com/c/abc123"),
                    90,
                    typing,
                );
            }
            // Code 300s → no input for 595s (VS Code still frontmost) → 5s unlocking at the
            // login window → code 180s.
            "walk_away_and_return" => {
                g.span(&vscode("main.rs — timewent"), 300, typing);
                g.span(&vscode("main.rs — timewent"), 595, nothing);
                g.span(&loginwindow(), 5, |_| vec![Ev::Key]);
                g.span(&vscode("main.rs — timewent"), 180, typing);
            }
            // Code 90s → lid closed 30 min → 4s unlocking → code 60s → 6s Slack glance →
            // code 30s.
            "laptop_sleep_gap" => {
                g.span(&vscode("lib.rs — timewent"), 90, typing);
                g.sleep(1_800);
                g.span(&loginwindow(), 4, |_| vec![Ev::Key]);
                g.span(&vscode("lib.rs — timewent"), 60, typing);
                g.span(
                    &app("Slack", "com.tinyspeck.slackmacgap", "general"),
                    6,
                    mousing,
                );
                g.span(&vscode("lib.rs — timewent"), 30, typing);
            }
            // Code 60s → 10 min of YouTube with no input at all while the video plays →
            // code 60s. Watching is passive, never away (PLAN §10.1).
            "watching_video_no_input" => {
                g.span(&vscode("main.rs — timewent"), 60, typing);
                g.span(
                    &playing(chrome(
                        "cat video - YouTube",
                        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                    )),
                    600,
                    nothing,
                );
                g.span(&vscode("main.rs — timewent"), 60, typing);
            }
            // §11.1: code 300s → ChatGPT 120s → docs.rs 90s → the project's GitHub PR 60s →
            // code 180s → YouTube 15s → code 120s. ChatGPT and docs are research for
            // bank-agent-lab (between its code block and its PR); YouTube stays its own row.
            "project_research_session" => {
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 300, typing);
                g.span(
                    &chrome("Rust lifetimes explained", "https://chatgpt.com/c/abc123"),
                    120,
                    typing,
                );
                g.span(
                    &chrome(
                        "serde_json - Rust",
                        "https://docs.rs/serde_json/latest/serde_json/",
                    ),
                    90,
                    scrolls_at(&[0, 40, 80]),
                );
                g.span(
                    &chrome(
                        "Add risk limits · Pull Request #12 · acme/bank-agent-lab",
                        "https://github.com/acme/bank-agent-lab/pull/12",
                    ),
                    60,
                    mousing,
                );
                g.span(&vscode("models.py — bank-agent-lab"), 180, typing);
                g.span(
                    &chrome(
                        "cat video - YouTube",
                        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                    ),
                    15,
                    mousing,
                );
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 120, typing);
            }
            // §13.1: coding = VS Code + iTerm2. VS Code 120s → iTerm2 60s → ChatGPT 90s (its
            // title names the project) → VS Code 120s → iTerm2 30s → VS Code 60s. One "coding"
            // row with a member breakdown; ChatGPT stays its own row: members define no
            // projects and anchor nothing.
            "activity_coding" => {
                let iterm = app(
                    "iTerm2",
                    "com.googlecode.iterm2",
                    "~/src/bank-agent-lab — zsh",
                );
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 120, typing);
                g.span(&iterm, 60, typing);
                g.span(
                    &chrome("bank-agent-lab risk model", "https://chatgpt.com/c/abc123"),
                    90,
                    typing,
                );
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 120, typing);
                g.span(&iterm, 30, typing);
                g.span(&vscode("risk_engine.py — bank-agent-lab"), 60, typing);
            }
            // §14.2: Spotify plays behind VS Code for 10 min, then YouTube is frontmost (and
            // plays itself) for 2 min. In-use is the 12 minutes of what was on screen; the
            // Spotify track is a parallel listening lane; YouTube in front is watching (media).
            "music_behind_code" => {
                let spotify = Audio {
                    bundle_id: "com.spotify.client".into(),
                    app: "Spotify".into(),
                    title: Some("lofi beats — ChilledCow".into()),
                    host: None,
                };
                let youtube_tab = Audio {
                    bundle_id: "com.google.Chrome".into(),
                    app: "Google Chrome".into(),
                    title: Some("cat video - YouTube".into()),
                    host: Some("youtube.com".into()),
                };
                g.span(
                    &with_audio(vscode("main.rs — timewent"), spotify),
                    600,
                    typing,
                );
                g.span(
                    &with_audio(
                        chrome(
                            "cat video - YouTube",
                            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                        ),
                        youtube_tab,
                    ),
                    120,
                    mousing,
                );
            }
            // §21.1, the user's session 15: System Settings 40s → 95s looking at timewent →
            // PDFgear 30s. The 95s is in use but credited to no one (it used to go to PDFgear).
            "passthrough_not_credited" => {
                g.span(
                    &app("Sistem Ayarları", "com.apple.systempreferences", "Gizlilik"),
                    40,
                    mousing,
                );
                g.span(
                    &app(
                        "timewent",
                        "dev.timewent.app",
                        "timewent — see where your time went",
                    ),
                    95,
                    mousing,
                );
                g.span(
                    &app("PDFgear", "com.pdfgear.mac", "report.pdf"),
                    30,
                    mousing,
                );
            }
            other => panic!("unknown scenario {other}"),
        }
        g.out
    }
}
