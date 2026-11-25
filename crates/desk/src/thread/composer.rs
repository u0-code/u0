//! The line you type into, and the chips under it: the agent, its model
//! and mode, how full its context is, and Send or Stop.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory;
use gpui::component::{
    Sizable as _,
    button::Button,
    input::{Enter, Input, MoveDown, MoveUp, Textarea},
    kbd::Kbd,
    popover::Popover,
    progress::ProgressCircle,
    scroll::ScrollableElement as _,
};
use gpui::{AnyElement, Context, Div, SharedString, Stateful, Window, div, prelude::*, px};

use super::{Choices, select_choices};
use crate::{
    desk::{Desk, Popup, mention},
    fonts::Faces,
    keys::{SendAside, Stash},
    session::{Session, State},
    theme,
    ui::{self, Mark},
};

impl Desk {
    /// The line you type into, and under it the agent, its model and mode,
    /// how full its context is, and Send or Stop; over it what asks for
    /// you first.
    pub(crate) fn composer(
        &self,
        faces: &Faces,
        column: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(s) = self.session() else {
            return div();
        };
        if let Some(shut) = self.shut(faces, s, column, cx) {
            return shut;
        }
        let name = self.name_of(&s.meta.agent);
        let says = if s.items.is_empty() {
            format!("Tell {name} what to do…")
        } else {
            format!("Reply to {name}, or tell it what to do next…")
        };
        // Said again only when it changes: setting it draws the line again.
        self.composer.input.update(cx, |i, cx| {
            if *i.presentation().placeholder() != says {
                i.set_placeholder(says, window, cx);
            }
        });
        let focused =
            gpui::Focusable::focus_handle(self.composer.input.read(cx), cx).is_focused(window);
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .pb(px(18.))
            .children(self.side_question(faces, s, column, cx))
            .children(self.sign_in_banner(faces, s, &name, column, cx))
            .children(Self::approval_card(faces, s, &name, column, cx))
            .children(self.offers_menu(faces, column, cx))
            .child(
                div()
                    .id("composer")
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .w(px(column))
                    .on_drop(
                        cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                            this.drop_files(paths.paths(), window, cx);
                        }),
                    )
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
                    .children(Self::queued_list(faces, s, cx))
                    .children(self.pictures_row(faces, cx))
                    .child(
                        div()
                            .min_h(px(if self.composer.tall { 300. } else { 22. }))
                            .pr(px(6.))
                            .child(self.composer_line(cx)),
                    )
                    .child(self.bar(faces, s, &name, cx)),
            )
    }

    /// The line itself: gpui-kit's text area, with the composer's own keys
    /// over it. ⌘S puts what is typed aside, ⇧⌘Return sends it aside; a
    /// new line in a list begins its next item; ↑ and ↓ on its first and
    /// last lines go back through what was sent; an image pasted goes
    /// with what you say.
    fn composer_line(&self, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity().downgrade();
        div()
            .key_context(mention::CONTEXT)
            .on_action(cx.listener(|this, _: &Stash, window, cx| this.stash_typed(window, cx)))
            .on_action(cx.listener(|this, _: &SendAside, window, cx| {
                this.send_typed(true, window, cx);
            }))
            .capture_action(cx.listener(|this, enter: &Enter, window, cx| {
                let breaks = !enter.secondary && (enter.shift || this.settings.cmd_sends);
                if breaks && this.continue_list(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.recall(-1, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.recall(1, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(
                Textarea::new(&self.composer.input)
                    .appearance(false)
                    .on_paste(move |item, _, cx| {
                        let Some(image) = item.entries().iter().find_map(|e| match e {
                            gpui::ClipboardEntry::Image(i) => Some(i.clone()),
                            _ => None,
                        }) else {
                            return false;
                        };
                        desk.update(cx, |this, cx| this.paste_picture(&image, cx))
                            .is_ok()
                    }),
            )
    }

    /// In place of the line, for a session you do not write in: one
    /// archived, until it is restored; a subagent's, which answers to its
    /// parent, not to you.
    fn shut(&self, faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let (icon, says, button) = if s.meta.flags.archived {
            let key = s.key;
            (
                "archive",
                "Archived. Restore it to write in it again.".to_owned(),
                ui::button("restore", "Restore", ui::Weight::Secondary)
                    .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
            )
        } else {
            let up = s.parent?;
            let parent = self
                .sessions
                .at(up)
                .map(|p| ui::fit(&p.meta.title, 40))
                .unwrap_or_default();
            (
                "user",
                format!("A subagent of “{parent}”: it answers to it."),
                ui::button("to-parent", "Back to it", ui::Weight::Secondary)
                    .on_click(cx.listener(move |this, _, window, cx| this.show(up, window, cx))),
            )
        };
        Some(
            div().flex().flex_none().justify_center().pb(px(18.)).child(
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
                    .child(ui::icon(icon, 16.).text_color(theme::faint()))
                    .child(
                        ui::text(faces, says)
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(theme::text2()),
                    )
                    .child(button),
            ),
        )
    }

    /// The agent, its model and mode as words, every other option behind
    /// the slider mark; what was put aside, and what is to be said later.
    fn chips(&self, faces: &Faces, s: &Session, name: &str, cx: &mut Context<Self>) -> Div {
        let draft = s.state == State::Draft && s.items.is_empty();
        let agent_choices = if draft {
            self.slots
                .iter()
                .map(|x| {
                    (
                        x.entry.id.clone(),
                        self.name_of(&x.entry.id),
                        x.entry.id == s.meta.agent,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let agent_label = match (draft, self.composer.fanout) {
            (true, true) => format!("All {} agents", self.usable_agents()),
            (true, false) => name.to_owned(),
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
                Some(&self.mark_of(&s.meta.agent)),
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
        let rank = |o: &&agent_client_protocol::v1::schema::SessionConfigOption| match o.category {
            Some(SessionConfigOptionCategory::Model) => 0,
            Some(SessionConfigOptionCategory::Mode) => 1,
            _ => 2,
        };
        let mut options: Vec<_> = s.options.iter().collect();
        options.sort_by_key(rank);
        let mut more: Vec<More> = Vec::new();
        for option in options {
            let Some((current, choices)) = self.option_choices(s, option) else {
                continue;
            };
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
        if let Some((current, choices)) = mode_choices(s).filter(|_| !mode_option) {
            words =
                words.child(self.chip(faces, &Popup::Mode, None, &current, "Mode", choices, cx));
        }
        if !more.is_empty() {
            words = words.child(self.more(faces, name, more, cx));
        }
        if !self.composer.stashed.is_empty() {
            words = words.child(self.stash_menu(faces, cx));
        }
        if s.meta.id.is_some() && s.parent.is_none() {
            words = words.child(self.later_menu(faces, s, cx));
        }
        words
    }

    /// The line under the text, one line never two: the chips, then the
    /// context, a window to attach, writing at length, dictation and send
    /// at the right.
    fn bar(&self, faces: &Faces, s: &Session, name: &str, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex()
            .items_center()
            .h(px(28.))
            .gap(px(2.))
            .child(self.chips(faces, s, name, cx));
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
                    .child(
                        ProgressCircle::new("context-ring")
                            .value(share * 100.)
                            .color(theme::muted())
                            .size(px(16.)),
                    )
                    .tooltip(ui::tip(format!(
                        "{}% of the context used",
                        (share * 100.).round()
                    ))),
            );
        }
        // A window of yours, captured to go with it; the line opened tall.
        let tall = self.composer.tall;
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
                    this.composer.tall = !this.composer.tall;
                    let most = if this.composer.tall { 30 } else { 8 };
                    this.composer
                        .input
                        .update(cx, |i, cx| i.set_auto_grow(1, most, cx));
                    cx.notify();
                })),
            );
        // macOS dictation, typed into the line as you speak.
        let input = self.composer.input.clone();
        bar = bar.child(
            ui::icon_button("dictate", "mic", "Dictate")
                .text_color(theme::faint())
                .on_click(move |_, window, cx| {
                    window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
                    crate::macos::dictate();
                }),
        );
        bar.child(self.send_button(faces, s, cx))
    }

    /// Send; Stop while a turn goes on and nothing is typed; Force stop
    /// when asked to stop and still going a while after.
    fn send_button(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Stateful<Div> {
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.composer.input.read(cx).value().trim().is_empty();
        let button = div()
            .size(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(8.));
        // Asked to stop, and still going a while after: stopped by force.
        let key = s.key;
        let force = working
            && self
                .stopping
                .is_some_and(|(k, t)| k == key && t.elapsed() > std::time::Duration::from_secs(8));
        if force {
            button
                .id("force-stop")
                .w_auto()
                .px(px(10.))
                .cursor_pointer()
                .border_1()
                .border_color(theme::coral().opacity(0.5))
                .hover(|st| st.bg(theme::hover()))
                .child(
                    ui::text(faces, "Force stop")
                        .text_size(px(12.5))
                        .text_color(theme::coral()),
                )
                .tooltip(ui::tip(
                    "It would not stop: its process is ended, and started again when wanted",
                ))
                .on_click(cx.listener(move |this, _, _, cx| this.force_stop(key, cx)))
        } else if working && empty {
            button
                .id("stop")
                .cursor_pointer()
                .bg(theme::panel2())
                .hover(|st| st.bg(theme::ghost_line()))
                .child(div().size(px(8.)).rounded(px(1.5)).bg(theme::text()))
                .tooltip(ui::tip(crate::keys::hint(cx, "Stop the turn", "stop")))
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else {
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
                .on_click(cx.listener(|this, _, window, cx| this.send_typed(false, window, cx)))
        }
    }

    /// An agent that wants a sign-in says so above the line, with its
    /// way to sign in, before anything said to it is refused.
    fn sign_in_banner(
        &self,
        faces: &Faces,
        s: &Session,
        name: &str,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let Some(crate::hub::Auth::Needed(why)) = self.slot(&s.meta.agent).map(|x| &x.auth) else {
            return None;
        };
        let (a, b) = (s.meta.agent.clone(), s.meta.agent.clone());
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
                    ui::button("sign-in-now", "Sign in", ui::Weight::Primary).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.sign_in_agent(&a, cx);
                        }),
                    ),
                )
                .child(
                    ui::button("signed-in", "I've signed in", ui::Weight::Quiet).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.recheck_auth(&b, cx);
                        }),
                    ),
                ),
        )
    }

    /// What is offered for the word being typed: the agent's commands
    /// after `/`, after `@` files, sessions and pull requests. ↑ and ↓
    /// choose one, Return or Tab takes it.
    fn offers_menu(&self, faces: &Faces, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let typed = self.typed(cx);
        self.offers(&typed)
            .filter(|(_, o)| !o.is_empty())
            .map(|(_, list)| {
                use crate::desk::mention::Offer;
                let mut menu = div().flex().flex_col().p(px(6.)).w(px(column));
                let chosen = self.composer.offer.min(list.len() - 1);
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
                        ui::menu_row(SharedString::from(format!("offer-{n}")), n == chosen)
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
            })
    }

    /// Images waiting to go with what is said.
    fn pictures_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        (!self.composer.pictures.is_empty()).then(|| {
            let mut row = div().flex().flex_wrap().gap(px(6.));
            for (n, p) in self.composer.pictures.iter().enumerate() {
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
        })
    }

    /// What was said while the turn goes on, waiting its turn: each can
    /// be said now (the turn is stopped first) or taken back to edit.
    fn queued_list(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        (!s.meta.queued.is_empty()).then(|| {
            let mut list = div().flex().flex_col().gap(px(4.));
            for (n, text) in s.meta.queued.iter().enumerate() {
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
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.unqueue(n, window, cx)),
                            ),
                        ),
                );
            }
            list
        })
    }

    /// A question waiting is answered here too.
    fn approval_card(
        faces: &Faces,
        s: &Session,
        name: &str,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        s.waiting().map(|asked| {
            use agent_client_protocol::v1::schema::{PermissionOptionKind, ToolKind};
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
                let (request, chosen) = (asked.id, option.clone());
                buttons = buttons.child(
                    ui::button(
                        SharedString::from(format!("bar-ask-{n}")),
                        option.name.clone(),
                        weight,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(request, &chosen, cx))),
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
        })
    }

    /// A side question: what is asked here stays out of the session it
    /// was asked of; its answer can be taken back there.
    fn side_question(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        s.beside.map(|up| {
            let title = self
                .sessions
                .at(up)
                .map(|p| ui::fit(&p.meta.title, 40))
                .unwrap_or_default();
            let key = s.key;
            let answered = s
                .items
                .iter()
                .any(|i| matches!(i, crate::session::Item::Agent(_)));
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w(px(column))
                .mb(px(8.))
                .px(px(14.))
                .py(px(8.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::border2())
                .child(ui::icon("bulb", 15.).text_color(theme::faint()))
                .child(
                    ui::text(
                        faces,
                        format!("A side question of “{title}”: it does not go in it."),
                    )
                    .flex_1()
                    .text_size(px(13.))
                    .text_color(theme::text2()),
                )
                .when(answered, |d| {
                    d.child(
                        ui::button("answer-back", "Add the answer to it", ui::Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.answer_back(key, window, cx);
                            })),
                    )
                })
                .child(
                    ui::button("beside-back", "Back to it", ui::Weight::Quiet).on_click(
                        cx.listener(move |this, _, window, cx| this.show(up, window, cx)),
                    ),
                )
        })
    }

    /// How many agents a session can be begun with now.
    fn usable_agents(&self) -> usize {
        self.slots.iter().filter(|x| x.usable()).count()
    }

    /// A word that opens a menu of choices, `(id, name, current)` each;
    /// with none, a word alone. An agent's is its mark, named with its menu
    /// in a new session.
    fn chip(
        &self,
        faces: &Faces,
        popup: &Popup,
        mark: Option<&Mark>,
        current: &str,
        head: &str,
        choices: Choices,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let says: Vec<AnyElement> = mark
            .map(|m| ui::mark(faces, m, 15.))
            .into_iter()
            .chain((!current.is_empty()).then(|| {
                ui::one_line(
                    ui::text(faces, ui::fit(current, 24))
                        .text_size(px(12.5))
                        .text_color(if mark.is_some() {
                            theme::text2()
                        } else {
                            theme::muted()
                        }),
                )
                .w_auto()
                .into_any_element()
            }))
            .collect();
        if choices.is_empty() {
            return div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px(px(7.))
                .children(says)
                .into_any_element();
        }
        let id = match popup {
            Popup::Mode => "chip-mode".to_owned(),
            Popup::Option(o) => format!("chip-{o}"),
            Popup::Agent => "chip-agent".to_owned(),
            _ => "chip".to_owned(),
        };
        let chip = ui::opener(SharedString::from(id.clone()))
            .h(px(26.))
            .px(px(7.))
            .children(says)
            .child(
                ui::icon("chevron-down", 11.)
                    .flex_none()
                    .text_color(theme::faint()),
            );
        // A long list can be narrowed by what is typed, which goes there as
        // it opens.
        let long = choices.len() > 8;
        let search = gpui::Focusable::focus_handle(self.menu_search.read(cx), cx);
        let (faces, shown, head) = (faces.clone(), popup.clone(), head.to_owned());
        over_line(self.menu(
            popup.clone(),
            chip,
            move |d, _, cx| d.chip_menu(&faces, &shown, &id, &head, choices.clone(), cx),
            cx,
        ))
        .when(long, |p| p.track_focus(&search))
        .into_any_element()
    }

    /// The choices of `popup`, under `head`: narrowed by what is typed
    /// when there are many; the first nine numbered.
    fn chip_menu(
        &self,
        faces: &Faces,
        popup: &Popup,
        id: &str,
        head: &str,
        choices: Choices,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut menu = div()
            .flex()
            .flex_col()
            .w(px(248.))
            .child(ui::label(faces, head).px(px(8.)).pt(px(6.)).pb(px(4.)));
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        if choices.len() > 8 {
            menu = menu.child(
                div()
                    .mx(px(4.))
                    .mb(px(4.))
                    .child(Input::new(&self.menu_search).small()),
            );
        }
        let mut scroll = div()
            .id(SharedString::from(format!("{id}-list")))
            .flex()
            .flex_col()
            .max_h(px(360.));
        let mut shown = 0;
        for (n, (value, label, on)) in choices.into_iter().enumerate() {
            if !shown_by(&query, &value, &label) {
                continue;
            }
            shown += 1;
            // ⌘1 to ⌘9 choose among the first nine, while it is open.
            let numbered =
                (shown <= 9 && *popup != Popup::Agent).then(|| crate::keys::numbered(shown));
            let popup = popup.clone();
            let agent_mark = (popup == Popup::Agent).then(|| self.mark_of(&value));
            scroll = scroll.child(
                ui::menu_row(SharedString::from(format!("{id}-{n}")), on)
                    .children(agent_mark.map(|m| ui::mark(faces, &m, 16.)))
                    .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                    .when(on, |d| {
                        d.child(ui::icon("check", 13.).text_color(theme::muted()))
                    })
                    .children(numbered.map(Kbd::new))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        match &popup {
                            Popup::Mode => this.set_mode(value.clone(), cx),
                            Popup::Option(config) => {
                                this.set_option(config.clone(), value.clone(), cx);
                            }
                            Popup::Agent => this.switch_agent(&value, cx),
                            _ => {}
                        }
                        this.close_popup(window, cx);
                    })),
            );
        }
        menu = menu.child(scroll.overflow_y_scrollbar());
        // A new session can go to every agent at once, one each.
        let usable = self.usable_agents();
        if *popup == Popup::Agent && usable > 1 {
            let on = self.composer.fanout;
            menu = menu
                .child(div().h(px(1.)).mx(px(2.)).my(px(4.)).bg(theme::border2()))
                .child(
                    ui::menu_row("fanout", on)
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
                            this.composer.fanout = !this.composer.fanout;
                            this.popup = Popup::Closed;
                            cx.notify();
                        })),
                );
        }
        menu
    }

    /// The slider mark, and under it the options beyond the model and the
    /// mode, each with its value; one chosen opens its choices beside.
    fn more(&self, faces: &Faces, name: &str, more: Vec<More>, cx: &mut Context<Self>) -> Popover {
        let open_one = matches!(&self.popup, Popup::Option(id) if more.iter().any(|m| &m.id == id));
        let tip = more
            .iter()
            .map(|m| format!("{}: {}", m.name, m.current))
            .collect::<Vec<_>>()
            .join(" · ");
        let mark = ui::icon_button("chip-more", "sliders", tip)
            .size(px(22.))
            .text_color(theme::faint());
        let (faces, name) = (faces.clone(), name.to_owned());
        over_line(self.menu(
            Popup::More,
            mark,
            move |d, _, cx| d.more_menu(&faces, &name, more.clone(), cx),
            cx,
        ))
        // Open too while one of its options shows its choices.
        .open(self.popup == Popup::More || open_one)
    }

    /// The options behind the slider mark, each a line whose choices open
    /// beside it, in a Popover within the menu's.
    fn more_menu(&self, faces: &Faces, name: &str, more: Vec<More>, cx: &mut Context<Self>) -> Div {
        let mut list = div()
            .flex()
            .flex_col()
            .w(px(268.))
            .child(ui::label(faces, name).px(px(8.)).pt(px(6.)).pb(px(4.)));
        let desk = cx.entity();
        for (n, m) in more.into_iter().enumerate() {
            let line = ui::opener(SharedString::from(format!("more-{n}")))
                .w_full()
                .h(px(30.))
                .px(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::text(faces, m.name).flex_1().text_size(px(13.)))
                        .child(
                            ui::text(faces, ui::fit(&m.current, 18))
                                .text_size(px(12.5))
                                .text_color(theme::muted()),
                        )
                        .child(ui::icon("chevron-right", 11.).text_color(theme::faint())),
                );
            let shown = Popup::Option(m.id.clone());
            let (desk, drawn, faces) = (desk.clone(), desk.clone(), faces.clone());
            let (id, choices) = (m.id, m.choices);
            list = list.child(
                Popover::new(SharedString::from(format!("more-{n}-choices")))
                    .anchor(gpui::Anchor::LeftCenter)
                    .trigger_style(gpui::StyleRefinement::default().w_full())
                    .p(px(6.))
                    .open(self.popup == shown)
                    .trigger(line)
                    .on_open_change(move |open, _, cx| {
                        desk.update(cx, |d, cx| {
                            if *open {
                                d.popup = shown.clone();
                            } else if d.popup == shown {
                                d.popup = Popup::More;
                            }
                            cx.notify();
                        });
                    })
                    .content(move |_, _, cx| {
                        drawn.update(cx, |_, cx| {
                            Self::more_choices(&faces, &id, choices.clone(), cx)
                        })
                    }),
            );
        }
        list
    }

    /// The choices of the option `config`, beside its line.
    fn more_choices(faces: &Faces, config: &str, choices: Choices, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().w(px(188.));
        for (k, (value, label, chosen)) in choices.into_iter().enumerate() {
            let config = config.to_owned();
            list = list.child(
                ui::menu_row(SharedString::from(format!("more-{config}-{k}")), chosen)
                    .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                    .when(chosen, |d| {
                        d.child(ui::icon("check", 13.).text_color(theme::muted()))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_option(config.clone(), value.clone(), cx);
                    })),
            );
        }
        list
    }
}

