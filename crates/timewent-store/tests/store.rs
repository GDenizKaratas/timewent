//! Behavior of the public `Store` API against in-memory and on-disk SQLite.

use rusqlite::Connection;
use timewent_core::{Idle, Sample, SessionMeta};
use timewent_store::{Error, Store};

fn idle(keyboard_s: f64, mouse_s: f64, click_s: f64, scroll_s: f64) -> Idle {
    Idle {
        keyboard_s,
        mouse_s,
        click_s,
        scroll_s,
    }
}

fn sample(ts_ms: i64, app: &str, title: Option<&str>, url: Option<&str>) -> Sample {
    Sample {
        ts_ms,
        app_name: app.into(),
        bundle_id: format!("test.{app}"),
        window_title: title.map(String::from),
        url: url.map(String::from),
        idle: idle(0.4, 1.25, 20.001, 3600.5),
        locked: false,
        media_active: false,
        audio: None,
    }
}

fn mem() -> Store {
    Store::open_in_memory().expect("open in-memory store")
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

#[test]
fn sample_round_trips_exactly_including_none_title_and_url() {
    let mut store = mem();
    let id = store.start_session(1_000).expect("start");
    let samples = vec![
        sample(1_000, "Code", Some("main.rs — timewent"), None),
        sample(2_000, "Finder", None, None),
        Sample {
            locked: true,
            idle: idle(0.0, 0.001, 86_400.123, 1e9),
            ..sample(3_000, "Google Chrome", Some(""), Some("https://docs.rs/x"))
        },
        Sample {
            audio: Some(timewent_core::Audio {
                bundle_id: "com.spotify.client".into(),
                app: "Spotify".into(),
                title: Some("lofi — ChilledCow".into()),
                host: None,
            }),
            ..sample(5_000, "Code", Some("main.rs — timewent"), None)
        },
        Sample {
            audio: Some(timewent_core::Audio {
                bundle_id: "com.google.Chrome".into(),
                app: "Google Chrome".into(),
                title: None,
                host: Some("youtube.com".into()),
            }),
            ..sample(6_000, "Code", Some("main.rs — timewent"), None)
        },
        Sample {
            media_active: true,
            ..sample(
                7_000,
                "Google Chrome",
                Some("cat video"),
                Some("https://youtube.com/"),
            )
        },
    ];
    for s in &samples {
        store.append(id, s).expect("append");
    }
    assert_eq!(store.samples(id).expect("samples"), samples);
}

#[test]
fn sub_millisecond_idle_is_stored_at_nearest_millisecond() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    let mut s = sample(0, "A", None, None);
    s.idle = idle(1.23456, 0.0004, 0.0005, 2.9999);
    store.append(id, &s).expect("append");
    let back = store.samples(id).expect("samples");
    assert_eq!(back[0].idle, idle(1.235, 0.0, 0.001, 3.0));
}

#[test]
fn repeated_context_is_stored_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let mut store = Store::open(&path).expect("open");
    let id = store.start_session(0).expect("start");
    for i in 0..50 {
        store
            .append(id, &sample(i * 1000, "Code", Some("a.rs — p"), None))
            .expect("append");
    }
    let raw = Connection::open(&path).expect("raw");
    assert_eq!(count(&raw, "contexts"), 1);
    assert_eq!(count(&raw, "samples"), 50);
}

#[test]
fn contexts_without_title_or_url_are_deduplicated_too() {
    // SQLite UNIQUE treats NULLs as distinct; the store must still dedup them.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let id = {
        let mut store = Store::open(&path).expect("open");
        let id = store.start_session(0).expect("start");
        store
            .append(id, &sample(0, "Finder", None, None))
            .expect("a");
        id
    };
    // A fresh store has an empty context cache, so this exercises the index, not the cache.
    let mut store = Store::open(&path).expect("reopen");
    store
        .append(id, &sample(1_000, "Finder", None, None))
        .expect("b");
    let raw = Connection::open(&path).expect("raw");
    assert_eq!(count(&raw, "contexts"), 1);
}

#[test]
fn none_and_empty_strings_are_distinct_contexts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let mut store = Store::open(&path).expect("open");
    let id = store.start_session(0).expect("start");
    let samples = vec![
        sample(0, "A", None, None),
        sample(1_000, "A", Some(""), None),
        sample(2_000, "A", None, Some("")),
        sample(3_000, "A", Some("0"), Some("0")),
        sample(4_000, "A", None, None),
    ];
    for s in &samples {
        store.append(id, s).expect("append");
    }
    let raw = Connection::open(&path).expect("raw");
    assert_eq!(count(&raw, "contexts"), 4);
    assert_eq!(store.samples(id).expect("samples"), samples);
}

