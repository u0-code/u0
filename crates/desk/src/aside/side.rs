//! A side chat in the column: a branch of the session whole, asked beside
//! it so that nothing said there goes in it. What was asked and answered,
//! a line to ask more, and the last answer taken back into the session.

use gpui::component::{Sizable as _, input::Input, scroll::ScrollableElement as _};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::fine;
use crate::{
    desk::Desk,
    fonts::Faces,
    markdown,
    session::{Item, State},
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// The side chat `key`: its exchanges after what it was branched with,
    /// and the line to ask in it.
    pub(super) fn side_chat(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.sessions.at(key) else {
            return div()
                .p(px(16.))
                .child(fine(faces, crate::tr("This side chat is gone.")));
        };
        let up = s.beside;
        // What it was branched with is the session's own: shown there.
        let from = up
            .and_then(|u| self.sessions.at(u))
            .map_or(0, |u| u.items.len().min(s.items.len()));
        let mut said = div().flex().flex_col().gap(px(12.)).px(px(16.)).py(px(12.));
        for (n, item) in s.items.iter().enumerate().skip(from) {
            said = match item {
                Item::You(text) => said.child(
                    div()
                        .self_end()
                        .max_w(px(260.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(10.))
                        .bg(theme::panel2())
                        .child(ui::text(faces, text.clone()).text_size(px(crate::style::LABEL))),
                ),
                Item::Agent(text) => said.child(div().text_size(px(crate::style::LABEL)).child(
                    markdown::render(faces, &format!("side-{key}-{n}"), text, cx),
                )),
                _ => said,
            };
        }
        if s.items.len() <= from {
            said = said.child(fine(
                faces,
                crate::tr("Ask what you would rather not ask in the thread: the agent knows all of it, and nothing said here goes in it."),
            ));
        }
        let working = matches!(s.state, State::Working | State::Opening);
        let answered = s.items[from..].iter().any(|i| matches!(i, Item::Agent(_)));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(
                div()
                    .id(SharedString::from(format!("side-{key}")))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(said)
                    .overflow_y_scrollbar(),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(8.))
                    .p(px(10.))
                    .border_t_1()
                    .border_color(theme::border())
                    .child(Input::new(&self.side_line).small().disabled(working))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(working, |d| d.child(fine(faces, crate::tr("Answering…"))))
                            .child(div().flex_1())
                            .when(answered, |d| {
                                d.child(
                                    ui::button(
                                        "side-back",
                                        crate::tr("Add to chat"),
                                        Weight::Secondary,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.answer_back(key, window, cx);
                                        },
                                    )),
                                )
                            })
                            .child(
                                ui::button(
                                    "side-main",
                                    crate::tr("Open in main view"),
                                    Weight::Quiet,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.show(key, window, cx);
                                    },
                                )),
                            ),
                    ),
            )
    }

    /// Ask what is on the side chat's line in the side chat shown.
    pub(crate) fn ask_side(
        &mut self,
        text: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(crate::desk::Surface::Side(key)) = self.surface() else {
            return;
        };
        self.send_in(key, text, cx);
        self.side_line
            .update(cx, |line, cx| line.set_value("", window, cx));
    }
}
