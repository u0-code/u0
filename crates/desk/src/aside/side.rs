//! A side chat in the column: a branch of the session whole, asked beside
//! it so that nothing said there goes in it. What was asked and answered,
//! a line to ask more (u0ui's `ComposerCompact`), and the last answer
//! taken back into the session.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory;
use gpui::component::scroll::ScrollableElement as _;
use gpui::{Context, Div, SharedString, div, prelude::*, px};
use u0ui::{
    components::{Button, ButtonSize, ButtonVariant},
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::composer::{ComposerCompact, ComposerCompactTexts},
};

use super::fine;
use crate::{
    desk::Desk,
    fonts::Faces,
    markdown,
    session::{Item, State},
    theme, ui, when,
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
        // Who answers: the agent, and its model where it has one chosen.
        let agent = self.name_of(&s.meta.agent);
        let model = Self::option_name(s, &SessionConfigOptionCategory::Model);
        let who = model
            .as_ref()
            .map_or_else(|| agent.clone(), |m| format!("{agent} · {m}"));
        let up = s.beside;
        // What it was branched with is the session's own: shown there.
        let from = up
            .and_then(|u| self.sessions.at(u))
            .map_or(0, |u| u.items.len().min(s.items.len()));
        // `SplitSideChat`: 16 in, 20 between each part.
        let mut said = div().flex().flex_col().gap(px(20.)).p(px(16.)).child(
            div()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(crate::tr_with(
                    "Side chat · %{name} · doesn't interrupt the thread",
                    &[("name", &who)],
                )),
        );
        for (n, item) in s.items.iter().enumerate().skip(from) {
            said = match item {
                Item::You(text) => said.child(
                    div().flex().justify_end().child(
                        crate::thread::you_said(faces, format!("side-{key}-{n}"), text.clone())
                            .max_w(px(300.)),
                    ),
                ),
                // `.t3-amsg { gap: 8px }`, its meta line `.t3-amsg-meta`: 24
                // high, 4 apart, 12px `text-helper`: the mark, who, when.
                Item::Agent(text) => said.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .min_h(px(24.))
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
                                .child(who.clone())
                                .children(s.time_of(n).map(|t| {
                                    format!("· {}", when::clock(t, self.settings.twelve))
                                })),
                        )
                        .child(markdown::render(
                            faces,
                            &format!("side-{key}-{n}"),
                            text,
                            cx,
                        )),
                ),
                _ => said,
            };
        }
        if s.items.len() <= from {
            said = said.child(fine(
                faces,
                crate::tr("Ask on the side: the agent knows the thread; this stays out of it."),
            ));
        }
        let working = matches!(s.state, State::Working | State::Opening);
        let answered = s.items[from..].iter().any(|i| matches!(i, Item::Agent(_)));
        let asks = cx.entity();
        let line = ComposerCompact::new(SharedString::from(format!("side-line-{key}")), agent)
            .lead(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
            .disabled(working)
            .texts(ComposerCompactTexts {
                placeholder: crate::tr_with(
                    "Ask %{name} on the side…",
                    &[(
                        "name",
                        &model.unwrap_or_else(|| self.name_of(&s.meta.agent)),
                    )],
                ),
                message: crate::tr("Side chat message"),
                attach: crate::tr("Attach"),
                send: crate::tr("Send"),
            })
            .on_send(move |text, _, cx| {
                asks.update(cx, |this, cx| this.ask_side(key, &text, cx));
            });
        let (back, main) = (cx.entity(), cx.entity());
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .bg(theme::role(ColorRole::Background))
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
                    .px(px(16.))
                    .pt(px(4.))
                    .pb(px(16.))
                    .when(working, |d| d.child(fine(faces, crate::tr("Answering…"))))
                    .child(line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(4.))
                            .when(answered, |d| {
                                d.child(
                                    Button::new("side-back", crate::tr("Add to chat"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(move |_, window, cx| {
                                            back.update(cx, |this, cx| {
                                                this.answer_back(key, window, cx);
                                            });
                                        }),
                                )
                            })
                            .child(
                                Button::new("side-main", crate::tr("Open in main view"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_, window, cx| {
                                        main.update(cx, |this, cx| this.show(key, window, cx));
                                    }),
                            ),
                    ),
            )
    }

    /// Ask `text` in the side chat `key`.
    pub(crate) fn ask_side(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        self.send_in(key, text, cx);
    }
}
