//! The window's keys: what each does, the keys it is bound to, and those
//! you bind yourself in `~/.ghost/keybindings.json`, as t3code takes its
//! keymap from a file:
//!
//! ```json
//! [ { "key": "cmd-k", "action": "palette" },
//!   { "key": "cmd-shift-n", "action": "new_session" } ]
//! ```
//!
//! The actions are named as in `ACTIONS`. A binding of yours comes after the
//! window's own, so it wins; a file that does not read is said so in the
//! log and left out.

use std::path::Path;

use agent_client_protocol_schema::v1::SessionConfigOptionCategory as Kind;
use gpui::{Action, App, KeyBinding, WindowHandle, actions};
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
        Session9,
        Review,
        RightColumn,
        Mode,
        Effort,
        CopyPr,
        CopyId,
        Settle,
        Pin,
        TakeBack,
        OpenFolder,
        Dictate,
        NewScratch,
        Back,
        Ahead,
        NextArea
    ]
);

/// Each action a binding of yours can name: its name, what it does, and
/// the action. The keys page lists them in this order.
type Make = fn() -> Box<dyn Action>;
pub const ACTIONS: [(&str, &str, Make); 27] = [
    ("palette", "Go to anything", || Box::new(Palette)),
    ("new_session", "New session", || Box::new(NewSession)),
    ("find", "Find in the session", || Box::new(Find)),
    ("drawer", "Terminal", || Box::new(Drawer)),
    ("model", "Choose the model", || Box::new(Model)),
    ("stop", "Stop the turn", || Box::new(Stop)),
    ("next_session", "Next session", || Box::new(NextSession)),
    ("previous_session", "Previous session", || {
        Box::new(PreviousSession)
    }),
    ("toggle_sidebar", "Show or hide the sidebar", || {
        Box::new(ToggleSidebar)
    }),
    ("settings", "Settings", || Box::new(OpenSettings)),
    ("minimize", "Minimize the window", || Box::new(Minimize)),
    ("quit", "Quit Ghost", || Box::new(Quit)),
    ("review", "Review the changes", || Box::new(Review)),
    ("right_column", "Show or hide the right column", || {
        Box::new(RightColumn)
    }),
    ("mode", "Choose the mode", || Box::new(Mode)),
    ("effort", "Choose how hard it thinks", || Box::new(Effort)),
    ("copy_pr", "Copy the pull request's address", || {
        Box::new(CopyPr)
    }),
    ("copy_id", "Copy the session's id", || Box::new(CopyId)),
    ("settle", "Done, or not done", || Box::new(Settle)),
    ("pin", "Pin, or unpin", || Box::new(Pin)),
    (
        "take_back",
        "Take back what was deleted or archived",
        || Box::new(TakeBack),
    ),
    ("open_folder", "Open its folder in your editor", || {
        Box::new(OpenFolder)
    }),
    ("dictate", "Dictate", || Box::new(Dictate)),
    ("new_scratch", "New scratch session", || {
        Box::new(NewScratch)
    }),
    ("back", "Back to the session before", || Box::new(Back)),
    ("ahead", "Ahead again", || Box::new(Ahead)),
    ("next_area", "Keys to the next part of the window", || {
        Box::new(NextArea)
    }),
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
    let (_, _, make) = ACTIONS.iter().find(|(n, _, _)| *n == name)?;
    KeyBinding::load(key, make(), None, false, None, &gpui::DummyKeyboardMapper).ok()
}

