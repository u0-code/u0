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
        // Archived: read only until it is restored.
        if s.flags.archived {
            let key = s.key;
            return div().flex().flex_none().justify_center().pb(px(18.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w(px(column.min(COLUMN)))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme::border2())
                    .child(ui::icon("archive", 16.).text_color(theme::faint()))
                    .child(
                        ui::text(faces, "Archived. Restore it to write in it again.")
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(theme::text2()),
                    )
                    .child(
                        ui::button(faces, "restore", "Restore", ui::Weight::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
                    ),
            );
        }
        self.input.update(cx, |i, cx| {
            i.set_placeholder(&format!("Reply to {name}, or tell it what to do next…"), cx);
        });
        let focused = gpui::Focusable::focus_handle(self.input.read(cx), cx).is_focused(window);
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.input.read(cx).is_empty();

        let draft = s.state == State::Draft && s.items.is_empty();
        // The agent and its options, wrapping onto a second line when an
        // agent offers many; the context, dictation and send stay right.
        let mut bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .ml(px(-2.));
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
        // A draft can go to every agent at once, a session for each.
        let usable = self
            .slots
            .iter()
            .filter(|x| {
                matches!(
                    x.stage,
                    crate::hub::Stage::Ready
                        | crate::hub::Stage::Starting
                        | crate::hub::Stage::Running { .. }
                )
            })
            .count();
        if draft && usable > 1 {
            let on = self.fanout;
            bar = bar.child(
                div()
                    .id("fanout")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .hover(|st| st.bg(theme::hover()))
                    .child(
                        ui::icon(if on { "check" } else { "plus" }, 12.).text_color(if on {
                            theme::accent()
                        } else {
                            theme::faint()
                        }),
                    )
                    .child(
                        ui::text(faces, format!("All {usable} agents"))
                            .text_size(px(12.5))
                            .text_color(if on { theme::text() } else { theme::faint() }),
                    )
                    .tooltip(ui::tip("Send this to every agent, a session for each"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.fanout = !this.fanout;
                        cx.notify();
                    })),
            );
        }
        let mode_option = s
            .options
            .iter()
            .any(|o| o.category == Some(SessionConfigOptionCategory::Mode));
        let mut chips: Vec<(Popup, String, String, Choices)> = Vec::new();
        // Every option the agent offers: its model, how hard it thinks and
        // its mode first, then the rest it lists (a speed, a way of working).
        let rank = |o: &&agent_client_protocol_schema::v1::SessionConfigOption| match o.category {
            Some(SessionConfigOptionCategory::Model) => 0,
            Some(SessionConfigOptionCategory::ThoughtLevel) => 1,
            Some(SessionConfigOptionCategory::Mode) => 2,
            _ => 3,
        };
        let mut options: Vec<_> = s.options.iter().collect();
        options.sort_by_key(rank);
        for option in options {
            let Some((mut current, choices)) = select_choices(&option.kind) else {
                continue;
            };
            // One outside the usual three says what it is: "Fast mode: Off".
            if rank(&option) == 3 {
                current = format!("{}: {current}", option.name);
            }
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
        let chips = bar.flex_1().min_w(px(0.));
        let mut bar = div().flex().items_end().gap(px(4.)).child(chips);
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
        // macOS dictation, typed into the line as you speak.
        let input = self.input.clone();
        bar = bar.child(
            ui::icon_button("dictate", "mic", "Dictate")
                .size(px(26.))
                .text_color(theme::faint())
                .on_click(move |_, window, cx| {
                    window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx));
                    crate::icon::dictate();
                }),
        );
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
            let live = !empty;
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
                .tooltip(ui::tip(if working {
                    "Queue it for when this turn ends · Return"
                } else {
                    "Send · Return, or ⌘Return to go on in the background"
                }))
                .on_click(cx.listener(move |_, _, _, cx| {
                    input.update(cx, crate::input::TextInput::send);
                }))
        };
        bar = bar.child(button);

        // An agent that wants a sign-in says so above the line, with its
        // way to sign in, before anything said to it is refused.
        let sign_in = match self.slot(&s.agent).map(|x| &x.auth) {
            Some(crate::hub::Auth::Needed(why)) => {
                let (a, b) = (s.agent.clone(), s.agent.clone());
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w(px(column.min(COLUMN)))
                        .mb(px(8.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(12.))
                        .bg(theme::rail())
                        .border_1()
                        .border_color(theme::border2())
                        .child(ui::icon("alert", 16.).text_color(theme::accent()))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    ui::text(faces, format!("{name} needs you to sign in"))
                                        .text_size(px(13.)),
                                )
                                .child(ui::one_line(
                                    ui::text(faces, ui::fit(why, 80))
                                        .text_size(px(11.5))
                                        .text_color(theme::faint()),
                                )),
                        )
                        .child(
                            ui::button(faces, "sign-in-now", "Sign in", ui::Weight::Primary)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.sign_in_agent(&a, cx);
                                })),
                        )
                        .child(
                            ui::button(faces, "signed-in", "I've signed in", ui::Weight::Quiet)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.recheck_auth(&b, cx);
                                })),
                        ),
                )
            }
            _ => None,
        };

        // What is offered for the word being typed: the agent's commands
        // after `/`, after `@` files, sessions and pull requests. Tab takes
        // the first.
        let typed = self.input.read(cx).text();
        let offers = self
            .offers(&typed)
            .filter(|(_, o)| !o.is_empty())
            .map(|(_, list)| {
                use crate::desk::mention::Offer;
                let mut menu = div().flex().flex_col().p(px(6.)).w(px(column.min(COLUMN)));
                for (n, offer) in list.into_iter().enumerate() {
                    let (icon, name, about) = match offer {
                        Offer::Command { name, about } => ("terminal", format!("/{name}"), about),
                        Offer::File(f) => ("file", f, String::new()),
                        Offer::Session { title, .. } => ("list", title, "session".into()),
                        Offer::Pr { number, title } => (
                            "branch",
                            format!("#{number} {title}"),
                            "pull request".into(),
                        ),
                    };
                    menu = menu.child(
                        div()
                            .id(SharedString::from(format!("offer-{n}")))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(8.))
                            .h(px(30.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .when(n == 0, |d| d.bg(theme::panel2()))
                            .hover(|st| st.bg(theme::hover()))
                            .child(ui::icon(icon, 14.).text_color(theme::faint()))
                            .child(
                                ui::one_line(
                                    ui::mono(faces, name)
                                        .text_size(px(12.5))
                                        .text_color(theme::text()),
                                )
                                .w_auto()
                                .max_w(px(360.)),
                            )
                            .child(
                                ui::one_line(
                                    ui::text(faces, about)
                                        .text_size(px(12.))
                                        .text_color(theme::faint()),
                                )
                                .flex_1(),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.take_offer(n, cx))),
                    );
                }
                div().mb(px(8.)).child(ui::menu("offers").child(menu))
            });

        // Images waiting to go with what is said.
        let pictures = (!self.pictures.is_empty()).then(|| {
            let mut row = div().flex().flex_wrap().gap(px(6.));
            for (n, p) in self.pictures.iter().enumerate() {
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(26.))
                        .pl(px(8.))
                        .pr(px(2.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(theme::tag_line())
                        .child(ui::icon("file", 12.).text_color(theme::faint()))
                        .child(
                            ui::text(
                                faces,
                                format!("Image {} · {} KB", n + 1, p.bytes.len() / 1024),
                            )
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("drop-pic-{n}")),
                                "x",
                                "Take it out",
                            )
                            .size(px(20.))
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_picture(n, cx))),
                        ),
                );
            }
            row
        });

        // What was said while the turn goes on, waiting its turn: each can
        // be said now (the turn is stopped first) or taken back to edit.
        let queued = (!s.queued.is_empty()).then(|| {
            let mut list = div().flex().flex_col().gap(px(4.));
            for (n, text) in s.queued.iter().enumerate() {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("clock", 12.).text_color(theme::faint()))
                        .child(
                            ui::one_line(
                                ui::text(faces, ui::fit(text, 70))
                                    .text_size(px(12.5))
                                    .text_color(theme::text2()),
                            )
                            .flex_1(),
                        )
                        .child(
                            ui::button(
                                faces,
                                SharedString::from(format!("steer-{n}")),
                                "Send now",
                                ui::Weight::Quiet,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.steer(n, cx))),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("unqueue-{n}")),
                                "pencil",
                                "Edit",
                            )
                            .size(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.unqueue(n, cx))),
                        ),
                );
            }
            list
        });

        // A question waiting is answered here too, and a kind of call can
        // be allowed for the rest of the session.
        let approval = s.waiting().map(|asked| {
            use agent_client_protocol_schema::v1::{PermissionOptionKind, ToolKind};
            let headline = match asked.kind {
                Some(ToolKind::Execute) => format!("{name} wants to run a command"),
                Some(ToolKind::Edit) => format!("{name} wants to edit a file"),
                Some(ToolKind::Delete) => format!("{name} wants to delete a file"),
                Some(ToolKind::Fetch) => format!("{name} wants to fetch a page"),
                _ => format!("{name} asks before it goes on"),
            };
            let mut buttons = div().flex().flex_wrap().items_center().gap(px(6.));
            for (n, option) in asked.options.iter().enumerate() {
                let weight = match option.kind {
                    PermissionOptionKind::AllowOnce => ui::Weight::Primary,
                    PermissionOptionKind::AllowAlways => ui::Weight::Secondary,
                    _ => ui::Weight::Quiet,
                };
                let (request, chosen) = (asked.id.clone(), option.clone());
                buttons = buttons.child(
                    ui::button(
                        faces,
                        SharedString::from(format!("bar-ask-{n}")),
                        option.name.clone(),
                        weight,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.choose(&request, &chosen, cx)),
                    ),
                );
            }
            if let (Some(kind), Some(once)) = (
                asked.kind,
                asked
                    .options
                    .iter()
                    .find(|o| o.kind == PermissionOptionKind::AllowOnce),
            ) {
                let (request, chosen) = (asked.id.clone(), once.clone());
                buttons = buttons.child(
                    ui::button(
                        faces,
                        "bar-ask-session",
                        "Allow for this session",
                        ui::Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.allow_for_session(&request, &chosen, kind, cx);
                    })),
                );
            }
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w(px(column.min(COLUMN)))
                .mb(px(8.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(12.))
                .bg(theme::rail())
                .border_1()
                .border_color(theme::border2())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::live_dot())
                        .child(ui::text(faces, headline).text_size(px(13.)))
                        .child(
                            ui::one_line(
                                ui::mono(faces, ui::fit(&asked.title, 60))
                                    .text_size(px(12.5))
                                    .text_color(theme::text2()),
                            )
                            .flex_1(),
                        ),
                )
                .child(buttons)
        });

        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .pb(px(18.))
            .children(sign_in)
            .children(approval)
            .children(offers)
            .child(
                div()
                    .id("composer")
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w(px(column.min(COLUMN)))
                    .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                        this.drop_files(paths.paths(), cx);
                    }))
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
                    .children(queued)
                    .children(pictures)
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
            Popup::Closed | Popup::Session(_) | Popup::Only | Popup::Project(_) => {
                "chip".to_owned()
            }
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
        let mut wrap = div().relative().flex().flex_none().child(chip);
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
                            Popup::Closed | Popup::Session(_) | Popup::Only | Popup::Project(_) => {
                            }
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
