//! Sample builders shared by unit tests.

use crate::sample::{Idle, Sample};

pub(crate) fn idle_all(s: f64) -> Idle {
    Idle {
        keyboard_s: s,
        mouse_s: s,
        click_s: s,
        scroll_s: s,
    }
}

/// A non-browser, non-editor app sample: context key `app:{app}`.
pub(crate) fn sample(ts_ms: i64, app: &str, idle_s: f64) -> Sample {
    Sample {
        ts_ms,
        app_name: app.into(),
        bundle_id: format!("test.{app}"),
        window_title: None,
        url: None,
        idle: idle_all(idle_s),
        locked: false,
        media_active: false,
        audio: None,
    }
}

/// Consecutive active samples, 1s apart, starting at `start_ms`: `[("A", 20), ("B", 2)]`.
pub(crate) fn seq(start_ms: i64, runs: &[(&str, usize)]) -> Vec<Sample> {
    let mut out = Vec::new();
    let mut ts = start_ms;
    for (app, n) in runs {
        for _ in 0..*n {
            out.push(sample(ts, app, 0.0));
            ts += 1000;
        }
    }
    out
}

/// Samples of one app, 1s apart, with the given overall idle per sample.
pub(crate) fn idles(start_ms: i64, app: &str, idle_s: &[f64]) -> Vec<Sample> {
    idle_s
        .iter()
        .enumerate()
        .map(|(i, s)| sample(start_ms + 1000 * i as i64, app, *s))
        .collect()
}

/// Realistic contexts laid out as consecutive 1s spans (attribution / focus tests).
pub(crate) mod spans {
    use crate::sample::{Idle, Sample};

    pub(crate) const IDLE: Idle = Idle {
        keyboard_s: 0.5,
        mouse_s: 0.5,
        click_s: 0.5,
        scroll_s: 0.5,
    };

    pub(crate) fn editor(project: &str, file: &str) -> Sample {
        Sample {
            ts_ms: 0,
            app_name: "Code".into(),
            bundle_id: "com.microsoft.VSCode".into(),
            window_title: Some(format!("{file} — {project} — Visual Studio Code")),
            url: None,
            idle: IDLE,
            locked: false,
            media_active: false,
            audio: None,
        }
    }

    pub(crate) fn browser(url: &str, title: &str) -> Sample {
        Sample {
            app_name: "Google Chrome".into(),
            bundle_id: "com.google.Chrome".into(),
            window_title: Some(title.into()),
            url: Some(url.into()),
            ..editor("x", "y")
        }
    }

    pub(crate) fn terminal(title: &str) -> Sample {
        Sample {
            app_name: "Terminal".into(),
            bundle_id: "com.apple.Terminal".into(),
            window_title: Some(title.into()),
            url: None,
            ..editor("x", "y")
        }
    }

    pub(crate) fn away(mut s: Sample) -> Sample {
        s.locked = true;
        s
    }

    pub(crate) fn code(p: &str) -> Sample {
        editor(p, "main.rs")
    }
    pub(crate) fn chatgpt(title: &str) -> Sample {
        browser("https://chatgpt.com/c/1", title)
    }
    pub(crate) fn docs() -> Sample {
        browser("https://docs.rs/serde/latest/serde/", "serde - Rust")
    }
    pub(crate) fn youtube() -> Sample {
        browser("https://www.youtube.com/watch?v=x", "cats")
    }

    /// Consecutive 1s samples: `[(sample, seconds), ...]`, starting at t=0. `None` = a
    /// 10-minute hole (sleep) before the next span.
    pub(crate) fn timeline(spans: &[(Option<Sample>, i64)]) -> Vec<Sample> {
        let mut out = Vec::new();
        let mut t = 0;
        for (s, secs) in spans {
            match s {
                Some(s) => {
                    for _ in 0..*secs {
                        out.push(Sample {
                            ts_ms: t * 1000,
                            ..s.clone()
                        });
                        t += 1;
                    }
                }
                None => t += secs,
            }
        }
        out
    }
}