/// How far above its mark a menu of the composer opens: clear of the line
/// you write in (the gap, one line, the padding, and 8 more).
const LIFT: f32 = 6. + 22. + 10. + 8.;

/// A menu of the composer, opened upward from its mark over the line you
/// write in.
fn over_line(menu: Popover) -> Popover {
    menu.anchor(gpui::Anchor::BottomLeft).offset(px(LIFT))
}

impl Desk {
    /// The mark that opens `popup`: its icon, and how many it holds when
    /// it holds any.
    fn popup_chip(faces: &Faces, icon: &'static str, count: usize, tip: &'static str) -> Button {
        ui::opener(SharedString::from(format!("chip-{icon}")))
            .h(px(26.))
            .px(px(7.))
            .text_color(theme::faint())
            .child(ui::icon(icon, 14.))
            .when(count > 0, |d| {
                d.child(ui::text(faces, count.to_string()).text_size(px(12.)))
            })
            .tooltip(tip)
    }

    /// What you put aside (⌘S): a mark with how many, and under it each,
    /// the latest first, to take back into the line or let go.
    fn stash_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Popover {
        let mark = Self::popup_chip(
            faces,
            "archive",
            self.composer.stashed.len(),
            "Put aside (⌘S), to take back",
        );
        let faces = faces.clone();
        over_line(self.menu(
            Popup::Stash,
            mark,
            move |d, _, cx| d.stashed_list(&faces, cx),
            cx,
        ))
    }

