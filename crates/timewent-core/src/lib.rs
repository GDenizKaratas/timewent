//! Pure domain logic for timewent.

mod activity;
mod attribute;
mod config;
mod context;
mod explain;
mod lang;
mod listening;
mod one_liner;
mod presence;
mod range;
mod report;
mod run;
mod sample;
mod segment;
mod sources;
mod summary;
#[cfg(test)]
mod testkit;

pub use activity::{activity_member, Member, ACTIVITY_KEY_PREFIX};
pub use config::{Activity, Config, LabelRule, SELF_BUNDLE_ID};
pub use context::{derive_context, web_host, Category, Context};
pub use explain::{explain, VIA_LOCALHOST, VIA_TITLE};
pub use lang::Lang;
pub use one_liner::one_liner;
pub use presence::{classify_presence, Presence};
pub use range::{Range, SessionMeta};
pub use report::{local_date, local_iso, local_time, report, Report, ReportInput, REPORT_SCHEMA};
pub use sample::{Audio, Idle, Sample};
pub use segment::{
    segment, DetailTime, Evidence, Interruption, ListenTime, MemberTime, Segment, SegmentKind,
    Segmenter, CODE_KEY_PREFIX, PASSTHROUGH_KEY_PREFIX,
};
pub use sources::{seen_sources, SeenApp, SeenDomain, SeenSources, SourceCount, MAX_SOURCES};
pub use summary::{
    summarize, BreakdownItem, CategoryShare, Listening, NotShown, Row, RowKind, Summary,
};
