//! The managed app state, plus the start/stop actions shared by commands and the tray.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use timewent_probe::{permissions, request_accessibility};

use crate::config_file;
use crate::dto::Status;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::peek::{Layout, Peek};
use crate::prefs::Prefs;
use crate::shared::lock;
use crate::shortcut::parse_peek;
use crate::{tray, window};

pub struct AppState {
    pub engine: Engine,
    pub db_path: PathBuf,
    prefs: Mutex<Prefs>,
    prefs_path: PathBuf,
    /// Last window top-left, kept across launches (§22: "starts at its last position").
    window_path: PathBuf,
    pub peek: Mutex<Peek>,
    /// As last reported by the ui (`set_layout`).
    pub layout: Mutex<Layout>,
    ax_prompted: AtomicBool,
}

impl AppState {
    pub fn new(
        engine: Engine,
        db_path: PathBuf,
        prefs: Prefs,
        prefs_path: PathBuf,
        window_path: PathBuf,
    ) -> Self {
        Self {
            engine,
            db_path,
            prefs: Mutex::new(prefs),
            prefs_path,
            window_path,
            peek: Mutex::new(Peek::Closed),
            layout: Mutex::new(Layout::Pill),
            ax_prompted: AtomicBool::new(false),
        }
    }

    pub fn prefs(&self) -> Prefs {
        lock(&self.prefs).clone()
    }

    /// The language text is rendered in: the pref, or macOS's when `system`.
    pub fn lang(&self) -> timewent_core::Lang {
        crate::language::resolve(self.prefs().language, &crate::language::system_preferred())
    }

    /// §22 first run: register (or not) per the default, then write the setting down so it
    /// is never applied again — the user's later choice in System Settings stands. Only a
    /// built bundle does this; a dev binary leaves everything untouched.
    pub fn apply_first_launch_at_login(&self, app: &AppHandle, on: bool) {
        let in_bundle = std::env::current_exe().is_ok_and(|p| crate::login::in_app_bundle(&p));
        if !in_bundle {
            return;
        }
        if let Err(e) = set_login_item(app, on) {
            eprintln!("timewent: open at login: {e}");
            return;
        }
        if let Err(e) = config_file::save(&self.prefs_path, &self.prefs()) {
            eprintln!("timewent: saving prefs: {e}");
        }
    }