#[test]
fn switching_back_to_a_known_context_reuses_its_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let mut store = Store::open(&path).expect("open");
    let id = store.start_session(0).expect("start");
    for (i, app) in ["A", "B", "A", "B", "A"].iter().enumerate() {
        store
            .append(id, &sample(i as i64 * 1000, app, Some("t"), None))
            .expect("append");
    }
    let raw = Connection::open(&path).expect("raw");
    assert_eq!(count(&raw, "contexts"), 2);
    let apps: Vec<String> = store
        .samples(id)
        .expect("samples")
        .into_iter()
        .map(|s| s.app_name)
        .collect();
    assert_eq!(apps, ["A", "B", "A", "B", "A"]);
}

#[test]
fn samples_are_returned_in_timestamp_order() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    for ts in [3_000, 1_000, 2_000] {
        store
            .append(id, &sample(ts, "A", None, None))
            .expect("append");
    }
    let ts: Vec<i64> = store
        .samples(id)
        .expect("samples")
        .iter()
        .map(|s| s.ts_ms)
        .collect();
    assert_eq!(ts, [1_000, 2_000, 3_000]);
}

#[test]
fn samples_of_one_session_exclude_other_sessions() {
    let mut store = mem();
    let a = store.start_session(0).expect("start a");
    store
        .append(a, &sample(0, "A", None, None))
        .expect("append");
    store.end_session(a, 1_000).expect("end a");
    let b = store.start_session(5_000).expect("start b");
    store
        .append(b, &sample(5_000, "B", None, None))
        .expect("append");
    let apps: Vec<String> = store
        .samples(b)
        .expect("samples")
        .into_iter()
        .map(|s| s.app_name)
        .collect();
    assert_eq!(apps, ["B"]);
    assert!(store.samples(999).expect("unknown session").is_empty());
}

#[test]
fn duplicate_timestamp_within_a_session_is_rejected() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    store
        .append(id, &sample(0, "A", None, None))
        .expect("first");
    assert!(matches!(
        store.append(id, &sample(0, "B", None, None)),
        Err(Error::Sqlite(_))
    ));
}

#[test]
fn appending_to_an_unknown_session_is_rejected() {
    let mut store = mem();
    assert!(matches!(
        store.append(42, &sample(0, "A", None, None)),
        Err(Error::Sqlite(_))
    ));
}

#[test]
fn context_counts_aggregate_unlocked_samples_since_a_time() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    let chrome = |ts| sample(ts, "Chrome", Some("PR"), Some("https://github.com/a"));
    for ts in [0, 1_000, 2_000] {
        store.append(id, &chrome(ts)).expect("append");
    }
    store
        .append(id, &sample(3_000, "Code", Some("x"), None))
        .expect("append");
    store
        .append(
            id,
            &Sample {
                locked: true,
                ..sample(4_000, "Code", Some("x"), None)
            },
        )
        .expect("append");
    store.append(id, &chrome(5_000)).expect("append");

    let mut counts = store.context_counts(1_000).expect("counts");
    counts.sort_by(|a, b| a.app_name.cmp(&b.app_name));
    let got: Vec<(&str, Option<&str>, i64, i64)> = counts
        .iter()
        .map(|c| {
            (
                c.app_name.as_str(),
                c.url.as_deref(),
                c.samples,
                c.last_ts_ms,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("Chrome", Some("https://github.com/a"), 3, 5_000),
            ("Code", None, 1, 3_000),
        ]
    );
    assert_eq!(counts[0].bundle_id, "test.Chrome");
}

#[test]
fn samples_after_returns_only_newer_samples_of_that_session() {
    let mut store = mem();
    let a = store.start_session(0).expect("start");
    for ts in [0, 1_000, 2_000, 3_000] {
        store
            .append(a, &sample(ts, "A", None, None))
            .expect("append");
    }
    store.end_session(a, 4_000).expect("end");
    let b = store.start_session(5_000).expect("start");
    store
        .append(b, &sample(5_000, "B", None, None))
        .expect("append");

    let ts: Vec<i64> = store
        .samples_after(a, 1_000)
        .expect("after")
        .iter()
        .map(|s| s.ts_ms)
        .collect();
    assert_eq!(ts, [2_000, 3_000]);
    assert_eq!(store.samples_after(a, i64::MIN).expect("all").len(), 4);
    assert!(store.samples_after(a, 3_000).expect("none").is_empty());
}

