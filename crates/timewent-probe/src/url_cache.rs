//! When to ask a browser for its url (DESIGN §3.2). Pure: the clock and the query are injected.
//!
//! Asking costs an `osascript` process (~tens of ms), so it happens only when the
//! (bundle id, window title) pair differs from the last answered one. A failed ask backs that
//! browser off: a denied Automation permission or a script error for [`FAILURE_BACKOFF_MS`], so
//! neither can spawn a process every second; a timeout (often just a cold browser) only for
//! [`TIMEOUT_BACKOFF_MS`].

use crate::url_script::script_for;

pub const FAILURE_BACKOFF_MS: u64 = 30_000;
pub const TIMEOUT_BACKOFF_MS: u64 = 5_000;

/// Without Accessibility the title is always `None`, so the (bundle, title) key never changes
/// while the user navigates. Re-ask at this period instead of trusting a stale url forever.
pub const UNTITLED_REFRESH_MS: u64 = 5_000;

/// The browser was asked but could not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFailed {
    /// No answer within the deadline.
    TimedOut,
    /// Automation permission missing or refused.
    Denied,
    /// Anything else: script error, spawn failure.
    Error,
}

impl QueryFailed {
    pub fn backoff_ms(self) -> u64 {
        match self {
            QueryFailed::TimedOut => TIMEOUT_BACKOFF_MS,
            QueryFailed::Denied | QueryFailed::Error => FAILURE_BACKOFF_MS,
        }
    }
}

#[derive(Debug, Default)]
pub struct UrlCache {
    last: Option<Answer>,
    /// (bundle id, monotonic ms until which it is not asked). A handful of browsers at most.
    backoff_until: Vec<(String, u64)>,
}

#[derive(Debug)]
struct Answer {
    bundle_id: String,
    title: Option<String>,
    url: Option<String>,
    at_ms: u64,
}

