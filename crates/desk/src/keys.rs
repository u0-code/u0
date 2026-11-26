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

use gpui::component::kbd::Kbd;
use gpui::{Action, AnyWindowHandle, App, Context, Entity, KeyBinding, Keystroke, Window, actions};
use serde::Deserialize;

use crate::{desk::Desk, pages::Tab};

actions!(
    desk,
    [
        Quit,
        NewSession,
        NewHere,
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
        WholeRight,
        Usage,
        LightOrDark,
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
        NextArea,
        PreviousArea,
        NextTerminal,
        PreviousTerminal,
        GoToFile,
        SearchWords,
        About,
        Hide,
        HideOthers,
        ShowAll,
        Stash,
        SendAside,
        TranscriptView,
        OpenSurface,
        ReopenSurface,
        CloseSurface,
        PullRequests,
        Issues,
        ClearTerminal,
        EditQueued,
        PreviousTurn,
        NextTurn,
        FullScreen,
        ZoomIn,
        ZoomOut,
        ActualSize,
        StartSpeaking,
        StopSpeaking,
        CheckForUpdates,
        ZoomWindow,
        BringAllToFront,
        PairDevice,
        ShowPairingCode
    ]
);

/// Each action a binding of yours can name: its name, what it does, and
/// the action. The keys page lists them in this order.
type Make = fn() -> Box<dyn Action>;
pub const ACTIONS: [(&str, &str, Make); 52] = [
    ("palette", "Go to anything", || Box::new(Palette)),
    ("new_session", "New session", || Box::new(NewSession)),
    ("new_here", "New session in this project", || {
        Box::new(NewHere)
    }),
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
    (
        "whole_right",
        "Widen the right column over the thread",
        || Box::new(WholeRight),
    ),
    ("usage", "What sessions spent, and what runs", || {
        Box::new(Usage)
    }),
    ("light_or_dark", "Light or dark", || Box::new(LightOrDark)),
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
    (
        "previous_area",
        "Keys to the part of the window before",
        || Box::new(PreviousArea),
    ),
    ("next_terminal", "The next terminal", || {
        Box::new(NextTerminal)
    }),
    ("previous_terminal", "The terminal before", || {
        Box::new(PreviousTerminal)
    }),
    ("go_to_file", "Go to a file", || Box::new(GoToFile)),
    ("search_words", "Search the project's files", || {
        Box::new(SearchWords)
    }),
    ("open_surface", "Open a tab in the right column", || {
        Box::new(OpenSurface)
    }),
    ("reopen_surface", "Reopen the tab closed last", || {
        Box::new(ReopenSurface)
    }),
    ("close_surface", "Close the right column's tab", || {
        Box::new(CloseSurface)
    }),
    ("pull_requests", "Pull requests", || Box::new(PullRequests)),
    ("issues", "Issues", || Box::new(Issues)),
    ("clear_terminal", "Clear the terminal", || {
        Box::new(ClearTerminal)
    }),
    ("previous_turn", "To what you said before", || {
        Box::new(PreviousTurn)
    }),
    ("next_turn", "To what you said next", || Box::new(NextTurn)),
    ("full_screen", "Full screen, or not", || {
        Box::new(FullScreen)
    }),
    (
        "transcript_view",
        "Transcript view: normal, thinking, verbose",
        || Box::new(TranscriptView),
    ),
    ("zoom_in", "Everything larger", || Box::new(ZoomIn)),
    ("zoom_out", "Everything smaller", || Box::new(ZoomOut)),
    ("actual_size", "Everything at its actual size", || {
        Box::new(ActualSize)
    }),
    ("check_for_updates", "Check for updates", || {
        Box::new(CheckForUpdates)
    }),
    ("pair_device", "Pair a device: control another Mac", || {
        Box::new(PairDevice)
    }),
    (
        "show_pairing_code",
        "Show pairing code: let another device control this Mac",
        || Box::new(ShowPairingCode),
    ),
];

