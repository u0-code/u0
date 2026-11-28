//! u0: one desktop app for the coding agents you already use.
//!
//! Down the left, your projects, each with its sessions, and the agents
//! switched on (Claude Code, Codex, Antigravity, or any other the ACP
//! registry lists). In the middle, the session on screen: talk to the agent and give it work, drawn
//! the same way whichever agent it is. Down the right, what the session has
//! come to. u0 has no agent of its own: each agent runs as its own
//! program, with its own tools and its own sign-in, spoken to over the
//! Agent Client Protocol.

// The desk's words in your language (`i18n`), from `locales/`.
rust_i18n::i18n!("locales", fallback = "en");

mod aside;
mod assets;
mod desk;
mod figures;
mod fonts;
mod hub;
mod i18n;
mod keys;
mod macos;
mod markdown;
mod pages;
mod sessions;
mod sidebar;
mod style;
mod terminal;
mod theme;
mod themes;
mod thread;
mod ui;
mod voice;

// What needs no window is u0-core's. Named here, `crate::git` and the
// rest name it alike in every module of the desk.
use i18n::{tr, tr_with};
use keys::{
    About, BringAllToFront, CheckForUpdates, Hide, HideOthers, Minimize, NewSession, OpenSettings,
    Quit, ShowAll, Stop, ToggleSidebar, ZoomWindow,
};
use u0_core::{
    access, acp, devices, diagnostics, forge, git, github, http, key, link, path, pictures, policy,
    proto, reach, registry, repo, review, scripts, secrets, server, servers, session, settings,
    share, store, tasks, telemetry, tidy, update, usage, when, whole, workflows, writer,
};

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, Menu, MenuItem, OsAction, SystemMenuType,
    TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

/// When u0 was launched: how long it took to open is counted from it.
static LAUNCHED: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

