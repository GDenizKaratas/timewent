//! The single `main` window: placement and visibility.

use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewWindow};

use crate::frame::keep_top_edge;
#[cfg(target_os = "macos")]
use crate::frame::Frame;
use crate::peek::{self, Action, Facts, WindowNow};
use crate::prefs::Prefs;
use crate::shared::lock;
use crate::state::AppState;

/// Event the webview listens to: payload [`peek::PeekEvent`].
pub const PEEK_EVENT: &str = "peek";

const MAIN: &str = "main";
/// Below the menu bar, clear of the notch's shadow.
const TOP_MARGIN: f64 = 10.0;

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN)
}

/// Horizontally centered, just under the menu bar, on the screen the window is on.
pub fn place_top_center(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let pos = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    let width = window.outer_size()?.to_logical::<f64>(scale).width;
    window.set_position(LogicalPosition::new(
        pos.x + ((size.width - width) / 2.0).max(0.0),
        pos.y + TOP_MARGIN,
    ))
}

/// Puts `text` on the general pasteboard (main thread, as AppKit wants).
#[cfg(target_os = "macos")]
pub fn copy_to_pasteboard(app: &AppHandle, text: String) -> crate::error::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        // SAFETY: reading an immutable framework constant.
        let kind = unsafe { NSPasteboardTypeString };
        let _ = tx.send(pb.setString_forType(&NSString::from_str(&text), kind));
    })?;
    if rx.recv().unwrap_or(false) {
        Ok(())
    } else {
        Err(crate::error::Error::Io(std::io::Error::other(
            "the pasteboard refused the text",
        )))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn copy_to_pasteboard(_app: &AppHandle, _text: String) -> crate::error::Result<()> {
    Err(crate::error::Error::Io(std::io::Error::other(
        "copy is macOS only",
    )))
}

/// The window's top-left in logical points.
pub fn current_pos(app: &AppHandle) -> Option<crate::login::WindowPos> {
    let w = main_window(app)?;
    let scale = w.scale_factor().ok()?;
    let p = w.outer_position().ok()?.to_logical::<f64>(scale);
    Some(crate::login::WindowPos { x: p.x, y: p.y })
}

/// Puts the window at `saved` if it is still on a screen; `false` if not (caller centres).
pub fn restore_position(
    window: &WebviewWindow,
    saved: Option<crate::login::WindowPos>,
) -> tauri::Result<bool> {
    let areas: Vec<crate::login::Area> = window
        .available_monitors()?
        .iter()
        .map(|m| {
            let s = m.scale_factor();
            let p = m.work_area().position.to_logical::<f64>(s);
            let z = m.work_area().size.to_logical::<f64>(s);
            crate::login::Area {
                x: p.x,
                y: p.y,
                w: z.width,
                h: z.height,
            }
        })
        .collect();
    let width = window
        .outer_size()?
        .to_logical::<f64>(window.scale_factor()?)
        .width;
    match crate::login::restore(saved, &areas, width) {
        Some(p) => {
            window.set_position(LogicalPosition::new(p.x, p.y))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Shows the window without making it key or activating the app (a login launch).
pub fn show_quietly(app: &AppHandle) {
    let Some(w) = main_window(app) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindow;
        match w.ns_window() {
            Ok(ptr) if !ptr.is_null() => {
                // SAFETY: Tauri's live NSWindow; setup runs on the main thread.
                let ns: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
                ns.orderFrontRegardless();
                return;
            }
            _ => {}
        }
    }
    if let Err(e) = w.show() {
        eprintln!("timewent: show window failed: {e}");
    }
}

pub fn is_visible(app: &AppHandle) -> bool {
    main_window(app).is_some_and(|w| w.is_visible().unwrap_or(false))
}

pub fn show(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        if let Err(e) = w.show().and_then(|()| w.set_focus()) {
            eprintln!("timewent: show window failed: {e}");
        }
    }
}

pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    main_window(app).map_or(Ok(()), |w| w.hide())
}

pub fn toggle(app: &AppHandle) {
    if is_visible(app) {
        if let Err(e) = hide(app) {
            eprintln!("timewent: hide window failed: {e}");
        }
    } else {
        show(app);
    }
}

/// While peeking the window stays on top regardless; closing the peek applies the pref.
pub fn apply_prefs(app: &AppHandle, prefs: &Prefs, peeking: bool) {
    if peeking {
        return;
    }
    if let Some(w) = main_window(app) {
        if let Err(e) = w.set_always_on_top(prefs.always_on_top) {
            eprintln!("timewent: always-on-top failed: {e}");
        }
    }
}

/// Feeds one peek input through the state machine and performs what it decides.
pub fn peek(app: &AppHandle, event: peek::Event) {
    let (Some(w), Some(state)) = (main_window(app), app.try_state::<AppState>()) else {
        return;
    };
    let visible = match w.is_visible() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("timewent: peek: {e}");
            return;
        }
    };
    let keyboard = keyboard_owner();
    let facts = Facts {
        window: WindowNow {
            visible,
            layout: *lock(&state.layout),
        },
        always_on_top: state.prefs().always_on_top,
        self_frontmost: keyboard.0,
        other_app: keyboard.1,
    };
    let actions = {
        let mut p = lock(&state.peek);
        let (next, actions) = peek::step(*p, event, facts);
        *p = next;
        actions
    };
    for action in actions {
        let result = match action {
            Action::AllSpaces(on) => w.set_visible_on_all_workspaces(on),
            Action::OnTop(on) => w.set_always_on_top(on),
            Action::Show => w.show(),
            Action::Focus => w.set_focus(),
            Action::Hide => w.hide(),
            Action::Emit(payload) => w.emit_to(MAIN, PEEK_EVENT, payload),
            Action::Activate(pid) => {
                activate(pid);
                Ok(())
            }
        };
        if let Err(e) = result {
            eprintln!("timewent: peek {action:?}: {e}");
        }
    }
    crate::tray::sync(app);
}

