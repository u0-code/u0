//! The line you type into, and the chips under it: the agent, its model
//! and mode, how full its context is, and Send or Stop.

use agent_client_protocol_schema::v1::SessionConfigOptionCategory;
use gpui::{Context, Div, SharedString, Stateful, Window, div, prelude::*, px};

use super::{Choices, select_choices};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    session::State,
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
                    .w(px(column))
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
        // A subagent's session answers to its parent, not to you.
        if let Some(up) = s.parent {
            let parent = self
                .sessions
                .iter()
                .find(|p| p.key == up)
                .map(|p| ui::fit(&p.title, 40))
                .unwrap_or_default();
            return div().flex().flex_none().justify_center().pb(px(18.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w(px(column))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme::border2())
                    .child(ui::icon("user", 16.).text_color(theme::faint()))
                    .child(
                        ui::text(
                            faces,
                            format!("A subagent of “{parent}”: it answers to it."),
                        )
                        .flex_1()
                        .text_size(px(13.))
                        .text_color(theme::text2()),
                    )
                    .child(
                        ui::button(faces, "to-parent", "Back to it", ui::Weight::Secondary)
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.show(up, window, cx)),
                            ),
                    ),
            );
        }
        self.input.update(cx, |i, cx| {
            i.set_placeholder(
                &if s.items.is_empty() {
                    format!("Tell {name} what to do…")
                } else {
                    format!("Reply to {name}, or tell it what to do next…")
                },
                cx,
            );
        });
        let focused = gpui::Focusable::focus_handle(self.input.read(cx), cx).is_focused(window);
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.input.read(cx).is_empty();

        let draft = s.state == State::Draft && s.items.is_empty();
        // One line, never two: the agent, its model and mode as words, every
        // other option behind the slider mark; the context, dictation and
        // send at the right.
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
        let agent_label = match (draft, self.fanout) {
            (true, true) => format!("All {} agents", self.usable_agents()),
            (true, false) => name.clone(),
            (false, _) => String::new(),
        };
        let mut words = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap(px(2.))
            .ml(px(-6.))
            .child(self.chip(
                faces,
                &Popup::Agent,
                Some(&Mark::of(&s.agent, &name)),
                &agent_label,
                "Start this session with",
                agent_choices,
                cx,
            ));
        let mode_option = s
            .options
            .iter()
            .any(|o| o.category == Some(SessionConfigOptionCategory::Mode));
        // The model first, then the mode; the rest (how hard it thinks, a
        // speed, a way of working) in the order the agent lists them.
        let rank = |o: &&agent_client_protocol_schema::v1::SessionConfigOption| match o.category {
            Some(SessionConfigOptionCategory::Model) => 0,
            Some(SessionConfigOptionCategory::Mode) => 1,
            _ => 2,
        };
        let mut options: Vec<_> = s.options.iter().collect();
        options.sort_by_key(rank);
        let mut more: Vec<More> = Vec::new();
        for option in options {
            let Some((current, mut choices)) = select_choices(&option.kind) else {
                continue;
            };
            // Model ids of yours, beside those the agent lists.
            if option.category == Some(SessionConfigOptionCategory::Model) {
                for m in self.settings.models.get(&s.agent).into_iter().flatten() {
                    if !choices.iter().any(|(v, _, _)| v == m) {
                        choices.push((m.clone(), m.clone(), false));
                    }
                }
            }
            let id = option.id.0.to_string();
            if rank(&option) < 2 {
                let head = format!("{name} {}", option.name.to_lowercase());
                words = words.child(self.chip(
                    faces,
                    &Popup::Option(id),
                    None,
                    &current,
                    &head,
                    choices,
                    cx,
                ));
            } else {
                more.push(More {
                    id,
                    name: option.name.clone(),
                    current,
                    choices,
                });
            }
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
            words =
                words.child(self.chip(faces, &Popup::Mode, None, &current, "Mode", choices, cx));
        }
        if !more.is_empty() {
            words = words.child(self.more(faces, &name, more, cx));
        }
        if !self.stashed.is_empty() {
            words = words.child(self.stash_menu(faces, cx));
        }
        let mut bar = div()
            .flex()
            .items_center()
            .h(px(28.))
            .gap(px(2.))
            .child(words);
        if let Some(u) = s.usage.as_ref().filter(|u| u.size > 0) {
            let share = u.used as f32 / u.size as f32;
            bar = bar.child(
                div()
                    .id("context-used")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .child(ui::ring(share, 16.))
                    .tooltip(ui::tip(format!(
                        "{}% of the context used",
                        (share * 100.).round()
                    ))),
            );
        }
        // A window of yours, captured to go with it; the line opened tall.
        let tall = self.tall;
        bar = bar
            .child(
                ui::icon_button("capture", "camera", "Attach a window")
                    .text_color(theme::faint())
                    .on_click(cx.listener(|this, _, _, cx| this.capture(cx))),
            )
            .child(
                ui::icon_button(
                    "tall",
                    "expand",
                    if tall {
                        "Make it small"
                    } else {
                        "Write at length"
                    },
                )
                .text_color(if tall { theme::text2() } else { theme::faint() })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tall = !this.tall;
                    let most = if this.tall { 30 } else { 8 };
                    this.input.update(cx, |i, cx| i.set_most(most, cx));
                    cx.notify();
                })),
            );
        // macOS dictation, typed into the line as you speak.
        let input = self.input.clone();
        bar = bar.child(
            ui::icon_button("dictate", "mic", "Dictate")
                .text_color(theme::faint())
                .on_click(move |_, window, cx| {
                    window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx));
                    crate::icon::dictate();
                }),
        );
        let button = div()
            .size(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(8.));
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
                .child(ui::icon("arrow-up", 13.).text_color(if live {
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
                        .w(px(column))
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
                let mut menu = div().flex().flex_col().p(px(6.)).w(px(column));
                for (n, offer) in list.into_iter().enumerate() {
                    let (icon, name, about) = match offer {
                        Offer::Command { name, about } => ("terminal", format!("/{name}"), about),
                        Offer::Own { name, about } => {
                            ("ghost", format!("/{name}"), about.to_owned())
                        }
                        Offer::Model { label, .. } => (
                            "sliders",
                            format!("@{label}"),
                            "switch to this model".into(),
                        ),
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
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.take_offer(n, window, cx);
                            })),
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
                        .when(n > 0, |d| {
                            d.child(
                                ui::icon_button(
                                    SharedString::from(format!("queue-up-{n}")),
                                    "arrow-up",
                                    "Earlier",
                                )
                                .size(px(22.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.queue_earlier(n, cx)),
                                ),
                            )
                        })
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
                .w(px(column))
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
                    .gap(px(6.))
                    .w(px(column))
                    .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                        this.drop_files(paths.paths(), cx);
                    }))
                    .pl(px(14.))
                    .pr(px(8.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .rounded(px(14.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(if focused {
                        theme::ghost_line()
                    } else {
                        theme::border2()
                    })
                    .children(queued)
                    .children(pictures)
                    .child(
                        div()
                            .min_h(px(if self.tall { 300. } else { 22. }))
                            .pr(px(6.))
                            .child(self.input.clone()),
                    )
                    .child(bar),
            )
    }

    /// How many agents a session can be begun with now.
    fn usable_agents(&self) -> usize {
        self.slots.iter().filter(|x| x.usable()).count()
    }

    /// A word that opens a menu of choices, `(id, name, current)` each;
    /// with none, a word alone. An agent's is its mark, named with its menu
    /// in a new session.
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
            _ => "chip".to_owned(),
        };
        let for_click = popup.clone();
        let named = !current.is_empty();
        let mut chip: Stateful<Div> = div()
            .id(SharedString::from(id.clone()))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(7.))
            .rounded(px(7.))
            .when(open, |d| d.bg(theme::hover()))
            .children(mark.map(|m| ui::mark(faces, m, 15.)))
            .when(named, |d| {
                d.child(
                    ui::one_line(
                        ui::text(faces, ui::fit(current, 24))
                            .text_size(px(12.5))
                            .text_color(if mark.is_some() {
                                theme::text2()
                            } else {
                                theme::muted()
                            }),
                    )
                    .w_auto(),
                )
            });
        if menu_able {
            chip = chip
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .when(mark.is_some(), |d| {
                    d.child(
                        ui::icon("chevron-down", 11.)
                            .flex_none()
                            .text_color(theme::faint()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_popup(for_click.clone(), cx);
                }));
        }
        let mut wrap = div().relative().flex().flex_none().child(chip);
        if open {
            let mut menu = div()
                .flex()
                .flex_col()
                .w(px(260.))
                .p(px(6.))
                .child(ui::label(faces, head).px(px(10.)).pt(px(6.)).pb(px(4.)));
            // A long list can be narrowed by what is typed.
            let long = choices.len() > 8;
            let query = self.menu_search.read(cx).text().to_lowercase();
            if long {
                menu = menu.child(
                    div()
                        .mx(px(4.))
                        .mb(px(4.))
                        .px(px(8.))
                        .rounded(px(7.))
                        .bg(theme::well())
                        .border_1()
                        .border_color(theme::border2())
                        .child(self.menu_search.clone()),
                );
            }
            let mut scroll = div()
                .id(SharedString::from(format!("{id}-list")))
                .flex()
                .flex_col()
                .max_h(px(360.))
                .overflow_y_scroll();
            for (n, (value, label, on)) in choices.into_iter().enumerate() {
                if !query.is_empty()
                    && !label.to_lowercase().contains(&query)
                    && !value.to_lowercase().contains(&query)
                {
                    continue;
                }
                let popup = popup.clone();
                let agent_mark = (popup == Popup::Agent).then(|| Mark::of(&value, &label));
                scroll = scroll.child(
                    row(SharedString::from(format!("{id}-{n}")), on)
                        .children(agent_mark.map(|m| ui::mark(faces, &m, 16.)))
                        .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                        .when(on, |d| {
                            d.child(ui::icon("check", 13.).text_color(theme::muted()))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| match &popup {
                            Popup::Mode => this.set_mode(value.clone(), cx),
                            Popup::Option(config) => {
                                this.set_option(config.clone(), value.clone(), cx);
                            }
                            Popup::Agent => this.switch_agent(&value, cx),
                            _ => {}
                        })),
                );
            }
            menu = menu.child(scroll);
            // A new session can go to every agent at once, one each.
            let usable = self.usable_agents();
            if *popup == Popup::Agent && usable > 1 {
                let on = self.fanout;
                menu = menu
                    .child(div().h(px(1.)).mx(px(2.)).my(px(4.)).bg(theme::border2()))
                    .child(
                        row("fanout".into(), on)
                            .child(ui::icon("plus", 14.).text_color(theme::muted()))
                            .child(
                                ui::text(faces, format!("Send to all {usable} agents"))
                                    .flex_1()
                                    .text_size(px(13.)),
                            )
                            .child(
                                ui::text(faces, if on { "on" } else { "a session each" })
                                    .text_size(px(12.))
                                    .text_color(theme::faint()),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fanout = !this.fanout;
                                this.popup = Popup::Closed;
                                cx.notify();
                            })),
                    );
            }
            wrap = wrap.child(gpui::deferred(
                div()
                    .absolute()
                    .bottom(px(LIFT))
                    .left(px(0.))
                    .child(ui::menu(SharedString::from(format!("{id}-menu"))).child(menu)),
            ));
        }
        wrap
    }

    /// The slider mark, and under it the options beyond the model and the
    /// mode, each with its value; one chosen opens its choices beside.
    fn more(&self, faces: &Faces, name: &str, more: Vec<More>, cx: &mut Context<Self>) -> Div {
        let open_one = match &self.popup {
            Popup::Option(id) => more.iter().position(|m| &m.id == id),
            _ => None,
        };
        let open = self.popup == Popup::More || open_one.is_some();
        let tip = more
            .iter()
            .map(|m| format!("{}: {}", m.name, m.current))
            .collect::<Vec<_>>()
            .join(" · ");
        let mut wrap = div().relative().flex().flex_none().child(
            div()
                .id("chip-more")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(theme::faint())
                .hover(|st| st.bg(theme::hover()))
                .when(open, |d| d.bg(theme::hover()))
                .child(ui::icon("sliders", 15.))
                .tooltip(ui::tip(tip))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_popup(Popup::More, cx))),
        );
        if !open {
            return wrap;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .w(px(280.))
            .p(px(6.))
            .child(ui::label(faces, name).px(px(10.)).pt(px(6.)).pb(px(4.)));
        let mut beside = None;
        for (n, m) in more.into_iter().enumerate() {
            let on = open_one == Some(n);
            let id = m.id.clone();
            list = list.child(
                row(SharedString::from(format!("more-{n}")), on)
                    .child(ui::text(faces, m.name).flex_1().text_size(px(13.)))
                    .child(
                        ui::text(faces, ui::fit(&m.current, 18))
                            .text_size(px(12.5))
                            .text_color(theme::muted()),
                    )
                    .child(ui::icon("chevron-right", 11.).text_color(theme::faint()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.popup = Popup::Option(id.clone());
                        cx.notify();
                    })),
            );
            if on {
                let mut choices = div().flex().flex_col().w(px(200.)).p(px(6.));
                for (k, (value, label, chosen)) in m.choices.into_iter().enumerate() {
                    let config = m.id.clone();
                    choices = choices.child(
                        row(SharedString::from(format!("more-{n}-{k}")), chosen)
                            .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                            .when(chosen, |d| {
                                d.child(ui::icon("check", 13.).text_color(theme::muted()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_option(config.clone(), value.clone(), cx);
                            })),
                    );
                }
                beside = Some(
                    div()
                        .absolute()
                        .left(px(286.))
                        .bottom(px(0.))
                        .child(ui::menu("more-choices").child(choices)),
                );
            }
        }
        wrap = wrap.child(gpui::deferred(
            div().absolute().bottom(px(LIFT)).left(px(0.)).child(
                div()
                    .relative()
                    .child(ui::menu("more-menu").child(list))
                    .children(beside),
            ),
        ));
        wrap
    }
}