    /// What was put aside, the latest first: a click takes it back.
    fn stashed_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().w(px(348.)).child(
            ui::label(faces, "Put aside")
                .px(px(8.))
                .pt(px(6.))
                .pb(px(4.)),
        );
        for (n, text) in self.composer.stashed.iter().enumerate().rev() {
            list = list.child(
                ui::menu_row(SharedString::from(format!("stash-{n}")), false)
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
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if n < this.composer.stashed.len() {
                                this.composer.stashed.remove(n);
                                this.save_stash();
                            }
                            cx.notify();
                        })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.take_stashed(n, window, cx);
                    })),
            );
        }
        list
    }

    /// The clock: what is in the composer said later, once or each day,
    /// and what is waiting to be said, each can be let go.
    fn later_menu(
        &self,
        faces: &Faces,
        s: &crate::session::Session,
        cx: &mut Context<Self>,
    ) -> Popover {
        let mark = Self::popup_chip(faces, "clock", s.meta.later.len(), "Say it later");
        let (faces, key) = (faces.clone(), s.key);
        over_line(self.menu(
            Popup::Later,
            mark,
            move |d, _, cx| d.later_list(&faces, key, cx),
            cx,
        ))
    }

    /// When to say what is written, and what waits to be said.
    fn later_list(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        use crate::desk::Later;
        let written = !self.typed(cx).trim().is_empty();
        let mut list = div().flex().flex_col().w(px(308.)).child(
            ui::label(
                faces,
                if written {
                    "Say what you wrote"
                } else {
                    "Write it first, then say when"
                },
            )
            .px(px(8.))
            .pt(px(6.))
            .pb(px(4.)),
        );
        if written {
            for (n, (label, when)) in [
                ("In 30 minutes", Later::In(30)),
                ("In an hour", Later::In(60)),
                ("In 3 hours", Later::In(180)),
                ("Tomorrow at 9:00", Later::Tomorrow),
                ("Each day at 9:00", Later::Daily),
            ]
            .into_iter()
            .enumerate()
            {
                list = list.child(
                    ui::menu_row(SharedString::from(format!("later-{n}")), false)
                        .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.send_later(when, window, cx);
                        })),
                );
            }
        }
        let waiting = self
            .sessions
            .at(key)
            .map(|s| s.meta.later.as_slice())
            .unwrap_or_default();
        if !waiting.is_empty() {
            let now = std::time::SystemTime::now();
            list = list.child(
                ui::label(faces, "Waiting to be said")
                    .px(px(8.))
                    .pt(px(10.))
                    .pb(px(4.)),
            );
            for (n, l) in waiting.iter().enumerate() {
                let at = crate::store::time(l.at);
                let when = if l.daily {
                    format!("each day {}", crate::when::clock(at, self.settings.twelve))
                } else {
                    crate::when::said_at(at, now, self.settings.twelve)
                };
                list = list.child(
                    ui::menu_row(SharedString::from(format!("waiting-{n}")), false)
                        .child(
                            ui::text(faces, when)
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(theme::faint()),
                        )
                        .child(
                            ui::one_line(
                                ui::text(
                                    faces,
                                    l.text.lines().next().unwrap_or_default().to_owned(),
                                )
                                .text_size(px(13.)),
                            )
                            .flex_1()
                            .min_w(px(0.)),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("later-drop-{n}")),
                                "x",
                                "Let it go",
                            )
                            .size(px(22.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(s) = this.sessions.at_mut(key)
                                        && n < s.meta.later.len()
                                    {
                                        s.meta.later.remove(n);
                                    }
                                    this.save_index();
                                    cx.notify();
                                },
                            )),
                        ),
                );
            }
        }
        list
    }

    /// Take back what was put aside `n`th: into the line, after what is
    /// in it.
    fn take_stashed(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if n >= self.composer.stashed.len() {
            return;
        }
        let text = self.composer.stashed.remove(n);
        self.save_stash();
        self.popup = Popup::Closed;
        let typed = self.typed(cx);
        let line = if typed.is_empty() {
            text
        } else {
            format!("{typed}\n{text}")
        };
        self.put_line(&line, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }
}

