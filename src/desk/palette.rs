//! The command palette (⌘K), as t3code's: what can be done, the sessions
//! to go to, the projects to begin one in, found by what is typed and
//! grouped; ↑ and ↓ choose, Return does it. And the sessions in the
//! sidebar's order, gone to by number (⌘1 to ⌘9) or one after another
//! (⌘[ and ⌘]).

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, Focusable as _, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Desk, Page, Popup};
use crate::{
    fonts::Faces,
    session::State,
    theme,
    ui::{self, Mark},
};

/// What a line of the palette does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    NewSession,
    Scratch,
    AddProject,
    Settings,
    Find,
    Drawer,
    Sidebar,
    Grouping,
    CopyThread,
    Go(u64),
    NewIn(PathBuf),
}

/// How many lines the palette shows.
const LINES: usize = 12;

/// A line of the palette.
struct Line {
    group: &'static str,
    label: String,
    /// What is said at its end: a session's project, a key.
    hint: String,
    act: Act,
    /// A session's agent, drawn as its mark; else `icon`.
    agent: Option<String>,
    icon: &'static str,
    live: bool,
}

impl Desk {
    /// The sessions as the sidebar lists them by project: pinned first,
    /// then each in its place; none done or put away.
    pub(crate) fn ordered(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for folder in self.projects() {
            let mut here: Vec<_> = self
                .sessions
                .iter()
                .filter(|s| s.project == folder && self.listed_now(s) && s.flags.settled.is_none())
                .collect();
            here.sort_by_key(|s| {
                (
                    s.flags.pinned.is_none(),
                    s.flags.pinned,
                    s.flags.order,
                    s.key,
                )
            });
            out.extend(here.into_iter().map(|s| s.key));
        }
        out
    }

    /// Go to the session `n` in the sidebar's order.
    pub fn nth_session(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.ordered().get(n).copied() {
            self.show(key, window, cx);
        }
    }

    /// Go to the next session in the sidebar's order (or, `by` below zero,
    /// the one before).
    pub fn step_session(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.ordered();
        if order.is_empty() {
            return;
        }
        let at = self
            .current
            .and_then(|k| order.iter().position(|o| *o == k))
            .map_or(0, |i| i.cast_signed() + by);
        let at = at.rem_euclid(order.len().cast_signed()).cast_unsigned();
        self.show(order[at], window, cx);
    }

    /// Open the model's menu in the composer (⌘⇧M).
    pub fn open_model(&mut self, cx: &mut Context<Self>) {
        let model = self.session().and_then(|s| {
            s.options.iter().find(|o| {
                o.category
                    == Some(agent_client_protocol_schema::v1::SessionConfigOptionCategory::Model)
            })
        });
        if let Some(o) = model {
            self.popup = Popup::Option(o.id.0.to_string());
            cx.notify();
        }
    }

