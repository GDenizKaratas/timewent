//! CoreGraphics: input idleness and session lock state. Neither needs any permission.

use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGEventSource, CGEventSourceStateID, CGEventType, CGSessionCopyCurrentDictionary,
};
use timewent_core::Idle;

use crate::idle::{fold_idle, IDLE_EVENT_TYPES};

/// Seconds since the last event of each type, in the combined (HID + programmatic) session
/// state. A read of a timestamp, not of events: no event tap, no Input Monitoring.
pub(super) fn idle() -> Idle {
    fold_idle(IDLE_EVENT_TYPES.iter().map(|&(code, kind)| {
        let secs = CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            CGEventType(code),
        );
        (kind, secs)
    }))
}

/// `CGSSessionScreenIsLocked` from the current session dictionary. The key is absent when
/// unlocked; the dictionary is absent outside a GUI session (e.g. over ssh) → not locked.
pub(super) fn session_locked() -> bool {
    let Some(dict) = CGSessionCopyCurrentDictionary() else {
        return false;
    };
    // SAFETY: the session dictionary maps CFString keys to CF property-list values.
    let dict: CFRetained<CFDictionary<CFString, CFType>> =
        unsafe { CFRetained::cast_unchecked(dict) };
    dict.get(&CFString::from_static_str("CGSSessionScreenIsLocked"))
        .and_then(|v| v.downcast::<CFBoolean>().ok())
        .is_some_and(|b| b.as_bool())
}