impl UrlCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The url for the frontmost window. `now_ms` must be monotonic. `query` runs the given
    /// AppleScript and is only called when the policy allows asking.
    pub fn resolve(
        &mut self,
        bundle_id: &str,
        title: Option<&str>,
        now_ms: u64,
        query: impl FnOnce(&str) -> Result<Option<String>, QueryFailed>,
    ) -> Option<String> {
        let script = script_for(bundle_id)?;
        if let Some(last) = &self.last {
            let same_key = last.bundle_id == bundle_id && last.title.as_deref() == title;
            let fresh = title.is_some() || now_ms.saturating_sub(last.at_ms) < UNTITLED_REFRESH_MS;
            if same_key && fresh {
                return last.url.clone();
            }
        }
        if self.backing_off(bundle_id, now_ms) {
            return None;
        }
        match query(&script) {
            Ok(url) => {
                self.backoff_until.retain(|(b, _)| b != bundle_id);
                self.last = Some(Answer {
                    bundle_id: bundle_id.to_string(),
                    title: title.map(String::from),
                    url: url.clone(),
                    at_ms: now_ms,
                });
                url
            }
            Err(failed) => {
                self.backoff_until.retain(|(b, _)| b != bundle_id);
                let until = now_ms.saturating_add(failed.backoff_ms());
                self.backoff_until.push((bundle_id.to_string(), until));
                None
            }
        }
    }

    fn backing_off(&self, bundle_id: &str, now_ms: u64) -> bool {
        self.backoff_until
            .iter()
            .any(|(b, until)| b == bundle_id && now_ms < *until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const CHROME: &str = "com.google.Chrome";
    const SAFARI: &str = "com.apple.Safari";

    /// Runs `resolve` with a query that answers `answer` and counts calls in `calls`.
    fn ask(
        cache: &mut UrlCache,
        calls: &Cell<u32>,
        bundle: &str,
        title: Option<&str>,
        now_ms: u64,
        answer: Result<Option<&str>, QueryFailed>,
    ) -> Option<String> {
        cache.resolve(bundle, title, now_ms, |_| {
            calls.set(calls.get() + 1);
            answer.map(|u| u.map(String::from))
        })
    }

    #[test]
    fn non_browsers_are_never_queried() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        assert_eq!(
            ask(
                &mut c,
                &n,
                "com.microsoft.VSCode",
                Some("t"),
                0,
                Ok(Some("x"))
            ),
            None
        );
        assert_eq!(
            ask(
                &mut c,
                &n,
                "org.mozilla.firefox",
                Some("t"),
                0,
                Ok(Some("x"))
            ),
            None
        );
        assert_eq!(n.get(), 0);
    }

    #[test]
    fn same_browser_and_title_reuses_the_cached_url() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        let ok = Ok(Some("https://a.com/"));
        assert_eq!(
            ask(&mut c, &n, CHROME, Some("A"), 0, ok).as_deref(),
            Some("https://a.com/")
        );
        for t in 1..100 {
            let got = ask(&mut c, &n, CHROME, Some("A"), t * 1000, Ok(Some("other")));
            assert_eq!(got.as_deref(), Some("https://a.com/"));
        }
        assert_eq!(n.get(), 1);
    }

    #[test]
    fn a_title_change_triggers_a_new_query() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Ok(Some("https://a.com/")));
        let got = ask(
            &mut c,
            &n,
            CHROME,
            Some("B"),
            1000,
            Ok(Some("https://b.com/")),
        );
        assert_eq!(got.as_deref(), Some("https://b.com/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn a_browser_change_with_the_same_title_triggers_a_new_query() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Ok(Some("https://a.com/")));
        ask(
            &mut c,
            &n,
            SAFARI,
            Some("A"),
            1000,
            Ok(Some("https://s.com/")),
        );
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn visiting_a_non_browser_keeps_the_browser_answer() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Ok(Some("https://a.com/")));
        ask(&mut c, &n, "com.apple.finder", Some("x"), 1000, Ok(None));
        let back = ask(&mut c, &n, CHROME, Some("A"), 2000, Ok(Some("other")));
        assert_eq!(back.as_deref(), Some("https://a.com/"));
        assert_eq!(n.get(), 1);
    }

    #[test]
    fn a_no_url_answer_is_cached_without_backoff() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        assert_eq!(ask(&mut c, &n, CHROME, Some("A"), 0, Ok(None)), None);
        assert_eq!(
            ask(&mut c, &n, CHROME, Some("A"), 1000, Ok(Some("u"))),
            None
        );
        assert_eq!(n.get(), 1);
        let got = ask(&mut c, &n, CHROME, Some("B"), 2000, Ok(Some("https://b/")));
        assert_eq!(got.as_deref(), Some("https://b/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn permission_denial_backs_off_that_browser_for_thirty_seconds() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        assert_eq!(
            ask(&mut c, &n, CHROME, Some("A"), 0, Err(QueryFailed::Denied)),
            None
        );
        // Title changes every second during backoff: still no process spawned.
        for t in 1..30 {
            let title = format!("T{t}");
            let got = ask(&mut c, &n, CHROME, Some(&title), t * 1000, Ok(Some("u")));
            assert_eq!(got, None);
        }
        assert_eq!(n.get(), 1);
        let got = ask(
            &mut c,
            &n,
            CHROME,
            Some("A"),
            FAILURE_BACKOFF_MS,
            Ok(Some("https://a/")),
        );
        assert_eq!(got.as_deref(), Some("https://a/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn a_script_error_backs_off_for_thirty_seconds() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Err(QueryFailed::Error));
        let during = ask(&mut c, &n, CHROME, Some("B"), 29_999, Ok(Some("u")));
        assert_eq!((during, n.get()), (None, 1));
        let after = ask(
            &mut c,
            &n,
            CHROME,
            Some("B"),
            30_000,
            Ok(Some("https://b/")),
        );
        assert_eq!(after.as_deref(), Some("https://b/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn a_timeout_backs_off_for_only_five_seconds() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Err(QueryFailed::TimedOut));
        let during = ask(&mut c, &n, CHROME, Some("B"), 4_999, Ok(Some("u")));
        assert_eq!((during, n.get()), (None, 1));
        let after = ask(&mut c, &n, CHROME, Some("B"), 5_000, Ok(Some("https://b/")));
        assert_eq!(after.as_deref(), Some("https://b/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn backoff_durations_match_the_policy() {
        assert_eq!(QueryFailed::TimedOut.backoff_ms(), 5_000);
        assert_eq!(QueryFailed::Denied.backoff_ms(), 30_000);
        assert_eq!(QueryFailed::Error.backoff_ms(), 30_000);
    }

    #[test]
    fn backoff_is_per_browser() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Err(QueryFailed::Denied));
        let got = ask(&mut c, &n, SAFARI, Some("A"), 1000, Ok(Some("https://s/")));
        assert_eq!(got.as_deref(), Some("https://s/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn repeated_failures_keep_backing_off() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Err(QueryFailed::Denied));
        ask(
            &mut c,
            &n,
            CHROME,
            Some("A"),
            FAILURE_BACKOFF_MS,
            Err(QueryFailed::Denied),
        );
        assert_eq!(
            ask(
                &mut c,
                &n,
                CHROME,
                Some("A"),
                FAILURE_BACKOFF_MS + 1000,
                Ok(Some("u"))
            ),
            None
        );
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn a_known_answer_is_still_reused_during_backoff() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, Some("A"), 0, Ok(Some("https://a/")));
        ask(
            &mut c,
            &n,
            CHROME,
            Some("B"),
            1000,
            Err(QueryFailed::Denied),
        );
        let back = ask(&mut c, &n, CHROME, Some("A"), 2000, Ok(Some("x")));
        assert_eq!(back.as_deref(), Some("https://a/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn untitled_windows_are_re_asked_periodically() {
        let (mut c, n) = (UrlCache::new(), Cell::new(0));
        ask(&mut c, &n, CHROME, None, 0, Ok(Some("https://a/")));
        let soon = ask(
            &mut c,
            &n,
            CHROME,
            None,
            UNTITLED_REFRESH_MS - 1,
            Ok(Some("b")),
        );
        assert_eq!(soon.as_deref(), Some("https://a/"));
        let later = ask(
            &mut c,
            &n,
            CHROME,
            None,
            UNTITLED_REFRESH_MS,
            Ok(Some("https://b/")),
        );
        assert_eq!(later.as_deref(), Some("https://b/"));
        assert_eq!(n.get(), 2);
    }

    #[test]
    fn the_query_receives_the_browser_script() {
        let mut c = UrlCache::new();
        let mut seen = String::new();
        c.resolve(SAFARI, Some("A"), 0, |s| {
            seen = s.to_string();
            Ok(None)
        });
        assert_eq!(Some(seen), script_for(SAFARI));
    }
}
