//! What Ghost tells you outside the thread, as gpui-kit's notifications at
//! the window's top right: a thing you asked for that did not happen, and
//! why; a thing done, with the way on from it (a pull request, a session);
//! what can still be taken back. While Ghost is behind other windows, what
//! a session did goes to macOS's notification centre instead, and a click
//! there brings the window up on that session. The thread keeps only what
//! happened in its session.
//!
//! A notification is pushed through the window, which what happens off it
//! (an answer from the server) does not have: notices wait in the desk
//! and are pushed as it is drawn next.

use gpui::component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    notification::{Notification, NotificationDelivery, NotificationType},
};
use gpui::{ClipboardItem, Context, Window};

use super::Desk;
use crate::settings::Notify;

/// A notice: what happened, why, and the way on; told by macOS when
/// `system`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub trouble: bool,
    pub text: String,
    pub why: Option<String>,
    pub way: Option<Way>,
    pub system: bool,
}

/// The one way on from a notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// An address to open: a pull request.
    Open(String),
    /// An agent's page: to sign in to it, or see why it stopped.
    Agent(String),
    /// A session, to put it on screen.
    Show(u64),
    /// What was just done to sessions, to take it back.
    Undo,
    /// An update of Ghost's own, to see and install.
    Update,
    /// A clone that failed, of the address into the folder, to try again.
    Clone(String, std::path::PathBuf),
}

impl Desk {
    /// Say that something you asked for did not happen, and `why`.
    pub(crate) fn trouble(
        &mut self,
        text: impl Into<String>,
        why: Option<String>,
        way: Option<Way>,
    ) {
        let why = why.map(|w| w.trim().to_owned()).filter(|w| !w.is_empty());
        self.notice(true, text.into(), why, way, false);
    }

    /// Say that something was done, with the way on from it.
    pub(crate) fn done(&mut self, text: impl Into<String>, way: Option<Way>) {
        self.notice(false, text.into(), None, way, false);
    }

    /// Say through macOS what the session `key` did, as you asked it to:
    /// a click on it brings the session up.
    pub(crate) fn tell(&mut self, key: u64, title: String, body: &str) {
        if self.settings.notify == Notify::Off {
            return;
        }
        if self.settings.notify == Notify::BannerAndSound {
            crate::macos::chime();
        }
        self.notice(
            false,
            title,
            Some(body.to_owned()),
            Some(Way::Show(key)),
            true,
        );
    }

    fn notice(
        &mut self,
        trouble: bool,
        text: String,
        why: Option<String>,
        way: Option<Way>,
        system: bool,
    ) {
        self.notices.push(Notice {
            trouble,
            text,
            why,
            way,
            system,
        });
    }

    /// The notices waiting, pushed as notifications: as the window is
    /// drawn, where it is at hand.
    pub(crate) fn show_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for n in std::mem::take(&mut self.notices) {
            let note = Self::notification(&n, cx);
            window.push_notification(note, cx);
        }
    }

    fn notification(n: &Notice, cx: &mut Context<Self>) -> Notification {
        let desk = cx.entity().downgrade();
        let mut note = Notification::new()
            .title(n.text.clone())
            .with_type(if n.trouble {
                NotificationType::Error
            } else {
                NotificationType::Success
            });
        if let Some(why) = &n.why {
            note = note.message(crate::ui::fit(why, 160));
        }
        if n.system {
            note = note.delivery(NotificationDelivery::System);
        }
        // One undo notice, said again as more joins what it takes back.
        if n.way == Some(Way::Undo) {
            note = note.id::<super::lists::Undo>();
        }
        let label = match &n.way {
            Some(Way::Open(_)) => "Open",
            Some(Way::Agent(_)) => "Open the agent",
            Some(Way::Show(_)) => "Show",
            Some(Way::Undo) => "Undo",
            Some(Way::Update) => "See",
            Some(Way::Clone(..)) => "Retry",
            None if n.trouble => "Copy details",
            None => return note,
        };
        let details = match &n.why {
            Some(why) => format!("{}\n{why}", n.text),
            None => n.text.clone(),
        };
        let go = move |way: &Option<Way>, window: &mut Window, cx: &mut gpui::App| {
            let _ = desk.update(cx, |d, cx| match way.clone() {
                Some(Way::Open(url)) => cx.open_url(&url),
                Some(Way::Agent(agent)) => d.open_page(super::Page::Agent(agent), window, cx),
                Some(Way::Show(key)) => d.show(key, window, cx),
                Some(Way::Undo) => d.take_back(cx),
                Some(Way::Update) => d.open_tab(crate::pages::Tab::About, window, cx),
                Some(Way::Clone(url, dir)) => d.clone_into(url, dir, cx),
                None => cx.write_to_clipboard(ClipboardItem::new_string(details.clone())),
            });
        };
        let (way, clicked) = (n.way.clone(), go.clone());
        // A session's notification goes to it wherever it is clicked: the
        // only way on from one in macOS's centre.
        if matches!(n.way, Some(Way::Show(_))) {
            let way = n.way.clone();
            note = note.on_click(move |_, window, cx| clicked(&way, window, cx));
        }
        note.action(move |_, _, cx| {
            let (way, go) = (way.clone(), go.clone());
            Button::new("way")
                .label(label)
                .small()
                .ghost()
                .on_click(cx.listener(move |note, _, window, cx| {
                    go(&way, window, cx);
                    note.dismiss(window, cx);
                }))
        })
    }
}
