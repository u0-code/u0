//! The line you type into, and saying what is typed in it: to the little
//! ghost in the session on screen, or to the character open in your place
//! with her. Everything goes as `console.say`, as you, an operator of hers.
//! A line is a line: nobody picks what kind it is. She tells a question
//! from a thing to do from something to take on as work, the way a coding
//! agent does.
//!
//! It is one rounded box, the width of the thread, the way a coding
//! agent's app has it: what you type on top, which wraps and grows to six
//! lines before it scrolls; under it a quiet row with, for Ghost, the
//! brain and model it thinks with, and at the right send, which becomes
//! Stop while she is busy with what you said there and nothing new is
//! typed. Stop ends her look there; work she took on runs on, and is
//! stopped from its own card.
//!
//! Under the box, a line says what the desk just did or could not: short,
//! with the whole of a long error under Details.

use ghost_link::protocol::SayMode;
use gpui::{
    AnyElement, ClipboardItem, Context, Corner, Div, Focusable as _, SharedString, Window, div,
    prelude::*, px,
};

use super::{At, Desk, Menu};
use crate::{brains, fonts::Faces, input::TextInput, link::Order, theme, thread, ui};

/// A line at the foot of the composer: what the desk just did, or why it
/// could not. It is short enough to read at a glance; a long message keeps
/// the whole of what was said under Details.
pub(crate) struct Notice {
    pub line: SharedString,
    pub detail: Option<SharedString>,
    /// Something went wrong, rather than something done.
    pub trouble: bool,
    /// Whether the detail is shown.
    pub open: bool,
}

/// How long a notice's line may be before it is cut to its gist, in
/// characters: about one line of the composer's width.
const LONG: usize = 120;

impl Notice {
    /// Something the desk did.
    pub fn said(text: impl Into<String>) -> Self {
        Self::new(text.into(), false)
    }

    /// Something that went wrong.
    pub fn trouble(text: impl Into<String>) -> Self {
        Self::new(text.into(), true)
    }

    /// A long message, or one of many lines, is cut to its gist; the
    /// whole of it is kept as its detail.
    fn new(text: String, trouble: bool) -> Self {
        let long = text.trim().lines().count() > 1 || text.chars().count() > LONG;
        let (line, detail) = if long {
            (brains::gist(&text), Some(text.into()))
        } else {
            (text, None)
        };
        Self {
            line: line.into(),
            detail,
            trouble,
            open: false,
        }
    }

    /// Keep `detail`, the whole of what was said, under Details, unless
    /// the line already says all of it.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !self.line.contains(detail.trim()) {
            self.detail = Some(detail.into());
        }
        self
    }
}

