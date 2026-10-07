//! Background listening (DESIGN §8.4): audio playing from something other than what you are
//! looking at. A parallel lane — never added to in-use time.

use crate::config::Config;
use crate::context::{is_browser, web_host, Category};
use crate::sample::Sample;
use crate::segment::{ListenTime, Segment};

/// What a sample counts as listening to: `(label, title)`, or `None` (nothing playing, the
/// source is what you are looking at — watching —, a call, or the screen is locked).
pub(crate) fn listen_key(s: &Sample, config: &Config) -> Option<(String, Option<String>)> {
    let a = s.audio.as_ref()?;
    if s.locked {
        return None;
    }
    let host_rule = a.host.as_deref().and_then(|h| config.labels.get(h));
    let category = host_rule
        .map(|r| r.category)
        .or_else(|| config.app_categories.get(&a.bundle_id).copied());
    if category == Some(Category::Comms) {
        return None;
    }
    if a.bundle_id == s.bundle_id {
        // The frontmost app plays it. A browser counts as watching only when the visible tab
        // is the one playing (same host); an unknown tab is assumed visible.
        if !is_browser(&a.bundle_id) {
            return None;
        }
        let front = s.url.as_deref().and_then(web_host);
        if a.host.is_none() || front.as_deref() == a.host.as_deref() {
            return None;
        }
    }
    let label = match (&a.host, host_rule) {
        (Some(_), Some(rule)) => rule.label.clone(),
        (Some(host), None) => host.clone(),
        (None, _) => a.app.clone(),
    };
    Some((label, a.title.clone()))
}

/// Adds each sample's listening time to the segment it falls in. Runs of one source shorter
/// than `glance_max_s` (notification sounds) are dropped; a gap ends a run.
pub(crate) fn assign(samples: &[Sample], config: &Config, segments: &mut [Segment]) {
    let n = samples.len();
    let weight = |i: usize| match samples.get(i + 1) {
        Some(next) if !config.is_gap(samples[i].ts_ms, next.ts_ms) => {
            (next.ts_ms - samples[i].ts_ms).max(0)
        }
        _ => config.poll_ms(),
    };
    let keys: Vec<Option<(String, Option<String>)>> =
        samples.iter().map(|s| listen_key(s, config)).collect();
    let mut i = 0;
    while i < n {
        let Some(key) = &keys[i] else {
            i += 1;
            continue;
        };
        let mut j = i;
        let mut ms = 0;
        loop {
            ms += weight(j);
            let continues = j + 1 < n
                && keys[j + 1].as_ref() == Some(key)
                && !config.is_gap(samples[j].ts_ms, samples[j + 1].ts_ms);
            if !continues {
                break;
            }
            j += 1;
        }
        if ms >= config.glance_ms() {
            for (k, sample) in samples.iter().enumerate().take(j + 1).skip(i) {
                let ts = sample.ts_ms;
                let at = segments.partition_point(|s| s.start_ms <= ts);
                if let Some(seg) = at.checked_sub(1).and_then(|x| segments.get_mut(x)) {
                    add(&mut seg.listening, key, weight(k));
                }
            }
        }
        i = j + 1;
    }
    for seg in segments.iter_mut() {
        seg.listening.sort_by(|a, b| {
            b.ms.cmp(&a.ms)
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.title.cmp(&b.title))
        });
    }
}

