//! The middle: the top bar, the session's thread, and the line you type
//! into.
//!
//! The thread is drawn the way a coding agent's own app draws it: what you
//! said in a bubble, what the agent said in prose, each tool call as a row
//! (its kind as a verb, what it was about, how it ended) that opens to
//! what it did, a diff as a diff and output as output; a question the
//! agent asks as a card with its choices. Every agent is drawn the same
//! way, from the same ACP updates.

pub mod drawn;
pub mod kinds;
pub mod output;

use agent_client_protocol_schema::v1::{
    PermissionOptionKind, SessionConfigOptionCategory, StopReason, ToolCall, ToolCallContent,
    ToolCallStatus, ToolKind,
};
use gpui::{AnyElement, Context, Div, SharedString, Window, div, prelude::*, px};

use crate::{
    desk::{Desk, Popup, TOPBAR, draggable},
    fonts::Faces,
    markdown,
    session::{Asked, Item, Session, State, text_of},
    style::{
        BODY, BUBBLE_RADIUS, CARD_RADIUS, DISPLAY, META, MONO, NAME, S1, S2, S3, S4, SMALL, TURN,
        boxed, meta, row,
    },
    theme,
    ui::{self, Mark, Weight},
};

/// How a tool call's kind reads as a verb.
fn verb(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "Read",
        ToolKind::Edit => "Edited",
        ToolKind::Delete => "Deleted",
        ToolKind::Move => "Moved",
        ToolKind::Search => "Searched",
        ToolKind::Execute => "Ran",
        ToolKind::Think => "Thought",
        ToolKind::Fetch => "Fetched",
        ToolKind::SwitchMode => "Switched mode",
        _ => "Used",
    }
}

