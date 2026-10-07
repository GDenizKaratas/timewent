//! Schema migrations keyed by `PRAGMA user_version`: migration `i` takes the database from
//! version `i` to `i + 1`. Append new migrations; never edit a shipped one.

use rusqlite::Connection;

use crate::error::{Error, Result};

const MIGRATIONS: &[&str] = &[V1, V2, V3];

/// v1 — PLAN §4, with two deliberate additions:
/// - `contexts` dedup uses an expression index instead of `UNIQUE(...)`, because SQLite
///   treats NULLs as distinct in UNIQUE constraints, so title-less or url-less contexts would
///   never dedup. NULL maps to integer 0, which never equals a TEXT value (not even '0').
/// - a partial unique index enforces "at most one open session" in the database itself.
const V1: &str = "
CREATE TABLE sessions (
    id            INTEGER PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    ended_at_ms   INTEGER
);
CREATE UNIQUE INDEX sessions_one_open ON sessions (ended_at_ms IS NULL)
    WHERE ended_at_ms IS NULL;

CREATE TABLE contexts (
    id           INTEGER PRIMARY KEY,
    bundle_id    TEXT NOT NULL,
    app_name     TEXT NOT NULL,
    window_title TEXT,
    url          TEXT
);
CREATE UNIQUE INDEX contexts_identity ON contexts
    (bundle_id, app_name, coalesce(window_title, 0), coalesce(url, 0));

CREATE TABLE samples (
    session_id     INTEGER NOT NULL REFERENCES sessions (id),
    ts_ms          INTEGER NOT NULL,
    context_id     INTEGER NOT NULL REFERENCES contexts (id),
    idle_kb_ms     INTEGER NOT NULL,
    idle_mouse_ms  INTEGER NOT NULL,
    idle_click_ms  INTEGER NOT NULL,
    idle_scroll_ms INTEGER NOT NULL,
    locked         INTEGER NOT NULL,
    PRIMARY KEY (session_id, ts_ms)
) WITHOUT ROWID;
-- samples_between reads across sessions by time.
CREATE INDEX samples_ts ON samples (ts_ms);
";

/// v2 — PLAN §10.1: whether media/a call held the display awake at that sample. Existing
/// rows predate the signal and read as false, exactly like old JSONL fixtures.
const V2: &str = "
ALTER TABLE samples ADD COLUMN media_active INTEGER NOT NULL DEFAULT 0;
";

/// v3 — PLAN §14.2: what held audio output at each sample, deduplicated like contexts
/// (NULL-safe identity index). Existing rows have no audio.
const V3: &str = "
CREATE TABLE audio (
    id        INTEGER PRIMARY KEY,
    bundle_id TEXT NOT NULL,
    app       TEXT NOT NULL,
    title     TEXT,
    host      TEXT
);
CREATE UNIQUE INDEX audio_identity ON audio
    (bundle_id, app, coalesce(title, 0), coalesce(host, 0));
ALTER TABLE samples ADD COLUMN audio_id INTEGER REFERENCES audio (id);
";

pub(crate) const VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > VERSION {
        return Err(Error::UnsupportedSchemaVersion {
            found,
            supported: VERSION,
        });
    }
    for (from, sql) in MIGRATIONS.iter().enumerate().skip(found.max(0) as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", from as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_database_gains_an_empty_audio_reference() {
        let mut conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(V1).expect("v1");
        conn.execute_batch(V2).expect("v2");
        conn.pragma_update(None, "user_version", 2)
            .expect("version");
        conn.execute_batch(
            "INSERT INTO sessions (id, started_at_ms) VALUES (1, 0);
             INSERT INTO contexts (id, bundle_id, app_name) VALUES (1, 'b', 'a');
             INSERT INTO samples VALUES (1, 0, 1, 0, 0, 0, 0, 0, 1);",
        )
        .expect("v2 rows");
        migrate(&mut conn).expect("migrate");
        let audio: Option<i64> = conn
            .query_row("SELECT audio_id FROM samples", [], |r| r.get(0))
            .expect("column");
        assert_eq!(audio, None);
        let media: bool = conn
            .query_row("SELECT media_active FROM samples", [], |r| r.get(0))
            .expect("kept");
        assert!(media);
    }

    #[test]
    fn v1_database_gains_media_active_defaulting_to_false() {
        let mut conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(V1).expect("v1");
        conn.pragma_update(None, "user_version", 1)
            .expect("version");
        conn.execute_batch(
            "INSERT INTO sessions (id, started_at_ms) VALUES (1, 0);
             INSERT INTO contexts (id, bundle_id, app_name) VALUES (1, 'b', 'a');
             INSERT INTO samples VALUES (1, 0, 1, 0, 0, 0, 0, 0);",
        )
        .expect("v1 rows");

        migrate(&mut conn).expect("migrate");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("version");
        assert_eq!(version, VERSION);
        let media: bool = conn
            .query_row("SELECT media_active FROM samples", [], |r| r.get(0))
            .expect("column");
        assert!(!media);
    }
}
