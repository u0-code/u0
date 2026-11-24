//! Ghost: one desktop app for the coding agents you already use.
//!
//! Down the left, your projects, each with its sessions, and the agents
//! switched on (Claude Code, Codex, Antigravity, or any other the ACP
//! registry lists). In the middle, the session on screen: talk to the agent and give it work, drawn
//! the same way whichever agent it is. Down the right, what the session has
//! come to. Ghost has no agent of its own: each agent runs as its own
//! program, with its own tools and its own sign-in, spoken to over the
//! Agent Client Protocol.

mod acp;
mod aside;
mod assets;
mod desk;
mod fonts;
mod git;
mod hub;
mod icon;
mod input;
mod markdown;
mod pages;
mod path;
mod registry;
mod session;
mod settings;
mod sidebar;
mod store;
mod style;
mod syntax;
mod theme;
mod thread;
mod ui;
mod when;
mod whole;

use gpui::{
    App, AppContext as _, Application, Bounds, Focusable as _, KeyBinding, Menu, MenuItem,
    OsAction, TitlebarOptions, WindowBounds, WindowOptions, actions, point, px, size,
};

actions!(desk, [Quit, NewSession, ToggleSidebar, Stop, Minimize]);

fn main() {
    // Before any thread starts, since it changes the environment.
    path::widen();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_desk=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            fonts::load(cx);
            icon::set();
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-n", NewSession, None),
                KeyBinding::new("cmd-b", ToggleSidebar, None),
                KeyBinding::new("cmd-.", Stop, None),
                KeyBinding::new("cmd-m", Minimize, None),
                KeyBinding::new("backspace", input::Backspace, Some(input::CONTEXT)),
                KeyBinding::new("delete", input::Delete, Some(input::CONTEXT)),
                KeyBinding::new("left", input::Left, Some(input::CONTEXT)),
                KeyBinding::new("right", input::Right, Some(input::CONTEXT)),
                KeyBinding::new("shift-left", input::SelectLeft, Some(input::CONTEXT)),
                KeyBinding::new("shift-right", input::SelectRight, Some(input::CONTEXT)),
                KeyBinding::new("cmd-a", input::SelectAll, Some(input::CONTEXT)),
                KeyBinding::new("cmd-v", input::Paste, Some(input::CONTEXT)),
                KeyBinding::new("cmd-c", input::Copy, Some(input::CONTEXT)),
                KeyBinding::new("cmd-x", input::Cut, Some(input::CONTEXT)),
                KeyBinding::new("home", input::Home, Some(input::CONTEXT)),
                KeyBinding::new("end", input::End, Some(input::CONTEXT)),
                KeyBinding::new("enter", input::Submit, Some(input::CONTEXT)),
                KeyBinding::new("cmd-enter", input::SubmitAside, Some(input::CONTEXT)),
                KeyBinding::new("escape", input::Cancel, Some(input::CONTEXT)),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &Minimize, cx| {
                if let Some(w) = cx.active_window() {
                    let _ = w.update(cx, |_, window, _| window.minimize_window());
                }
            });
            // Without a main menu macOS has no menu bar to show in full
            // screen, so there is no way out of it. AppKit adds Enter and
            // Exit Full Screen to the View menu, and the window list to
            // the Window menu, by itself.
            cx.set_menus(vec![
                Menu {
                    name: "Ghost".into(),
                    items: vec![MenuItem::action("Quit Ghost", Quit)],
                },
                Menu {
                    name: "File".into(),
                    items: vec![
                        MenuItem::action("New Session", NewSession),
                        MenuItem::action("Stop", Stop),
                    ],
                },
                Menu {
                    name: "Edit".into(),
                    items: vec![
                        MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
                        MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
                        MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
                        MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
                    ],
                },
                Menu {
                    name: "View".into(),
                    items: vec![MenuItem::action("Toggle Sidebar", ToggleSidebar)],
                },
                Menu {
                    name: "Window".into(),
                    items: vec![MenuItem::action("Minimize", Minimize)],
                },
            ]);

            let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Ghost".into()),
                    appears_transparent: true,
                    // The lights share a line with the controls beside them, the
                    // middle of the 44-pixel top row, 18 in from the edge.
                    traffic_light_position: Some(point(px(18.), px(16.))),
                }),
                window_min_size: Some(size(px(800.), px(520.))),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    cx.new(|cx| desk::Desk::new(window, cx))
                })
                .expect("open the window");
            let desk = window
                .update(cx, |desk, window, cx| {
                    window.focus(&desk.input.focus_handle(cx));
                    cx.entity()
                })
                .expect("the desk");
            let d = desk.clone();
            cx.on_action(move |_: &NewSession, cx| {
                let _ = window.update(cx, desk::Desk::new_default);
            });
            cx.on_action(move |_: &ToggleSidebar, cx| d.update(cx, desk::Desk::toggle_sidebar));
            cx.on_action(move |_: &Stop, cx| desk.update(cx, desk::Desk::stop));
            cx.activate(true);
        });
}
