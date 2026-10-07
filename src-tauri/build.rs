// Declaring the app's commands makes each one a permission (`allow-<command>`), so the
// capability file grants them explicitly instead of every command being callable by default.
const COMMANDS: &[&str] = &[
    "start_session",
    "stop_session",
    "get_status",
    "get_view",
    "list_sessions",
    "export_json",
    "get_config",
    "set_config",
    "get_info",
    "open_accessibility_settings",
    "get_prefs",
    "set_prefs",
    "hide_window",
    "quit_app",
    "end_peek",
    "set_layout",
    "seen_sources",
    "resize_window",
    "sessions_overview",
    "delete_session",
    "export_json_string",
    "copy_json",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build script");
}
