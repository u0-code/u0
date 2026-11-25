//! What Ghost tells you outside the thread, at the foot of the window: a
//! thing you asked for that did not happen, and why, with the way on; a
//! thing done that can be followed (a pull request) or taken back (Undo).
//! One frame for all of them. The thread keeps only what happened in its
//! session.

use gpui::{ClipboardItem, Context, SharedString, div, prelude::*, px};

use super::Desk;
use crate::{
    fonts::Faces,
    theme,
    ui::{self, Weight},
};

/// How many notices stand at once; a new one lets the oldest go.
const MOST: usize = 3;

/// A notice: what happened, why, and the way on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u64,
    /// Something you asked for did not happen.
    pub trouble: bool,
    pub text: String,
    pub why: Option<String>,
    pub way: Option<Way>,
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
}

impl Desk {
    /// Say that something you asked for did not happen, and `why`.
    pub(crate) fn trouble(
        &mut self,
        text: impl Into<String>,
        why: Option<String>,
        way: Option<Way>,
    ) {
        self.tell_notice(true, text.into(), why, way);
    }

    /// Say that something was done, with the way on from it.
    pub(crate) fn done(&mut self, text: impl Into<String>, way: Option<Way>) {
        self.tell_notice(false, text.into(), None, way);
    }

    fn tell_notice(&mut self, trouble: bool, text: String, why: Option<String>, way: Option<Way>) {
        // The same said again stands once, lowest.
        self.notices.retain(|n| n.text != text);
        let id = self.sessions.take_key();
        self.notices.push(Notice {
            id,
            trouble,
            text,
            why: why.map(|w| w.trim().to_owned()).filter(|w| !w.is_empty()),
            way,
        });
        if self.notices.len() > MOST {
            self.notices.remove(0);
        }
    }

    /// The notices, and Undo while it stands, at the foot of the window.
    pub(crate) fn notices_view(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let undo = self.undo_notice(faces, cx);
        if self.notices.is_empty() && undo.is_none() {
            return None;
        }
        let mut stack = div()
            .absolute()
            .bottom(px(18.))
            .right(px(18.))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.));
        for n in &self.notices {
            stack = stack.child(Self::notice(faces, n, cx));
        }
        Some(stack.children(undo).into_any_element())
    }

    fn notice(faces: &Faces, n: &Notice, cx: &mut Context<Self>) -> gpui::Div {
        let id = n.id;
        let mut words = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(2.))
            .child(
                ui::text(faces, n.text.clone())
                    .text_size(px(13.))
                    .line_height(px(19.)),
            );
        if let Some(why) = &n.why {
            words = words.child(
                ui::text(faces, ui::fit(why, 160))
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(theme::muted()),
            );
        }
        let mut ways = div().flex().gap(px(6.)).mt(px(8.));
        let mut any = false;
        match n.way.clone() {
            Some(Way::Open(url)) => {
                any = true;
                ways = ways.child(
                    ui::button(
                        SharedString::from(format!("way-{id}")),
                        "Open",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                );
            }
            Some(Way::Agent(agent)) => {
                any = true;
                ways = ways.child(
                    ui::button(
                        SharedString::from(format!("way-{id}")),
                        "Open the agent",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_page(crate::desk::Page::Agent(agent.clone()), cx);
                    })),
                );
            }
            Some(Way::Show(key)) => {
                any = true;
                let id = n.id;
                ways = ways.child(
                    ui::button(
                        SharedString::from(format!("way-{id}")),
                        "Show",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.notices.retain(|n| n.id != id);
                        this.show(key, window, cx);
                    })),
                );
            }
            None => {}
        }
        if n.trouble {
            any = true;
            let details = match &n.why {
                Some(why) => format!("{}\n{why}", n.text),
                None => n.text.clone(),
            };
            ways = ways.child(
                ui::button(
                    SharedString::from(format!("copy-{id}")),
                    "Copy details",
                    Weight::Quiet,
                )
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(details.clone()));
                }),
            );
        }
        if any {
            words = words.child(ways);
        }
        frame()
            .items_start()
            .child(
                ui::icon(if n.trouble { "alert" } else { "check" }, 16.)
                    .mt(px(2.))
                    .text_color(if n.trouble {
                        theme::coral()
                    } else {
                        theme::muted()
                    }),
            )
            .child(words)
            .child(
                ui::icon_button(SharedString::from(format!("close-{id}")), "x", "Close")
                    .size(px(24.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notices.retain(|n| n.id != id);
                        cx.notify();
                    })),
            )
    }
}

/// The frame of a notice: 380 wide on the panel, raised.
pub(crate) fn frame() -> gpui::Div {
    div()
        .flex()
        .gap(px(12.))
        .w(px(380.))
        .pl(px(14.))
        .pr(px(8.))
        .py(px(10.))
        .rounded(px(12.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border2())
        .shadow_lg()
}
