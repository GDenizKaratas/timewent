//! Property: a `Segmenter` fed one sample at a time always equals `segment` over the same
//! prefix (PLAN §11.5) — over every fixture and many pseudo-random streams.

use std::fs;
use std::path::PathBuf;

use timewent_core::{segment, Audio, Config, Idle, Sample, Segmenter};

/// Deterministic xorshift: reproducible "random" streams without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn idle(s: f64) -> Idle {
    Idle {
        keyboard_s: s,
        mouse_s: s + 1.0,
        click_s: s + 2.0,
        scroll_s: s + 3.0,
    }
}

fn base(app: &str, bundle: &str, title: Option<&str>, url: Option<&str>) -> Sample {
    Sample {
        ts_ms: 0,
        app_name: app.into(),
        bundle_id: bundle.into(),
        window_title: title.map(String::from),
        url: url.map(String::from),
        idle: idle(0.5),
        locked: false,
        media_active: false,
        audio: None,
    }
}

/// A pool covering every rule: projects first seen late, repo URLs, titles, localhost,
/// support candidates, plain web, pass-through apps, lock.
fn pool() -> Vec<Sample> {
    let vs = "com.microsoft.VSCode";
    let chrome = "com.google.Chrome";
    vec![
        base(
            "Code",
            vs,
            Some("main.rs — alpha — Visual Studio Code"),
            None,
        ),
        base("Code", vs, Some("lib.rs — beta_core"), None),
        base("Code", vs, None, None),
        base(
            "Google Chrome",
            chrome,
            Some("alpha design"),
            Some("https://chatgpt.com/c/1"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("dinner"),
            Some("https://chatgpt.com/c/2"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("PR"),
            Some("https://github.com/o/beta-core/pull/1"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("MR"),
            Some("https://gitlab.com/g/s/alpha/-/issues"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("serde"),
            Some("https://docs.rs/serde"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("cats"),
            Some("https://www.youtube.com/watch?v=1"),
        ),
        base(
            "Google Chrome",
            chrome,
            Some("dev"),
            Some("http://localhost:3000/"),
        ),
        base(
            "Terminal",
            "com.apple.Terminal",
            Some("~/src/alpha — zsh"),
            None,
        ),
        base("timewent", "dev.timewent.app", Some("timewent"), None),
        base("Raycast", "com.raycast.macos", None, None),
        Sample {
            locked: true,
            ..base("loginwindow", "com.apple.loginwindow", None, None)
        },
    ]
}

fn random_stream(seed: u64, len: usize) -> Vec<Sample> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let pool = pool();
    let mut out = Vec::with_capacity(len);
    let mut ts: i64 = 1_700_000_000_000;
    while out.len() < len {
        let s = &pool[rng.below(pool.len() as u64) as usize];
        let run = 1 + rng.below(40);
        let idle_from = match rng.below(6) {
            0 => 40.0 + rng.below(300) as f64, // reading, or walked off
            _ => 0.0,
        };
        let media = rng.below(5) == 0;
        let audio = match rng.below(6) {
            0 => Some(Audio {
                bundle_id: "com.spotify.client".into(),
                app: "Spotify".into(),
                title: Some(format!("track {}", rng.below(3))),
                host: None,
            }),
            1 => Some(Audio {
                bundle_id: "com.google.Chrome".into(),
                app: "Google Chrome".into(),
                title: Some("lofi".into()),
                host: Some("youtube.com".into()),
            }),
            _ => None,
        };
        for k in 0..run {
            out.push(Sample {
                ts_ms: ts,
                idle: idle(idle_from + k as f64),
                media_active: media,
                audio: audio.clone(),
                ..s.clone()
            });
            ts += match rng.below(50) {
                0 => 10_000 + rng.below(600_000) as i64, // a gap
                1 => 2_000,                              // a slow tick
                _ => 1_000,
            };
            if out.len() == len {
                break;
            }
        }
    }
    out
}

fn assert_incremental_equals_full(samples: &[Sample], config: &Config, every: usize, name: &str) {
    let mut inc = Segmenter::new(config.clone());
    for (i, s) in samples.iter().enumerate() {
        inc.push(s.clone());
        if i % every == 0 || i + 1 == samples.len() {
            assert_eq!(
                inc.segments(),
                segment(&samples[..=i], config),
                "{name}: diverged after sample {i}"
            );
        }
    }
    assert_eq!(inc.samples(), samples);
}

#[test]
fn random_streams_match_a_full_recompute_after_every_sample() {
    // Full recompute per prefix is quadratic: sizes keep the debug run around a few seconds.
    for seed in 1..=30 {
        assert_incremental_equals_full(
            &random_stream(seed, 220),
            &Config::default(),
            1,
            &format!("seed {seed}"),
        );
    }
}

#[test]
fn random_streams_match_with_attribution_off_and_odd_thresholds() {
    let configs = [
        Config {
            attribute_projects: false,
            ..Config::default()
        },
        Config {
            count_self: true,
            support_window_s: 30,
            transient_max_s: 1,
            glance_max_s: 2,
            ..Config::default()
        },
    ];
    for (c, config) in configs.iter().enumerate() {
        for seed in 100..112 {
            assert_incremental_equals_full(
                &random_stream(seed, 160),
                config,
                1,
                &format!("config {c} seed {seed}"),
            );
        }
    }
}

#[test]
fn fixtures_match_a_full_recompute() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut seen = 0;
    for entry in fs::read_dir(&dir).expect("fixtures dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let samples: Vec<Sample> = fs::read_to_string(&path)
            .expect("read")
            .lines()
            .map(|l| serde_json::from_str(l).expect("sample"))
            .collect();
        assert_incremental_equals_full(
            &samples,
            &Config::default(),
            7,
            &path.display().to_string(),
        );
        seen += 1;
    }
    assert!(seen >= 5, "expected the fixture streams, found {seen}");
}