#[test]
fn samples_between_includes_from_and_excludes_to_across_sessions() {
    let mut store = mem();
    let a = store.start_session(0).expect("start a");
    for ts in [0, 1_000, 2_000] {
        store
            .append(a, &sample(ts, "A", None, None))
            .expect("append");
    }
    store.end_session(a, 3_000).expect("end a");
    let b = store.start_session(3_000).expect("start b");
    for ts in [5_000, 4_000, 3_000] {
        store
            .append(b, &sample(ts, "B", None, None))
            .expect("append");
    }
    let ts: Vec<i64> = store
        .samples_between(1_000, 5_000)
        .expect("between")
        .iter()
        .map(|s| s.ts_ms)
        .collect();
    assert_eq!(ts, [1_000, 2_000, 3_000, 4_000]);
    assert!(store.samples_between(5_001, 9_000).expect("e").is_empty());
    assert!(store.samples_between(2_000, 2_000).expect("e").is_empty());
}

#[test]
fn start_session_returns_it_as_the_open_session() {
    let mut store = mem();
    assert_eq!(store.open_session().expect("none"), None);
    let id = store.start_session(1_234).expect("start");
    assert_eq!(
        store.open_session().expect("open"),
        Some(SessionMeta {
            id,
            started_at_ms: 1_234,
            ended_at_ms: None
        })
    );
}

#[test]
fn starting_a_second_session_while_one_is_open_errors() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    match store.start_session(5_000) {
        Err(Error::SessionAlreadyOpen(open)) => assert_eq!(open, id),
        other => panic!("expected SessionAlreadyOpen, got {other:?}"),
    }
    assert_eq!(store.sessions(10).expect("sessions").len(), 1);
}

#[test]
fn end_session_records_end_and_allows_a_new_session() {
    let mut store = mem();
    let id = store.start_session(0).expect("start");
    store.end_session(id, 9_000).expect("end");
    assert_eq!(store.open_session().expect("open"), None);
    assert_eq!(
        store.sessions(1).expect("sessions"),
        vec![SessionMeta {
            id,
            started_at_ms: 0,
            ended_at_ms: Some(9_000)
        }]
    );
    assert!(store.start_session(10_000).is_ok());
}

#[test]
fn ending_an_unknown_or_ended_session_errors() {
    let mut store = mem();
    assert!(matches!(
        store.end_session(7, 1_000),
        Err(Error::NoSuchSession(7))
    ));
    let id = store.start_session(0).expect("start");
    store.end_session(id, 1_000).expect("end");
    assert!(matches!(
        store.end_session(id, 2_000),
        Err(Error::SessionAlreadyEnded(i)) if i == id
    ));
}

#[test]
fn ending_before_the_start_errors() {
    let mut store = mem();
    let id = store.start_session(5_000).expect("start");
    assert!(matches!(
        store.end_session(id, 4_999),
        Err(Error::EndBeforeStart { .. })
    ));
    assert!(store.open_session().expect("open").is_some());
}

#[test]
fn close_dangling_ends_at_last_sample_plus_poll_interval() {
    let mut store = mem();
    let id = store.start_session(1_000).expect("start");
    for ts in [1_000, 2_000, 3_500] {
        store
            .append(id, &sample(ts, "A", None, None))
            .expect("append");
    }
    let closed = store.close_dangling(1_000).expect("close");
    let expected = SessionMeta {
        id,
        started_at_ms: 1_000,
        ended_at_ms: Some(4_500),
    };
    assert_eq!(closed, Some(expected.clone()));
    assert_eq!(store.open_session().expect("open"), None);
    assert_eq!(store.sessions(1).expect("sessions"), vec![expected]);
}

#[test]
fn close_dangling_without_samples_ends_at_start() {
    let mut store = mem();
    let id = store.start_session(1_000).expect("start");
    assert_eq!(
        store.close_dangling(1_000).expect("close"),
        Some(SessionMeta {
            id,
            started_at_ms: 1_000,
            ended_at_ms: Some(1_000)
        })
    );
}

#[test]
fn close_dangling_without_open_session_is_a_no_op() {
    let mut store = mem();
    assert_eq!(store.close_dangling(1_000).expect("close"), None);
    let id = store.start_session(0).expect("start");
    store.end_session(id, 500).expect("end");
    assert_eq!(store.close_dangling(1_000).expect("close"), None);
    assert_eq!(
        store.sessions(1).expect("sessions")[0].ended_at_ms,
        Some(500)
    );
}

#[test]
fn sessions_are_listed_newest_first_up_to_limit() {
    let mut store = mem();
    let mut ids = Vec::new();
    for start in [0, 10_000, 20_000] {
        let id = store.start_session(start).expect("start");
        store.end_session(id, start + 1_000).expect("end");
        ids.push(id);
    }
    let listed: Vec<i64> = store
        .sessions(2)
        .expect("sessions")
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(listed, [ids[2], ids[1]]);
    assert!(store.sessions(0).expect("none").is_empty());
}

