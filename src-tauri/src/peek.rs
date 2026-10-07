//! Peek (DESIGN §11.3): one key shows the panel wherever you are, the same key puts everything
//! back exactly as it was — including which app had the keyboard. Pure: facts in, actions
//! out; `window.rs` gathers the facts and performs the actions.

use serde::{Deserialize, Serialize};

/// The ui's layout, as last reported by the ui (`set_layout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Pill,
    Expanded,
}

/// The window as it is right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowNow {
    pub visible: bool,
    pub layout: Layout,
}

/// Everything a transition depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub window: WindowNow,
    /// The user's keep-on-top pref, applied when a peek ends.
    pub always_on_top: bool,
    /// timewent has the keyboard right now.
    pub self_frontmost: bool,
    /// The app to give the keyboard back to: the frontmost app, or — while timewent is
    /// frontmost — the app that still owns the menu bar (an accessory app never does).
    pub other_app: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The global peek shortcut.
    Key,
    /// `esc` in the ui while peeking: one step down, to the pill (closes only).
    Esc,
    /// Left click on the menu-bar icon.
    TrayClick,
    /// The window was just hidden by the user (esc on the pill, ×, ⌘W, tray).
    WindowHidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Peek {
    #[default]
    Closed,
    /// `before`: what to restore; `return_to`: the app to hand the keyboard back to.
    Open {
        before: WindowNow,
        return_to: Option<i32>,
    },
}

/// Payload of the `peek` event to the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PeekEvent {
    pub open: bool,
    /// Closing: the layout to return to. Opening: always expanded.
    pub mode: Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Join every Space (incl. full-screen ones) — or stop doing so.
    AllSpaces(bool),
    OnTop(bool),
    Show,
    Focus,
    Hide,
    Emit(PeekEvent),
    /// Re-activate this app (no-op if it has quit since).
    Activate(i32),
}

/// The keyboard goes back only if timewent still has it: if the user already clicked into
/// another app, we never pull them away from it.
fn hand_back(to: Option<i32>, facts: &Facts) -> Option<Action> {
    to.filter(|_| facts.self_frontmost).map(Action::Activate)
}

fn end(mode: Layout, facts: &Facts) -> Vec<Action> {
    vec![
        Action::Emit(PeekEvent { open: false, mode }),
        Action::AllSpaces(false),
        Action::OnTop(facts.always_on_top),
    ]
}

