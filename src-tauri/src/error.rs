use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] timewent_store::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid config: {}", .0.join("; "))]
    InvalidConfig(Vec<String>),
    #[error("export path must be an absolute path to a .json file: {0}")]
    ExportPath(String),
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    /// Unusable or unavailable peek key; the previous key stays registered.
    #[error("{0}")]
    Shortcut(String),
    #[error("invalid prefs: {0}")]
    InvalidPrefs(String),
}

/// Commands reject with the message string; the ui shows it as-is.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
