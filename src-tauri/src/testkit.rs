//! Sample builders for the app's unit tests.

use timewent_core::{Idle, Sample};

pub(crate) fn idle_all(s: f64) -> Idle {
    Idle {
        keyboard_s: s,
        mouse_s: s,
        click_s: s,
        scroll_s: s,
    }
}

/// VS Code editing `file` in `project`: context `code:{project}`, detail `file`.
pub(crate) fn code(ts_ms: i64, project: &str, file: &str) -> Sample {
    Sample {
        ts_ms,
        app_name: "Code".into(),
        bundle_id: "com.microsoft.VSCode".into(),
        window_title: Some(format!("{file} — {project} — Visual Studio Code")),
        url: None,
        idle: idle_all(0.5),
        locked: false,
        media_active: false,
        audio: None,
    }
}

/// Chrome on `url` with page title `title`.
pub(crate) fn web(ts_ms: i64, url: &str, title: &str) -> Sample {
    Sample {
        ts_ms,
        app_name: "Google Chrome".into(),
        bundle_id: "com.google.Chrome".into(),
        window_title: Some(title.into()),
        url: Some(url.into()),
        idle: idle_all(0.5),
        locked: false,
        media_active: false,
        audio: None,
    }
}

pub(crate) fn idle(mut s: Sample, idle_s: f64) -> Sample {
    s.idle = idle_all(idle_s);
    s
}

/// `n` samples 1s apart from `start_ms`, each built by `f(ts)`.
pub(crate) fn run(start_ms: i64, n: usize, f: impl Fn(i64) -> Sample) -> Vec<Sample> {
    (0..n).map(|i| f(start_ms + 1000 * i as i64)).collect()
}

/// timewent itself frontmost (a default passthrough app).
pub(crate) fn me(ts_ms: i64) -> Sample {
    Sample {
        ts_ms,
        app_name: "timewent".into(),
        bundle_id: "dev.timewent.app".into(),
        window_title: Some("timewent — see where your time went".into()),
        url: None,
        idle: idle_all(0.5),
        locked: false,
        media_active: false,
        audio: None,
    }
}
