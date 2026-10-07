//! Sampling the frontmost context (PLAN §5).
//!
//! The impure layer (`mac`) only reads raw values from the OS; every decision about them lives
//! in small pure modules ([`idle`], [`frontmost`], [`url_script`], [`url_cache`]) that are unit
//! tested on any platform.

pub mod audio;
mod fake;
pub mod frontmost;
pub mod idle;
#[cfg(target_os = "macos")]
mod mac;
pub mod media;
pub mod osascript;
pub mod url_cache;
pub mod url_script;

use timewent_core::Sample;

pub use fake::{FakeProbe, WhenExhausted};
#[cfg(target_os = "macos")]
pub use mac::{pump_main_run_loop, MacProbe};

/// One poll of what the user is looking at. `now_ms` (unix ms) becomes `Sample::ts_ms`: the
/// caller owns the clock.
pub trait Probe {
    fn sample(&mut self, now_ms: i64) -> Sample;
}

/// The cheapest "is anyone here" read: four idle timestamps and the session lock flag.
/// For waiting between sessions (auto mode) without full sampling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputNow {
    /// Seconds since the last input of any kind.
    pub idle_s: f64,
    pub locked: bool,
}

#[cfg(target_os = "macos")]
pub fn input_now() -> InputNow {
    mac::input_now()
}

#[cfg(not(target_os = "macos"))]
pub fn input_now() -> InputNow {
    InputNow {
        idle_s: f64::INFINITY,
        locked: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Needed for window titles. Without it samples still record app, url, idle and lock.
    pub accessibility: bool,
}

/// Current permission state. Cheap; safe to call every status poll.
pub fn permissions() -> Permissions {
    Permissions {
        accessibility: accessibility_trusted(),
    }
}

/// Like [`permissions`], but if Accessibility is not granted macOS shows its prompt and adds
/// the app to the Privacy → Accessibility list. Call it on a user action, not on a timer.
pub fn request_accessibility() -> Permissions {
    Permissions {
        accessibility: request_accessibility_impl(),
    }
}

#[cfg(target_os = "macos")]
fn request_accessibility_impl() -> bool {
    mac::request_accessibility()
}

#[cfg(not(target_os = "macos"))]
fn request_accessibility_impl() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> bool {
    mac::accessibility_trusted()
}

#[cfg(not(target_os = "macos"))]
fn accessibility_trusted() -> bool {
    false
}
