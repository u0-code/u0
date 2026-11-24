//! The column down the left: the agents switched on, each with its logo,
//! where it stands, and its sessions under it; the agents off, quieter;
//! and the settings at the foot.

use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, LEFT, Page, TOPBAR, draggable},
    fonts::Faces,
    hub::{Slot, Stage},
    session::State,
    style::{FINE, META, NAME, S1, S2},
    theme,
    ui::{self, Mark},
};

/// A row's width for its words, less the mark and the padding.
const ROOM: f32 = LEFT - 84.;

impl Desk {
    pub(crate) fn sidebar(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
        let mut list = div()
            .id("agents")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(S2))
            .gap(px(2.))
            .child(section(faces, "Agents"));
        for slot in &self.slots {
            list = list.child(self.agent_row(faces, slot, cx));
            for s in self.sessions.iter().filter(|s| s.agent == slot.entry.id) {
                list = list.child(self.session_row(faces, s, cx));
            }
        }
        if self.slots.is_empty() {
            list = list.child(
                ui::text(faces, "No agent is switched on.")
                    .text_size(px(META))
                    .text_color(theme::faint())
                    .px(px(10.))
                    .py(px(S2)),
            );
        }

        let settings_on = self.page == Page::Settings;
        let foot = div()
            .flex()
            .flex_col()
            .px(px(S2))
            .py(px(S2))
            .border_t_1()
            .border_color(theme::border())
            .child(
                row("settings", settings_on)
                    .child(ui::icon("power", 15.).text_color(theme::muted()))
                    .child(
                        ui::text(faces, "Agents & settings")
                            .text_size(px(NAME))
                            .text_color(theme::text2()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_page(Page::Settings, cx))),
            );

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LEFT))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .child(
                draggable()
                    .flex()
                    .items_center()
                    .h(px(TOPBAR))
                    .pl(px(lights))
                    .pr(px(S2))
                    .child(
                        div()
                            .id("hide-sidebar")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(ui::icon("sidebar", 16.))
                            .tooltip(ui::tip("Hide the sidebar · ⌘B"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                    ),
            )
            .child(list)
            .child(foot)
    }

    fn agent_row(&self, faces: &Faces, slot: &Slot, cx: &mut Context<Self>) -> Div {
        let id = slot.entry.id.clone();
        let on = self.page == Page::Agent(id.clone());
        let (word, live) = match &slot.stage {
            Stage::Missing(_) => (Some("Install"), false),
            Stage::Installing(_) => (Some("Installing"), true),
            Stage::Starting => (Some("Starting"), true),
            Stage::Failed(_) => (Some("Stopped"), false),
            Stage::Ready => (None, false),
            Stage::Running { .. } => (
                None,
                self.sessions
                    .iter()
                    .any(|s| s.agent == id && s.state == State::Working),
            ),
        };
        let usable = matches!(
            slot.stage,
            Stage::Ready | Stage::Starting | Stage::Running { .. }
        );
        let (for_page, for_new) = (id.clone(), id.clone());
        let mark = ui::mark(faces, &Mark::of(&id, &slot.entry.name), 16.);
        let mark: AnyElement = if usable {
            mark
        } else {
            div().opacity(0.45).child(mark).into_any_element()
        };
        div().child(
            row(SharedString::from(format!("agent-{id}")), on)
                .h(px(32.))
                .child(mark)
                .child(ui::one_line(
                    ui::text(faces, ui::fit(&slot.entry.name, ui::cols(ROOM)))
                        .text_size(px(NAME))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if usable {
                            theme::text()
                        } else {
                            theme::muted()
                        }),
                ))
                .when(live, |d| d.child(ui::breathing(ui::dot(theme::accent()))))
                .children(word.map(|w| {
                    ui::text(faces, w)
                        .flex_none()
                        .text_size(px(FINE))
                        .text_color(theme::faint())
                }))
                .when(usable, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("new-{id}")))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(22.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(theme::border2()))
                            .child(ui::icon("plus", 13.))
                            .tooltip(ui::tip("New session"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.new_session(&for_new, window, cx);
                            })),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_page(Page::Agent(for_page.clone()), cx);
                })),
        )
    }

    fn session_row(
        &self,
        faces: &Faces,
        s: &crate::session::Session,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = s.key;
        let on = self.page == Page::Thread && self.current == Some(key);
        let title = if s.state == State::Draft && s.items.is_empty() {
            "New session".to_owned()
        } else {
            s.title.clone()
        };
        let mark: AnyElement = match &s.state {
            State::Working | State::Opening => {
                ui::breathing(ui::dot(theme::accent())).into_any_element()
            }
            State::Broken(_) => ui::dot(theme::coral()).into_any_element(),
            _ if s.waiting().is_some() => ui::dot(theme::accent()).into_any_element(),
            _ => div().size(px(6.)).into_any_element(),
        };
        div().child(
            row(SharedString::from(format!("session-{key}")), on)
                .h(px(28.))
                .pl(px(34.))
                .child(ui::one_line(
                    ui::text(faces, ui::fit(&title, ui::cols(ROOM - 10.)))
                        .text_size(px(META))
                        .text_color(if on { theme::text() } else { theme::text2() }),
                ))
                .child(mark)
                .on_click(cx.listener(move |this, _, window, cx| this.show(key, window, cx))),
        )
    }
}

/// A row of the sidebar: rounded, lit when it is what is open.
fn row(id: impl Into<SharedString>, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(10.))
        .h(px(30.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(on, |d| d.bg(theme::panel2()))
        .when(!on, |d| d.hover(|s| s.bg(theme::hover())))
}

/// The quiet head of a section.
fn section(faces: &Faces, text: &'static str) -> Div {
    ui::text(faces, text)
        .text_size(px(FINE))
        .text_color(theme::faint())
        .px(px(10.))
        .pt(px(S1))
        .pb(px(S1))
}
