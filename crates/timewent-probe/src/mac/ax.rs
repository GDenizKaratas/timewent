//! Focused window title via the Accessibility API.
//!
//! Ownership: `AXUIElementCreateApplication` and `AXUIElementCopyAttributeValue` follow the
//! Create/Copy rule, so every result is wrapped in `CFRetained` the moment it exists and is
//! released on every path by `Drop`. Attribute names are `CFString::from_static_str`, also
//! `CFRetained`. Nothing here outlives a call, so nothing can accumulate at 1 Hz.

use std::ptr::{self, NonNull};

use objc2_application_services::{
    kAXTrustedCheckOptionPrompt, AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
    AXUIElement,
};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};

/// A hung (beachballing) app would otherwise block each AX call for the 6 s system default.
const AX_TIMEOUT_S: f32 = 0.25;

pub(crate) fn accessibility_trusted() -> bool {
    // SAFETY: no arguments; reads this process's TCC state.
    unsafe { AXIsProcessTrusted() }
}

/// Like [`accessibility_trusted`], but when untrusted macOS also shows its "grant access"
/// prompt (asynchronously) and lists this app under Privacy → Accessibility.
pub(crate) fn request_accessibility() -> bool {
    // SAFETY: reading an immutable framework constant.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: the dictionary has the documented key/value types (CFString → CFBoolean).
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// `None` when untrusted, the app has no focused window, or on any AX error.
pub(super) fn focused_window_title(pid: i32) -> Option<String> {
    if !accessibility_trusted() {
        return None;
    }
    // SAFETY: any pid is accepted; an invalid one yields an element whose calls fail.
    let app = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: `app` is a valid element. Failure just keeps the default timeout.
    unsafe { app.set_messaging_timeout(AX_TIMEOUT_S) };
    let window = copy_attribute(&app, &CFString::from_static_str("AXFocusedWindow"))?
        .downcast::<AXUIElement>()
        .ok()?;
    let title = copy_attribute(&window, &CFString::from_static_str("AXTitle"))?
        .downcast::<CFString>()
        .ok()?;
    Some(title.to_string())
}

fn copy_attribute(element: &AXUIElement, attribute: &CFString) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = ptr::null();
    // SAFETY: `value` is a valid out-pointer; on success it receives a +1 reference.
    let err = unsafe { element.copy_attribute_value(attribute, NonNull::from(&mut value)) };
    // Take ownership before looking at `err`, so a value is released even on a failure path.
    // SAFETY: a non-null out value is an owned (+1) CF object per the Copy rule.
    let owned = NonNull::new(value.cast_mut()).map(|v| unsafe { CFRetained::from_raw(v) });
    if err == AXError::Success {
        owned
    } else {
        None
    }
}
