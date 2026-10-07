//! timewent desktop app (DESIGN §11.10): wires probe → store → core to the ui.
//!
//! Testable logic lives in Tauri-free modules ([`views`], [`tracker`], [`engine`],
//! [`config_file`], [`clock`]); the rest is thin glue.

pub mod auto;
pub mod clock;
mod commands;
pub mod config_file;
pub mod dto;
pub mod engine;
pub mod error;
pub mod frame;
pub mod language;
pub mod login;

pub mod peek;
pub mod prefs;
mod shared;
pub mod shortcut;
mod state;
#[cfg(test)]
mod testkit;
pub mod tracker;
mod tray;
pub mod views;
mod window;

use std::fs;

use tauri::{App, Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;
use timewent_probe::Probe;
use timewent_store::Store;

use crate::engine::{Clock, Engine, ProbeFactory};
use crate::state::AppState;

/// Dev/verification hook: `TIMEWENT_AUTOSTART=1` starts a session at launch, without the
/// Accessibility prompt (nobody is there to answer it).
const AUTOSTART_ENV: &str = "TIMEWENT_AUTOSTART";

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent)
                .arg(login::LOGIN_ARG)
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // Only one shortcut is ever registered: the peek key.
                    if event.state == ShortcutState::Pressed {
                        window::peek(app, peek::Event::Key);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::start_session,
            commands::stop_session,
            commands::get_status,
            commands::get_view,
            commands::list_sessions,
            commands::export_json,
            commands::get_config,
            commands::set_config,
            commands::get_info,
            commands::open_accessibility_settings,
            commands::get_prefs,
            commands::set_prefs,
            commands::hide_window,
            commands::quit_app,
            commands::end_peek,
            commands::set_layout,
            commands::seen_sources,
            commands::resize_window,
            commands::sessions_overview,
            commands::delete_session,
            commands::export_json_string,
            commands::copy_json,
        ])
        .setup(|app| setup(app).map_err(Into::into))
        .on_window_event(|window, event| {
            // No close button, but ⌘W or similar must not destroy the only window: the tray
            // stays and "show window" has to have something to show.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    eprintln!("timewent: hide on close failed: {e}");
                }
                window::peek(window.app_handle(), peek::Event::WindowHidden);
            }
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("timewent: failed to start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        // Opened again from Finder / Spotlight / Dock while running: show the window.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            window::show(app);
            tray::sync(app);
        }
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                state.save_window_pos(app);
            }
            // Quit while tracking ends the session cleanly instead of leaving it to the
            // crash recovery of the next launch.
            if let Some(state) = app.try_state::<AppState>() {
                if let Err(e) = state.engine.stop() {
                    eprintln!("timewent: could not end session on quit: {e}");
                }
            }
        }
        _ => {}
    });
}

fn setup(app: &mut App) -> error::Result<()> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("timewent.db");
    let config_path = data_dir.join("config.json");
    let prefs_path = data_dir.join("prefs.json");
    let window_path = data_dir.join("window.json");
    let first_time = login::first_time(fs::read_to_string(&prefs_path).ok().as_deref());
    let prefs = prefs::load(&prefs_path);
    let at_login = login::launched_at_login(std::env::args());

    let config = config_file::load(&config_path);
    let engine = Engine::new(
        Store::open(&db_path)?,
        config,
        Some(config_path),
        probe_factory(),
        Clock {
            now_ms: clock::now_ms,
            day_start_ms: clock::local_midnight_ms,
            week_start_ms: clock::local_week_start_ms,
            input_now: timewent_probe::input_now,
            offset_at: clock::local_offset_s,
            timezone: clock::timezone_name,
        },
    )?;
    let peek_key = prefs.peek_shortcut.clone();
    let launch_at_login = prefs.launch_at_login;
    app.manage(AppState::new(
        engine,
        db_path,
        prefs,
        prefs_path,
        window_path,
    ));

    let handle = app.handle().clone();
    window::apply_prefs(&handle, &app.state::<AppState>().prefs(), false);
    state::register_peek(&handle, &peek_key);
    tray::build(&handle)?;
    if let Some(w) = app.get_webview_window("main") {
        // Back where the user left it (if that screen is still there), else top-center.
        if !window::restore_position(&w, app.state::<AppState>().saved_window_pos())? {
            window::place_top_center(&w)?;
        }
        #[cfg(target_os = "macos")]
        window::allow_over_fullscreen(&w)?;
    }
    if at_login {
        // Started with the Mac: the pill appears, takes no focus, nothing flashes.
        window::show_quietly(&handle);
    } else {
        window::show(&handle);
    }
    // DESIGN §11.8: on by default — registered once, the first time this version runs from a bundle.
    if first_time {
        app.state::<AppState>()
            .apply_first_launch_at_login(&handle, launch_at_login);
    }

    // The loop reports auto starts / splits: keep the tray in step.
    let tray_handle = handle.clone();
    let state = app.state::<AppState>();
    state
        .engine
        .set_on_change(std::sync::Arc::new(move || tray::sync(&tray_handle)));
    state.engine.set_auto(state.prefs().auto())?;
    if std::env::var(AUTOSTART_ENV).as_deref() == Ok("1") {
        app.state::<AppState>().start(&handle, false)?;
    }
    tray::sync(&handle);
    Ok(())
}

#[cfg(target_os = "macos")]
fn probe_factory() -> ProbeFactory {
    Box::new(|| Box::new(timewent_probe::MacProbe::new()) as Box<dyn Probe + Send>)
}

/// timewent only samples on macOS; elsewhere the app builds (for CI) but records a
/// placeholder.
#[cfg(not(target_os = "macos"))]
fn probe_factory() -> ProbeFactory {
    Box::new(|| {
        let placeholder = timewent_core::Sample {
            ts_ms: 0,
            app_name: "unsupported".into(),
            bundle_id: "unsupported".into(),
            window_title: None,
            url: None,
            idle: timewent_core::Idle {
                keyboard_s: 0.0,
                mouse_s: 0.0,
                click_s: 0.0,
                scroll_s: 0.0,
            },
            locked: false,
            media_active: false,
            audio: None,
        };
        let probe = timewent_probe::FakeProbe::new(
            vec![placeholder],
            timewent_probe::WhenExhausted::HoldLast,
        );
        Box::new(probe.expect("non-empty")) as Box<dyn Probe + Send>
    })
}
