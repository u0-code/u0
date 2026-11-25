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
mod input;
mod keys;
mod macos;
mod markdown;
mod pages;
mod path;
mod registry;
mod remote;
mod session;
mod sessions;
mod settings;
mod sidebar;
mod store;
mod style;
mod syntax;
mod terminal;
mod theme;
mod thread;
mod tidy;
mod ui;
mod when;
mod whole;

use keys::{
    About, Hide, HideOthers, Minimize, NewSession, OpenSettings, Quit, ShowAll, Stop, ToggleSidebar,
};

use gpui::{
    App, AppContext as _, Application, Bounds, Focusable as _, Menu, MenuItem, OsAction,
    TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

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
            macos::set();
            keys::bind(cx, &keys::yours(&settings::home()));
            keys::handle(cx);
            // Without a main menu macOS has no menu bar to show in full
            // screen, so there is no way out of it. AppKit adds Enter and
            // Exit Full Screen to the View menu, and the window list to
            // the Window menu, by itself.
            cx.set_menus(vec![
                Menu {
                    name: "Ghost".into(),
                    items: vec![
                        MenuItem::action("About Ghost", About),
                        MenuItem::separator(),
                        MenuItem::action("Settings…", OpenSettings),
                        MenuItem::separator(),
                        MenuItem::action("Hide Ghost", Hide),
                        MenuItem::action("Hide Others", HideOthers),
                        MenuItem::action("Show All", ShowAll),
                        MenuItem::separator(),
                        MenuItem::action("Quit Ghost", Quit),
                    ],
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
            window
                .update(cx, |desk, window, cx| {
                    window.focus(&desk.composer.input.focus_handle(cx));
                })
                .expect("the desk");
            cx.activate(true);
        });
}
