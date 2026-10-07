use crate::store::SessionId;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("session {0} is already open")]
    SessionAlreadyOpen(SessionId),
    #[error("no session with id {0}")]
    NoSuchSession(SessionId),
    #[error("session {0} is still being recorded")]
    SessionOpen(SessionId),
    #[error("session {0} has already ended")]
    SessionAlreadyEnded(SessionId),
    #[error("session {id} cannot end at {ended_at_ms} before its start at {started_at_ms}")]
    EndBeforeStart {
        id: SessionId,
        started_at_ms: i64,
        ended_at_ms: i64,
    },
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
}
