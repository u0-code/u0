//! The window's keys: what each does, the keys it is bound to, and those
//! you bind yourself in `~/.ghost/keybindings.json`, as t3code takes its
//! keymap from a file:
//!
//! ```json
//! [ { "key": "cmd-k", "action": "palette" },
//!   { "key": "cmd-shift-n", "action": "new_session" } ]
//! ```
//!
//! The actions are named as in `NAMES`. A binding of yours comes after the
//! window's own, so it wins; a file that does not read is said so in the
//! log and left out.

use std::path::Path;

use gpui::{App, KeyBinding, WindowHandle, actions};
use serde::Deserialize;

use crate::{desk::Desk, input};

actions!(
    desk,
    [
        Quit,
        NewSession,
        ToggleSidebar,
        Stop,
        Minimize,
        Find,
        Drawer,
        Palette,
        OpenSettings,
        NextSession,
        PreviousSession,
        Model,
        Session1,
        Session2,
        Session3,
        Session4,
        Session5,
        Session6,
        Session7,
        Session8,
        Session9
    ]
);

/// The actions a binding of yours can name.
pub const NAMES: [&str; 12] = [
    "quit",
    "new_session",
    "toggle_sidebar",
    "stop",
    "minimize",
    "find",
    "drawer",
    "palette",
    "settings",
    "next_session",
    "previous_session",
    "model",
];

/// A binding of yours.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    key: String,
    action: String,
}

/// A binding of `key` to the action named `name`.
fn binding(key: &str, name: &str) -> Option<KeyBinding> {
    Some(match name {
        "quit" => KeyBinding::new(key, Quit, None),
        "new_session" => KeyBinding::new(key, NewSession, None),
        "toggle_sidebar" => KeyBinding::new(key, ToggleSidebar, None),
        "stop" => KeyBinding::new(key, Stop, None),
        "minimize" => KeyBinding::new(key, Minimize, None),
        "find" => KeyBinding::new(key, Find, None),
        "drawer" => KeyBinding::new(key, Drawer, None),
        "palette" => KeyBinding::new(key, Palette, None),
        "settings" => KeyBinding::new(key, OpenSettings, None),
        "next_session" => KeyBinding::new(key, NextSession, None),
        "previous_session" => KeyBinding::new(key, PreviousSession, None),
        "model" => KeyBinding::new(key, Model, None),
        _ => return None,
    })
}

/// The bindings in `home`'s `keybindings.json`; none where there is none.
fn yours(home: &Path) -> Vec<KeyBinding> {
    let path = home.join("keybindings.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<Binding>>(&text) {
        Ok(list) => list
            .iter()
            .filter_map(|b| {
                let found = binding(&b.key, &b.action);
                if found.is_none() {
                    tracing::warn!(
                        "{}: no action is named {:?}; the names are {NAMES:?}",
                        path.display(),
                        b.action
                    );
                }
                found
            })
            .collect(),
        Err(e) => {
            tracing::warn!("{} does not read: {e}", path.display());
            Vec::new()
        }
    }
}

/// Bind the window's keys, then yours.
pub fn bind(cx: &mut App, home: &Path) {
    let input = Some(input::CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-n", NewSession, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-.", Stop, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-f", Find, None),
        KeyBinding::new("cmd-j", Drawer, None),
        KeyBinding::new("cmd-k", Palette, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-]", NextSession, None),
        KeyBinding::new("cmd-[", PreviousSession, None),
        KeyBinding::new("cmd-shift-m", Model, None),
        KeyBinding::new("cmd-1", Session1, None),
        KeyBinding::new("cmd-2", Session2, None),
        KeyBinding::new("cmd-3", Session3, None),
        KeyBinding::new("cmd-4", Session4, None),
        KeyBinding::new("cmd-5", Session5, None),
        KeyBinding::new("cmd-6", Session6, None),
        KeyBinding::new("cmd-7", Session7, None),
        KeyBinding::new("cmd-8", Session8, None),
        KeyBinding::new("cmd-9", Session9, None),
        KeyBinding::new("backspace", input::Backspace, input),
        KeyBinding::new("delete", input::Delete, input),
        KeyBinding::new("left", input::Left, input),
        KeyBinding::new("right", input::Right, input),
        KeyBinding::new("shift-left", input::SelectLeft, input),
        KeyBinding::new("shift-right", input::SelectRight, input),
        KeyBinding::new("cmd-a", input::SelectAll, input),
        KeyBinding::new("cmd-v", input::Paste, input),
        KeyBinding::new("cmd-c", input::Copy, input),
        KeyBinding::new("cmd-x", input::Cut, input),
        KeyBinding::new("home", input::Home, input),
        KeyBinding::new("end", input::End, input),
        KeyBinding::new("enter", input::Submit, input),
        KeyBinding::new("cmd-enter", input::SubmitAside, input),
        KeyBinding::new("shift-enter", input::Newline, input),
        KeyBinding::new("tab", input::Tab, input),
        KeyBinding::new("up", input::Up, input),
        KeyBinding::new("down", input::Down, input),
        KeyBinding::new("escape", input::Cancel, input),
    ]);
    cx.bind_keys(yours(home));
}

/// What each action does in the window.
pub fn handle(cx: &mut App, window: WindowHandle<Desk>) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(move |_: &NewSession, cx| {
        let _ = window.update(cx, Desk::new_default);
    });
    cx.on_action(move |_: &Find, cx| {
        let _ = window.update(cx, Desk::toggle_find);
    });
    cx.on_action(move |_: &Drawer, cx| {
        let _ = window.update(cx, Desk::toggle_drawer);
    });
    cx.on_action(move |_: &Palette, cx| {
        let _ = window.update(cx, Desk::toggle_palette);
    });
    cx.on_action(move |_: &OpenSettings, cx| {
        let _ = window.update(cx, |d, _, cx| d.open_page(crate::desk::Page::Settings, cx));
    });
    cx.on_action(move |_: &NextSession, cx| {
        let _ = window.update(cx, |d, window, cx| d.step_session(1, window, cx));
    });
    cx.on_action(move |_: &PreviousSession, cx| {
        let _ = window.update(cx, |d, window, cx| d.step_session(-1, window, cx));
    });
    cx.on_action(move |_: &Model, cx| {
        let _ = window.update(cx, |d, _, cx| d.open_model(cx));
    });
    cx.on_action(move |_: &ToggleSidebar, cx| {
        let _ = window.update(cx, |d, _, cx| d.toggle_sidebar(cx));
    });
    cx.on_action(move |_: &Stop, cx| {
        let _ = window.update(cx, |d, _, cx| d.stop(cx));
    });
    macro_rules! nth {
        ($($action:ident => $n:expr),*) => {$(
            cx.on_action(move |_: &$action, cx| {
                let _ = window.update(cx, |d, window, cx| d.nth_session($n, window, cx));
            });
        )*};
    }
    nth!(Session1 => 0, Session2 => 1, Session3 => 2, Session4 => 3, Session5 => 4,
         Session6 => 5, Session7 => 6, Session8 => 7, Session9 => 8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_a_binding_can_take_is_an_action() {
        for name in NAMES {
            assert!(binding("cmd-k", name).is_some(), "{name}");
        }
        assert!(binding("cmd-k", "no_such_action").is_none());
    }
}