/// An option kept behind the slider mark: its id, name, value now, and
/// what it can be.
#[derive(Clone)]
struct More {
    id: String,
    name: String,
    current: String,
    choices: Choices,
}

/// Whether a menu's choice stands when `query` (lower case) is typed.
fn shown_by(query: &str, value: &str, label: &str) -> bool {
    query.is_empty() || label.to_lowercase().contains(query) || value.to_lowercase().contains(query)
}

/// The session's modes, when it says them as modes: the one it is in, and
/// each to choose.
fn mode_choices(s: &crate::session::Session) -> Option<(String, Choices)> {
    let modes = s.modes.as_ref()?;
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
    Some((current, choices))
}

impl Desk {
    /// An option's value now and its choices, with the model ids of yours
    /// beside those the agent lists for its model.
    fn option_choices(
        &self,
        s: &crate::session::Session,
        option: &agent_client_protocol::v1::schema::SessionConfigOption,
    ) -> Option<(String, Choices)> {
        let (current, mut choices) = select_choices(&option.kind)?;
        if option.category == Some(SessionConfigOptionCategory::Model) {
            for m in self
                .settings
                .models
                .get(&s.meta.agent)
                .into_iter()
                .flatten()
            {
                if !choices.iter().any(|(v, _, _)| v == m) {
                    choices.push((m.clone(), m.clone(), false));
                }
            }
        }
        Some((current, choices))
    }

    /// Choose the `n`th of the choices shown in the open menu of an option
    /// or the modes; false when no such menu is open.
    pub(crate) fn choose_nth(&mut self, n: usize, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.session() else {
            return false;
        };
        let choices = match &self.popup {
            Popup::Mode => mode_choices(s).map(|c| c.1),
            Popup::Option(id) => s
                .options
                .iter()
                .find(|o| o.id.0.as_ref() == id)
                .and_then(|o| self.option_choices(s, o))
                .map(|c| c.1),
            _ => None,
        };
        let Some(choices) = choices else { return false };
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        let Some((value, _, _)) = choices
            .into_iter()
            .filter(|(v, l, _)| shown_by(&query, v, l))
            .nth(n)
        else {
            return true;
        };
        match self.popup.clone() {
            Popup::Mode => self.set_mode(value, cx),
            Popup::Option(config) => self.set_option(config, value, cx),
            _ => {}
        }
        true
    }
}