fn add(list: &mut Vec<ListenTime>, (label, title): &(String, Option<String>), ms: i64) {
    match list
        .iter_mut()
        .find(|l| &l.label == label && &l.title == title)
    {
        Some(l) => l.ms += ms,
        None => list.push(ListenTime {
            label: label.clone(),
            title: title.clone(),
            ms,
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::sample::{Audio, Sample};
    use crate::segment::segment;
    use crate::summary::{summarize, Listening};
    use crate::testkit::spans::*;

    fn spotify(title: &str) -> Audio {
        Audio {
            bundle_id: "com.spotify.client".into(),
            app: "Spotify".into(),
            title: Some(title.into()),
            host: None,
        }
    }

    fn tab(host: &str, title: &str) -> Audio {
        Audio {
            bundle_id: "com.google.Chrome".into(),
            app: "Google Chrome".into(),
            title: Some(title.into()),
            host: Some(host.into()),
        }
    }

    fn with(mut s: Sample, a: Audio) -> Sample {
        s.audio = Some(a);
        s
    }

    fn listening(spans: &[(Option<Sample>, i64)]) -> Vec<Listening> {
        summarize(&segment(&timeline(spans), &Config::default())).listening
    }

    fn row(label: &str, title: Option<&str>, ms: i64) -> Listening {
        Listening {
            label: label.into(),
            title: title.map(String::from),
            ms,
        }
    }

    #[test]
    fn music_behind_work_is_listening_and_not_in_use() {
        let spans = [
            (Some(with(code("p"), spotify("lofi — ChilledCow"))), 600),
            (Some(code("p")), 60),
        ];
        assert_eq!(
            listening(&spans),
            [row("Spotify", Some("lofi — ChilledCow"), 600_000)]
        );
        let s = summarize(&segment(&timeline(&spans), &Config::default()));
        assert_eq!(s.total_ms, 660_000, "listening never adds to in-use");
    }

    #[test]
    fn the_frontmost_source_is_watching_not_listening() {
        let yt = browser("https://www.youtube.com/watch?v=1", "cats");
        assert!(listening(&[(Some(with(yt, tab("youtube.com", "cats"))), 120)]).is_empty());
        let spotify_front = Sample {
            app_name: "Spotify".into(),
            bundle_id: "com.spotify.client".into(),
            window_title: None,
            url: None,
            ..code("x")
        };
        assert!(listening(&[(Some(with(spotify_front, spotify("t"))), 60)]).is_empty());
    }

    #[test]
    fn a_media_tab_behind_another_tab_of_the_same_browser_is_listening() {
        let s = with(docs(), tab("youtube.com", "lofi beats to code to"));
        assert_eq!(
            listening(&[(Some(s), 60)]),
            [row("YouTube", Some("lofi beats to code to"), 60_000)]
        );
    }

    #[test]
    fn short_runs_are_dropped_as_notification_sounds() {
        let ping = Audio {
            bundle_id: "com.apple.finder".into(),
            app: "Finder".into(),
            title: None,
            host: None,
        };
        let spans = [
            (Some(code("p")), 20),
            (Some(with(code("p"), ping)), 9),
            (Some(code("p")), 20),
            (Some(with(code("p"), spotify("song"))), 10),
        ];
        assert_eq!(listening(&spans), [row("Spotify", Some("song"), 10_000)]);
    }

    #[test]
    fn calls_are_not_listening() {
        let zoom = Audio {
            bundle_id: "us.zoom.xos".into(),
            app: "zoom.us".into(),
            title: None,
            host: None,
        };
        let meet = tab("meet.google.com", "standup");
        assert!(listening(&[(Some(with(code("p"), zoom)), 60)]).is_empty());
        assert!(listening(&[(Some(with(code("p"), meet)), 60)]).is_empty());
    }

    #[test]
    fn locked_samples_and_gaps_are_not_listening() {
        let spans = [
            (Some(with(away(code("p")), spotify("song"))), 60),
            (Some(with(code("p"), spotify("song"))), 6),
            (None, 600),
            (Some(with(code("p"), spotify("song"))), 6),
        ];
        // The two 6s runs are separated by a gap: each is under the 10s floor.
        assert!(listening(&spans).is_empty());
    }

    #[test]
    fn tracks_are_listed_separately_sorted_by_time() {
        let spans = [
            (Some(with(code("p"), spotify("a"))), 30),
            (Some(with(code("p"), spotify("b"))), 90),
            (Some(with(code("p"), spotify("a"))), 30),
            (Some(with(code("p"), tab("open.spotify.com", "c"))), 20),
        ];
        assert_eq!(
            listening(&spans),
            [
                row("Spotify", Some("b"), 90_000),
                row("Spotify", Some("a"), 60_000),
                row("Spotify", Some("c"), 20_000),
            ]
        );
    }

    #[test]
    fn unknown_title_groups_under_the_source() {
        let a = Audio {
            title: None,
            ..spotify("x")
        };
        assert_eq!(
            listening(&[(Some(with(code("p"), a)), 30)]),
            [row("Spotify", None, 30_000)]
        );
    }
}
