//! IPC commands (DESIGN §11.10). Thin: each one delegates to [`Engine`](crate::engine::Engine).
//! Async so none of them runs on (and blocks) the main thread.

use std::path::Path;
use std::process::Command;

use tauri::{AppHandle, State};
use timewent_core::{Config, Range, SessionMeta};

use crate::dto::{Info, Status, View};
use crate::error::{Error, Result};
use crate::prefs::Prefs;
use crate::state::AppState;

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[tauri::command]
pub async fn start_session(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    state.start(&app, true)
}

#[tauri::command]
pub async fn stop_session(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    state.stop(&app)
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status> {
    Ok(state.status())
}

#[tauri::command]
pub async fn get_view(range: Range, state: State<'_, AppState>) -> Result<View> {
    state.engine.view(range, state.lang())
}

#[tauri::command]
pub async fn list_sessions(limit: u32, state: State<'_, AppState>) -> Result<Vec<SessionMeta>> {
    state.engine.sessions(limit)
}

#[tauri::command]
pub async fn export_json(range: Range, path: String, state: State<'_, AppState>) -> Result<String> {
    let written = state.engine.export(range, Path::new(&path), state.lang())?;
    Ok(written.display().to_string())
}

/// Past sessions for the data tab (DESIGN §11.5), newest first.
#[tauri::command]
pub async fn sessions_overview(
    limit: u32,
    offset: u32,
    state: State<'_, AppState>,
) -> Result<Vec<crate::dto::SessionOverview>> {
    state.engine.sessions_overview(limit, offset)
}

/// Deletes a past session and its raw samples permanently (never the open one).
#[tauri::command]
pub async fn delete_session(id: i64, state: State<'_, AppState>) -> Result<()> {
    state.engine.delete_session(id)
}

/// Apps and sites used in the last 30 days, for the activity editor (DESIGN §7.3).
#[tauri::command]
pub async fn seen_sources(state: State<'_, AppState>) -> Result<timewent_core::SeenSources> {
    state.engine.seen_sources()
}

/// Copies the export document to the macOS pasteboard natively (DESIGN §11.10): the webview's
/// clipboard API refuses writes that follow an awaited command (no user activation).
#[tauri::command]
pub async fn copy_json(range: Range, app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let text = state.export_text(range)?;
    crate::window::copy_to_pasteboard(&app, text)
}

/// The same document `export_json` writes, as a string — for "copy as json".
#[tauri::command]
pub async fn export_json_string(range: Range, state: State<'_, AppState>) -> Result<String> {
    state.export_text(range)
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<Config> {
    Ok(state.engine.config())
}

#[tauri::command]
pub async fn set_config(config: Config, state: State<'_, AppState>) -> Result<Config> {
    state.engine.set_config(config)
}

#[tauri::command]
pub async fn get_info(app: AppHandle, state: State<'_, AppState>) -> Result<Info> {
    Ok(Info {
        data_path: state.db_path.display().to_string(),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub async fn get_prefs(state: State<'_, AppState>) -> Result<Prefs> {
    Ok(state.prefs())
}

#[tauri::command]
pub async fn set_prefs(prefs: Prefs, app: AppHandle, state: State<'_, AppState>) -> Result<Prefs> {
    state.set_prefs(&app, prefs)
}

/// `esc` while peeking: puts the window back as it was (no-op when not peeking).
#[tauri::command]
pub async fn end_peek(app: AppHandle) {
    crate::window::peek(&app, crate::peek::Event::Esc);
}

/// Hides the window; tracking continues and the tray's "show window" brings it back.
/// During a peek this also ends it and hands the keyboard back.
#[tauri::command]
pub async fn hide_window(app: AppHandle) -> Result<()> {
    crate::window::hide(&app)?;
    crate::window::peek(&app, crate::peek::Event::WindowHidden);
    Ok(())
}

/// Resizes the window keeping its top-left corner where it is (no drift across peeks).
#[tauri::command]
pub async fn resize_window(width: f64, height: f64, app: AppHandle) -> Result<()> {
    crate::window::resize_keep_top(&app, width, height)?;
    Ok(())
}

/// The ui reports every expand / collapse, so peek restores exactly what was showing.
#[tauri::command]
pub async fn set_layout(layout: crate::peek::Layout, state: State<'_, AppState>) -> Result<()> {
    *crate::shared::lock(&state.layout) = layout;
    Ok(())
}

/// Same path as the tray's quit: `RunEvent::Exit` ends an open session, then the process
/// exits. (Accessory apps have no app menu, so ⌘Q reaches the app only through the ui.)
#[tauri::command]
pub async fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn open_accessibility_settings() -> Result<()> {
    let status = Command::new("/usr/bin/open")
        .arg(ACCESSIBILITY_PANE)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Io(std::io::Error::other(format!(
            "open {ACCESSIBILITY_PANE} failed: {status}"
        ))))
    }
}
