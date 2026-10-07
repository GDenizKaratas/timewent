//! Frontmost application via `NSWorkspace`.

use objc2_app_kit::{NSRunningApplication, NSWorkspace};

pub(super) struct Frontmost {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    /// -1 for apps without a process.
    pub pid: i32,
}

pub(super) fn frontmost() -> Option<Frontmost> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(Frontmost {
        name: app.localizedName().map(|s| s.to_string()),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
        pid: app.processIdentifier(),
    })
}

/// `(name, bundle id)` of a running process; `None` once it has quit or for daemons.
pub(super) fn app_of_pid(pid: i32) -> Option<(String, String)> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    let bundle = app.bundleIdentifier()?.to_string();
    let name = app
        .localizedName()
        .map_or_else(|| bundle.clone(), |s| s.to_string());
    Some((name, bundle))
}
