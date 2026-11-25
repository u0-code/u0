//! The column down the left, as the design draws it: the window's lights
//! with search and a new session beside them; New session; the projects,
//! each a folder with its sessions under it, whichever agent each is with;
//! the agents switched on; a few more agents, quieter; and at the foot the
//! settings.

use std::{path::Path, time::SystemTime};

use gpui::component::InteractiveElementExt as _;
use gpui::component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    popover::Popover,
    resizable::{HandleEdge, resize_handle},
    scroll::ScrollableElement as _,
    separator::Separator,
};
use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, Later, Page, Popup, Renaming, TOPBAR, draggable},
    fonts::Faces,
    hub::Stage,
    session::{Session, State},
    theme, ui, when,
};

impl Desk {
    /// The name being written in place of a row's own, at the size and
    /// place of the words it replaces.
    fn rename_field(&self) -> gpui::Div {
        div()
            .flex_1()
            .min_w(px(0.))
            .ml(px(-6.))
            .child(Input::new(&self.rename).xsmall())
    }

    pub(crate) fn sidebar(
        &self,
        faces: &Faces,
        lights: f32,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        // The window's lights, and back and ahead through the sessions seen
        // at the end of the sidebar's part of the bar.
        let step = |id: &'static str, icon: &'static str, tip: &str, name: &str, ahead: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .child(ui::icon(icon, 14.))
                .tooltip(crate::keys::hint(cx, tip, name))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.step_trail(ahead, window, cx);
                }))
        };
        let head = draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(8.))
            .child(div().flex_1())
            .child(step("back", "arrow-left", "Back", "back", false))
            .child(step("ahead", "arrow-right", "Ahead", "ahead", true));

        // t3code's row under it: the search, the projects to show, and a
        // new session (⇧-click: in the project on screen).
        let projects_faces = faces.clone();
        let controls = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .hover(|d| d.bg(theme::row_hover()))
                    .child(
                        Input::new(&self.search.input)
                            .appearance(false)
                            .cleanable(true)
                            .prefix(ui::icon("search", 16.).text_color(theme::muted())),
                    ),
            )
            .child(
                self.menu(
                    Popup::Projects,
                    Button::new("projects")
                        .ghost()
                        .small()
                        .child(ui::icon("folder", 16.))
                        .tooltip("Projects")
                        .selected(!self.filter.is_empty()),
                    move |d, _, cx| d.projects_menu(&projects_faces, cx),
                    cx,
                )
                .anchor(gpui::Anchor::TopRight),
            )
            .child(
                Button::new("new-session")
                    .ghost()
                    .small()
                    .child(ui::icon("square-pen", 16.))
                    .tooltip(crate::keys::hint(cx, "New session", "new_session"))
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                        let here = this.session().map(|s| s.meta.project.clone());
                        match here.filter(|_| e.modifiers().shift) {
                            Some(project) => this.new_in(project, window, cx),
                            None => this.new_default(window, cx),
                        }
                    })),
            );

        // A search begins with its first letter: until then the sidebar is
        // as it was.
        let query = Some(self.search.input.read(cx).value().trim().to_lowercase())
            .filter(|q| !q.is_empty());
        let mut projects = div().flex().flex_col().gap(px(2.)).px(px(8.));
        // The projects chosen in the picker, or all.
        let shown = self
            .projects()
            .into_iter()
            .filter(|f| self.filter.is_empty() || self.filter.contains(f));
        for folder in shown {
            let now = crate::store::secs(SystemTime::now());
            let snoozed = self
                .sessions
                .all
                .iter()
                .filter(|s| s.meta.project == folder && !s.meta.flags.archived)
                .filter(|s| s.meta.flags.snoozed.is_some_and(|t| t > now))
                .count();
            let found = |s: &&Session| query.as_ref().is_none_or(|q| self.matches(s, q));
            let sessions: Vec<&Session> =
                self.in_project(&folder).into_iter().filter(found).collect();
            let done: Vec<&Session> = self
                .sessions
                .all
                .iter()
                .filter(|s| {
                    s.meta.project == folder && self.listed_now(s) && s.meta.flags.settled.is_some()
                })
                .filter(found)
                .collect();
            if query.is_some() && sessions.is_empty() && done.is_empty() {
                continue;
            }
            let open = query.is_some() || !self.closed.contains(&folder);
            projects = projects.child(self.project_row(faces, &folder, sessions.len(), open, cx));
            if !open {
                continue;
            }
            if sessions.is_empty() && done.is_empty() {
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
            // Done for now, and out of the way until later: folded.
            if !done.is_empty() {
                let key = format!("done-{}", folder.display());
                let shown = query.is_some() || self.expanded.contains(&key);
                projects =
                    projects.child(Self::shelf_row(faces, key, "Done", done.len(), shown, cx));
                if shown {
                    let mut done = done;
                    done.sort_by_key(|s| std::cmp::Reverse(s.meta.flags.settled));
                    for s in done {
                        projects = projects.child(self.session_row(faces, s, cx));
                    }
                }
            }
            if snoozed > 0 && query.is_none() {
                let key = format!("snoozed-{}", folder.display());
                let shown = self.expanded.contains(&key);
                projects =
                    projects.child(Self::shelf_row(faces, key, "Snoozed", snoozed, shown, cx));
                if shown {
                    for s in self.sessions.all.iter().filter(|s| {
                        s.meta.project == folder
                            && !s.meta.flags.archived
                            && s.meta.flags.snoozed.is_some_and(|t| t > now)
                    }) {
                        projects = projects.child(self.session_row(faces, s, cx));
                    }
                }
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
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.add_project_page(window, cx);
                    })),
            );
        }

        let list = div()
            .id("sidebar-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(projects);

        // t3code's foot: a row of icons, and at its end what the agents
        // need of you, each a click from its page.
        let mut pills = div().flex().items_center().gap(px(4.));
        for slot in &self.slots {
            let word = match &slot.stage {
                Stage::Ready | Stage::Running { .. }
                    if matches!(slot.auth, crate::hub::Auth::Needed(_)) =>
                {
                    "Sign in"
                }
                Stage::Missing(_) => "Install",
                Stage::Installing(_) => "Installing",
                Stage::Failed(_) => "Stopped",
                _ => continue,
            };
            let id = slot.entry.id.clone();
            pills = pills.child(
                Button::new(SharedString::from(format!("pill-{id}")))
                    .outline()
                    .xsmall()
                    .child(ui::mark(faces, &self.mark_of(&id), 12.))
                    .child(word)
                    .tooltip(slot.entry.name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_page(Page::Agent(id.clone()), window, cx);
                    })),
            );
        }
        let foot = div()
            .flex()
            .flex_none()
            .items_center()
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(
                Button::new("settings")
                    .ghost()
                    .child(ui::icon("settings", 16.))
                    .tooltip(crate::keys::hint(cx, "Settings", "settings"))
                    .selected(self.pages.open)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_tab(this.pages.tab, window, cx);
                    })),
            )
            .child(div().flex_1())
            .child(pills);

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .child(head)
            .child(controls)
            .child(list.overflow_y_scrollbar())
            .children(self.chosen_bar(faces, cx))
            .child(foot)
            // Its edge: dragged, its width; double-clicked, t3code's.
            .child(
                resize_handle::<LeftEdge, LeftEdge>("left-edge", gpui::Axis::Horizontal)
                    .inside(HandleEdge::Trailing)
                    .on_drag(LeftEdge, |edge, _, _, cx| cx.new(|_| edge.clone()))
                    .on_double_click(cx.listener(|this, _, _, cx| {
                        this.settings.left = crate::settings::LEFT;
                        this.save_settings();
                        cx.notify();
                    })),
            )
    }

    /// A project: its folder's name, how many sessions it has when it is
    /// folded shut, and on hover a new session in it, or, with none, taking
    /// it off the list.
    fn project_row(
        &self,
        faces: &Faces,
        folder: &Path,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let group = SharedString::from(format!("project-{}", folder.display()));
        let toggle = folder.to_path_buf();
        let new = folder.to_path_buf();
        let menu_open = self.popup == Popup::Project(folder.to_path_buf());
        let pinned = self.prefs(folder).pinned;
        let name: gpui::AnyElement = if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder)
        {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, self.project_name(folder))
                    .text_size(px(13.5))
                    .text_color(theme::text()),
            )
            .into_any_element()
        };
        let (menu_faces, menu_for) = (faces.clone(), folder.to_path_buf());
        let row = agent_shell(group.clone(), false)
            .group(group.clone())
            .child(
                // Open, a project shows its own icon if it has one; one on
                // another machine shows as one.
                match (
                    open,
                    crate::remote::parse(folder).is_some(),
                    self.held.get(folder).and_then(|h| h.icon.clone()),
                ) {
                    (true, false, Some(icon)) => div()
                        .size(px(16.))
                        .flex_none()
                        .child(gpui::img(icon).size(px(16.)).rounded(px(3.)))
                        .into_any_element(),
                    (open, remote, _) => ui::icon(
                        match (open, remote) {
                            (false, _) => "chevron-right",
                            (true, true) => "server",
                            (true, false) => "folder",
                        },
                        16.,
                    )
                    .text_color(theme::faint())
                    .into_any_element(),
                },
            )
            .child(name)
            .when(pinned, |d| {
                d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
            })
            .tooltip(ui::tip(Desk::shown(folder)))
            .when(!open && count > 0, |d| {
                d.child(fine(faces, count.to_string()))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .when(!menu_open, |d| {
                        d.invisible().group_hover(group, Styled::visible)
                    })
                    .child(
                        self.menu(
                            Popup::Project(folder.to_path_buf()),
                            ui::icon_button(
                                SharedString::from(format!("pmore-{}", folder.display())),
                                "dots",
                                "More",
                            )
                            .size(px(22.))
                            .text_color(theme::faint()),
                            move |d, _, cx| d.project_menu(&menu_faces, &menu_for, cx),
                            cx,
                        )
                        .anchor(gpui::Anchor::TopRight),
                    )
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
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(&toggle, cx)));
        row.into_any_element()
    }

    /// What can be done with a project: name it, pin it, the agent and the
    /// way of working its new sessions take, find it, put it away.
    fn project_menu(&self, faces: &Faces, folder: &Path, cx: &mut Context<Self>) -> Div {
        let prefs = self.prefs(folder);
        let id = folder.display().to_string();
        let item = |n: usize, icon: &'static str, label: String| {
            ui::menu_row(SharedString::from(format!("pmenu-{id}-{n}")), false)
                .child(ui::icon(icon, 14.).text_color(theme::faint()))
                .child(ui::text(faces, label).flex_1().text_size(px(13.)))
        };
        let head = |label: &str| {
            ui::text(faces, label.to_owned())
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(11.5))
                .text_color(theme::faint())
        };
        let dir = folder.to_path_buf();
        let mut menu = div().flex().flex_col().w(px(228.));
        let (rename_dir, pin_dir, show_dir, archive_dir) =
            (dir.clone(), dir.clone(), dir.clone(), dir.clone());
        menu = menu
            .child(item(0, "pencil", "Rename".into()).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.begin_rename(Renaming::Project(rename_dir.clone()), window, cx);
                },
            )))
            .child(
                item(1, "pin", if prefs.pinned { "Unpin" } else { "Pin" }.into()).on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_project_pin(&pin_dir, cx)),
                ),
            )
            .child(
                item(2, "folder", "Show in Finder".into())
                    .on_click(move |_, _, cx| cx.open_with_system(&show_dir)),
            );
        if self.held.get(folder).is_some_and(|h| h.repo) {
            let on = prefs.worktree.unwrap_or(self.settings.worktree);
            let wt_dir = dir.clone();
            menu = menu.child(
                item(
                    3,
                    "branch",
                    if on {
                        "New sessions: own worktree ✓".into()
                    } else {
                        "New sessions: own worktree".into()
                    },
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.set_project_worktree(&wt_dir, !on, cx)),
                ),
            );
        }
        menu = menu.child(head("New sessions begin with"));
        let agents: Vec<(Option<String>, String)> =
            std::iter::once((None, "The one on screen".to_owned()))
                .chain(
                    self.slots
                        .iter()
                        .map(|s| (Some(s.entry.id.clone()), self.name_of(&s.entry.id))),
                )
                .collect();
        for (n, (agent, name)) in agents.into_iter().enumerate() {
            let on = prefs.agent == agent;
            let agent_dir = dir.clone();
            menu = menu.child(
                item(10 + n, if on { "check" } else { "minus" }, name).on_click(cx.listener(
                    move |this, _, _, cx| this.set_project_agent(&agent_dir, agent.clone(), cx),
                )),
            );
        }
        menu = menu.child(Separator::horizontal().mx(px(2.)).my(px(4.)));
        menu =
            menu.child(item(8, "archive", "Archive".into()).on_click(
                cx.listener(move |this, _, _, cx| this.archive_project(&archive_dir, cx)),
            ));
        if self.can_forget_project(&dir) {
            let forget_dir = dir.clone();
            menu =
                menu.child(item(9, "x", "Remove from the list".into()).on_click(
                    cx.listener(move |this, _, _, cx| this.forget_project(&forget_dir, cx)),
                ));
        }
        menu
    }

    fn session_row(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> AnyElement {
        let key = s.key;
        let on = self.page == Page::Thread && self.current == Some(key);
        let title = if s.state == State::Draft && s.items.is_empty() {
            "New session".to_owned()
        } else {
            s.meta.title.clone()
        };
        let (when, live, trouble) = match &s.state {
            State::Working | State::Opening if s.waiting().is_some() => {
                ("waiting".to_owned(), true, false)
            }
            State::Working | State::Opening => ("running".to_owned(), true, false),
            State::SignIn => ("sign in".to_owned(), true, false),
            State::Broken(_) => ("stopped".to_owned(), false, true),
            State::Draft => (String::new(), false, false),
            State::Idle | State::Closed => match s.outcome() {
                Some(o) => (o.to_owned(), false, o == "failed"),
                None => (when::ago(s.meta.updated, SystemTime::now()), false, false),
            },
        };
        let agent = self
            .entry(&s.meta.agent)
            .map_or_else(|| s.meta.agent.clone(), |e| e.name.clone());
        let group = SharedString::from(format!("session-{key}"));
        let done = s.meta.flags.settled.is_some();
        let menu_open = self.popup == Popup::Session(key);
        // The title has the room the row's other marks leave it: the time
        // or state, the unread dot, the pin, the pull request's number.
        let room = self.left_width()
            - 108.
            - (ui::fit(&when, 40).chars().count() as f32 * 7.5).max(30.)
            - if s.meta.flags.unread && !on { 12. } else { 0. }
            - if s.meta.flags.pinned.is_some() {
                18.
            } else {
                0.
            }
            - if s.git.pr.is_some() { 36. } else { 0. };
        let name: AnyElement = if self.renaming == Some(Renaming::Session { key, here: false }) {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, ui::fit(&title, ui::cols(room)))
                    .text_size(px(13.))
                    .text_color(if on {
                        theme::text()
                    } else if done {
                        theme::faint()
                    } else {
                        theme::text2()
                    }),
            )
            .into_any_element()
        };
        // On hover, a card: its whole title, where it works, and the last
        // thing the agent said.
        let last = s.items.iter().rev().find_map(|i| match i {
            crate::session::Item::Agent(t) => Some(gist(t)).filter(|g| !g.is_empty()),
            _ => None,
        });
        let mut card = format!(
            "{}\n{} · {}",
            s.meta.title,
            agent,
            Desk::shown(&s.meta.folder)
        );
        if let Some(b) = &s.branch {
            card.push_str(" · ");
            card.push_str(b);
        }
        if let Some(l) = last {
            card.push_str("\n\n");
            card.push_str(&l);
        }
        let dragged = Dragged {
            key,
            title: SharedString::from(ui::fit(&title, 40)),
        };
        let row = session_shell(group.clone(), on)
            .group(group.clone())
            .cursor_pointer()
            .when(!menu_open, |d| d.tooltip(ui::tip(card)))
            // Dragged onto another, it goes before it.
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .drag_over::<Dragged>(|st, _, _, _| st.bg(theme::row_hover()))
            .on_drop(cx.listener(move |this, d: &Dragged, _, cx| this.move_session(d.key, key, cx)))
            .when(!on, |d| d.hover(|st| st.bg(theme::row_hover())))
            .child(div().when(done, |d| d.opacity(0.55)).child(ui::mark(
                faces,
                &self.mark_of(&s.meta.agent),
                14.,
            )))
            .child(name)
            .when(s.meta.flags.pinned.is_some(), |d| {
                d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
            })
            // Its pull request, coloured by its checks.
            .children(s.git.pr.as_ref().map(|pr| {
                let color = if pr.failed > 0 {
                    theme::coral()
                } else if pr.going > 0 {
                    theme::accent()
                } else if pr.state == "merged" {
                    theme::link()
                } else {
                    theme::ok()
                };
                ui::text(faces, format!("#{}", pr.number))
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(color)
            }))
            .when(s.meta.flags.unread && !on, |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme::link()),
                )
            })
            .child(fine(faces, when).text_color(if live {
                theme::accent()
            } else if trouble {
                theme::coral()
            } else {
                theme::faint()
            }))
            .child(
                div()
                    .flex_none()
                    .when(!menu_open, |d| {
                        d.invisible().group_hover(group, Styled::visible)
                    })
                    .child({
                        let faces = faces.clone();
                        self.menu(
                            Popup::Session(key),
                            ui::icon_button(
                                SharedString::from(format!("more-{key}")),
                                "dots",
                                "More",
                            )
                            .size(px(20.))
                            .text_color(theme::faint()),
                            move |d, _, cx| {
                                div().children(
                                    d.sessions.at(key).map(|s| d.session_menu(&faces, s, cx)),
                                )
                            },
                            cx,
                        )
                        .anchor(gpui::Anchor::TopRight)
                    }),
            )
            // ⌘-click chooses it among others, to act on them together.
            .when(self.chosen.contains(&key), |d| d.bg(theme::row_chosen()))
            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                if e.modifiers().platform {
                    if !this.chosen.remove(&key) {
                        this.chosen.insert(key);
                    }
                    cx.notify();
                } else {
                    this.chosen.clear();
                    this.show(key, window, cx);
                }
            }));
        row.into_any_element()
    }

    /// The sessions chosen with ⌘-click, and what can be done with all of
    /// them at once.
    fn chosen_bar(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        if self.chosen.is_empty() {
            return None;
        }
        let act = |id: &'static str, label: &'static str| {
            ui::button(id, label, ui::Weight::Quiet)
                .h(px(24.))
                .px(px(7.))
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .mx(px(8.))
                .mb(px(8.))
                .p(px(8.))
                .rounded(px(10.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            ui::text(faces, format!("{} chosen", self.chosen.len()))
                                .flex_1()
                                .text_size(px(12.5))
                                .text_color(theme::text2()),
                        )
                        .child(
                            ui::icon_button("chosen-clear", "x", "Choose none")
                                .size(px(22.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.chosen.clear();
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(2.))
                        .child(act("chosen-done", "Done").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.for_chosen(cx, |f| {
                                    f.settled = Some(crate::store::secs(SystemTime::now()));
                                });
                            },
                        )))
                        .child(act("chosen-snooze", "Snooze an hour").on_click(cx.listener(
                            |this, _, _, cx| {
                                let at = crate::store::secs(
                                    SystemTime::now() + std::time::Duration::from_secs(3600),
                                );
                                this.for_chosen(cx, |f| f.snoozed = Some(at));
                            },
                        )))
                        .child(act("chosen-unread", "Unread").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.for_chosen(cx, |f| f.unread = true);
                            },
                        )))
                        .child(act("chosen-unpin", "Unpin").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.for_chosen(cx, |f| f.pinned = None);
                            },
                        )))
                        .child(act("chosen-delete", "Delete").on_click(cx.listener(
                            |this, _, _, cx| {
                                let keys: Vec<u64> =
                                    std::mem::take(&mut this.chosen).into_iter().collect();
                                for key in keys {
                                    this.delete_later(key, cx);
                                }
                            },
                        ))),
                ),
        )
    }

    /// What can be done with a session: name it, pin it, mark it, put it
    /// away, branch it off, delete it.
    fn session_menu(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let now = crate::store::secs(SystemTime::now());
        let busy = matches!(s.state, State::Working | State::Opening);
        let item =
            |n: usize, icon: &'static str, label: String| session_item(faces, key, n, icon, label);
        // A part of the menu, opened beside the line that names it: a
        // Popover within the menu's.
        let beside = |n: usize,
                      icon: &'static str,
                      label: &'static str,
                      part: fn(&Self, &Faces, &Session, &mut Context<Self>) -> Div,
                      cx: &mut Context<Self>| {
            let (faces, desk) = (faces.clone(), cx.entity());
            let line = ui::opener(SharedString::from(format!("smenu-{key}-{n}")))
                .w_full()
                .h(px(30.))
                .px(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::icon(icon, 14.).text_color(theme::faint()))
                        .child(ui::text(&faces, label).flex_1().text_size(px(13.)))
                        .child(ui::icon("chevron-right", 12.).text_color(theme::faint())),
                );
            Popover::new(SharedString::from(format!("session-sub-{key}-{n}")))
                .anchor(gpui::Anchor::LeftCenter)
                .trigger_style(gpui::StyleRefinement::default().w_full())
                .p(px(6.))
                .trigger(line)
                .content(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        div().children(d.sessions.at(key).map(|s| part(d, &faces, s, cx)))
                    })
                })
        };
        let mut menu = div().flex().flex_col().w(px(208.));
        menu = menu.child(item(0, "pencil", "Rename".into()).on_click(cx.listener(
            move |this, _, window, cx| {
                this.begin_rename(Renaming::Session { key, here: false }, window, cx);
            },
        )));
        let pin = if s.meta.flags.pinned.is_some() {
            "Unpin"
        } else {
            "Pin"
        };
        menu = menu.child(
            item(1, "pin", pin.into())
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_pin(key, cx))),
        );
        if !s.meta.flags.unread {
            menu = menu.child(
                item(2, "bell", "Mark as unread".into())
                    .on_click(cx.listener(move |this, _, _, cx| this.mark_unread(key, cx))),
            );
        }
        let done = if s.meta.flags.settled.is_some() {
            "Not done"
        } else {
            "Mark as done"
        };
        menu = menu.child(
            item(3, "check", done.into())
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_settled(key, cx))),
        );
        if s.meta.flags.snoozed.is_some_and(|t| t > now) {
            menu = menu.child(
                item(4, "clock", "Bring back now".into())
                    .on_click(cx.listener(move |this, _, _, cx| this.wake(key, cx))),
            );
        } else {
            menu = menu.child(beside(5, "clock", "Snooze", Self::snooze_part, cx));
        }
        // What it said and what names it, copied.
        menu = menu.child(beside(
            6,
            "copy",
            "Copy",
            |_, faces, s, cx| Self::copy_part(faces, s, cx),
            cx,
        ));
        if self.can_fork(s) && !busy {
            menu = menu.child(
                item(8, "branch", "Branch off a copy".into())
                    .on_click(cx.listener(move |this, _, _, cx| this.fork(key, cx))),
            );
        }
        // Another session where this one works: its worktree, its branch.
        if s.meta.worktree.is_some() {
            let on = s.branch.clone().unwrap_or_else(|| "its worktree".into());
            menu = menu.child(
                item(
                    24,
                    "plus",
                    format!("New session on {}", crate::ui::fit(&on, 22)),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.new_in_worktree(key, window, cx)),
                ),
            );
        }
        let kept = if s.meta.flags.kept {
            "Let it be done on its own"
        } else {
            "Never done on its own"
        };
        menu = menu.child(item(25, "pin", kept.into()).on_click(cx.listener(
            move |this, _, _, cx| {
                this.popup = crate::desk::Popup::Closed;
                this.mark(key, cx, |f| f.kept = !f.kept);
            },
        )));
        if !busy {
            menu = menu
                .child(Separator::horizontal().mx(px(2.)).my(px(4.)))
                .child(
                    item(9, "archive", "Archive".into())
                        .on_click(cx.listener(move |this, _, _, cx| this.archive(key, cx))),
                )
                .child(
                    item(10, "trash", "Delete".into())
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_later(key, cx))),
                );
        }
        menu
    }

    /// For how long a session is put out of sight: a while chosen, or one
    /// written.
    fn snooze_part(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let mut snooze = div().flex().flex_col().w(px(208.));
        for (n, (label, later)) in [
            ("For an hour", Later::In(60)),
            ("Until tomorrow", Later::Tomorrow),
            ("Until next week", Later::NextWeek),
        ]
        .into_iter()
        .enumerate()
        {
            snooze = snooze.child(
                session_item(faces, key, 30 + n, "clock", label.into())
                    .on_click(cx.listener(move |this, _, _, cx| this.snooze(key, later, cx))),
            );
        }
        // Or for as long as you say.
        snooze.child(
            div()
                .flex()
                .items_center()
                .px(px(8.))
                .h(px(32.))
                .child(div().flex_1().child(Input::new(&self.menu_search).xsmall())),
        )
    }

    /// What of a session can be copied: what was said, and what names it.
    fn copy_part(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let mut copy = div().flex().flex_col().w(px(208.)).child(
            session_item(faces, key, 40, "copy", "The conversation".into())
                .on_click(cx.listener(move |this, _, _, cx| this.copy_thread(key, cx))),
        );
        let copies = [
            Some(("The folder's path", s.meta.folder.display().to_string())),
            s.branch.clone().map(|b| ("The branch's name", b)),
            s.meta
                .id
                .as_ref()
                .map(|id| ("The session's id", id.0.to_string())),
        ];
        for (n, (label, text)) in copies.into_iter().flatten().enumerate() {
            copy = copy.child(
                session_item(faces, key, 41 + n, "copy", label.into()).on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                        this.popup = crate::desk::Popup::Closed;
                        cx.notify();
                    },
                )),
            );
        }
        copy
    }

    /// The sessions of `folder` as the sidebar lists them: pinned on top
    /// in their order; then each where it was put as it began. What goes on
    /// in a session does not move it. None done is among them.
    pub(crate) fn in_project(&self, folder: &Path) -> Vec<&Session> {
        let mut here: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.meta.project == folder && self.listed_now(s) && s.meta.flags.settled.is_none()
            })
            .collect();
        here.sort_by_key(|s| {
            (
                s.meta.flags.pinned.is_none(),
                s.meta.flags.pinned,
                s.meta.flags.order,
                s.key,
            )
        });
        here
    }

    /// The projects the sidebar shows, as t3code's picker: searched, each
    /// chosen or not (none chosen, all shown), and a project added.
    fn projects_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let typed = self.menu_search.read(cx).value().trim().to_owned();
        let query = crate::desk::rank::Query::new(&typed);
        let mut menu = div().flex().flex_col().w(px(240.)).child(
            div()
                .pb(px(4.))
                .child(Input::new(&self.menu_search).xsmall()),
        );
        for (n, p) in self.projects().into_iter().enumerate() {
            let name = self.project_name(&p);
            if !query.is_empty() && crate::desk::rank::terms(&query, &name, &[], true).is_none() {
                continue;
            }
            let on = self.filter.contains(&p);
            let icon = self.held.get(&p).and_then(|h| h.icon.clone());
            menu = menu.child(
                ui::menu_row(SharedString::from(format!("filter-{n}")), on)
                    .child(crate::desk::projects::project_icon(&p, icon, 14.))
                    .child(ui::one_line(ui::text(faces, name).text_size(px(13.))).flex_1())
                    .when(on, |d| d.child(ui::icon("check", 14.)))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_filter(&p, cx))),
            );
        }
        menu.child(Separator::horizontal().my(px(4.))).child(
            ui::menu_row("filter-add", false)
                .child(ui::icon("folder-plus", 14.))
                .child(ui::text(faces, "Add project").text_size(px(13.)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_popup(window, cx);
                    this.add_project_page(window, cx);
                })),
        )
    }

    /// A folded shelf under a project: its sessions done for now, or
    /// snoozed, and how many.
    fn shelf_row(
        faces: &Faces,
        key: String,
        label: &str,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let toggle = key.clone();
        session_shell(SharedString::from(key), false)
            .cursor_pointer()
            .hover(|st| st.bg(theme::row_hover()))
            .child(
                ui::icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.,
                )
                .text_color(theme::faint()),
            )
            .child(
                ui::text(faces, format!("{label} · {count}"))
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(theme::faint()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
    }

    /// Whether the session `s` answers the search `query`: by its title,
    /// or by what was said in it.
    fn matches(&self, s: &Session, query: &str) -> bool {
        s.meta.title.to_lowercase().contains(query) || self.search.found.contains(&s.key)
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
        .when(on, |d| d.bg(theme::row_open()))
        .when(!on, |d| d.hover(|s| s.bg(theme::row_hover())))
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
        .when(on, |d| d.bg(theme::row_open()))
}

/// A line of a session's menu, `n`th: its icon and what it does.
fn session_item(
    faces: &Faces,
    key: u64,
    n: usize,
    icon: &'static str,
    label: String,
) -> gpui::Stateful<Div> {
    ui::menu_row(SharedString::from(format!("smenu-{key}-{n}")), false)
        .child(ui::icon(icon, 14.).text_color(theme::faint()))
        .child(ui::text(faces, label).flex_1().text_size(px(13.)))
}

/// Small print at the right of a row.
fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// The sidebar's edge, dragged.
#[derive(Clone)]
pub struct LeftEdge;

impl gpui::Render for LeftEdge {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A session's row being dragged, as it shows under the pointer.
#[derive(Clone)]
struct Dragged {
    key: u64,
    title: SharedString,
}

impl gpui::Render for Dragged {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(5.))
            .rounded(px(8.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .shadow_lg()
            .text_size(px(13.))
            .text_color(theme::text())
            .child(self.title.clone())
    }
}

/// What an agent said, as a line of plain words for a card: its prose
/// only (no code, no drawn trees or tables), the marks of Markdown taken
/// off, about two lines of a tip.
fn gist(said: &str) -> String {
    let mut words = String::new();
    let mut code = false;
    for line in said.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        let drawn = line.contains(['│', '├', '└', '┌', '─']) || line.starts_with('|');
        if code || drawn || line.is_empty() {
            continue;
        }
        let line = line
            .trim_start_matches(['#', '>', '-', '*', ' '])
            .replace(['`', '*'], "");
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(line.trim());
        if words.chars().count() > 120 {
            break;
        }
    }
    ui::fit(&words, 110)
}

#[cfg(test)]
mod tests {
    use super::gist;

    #[test]
    fn a_cards_words_are_the_prose_without_code_trees_or_marks() {
        let said = "这次调研生成的文件都在 `/Users/x/jepa` 下：\n```\n/Users/x/jepa/\n├── README.md\n└── reports/\n```\n## 结论\n- **核心思想**：JEPA 在表征空间里预测。";
        assert_eq!(
            gist(said),
            "这次调研生成的文件都在 /Users/x/jepa 下： 结论 核心思想：JEPA 在表征空间里预测。"
        );
    }
}
