//! The peek key (PLAN §10.4): parsing and the "is this a sane global shortcut" rule. The
//! registration itself is in `state.rs`, which keeps the old key whenever the new one fails.

use tauri_plugin_global_shortcut::{Modifiers, Shortcut};

pub const DEFAULT_PEEK: &str = "Alt+Shift+Space";

/// A Tauri accelerator (`"Alt+Shift+Space"`) holding at least one of ⌘ ⌃ ⌥ plus one key —
/// anything less would steal ordinary typing system-wide.
pub fn parse_peek(accelerator: &str) -> Result<Shortcut, String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| format!("\"{accelerator}\" is not a shortcut: {e}"))?;
    if !shortcut
        .mods
        .intersects(Modifiers::SUPER | Modifiers::CONTROL | Modifiers::ALT)
    {
        return Err(format!("\"{accelerator}\" needs ⌘, ⌃ or ⌥ plus one key"));
    }
    Ok(shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Code;

    #[test]
    fn default_is_option_shift_space() {
        let s = parse_peek(DEFAULT_PEEK).expect("valid");
        assert_eq!(s.key, Code::Space);
        assert_eq!(s.mods, Modifiers::ALT | Modifiers::SHIFT);
    }

    #[test]
    fn command_control_or_option_qualify() {
        for ok in [
            "Cmd+Shift+T",
            "Super+K",
            "Control+Alt+P",
            "Alt+Space",
            "CmdOrCtrl+F12",
        ] {
            assert!(parse_peek(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn shift_alone_or_no_modifier_is_refused() {
        for bad in ["Space", "Shift+Space", "K"] {
            let err = parse_peek(bad).expect_err(bad);
            assert!(err.contains("needs ⌘, ⌃ or ⌥"), "{err}");
        }
    }

    #[test]
    fn garbage_is_refused_with_the_input_named() {
        for bad in ["", "Alt+", "Alt+Shift", "Alt+Nope"] {
            let err = parse_peek(bad).expect_err(bad);
            assert!(err.contains(&format!("\"{bad}\"")), "{err}");
        }
    }
}
