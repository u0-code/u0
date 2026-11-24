//! The column down the left, as the design draws it: the window's lights
//! with search and a new session beside them; New session; the agents
//! switched on, each with its sessions under it; a few more agents, quieter;
//! and at the foot the settings and the folder new sessions start in.

use std::time::SystemTime;

use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, LEFT, Page, TOPBAR, draggable},
    fonts::Faces,
    hub::{Slot, Stage},
    session::{Session, State},
    theme,
    ui::{self, Mark},
    when,
};

/// The agents offered under "More agents" when they are not switched on.
const MORE: [&str; 4] = ["gemini", "opencode", "github-copilot-cli", "cursor"];

impl Desk {
    pub(crate) fn sidebar(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
        let head = draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(10.))
            .child(div().flex_1())
            .child(
                ui::icon_button("search", "search", "Search sessions")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
            )
            .child(
                ui::icon_button("new-icon", "pencil", "New session · ⌘N")
                    .on_click(cx.listener(|this, _, window, cx| this.new_default(window, cx))),
            );

        let new = div().px(px(8.)).pt(px(8.)).pb(px(12.)).child(
            div()
                .id("new-session")
                .flex()
                .items_center()
                .gap(px(10.))
                .w_full()
                .h(px(34.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::tag_line())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon("plus", 16.).text_color(theme::faint()))
                .child(
                    ui::text(faces, "New session")
                        .flex_1()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM),
                )
                .child(kbd(faces, "⌘N"))
                .on_click(cx.listener(|this, _, window, cx| this.new_default(window, cx))),
        );

        let query = self
            .searching
            .then(|| self.search.read(cx).text().to_lowercase());
        let mut agents = div().flex().flex_col().gap(px(2.)).px(px(8.));
        let mut shown = 0;
        for slot in &self.slots {
            let sessions: Vec<&Session> = self
                .sessions
                .iter()
                .rev()
                .filter(|s| s.agent == slot.entry.id)
                .filter(|s| {
                    query
                        .as_ref()
                        .is_none_or(|q| s.title.to_lowercase().contains(q))
                })
                .collect();
            if query.is_some() && sessions.is_empty() {
                continue;
            }
            agents = agents.child(self.agent_row(faces, slot, shown > 0, cx));
            shown += 1;
            if sessions.is_empty() {
                agents = agents.child(
                    session_shell(None, false).child(
                        ui::text(faces, "No sessions yet")
                            .text_size(px(13.))
                            .text_color(theme::faint()),
                    ),
                );
            }
            for s in sessions {
                agents = agents.child(self.session_row(faces, s, cx));
            }
        }

        let mut more = div().flex().flex_col().gap(px(2.)).px(px(8.));
        for id in MORE
            .iter()
            .filter(|id| !self.settings.enabled.iter().any(|e| e == *id))
        {
            let Some(entry) = self.entry(id) else {
                continue;
            };
            let word = if entry.launch(&self.home).is_ok() {
                "Off"
            } else {
                "Install"
            };
            let on = self.page == Page::Agent((*id).to_owned());
            let target = (*id).to_owned();
            more = more.child(
                agent_shell(SharedString::from(format!("more-{id}")), on)
                    .child(div().opacity(0.45).child(ui::mark(
                        faces,
                        &Mark::of(id, &entry.name),
                        16.,
                    )))
                    .child(
                        ui::one_line(
                            ui::text(faces, theme::agent_name(id, &entry.name).into_owned())
                                .text_size(px(13.5)),
                        )
                        .opacity(0.45),
                    )
                    .child(fine(faces, word))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_page(Page::Agent(target.clone()), cx);
                    })),
            );
        }
        more = more.child(
            agent_shell("add-agents", false)
                .child(ui::icon("plus", 16.).mx(px(2.)).text_color(theme::faint()))
                .child(
                    ui::text(faces, "Add from the ACP registry")
                        .flex_1()
                        .text_size(px(13.))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_page(Page::Settings, cx))),
        );

        let list = div()
            .id("agents")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .gap(px(14.))
            .children(self.searching.then(|| {
                div().px(px(8.)).child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .bg(theme::well())
                        .border_1()
                        .border_color(theme::border2())
                        .child(div().flex_1().child(self.search.clone())),
                )
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(ui::label(faces, "Agents").px(px(16.)))
                    .child(agents),
            )
            .when(query.is_none(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(ui::label(faces, "More agents").px(px(16.)))
                        .child(more),
                )
            });

        let folder = Desk::shown(&self.folder());
        let settings_on = self.page == Page::Settings;
        let foot = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(10.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                agent_shell("settings", settings_on)
                    .flex_1()
                    .child(ui::icon("settings", 16.).text_color(theme::faint()))
                    .child(ui::text(faces, "Settings").flex_1().text_size(px(13.)))
                    .on_click(cx.listener(|this, _, _, cx| this.open_page(Page::Settings, cx))),
            )
            .child(
                div()
                    .id("folder-chip")
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        ui::mono(faces, ui::fit(&folder, 18))
                            .text_size(px(11.5))
                            .text_color(theme::text2()),
                    )
                    .tooltip(ui::tip("The folder new sessions start in"))
                    .on_click(cx.listener(|_, _, _, cx| Self::choose_default_folder(cx))),
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
            .child(head)
            .child(new)
            .child(list)
            .child(foot)
    }

    fn agent_row(&self, faces: &Faces, slot: &Slot, gap: bool, cx: &mut Context<Self>) -> Div {
        let id = slot.entry.id.clone();
        let on = self.page == Page::Agent(id.clone());
        let usable = matches!(
            slot.stage,
            Stage::Ready | Stage::Starting | Stage::Running { .. }
        );
        let running = self
            .sessions
            .iter()
            .any(|s| s.agent == id && matches!(s.state, State::Working | State::Opening));
        let word = match &slot.stage {
            Stage::Missing(_) => Some(("Install", false)),
            Stage::Installing(_) => Some(("Installing", true)),
            Stage::Starting => Some(("Starting", true)),
            Stage::Failed(_) => Some(("Stopped", false)),
            _ => None,
        };
        let mark: AnyElement = if usable {
            ui::mark(faces, &Mark::of(&id, &slot.entry.name), 16.)
        } else {
            div()
                .opacity(0.45)
                .child(ui::mark(faces, &Mark::of(&id, &slot.entry.name), 16.))
                .into_any_element()
        };
        let target = id.clone();
        div().when(gap, |d| d.mt(px(6.))).child(
            agent_shell(SharedString::from(format!("agent-{id}")), on)
                .child(mark)
                .child(ui::one_line(
                    ui::text(faces, theme::agent_name(&id, &slot.entry.name).into_owned())
                        .text_size(px(13.5))
                        .text_color(if usable {
                            theme::text()
                        } else {
                            theme::muted()
                        }),
                ))
                .when(running, |d| d.child(ui::live_dot()))
                .children(
                    word.map(|(w, live)| {
                        fine(faces, w).when(live, |d| d.text_color(theme::accent()))
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_page(Page::Agent(target.clone()), cx);
                })),
        )
    }

    fn session_row(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let key = s.key;
        let on = self.page == Page::Thread && self.current == Some(key);
        let title = if s.state == State::Draft && s.items.is_empty() {
            "New session".to_owned()
        } else {
            s.title.clone()
        };
        let (when, live, trouble) = match &s.state {
            State::Working | State::Opening if s.waiting().is_some() => {
                ("waiting".to_owned(), true, false)
            }
            State::Working | State::Opening => ("running".to_owned(), true, false),
            State::SignIn => ("sign in".to_owned(), true, false),
            State::Broken(_) => ("stopped".to_owned(), false, true),
            State::Draft => (String::new(), false, false),
            State::Idle => (when::ago(s.updated, SystemTime::now()), false, false),
        };
        session_shell(Some(SharedString::from(format!("session-{key}"))), on)
            .cursor_pointer()
            .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
            .child(ui::one_line(
                ui::text(faces, ui::fit(&title, ui::cols(LEFT - 90.)))
                    .text_size(px(13.))
                    .text_color(if on { theme::text() } else { theme::text2() }),
            ))
            .child(fine(faces, when).text_color(if live {
                theme::accent()
            } else if trouble {
                theme::coral()
            } else {
                theme::faint()
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.show(key, window, cx)))
    }
}

/// A row for an agent or a control in the column: 32 high, corners of 8,
/// the selected one on the lifted ground.
fn agent_shell(id: impl Into<SharedString>, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(32.))
        .px(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(on, |d| d.bg(theme::panel2()))
        .when(!on, |d| d.hover(|s| s.bg(theme::hover())))
}

/// A row for a session under its agent: 28 high, in by 38.
fn session_shell(id: Option<SharedString>, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(
            id.unwrap_or_else(|| SharedString::from("no-sessions")),
        ))
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .pl(px(38.))
        .pr(px(8.))
        .rounded(px(8.))
        .when(on, |d| d.bg(theme::panel2()))
}

/// Small print at the right of a row.
fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// A key, in a hairline box.
pub(crate) fn kbd(faces: &Faces, text: &'static str) -> Div {
    ui::mono(faces, text)
        .flex_none()
        .px(px(4.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::tag_line())
        .text_size(px(10.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}