/// `(timewent has the keyboard, the app to give it back to)`. While timewent is frontmost
/// the previous app still owns the menu bar: an accessory app never takes it over.
#[cfg(target_os = "macos")]
fn keyboard_owner() -> (bool, Option<i32>) {
    use objc2_app_kit::NSWorkspace;
    let own = i32::try_from(std::process::id()).unwrap_or(-1);
    let ws = NSWorkspace::sharedWorkspace();
    let other = |pid: i32| (pid > 0 && pid != own).then_some(pid);
    let front = ws.frontmostApplication().map(|a| a.processIdentifier());
    match front {
        Some(pid) if pid == own => (
            true,
            ws.menuBarOwningApplication()
                .and_then(|a| other(a.processIdentifier())),
        ),
        Some(pid) => (false, other(pid)),
        None => (false, None),
    }
}

#[cfg(not(target_os = "macos"))]
fn keyboard_owner() -> (bool, Option<i32>) {
    (false, None)
}

/// Resizes the window keeping its top-left corner (DESIGN §11.3), in one
/// `setFrame:display:` on the main thread so there is no intermediate frame.
#[cfg(target_os = "macos")]
pub fn resize_keep_top(app: &AppHandle, width: f64, height: f64) -> tauri::Result<()> {
    let Some(w) = main_window(app) else {
        return Ok(());
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let win = w.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(resize_on_main(&win, width, height));
    })?;
    rx.recv().unwrap_or(Ok(()))
}

#[cfg(target_os = "macos")]
fn resize_on_main(w: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let ptr = w.ns_window()?;
    if ptr.is_null() {
        return Ok(());
    }
    // SAFETY: Tauri's live NSWindow; this runs on the main thread (run_on_main_thread).
    let ns: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
    let f = ns.frame();
    let to_frame = |r: NSRect| Frame {
        x: r.origin.x,
        y: r.origin.y,
        w: r.size.width,
        h: r.size.height,
    };
    let screen = ns.screen().map(|s| to_frame(s.visibleFrame()));
    let next = keep_top_edge(to_frame(f), width, height, screen);
    ns.setFrame_display(
        NSRect::new(NSPoint::new(next.x, next.y), NSSize::new(next.w, next.h)),
        true,
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn resize_keep_top(app: &AppHandle, width: f64, height: f64) -> tauri::Result<()> {
    match main_window(app) {
        Some(w) => w.set_size(tauri::LogicalSize::new(width, height)),
        None => Ok(()),
    }
}

/// Gives the keyboard back. Nothing happens if that app has quit in the meantime.
#[cfg(target_os = "macos")]
fn activate(pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

#[cfg(not(target_os = "macos"))]
fn activate(_pid: i32) {}

/// Lets the window show up on a full-screen app's Space while peeking. Tauri only exposes
/// "all Spaces" (`CanJoinAllSpaces`); full-screen Spaces also need `FullScreenAuxiliary`.
/// Harmless outside a peek: without `CanJoinAllSpaces` the window stays on its own Space.
#[cfg(target_os = "macos")]
pub fn allow_over_fullscreen(w: &WebviewWindow) -> tauri::Result<()> {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    let ptr = w.ns_window()?;
    if ptr.is_null() {
        return Ok(());
    }
    // SAFETY: Tauri hands out its live NSWindow; setup runs on the main thread, which AppKit
    // requires for collection-behavior changes.
    let ns_window: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
    let behavior = ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary;
    ns_window.setCollectionBehavior(behavior);
    Ok(())
}