/// The keys the action named `name` is bound to now, yours over the
/// window's, as a Mac writes them ("⌘⇧M"); none when it has none.
pub fn keys_of(cx: &App, name: &str) -> Option<String> {
    let (_, _, make) = ACTIONS.iter().find(|(n, _, _)| *n == name)?;
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let action = make();
    let binding = keymap.bindings_for_action(action.as_ref()).last()?;
    Some(
        binding
            .keystrokes()
            .iter()
            .map(|k| shown(*k.modifiers(), k.key()))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// One keystroke as a Mac writes it.
fn shown(m: gpui::Modifiers, key: &str) -> String {
    let mut out = String::new();
    for (on, mark) in [
        (m.control, "⌃"),
        (m.alt, "⌥"),
        (m.shift, "⇧"),
        (m.platform, "⌘"),
    ] {
        if on {
            out.push_str(mark);
        }
    }
    out.push_str(&match key {
        "enter" => "↩".to_owned(),
        "backspace" => "⌫".to_owned(),
        "escape" => "esc".to_owned(),
        "space" => "Space".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        k => k.to_uppercase(),
    });
    out
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
                        "{}: no action is named {:?}; the names are {:?}",
                        path.display(),
                        b.action,
                        ACTIONS.map(|(n, _, _)| n)
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
        KeyBinding::new("cmd-d", Review, None),
        KeyBinding::new("cmd-alt-b", RightColumn, None),
        KeyBinding::new("cmd-shift-a", Mode, None),
        KeyBinding::new("cmd-shift-e", Effort, None),
        KeyBinding::new("cmd-shift-k", CopyPr, None),
        KeyBinding::new("cmd-shift-c", CopyId, None),
        KeyBinding::new("cmd-shift-s", Settle, None),
        KeyBinding::new("cmd-shift-p", Pin, None),
        KeyBinding::new("cmd-z", TakeBack, None),
        KeyBinding::new("cmd-o", OpenFolder, None),
        KeyBinding::new("cmd-shift-d", Dictate, None),
        KeyBinding::new("cmd-alt-n", NewScratch, None),
        KeyBinding::new("cmd-alt-[", Back, None),
        KeyBinding::new("cmd-shift-t", Back, None),
        KeyBinding::new("cmd-alt-]", Ahead, None),
        KeyBinding::new("f6", NextArea, None),
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
        KeyBinding::new("alt-left", input::WordLeft, input),
        KeyBinding::new("alt-right", input::WordRight, input),
        KeyBinding::new("alt-shift-left", input::SelectWordLeft, input),
        KeyBinding::new("alt-shift-right", input::SelectWordRight, input),
        KeyBinding::new("cmd-left", input::LineStart, input),
        KeyBinding::new("cmd-right", input::LineEnd, input),
        KeyBinding::new("cmd-shift-left", input::SelectLineStart, input),
        KeyBinding::new("cmd-shift-right", input::SelectLineEnd, input),
        KeyBinding::new("alt-backspace", input::DeleteWord, input),
        KeyBinding::new("cmd-backspace", input::DeleteLine, input),
        KeyBinding::new("cmd-z", input::Undo, input),
        KeyBinding::new("cmd-shift-z", input::Redo, input),
        KeyBinding::new("cmd-s", input::Stash, input),
        KeyBinding::new("cmd-shift-enter", input::SubmitAsideToo, input),
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
    cx.on_action(move |_: &Review, cx| {
        let _ = window.update(cx, |d, _, cx| d.show_changes(crate::desk::Scope::All, cx));
    });
    cx.on_action(move |_: &RightColumn, cx| {
        let _ = window.update(cx, |d, _, cx| d.toggle_aside(cx));
    });
    cx.on_action(move |_: &Mode, cx| {
        let _ = window.update(cx, |d, _, cx| d.open_option(&Kind::Mode, cx));
    });
    cx.on_action(move |_: &Effort, cx| {
        let _ = window.update(cx, |d, _, cx| d.open_option(&Kind::ThoughtLevel, cx));
    });
    cx.on_action(move |_: &CopyPr, cx| {
        let _ = window.update(cx, |d, _, cx| {
            d.copy_of_session(|s| s.pr.as_ref().map(|p| p.url.clone()), cx);
        });
    });
    cx.on_action(move |_: &CopyId, cx| {
        let _ = window.update(cx, |d, _, cx| {
            d.copy_of_session(|s| s.id.as_ref().map(|i| i.0.to_string()), cx);
        });
    });
    cx.on_action(move |_: &Settle, cx| {
        let _ = window.update(cx, |d, _, cx| {
            if let Some(key) = d.current {
                d.toggle_settled(key, cx);
            }
        });
    });
    cx.on_action(move |_: &Pin, cx| {
        let _ = window.update(cx, |d, _, cx| {
            if let Some(key) = d.current {
                d.toggle_pin(key, cx);
            }
        });
    });
    cx.on_action(move |_: &TakeBack, cx| {
        let _ = window.update(cx, |d, _, cx| d.take_back(cx));
    });
    cx.on_action(move |_: &OpenFolder, cx| {
        let _ = window.update(cx, |d, _, _| {
            if let Some(s) = d.session() {
                crate::desk::open_in_editor(&s.folder);
            }
        });
    });
    cx.on_action(move |_: &Dictate, cx| {
        let _ = window.update(cx, |d, window, cx| {
            window.focus(&gpui::Focusable::focus_handle(d.input.read(cx), cx));
            crate::icon::dictate();
        });
    });
    cx.on_action(move |_: &NewScratch, cx| {
        let _ = window.update(cx, Desk::new_scratch);
    });
    cx.on_action(move |_: &Back, cx| {
        let _ = window.update(cx, |d, window, cx| d.step_trail(false, window, cx));
    });
    cx.on_action(move |_: &Ahead, cx| {
        let _ = window.update(cx, |d, window, cx| d.step_trail(true, window, cx));
    });
    cx.on_action(move |_: &NextArea, cx| {
        let _ = window.update(cx, Desk::next_area);
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
        for (name, _, _) in ACTIONS {
            assert!(binding("cmd-k", name).is_some(), "{name}");
        }
        assert_eq!(shown(gpui::Modifiers::command_shift(), "m"), "⇧⌘M");
        assert!(binding("cmd-k", "no_such_action").is_none());
    }
}
