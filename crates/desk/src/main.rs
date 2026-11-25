//! Ghost: one desktop app for the coding agents you already use.
//!
//! Down the left, your projects, each with its sessions, and the agents
//! switched on (Claude Code, Codex, Antigravity, or any other the ACP
//! registry lists). In the middle, the session on screen: talk to the agent and give it work, drawn
//! the same way whichever agent it is. Down the right, what the session has
//! come to. Ghost has no agent of its own: each agent runs as its own
//! program, with its own tools and its own sign-in, spoken to over the
//! Agent Client Protocol.

mod aside;
mod assets;
mod desk;
mod fonts;
mod hub;
mod keys;
mod macos;
mod markdown;
mod pages;
mod sessions;
mod sidebar;
mod style;
mod terminal;
mod theme;
mod thread;
mod ui;
mod voice;

// What needs no window is ghost-core's. Named here, `crate::git` and the
// rest name it alike in every module of the desk.
use ghost_core::{
    acp, git, github, path, proto, registry, remote, repo, scripts, server, session, settings,
    store, tidy, when, whole, writer,
};
use keys::{
    About, Hide, HideOthers, Minimize, NewSession, OpenSettings, Quit, ShowAll, Stop, ToggleSidebar,
};

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, Menu, MenuItem, OsAction, TitlebarOptions,
    WindowBounds, WindowOptions, point, px, size,
};

fn main() {
    // Before any thread starts, since it changes the environment.
    path::widen();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_desk=info,ghost_core=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // The links and folders macOS opens Ghost with (`ghost://`, `ghost
    // <folder>`), held until the window is up to take them.
    let (to_desk, mut for_desk) = tokio::sync::mpsc::unbounded_channel::<Vec<String>>();
    let app = gpui::application().with_assets(assets::Assets);
    app.on_open_urls(move |urls| {
        let _ = to_desk.send(urls);
    });
    app.run(|cx: &mut App| {
        gpui::init(cx);
        fonts::load(cx);
        macos::set();
        keys::bind(cx, &keys::yours(&settings::home()));
        // Without a main menu macOS has no menu bar to show in full
        // screen, so there is no way out of it. AppKit adds Enter and
        // Exit Full Screen to the View menu, and the window list to
        // the Window menu, by itself.
        cx.set_menus(vec![
            Menu::new("Ghost").items(vec![
                MenuItem::action("About Ghost", About),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Hide Ghost", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Ghost", Quit),
            ]),
            Menu::new("File").items(vec![
                MenuItem::action("New Session", NewSession),
                MenuItem::action("Stop", Stop),
            ]),
            Menu::new("Edit").items(vec![
                MenuItem::os_action("Cut", gpui::component::input::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", gpui::component::input::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", gpui::component::input::Paste, OsAction::Paste),
                MenuItem::os_action(
                    "Select All",
                    gpui::component::input::SelectAll,
                    OsAction::SelectAll,
                ),
                MenuItem::separator(),
                MenuItem::submenu(Menu::new("Speech").items(vec![
                    MenuItem::action("Start Speaking", keys::StartSpeaking),
                    MenuItem::action("Stop Speaking", keys::StopSpeaking),
                ])),
            ]),
            Menu::new("View").items(vec![
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action("Actual Size", keys::ActualSize),
                MenuItem::action("Zoom In", keys::ZoomIn),
                MenuItem::action("Zoom Out", keys::ZoomOut),
                MenuItem::separator(),
                MenuItem::action("Enter Full Screen", keys::FullScreen),
            ]),
            Menu::new("Window").items(vec![MenuItem::action("Minimize", Minimize)]),
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
        let (window, desk) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| desk::Desk::new(window, cx))
        })
        .expect("open the window");
        let shown = keys::Shown { window, desk };
        keys::handle(cx, &shown);
        let taker = shown.clone();
        cx.spawn(async move |cx| {
            while let Some(urls) = for_desk.recv().await {
                cx.update(|cx| taker.update(cx, |d, window, cx| d.open_links(&urls, window, cx)));
            }
        })
        .detach();
        shown
            .window
            .update(cx, |_, window, cx| {
                let input = shown.desk.read(cx).composer.input.focus_handle(cx);
                window.focus(&input, cx);
            })
            .expect("the desk");
        cx.activate(true);
    });
}
