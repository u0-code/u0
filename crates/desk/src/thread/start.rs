//! The window with no session on screen: an agent to begin with and the
//! project it begins in; the sessions that need you; and those to pick up
//! where you left off, in every project.

use std::time::SystemTime;

use gpui::{Context, Div, SharedString, Styled as _, div, prelude::*, px};

use crate::{
    desk::{Desk, Popup},
    fonts::{Faces, Weighted as _},
    session::{Session, State},
    sidebar::kbd,
    theme, ui, when,
};

/// How many sessions to pick up are listed.
const RECENT: usize = 5;

impl Desk {
    /// The start, `width` wide at most.
    pub(crate) fn start(&self, faces: &Faces, width: f32, cx: &mut Context<Self>) -> Div {
        let mut agents = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .justify_center()
            .mt(px(6.));
        for slot in &self.slots {
            let id = slot.entry.id.clone();
            let name = self.name_of(&id);
            agents = agents.child(
                ui::led_button(
                    SharedString::from(format!("start-{id}")),
                    Some(ui::mark(faces, &self.mark_of(&id), 16.)),
                    name,
                    ui::Weight::Secondary,
                )
                .h(px(34.))
                .px(px(12.))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(project) = this.project() {
                        this.new_session(&id, project, window, cx);
                    }
                })),
            );
        }
        agents = agents.child(
            ui::button("start-more", "+ More agents", ui::Weight::Quiet)
                .h(px(34.))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_tab(crate::pages::Tab::Agents, window, cx);
                })),
        );

        let (needs, recent) = self.to_pick_up();
        let mut page = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .w(px(width.min(600.)))
            .mt(px(96.))
            .child(
                ui::text(faces, "What should we work on?")
                    .text_size(px(22.))
                    .line_height(px(30.))
                    .weight(gpui::FontWeight::SEMIBOLD)
                    .flex()
                    .justify_center(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        ui::text(faces, "Pick an agent to start in")
                            .text_size(px(14.))
                            .text_color(theme::text2()),
                    )
                    .child(self.start_in(faces, cx)),
            )
            .child(agents);
        if !needs.is_empty() {
            page = page
                .child(head(faces, "Needs you", needs.len().to_string()).mt(px(34.)))
                .child(self.picks(faces, &needs, cx));
        }
        if !recent.is_empty() {
            page = page
                .child(
                    head(faces, "Pick up where you left off", "all projects".into())
                        .mt(px(if needs.is_empty() { 34. } else { 14. })),
                )
                .child(self.picks(faces, &recent, cx));
        }
        page.child(
            div()
                .flex()
                .justify_center()
                .gap(px(18.))
                .mt(px(18.))
                .children(
                    crate::keys::keys_of(cx, "palette").map(|k| key(faces, k, "go to anything")),
                )
                .children(crate::keys::keys_of(cx, "drawer").map(|k| key(faces, k, "terminal")))
                .child(key(faces, "⌘1–9", "a session")),
        )
    }

    /// The sessions that wait on you, and the others most lately touched.
    fn to_pick_up(&self) -> (Vec<&Session>, Vec<&Session>) {
        let shown: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                self.listed_now(s)
                    && s.meta.flags.settled.is_none()
                    && !(s.state == State::Draft && s.items.is_empty())
            })
            .collect();
        let (needs, mut rest): (Vec<&Session>, Vec<&Session>) =
            shown.into_iter().partition(|s| asks(s).is_some());
        rest.sort_by_key(|s| std::cmp::Reverse(s.meta.updated));
        rest.truncate(RECENT);
        (needs, rest)
    }

    /// The project a session begun here starts in, and the others to
    /// choose from.
    fn start_in(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let here = self.chosen_project();
        let named = here
            .as_ref()
            .map_or_else(|| "a scratch folder".into(), |p| self.project_name(p));
        let mut chip = div().relative().child(
            div()
                .id("start-in")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px(px(8.))
                .rounded(px(7.))
                .border_1()
                .border_color(theme::tag_line())
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .child(ui::icon("folder", 13.).text_color(theme::faint()))
                .child(
                    ui::mono(faces, ui::fit(&named, 28))
                        .text_size(px(12.))
                        .text_color(theme::text2()),
                )
                .child(ui::icon("chevron-down", 11.).text_color(theme::faint()))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_popup(Popup::StartIn, window, cx);
                })),
        );
        if self.popup == Popup::StartIn {
            let mut menu = div().flex().flex_col().w(px(240.)).p(px(6.));
            for (n, p) in self.projects().into_iter().enumerate() {
                let on = here.as_ref() == Some(&p);
                menu = menu.child(
                    ui::menu_row(SharedString::from(format!("start-in-{n}")), on)
                        .child(ui::icon("folder", 13.).text_color(theme::faint()))
                        .child(
                            ui::text(faces, ui::fit(&self.project_name(&p), 28)).text_size(px(13.)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_in = Some(p.clone());
                            this.popup = Popup::Closed;
                            cx.notify();
                        })),
                );
            }
            // Or a folder not listed yet.
            menu = menu.child(
                ui::menu_row("start-in-open", false)
                    .child(ui::icon("plus", 13.).text_color(theme::faint()))
                    .child(ui::text(faces, "Open a folder…").text_size(px(13.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.popup = Popup::Closed;
                        Self::add_project(cx);
                    })),
            );
            chip = chip.child(gpui::deferred(
                gpui::anchored().child(
                    div()
                        .mt(px(30.))
                        .child(ui::menu("start-in-menu").child(menu)),
                ),
            ));
        }
        chip
    }

    /// Sessions as rows of a card: the agent, the title, the project, what
    /// changed or what it asks, and when.
    fn picks(&self, faces: &Faces, sessions: &[&Session], cx: &mut Context<Self>) -> Div {
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(theme::border())
            .overflow_hidden();
        let now = SystemTime::now();
        for (n, s) in sessions.iter().enumerate() {
            let key = s.key;
            let tail = match asks(s) {
                Some(what) => fine(faces, what).text_color(theme::accent()),
                None if matches!(s.state, State::Working | State::Opening) => {
                    fine(faces, "running").text_color(theme::accent())
                }
                None => fine(faces, when::ago(s.meta.updated, now)),
            };
            let (added, gone) = s.total.as_ref().map_or((0, 0), |files| {
                files
                    .iter()
                    .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone))
            });
            card = card.child(
                div()
                    .id(SharedString::from(format!("pick-{key}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(40.))
                    .px(px(12.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 14.))
                    .child(
                        ui::one_line(
                            ui::text(faces, ui::fit(&s.meta.title, 52))
                                .text_size(px(13.5))
                                .text_color(theme::text()),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(fine(
                        faces,
                        ui::fit(&self.project_name(&s.meta.project), 16),
                    ))
                    .when(added + gone > 0, |d| {
                        d.child(
                            ui::mono(faces, format!("+{added}"))
                                .text_size(px(12.))
                                .text_color(theme::added()),
                        )
                        .child(
                            ui::mono(faces, format!("−{gone}"))
                                .text_size(px(12.))
                                .text_color(theme::gone()),
                        )
                    })
                    .child(tail)
                    .on_click(cx.listener(move |this, _, window, cx| this.show(key, window, cx))),
            );
        }
        card
    }
}

/// What a session waits on you for, if it does.
fn asks(s: &Session) -> Option<&'static str> {
    if s.waiting().is_some() {
        Some("asks you")
    } else {
        match s.state {
            State::SignIn => Some("sign in"),
            State::Broken(_) => Some("stopped"),
            _ => None,
        }
    }
}

fn head(faces: &Faces, label: &str, note: String) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mx(px(2.))
        .child(ui::label(faces, label).flex_1())
        .child(fine(faces, note))
}

fn key(faces: &Faces, keys: impl Into<SharedString>, does: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(kbd(faces, keys))
        .child(fine(faces, does))
}

fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(12.))
        .text_color(theme::faint())
}
