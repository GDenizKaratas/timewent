//! What is playing (PLAN §14.2). Pure: who holds audio output comes in, one source and how to
//! ask it for a title go out. The OS reads live in `mac::power` / `mac::workspace`.

use timewent_core::{web_host, Category, Config};

use crate::url_cache::QueryFailed;

/// A process holding an `audio-out` power assertion (on whose behalf coreaudiod plays).
#[derive(Debug, Clone, PartialEq)]
pub struct Holder {
    pub pid: i32,
    pub bundle_id: String,
    pub app: String,
    /// When its assertion started (any monotonic-ish seconds; only compared).
    pub started_s: f64,
}

/// Dedicated players: their title is the track.
const SPOTIFY: &str = "com.spotify.client";
const MUSIC: &str = "com.apple.Music";
const PLAYERS: &[&str] = &[
    SPOTIFY,
    MUSIC,
    "com.apple.podcasts",
    "org.videolan.vlc",
    "com.colliderli.iina",
];
const CHROMIUM: &[&str] = &[
    "com.google.Chrome",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "com.microsoft.edgemac",
];
const SAFARI: &str = "com.apple.Safari";
const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "com.apple.Safari",
    "org.mozilla.firefox",
];

/// Decision Q4: one source per sample. Not the frontmost app first (that is watching), then
/// dedicated players, then browsers, then anything else; ties → playing longest, then pid.
pub fn pick<'a>(holders: &'a [Holder], frontmost_bundle: &str) -> Option<&'a Holder> {
    let rank = |h: &Holder| {
        let class = if PLAYERS.contains(&h.bundle_id.as_str()) {
            0
        } else if BROWSERS.contains(&h.bundle_id.as_str()) {
            1
        } else {
            2
        };
        (h.bundle_id == frontmost_bundle, class)
    };
    holders.iter().min_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.started_s.total_cmp(&b.started_s))
            .then_with(|| a.pid.cmp(&b.pid))
    })
}

/// Hosts whose tabs count as media (labels with category media).
pub fn media_hosts(config: &Config) -> Vec<String> {
    config
        .labels
        .iter()
        .filter(|(_, r)| r.category == Category::Media)
        .map(|(h, _)| h.clone())
        .collect()
}

fn is_media_host(host: &str, media: &[String]) -> bool {
    media
        .iter()
        .any(|m| host == m || host.ends_with(&format!(".{m}")))
}

/// The AppleScript that asks a source what it plays; `None` = nothing to ask (title stays
/// unknown). Addressed by bundle id and guarded by `is running`, like the url scripts.
pub fn script_for(bundle_id: &str, media: &[String]) -> Option<String> {
    let body = if bundle_id == SPOTIFY || bundle_id == MUSIC {
        "if player state is playing then return (name of current track) & \" — \" & (artist of current track)".to_string()
    } else if CHROMIUM.contains(&bundle_id) || bundle_id == SAFARI {
        let (url, title) = if bundle_id == SAFARI {
            ("URL", "name")
        } else {
            ("URL", "title")
        };
        let test = media
            .iter()
            .map(|h| format!("u contains \"{h}\""))
            .collect::<Vec<_>>()
            .join(" or ");
        if test.is_empty() {
            return None;
        }
        format!(
            "repeat with w in windows\n\
             \t\t\trepeat with t in tabs of w\n\
             \t\t\t\tset u to {url} of t\n\
             \t\t\t\tif {test} then return u & linefeed & ({title} of t)\n\
             \t\t\tend repeat\n\
             \t\tend repeat"
        )
    } else {
        return None;
    };
    Some(format!(
        "if application id \"{bundle_id}\" is running then\n\
         \ttell application id \"{bundle_id}\"\n\
         \t\t{body}\n\
         \tend tell\n\
         end if\n\
         return \"\"\n"
    ))
}

/// What a source plays: `(title, host)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Playing {
    pub title: Option<String>,
    pub host: Option<String>,
}