    /// Open the palette, or close it.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = !self.palette;
        self.palette_at = 0;
        if self.palette {
            self.palette_input.update(cx, |i, cx| i.set_text("", cx));
            window.focus(&self.palette_input.focus_handle(cx));
        } else {
            window.focus(&self.input.focus_handle(cx));
        }
        cx.notify();
    }

    /// Move the palette's choice up (below zero) or down.
    pub(crate) fn step_palette(&mut self, by: isize, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).text();
        let last = self.lines(&query).len().saturating_sub(1);
        self.palette_at = self.palette_at.saturating_add_signed(by).min(last);
        cx.notify();
    }

    /// The lines the palette offers for what is typed: with nothing typed
    /// what can be done leads, else the sessions found.
    fn lines(&self, query: &str) -> Vec<Line> {
        let q = query.trim().to_lowercase();
        let found = |text: &str| q.is_empty() || text.to_lowercase().contains(&q);
        let act = |label: &str, hint: &str, act: Act, icon: &'static str| Line {
            group: "Actions",
            label: label.to_owned(),
            hint: hint.to_owned(),
            act,
            agent: None,
            icon,
            live: false,
        };
        let actions: Vec<Line> = [
            act("New session", "⌘N", Act::NewSession, "plus"),
            act("New scratch session", "", Act::Scratch, "file-plus"),
            act("Add a project", "", Act::AddProject, "folder"),
            act("Find in this session", "⌘F", Act::Find, "search"),
            act("Commands drawer", "⌘J", Act::Drawer, "terminal"),
            act("Show or hide the sidebar", "⌘B", Act::Sidebar, "sidebar"),
            act(
                "Group sessions by project or status",
                "",
                Act::Grouping,
                "list",
            ),
            act("Settings", "⌘,", Act::Settings, "settings"),
            act("Copy this conversation", "", Act::CopyThread, "copy"),
        ]
        .into_iter()
        .filter(|l| found(&l.label))
        .collect();
        let mut sessions = Vec::new();
        for (n, key) in self.ordered().into_iter().enumerate() {
            let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
                continue;
            };
            let project = self.project_name(&s.project);
            if !found(&s.title) && !found(&project) {
                continue;
            }
            let hint = if n < 9 {
                format!("{project}  ⌘{}", n + 1)
            } else {
                project
            };
            sessions.push(Line {
                group: "Sessions",
                label: s.title.clone(),
                hint,
                act: Act::Go(key),
                agent: Some(s.agent.clone()),
                icon: "",
                live: matches!(s.state, State::Working | State::Opening),
            });
        }
        let projects = self.projects().into_iter().filter_map(|folder| {
            let label = format!("New session in {}", self.project_name(&folder));
            found(&label).then(|| Line {
                group: "Projects",
                label,
                hint: String::new(),
                act: Act::NewIn(folder),
                agent: None,
                icon: "plus",
                live: false,
            })
        });
        let mut all = if q.is_empty() {
            let mut all = actions;
            all.extend(sessions.into_iter().take(5));
            all
        } else {
            let mut all = sessions;
            all.extend(actions);
            all
        };
        all.extend(projects);
        all.truncate(LINES);
        all
    }

    /// Do the line `n` of the palette.
    pub fn run_act(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).text();
        let Some(line) = self.lines(&query).into_iter().nth(n) else {
            return;
        };
        self.palette = false;
        window.focus(&self.input.focus_handle(cx));
        match line.act {
            Act::NewSession => self.new_default(window, cx),
            Act::Scratch => self.new_scratch(window, cx),
            Act::AddProject => self.open_page(Page::NewProject, cx),
            Act::Settings => self.open_page(Page::Settings, cx),
            Act::CopyThread => {
                if let Some(key) = self.current {
                    self.copy_thread(key, cx);
                }
            }
            Act::Find => self.toggle_find(window, cx),
            Act::Drawer => self.toggle_drawer(window, cx),
            Act::Sidebar => self.toggle_sidebar(cx),
            Act::Grouping => self.toggle_grouping(cx),
            Act::Go(key) => self.show(key, window, cx),
            Act::NewIn(folder) => self.new_in(folder, window, cx),
        }
        cx.notify();
    }

    /// The palette, over the window's top.
    pub(crate) fn palette_view(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.palette {
            return None;
        }
        let query = self.palette_input.read(cx).text();
        let mut list = div().flex().flex_col().p(px(6.));
        let mut group = "";
        for (n, line) in self.lines(&query).into_iter().enumerate() {
            if line.group != group {
                group = line.group;
                list = list.child(
                    div()
                        .px(px(10.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .child(ui::label(faces, group)),
                );
            }
            let lead = match &line.agent {
                Some(agent) => ui::mark(faces, &Mark::of(agent, &self.name_of(agent)), 16.),
                None => ui::icon(line.icon, 15.)
                    .flex_none()
                    .text_color(theme::muted())
                    .into_any_element(),
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("act-{n}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .h(px(34.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(n == self.palette_at, |d| d.bg(theme::panel2()))
                    .when(n != self.palette_at, |d| {
                        d.hover(|st| st.bg(theme::hover()))
                    })
                    .child(lead)
                    .child(
                        ui::one_line(ui::text(faces, ui::fit(&line.label, 52)).text_size(px(13.5)))
                            .flex_1(),
                    )
                    .when(line.live, |d| d.child(ui::live_dot()))
                    .child(
                        ui::text(faces, line.hint)
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme::faint()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.run_act(n, window, cx))),
            );
        }
        let key = |k: &'static str, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(crate::sidebar::kbd(faces, k))
                .child(
                    ui::text(faces, what)
                        .text_size(px(11.5))
                        .text_color(theme::faint()),
                )
        };
        Some(
            div()
                .absolute()
                .top(px(72.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    ui::menu("palette")
                        .flex_col()
                        .w(px(600.))
                        .p_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .px(px(16.))
                                .h(px(48.))
                                .border_b_1()
                                .border_color(theme::border2())
                                .child(ui::icon("search", 16.).text_color(theme::faint()))
                                .child(div().flex_1().child(self.palette_input.clone()))
                                .child(crate::sidebar::kbd(faces, "esc")),
                        )
                        .child(list)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(14.))
                                .px(px(16.))
                                .h(px(34.))
                                .border_t_1()
                                .border_color(theme::border2())
                                .child(key("↑↓", "move"))
                                .child(key("↵", "open"))
                                .child(key("⌘1–9", "a session, from anywhere")),
                        ),
                )
                .into_any_element(),
        )
    }
}
