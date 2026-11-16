//! The column down the left, laid out the way a coding agent's app has it:
//! the controls beside the window's lights, New task and Search, your
//! sessions with the solver on this machine with the parts of their open
//! work under them, and at the foot the solver and the brain it thinks
//! with. Every row is a session you opened, the solver's own state read
//! from its journal, or a control that does what it says; there are no
//! entries for things that do not work yet.

use gpui::{
    AnyElement, Context, Corner, Div, FontWeight, SharedString, Stateful, div, prelude::*, px,
};

use crate::{
    desk::{At, Desk, LEFT, Menu, TOPBAR, draggable},
    fonts::Faces,
    model::LinkState,
    settings::{self, Session},
    solver::Provider,
    theme,
    thread::{self, State, Task},
    ui,
};

/// A row's height in the sidebar.
const ROW: f32 = 34.;
/// A part's row under its session: a little shorter, so that it reads as
/// belonging to the row above.
const PART_ROW: f32 = 28.;
/// How far a part's row is indented from its session's.
const INDENT: f32 = 18.;
/// The key the "This machine" group is folded under.
const MACHINE: &str = "group:machine";

/// The colour of the daemon link's dot: green when connected, grey while
/// connecting, coral when lost.
pub fn link_color(link: &LinkState) -> gpui::Hsla {
    match link {
        LinkState::Connected => theme::green(),
        LinkState::Connecting => theme::muted(),
        LinkState::Lost(_) => theme::coral(),
    }
}

/// A row of the sidebar's menu: an icon and a word, in a pill on hover.
fn menu_row(
    id: &'static str,
    faces: &Faces,
    icon: &'static str,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .h(px(ROW))
        .px(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(ui::icon(icon, 17.).text_color(theme::muted()))
        .child(
            ui::text(faces, label)
                .text_size(px(14.))
                .text_color(theme::text2()),
        )
}

/// The thin └ that ties a part to the session above it: a 1-pixel line
/// down from the row's top and across at its middle. A part with another
/// after it carries the line on down, so the parts read as one branch.
fn connector(last: bool) -> Div {
    let mid = PART_ROW / 2.;
    div()
        .relative()
        .flex_none()
        .w(px(10.))
        .h(px(PART_ROW))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w(px(1.))
                .h(px(if last { mid } else { PART_ROW }))
                .bg(theme::ghost_line()),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(mid - 0.5))
                .w(px(10.))
                .h(px(1.))
                .bg(theme::ghost_line()),
        )
}

