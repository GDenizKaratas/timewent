//! Raw observations: one `Sample` per poll. Immutable once recorded.

use serde::{Deserialize, Serialize};

/// One poll of the frontmost context plus input idleness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Unix ms, wall clock at poll time.
    pub ts_ms: i64,
    pub app_name: String,
    pub bundle_id: String,
    pub window_title: Option<String>,
    /// Browsers only, when obtainable.
    pub url: Option<String>,
    pub idle: Idle,
    /// Screen locked / screensaver / loginwindow.
    pub locked: bool,
    /// Some process holds a `PreventUserIdleDisplaySleep` power assertion (video playback, a
    /// call). Absent in older recordings (= false) and omitted when false, so those stay
    /// byte-identical (PLAN §10.1).
    #[serde(default, skip_serializing_if = "is_false")]
    pub media_active: bool,
    /// What is playing sound right now, if anything (§14.2). Absent in older recordings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Audio>,
}

/// The app holding audio output, and what it plays when that can be told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audio {
    pub bundle_id: String,
    pub app: String,
    /// `name — artist` (Spotify / Music) or a media tab's title (browsers).
    pub title: Option<String>,
    /// Browsers: the host of the playing media tab (lowercase, no `www.`).
    pub host: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Seconds since the last input event, per event type. Never event contents.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Idle {
    pub keyboard_s: f64,
    pub mouse_s: f64,
    pub click_s: f64,
    pub scroll_s: f64,
}

impl Idle {
    /// Overall idle: time since the most recent event of any type.
    pub fn min_s(&self) -> f64 {
        self.keyboard_s
            .min(self.mouse_s)
            .min(self.click_s)
            .min(self.scroll_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_s_is_the_most_recent_event_of_any_type() {
        let idle = Idle {
            keyboard_s: 30.0,
            mouse_s: 12.5,
            click_s: 40.0,
            scroll_s: 2.25,
        };
        assert_eq!(idle.min_s(), 2.25);
    }

    #[test]
    fn sample_jsonl_line_round_trips() {
        let sample = Sample {
            ts_ms: 1_700_000_000_000,
            app_name: "Code".into(),
            bundle_id: "com.microsoft.VSCode".into(),
            window_title: Some("main.rs — timewent".into()),
            url: None,
            idle: Idle {
                keyboard_s: 0.5,
                mouse_s: 3.0,
                click_s: 10.0,
                scroll_s: 60.0,
            },
            locked: false,
            media_active: false,
            audio: None,
        };
        let line = serde_json::to_string(&sample).expect("serialize");
        assert!(!line.contains('\n'));
        let back: Sample = serde_json::from_str(&line).expect("deserialize");
        assert_eq!(back, sample);
    }

    #[test]
    fn audio_defaults_to_none_and_is_omitted_when_none() {
        let line = r#"{"ts_ms":1,"app_name":"A","bundle_id":"a","window_title":null,"url":null,"idle":{"keyboard_s":0.0,"mouse_s":0.0,"click_s":0.0,"scroll_s":0.0},"locked":false}"#;
        let s: Sample = serde_json::from_str(line).expect("old line parses");
        assert_eq!(s.audio, None);
        assert_eq!(serde_json::to_string(&s).expect("serialize"), line);
        let playing = Sample {
            audio: Some(Audio {
                bundle_id: "com.spotify.client".into(),
                app: "Spotify".into(),
                title: Some("lofi — ChilledCow".into()),
                host: None,
            }),
            ..s
        };
        let json = serde_json::to_string(&playing).expect("serialize");
        assert!(
            json.ends_with(r#""audio":{"bundle_id":"com.spotify.client","app":"Spotify","title":"lofi — ChilledCow","host":null}}"#),
            "{json}"
        );
        let back: Sample = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, playing);
    }

    #[test]
    fn media_active_defaults_to_false_and_is_omitted_when_false() {
        let line = r#"{"ts_ms":1,"app_name":"A","bundle_id":"a","window_title":null,"url":null,"idle":{"keyboard_s":0.0,"mouse_s":0.0,"click_s":0.0,"scroll_s":0.0},"locked":false}"#;
        let s: Sample = serde_json::from_str(line).expect("old line parses");
        assert!(!s.media_active);
        assert_eq!(serde_json::to_string(&s).expect("serialize"), line);

        let watching = Sample {
            media_active: true,
            ..s
        };
        let json = serde_json::to_string(&watching).expect("serialize");
        assert!(
            json.ends_with(r#""locked":false,"media_active":true}"#),
            "{json}"
        );
        let back: Sample = serde_json::from_str(&json).expect("parse");
        assert!(back.media_active);
    }
}