#[test]
fn reopening_a_file_keeps_data_and_does_not_re_migrate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let id = {
        let mut store = Store::open(&path).expect("open");
        let id = store.start_session(0).expect("start");
        store
            .append(id, &sample(0, "A", None, None))
            .expect("append");
        id
    };
    for _ in 0..2 {
        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.samples(id).expect("samples").len(), 1);
        assert_eq!(store.open_session().expect("open").map(|s| s.id), Some(id));
    }
    let raw = Connection::open(&path).expect("raw");
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 3);
}

#[test]
fn file_store_uses_wal_journal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    let _store = Store::open(&path).expect("open");
    let raw = Connection::open(&path).expect("raw");
    let mode: String = raw
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("mode");
    assert_eq!(mode, "wal");
}

#[test]
fn a_database_from_a_newer_version_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    Connection::open(&path)
        .expect("raw")
        .pragma_update(None, "user_version", 99)
        .expect("set version");
    assert!(matches!(
        Store::open(&path),
        Err(Error::UnsupportedSchemaVersion { found: 99, .. })
    ));
}

// ── deleting a session (PLAN §18) ──────────────────────────────

fn closed_session(store: &mut Store, start: i64, samples: &[Sample]) -> i64 {
    let id = store.start_session(start).expect("start");
    for s in samples {
        store.append(id, s).expect("append");
    }
    store.end_session(id, start + 100_000).expect("end");
    id
}

#[test]
fn sessions_page_is_newest_first_with_offset() {
    let mut store = mem();
    let ids: Vec<i64> = (0..5)
        .map(|i| closed_session(&mut store, i * 1_000_000, &[]))
        .collect();
    let page: Vec<i64> = store
        .sessions_page(2, 1)
        .expect("page")
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(page, [ids[3], ids[2]]);
    assert!(store.sessions_page(10, 5).expect("past the end").is_empty());
}

#[test]
fn deleting_a_session_removes_only_its_samples() {
    let mut store = mem();
    let a = closed_session(
        &mut store,
        0,
        &[
            sample(0, "A", Some("x"), None),
            sample(1_000, "A", Some("x"), None),
        ],
    );
    let b = closed_session(
        &mut store,
        200_000,
        &[sample(200_000, "B", Some("y"), None)],
    );
    store.delete_session(a).expect("delete");
    assert!(store.samples(a).expect("samples").is_empty());
    assert_eq!(store.samples(b).expect("samples").len(), 1);
    let ids: Vec<i64> = store
        .sessions(10)
        .expect("sessions")
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, [b]);
    assert_eq!(store.samples_between(0, i64::MAX).expect("all").len(), 1);
}

#[test]
fn context_cleanup_keeps_contexts_still_used_elsewhere() {
    let mut store = mem();
    let shared = sample(0, "Shared", Some("s"), None);
    let a = closed_session(
        &mut store,
        0,
        &[shared.clone(), sample(1_000, "OnlyA", Some("a"), None)],
    );
    let _b = closed_session(
        &mut store,
        200_000,
        &[Sample {
            ts_ms: 200_000,
            ..shared
        }],
    );
    // The shared context is still used by b; the a-only one goes.
    store.delete_session(a).expect("delete");
    assert_eq!(store.context_count().expect("count"), 1);
}

#[test]
fn orphaned_audio_sources_are_cleaned_up_too() {
    let mut store = mem();
    let playing = Sample {
        audio: Some(timewent_core::Audio {
            bundle_id: "com.spotify.client".into(),
            app: "Spotify".into(),
            title: Some("t".into()),
            host: None,
        }),
        ..sample(0, "A", None, None)
    };
    let a = closed_session(&mut store, 0, &[playing]);
    assert_eq!(store.audio_count().expect("count"), 1);
    store.delete_session(a).expect("delete");
    assert_eq!(store.audio_count().expect("count"), 0);
}

#[test]
fn the_open_session_and_unknown_ids_cannot_be_deleted() {
    let mut store = mem();
    let open = store.start_session(0).expect("start");
    store
        .append(open, &sample(0, "A", None, None))
        .expect("append");
    assert!(matches!(store.delete_session(open), Err(Error::SessionOpen(id)) if id == open));
    assert_eq!(store.samples(open).expect("kept").len(), 1);
    assert!(matches!(
        store.delete_session(999),
        Err(Error::NoSuchSession(999))
    ));
}

#[test]
fn appending_after_a_delete_never_reuses_a_deleted_context_id() {
    // The one-entry context cache must not hand out an id that the cleanup removed.
    let mut store = mem();
    let s = sample(0, "A", Some("x"), None);
    let a = closed_session(&mut store, 0, std::slice::from_ref(&s));
    store.delete_session(a).expect("delete");
    let b = store.start_session(500_000).expect("start");
    store
        .append(
            b,
            &Sample {
                ts_ms: 500_000,
                ..s.clone()
            },
        )
        .expect("append after delete");
    assert_eq!(store.samples(b).expect("samples")[0].app_name, "A");
}