fn main() {
    std::sync::LazyLock::force(&LAUNCHED);
    // The command's words (`u0 serve`, `u0 service`, ...) and
    // u0's tools as an agent starts them (`--mcp`): no window.
    u0_core::cli::when_asked();
    // Before any thread starts, since it changes the environment.
    path::widen();
    let kept = settings::Settings::load(&settings::home()).0;
    u0_core::logs::start(kept.log_days);

    // The links and folders macOS opens u0 with (`u0://`, `u0
    // <folder>`), held until the window is up to take them.
    let (to_desk, mut for_desk) = tokio::sync::mpsc::unbounded_channel::<Vec<String>>();
    let app = gpui::application().with_assets(u0ui::Assets::with(assets::Assets));
    app.on_open_urls(move |urls| {
        let _ = to_desk.send(urls);
    });
    app.run(move |cx: &mut App| {
        cx.set_global(u0ui::theme::ActiveTheme(u0ui::CarbonTheme::G100));
        gpui::init(cx);
        u0ui::init(cx);
        fonts::load(cx);
        macos::set();
        keys::bind(cx, &keys::yours(&settings::home()).0);
        // Without a main menu macOS has no menu bar to show in full
        // screen, so there is no way out of it. AppKit adds Enter and
        // Exit Full Screen to the View menu, and the window list to
        // the Window menu, by itself.
        cx.set_menus(vec![
            Menu::new(crate::tr("u0")).items(vec![
                MenuItem::action(crate::tr("About u0"), About),
                MenuItem::action(crate::tr("Check for Updates…"), CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Settings…"), OpenSettings),
                MenuItem::action(crate::tr("Pair a Device…"), keys::PairDevice),
                MenuItem::action(crate::tr("Show Pairing Link…"), keys::ShowPairingCode),
                MenuItem::separator(),
                MenuItem::os_submenu(crate::tr("Services"), SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Hide u0"), Hide),
                MenuItem::action(crate::tr("Hide Others"), HideOthers),
                MenuItem::action(crate::tr("Show All"), ShowAll),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Quit u0"), Quit),
            ]),
            Menu::new(crate::tr("File")).items(vec![
                MenuItem::action(crate::tr("New Thread"), NewSession),
                MenuItem::action(crate::tr("Stop"), Stop),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Close Window"), keys::CloseSurface),
            ]),
            Menu::new(crate::tr("Edit")).items(vec![
                MenuItem::os_action(
                    crate::tr("Undo"),
                    gpui::component::input::Undo,
                    OsAction::Undo,
                ),
                MenuItem::os_action(
                    crate::tr("Redo"),
                    gpui::component::input::Redo,
                    OsAction::Redo,
                ),
                MenuItem::separator(),
                MenuItem::os_action(crate::tr("Cut"), gpui::component::input::Cut, OsAction::Cut),
                MenuItem::os_action(
                    crate::tr("Copy"),
                    gpui::component::input::Copy,
                    OsAction::Copy,
                ),
                MenuItem::os_action(
                    crate::tr("Paste"),
                    gpui::component::input::Paste,
                    OsAction::Paste,
                ),
                MenuItem::action(crate::tr("Delete"), gpui::component::input::Delete),
                MenuItem::os_action(
                    crate::tr("Select All"),
                    gpui::component::input::SelectAll,
                    OsAction::SelectAll,
                ),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Find…"), keys::Find),
                MenuItem::action(crate::tr("Find Next"), keys::FindNext),
                MenuItem::action(crate::tr("Find Previous"), keys::FindPrevious),
                MenuItem::action(
                    crate::tr("Use Selection for Find"),
                    keys::UseSelectionForFind,
                ),
                MenuItem::separator(),
                MenuItem::submenu(Menu::new(crate::tr("Speech")).items(vec![
                    MenuItem::action(crate::tr("Start Speaking"), keys::StartSpeaking),
                    MenuItem::action(crate::tr("Stop Speaking"), keys::StopSpeaking),
                ])),
            ]),
            Menu::new(crate::tr("View")).items(vec![
                MenuItem::action(crate::tr("Toggle Sidebar"), ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Actual Size"), keys::ActualSize),
                MenuItem::action(crate::tr("Zoom In"), keys::ZoomIn),
                MenuItem::action(crate::tr("Zoom Out"), keys::ZoomOut),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Enter Full Screen"), keys::FullScreen),
            ]),
            Menu::new(crate::tr("Window")).items(vec![
                MenuItem::action(crate::tr("Minimize"), Minimize),
                MenuItem::action(crate::tr("Zoom"), ZoomWindow),
                MenuItem::separator(),
                MenuItem::action(crate::tr("Bring All to Front"), BringAllToFront),
            ]),
            Menu::new(crate::tr("Help")).items(vec![MenuItem::action(
                crate::tr("Check for Updates…"),
                CheckForUpdates,
            )]),
        ]);

        // Where the window was as u0 last quit, while that is on a
        // display; else in the middle of the main one.
        let bounds = (kept.frame)
            .map(|[x, y, w, h]| {
                Bounds::new(
                    point(px(x as f32), px(y as f32)),
                    size(px(w as f32), px(h as f32)),
                )
            })
            .filter(|b| {
                cx.displays()
                    .iter()
                    .any(|d| d.bounds().contains(&b.center()))
            })
            .unwrap_or_else(|| Bounds::centered(None, size(px(1320.), px(860.)), cx));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("u0".into()),
                appears_transparent: true,
                // Traffic lights centred vertically in the 38px unified title
                // bar (`traffic-light-start` 14px, `traffic-light-offset-y` 19px).
                traffic_light_position: Some(point(
                    px(style::TRAFFIC_LIGHT_START),
                    px(style::TRAFFIC_LIGHT_OFFSET_Y - 6.),
                )),
            }),
            window_min_size: Some(size(
                px(style::WINDOW_MIN_WIDTH),
                px(style::WINDOW_MIN_HEIGHT),
            )),
            ..Default::default()
        };
        let (window, desk) = gpui::open_window(options, cx, |window, cx| {
            // Closed, the window hides: u0 and its agents go on, and the
            // Dock's icon brings it back.
            window.on_window_should_close(cx, |_, cx| {
                cx.hide();
                false
            });
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
                // Asked once the composer has the keys, so the sheet takes
                // them from it and Escape reaches the sheet.
                shown.desk.update(cx, |d, cx| {
                    // The Welcome page says how this Mac is reached, and
                    // asks of telemetry as it is done (finish_welcome):
                    // answered first, it would end the Welcome page unseen.
                    if d.settings.needs_welcome() {
                        let look = proto::Call::Share(share::Share::Look);
                        d.call(hub::Host::HERE, look);
                    } else {
                        d.ask_telemetry(window, cx);
                    }
                });
            })
            .expect("the desk");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    /// A test of the desk has no home of yours, so it never reads or
    /// writes there, and keeps its secrets in memory: macOS never asks you
    /// to let a test binary into u0's Keychain. Both hold only where the
    /// desk's tests build `u0-core` with its feature `test`.
    #[test]
    fn a_desk_test_reaches_neither_your_home_nor_your_keychain() {
        let yours = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".u0"));
        if let Ok(home) = std::panic::catch_unwind(crate::settings::home) {
            assert_ne!(Some(home), yours, "a test may not have your home");
        }
        crate::secrets::set("env/probe/KEY", Some("kept")).unwrap();
        assert_eq!(
            crate::secrets::get("env/probe/KEY").unwrap().as_deref(),
            Some("kept")
        );
        crate::secrets::set("env/probe/KEY", None).unwrap();
    }
}
