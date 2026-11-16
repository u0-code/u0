//! Ghost Desk: a console for `ghostd`.
//!
//! Two things on screen. Down the left, the characters: the one that works
//! on this machine first, then everyone the desk follows, on whatever
//! daemon. On the right, the thread with whoever is chosen: what passed
//! between you, what she did about it, the rest of her life as her journal
//! tells it, and the line you speak into. Choosing a character changes the
//! whole environment to hers.
//!
//! It is a client of the daemon like any presence: a console that asks and
//! watches, and a presence named `desk` for the conversation between you
//! and her. It never touches a ghost's files, and holds no key of hers.

mod assets;
mod desk;
mod fonts;
mod input;
mod link;
mod model;
mod settings;
mod theme;
mod thread;
mod ui;

use gpui::{
    App, AppContext as _, Application, Bounds, Focusable as _, KeyBinding, TitlebarOptions,
    WindowBounds, WindowOptions, actions, point, px, size,
};

actions!(desk, [Quit]);

fn main() {
    // Keys come from the environment, or the ignored .env every binary loads.
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_desk=info,ghost_link=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            fonts::load(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
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
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());

            let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Ghost".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(18.))),
                }),
                window_min_size: Some(size(px(800.), px(520.))),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |_, cx| cx.new(desk::Desk::new))
                .expect("open the window");
            window
                .update(cx, |desk, window, cx| {
                    window.focus(&desk.input.focus_handle(cx));
                })
                .expect("the desk");
            cx.activate(true);
        });
}