/// How long a turn has gone on, as a person says it.
fn took(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

impl Desk {
    pub(crate) fn topbar(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
        let left = if self.panes.left { 16. } else { lights };
        let mut title = div().flex().items_center().gap(px(S2)).min_w(px(0.));
        if !self.panes.left {
            title = title.child(
                div()
                    .id("show-sidebar")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::icon("sidebar", 16.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            );
        }
        if self.page == crate::desk::Page::Thread
            && let Some(s) = self.session()
        {
            let name = self
                .slot(&s.agent)
                .map_or(s.agent.as_str(), |x| x.entry.name.as_str());
            title = title
                .child(ui::mark(faces, &Mark::of(&s.agent, name), 16.))
                .child(ui::one_line(
                    ui::text(faces, ui::fit(&s.title, 60))
                        .text_size(px(NAME))
                        .font_weight(gpui::FontWeight::MEDIUM),
                ))
                .child(
                    ui::text(faces, format!("{name} · {}", Desk::shown(&s.folder)))
                        .flex_none()
                        .text_size(px(META))
                        .text_color(theme::faint()),
                );
        }
        draggable()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(TOPBAR))
            .pl(px(left))
            .pr(px(S3))
            .border_b_1()
            .border_color(theme::border())
            .child(title)
            .child(
                div()
                    .id("toggle-aside")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::icon("panel-right", 16.))
                    .tooltip(ui::tip("The session's side column"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_aside(cx))),
            )
    }

    pub(crate) fn thread(
        &self,
        faces: &Faces,
        column: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut col = div()
            .flex()
            .flex_col()
            .w(px(column))
            .pt(px(TURN))
            .pb(px(TURN));
        match self.session() {
            None => col = col.child(self.empty(faces, cx)),
            Some(s) if s.items.is_empty() && s.state == State::Draft => {
                col = col.child(self.fresh(faces, s, cx));
            }
            Some(s) => {
                let mut before: Option<&Item> = None;
                for (i, item) in s.items.iter().enumerate() {
                    let gap = match (before, item) {
                        (None, _) => 0.,
                        (_, Item::You(_)) => TURN,
                        (Some(Item::Tool(_)), Item::Tool(_)) => 0.,
                        _ => S3,
                    };
                    col = col.child(div().mt(px(gap)).child(self.item(faces, s, i, item, cx)));
                    before = Some(item);
                }
                col = col.children(live(faces, s));
                if s.state == State::SignIn {
                    col = col.child(self.sign_in_card(faces, s, cx));
                }
            }
        }
        div()
            .id("thread")
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.thread_scroll)
            .child(col)
    }

    /// The thread with nothing on it: the agents to begin with.
    fn empty(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut agents = div().flex().flex_wrap().gap(px(S2)).justify_center();
        for slot in &self.slots {
            let id = slot.entry.id.clone();
            agents = agents.child(
                div()
                    .id(SharedString::from(format!("start-{id}")))
                    .flex()
                    .items_center()
                    .gap(px(S2))
                    .h(px(36.))
                    .px(px(14.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::border2())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::mark(faces, &Mark::of(&id, &slot.entry.name), 16.))
                    .child(ui::text(faces, slot.entry.name.clone()).text_size(px(NAME)))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.new_session(&id, window, cx)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(S4))
            .mt(px(120.))
            .child(ui::serif(faces, "What should we work on?").text_size(px(DISPLAY)))
            .child(
                ui::text(faces, "Pick an agent. Each works with its own tools and its own sign-in; Ghost shows them all the same way.")
                    .text_size(px(SMALL))
                    .text_color(theme::muted()),
            )
            .child(agents)
    }

    /// A session begun, before anything is said: where it will work.
    fn fresh(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let name = self
            .slot(&s.agent)
            .map_or(s.agent.clone(), |x| x.entry.name.clone());
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(S4))
            .mt(px(120.))
            .child(ui::mark(faces, &Mark::of(&s.agent, &name), 32.))
            .child(ui::serif(faces, format!("New session with {name}")).text_size(px(DISPLAY)))
            .child(
                div()
                    .id("folder")
                    .flex()
                    .items_center()
                    .gap(px(S2))
                    .h(px(30.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme::border2())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::icon("folder", 14.))
                    .child(ui::mono(faces, Desk::shown(&s.folder)).text_color(theme::text2()))
                    .child(ui::icon("chevron-down", 11.).text_color(theme::faint()))
                    .tooltip(ui::tip("The folder it works in"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
            )
    }

    fn item(
        &self,
        faces: &Faces,
        s: &Session,
        i: usize,
        item: &Item,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("{}-{i}", s.key);
        match item {
            Item::You(text) => div()
                .flex()
                .justify_end()
                .child(
                    ui::text(faces, text.clone())
                        .max_w(px(520.))
                        .px(px(16.))
                        .py(px(10.))
                        .rounded(px(BUBBLE_RADIUS))
                        .bg(theme::bubble())
                        .line_height(px(21.)),
                )
                .into_any_element(),
            Item::Agent(text) => {
                markdown::render(faces, &key, text, &self.expanded, cx).into_any_element()
            }
            Item::Thought(text) => {
                let open = self.expanded.contains(&key);
                let toggle = key.clone();
                div()
                    .flex()
                    .flex_col()
                    .child(
                        row()
                            .id(SharedString::from(key.clone()))
                            .cursor_pointer()
                            .gap(px(S2))
                            .rounded(px(8.))
                            .hover(|s| s.bg(theme::hover()))
                            .child(ui::text(faces, "Thought").text_color(theme::muted()))
                            .child(ui::icon(
                                if open {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                11.,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
                    )
                    .when(open, |d| {
                        d.child(div().pl(px(10.)).child(markdown::render_as(
                            faces,
                            &key,
                            text,
                            markdown::Setting::Small,
                            &self.expanded,
                            cx,
                        )))
                    })
                    .into_any_element()
            }
            Item::Tool(call) => self.tool(faces, &key, call, cx).into_any_element(),
            Item::Asked(asked) => Self::asked(faces, &key, asked, cx).into_any_element(),
            Item::Note { text, trouble } => ui::text(faces, text.clone())
                .text_size(px(SMALL))
                .text_color(if *trouble {
                    theme::coral()
                } else {
                    theme::muted()
                })
                .into_any_element(),
            Item::Ended(reason) => {
                let word = match reason {
                    StopReason::Cancelled => "Stopped",
                    StopReason::MaxTokens => "Stopped: the answer reached its length limit",
                    StopReason::MaxTurnRequests => {
                        "Stopped: the turn reached its limit of requests"
                    }
                    StopReason::Refusal => "The agent refused to go on",
                    _ => "Ended",
                };
                row()
                    .gap(px(S2))
                    .child(div().size(px(8.)).rounded(px(1.5)).bg(theme::faint()))
                    .child(meta(faces, word))
                    .into_any_element()
            }
        }
    }

    /// A tool call: a row, and on a click what it did.
    fn tool(&self, faces: &Faces, key: &str, call: &ToolCall, cx: &mut Context<Self>) -> Div {
        let open = self.expanded.contains(key);
        let has_more = !call.content.is_empty();
        let toggle = key.to_owned();
        let (adds, gone) = call.content.iter().fold((0, 0), |(a, g), c| match c {
            ToolCallContent::Diff(d) => {
                let (gone, added) = kinds::counted(&kinds::diff(
                    d.old_text.as_deref().unwrap_or(""),
                    &d.new_text,
                ));
                (a + added, g + gone)
            }
            _ => (a, g),
        });
        let status: AnyElement = match call.status {
            ToolCallStatus::Pending | ToolCallStatus::InProgress => {
                ui::breathing(ui::dot(theme::accent())).into_any_element()
            }
            ToolCallStatus::Failed => ui::icon("x", 12.)
                .text_color(theme::coral())
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let mut line = row()
            .id(SharedString::from(format!("{key}-row")))
            .gap(px(S2))
            .rounded(px(8.))
            .child(
                ui::text(faces, verb(call.kind))
                    .flex_none()
                    .text_color(theme::muted()),
            )
            .child(ui::one_line(
                div()
                    .font_family(faces.mono.clone())
                    .text_size(px(MONO))
                    .text_color(theme::text())
                    .child(ui::fit(&call.title, 80)),
            ))
            .when(adds + gone > 0, |d| {
                d.child(
                    ui::mono(faces, format!("+{adds}"))
                        .flex_none()
                        .text_color(theme::code_argument()),
                )
                .child(
                    ui::mono(faces, format!("−{gone}"))
                        .flex_none()
                        .text_color(theme::coral()),
                )
            })
            .child(status);
        if has_more {
            line = line
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    ui::icon(
                        if open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        11.,
                    )
                    .flex_none(),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
        }
        let mut d = div().flex().flex_col().child(line);
        if open {
            let mut body = boxed().gap(px(S2)).mt(px(S1)).mb(px(S2));
            for (j, c) in call.content.iter().enumerate() {
                body = body.child(match c {
                    ToolCallContent::Diff(diff) => div()
                        .flex()
                        .flex_col()
                        .gap(px(S1))
                        .child(meta(faces, Desk::shown(&diff.path)))
                        .child(drawn::diff(
                            faces,
                            &kinds::diff(diff.old_text.as_deref().unwrap_or(""), &diff.new_text),
                        ))
                        .into_any_element(),
                    ToolCallContent::Content(content) => output::printed(
                        faces,
                        &format!("{key}-{j}"),
                        &text_of(&content.content),
                        self.expanded.contains(&format!("{key}-all")),
                    )
                    .into_any_element(),
                    _ => div().into_any_element(),
                });
            }
            d = d.child(body);
        }
        d
    }

    /// A question the agent asks: what it wants to do, and the choices it
    /// gives; once answered, what was chosen, quietly.
    fn asked(faces: &Faces, key: &str, asked: &Asked, cx: &mut Context<Self>) -> Div {
        if let Some(chosen) = &asked.chosen {
            return row().gap(px(S2)).child(ui::icon("check", 13.)).child(meta(
                faces,
                format!("{chosen} · {}", ui::fit(&asked.title, 60)),
            ));
        }
        let mut choices = div().flex().flex_wrap().gap(px(S2));
        for (n, option) in asked.options.iter().enumerate() {
            let weight = match option.kind {
                PermissionOptionKind::AllowOnce => Weight::Primary,
                PermissionOptionKind::AllowAlways => Weight::Secondary,
                _ => Weight::Quiet,
            };
            let (request, id, name) = (
                asked.id.clone(),
                option.option_id.0.to_string(),
                option.name.clone(),
            );
            choices = choices.child(
                ui::button(
                    faces,
                    SharedString::from(format!("{key}-opt-{n}")),
                    option.name.clone(),
                    weight,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.choose(&request, id.clone(), &name, cx);
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(S3))
            .p(px(S4))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(theme::accent().opacity(0.45))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(S2))
                    .child(ui::dot(theme::accent()))
                    .child(
                        ui::text(faces, "Waiting for you")
                            .text_size(px(META))
                            .text_color(theme::accent()),
                    ),
            )
            .child(
                div()
                    .font_family(faces.mono.clone())
                    .text_size(px(MONO))
                    .text_color(theme::text())
                    .child(asked.title.clone()),
            )
            .child(choices)
    }

    /// The agent asks to be signed in before it opens the session: the
    /// ways it offers, each a button, and a way to try again once a login
    /// done elsewhere is finished.
    fn sign_in_card(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        use agent_client_protocol_schema::v1::AuthMethod;
        let name = self
            .slot(&s.agent)
            .map_or(s.agent.clone(), |x| x.entry.name.clone());
        let methods: Vec<(String, String, Option<String>)> =
            match self.slot(&s.agent).map(|x| &x.stage) {
                Some(crate::hub::Stage::Running { init, .. }) => init
                    .auth_methods
                    .iter()
                    .filter_map(|m| match m {
                        AuthMethod::Agent(a) => {
                            Some((a.id.0.to_string(), a.name.clone(), a.description.clone()))
                        }
                        AuthMethod::Terminal(t) => {
                            Some((t.id.0.to_string(), t.name.clone(), t.description.clone()))
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
        let busy = self.signing.as_deref() == Some(s.agent.as_str());
        let mut list = div().flex().flex_col().gap(px(S2));
        for (n, (id, label, about)) in methods.into_iter().enumerate() {
            let agent = s.agent.clone();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(S3))
                    .child(
                        ui::button(
                            faces,
                            SharedString::from(format!("auth-{n}")),
                            label,
                            if n == 0 {
                                Weight::Primary
                            } else {
                                Weight::Secondary
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.sign_in(&agent, &id, cx))),
                    )
                    .children(about.map(|a| meta(faces, a))),
            );
        }
        let agent = s.agent.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(S3))
            .mt(px(S3))
            .p(px(S4))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(theme::accent().opacity(0.45))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(S2))
                    .child(ui::mark(faces, &Mark::of(&s.agent, &name), 16.))
                    .child(ui::text(faces, format!("Sign in to {name}")).font_weight(gpui::FontWeight::MEDIUM)),
            )
            .child(
                ui::text(faces, "It signs in with its own account; Ghost never sees it. What you said is sent once you are in.")
                    .text_size(px(SMALL))
                    .text_color(theme::muted()),
            )
            .child(if busy {
                row()
                    .gap(px(S2))
                    .child(ui::breathing(ui::dot(theme::accent())))
                    .child(ui::text(faces, "Waiting for the sign-in to finish").text_size(px(SMALL)))
            } else {
                list
            })
            .child(
                ui::button(faces, "auth-retry", "I have signed in, continue", Weight::Quiet)
                    .on_click(cx.listener(move |this, _, _, cx| this.retry_open(&agent, cx))),
            )
    }

    /// The line you type into, with the session's mode and options under
    /// your hand.
    pub(crate) fn composer(
        &self,
        faces: &Faces,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(s) = self.session() else {
            return div();
        };
        let focused = gpui::Focusable::focus_handle(self.input.read(cx), cx).is_focused(window);
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.input.read(cx).is_empty();
        let mut chips = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap(px(2.))
            .ml(px(-4.));
        let name = self
            .slot(&s.agent)
            .map_or(s.agent.clone(), |x| x.entry.name.clone());
        chips = chips.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px(px(8.))
                .child(ui::mark(faces, &Mark::of(&s.agent, &name), 14.))
                .child(
                    ui::text(faces, name)
                        .text_size(px(META))
                        .text_color(theme::muted()),
                ),
        );
        // An agent that offers its mode among its options says it there;
        // ACP has clients prefer the options to the older list of modes.
        let mode_option = s
            .options
            .iter()
            .any(|o| o.category == Some(SessionConfigOptionCategory::Mode));
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
            chips = chips.child(self.chip(faces, &Popup::Mode, &current, "Mode", choices, cx));
        }
        // Under your hand: the mode, the model and how hard it thinks. The
        // rest of what an agent offers is for its settings, not each turn.
        for option in s.options.iter().filter(|o| {
            matches!(
                o.category,
                Some(
                    SessionConfigOptionCategory::Mode
                        | SessionConfigOptionCategory::Model
                        | SessionConfigOptionCategory::ThoughtLevel
                )
            )
        }) {
            use agent_client_protocol_schema::v1::{SessionConfigKind, SessionConfigSelectOptions};
            let SessionConfigKind::Select(select) = &option.kind else {
                continue;
            };
            let all: Vec<_> = match &select.options {
                SessionConfigSelectOptions::Ungrouped(o) => o.iter().collect(),
                SessionConfigSelectOptions::Grouped(g) => {
                    g.iter().flat_map(|g| g.options.iter()).collect()
                }
                _ => continue,
            };
            let current = all
                .iter()
                .find(|o| o.value == select.current_value)
                .map_or_else(|| select.current_value.0.to_string(), |o| o.name.clone());
            let choices = all
                .iter()
                .map(|o| {
                    (
                        o.value.0.to_string(),
                        o.name.clone(),
                        o.value == select.current_value,
                    )
                })
                .collect();
            chips = chips.child(self.chip(
                faces,
                &Popup::Option(option.id.0.to_string()),
                &current,
                &option.name,
                choices,
                cx,
            ));
        }

        let button = div()
            .size(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .border_1();
        let button =
            if working && empty {
                button
                    .id("stop")
                    .cursor_pointer()
                    .bg(theme::border2())
                    .border_color(theme::ghost_line())
                    .hover(|s| s.bg(theme::ghost_line()))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(theme::text()))
                    .tooltip(ui::tip("Stop · Esc"))
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
            } else {
                let input = self.input.clone();
                button
                    .id("send")
                    .when(empty || working, |d| d.border_color(theme::border2()))
                    .when(!empty && !working, |d| {
                        d.cursor_pointer()
                            .bg(theme::border2())
                            .border_color(theme::ghost_line())
                            .hover(|s| s.bg(theme::ghost_line()))
                    })
                    .child(ui::mono(faces, "↑").text_size(px(BODY)).text_color(
                        if empty || working {
                            theme::faint()
                        } else {
                            theme::text()
                        },
                    ))
                    .tooltip(ui::tip("Send · Return"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        input.update(cx, crate::input::TextInput::send);
                    }))
            };

        let broken = match &s.state {
            State::Broken(why) => Some(why.clone()),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .flex_none()
            .px(px(S4))
            .pb(px(S4))
            .children(broken.map(|why| {
                ui::text(faces, why)
                    .w_full()
                    .max_w(px(crate::style::COLUMN))
                    .pb(px(S2))
                    .text_size(px(SMALL))
                    .text_color(theme::coral())
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .w_full()
                    .max_w(px(crate::style::COLUMN))
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(8.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(if focused {
                        theme::ghost_line()
                    } else {
                        theme::border2()
                    })
                    .rounded(px(16.))
                    .child(div().px(px(4.)).child(self.input.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(chips)
                            .child(button),
                    ),
            )
    }

    /// A chip that opens a menu of choices: `(id, name, current)` each.
    fn chip(
        &self,
        faces: &Faces,
        popup: &Popup,
        current: &str,
        head: &str,
        choices: Vec<(String, String, bool)>,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = &self.popup == popup;
        let id = match popup {
            Popup::Mode => "chip-mode".to_owned(),
            Popup::Option(o) => format!("chip-{o}"),
            Popup::Closed => "chip".to_owned(),
        };
        let for_click = popup.clone();
        let mut chip = div().relative().flex().min_w(px(0.)).child(
            div()
                .id(SharedString::from(id.clone()))
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(26.))
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(open, |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .child(ui::one_line(
                    ui::text(faces, ui::fit(current, 22))
                        .text_size(px(META))
                        .text_color(theme::muted()),
                ))
                .child(
                    ui::icon("chevron-down", 11.)
                        .flex_none()
                        .text_color(theme::faint()),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_popup(for_click.clone(), cx)),
                ),
        );
        if open {
            let mut menu = div()
                .flex()
                .flex_col()
                .w(px(240.))
                .py(px(4.))
                .child(ui::menu_head(faces, head.to_owned()));
            for (n, (value, name, on)) in choices.into_iter().enumerate() {
                let popup = popup.clone();
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("{id}-{n}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .mx(px(4.))
                        .px(px(8.))
                        .h(px(28.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(ui::text(faces, name).text_size(px(SMALL)))
                        .when(on, |d| d.child(ui::icon("check", 13.)))
                        .on_click(cx.listener(move |this, _, _, cx| match &popup {
                            Popup::Mode => this.set_mode(value.clone(), cx),
                            Popup::Option(config) => {
                                this.set_option(config.clone(), value.clone(), cx);
                            }
                            Popup::Closed => {}
                        })),
                );
            }
            chip = chip.child(
                div()
                    .absolute()
                    .bottom(px(32.))
                    .left(px(0.))
                    .child(ui::menu(SharedString::from(format!("{id}-menu"))).child(menu)),
            );
        }
        chip
    }
}

/// What goes on now, at the foot of the thread: the agent starting, or
/// working, and for how long.
fn live(faces: &Faces, s: &Session) -> Option<Div> {
    let word = match s.state {
        State::Opening => "Starting the agent",
        State::Working if s.waiting().is_some() => return None,
        State::Working => "Working",
        _ => return None,
    };
    let secs = s.since.map_or(0, |t| t.elapsed().as_secs());
    Some(
        row()
            .mt(px(S3))
            .gap(px(S2))
            .child(ui::breathing(ui::dot(theme::accent())))
            .child(
                ui::text(faces, word)
                    .text_size(px(SMALL))
                    .text_color(theme::text2()),
            )
            .child(meta(faces, took(secs))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_tool_call_reads_as_a_verb() {
        assert_eq!(verb(ToolKind::Execute), "Ran");
        assert_eq!(verb(ToolKind::Edit), "Edited");
        assert_eq!(verb(ToolKind::Other), "Used");
        assert_eq!(took(75), "1m 15s");
    }
}