/// `osascript` stdout → what plays. Players answer `name — artist`; browsers answer
/// `url⏎title`, and only a real media host counts (`contains` in the script is a pre-filter).
pub fn parse_output(bundle_id: &str, stdout: &str, media: &[String]) -> Playing {
    let out = stdout.trim_end_matches(['\n', '\r']);
    let clean = |s: &str| {
        let s = s.trim();
        (!s.is_empty() && s != "missing value").then(|| s.to_string())
    };
    if !BROWSERS.contains(&bundle_id) {
        return Playing {
            title: clean(out),
            host: None,
        };
    }
    let mut lines = out.splitn(2, '\n');
    let url = lines.next().unwrap_or("");
    let title = lines.next().and_then(clean);
    match web_host(url.trim()) {
        Some(host) if is_media_host(&host, media) => Playing {
            title,
            host: Some(host),
        },
        _ => Playing::default(),
    }
}

/// Re-ask a source when it changes or after this long (a playlist moves on).
pub const REFRESH_MS: u64 = 30_000;

/// When to ask the audio source for its title. Same shape as `UrlCache`: injected clock and
/// query, per-app backoff on failure.
#[derive(Debug, Default)]
pub struct AudioCache {
    last: Option<(String, i32, Playing, u64)>,
    backoff_until: Vec<(String, u64)>,
}