/// The Keyboard page's groups, as t3code's: each action named in one;
/// one named in none shows under Other.
pub const GROUPS: [(&str, &[&str]); 6] = [
    (
        "Sessions",
        &[
            "new_session",
            "new_here",
            "new_scratch",
            "next_session",
            "previous_session",
            "settle",
            "pin",
            "copy_id",
        ],
    ),
    (
        "The thread",
        &[
            "find",
            "stop",
            "model",
            "mode",
            "effort",
            "dictate",
            "previous_turn",
            "next_turn",
            "transcript_view",
        ],
    ),
    (
        "Going about",
        &[
            "palette",
            "go_to_file",
            "search_words",
            "back",
            "ahead",
            "next_area",
            "toggle_sidebar",
            "settings",
            "usage",
        ],
    ),
    (
        "The column beside",
        &[
            "review",
            "right_column",
            "open_surface",
            "reopen_surface",
            "close_surface",
            "pull_requests",
            "issues",
            "copy_pr",
            "open_folder",
        ],
    ),
    (
        "Terminal",
        &[
            "drawer",
            "next_terminal",
            "previous_terminal",
            "clear_terminal",
        ],
    ),
    (
        "The window",
        &["light_or_dark", "full_screen", "minimize", "quit"],
    ),
];

/// A binding of yours: `action` is a name in `ACTIONS`, or "none" to
/// take the key from whatever has it, or `-` and a name to take the key
/// from that action alone (as VS Code and t3code write it); `when`, where it holds, as a key
/// context predicate over the window's parts ("Terminal", "Composer",
/// with `!`, `&&` and `||`), as t3code's `when` clauses.
#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    key: String,
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when: Option<String>,
}

/// Run the script named so of the repository on screen: bound as
/// `"action": "script:<name>"`, as t3code's `script.{id}.run`.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = desk, no_json)]
pub struct RunScript(pub String);

/// The action named `name` in `ACTIONS`, or a script's (`script:<name>`).
pub fn action(name: &str) -> Option<Box<dyn Action>> {
    if let Some(script) = name.strip_prefix("script:") {
        return Some(Box::new(RunScript(script.to_owned())));
    }
    let (_, _, make) = ACTIONS.iter().find(|(n, _, _)| *n == name)?;
    Some(make())
}

/// A binding of yours as the window's: none where its action, keys or
/// `when` do not read, which is said in the log.
fn binding(cx: &App, b: &Binding) -> Option<KeyBinding> {
    let made = match b.action.strip_prefix('-') {
        _ if b.action == "none" => Some(Box::new(gpui::NoAction) as Box<dyn Action>),
        Some(name) => action(name).map(|_| Box::new(gpui::NoAction) as Box<dyn Action>),
        None => action(&b.action),
    };
    let Some(made) = made else {
        tracing::warn!(
            "keybindings.json: no action is named {:?}; the names are {:?}",
            b.action,
            ACTIONS.map(|(n, _, _)| n)
        );
        return None;
    };
    let when = match b
        .when
        .as_deref()
        .map(gpui::KeyBindingContextPredicate::parse)
    {
        Some(Ok(p)) => Some(std::rc::Rc::new(p)),
        Some(Err(e)) => {
            tracing::warn!(
                "keybindings.json: {:?} does not read as a when: {e}",
                b.when
            );
            return None;
        }
        None => None,
    };
    KeyBinding::load(&b.key, made, when, false, None, cx).ok()
}

/// ⌘ and the digit `n`, which goes to the nth of a list (a session, a
/// choice in a menu open).
pub fn numbered(n: usize) -> Keystroke {
    Keystroke {
        modifiers: gpui::Modifiers::command(),
        key: n.to_string(),
        key_char: None,
    }
}

/// `what`, with the keys the action named `name` is bound to now after it.
pub fn hint(cx: &App, what: &str, name: &str) -> String {
    match written(&keys_of(cx, name)) {
        keys if keys.is_empty() => what.to_owned(),
        keys => format!("{what} · {keys}"),
    }
}

