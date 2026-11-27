//! What u0 tells you outside the thread, as gpui-kit's notifications at
//! the window's top right: a thing you asked for that did not happen, and
//! why; a thing done, with the way on from it (a pull request, a session);
//! what can still be taken back. While u0 is behind other windows, what
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
    kbd::Kbd,
    notification::{Notification, NotificationDelivery, NotificationType},
};
use gpui::{ClipboardItem, Context, ParentElement as _, Window, prelude::FluentBuilder as _};

use super::Desk;
use crate::settings::Notify;

/// Visual kind of an in-app notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Error,
}

/// A notice: what happened, why, and the way on; told by macOS when
/// `system`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: Kind,
    pub text: String,
    pub why: Option<String>,
    pub way: Option<Way>,
    pub system: bool,
}

/// The notices of sessions that ask, one each, by the session.
struct Asking;

/// The one way on from a notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// An address to open: a pull request.
    Open(String),
    /// An agent's page, on a server: to sign in to it, or see why it
    /// stopped.
    Agent(crate::hub::Host, String),
    /// A session, to put it on screen.
    Show(u64),
    /// A session that asks before it goes on, to put it on screen: the
    /// notice goes once nothing there asks any more.
    Asks(u64),
    /// What was just done to sessions, to take it back.
    Undo,
    /// An update of u0's own, to see and install.
    Update,
    /// A clone that failed, of the address into the folder, to try again.
    Clone(String, super::Place),
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
        self.notice(Kind::Error, text.into(), why, way, false);
    }

    /// Say an informational notice (`MacChrome`: ⌘Q hold/press-twice).
    pub(crate) fn info(&mut self, text: impl Into<String>, why: Option<String>) {
        let why = why.map(|w| w.trim().to_owned()).filter(|w| !w.is_empty());
        self.notice(Kind::Info, text.into(), why, None, false);
    }

    /// Say that something was done, with the way on from it.
    pub(crate) fn done(&mut self, text: impl Into<String>, way: Option<Way>) {
        self.notice(Kind::Success, text.into(), None, way, false);
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
            Kind::Success,
            title,
            Some(body.to_owned()),
            Some(Way::Show(key)),
            true,
        );
    }

    fn notice(
        &mut self,
        kind: Kind,
        text: String,
        why: Option<String>,
        way: Option<Way>,
        system: bool,
    ) {
        self.notices.push(Notice {
            kind,
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
            if let Some(Way::Asks(key)) = n.way
                && !n.system
            {
                self.asking.insert(key);
            }
            let note = Self::notification(&n, cx);
            window.push_notification(note, cx);
        }
        // What a session asked, answered or given up on: its notice goes.
        let over: Vec<u64> = (self.asking.iter().copied())
            .filter(|&key| !self.sessions.asks(key))
            .collect();
        for key in over {
            self.asking.remove(&key);
            window.remove_notification1::<Asking>(gpui::ElementId::Integer(key), cx);
        }
    }

    fn notification(n: &Notice, cx: &mut Context<Self>) -> Notification {
        let desk = cx.entity().downgrade();
        let trouble = n.kind == Kind::Error;
        let kind = match n.kind {
            Kind::Info => NotificationType::Info,
            Kind::Success => NotificationType::Success,
            Kind::Error => NotificationType::Error,
        };
        let mut note = Notification::new().title(n.text.clone()).with_type(kind);
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
        // One for each session that asks, taken back once it no longer does.
        if let Some(Way::Asks(key)) = n.way {
            note = note.id1::<Asking>(gpui::ElementId::Integer(key));
        }
        let label = match &n.way {
            Some(Way::Open(_)) => "Open",
            Some(Way::Agent(..)) => "Open the agent",
            Some(Way::Show(_) | Way::Asks(_)) => "Show",
            Some(Way::Undo) => "Undo",
            Some(Way::Update) => "See",
            Some(Way::Clone(..)) => "Retry",

            None if trouble => "Copy details",
            None => return note.autohide(true),
        };
        let undo_keys = if n.way == Some(Way::Undo) {
            crate::keys::keys_of(cx, "take_back")
        } else {
            Vec::new()
        };
        let details = match &n.why {
            Some(why) => format!("{}\n{why}", n.text),
            None => n.text.clone(),
        };
        let go = move |way: &Option<Way>, window: &mut Window, cx: &mut gpui::App| {
            let _ = desk.update(cx, |d, cx| match way.clone() {
                Some(Way::Open(url)) => d.browse(&url, cx),
                Some(Way::Agent(host, agent)) => {
                    d.open_page(super::Page::Agent(host, agent), window, cx);
                }
                Some(Way::Show(key) | Way::Asks(key)) => d.show(key, window, cx),
                Some(Way::Undo) => d.take_back(cx),
                Some(Way::Update) => d.open_tab(crate::pages::Tab::About, window, cx),
                Some(Way::Clone(url, to)) => d.clone_into(to, url, cx),
                None => cx.write_to_clipboard(ClipboardItem::new_string(details.clone())),
            });
        };
        let (way, clicked) = (n.way.clone(), go.clone());
        // A session's notification goes to it wherever it is clicked: the
        // only way on from one in macOS's centre.
        if matches!(n.way, Some(Way::Show(_) | Way::Asks(_))) {
            let way = n.way.clone();
            note = note.on_click(move |_, window, cx| clicked(&way, window, cx));
        }
        // A way on keeps a notice up, but what went wrong still goes by
        // itself: it waits while the pointer is on it, and its close button
        // shows there.
        note.action(move |_, _, cx| {
            let (way, go, keys) = (way.clone(), go.clone(), undo_keys.clone());
            Button::new("way")
                .label(crate::tr(label))
                .small()
                .ghost()
                .children(keys.into_iter().map(Kbd::new))
                .on_click(cx.listener(move |note, _, window, cx| {
                    go(&way, window, cx);
                    note.dismiss(window, cx);
                }))
        })
        .when(trouble, |note| note.autohide(true))
    }
}
