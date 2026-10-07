//! What a view or export covers (PLAN §6 `Range`) and how a session is stored.

use serde::{Deserialize, Serialize};

/// A tracking session as stored (PLAN §4, §6 `SessionMeta`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
}

/// Which samples a view covers (PLAN §6 `Range`, §10.2). Resolving `Today` / `Week` to
/// timestamps needs a clock and time zone, so that happens outside core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Range {
    Session {
        id: i64,
    },
    Today,
    /// Local Monday 00:00 → now.
    Week,
    /// Every session ever recorded (PLAN §19: "download all").
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_serializes_with_kind_tag() {
        assert_eq!(
            serde_json::to_value(Range::Session { id: 3 }).expect("ser"),
            serde_json::json!({"kind": "session", "id": 3})
        );
        assert_eq!(
            serde_json::to_value(Range::Today).expect("ser"),
            serde_json::json!({"kind": "today"})
        );
        assert_eq!(
            serde_json::to_value(Range::Week).expect("ser"),
            serde_json::json!({"kind": "week"})
        );
        let back: Range = serde_json::from_str(r#"{"kind":"week"}"#).expect("parse");
        assert_eq!(back, Range::Week);
        assert_eq!(
            serde_json::to_value(Range::All).expect("ser"),
            serde_json::json!({"kind": "all"})
        );
    }
}