impl AudioCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(
        &mut self,
        holder: &Holder,
        media: &[String],
        now_ms: u64,
        query: impl FnOnce(&str) -> Result<String, QueryFailed>,
    ) -> Playing {
        if let Some((b, pid, playing, at)) = &self.last {
            if *b == holder.bundle_id
                && *pid == holder.pid
                && now_ms.saturating_sub(*at) < REFRESH_MS
            {
                return playing.clone();
            }
        }
        let Some(script) = script_for(&holder.bundle_id, media) else {
            self.last = Some((
                holder.bundle_id.clone(),
                holder.pid,
                Playing::default(),
                now_ms,
            ));
            return Playing::default();
        };
        if self
            .backoff_until
            .iter()
            .any(|(b, until)| *b == holder.bundle_id && now_ms < *until)
        {
            return Playing::default();
        }
        let playing = match query(&script) {
            Ok(out) => {
                self.backoff_until.retain(|(b, _)| *b != holder.bundle_id);
                parse_output(&holder.bundle_id, &out, media)
            }
            Err(e) => {
                self.backoff_until.retain(|(b, _)| *b != holder.bundle_id);
                self.backoff_until
                    .push((holder.bundle_id.clone(), now_ms + e.backoff_ms()));
                Playing::default()
            }
        };
        self.last = Some((
            holder.bundle_id.clone(),
            holder.pid,
            playing.clone(),
            now_ms,
        ));
        playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pid: i32, bundle: &str, started_s: f64) -> Holder {
        Holder {
            pid,
            bundle_id: bundle.into(),
            app: bundle.into(),
            started_s,
        }
    }

    fn media() -> Vec<String> {
        media_hosts(&Config::default())
    }

    #[test]
    fn pick_prefers_background_then_players_then_browsers_then_longest() {
        let hs = [
            h(1, "com.google.Chrome", 10.0),
            h(2, SPOTIFY, 50.0),
            h(3, "com.other.game", 1.0),
        ];
        assert_eq!(pick(&hs, "com.microsoft.VSCode").map(|h| h.pid), Some(2));
        // Spotify is frontmost: it is being watched, the background browser wins.
        assert_eq!(pick(&hs, SPOTIFY).map(|h| h.pid), Some(1));
        let two_browsers = [
            h(4, "com.apple.Safari", 30.0),
            h(5, "com.google.Chrome", 20.0),
        ];
        assert_eq!(
            pick(&two_browsers, "x").map(|h| h.pid),
            Some(5),
            "playing longest"
        );
        assert_eq!(pick(&[], "x"), None);
        // Only the frontmost app plays: still reported (core decides it is watching).
        assert_eq!(pick(&[h(6, SPOTIFY, 0.0)], SPOTIFY).map(|h| h.pid), Some(6));
    }

    #[test]
    fn media_hosts_come_from_labels() {
        let m = media();
        assert!(m.contains(&"youtube.com".to_string()));
        assert!(m.contains(&"netflix.com".to_string()));
        assert!(!m.contains(&"github.com".to_string()));
    }

    #[test]
    fn scripts_exist_for_players_and_scriptable_browsers_only() {
        let s = script_for(SPOTIFY, &media()).expect("spotify");
        assert!(
            s.contains("application id \"com.spotify.client\" is running"),
            "{s}"
        );
        assert!(s.contains("name of current track"), "{s}");
        let c = script_for("com.google.Chrome", &media()).expect("chrome");
        assert!(c.contains("u contains \"youtube.com\""), "{c}");
        assert!(c.contains("title of t"), "{c}");
        assert!(script_for(SAFARI, &media())
            .expect("safari")
            .contains("name of t"));
        assert_eq!(script_for("org.mozilla.firefox", &media()), None);
        assert_eq!(script_for("com.other.game", &media()), None);
    }

    #[test]
    fn player_output_is_the_track() {
        assert_eq!(
            parse_output(SPOTIFY, "lofi — ChilledCow\n", &media()),
            Playing {
                title: Some("lofi — ChilledCow".into()),
                host: None
            }
        );
        assert_eq!(parse_output(SPOTIFY, "\n", &media()), Playing::default());
    }

    #[test]
    fn browser_output_needs_a_real_media_host() {
        let m = media();
        assert_eq!(
            parse_output(
                "com.google.Chrome",
                "https://www.youtube.com/watch?v=1\nlofi beats\n",
                &m
            ),
            Playing {
                title: Some("lofi beats".into()),
                host: Some("youtube.com".into())
            }
        );
        // `contains` pre-filter false positive: not a media host.
        assert_eq!(
            parse_output(
                "com.google.Chrome",
                "https://notyoutube.com.example.org/\nx\n",
                &m
            ),
            Playing::default()
        );
        assert_eq!(
            parse_output("com.google.Chrome", "", &m),
            Playing::default()
        );
    }

    #[test]
    fn cache_asks_on_change_or_every_30s_and_backs_off_on_failure() {
        let m = media();
        let mut c = AudioCache::new();
        let spotify = h(2, SPOTIFY, 0.0);
        let mut asked = 0;
        let mut ask = |t: u64, c: &mut AudioCache, h: &Holder, r: Result<String, QueryFailed>| {
            c.resolve(h, &m, t, |_| {
                asked += 1;
                r
            })
        };
        assert_eq!(
            ask(0, &mut c, &spotify, Ok("a — b".into()))
                .title
                .as_deref(),
            Some("a — b")
        );
        assert_eq!(
            ask(29_999, &mut c, &spotify, Ok("x".into()))
                .title
                .as_deref(),
            Some("a — b")
        );
        assert_eq!(
            ask(30_000, &mut c, &spotify, Ok("c — d".into()))
                .title
                .as_deref(),
            Some("c — d")
        );
        let chrome = h(5, "com.google.Chrome", 0.0);
        assert_eq!(
            ask(30_001, &mut c, &chrome, Err(QueryFailed::Denied)),
            Playing::default()
        );
        // Denied: no new ask for 30s, even though the holder keeps changing back and forth.
        assert_eq!(
            ask(40_000, &mut c, &chrome, Ok("x".into())),
            Playing::default()
        );
        drop(ask);
        assert_eq!(
            asked, 3,
            "the cached and backed-off calls never ran the query"
        );
    }

    #[test]
    fn sources_without_a_script_are_not_asked() {
        let mut c = AudioCache::new();
        let p = c.resolve(&h(9, "com.other.game", 0.0), &media(), 0, |_| {
            panic!("must not ask")
        });
        assert_eq!(p, Playing::default());
    }
}