impl Desk {
    /// Say what was typed. At a console every line of yours is an
    /// operator's: her daemon treats it so without being told.
    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        self.speak(text, SayMode::Say, cx);
    }

    /// Say your last line again, after she could not think about it.
    pub(crate) fn say_again(&mut self, text: String, cx: &mut Context<Self>) {
        self.speak(text, SayMode::Say, cx);
    }

    /// Say `text` to whoever is on screen, as `mode` has it.
    pub(crate) fn speak(&mut self, text: String, mode: SayMode, cx: &mut Context<Self>) {
        // To a character you speak in your own place with her on the
        // console channel, and her thread turns to it.
        if let Some(open) = &mut self.open {
            let place = self.settings.who.to_lowercase();
            open.place = Some(thread::place_key(&place));
            open.her.say(place, text, mode);
            cx.notify();
            return;
        }
        let Some(session) = self.settings.sessions.get_mut(self.current) else {
            return;
        };
        let place = session.id.clone();
        if session.title.is_empty() {
            session.title = text.chars().take(48).collect();
            self.settings.save();
        }
        self.solver.say(place, text, mode);
        cx.notify();
    }

    /// Stop what she is doing about the place on screen: her look there,
    /// and the work taken on there.
    fn stop_here(&mut self, cx: &mut Context<Self>) {
        match &self.open {
            Some(open) => {
                let place = open.place(self.who());
                let id = thread::thread_key(&place)
                    .split_once(':')
                    .map(|(_, id)| id.to_owned())
                    .unwrap_or_default();
                open.her.order(Order::StopPlace { place: id });
            }
            None => self.solver.order(Order::StopPlace {
                place: self.session().id,
            }),
        }
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    /// Whom the composer speaks to: the character open, or Ghost.
    pub(crate) fn speaking_to(&self) -> String {
        self.open
            .as_ref()
            .map_or_else(|| self.solver.name(), |o| o.her.name())
    }

    /// What the line says while it is empty: whom it speaks to.
    pub(super) fn placeholder(&self) -> String {
        let name = self.speaking_to();
        if self.open.is_some() {
            format!("Say something to {name}")
        } else {
            format!("Ask {name} for anything on this machine")
        }
    }

    /// Give the keyboard to the line.
    pub(crate) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.focus_handle(cx).focus(window);
    }

    /// Whether the keyboard is in the line.
    fn composer_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    /// The brain and model Ghost thinks with, by its mark, as quiet text
    /// that opens the brain menu.
    fn brain_chip(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let open = self.menu == Menu::Brains && self.menu_at == At::Composer;
        let provider = self.current_provider();
        let label = provider.map_or_else(|| "No brain".to_owned(), |p| self.brain_label(p));
        let mut chip = div().relative().flex().min_w(px(0.)).child(
            div()
                .id("brain")
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w(px(0.))
                .h(px(26.))
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(open, |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .children(provider.map(|p| ui::mark(faces, p.mark(), 13.)))
                .child(ui::one_line(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(theme::muted()),
                ))
                .child(
                    ui::icon("chevron-down", 11.)
                        .flex_none()
                        .text_color(theme::faint()),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_menu(Menu::Brains, At::Composer, window, cx);
                })),
        );
        if self.menu == Menu::Brains
            && let Some(menu) = self.menu_over(At::Composer, Corner::BottomLeft, faces, cx)
        {
            chip = chip.child(div().absolute().top(px(-6.)).left(px(0.)).child(menu));
        }
        chip
    }

    /// Send, a small arrow in the grey of the desk's primary button, lit
    /// once something is typed; or, while she is busy with what you said
    /// here and nothing new is typed, Stop, a square.
    fn send_button(faces: &Faces, empty: bool, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let button = div()
            .size(px(28.))
            .flex_none()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .border_1();
        if busy && empty {
            return button
                .id("stop")
                .cursor_pointer()
                .bg(theme::border2())
                .border_color(theme::ghost_line())
                .hover(|s| s.bg(theme::ghost_line()))
                .child(div().size(px(8.)).rounded(px(2.)).bg(theme::text()))
                .tooltip(ui::tip("Stop"))
                .on_click(cx.listener(|this, _, _, cx| this.stop_here(cx)))
                .into_any_element();
        }
        button
            .id("send")
            .when(empty, |d| d.border_color(theme::border2()))
            .when(!empty, |d| {
                d.cursor_pointer()
                    .bg(theme::border2())
                    .border_color(theme::ghost_line())
                    .hover(|s| s.bg(theme::ghost_line()))
            })
            .child(
                ui::mono(faces, "↑")
                    .text_size(px(14.))
                    .text_color(if empty { theme::faint() } else { theme::text() }),
            )
            .tooltip(ui::tip("Send · Return"))
            .on_click(cx.listener(|this, _, _, cx| this.send(cx)))
            .into_any_element()
    }

    /// The composer: what you type, over one quiet row.
    pub(super) fn composer(
        &self,
        faces: &Faces,
        busy: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = self.composer_focused(window, cx);
        let border = if focused {
            theme::ghost_line()
        } else {
            theme::border2()
        };
        // Pulled left by the chip's own padding, so that its words stand
        // in line with what is typed above them.
        let left = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap(px(2.))
            .ml(px(-4.))
            .when(self.open.is_none(), |d| d.child(self.brain_chip(faces, cx)));
        let empty = self.input.read(cx).is_empty();
        let right = Self::send_button(faces, empty, busy, cx);
        let top = div().px(px(4.)).child(self.input.clone());
        let field = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .w_full()
            .max_w(px(thread::COLUMN))
            .px(px(8.))
            .pt(px(6.))
            .pb(px(8.))
            .bg(theme::panel())
            .border_1()
            .border_color(border)
            .rounded(px(14.))
            .child(top)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(left)
                    .child(right),
            );
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(32.))
            .pb(px(14.))
            .child(field)
            .children(self.notice_line(faces, cx))
    }

    /// The line saying what the desk just did or could not: a dot when it
    /// is trouble, Details for the whole of a long message, and a cross
    /// that puts it away.
    pub(crate) fn notice_line(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.notice.as_ref()?;
        let quiet = |id: &'static str, word: &'static str| {
            div()
                .id(id)
                .flex_none()
                .px(px(4.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    ui::text(faces, word)
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(theme::faint()),
                )
        };
        let line = div()
            .flex()
            .items_start()
            .gap(px(8.))
            .when(n.trouble, |d| d.child(ui::dot(theme::coral()).mt(px(6.))))
            .child(
                ui::text(faces, n.line.clone())
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(theme::text2()),
            )
            .when(n.detail.is_some(), |d| {
                d.child(
                    quiet("notice-details", if n.open { "Hide" } else { "Details" }).on_click(
                        cx.listener(|this, _, _, cx| {
                            if let Some(n) = &mut this.notice {
                                n.open = !n.open;
                            }
                            cx.notify();
                        }),
                    ),
                )
            })
            .child(
                div()
                    .id("notice-close")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(18.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::icon("x", 11.).text_color(theme::faint()))
                    .tooltip(ui::tip("Dismiss"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    })),
            );
        let detail = n.detail.clone().filter(|_| n.open).map(|detail| {
            let copied = detail.clone();
            div()
                .relative()
                .child(
                    div()
                        .id("notice-detail")
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .p(px(10.))
                        .pr(px(56.))
                        .bg(theme::well())
                        .border_1()
                        .border_color(theme::border())
                        .rounded(px(8.))
                        .child(
                            ui::mono(faces, detail)
                                .text_size(px(11.5))
                                .line_height(px(17.))
                                .text_color(theme::text2()),
                        ),
                )
                .child(
                    quiet("notice-copy", "Copy")
                        .absolute()
                        .top(px(6.))
                        .right(px(8.))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()));
                        })),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .w_full()
                .max_w(px(thread::COLUMN))
                .px(px(10.))
                .child(line)
                .children(detail)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Notice;

    #[test]
    fn a_long_error_is_cut_to_its_gist_and_kept_whole_under_details() {
        let raw = format!(
            "API Error: 403 {}· Please run /login",
            r#"{"error":{"type":"forbidden","message":"Request not allowed"}} "#.repeat(2)
        );
        let n = Notice::trouble(raw.clone());
        assert_eq!(n.line.as_ref(), "the request was refused (403)");
        assert_eq!(n.detail.as_ref().map(AsRef::as_ref), Some(raw.as_str()));
        assert!(n.trouble && !n.open);
    }

    #[test]
    fn a_short_line_has_no_details_and_none_that_only_repeat_it() {
        let n = Notice::said("Restarting Ghost with the new brain.");
        assert_eq!(n.detail, None);
        let n = Notice::trouble("Codex can't think here: model not found. Still thinking with x.")
            .with_detail("model not found");
        assert_eq!(n.detail, None, "the line already says all of it");
        let n = Notice::trouble("Claude Code is not logged in")
            .with_detail("API Error: 403 · Please run /login");
        assert!(n.detail.is_some());
    }
}
