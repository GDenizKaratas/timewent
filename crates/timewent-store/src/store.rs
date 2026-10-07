use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use timewent_core::{Audio, Idle, Sample, SessionMeta};

use crate::error::{Error, Result};
use crate::idle_ms::{ms_to_secs, secs_to_ms};
use crate::schema;

pub type SessionId = i64;

/// One context's sample count over a time range (see [`Store::context_counts`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCount {
    pub bundle_id: String,
    pub app_name: String,
    pub url: Option<String>,
    pub samples: i64,
    pub last_ts_ms: i64,
}

/// Raw sample storage. One `Store` per database file; not `Sync` — wrap it in a mutex to share.
pub struct Store {
    conn: Connection,
    /// The context of the previous append. At 1 Hz the frontmost context rarely changes, so
    /// this turns most appends into one INSERT. Context rows are never deleted, so a cached
    /// id cannot go stale.
    last_context: Option<(ContextIdentity, i64)>,
    /// Same for the audio source.
    last_audio: Option<(Audio, i64)>,
}

#[derive(Debug, PartialEq)]
struct ContextIdentity {
    bundle_id: String,
    app_name: String,
    window_title: Option<String>,
    url: Option<String>,
}

impl ContextIdentity {
    fn of(s: &Sample) -> Self {
        Self {
            bundle_id: s.bundle_id.clone(),
            app_name: s.app_name.clone(),
            window_title: s.window_title.clone(),
            url: s.url.clone(),
        }
    }

    fn matches(&self, s: &Sample) -> bool {
        self.bundle_id == s.bundle_id
            && self.app_name == s.app_name
            && self.window_title == s.window_title
            && self.url == s.url
    }
}

const SAMPLE_COLUMNS: &str = "s.ts_ms, c.app_name, c.bundle_id, c.window_title, c.url,
    s.idle_kb_ms, s.idle_mouse_ms, s.idle_click_ms, s.idle_scroll_ms, s.locked, s.media_active,
    a.bundle_id, a.app, a.title, a.host
    FROM samples s JOIN contexts c ON c.id = s.context_id
    LEFT JOIN audio a ON a.id = s.audio_id";

