//! The column down the left, as the design draws it: the window's lights
//! with search and a new session beside them; New session; the projects,
//! each a folder with its sessions under it, whichever agent each is with;
//! the agents switched on; a few more agents, quieter; and at the foot the
//! settings.

use std::{path::Path, time::SystemTime};

use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, LEFT, Page, TOPBAR, draggable},
    fonts::Faces,
    hub::{Slot, Stage},
    session::{Session, State},
    theme,
    ui::{self, Mark},
    when,
};

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
                        .weight(gpui::FontWeight::MEDIUM),
                )
                .child(kbd(faces, "⌘N"))
                .on_click(cx.listener(|this, _, window, cx| this.new_default(window, cx))),
        );

        let query = self
            .searching
            .then(|| self.search.read(cx).text().to_lowercase());
        let mut projects = div().flex().flex_col().gap(px(2.)).px(px(8.));
        for folder in self.projects() {
            let mut sessions: Vec<&Session> = self
                .sessions
                .iter()
                .filter(|s| s.project == folder)
                .filter(|s| {
                    query
                        .as_ref()
                        .is_none_or(|q| s.title.to_lowercase().contains(q))
                })
                .collect();
            if query.is_some() && sessions.is_empty() {
                continue;
            }
            // The latest first; one not begun yet on top.
            sessions
                .sort_by_key(|s| std::cmp::Reverse((s.state == State::Draft, s.updated, s.key)));
            let open = query.is_some() || !self.closed.contains(&folder);
            projects = projects.child(Self::project_row(faces, &folder, sessions.len(), open, cx));
            if !open {
                continue;
            }
            if sessions.is_empty() {
                projects = projects.child(
                    session_shell(
                        SharedString::from(format!("none-{}", folder.display())),
                        false,
                    )
                    .child(
                        ui::text(faces, "No sessions yet")
                            .text_size(px(13.))
                            .text_color(theme::faint()),
                    ),
                );
            }
            for s in sessions {
                projects = projects.child(self.session_row(faces, s, cx));
            }
        }
        if self.projects().is_empty() {
            projects = projects.child(
                agent_shell("first-project", false)
                    .child(ui::icon("folder", 16.).text_color(theme::faint()))
                    .child(
                        ui::text(faces, "Add a project")
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(theme::muted()),
                    )
                    .on_click(cx.listener(|_, _, _, cx| Self::add_project(cx))),
            );
        }

        let mut agents = div().flex().flex_col().gap(px(2.)).px(px(8.));
        for slot in &self.slots {
            agents = agents.child(self.agent_row(faces, slot, cx));
        }

        // Agents switched off that are on this machine still, then the
        // rest of the registry, one click away.
        let mut more = div().flex().flex_col().gap(px(2.)).px(px(8.));
        let off: Vec<&crate::registry::Entry> = self
            .registry
            .agents
            .iter()
            .filter(|e| !self.settings.enabled.contains(&e.id))
            .collect();
        for entry in off.iter().filter(|e| e.downloaded(&self.home)) {
            let id = entry.id.clone();
            let on = self.page == Page::Agent(id.clone());
            more = more.child(
                agent_shell(SharedString::from(format!("more-{id}")), on)
                    .child(div().opacity(0.45).child(ui::mark(
                        faces,
                        &Mark::of(&id, &entry.name),
                        16.,
                    )))
                    .child(
                        ui::one_line(
                            ui::text(faces, theme::agent_name(&id, &entry.name).into_owned())
                                .text_size(px(13.5)),
                        )
                        .opacity(0.45),
                    )
                    .child(fine(faces, "Off"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_page(Page::Agent(id.clone()), cx);
                    })),
            );
        }
        let rest = off.iter().filter(|e| !e.downloaded(&self.home)).count();
        more = more.child(
            agent_shell("add-agents", false)
                .child(ui::icon("plus", 16.).mx(px(2.)).text_color(theme::faint()))
                .child(
                    ui::text(faces, "Add from the ACP registry")
                        .flex_1()
                        .text_size(px(13.))
                        .text_color(theme::muted()),
                )
                .when(rest > 0, |d| d.child(fine(faces, rest.to_string())))
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .pl(px(16.))
                            .pr(px(10.))
                            .child(ui::label(faces, "Projects").flex_1())
                            .child(
                                ui::icon_button("add-project", "plus", "Add project")
                                    .size(px(20.))
                                    .text_color(theme::faint())
                                    .on_click(cx.listener(|_, _, _, cx| Self::add_project(cx))),
                            ),
                    )
                    .child(projects),
            )
            .when(query.is_none(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(ui::label(faces, "Agents").px(px(16.)))
                        .child(agents),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(ui::label(faces, "More agents").px(px(16.)))
                        .child(more),
                )
            });

        let settings_on = self.page == Page::Settings;
        let foot = div()
            .flex()
            .flex_none()
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

    fn agent_row(&self, faces: &Faces, slot: &Slot, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
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
                word.map(|(w, live)| fine(faces, w).when(live, |d| d.text_color(theme::accent()))),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_page(Page::Agent(target.clone()), cx);
            }))
    }

    /// A project: its folder's name, how many sessions it has when it is
    /// folded shut, and on hover a new session in it, or, with none, taking
    /// it off the list.
    fn project_row(
        faces: &Faces,
        folder: &Path,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let name = folder.file_name().map_or_else(
            || Desk::shown(folder),
            |n: &std::ffi::OsStr| n.to_string_lossy().into_owned(),
        );
        let group = SharedString::from(format!("project-{}", folder.display()));
        let toggle = folder.to_path_buf();
        let new = folder.to_path_buf();
        let forget = folder.to_path_buf();
        agent_shell(group.clone(), false)
            .group(group.clone())
            .child(
                ui::icon(if open { "folder" } else { "chevron-right" }, 16.)
                    .text_color(theme::faint()),
            )
            .child(ui::one_line(
                ui::text(faces, name)
                    .text_size(px(13.5))
                    .text_color(theme::text()),
            ))
            .tooltip(ui::tip(Desk::shown(folder)))
            .when(!open && count > 0, |d| {
                d.child(fine(faces, count.to_string()))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .invisible()
                    .group_hover(group, Styled::visible)
                    .when(count == 0 && open, |d| {
                        d.child(
                            ui::icon_button(
                                SharedString::from(format!("forget-{}", folder.display())),
                                "x",
                                "Remove from the list",
                            )
                            .size(px(22.))
                            .text_color(theme::faint())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.forget_project(&forget, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("new-in-{}", folder.display())),
                            "plus",
                            "New session here",
                        )
                        .size(px(22.))
                        .text_color(theme::faint())
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.new_in(new.clone(), window, cx);
                            },
                        )),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(&toggle, cx)))
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
            State::Idle | State::Closed => (when::ago(s.updated, SystemTime::now()), false, false),
        };
        let agent = self
            .entry(&s.agent)
            .map_or_else(|| s.agent.clone(), |e| e.name.clone());
        let group = SharedString::from(format!("session-{key}"));
        let deletable = s.id.is_none() || self.can_delete(&s.agent);
        let busy = matches!(s.state, State::Working | State::Opening);
        session_shell(group.clone(), on)
            .group(group.clone())
            .cursor_pointer()
            .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
            .child(ui::mark(faces, &Mark::of(&s.agent, &agent), 14.))
            .child(ui::one_line(
                ui::text(faces, ui::fit(&title, ui::cols(LEFT - 112.)))
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
            .when(deletable && !busy, |d| {
                d.child(
                    div()
                        .flex_none()
                        .invisible()
                        .group_hover(group, Styled::visible)
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("delete-{key}")),
                                "trash",
                                "Delete session",
                            )
                            .size(px(20.))
                            .text_color(theme::faint())
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.delete_session(key, window, cx);
                                },
                            )),
                        ),
                )
            })
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

/// A row for a session under its project: 28 high, in by 34.
fn session_shell(id: impl Into<SharedString>, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .pl(px(34.))
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
