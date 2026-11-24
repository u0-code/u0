//! The line you type into, and the chips under it: the agent, its model
//! and mode, how full its context is, and Send or Stop.

use agent_client_protocol_schema::v1::SessionConfigOptionCategory;
use gpui::{Context, Div, SharedString, Stateful, Window, div, prelude::*, px};

use super::{Choices, select_choices};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    session::State,
    style::COLUMN,
    theme,
    ui::{self, Mark},
};

impl Desk {
    /// The line you type into, and under it the agent, its model and mode,
    /// how full its context is, and Send or Stop.
    pub(crate) fn composer(
        &self,
        faces: &Faces,
        column: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(s) = self.session() else {
            return div();
        };
        let name = self.name_of(&s.agent);
        self.input.update(cx, |i, cx| {
            i.set_placeholder(&format!("Reply to {name}, or tell it what to do next…"), cx);
        });
        let focused = gpui::Focusable::focus_handle(self.input.read(cx), cx).is_focused(window);
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.input.read(cx).is_empty();

        let draft = s.state == State::Draft && s.items.is_empty();
        let mut bar = div().flex().items_center().gap(px(4.)).ml(px(-2.));
        let agent_choices = if draft {
            self.slots
                .iter()
                .map(|x| {
                    (
                        x.entry.id.clone(),
                        self.name_of(&x.entry.id),
                        x.entry.id == s.agent,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        bar = bar.child(self.chip(
            faces,
            &Popup::Agent,
            Some(&Mark::of(&s.agent, &name)),
            &name,
            "Start this session with",
            agent_choices,
            cx,
        ));
        let mode_option = s
            .options
            .iter()
            .any(|o| o.category == Some(SessionConfigOptionCategory::Mode));
        let mut chips: Vec<(Popup, String, String, Choices)> = Vec::new();
        for option in s.options.iter().filter(|o| {
            matches!(
                o.category,
                Some(
                    SessionConfigOptionCategory::Model
                        | SessionConfigOptionCategory::ThoughtLevel
                        | SessionConfigOptionCategory::Mode
                )
            )
        }) {
            let Some((current, choices)) = select_choices(&option.kind) else {
                continue;
            };
            let head = format!("{name} {}", option.name.to_lowercase());
            chips.push((
                Popup::Option(option.id.0.to_string()),
                current,
                head,
                choices,
            ));
        }
        if let Some(modes) = s.modes.as_ref().filter(|_| !mode_option) {
            let current = modes
                .available_modes
                .iter()
                .find(|m| m.id == modes.current_mode_id)
                .map_or_else(|| modes.current_mode_id.0.to_string(), |m| m.name.clone());
            let choices = modes
                .available_modes
                .iter()
                .map(|m| {
                    (
                        m.id.0.to_string(),
                        m.name.clone(),
                        m.id == modes.current_mode_id,
                    )
                })
                .collect();
            chips.push((Popup::Mode, current, "Mode".into(), choices));
        }
        for (popup, current, head, choices) in chips {
            bar = bar.child(self.chip(faces, &popup, None, &current, &head, choices, cx));
        }
        bar = bar.child(div().flex_1());
        if let Some(u) = s.usage.as_ref().filter(|u| u.size > 0) {
            let share = u.used as f32 / u.size as f32;
            bar = bar.child(
                div()
                    .id("context-used")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .px(px(8.))
                    .child(ui::ring(share, 18.))
                    .child(
                        ui::text(faces, format!("{}%", (share * 100.).round()))
                            .text_size(px(11.5))
                            .text_color(theme::faint()),
                    )
                    .tooltip(ui::tip("Context used")),
            );
        }
        let button = div()
            .size(px(30.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(9.));
        let button = if working && empty {
            button
                .id("stop")
                .cursor_pointer()
                .bg(theme::panel2())
                .hover(|st| st.bg(theme::ghost_line()))
                .child(div().size(px(8.)).rounded(px(1.5)).bg(theme::text()))
                .tooltip(ui::tip("Stop the turn · ⌘."))
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else {
            let input = self.input.clone();
            let live = !empty && !working;
            button
                .id("send")
                .bg(if live { theme::text() } else { theme::panel2() })
                .when(live, |d| {
                    d.cursor_pointer().hover(|st| st.bg(theme::text2()))
                })
                .child(ui::icon("arrow-up", 14.).text_color(if live {
                    theme::bg()
                } else {
                    theme::faint()
                }))
                .tooltip(ui::tip("Send · Return"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    input.update(cx, crate::input::TextInput::send);
                }))
        };
        bar = bar.child(button);

        div().flex().flex_none().justify_center().pb(px(18.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .w(px(column.min(COLUMN)))
                .pl(px(14.))
                .pr(px(12.))
                .pt(px(12.))
                .pb(px(10.))
                .rounded(px(16.))
                .bg(theme::panel())
                .border_1()
                .border_color(if focused {
                    theme::ghost_line()
                } else {
                    theme::border2()
                })
                .child(div().min_h(px(42.)).child(self.input.clone()))
                .child(bar),
        )
    }

    /// A chip that opens a menu of choices, `(id, name, current)` each; with
    /// none, a label.
    #[allow(
        clippy::too_many_arguments,
        reason = "a chip is all of these, each once"
    )]
    fn chip(
        &self,
        faces: &Faces,
        popup: &Popup,
        mark: Option<&Mark>,
        current: &str,
        head: &str,
        choices: Choices,
        cx: &mut Context<Self>,
    ) -> Div {
        let menu_able = !choices.is_empty();
        let open = &self.popup == popup && menu_able;
        let id = match popup {
            Popup::Mode => "chip-mode".to_owned(),
            Popup::Option(o) => format!("chip-{o}"),
            Popup::Agent => "chip-agent".to_owned(),
            Popup::Closed => "chip".to_owned(),
        };
        let for_click = popup.clone();
        let mut chip: Stateful<Div> = div()
            .id(SharedString::from(id.clone()))
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(8.))
            .rounded(px(7.))
            .when(open, |d| d.bg(theme::panel2()))
            .children(mark.map(|m| ui::mark(faces, m, 14.)))
            .child(
                ui::one_line(
                    ui::text(faces, ui::fit(current, 24))
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .w_auto(),
            );
        if menu_able {
            chip = chip
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .child(
                    ui::icon("chevron-down", 12.)
                        .flex_none()
                        .text_color(theme::faint()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_popup(for_click.clone(), cx);
                }));
        }
        let mut wrap = div().relative().flex().min_w(px(0.)).child(chip);
        if open {
            let mut menu = div()
                .flex()
                .flex_col()
                .w(px(240.))
                .p(px(6.))
                .child(ui::label(faces, head).px(px(8.)).pt(px(6.)).pb(px(4.)));
            for (n, (value, label, on)) in choices.into_iter().enumerate() {
                let popup = popup.clone();
                let agent_mark = (popup == Popup::Agent).then(|| Mark::of(&value, &label));
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("{id}-{n}")))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(8.))
                        .h(px(32.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(theme::panel2()))
                        .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
                        .children(agent_mark.map(|m| ui::mark(faces, &m, 16.)))
                        .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                        .on_click(cx.listener(move |this, _, _, cx| match &popup {
                            Popup::Mode => this.set_mode(value.clone(), cx),
                            Popup::Option(config) => {
                                this.set_option(config.clone(), value.clone(), cx);
                            }
                            Popup::Agent => this.switch_agent(&value, cx),
                            Popup::Closed => {}
                        })),
                );
            }
            wrap = wrap.child(
                div()
                    .absolute()
                    .bottom(px(32.))
                    .left(px(0.))
                    .child(ui::menu(SharedString::from(format!("{id}-menu"))).child(menu)),
            );
        }
        wrap
    }
}
