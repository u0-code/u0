//! The command palette (⌘K), as t3code's: what can be done, the sessions
//! to go to, the projects to begin one in, found by what is typed; Return
//! does the first. And the sessions in the sidebar's order, gone to by
//! number (⌘1 to ⌘9) or one after another (⌘[ and ⌘]).

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, Focusable as _, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Desk, Page, Popup};
use crate::{fonts::Faces, session::State, theme, ui};

/// What a line of the palette does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    NewSession,
    Scratch,
    AddProject,
    Settings,
    Find,
    Drawer,
    Sidebar,
    Grouping,
    Go(u64),
    NewIn(PathBuf),
}

/// How many lines the palette shows.
const LINES: usize = 12;

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
        if self.palette {
            self.palette_input.update(cx, |i, cx| i.set_text("", cx));
            window.focus(&self.palette_input.focus_handle(cx));
        } else {
            window.focus(&self.input.focus_handle(cx));
        }
        cx.notify();
    }

    /// The lines the palette offers for what is typed.
    fn acts(&self, query: &str) -> Vec<(String, &'static str, Act)> {
        let q = query.trim().to_lowercase();
        let mut all: Vec<(String, &'static str, Act)> = vec![
            ("New session".into(), "⌘N", Act::NewSession),
            ("New scratch session".into(), "", Act::Scratch),
            ("Add a project".into(), "", Act::AddProject),
            ("Find in this session".into(), "⌘F", Act::Find),
            ("Commands drawer".into(), "⌘J", Act::Drawer),
            ("Show or hide the sidebar".into(), "⌘B", Act::Sidebar),
            (
                "Group sessions by project or status".into(),
                "",
                Act::Grouping,
            ),
            ("Settings".into(), "⌘,", Act::Settings),
        ];
        for folder in self.projects() {
            all.push((
                format!("New session in {}", self.project_name(&folder)),
                "",
                Act::NewIn(folder),
            ));
        }
        for key in self.ordered() {
            if let Some(s) = self.sessions.iter().find(|s| s.key == key) {
                all.push((s.title.clone(), "session", Act::Go(key)));
            }
        }
        all.retain(|(label, _, _)| q.is_empty() || label.to_lowercase().contains(&q));
        all.truncate(LINES);
        all
    }

    /// Do the line `n` of the palette.
    pub fn run_act(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).text();
        let Some((_, _, act)) = self.acts(&query).into_iter().nth(n) else {
            return;
        };
        self.palette = false;
        window.focus(&self.input.focus_handle(cx));
        match act {
            Act::NewSession => self.new_default(window, cx),
            Act::Scratch => self.new_scratch(window, cx),
            Act::AddProject => self.open_page(Page::NewProject, cx),
            Act::Settings => self.open_page(Page::Settings, cx),
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
        for (n, (label, hint, act)) in self.acts(&query).into_iter().enumerate() {
            let busy = matches!(act, Act::Go(k) if self.sessions.iter().any(|s| s.key == k && s.state == State::Working));
            list = list.child(
                div()
                    .id(SharedString::from(format!("act-{n}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .h(px(32.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(n == 0, |d| d.bg(theme::panel2()))
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::one_line(ui::text(faces, label).text_size(px(13.5))).flex_1())
                    .when(busy, |d| d.child(ui::live_dot()))
                    .child(
                        ui::text(faces, hint)
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme::faint()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.run_act(n, window, cx))),
            );
        }
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
                        .w(px(560.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .px(px(14.))
                                .h(px(44.))
                                .border_b_1()
                                .border_color(theme::border2())
                                .child(ui::icon("search", 16.).text_color(theme::faint()))
                                .child(div().flex_1().child(self.palette_input.clone())),
                        )
                        .child(list),
                )
                .into_any_element(),
        )
    }
}