impl Store {
    /// Opens (creating if needed) a database file, in WAL mode with `synchronous=NORMAL`:
    /// a 1 Hz append then costs no fsync, and a power loss can lose at most the last few
    /// samples, never corrupt the file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            conn,
            last_context: None,
            last_audio: None,
        })
    }

    /// Errors with `SessionAlreadyOpen` if a session is open (the store holds at most one).
    pub fn start_session(&mut self, ts_ms: i64) -> Result<SessionId> {
        if let Some(open) = self.open_session()? {
            return Err(Error::SessionAlreadyOpen(open.id));
        }
        self.conn.execute(
            "INSERT INTO sessions (started_at_ms) VALUES (?1)",
            params![ts_ms],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_session(&mut self, id: SessionId, ts_ms: i64) -> Result<()> {
        let meta = self.session(id)?.ok_or(Error::NoSuchSession(id))?;
        if meta.ended_at_ms.is_some() {
            return Err(Error::SessionAlreadyEnded(id));
        }
        if ts_ms < meta.started_at_ms {
            return Err(Error::EndBeforeStart {
                id,
                started_at_ms: meta.started_at_ms,
                ended_at_ms: ts_ms,
            });
        }
        self.conn.execute(
            "UPDATE sessions SET ended_at_ms = ?2 WHERE id = ?1",
            params![id, ts_ms],
        )?;
        Ok(())
    }

    pub fn open_session(&self) -> Result<Option<SessionMeta>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, started_at_ms, ended_at_ms FROM sessions WHERE ended_at_ms IS NULL",
                [],
                session_meta,
            )
            .optional()?)
    }

    /// Crash recovery: ends a session left open by a previous run at its last sample's
    /// `ts + poll_interval_ms` (the time that sample represents), or at its start if it has
    /// no samples. Returns the closed session, or `None` if nothing was open.
    pub fn close_dangling(&mut self, poll_interval_ms: u32) -> Result<Option<SessionMeta>> {
        let Some(open) = self.open_session()? else {
            return Ok(None);
        };
        let last_ts: Option<i64> = self.conn.query_row(
            "SELECT max(ts_ms) FROM samples WHERE session_id = ?1",
            params![open.id],
            |r| r.get(0),
        )?;
        let end = last_ts.map_or(open.started_at_ms, |ts| {
            (ts + i64::from(poll_interval_ms)).max(open.started_at_ms)
        });
        self.end_session(open.id, end)?;
        Ok(Some(SessionMeta {
            ended_at_ms: Some(end),
            ..open
        }))
    }

    /// The hot path, called once per poll.
    pub fn append(&mut self, session_id: SessionId, sample: &Sample) -> Result<()> {
        let context_id = self.context_id(sample)?;
        let audio_id = match &sample.audio {
            Some(a) => Some(self.audio_id(a)?),
            None => None,
        };
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO samples (session_id, ts_ms, context_id, idle_kb_ms, idle_mouse_ms,
                idle_click_ms, idle_scroll_ms, locked, media_active, audio_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        let idle = &sample.idle;
        stmt.execute(params![
            session_id,
            sample.ts_ms,
            context_id,
            secs_to_ms(idle.keyboard_s),
            secs_to_ms(idle.mouse_s),
            secs_to_ms(idle.click_s),
            secs_to_ms(idle.scroll_s),
            sample.locked,
            sample.media_active,
            audio_id,
        ])?;
        Ok(())
    }

    /// Insert-or-select against `audio_identity`, behind a one-entry cache (a track plays for
    /// minutes, so most samples reuse the previous row).
    fn audio_id(&mut self, a: &Audio) -> Result<i64> {
        if let Some((cached, id)) = &self.last_audio {
            if cached == a {
                return Ok(*id);
            }
        }
        let existing = self
            .conn
            .prepare_cached(
                "SELECT id FROM audio WHERE bundle_id = ?1 AND app = ?2
                   AND coalesce(title, 0) = coalesce(?3, 0)
                   AND coalesce(host, 0) = coalesce(?4, 0)",
            )?
            .query_row(params![a.bundle_id, a.app, a.title, a.host], |r| r.get(0))
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO audio (bundle_id, app, title, host) VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![a.bundle_id, a.app, a.title, a.host])?;
                self.conn.last_insert_rowid()
            }
        };
        self.last_audio = Some((a.clone(), id));
        Ok(id)
    }

    /// Samples of one session, oldest first. Unknown session → empty.
    pub fn samples(&self, session_id: SessionId) -> Result<Vec<Sample>> {
        let sql = format!("SELECT {SAMPLE_COLUMNS} WHERE s.session_id = ?1 ORDER BY s.ts_ms");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![session_id], sample_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Samples of one session with `ts_ms > after_ms`, oldest first: what a live view has not
    /// seen yet.
    pub fn samples_after(&self, session_id: SessionId, after_ms: i64) -> Result<Vec<Sample>> {
        let sql = format!(
            "SELECT {SAMPLE_COLUMNS} WHERE s.session_id = ?1 AND s.ts_ms > ?2 ORDER BY s.ts_ms"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![session_id, after_ms], sample_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Per context, how many unlocked samples were taken since `from_ms` and when the last
    /// one was: the raw material for "apps and sites you used" (PLAN §13.1).
    pub fn context_counts(&self, from_ms: i64) -> Result<Vec<ContextCount>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT c.bundle_id, c.app_name, c.url, count(*), max(s.ts_ms)
             FROM samples s JOIN contexts c ON c.id = s.context_id
             WHERE s.ts_ms >= ?1 AND s.locked = 0
             GROUP BY c.id",
        )?;
        let rows = stmt.query_map(params![from_ms], |r| {
            Ok(ContextCount {
                bundle_id: r.get(0)?,
                app_name: r.get(1)?,
                url: r.get(2)?,
                samples: r.get(3)?,
                last_ts_ms: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Samples with `from_ms <= ts_ms < to_ms` across all sessions, oldest first.
    pub fn samples_between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<Sample>> {
        let sql = format!(
            "SELECT {SAMPLE_COLUMNS} WHERE s.ts_ms >= ?1 AND s.ts_ms < ?2
             ORDER BY s.ts_ms, s.session_id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![from_ms, to_ms], sample_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Newest first.
    pub fn sessions(&self, limit: u32) -> Result<Vec<SessionMeta>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, started_at_ms, ended_at_ms FROM sessions
             ORDER BY started_at_ms DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], session_meta)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Newest first, `limit` sessions after skipping `offset` (paging, PLAN §18).
    pub fn sessions_page(&self, limit: u32, offset: u32) -> Result<Vec<SessionMeta>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, started_at_ms, ended_at_ms FROM sessions
             ORDER BY started_at_ms DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], session_meta)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Deletes a closed session and its raw samples for good, then the contexts and audio
    /// sources no remaining sample uses — all in one transaction (PLAN §18). The session
    /// being recorded cannot be deleted.
    pub fn delete_session(&mut self, id: SessionId) -> Result<()> {
        let meta = self.session(id)?.ok_or(Error::NoSuchSession(id))?;
        if meta.ended_at_ms.is_none() {
            return Err(Error::SessionOpen(id));
        }
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM samples WHERE session_id = ?1", params![id])?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM contexts WHERE id NOT IN (SELECT context_id FROM samples)",
            [],
        )?;
        tx.execute(
            "DELETE FROM audio WHERE id NOT IN
               (SELECT audio_id FROM samples WHERE audio_id IS NOT NULL)",
            [],
        )?;
        tx.commit()?;
        // The cached ids may have just been deleted: never hand them out again.
        self.last_context = None;
        self.last_audio = None;
        Ok(())
    }

    /// Distinct contexts stored (diagnostics; tests of cleanup).
    pub fn context_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM contexts", [], |r| r.get(0))?)
    }

    /// Distinct audio sources stored (diagnostics; tests of cleanup).
    pub fn audio_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM audio", [], |r| r.get(0))?)
    }

    fn session(&self, id: SessionId) -> Result<Option<SessionMeta>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, started_at_ms, ended_at_ms FROM sessions WHERE id = ?1",
                params![id],
                session_meta,
            )
            .optional()?)
    }

    /// Insert-or-select against the `contexts_identity` index, behind a one-entry cache.
    fn context_id(&mut self, s: &Sample) -> Result<i64> {
        if let Some((identity, id)) = &self.last_context {
            if identity.matches(s) {
                return Ok(*id);
            }
        }
        // The WHERE clause repeats the index expressions so SQLite can use the index.
        let existing = self
            .conn
            .prepare_cached(
                "SELECT id FROM contexts WHERE bundle_id = ?1 AND app_name = ?2
                   AND coalesce(window_title, 0) = coalesce(?3, 0)
                   AND coalesce(url, 0) = coalesce(?4, 0)",
            )?
            .query_row(
                params![s.bundle_id, s.app_name, s.window_title, s.url],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO contexts (bundle_id, app_name, window_title, url)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![s.bundle_id, s.app_name, s.window_title, s.url])?;
                self.conn.last_insert_rowid()
            }
        };
        self.last_context = Some((ContextIdentity::of(s), id));
        Ok(id)
    }
}

fn session_meta(r: &Row) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        id: r.get(0)?,
        started_at_ms: r.get(1)?,
        ended_at_ms: r.get(2)?,
    })
}

fn sample_row(r: &Row) -> rusqlite::Result<Sample> {
    Ok(Sample {
        ts_ms: r.get(0)?,
        app_name: r.get(1)?,
        bundle_id: r.get(2)?,
        window_title: r.get(3)?,
        url: r.get(4)?,
        idle: Idle {
            keyboard_s: ms_to_secs(r.get(5)?),
            mouse_s: ms_to_secs(r.get(6)?),
            click_s: ms_to_secs(r.get(7)?),
            scroll_s: ms_to_secs(r.get(8)?),
        },
        locked: r.get(9)?,
        media_active: r.get(10)?,
        audio: match r.get::<_, Option<String>>(11)? {
            Some(bundle_id) => Some(Audio {
                bundle_id,
                app: r.get(12)?,
                title: r.get(13)?,
                host: r.get(14)?,
            }),
            None => None,
        },
    })
}