/// How far above the foot of the bar its menus open: clear of the line
/// you write in (the bar, the gap, one line, the padding, and 8 more).
const LIFT: f32 = 28. + 6. + 22. + 10. + 8.;

impl Desk {
    /// What you put aside (⌘S): a mark with how many, and under it each,
    /// the latest first, to take back into the line or let go.
    fn stash_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let open = self.popup == Popup::Stash;
        let mut wrap = div().relative().flex().flex_none().child(
            div()
                .id("chip-stash")
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(26.))
                .px(px(7.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(theme::faint())
                .hover(|st| st.bg(theme::hover()))
                .when(open, |d| d.bg(theme::hover()))
                .child(ui::icon("archive", 14.))
                .child(ui::text(faces, self.stashed.len().to_string()).text_size(px(12.)))
                .tooltip(ui::tip("Put aside (⌘S), to take back"))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_popup(Popup::Stash, cx))),
        );
        if open {
            let mut list = div().flex().flex_col().w(px(360.)).p(px(6.)).child(
                ui::label(faces, "Put aside")
                    .px(px(10.))
                    .pt(px(6.))
                    .pb(px(4.)),
            );
            for (n, text) in self.stashed.iter().enumerate().rev() {
                list = list.child(
                    row(SharedString::from(format!("stash-{n}")), false)
                        .child(
                            ui::one_line(
                                ui::text(faces, text.lines().next().unwrap_or_default().to_owned())
                                    .text_size(px(13.)),
                            )
                            .flex_1()
                            .min_w(px(0.)),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("stash-drop-{n}")),
                                "x",
                                "Let it go",
                            )
                            .size(px(22.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if n < this.stashed.len() {
                                        this.stashed.remove(n);
                                        this.save_index();
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.take_stashed(n, window, cx);
                        })),
                );
            }
            wrap = wrap.child(gpui::deferred(
                div()
                    .absolute()
                    .bottom(px(LIFT))
                    .left(px(0.))
                    .child(ui::menu("stash-menu").child(list)),
            ));
        }
        wrap
    }

    /// Take back what was put aside `n`th: into the line, after what is
    /// in it.
    fn take_stashed(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if n >= self.stashed.len() {
            return;
        }
        let text = self.stashed.remove(n);
        self.save_index();
        self.popup = Popup::Closed;
        let typed = self.input.read(cx).text();
        let line = if typed.is_empty() {
            text
        } else {
            format!("{typed}\n{text}")
        };
        self.input.update(cx, |i, cx| i.set_text(&line, cx));
        window.focus(&gpui::Focusable::focus_handle(self.input.read(cx), cx));
        cx.notify();
    }
}

/// An option kept behind the slider mark: its id, name, value now, and
/// what it can be.
struct More {
    id: String,
    name: String,
    current: String,
    choices: Choices,
}

/// A row of a menu, marked when it is the one chosen.
fn row(id: SharedString, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(10.))
        .h(px(32.))
        .rounded(px(7.))
        .cursor_pointer()
        .when(on, |d| d.bg(theme::hover()))
        .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
}