    pub fn saved_window_pos(&self) -> Option<crate::login::WindowPos> {
        let text = std::fs::read_to_string(&self.window_path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save_window_pos(&self, app: &AppHandle) {
        if let Some(pos) = crate::window::current_pos(app) {
            if let Err(e) = config_file::save(&self.window_path, &pos) {
                eprintln!("timewent: saving window position: {e}");
            }
        }
    }

    /// The export document in the user's language — the single entry point for the file
    /// export, `export_json_string` and `copy_json`, so none of them can pick another language.
    pub fn export_text(&self, range: timewent_core::Range) -> Result<String> {
        self.engine.export_text(range, self.lang())
    }

    pub fn is_peeking(&self) -> bool {
        matches!(*lock(&self.peek), Peek::Open { .. })
    }

    /// Re-binds the peek key if it changed (new one first: on failure the old one stays and
    /// nothing is saved), saves, then applies to the window and the tray check item.
    pub fn set_prefs(&self, app: &AppHandle, prefs: Prefs) -> Result<Prefs> {
        prefs.validate().map_err(Error::InvalidPrefs)?;
        let old = self.prefs();
        if old.launch_at_login != prefs.launch_at_login {
            set_login_item(app, prefs.launch_at_login)?;
        }
        let rebound = old.peek_shortcut != prefs.peek_shortcut;
        if rebound {
            rebind_peek(app, &old.peek_shortcut, &prefs.peek_shortcut)?;
        }
        if let Err(e) = config_file::save(&self.prefs_path, &prefs) {
            if rebound {
                // Keep what is registered consistent with what is saved.
                let _ = rebind_peek(app, &prefs.peek_shortcut, &old.peek_shortcut);
            }
            return Err(e.into());
        }
        *lock(&self.prefs) = prefs.clone();
        self.engine.set_auto(prefs.auto())?;
        window::apply_prefs(app, &prefs, self.is_peeking());
        tray::sync(app);
        Ok(prefs)
    }

    /// `prompt_ax`: on the first start without Accessibility, ask macOS to show its prompt
    /// once per run — that also lists timewent under Privacy → Accessibility, so the user
    /// only has to flip the switch.
    pub fn start(&self, app: &AppHandle, prompt_ax: bool) -> Result<Status> {
        if prompt_ax
            && !permissions().accessibility
            && !self.ax_prompted.swap(true, Ordering::SeqCst)
        {
            request_accessibility();
        }
        self.engine.start()?;
        tray::sync(app);
        Ok(self.status())
    }

    pub fn stop(&self, app: &AppHandle) -> Result<Status> {
        self.engine.stop()?;
        tray::sync(app);
        Ok(self.status())
    }

    pub fn status(&self) -> Status {
        self.engine.status(permissions())
    }
}

/// Registers the peek key at launch. A failure is logged, not fatal: the menu-bar icon
/// still peeks.
pub fn register_peek(app: &AppHandle, accelerator: &str) {
    let result = parse_peek(accelerator)
        .map_err(Error::Shortcut)
        .and_then(|s| {
            app.global_shortcut()
                .register(s)
                .map_err(|e| taken(accelerator, &e))
        });
    if let Err(e) = result {
        eprintln!("timewent: peek key: {e}");
    }
}

fn rebind_peek(app: &AppHandle, old: &str, new: &str) -> Result<()> {
    let new_key = parse_peek(new).map_err(Error::Shortcut)?;
    let shortcuts = app.global_shortcut();
    shortcuts.register(new_key).map_err(|e| taken(new, &e))?;
    if let Ok(old_key) = parse_peek(old) {
        if old_key != new_key {
            if let Err(e) = shortcuts.unregister(old_key) {
                eprintln!("timewent: could not release old peek key {old}: {e}");
            }
        }
    }
    Ok(())
}

fn taken(accelerator: &str, e: &tauri_plugin_global_shortcut::Error) -> Error {
    Error::Shortcut(format!(
        "{accelerator}: taken by another app — try another ({e})"
    ))
}

/// Registers / removes the LaunchAgent. Only from a built app bundle (see `login.rs`).
fn set_login_item(app: &AppHandle, on: bool) -> Result<()> {
    use tauri_plugin_autostart::ManagerExt;
    let in_bundle = std::env::current_exe().is_ok_and(|p| crate::login::in_app_bundle(&p));
    if !in_bundle {
        return Err(Error::InvalidPrefs(
            "open at login works only from the built app".into(),
        ));
    }
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| Error::InvalidPrefs(format!("open at login: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Clock, ProbeFactory};
    use crate::language::LanguagePref;
    use timewent_core::{Config, Range};
    use timewent_probe::{FakeProbe, Probe, WhenExhausted};
    use timewent_store::Store;

    fn state_with(language: LanguagePref) -> AppState {
        let now = crate::clock::now_ms();
        let mut store = Store::open_in_memory().expect("store");
        let id = store.start_session(now - 60_000).expect("session");
        // 20s coding, a 2s YouTube peek (absorbed), 20s coding: one explain line.
        for i in 0..42 {
            let s = if (20..22).contains(&i) {
                crate::testkit::web(now - 60_000 + i * 1000, "https://youtube.com/w", "v")
            } else {
                crate::testkit::code(now - 60_000 + i * 1000, "p", "a.rs")
            };
            store.append(id, &s).expect("append");
        }
        store.end_session(id, now - 10_000).expect("end");
        let probe: ProbeFactory = Box::new(|| {
            let p = FakeProbe::new(
                vec![crate::testkit::code(0, "p", "a.rs")],
                WhenExhausted::HoldLast,
            );
            Box::new(p.expect("script")) as Box<dyn Probe + Send>
        });
        let clock = Clock {
            now_ms: crate::clock::now_ms,
            day_start_ms: |now| now - 3_600_000,
            week_start_ms: |now| now - 7_200_000,
            input_now: || timewent_probe::InputNow {
                idle_s: 1e9,
                locked: false,
            },
            offset_at: |_| 3 * 3600,
            timezone: || "Europe/Istanbul".into(),
        };
        let engine = Engine::new(store, Config::default(), None, probe, clock).expect("engine");
        let dir = std::env::temp_dir();
        AppState::new(
            engine,
            dir.join("x.db"),
            Prefs {
                language,
                ..Prefs::default()
            },
            dir.join("timewent-test-prefs.json"),
            dir.join("timewent-test-window.json"),
        )
    }

    #[test]
    fn exports_speak_the_language_in_the_prefs() {
        // §23.2a: the user's prefs said "tr", the export's `why` came out in English.
        let tr = state_with(LanguagePref::Tr)
            .export_text(Range::Today)
            .expect("tr");
        assert!(tr.contains("YouTube (2sn) bu etkinliğe katıldı"), "{tr}");
        let en = state_with(LanguagePref::En)
            .export_text(Range::Today)
            .expect("en");
        assert!(en.contains("absorbed YouTube (2s)"), "{en}");
    }
}