/// The keystrokes the action named `name` is bound to now, yours over the
/// window's; none when it has none.
pub fn keys_of(cx: &App, name: &str) -> Vec<Keystroke> {
    let Some(action) = action(name) else {
        return Vec::new();
    };
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings_for_action(action.as_ref())
        .last()
        .map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(|k| Keystroke {
                    modifiers: *k.modifiers(),
                    key: k.key().into(),
                    key_char: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Keystrokes as gpui-kit's key chip writes them ("⇧⌘M"), one after
/// another.
pub fn written(keys: &[Keystroke]) -> String {
    keys.iter().map(Kbd::format).collect::<Vec<_>>().join(" ")
}

/// What you set for an action's keys.
pub enum Keys {
    /// Ghost's again: yours for it taken away.
    Ghosts,
    /// These ("cmd-shift-k").
    Yours(String),
    /// None: the keys it has now taken from it.
    Off(Vec<String>),
}

/// Set the keys of the action `name` in your keybindings.json as `keys`
/// says: your bindings now. A file that does not read is left alone.
/// Blocking.
pub fn set_yours(home: &Path, name: &str, keys: Keys) -> Result<Vec<Binding>, String> {
    let path = home.join("keybindings.json");
    let mut list: Vec<Binding> = crate::whole::read_json(&path)?;
    let off = format!("-{name}");
    list.retain(|b| b.action != name && b.action != off);
    let bound = |key, action: &str| Binding {
        key,
        action: action.to_owned(),
        when: None,
    };
    match keys {
        Keys::Ghosts => {}
        Keys::Yours(key) => list.push(bound(key, name)),
        Keys::Off(keys) => list.extend(keys.into_iter().map(|k| bound(k, &off))),
    }
    let text = serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?;
    crate::whole::write(&path, &text).map_err(|e| e.to_string())?;
    Ok(list)
}

/// The action other than `name` that `key` is bound to now, if one is.
pub fn taken(cx: &App, name: &str, key: &Keystroke) -> Option<&'static str> {
    let key = Kbd::format(key);
    ACTIONS
        .iter()
        .filter(|(n, _, _)| *n != name)
        .find(|(n, _, _)| written(&keys_of(cx, n)) == key)
        .map(|(_, does, _)| *does)
}

/// The bindings in `home`'s `keybindings.json`; none where there is none,
/// or (said in the log) where it does not read.
pub fn yours(home: &Path) -> Vec<Binding> {
    crate::whole::read_json(&home.join("keybindings.json")).unwrap_or_else(|e| {
        tracing::warn!("{e}");
        Vec::new()
    })
}

/// Bind the window's keys, then `yours` over them.
pub fn bind(cx: &mut App, yours: &[Binding]) {
    let composer = Some(crate::desk::mention::CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-n", NewSession, None),
        KeyBinding::new("cmd-shift-n", NewHere, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-.", Stop, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-f", Find, None),
        KeyBinding::new("cmd-j", Drawer, None),
        KeyBinding::new("cmd-k", Palette, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-}", NextSession, None),
        KeyBinding::new("cmd-{", PreviousSession, None),
        KeyBinding::new("cmd-shift-m", Model, None),
        KeyBinding::new("cmd-d", Review, None),
        KeyBinding::new("cmd-alt-b", RightColumn, None),
        KeyBinding::new("cmd-shift-b", WholeRight, None),
        KeyBinding::new("cmd-u", Usage, None),
        KeyBinding::new("cmd-alt-a", LightOrDark, None),
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
        KeyBinding::new("cmd-t", OpenSurface, None),
        KeyBinding::new("cmd-shift-t", ReopenSurface, None),
        KeyBinding::new("cmd-w", CloseSurface, None),
        KeyBinding::new("cmd-alt-]", Ahead, None),
        KeyBinding::new("f6", NextArea, None),
        KeyBinding::new("shift-f6", PreviousArea, None),
        KeyBinding::new("cmd-}", NextTerminal, Some(crate::desk::runs::CONTEXT)),
        KeyBinding::new("cmd-{", PreviousTerminal, Some(crate::desk::runs::CONTEXT)),
        KeyBinding::new("cmd-p", GoToFile, None),
        KeyBinding::new("cmd-shift-f", SearchWords, None),
        KeyBinding::new("ctrl-o", TranscriptView, None),
        KeyBinding::new("ctrl-shift-up", PreviousTurn, None),
        KeyBinding::new("ctrl-cmd-f", FullScreen, None),
        KeyBinding::new("ctrl-shift-down", NextTurn, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-1", Session1, None),
        KeyBinding::new("cmd-2", Session2, None),
        KeyBinding::new("cmd-3", Session3, None),
        KeyBinding::new("cmd-4", Session4, None),
        KeyBinding::new("cmd-5", Session5, None),
        KeyBinding::new("cmd-6", Session6, None),
        KeyBinding::new("cmd-7", Session7, None),
        KeyBinding::new("cmd-8", Session8, None),
        KeyBinding::new("cmd-9", Session9, None),
        // ⌘+ is ⌘= on most keyboards, and the key written for it.
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ActualSize, None),
        KeyBinding::new("cmd-s", Stash, composer),
        KeyBinding::new("cmd-shift-enter", SendAside, composer),
        KeyBinding::new("alt-up", EditQueued, composer),
        // Pasted as text, however long, not attached.
        KeyBinding::new("cmd-shift-v", gpui::component::input::Paste, composer),
    ]);
    let yours: Vec<KeyBinding> = yours.iter().filter_map(|b| binding(cx, b)).collect();
    cx.bind_keys(yours);
}

/// The desk, and the window it is drawn in under gpui-kit's root.
#[derive(Clone)]
pub struct Shown {
    pub window: AnyWindowHandle,
    pub desk: Entity<Desk>,
}

impl Shown {
    pub(crate) fn update<R>(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut Desk, &mut Window, &mut Context<Desk>) -> R,
    ) -> Option<R> {
        self.window
            .update(cx, |_, window, cx| {
                self.desk.update(cx, |d, cx| f(d, window, cx))
            })
            .ok()
    }
}

