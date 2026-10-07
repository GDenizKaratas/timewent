//! SQLite storage for raw samples (PLAN §4).
//!
//! Samples are append-only and round-trip exactly at millisecond idle precision
//! (see [`idle_ms`]). Window/app/url strings are deduplicated into a `contexts` table so the
//! 1 Hz hot path is normally a single small INSERT.

mod error;
pub mod idle_ms;
mod schema;
mod store;

pub use error::{Error, Result};
pub use store::{ContextCount, SessionId, Store};
