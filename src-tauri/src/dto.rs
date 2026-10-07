//! IPC shapes (PLAN §6) that are not already core types. Core `Range`, `Row`, `DetailTime`
//! and `SessionMeta` serialize to the contract verbatim, so they are reused as-is.

use serde::Serialize;
use timewent_core::{Category, DetailTime, Presence, Range, Row};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub tracking: bool,
    pub session_id: Option<i64>,
    pub started_at_ms: Option<i64>,
    pub elapsed_ms: i64,
    pub current: Option<Current>,
    pub permissions: PermissionsDto,
    /// Auto mode on and not paused (§11.2): the pill shows `auto` dimly.
    pub auto: bool,
}

/// What you are on now: the pill (PLAN §6, §10.2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Current {
    /// Context key of the last real activity (never a passthrough app, never "away").
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    pub category: Category,
    /// Of the latest sample: are you here right now?
    pub presence: Presence,
    /// Time on `key` this session — the same number as that key's row in the view.
    pub context_ms: i64,
    /// Start of the current passive/away stretch; `None` while active.
    pub since_ms: Option<i64>,
    /// The group row this belongs to — a project (§11.1) or an activity (§13.1) — if any.
    /// `key` / `context_ms` are then that row's; for an activity `label` is the member app.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PermissionsDto {
    pub accessibility: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InterruptionDto {
    pub label: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub key: String,
    pub label: String,
    pub category: Option<Category>,
    pub kind: timewent_core::SegmentKind,
    pub active_ms: i64,
    pub passive_ms: i64,
    pub details: Vec<DetailTime>,
    pub interruptions: Vec<InterruptionDto>,
    pub explain: Vec<String>,
    /// Project this segment was attributed to (§11.1).
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub range: Range,
    pub total_ms: i64,
    pub active_ms: i64,
    pub passive_ms: i64,
    pub away_ms: i64,
    /// Longest stretch on one row; number of row changes not split by away/gap (§11.3).
    pub longest_focus_ms: i64,
    pub switches: u32,
    /// Copyable summary line (§11.4).
    pub one_liner: String,
    /// What kind of time (§14.1), ms desc.
    pub categories: Vec<timewent_core::CategoryShare>,
    /// Background audio (§14.2), ms desc; never part of in-use.
    pub listening: Vec<timewent_core::Listening>,
    /// In use but no row: pass-through apps credited to nothing (§21.1), ms desc.
    pub not_shown: Vec<timewent_core::NotShown>,
    pub rows: Vec<Row>,
    /// Chronological.
    pub segments: Vec<SegmentDto>,
}

/// One line of the past-sessions list (PLAN §18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionOverview {
    pub id: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    /// Active + passive, as the header's "in use".
    pub in_use_ms: i64,
    /// Label of the session's top row, if it has any.
    pub top_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Info {
    pub data_path: String,
    pub version: String,
}