/// One transition (DESIGN §11.3 + "Esc & position fix"):
/// - peek key / tray click: full toggle — open expanded, or back to the exact pre-peek state
///   (hidden again if it was hidden) and the keyboard back to the previous app;
/// - esc while peeking: one step down — the pill, still focused; the peek is over;
/// - window hidden (by esc on the pill, ×, ⌘W): the keyboard goes back to the previous app.
pub fn step(state: Peek, event: Event, facts: Facts) -> (Peek, Vec<Action>) {
    // Hidden during a peek without us hearing about it: that peek is over.
    let state = match (state, event) {
        (Peek::Open { .. }, Event::Key | Event::Esc | Event::TrayClick)
            if !facts.window.visible =>
        {
            Peek::Closed
        }
        (s, _) => s,
    };
    match (state, event) {
        (Peek::Closed, Event::Esc) => (Peek::Closed, vec![]),
        (Peek::Closed, Event::WindowHidden) => (
            Peek::Closed,
            hand_back(facts.other_app, &facts).into_iter().collect(),
        ),
        (Peek::Closed, Event::Key | Event::TrayClick) => (
            Peek::Open {
                before: facts.window,
                return_to: facts.other_app,
            },
            vec![
                Action::AllSpaces(true),
                Action::OnTop(true),
                Action::Emit(PeekEvent {
                    open: true,
                    mode: Layout::Expanded,
                }),
                Action::Show,
                Action::Focus,
            ],
        ),
        (Peek::Open { .. }, Event::Esc) => (Peek::Closed, end(Layout::Pill, &facts)),
        (Peek::Open { before, return_to }, Event::WindowHidden) => {
            let mut actions = end(before.layout, &facts);
            actions.extend(hand_back(return_to, &facts));
            (Peek::Closed, actions)
        }
        (Peek::Open { before, return_to }, Event::Key | Event::TrayClick) => {
            let mut actions = end(before.layout, &facts);
            if !before.visible {
                actions.push(Action::Hide);
            }
            actions.extend(hand_back(return_to, &facts));
            (Peek::Closed, actions)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    const HIDDEN: WindowNow = WindowNow {
        visible: false,
        layout: Layout::Pill,
    };
    const PILL: WindowNow = WindowNow {
        visible: true,
        layout: Layout::Pill,
    };
    const EXPANDED: WindowNow = WindowNow {
        visible: true,
        layout: Layout::Expanded,
    };
    /// While peeking the window is visible and the ui reports expanded.
    const PEEKING: WindowNow = EXPANDED;
    const CODE: i32 = 4242;

    /// The user is in VS Code (CODE), timewent in the background.
    fn in_code(window: WindowNow) -> Facts {
        Facts {
            window,
            always_on_top: true,
            self_frontmost: false,
            other_app: Some(CODE),
        }
    }

    /// timewent has the keyboard; VS Code still owns the menu bar.
    fn in_timewent(window: WindowNow) -> Facts {
        Facts {
            self_frontmost: true,
            ..in_code(window)
        }
    }

    fn open(before: WindowNow) -> Peek {
        Peek::Open {
            before,
            return_to: Some(CODE),
        }
    }

    fn open_actions() -> Vec<Action> {
        vec![
            AllSpaces(true),
            OnTop(true),
            Emit(PeekEvent {
                open: true,
                mode: Layout::Expanded,
            }),
            Show,
            Focus,
        ]
    }

    fn close(mode: Layout) -> Action {
        Emit(PeekEvent { open: false, mode })
    }

    #[test]
    fn peek_from_every_state_shows_expanded_focused_on_top_everywhere() {
        for before in [HIDDEN, PILL, EXPANDED] {
            for ev in [Event::Key, Event::TrayClick] {
                let (s, a) = step(Peek::Closed, ev, in_code(before));
                assert_eq!(s, open(before));
                assert_eq!(a, open_actions(), "{before:?} {ev:?}");
            }
        }
    }

    #[test]
    fn opening_from_timewent_itself_remembers_the_menu_bar_owner() {
        let (s, _) = step(Peek::Closed, Event::Key, in_timewent(PILL));
        assert_eq!(s, open(PILL));
    }

    #[test]
    fn the_peek_key_toggles_back_to_the_exact_pre_peek_state() {
        let (s, a) = step(open(HIDDEN), Event::Key, in_timewent(PEEKING));
        assert_eq!(s, Peek::Closed);
        assert_eq!(
            a,
            vec![
                close(Layout::Pill),
                AllSpaces(false),
                OnTop(true),
                Hide,
                Activate(CODE)
            ]
        );
        let (_, a) = step(open(PILL), Event::TrayClick, in_timewent(PEEKING));
        assert_eq!(
            a,
            vec![
                close(Layout::Pill),
                AllSpaces(false),
                OnTop(true),
                Activate(CODE)
            ]
        );
        let (_, a) = step(open(EXPANDED), Event::Key, in_timewent(PEEKING));
        assert_eq!(
            a,
            vec![
                close(Layout::Expanded),
                AllSpaces(false),
                OnTop(true),
                Activate(CODE)
            ]
        );
    }

    #[test]
    fn esc_while_peeking_steps_down_to_the_focused_pill_from_any_start() {
        for before in [HIDDEN, PILL, EXPANDED] {
            let (s, a) = step(open(before), Event::Esc, in_timewent(PEEKING));
            assert_eq!(s, Peek::Closed, "{before:?}");
            // No Hide and no Activate: the pill stays up and keeps the keyboard.
            assert_eq!(
                a,
                vec![close(Layout::Pill), AllSpaces(false), OnTop(true)],
                "{before:?}"
            );
        }
    }

    #[test]
    fn then_hiding_the_pill_hands_the_keyboard_back() {
        let (s, a) = step(Peek::Closed, Event::WindowHidden, in_timewent(HIDDEN));
        assert_eq!((s, a), (Peek::Closed, vec![Activate(CODE)]));
    }

    #[test]
    fn esc_then_esc_is_pill_then_hidden() {
        let (s, _) = step(Peek::Closed, Event::Key, in_code(HIDDEN));
        let (s, a) = step(s, Event::Esc, in_timewent(PEEKING));
        assert!(!a.contains(&Hide) && !a.iter().any(|x| matches!(x, Activate(_))));
        // The ui hides the pill on the second esc and reports it.
        let (s, a) = step(s, Event::WindowHidden, in_timewent(HIDDEN));
        assert_eq!((s, a), (Peek::Closed, vec![Activate(CODE)]));
    }

    #[test]
    fn the_keyboard_is_never_pulled_from_an_app_the_user_already_chose() {
        // The user clicked into VS Code while peeking, then pressed the peek key there.
        let (_, a) = step(open(HIDDEN), Event::Key, in_code(PEEKING));
        assert!(!a.iter().any(|x| matches!(x, Activate(_))));
        let (_, a) = step(Peek::Closed, Event::WindowHidden, in_code(HIDDEN));
        assert!(a.is_empty());
    }

    #[test]
    fn nothing_to_hand_back_without_another_app() {
        let lonely = Facts {
            other_app: None,
            ..in_timewent(PEEKING)
        };
        let peek = Peek::Open {
            before: PILL,
            return_to: None,
        };
        let (_, a) = step(peek, Event::Key, lonely);
        assert!(!a.iter().any(|x| matches!(x, Activate(_))));
    }

    #[test]
    fn closing_restores_keep_on_top_off() {
        let f = Facts {
            always_on_top: false,
            ..in_timewent(PEEKING)
        };
        for ev in [Event::Key, Event::Esc] {
            let (_, a) = step(open(PILL), ev, f);
            assert!(
                a.contains(&OnTop(false)) && !a.contains(&OnTop(true)),
                "{ev:?}"
            );
        }
    }

    #[test]
    fn esc_never_opens_a_peek() {
        for before in [HIDDEN, PILL, EXPANDED] {
            assert_eq!(
                step(Peek::Closed, Event::Esc, in_timewent(before)),
                (Peek::Closed, vec![])
            );
        }
    }

    #[test]
    fn hiding_the_window_during_a_peek_ends_it_and_hands_back() {
        let (s, a) = step(open(PILL), Event::WindowHidden, in_timewent(HIDDEN));
        assert_eq!(s, Peek::Closed);
        assert_eq!(
            a,
            vec![
                close(Layout::Pill),
                AllSpaces(false),
                OnTop(true),
                Activate(CODE)
            ]
        );
    }

    #[test]
    fn a_key_after_an_unnoticed_hide_peeks_again() {
        let hidden_now = WindowNow {
            visible: false,
            layout: Layout::Expanded,
        };
        let (s, a) = step(open(PILL), Event::Key, in_code(hidden_now));
        assert_eq!(s, open(hidden_now));
        assert_eq!(a, open_actions());
    }

    #[test]
    fn ten_round_trips_return_to_closed() {
        for before in [HIDDEN, PILL, EXPANDED] {
            let mut s = Peek::Closed;
            for _ in 0..10 {
                s = step(s, Event::Key, in_code(before)).0;
                s = step(s, Event::Key, in_timewent(PEEKING)).0;
            }
            assert_eq!(s, Peek::Closed);
        }
    }

    #[test]
    fn payload_and_layout_shapes() {
        assert_eq!(
            serde_json::to_value(PeekEvent {
                open: false,
                mode: Layout::Pill
            })
            .expect("json"),
            serde_json::json!({"open": false, "mode": "pill"})
        );
        let l: Layout = serde_json::from_str("\"expanded\"").expect("parse");
        assert_eq!(l, Layout::Expanded);
    }
}