/// What each action does in the window.
pub fn handle(cx: &mut App, shown: &Shown) {
    let quitting = shown.clone();
    cx.on_action(move |_: &Quit, cx| {
        let shown = quitting.clone();
        cx.defer(move |cx| {
            let quit = shown.update(cx, |d, _, cx| {
                let may = d.may_quit(crate::macos::key_held(), cx);
                cx.notify();
                may
            });
            if quit.unwrap_or(true) {
                cx.quit();
            }
        });
    });
    let scripts = shown.clone();
    cx.on_action(move |run: &RunScript, cx| {
        let (shown, name) = (scripts.clone(), run.0.clone());
        cx.defer(move |cx| {
            shown.update(cx, |d, window, cx| d.run_script(&name, window, cx));
        });
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    on::<About>(cx, shown, |d, window, cx| {
        d.open_tab(Tab::About, window, cx);
    });
    on::<Minimize>(cx, shown, |_, window, _| window.minimize_window());
    on::<NewSession>(cx, shown, Desk::new_anywhere);
    on::<NewHere>(cx, shown, Desk::new_default);
    on::<Find>(cx, shown, Desk::toggle_find);
    on::<Drawer>(cx, shown, Desk::toggle_drawer);
    on::<Palette>(cx, shown, Desk::toggle_palette);
    on::<OpenSettings>(cx, shown, |d, window, cx| {
        if d.pages.open {
            d.close_settings(window, cx);
        } else {
            d.open_tab(d.pages.tab, window, cx);
        }
    });
    on::<NextSession>(cx, shown, |d, window, cx| d.step_session(1, window, cx));
    on::<PreviousSession>(cx, shown, |d, window, cx| d.step_session(-1, window, cx));
    on::<Model>(cx, shown, Desk::open_model);
    on::<ToggleSidebar>(cx, shown, |d, _, cx| d.toggle_sidebar(cx));
    on::<Stop>(cx, shown, |d, _, cx| d.stop(cx));
    on::<Review>(cx, shown, |d, _, cx| {
        d.show_changes(crate::git::Scope::All, cx);
    });
    on::<RightColumn>(cx, shown, |d, _, cx| d.toggle_aside(cx));
    on::<WholeRight>(cx, shown, |d, _, cx| d.toggle_whole(cx));
    on::<OpenSurface>(cx, shown, |d, _, cx| d.toggle_surfaces(cx));
    on::<ReopenSurface>(cx, shown, |d, _, cx| d.reopen_tab(cx));
    on::<CloseSurface>(cx, shown, |d, _, cx| d.close_shown(cx));
    on::<ClearTerminal>(cx, shown, |d, _, cx| d.clear_terminal(cx));
    on::<PullRequests>(cx, shown, |d, window, cx| {
        d.open_forge(crate::github::Kind::Pr, window, cx);
    });
    on::<Issues>(cx, shown, |d, window, cx| {
        d.open_forge(crate::github::Kind::Issue, window, cx);
    });
    on::<Usage>(cx, shown, |d, window, cx| {
        d.open_tab(Tab::Usage, window, cx);
    });
    on::<LightOrDark>(cx, shown, |d, _, cx| {
        // From the look as drawn now, the other one.
        d.settings.look = if crate::theme::is_light() {
            crate::settings::Look::Dark
        } else {
            crate::settings::Look::Light
        };
        d.save_settings();
        cx.notify();
    });
    on::<Mode>(cx, shown, |d, _, cx| d.open_mode(cx));
    on::<Effort>(cx, shown, |d, window, cx| {
        d.open_model(window, cx);
    });
    on::<TranscriptView>(cx, shown, |d, _, cx| d.set_view(d.settings.view.next(), cx));
    on::<PreviousTurn>(cx, shown, |d, _, cx| d.step_turn(false, cx));
    on::<NextTurn>(cx, shown, |d, _, cx| d.step_turn(true, cx));
    on::<FullScreen>(cx, shown, |_, window, _| window.toggle_fullscreen());
    on::<ZoomIn>(cx, shown, |d, _, cx| {
        rescale(d, crate::settings::next_scale(d.settings.scale, true), cx);
    });
    on::<ZoomOut>(cx, shown, |d, _, cx| {
        rescale(d, crate::settings::next_scale(d.settings.scale, false), cx);
    });
    on::<ActualSize>(cx, shown, |d, _, cx| rescale(d, 100, cx));
    on::<CopyPr>(cx, shown, |d, _, cx| {
        d.copy_of_session(|s| s.git.pr.as_ref().map(|p| p.url.clone()), cx);
    });
    on::<CopyId>(cx, shown, |d, _, cx| {
        d.copy_of_session(|s| s.meta.id.as_ref().map(|i| i.0.to_string()), cx);
    });
    on::<Settle>(cx, shown, |d, _, cx| {
        if let Some(key) = d.current {
            d.toggle_settled(key, cx);
        }
    });
    on::<Pin>(cx, shown, |d, _, cx| {
        if let Some(key) = d.current {
            d.toggle_pin(key, cx);
        }
    });
    on::<TakeBack>(cx, shown, |d, _, cx| d.take_back(cx));
    on::<OpenFolder>(cx, shown, |d, _, _| {
        if let Some(s) = d.session() {
            d.edit(s.meta.folder.clone(), None);
        }
    });
    on::<StartSpeaking>(cx, shown, |d, _, cx| d.read_aloud(cx));
    on::<CheckForUpdates>(cx, shown, Desk::check_update_shown);
    on::<PairDevice>(cx, shown, |d, window, cx| {
        d.set_control(true, window, cx);
        d.open_tab(Tab::Devices, window, cx);
        d.pages.pair_link.update(cx, |i, cx| i.focus(window, cx));
    });
    on::<ShowPairingCode>(cx, shown, |d, window, cx| {
        d.share(crate::share::Share::Serve(true));
        d.open_tab(Tab::Devices, window, cx);
    });
    on::<ZoomWindow>(cx, shown, |_, window, _| window.zoom_window());
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.on_action(|_: &StopSpeaking, _| crate::voice::stop_speaking());
    on::<Dictate>(cx, shown, Desk::dictate);
    on::<NewScratch>(cx, shown, Desk::new_scratch);
    on::<Back>(cx, shown, |d, window, cx| d.step_trail(false, window, cx));
    on::<Ahead>(cx, shown, |d, window, cx| d.step_trail(true, window, cx));
    on::<GoToFile>(cx, shown, |d, window, cx| {
        d.open_palette(crate::desk::palette::Through::Files, window, cx);
    });
    on::<SearchWords>(cx, shown, |d, window, cx| {
        d.open_palette(crate::desk::palette::Through::Words, window, cx);
    });
    on::<NextArea>(cx, shown, |d, window, cx| d.next_area(false, window, cx));
    on::<PreviousArea>(cx, shown, |d, window, cx| d.next_area(true, window, cx));
    on::<NextTerminal>(cx, shown, |d, _, cx| d.next_terminal(false, cx));
    on::<PreviousTerminal>(cx, shown, |d, _, cx| d.next_terminal(true, cx));
    on::<Session1>(cx, shown, |d, window, cx| d.nth_session(0, window, cx));
    on::<Session2>(cx, shown, |d, window, cx| d.nth_session(1, window, cx));
    on::<Session3>(cx, shown, |d, window, cx| d.nth_session(2, window, cx));
    on::<Session4>(cx, shown, |d, window, cx| d.nth_session(3, window, cx));
    on::<Session5>(cx, shown, |d, window, cx| d.nth_session(4, window, cx));
    on::<Session6>(cx, shown, |d, window, cx| d.nth_session(5, window, cx));
    on::<Session7>(cx, shown, |d, window, cx| d.nth_session(6, window, cx));
    on::<Session8>(cx, shown, |d, window, cx| d.nth_session(7, window, cx));
    on::<Session9>(cx, shown, |d, window, cx| d.nth_session(8, window, cx));
}

/// Do `f` to the desk on each `A`, once the action's dispatch is over: a
/// key or a menu dispatches in the window, which is lent to that dispatch
/// until it ends, so updating the window from inside it fails.
/// The whole window drawn at `scale` percent, kept for the next run.
fn rescale(d: &mut Desk, scale: u16, cx: &mut Context<Desk>) {
    d.settings.scale = scale;
    d.save_settings();
    cx.notify();
}

fn on<A: Action>(cx: &mut App, shown: &Shown, f: fn(&mut Desk, &mut Window, &mut Context<Desk>)) {
    let shown = shown.clone();
    cx.on_action(move |_: &A, cx| {
        let shown = shown.clone();
        cx.defer(move |cx| {
            shown.update(cx, f);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_of_yours_replaces_your_last_for_that_action_and_a_broken_file_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        set_yours(home, "palette", Keys::Yours("cmd-shift-k".into())).unwrap();
        let list = set_yours(home, "palette", Keys::Yours("cmd-y".into())).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(yours(home)[0].key, "cmd-y");
        // Taken away, its keys go to nothing; given back, nothing of yours is left.
        let list = set_yours(home, "palette", Keys::Off(vec!["cmd-k".into()])).unwrap();
        assert_eq!((list.len(), list[0].action.as_str()), (1, "-palette"));
        assert!(set_yours(home, "palette", Keys::Ghosts).unwrap().is_empty());
        std::fs::write(home.join("keybindings.json"), "{ not json").unwrap();
        assert!(set_yours(home, "palette", Keys::Ghosts).is_err());
        assert_eq!(
            std::fs::read_to_string(home.join("keybindings.json")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn each_name_a_binding_can_take_is_an_action() {
        for (name, _, _) in ACTIONS {
            assert!(action(name).is_some(), "{name}");
        }
        assert!(action("no_such_action").is_none());
    }

    #[test]
    fn a_when_of_yours_reads_as_the_window_s_key_contexts() {
        let b: Binding = serde_json::from_str(
            r#"{"key":"cmd-}","action":"next_terminal","when":"Terminal && !Composer"}"#,
        )
        .unwrap();
        let when = b.when.unwrap();
        assert!(gpui::KeyBindingContextPredicate::parse(&when).is_ok());
        assert!(gpui::KeyBindingContextPredicate::parse("Terminal &&").is_err());
        assert!(when.contains(crate::desk::runs::CONTEXT));
    }
}
