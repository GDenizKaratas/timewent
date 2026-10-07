//! Menu-bar icon: show/hide, start/stop (label follows state), quit.

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::state::AppState;
use crate::window;

pub const TOOLTIP: &str = "timewent — see where your time went";
const TRAY_ID: &str = "main";

/// Template images (black + alpha; macOS tints them): `›●` tracking, `›○` idle.
fn icon(tracking: bool) -> Image<'static> {
    if tracking {
        tauri::include_image!("icons/tray-tracking.png")
    } else {
        tauri::include_image!("icons/tray-idle.png")
    }
}

pub struct TrayMenu {
    visibility: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    on_top: CheckMenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let visibility = MenuItem::with_id(app, "visibility", "hide window", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "▶ start", true, None::<&str>)?;
    let on_top = CheckMenuItem::with_id(app, "on_top", "keep on top", true, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "quit timewent", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &visibility,
            &toggle,
            &separator,
            &on_top,
            &separator2,
            &quit,
        ],
    )?;

    // Left click peeks (DESIGN §11.3); right click opens the menu.
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .icon_as_template(true)
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_icon)
        .build(app)?;
    app.manage(TrayMenu {
        visibility,
        toggle,
        on_top,
    });
    sync(app);
    Ok(())
}

fn on_icon(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        window::peek(tray.app_handle(), crate::peek::Event::TrayClick);
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "visibility" => {
            window::toggle(app);
            sync(app);
        }
        "toggle" => {
            // Stopping joins the tracker thread (up to one in-flight tick): keep that off
            // the main thread, which runs this handler.
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                let result = if state.engine.is_tracking() {
                    state.stop(&app)
                } else {
                    state.start(&app, true)
                };
                if let Err(e) = result {
                    eprintln!("timewent: tray start/stop failed: {e}");
                }
            });
        }
        "on_top" => {
            // The check mark flips itself on click; persist and apply the new state from the
            // pref, then `sync` re-asserts the mark from what was actually saved.
            let state = app.state::<AppState>();
            let current = state.prefs();
            let prefs = crate::prefs::Prefs {
                always_on_top: !current.always_on_top,
                ..current
            };
            if let Err(e) = state.set_prefs(app, prefs) {
                eprintln!("timewent: saving prefs failed: {e}");
                sync(app);
            }
        }
        // Ending the session happens in the `RunEvent::Exit` handler.
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Re-labels the menu after tracking or window visibility changed.
pub fn sync(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let state = app.try_state::<AppState>();
    let tracking = state.as_ref().is_some_and(|s| s.engine.is_tracking());
    let on_top = state.as_ref().is_none_or(|s| s.prefs().always_on_top);
    let toggle = if tracking { "■ stop" } else { "▶ start" };
    let visibility = if window::is_visible(app) {
        "hide window"
    } else {
        "show window"
    };
    if let Err(e) = menu
        .toggle
        .set_text(toggle)
        .and_then(|()| menu.visibility.set_text(visibility))
        .and_then(|()| menu.on_top.set_checked(on_top))
    {
        eprintln!("timewent: tray update failed: {e}");
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // The template flag is per image: set it again with every swap.
        if let Err(e) = tray
            .set_icon(Some(icon(tracking)))
            .and_then(|()| tray.set_icon_as_template(true))
        {
            eprintln!("timewent: tray icon update failed: {e}");
        }
    }
}