impl Desk {
    /// The column, with `lights` of room at the left of its top row.
    pub(crate) fn left(
        &self,
        faces: &Faces,
        lights: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // The window's lights sit in this row; the controls follow them.
        let top = draggable()
            .flex()
            .flex_none()
            .items_center()
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(10.))
            .child(self.controls(cx));

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LEFT))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .child(top)
            .child(self.menu_rows(faces, cx))
            .child(self.session_list(faces, cx))
            .child(self.account(faces, cx))
    }

    /// New task, and Search, which turns into a field when clicked.
    fn menu_rows(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let search: AnyElement = if self.searching {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .h(px(ROW))
                .px(px(10.))
                .rounded(px(8.))
                .bg(theme::panel2())
                .child(ui::icon("search", 17.).text_color(theme::text2()))
                .child(div().flex_1().min_w(px(0.)).child(self.search.clone()))
                .into_any_element()
        } else {
            menu_row("search", faces, "search", "Search")
                .on_click(cx.listener(|this, _, window, cx| this.open_search(window, cx)))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(8.))
            .pb(px(6.))
            .child(
                menu_row("new", faces, "plus", "New task")
                    .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
            )
            .child(search)
    }

    /// The sessions on this machine, under a head that folds them away;
    /// while searching, only those the search finds, folded or not.
    fn session_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.searching || !self.collapsed.contains(MACHINE);
        let mut list = div()
            .id("sessions")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
            .child(group_head(faces, open, cx));
        if !open {
            return list;
        }
        let found = if self.searching {
            settings::matching(&self.settings.sessions, &self.search.read(cx).text())
        } else {
            (0..self.settings.sessions.len()).collect()
        };
        if found.is_empty() {
            list = list.child(
                ui::text(faces, "No session is called that.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .px(px(10.))
                    .py(px(8.)),
            );
        }
        for i in found {
            let session = &self.settings.sessions[i];
            let tasks = thread::session_tasks(&self.solver.journal, &session.id);
            list = list.child(self.session_row(faces, i, session, &tasks, cx));
            let parts = thread::open_parts(&tasks);
            let count = parts.len();
            for (n, part) in parts.into_iter().enumerate() {
                list = list.child(part_row(faces, i, part, n + 1 == count, cx));
            }
        }
        list
    }

    /// One session: its name, and on the right a dot only when its work is
    /// running (amber) or waiting on you (purple).
    fn session_row(
        &self,
        faces: &Faces,
        index: usize,
        s: &Session,
        tasks: &[Task<'_>],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on = index == self.current;
        let label_color = if s.title.is_empty() {
            theme::muted()
        } else if on {
            theme::text()
        } else {
            theme::text2()
        };
        div()
            .id(SharedString::from(format!("session-{}", s.id)))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(ROW))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .when(!on, |d| d.hover(|x| x.bg(theme::hover())))
            .child(ui::one_line(
                ui::text(faces, s.label())
                    .text_size(px(14.))
                    .text_color(label_color),
            ))
            .children(thread::session_state(tasks).map(|st| {
                ui::dot(match st {
                    State::Waiting => theme::purple(),
                    _ => theme::accent(),
                })
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }

    /// The solver at the foot of the column, the way a coding agent's app
    /// shows the account: who, the brain it thinks with (which opens the
    /// brain menu), and whether the daemon is there.
    fn account(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let brain = self
            .current_provider()
            .as_ref()
            .map_or_else(|| "no brain".to_owned(), Provider::label);
        let avatar = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_full()
            .bg(theme::panel2())
            .border_1()
            .border_color(theme::border2())
            .child(ui::icon("ghost", 13.).text_color(theme::text2()));
        let who = div()
            .id("account")
            .flex()
            .items_center()
            .gap(px(6.))
            .flex_1()
            .min_w(px(0.))
            .h(px(36.))
            .px(px(6.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(self.menu_open_at(At::Account), |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(
                ui::text(faces, self.solver.name())
                    .text_size(px(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .flex_none(),
            )
            .child(ui::one_line(
                ui::mono(faces, format!("· {brain}"))
                    .text_size(px(11.5))
                    .text_color(theme::muted()),
            ))
            .child(ui::icon("chevron-down", 12.).text_color(theme::muted()))
            .on_click(cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, At::Account, cx)));
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(54.))
            .px(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(avatar)
            .child(who)
            .child(ui::dot(link_color(&self.solver.link)).mr(px(4.)))
            .children(
                self.menu_over(At::Account, Corner::BottomLeft, faces, cx)
                    .map(|menu| div().absolute().top(px(-4.)).left(px(8.)).child(menu)),
            )
    }
}

/// A part of a session's open work, under the session: the └, its state,
/// its brief. Clicking it opens the session it belongs to.
fn part_row(
    faces: &Faces,
    session: usize,
    part: &Task<'_>,
    last: bool,
    cx: &mut Context<Desk>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("part-{session}-{}", part.id)))
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(PART_ROW))
        .pl(px(INDENT))
        .pr(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(connector(last))
        .child(ui::dot(part.state().color()))
        .child(ui::one_line(
            ui::text(faces, part.brief.clone())
                .text_size(px(13.))
                .text_color(theme::muted()),
        ))
        .on_click(cx.listener(move |this, _, _, cx| this.choose(session, cx)))
}

/// "This machine", with a chevron that says whether it is folded.
fn group_head(faces: &Faces, open: bool, cx: &mut Context<Desk>) -> Stateful<Div> {
    div()
        .id("group-machine")
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .mt(px(10.))
        .px(px(10.))
        .rounded(px(6.))
        .cursor_pointer()
        .child(
            ui::text(faces, "This machine")
                .text_size(px(12.5))
                .text_color(theme::muted()),
        )
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
        .on_click(cx.listener(|this, _, _, cx| this.toggle_collapsed(MACHINE, cx)))
}
