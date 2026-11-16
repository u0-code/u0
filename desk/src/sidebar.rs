//! The column down the left, laid out the way a coding agent's app has it:
//! the controls beside the window's lights, New task, Search and Attention
//! with how many things wait on you that you have not seen, then a group
//! for each daemon the desk follows, and at the foot Ghost and
//! the brain it thinks with.
//!
//! The first group is this machine: your sessions with Ghost, with
//! the parts of their open work under them, and any other character its
//! core runs. Then one group for each connection, its name and how the
//! link to it stands, with the characters it runs, each with a dot for
//! whether her day has her awake. Every group folds. At the end, a small
//! form adds a connection; a connection's `…`, or a right click on it,
//! removes it. Every row is a session you opened, a character a daemon
//! says it runs, or a control that does what it says; there are no entries
//! for things that do not work yet.

use ghost_link::protocol::Listed;
use gpui::{
    AnyElement, Context, Corner, Div, FontWeight, MouseButton, SharedString, Stateful, anchored,
    deferred, div, prelude::*, px,
};

use crate::{
    daemons::{self, Group, Host},
    desk::{At, Desk, LEFT, Menu, Popup, TOPBAR, draggable},
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

/// The colour of the daemon link's dot: green when connected, grey while
/// connecting, coral when lost.
pub fn link_color(link: &LinkState) -> gpui::Hsla {
    match link {
        LinkState::Connected => theme::green(),
        LinkState::Connecting => theme::muted(),
        // Down is a state, not an alarm: the right column says why.
        LinkState::Lost(_) => theme::faint(),
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
            .child(self.group_list(faces, cx))
            .child(self.account(faces, cx))
    }

    /// Attention, with a count of what you have not seen, when there is any.
    fn attention_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Stateful<Div> {
        let count = self.attention_count();
        menu_row("attention", faces, "bell", "Attention")
            .when(self.attending(), |d| d.bg(theme::panel2()))
            .when(count > 0, |d| {
                d.child(div().flex_1()).child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(18.))
                        .px(px(6.))
                        .rounded_full()
                        .bg(theme::purple().opacity(0.18))
                        .child(
                            ui::mono(faces, count.to_string())
                                .text_size(px(11.))
                                .text_color(theme::purple()),
                        ),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.open_attention(cx)))
    }

    /// New task, Search, which turns into a field when clicked, and
    /// Attention.
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
            .child(self.attention_row(faces, cx))
    }

    /// A group for each daemon, each under a head that folds it away, and
    /// the way to add another. While searching, only this machine's
    /// sessions the search finds, folded or not.
    fn group_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut list = div()
            .id("groups")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.));
        let solver = self.solver.card.as_ref().map(|c| c.seal.as_str());
        for group in daemons::groups(&self.daemons, solver) {
            let local = *group.host == Host::Local;
            if self.searching && !local {
                continue;
            }
            let open = (self.searching && local) || !self.collapsed.contains(&group.host.key());
            list = list.child(self.group_head(faces, &group, open, cx));
            if !open {
                continue;
            }
            if local {
                list = self.sessions(list, faces, cx);
                if self.searching {
                    continue;
                }
            } else if group.ghosts.is_empty() {
                let why = match group.link {
                    LinkState::Connecting => "Connecting…".to_owned(),
                    LinkState::Connected => "It runs nobody.".to_owned(),
                    LinkState::Lost(why) => crate::model::reason(why, group.host.label()),
                };
                let down = matches!(group.link, LinkState::Lost(_));
                let host = group.host.clone();
                list = list.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .child(
                            ui::text(faces, why)
                                .text_size(px(12.5))
                                .line_height(px(17.))
                                .text_color(theme::muted())
                                .flex_1()
                                .min_w(px(0.)),
                        )
                        .when(down, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("{}:retry", host.key())))
                                    .flex_none()
                                    .cursor_pointer()
                                    .child(
                                        ui::text(faces, "Retry")
                                            .text_size(px(12.5))
                                            .line_height(px(17.))
                                            .text_color(theme::text2()),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.retry_host(&host, cx);
                                    })),
                            )
                        }),
                );
            }
            for her in &group.ghosts {
                list = list.child(self.character_row(faces, group.host, her, cx));
            }
        }
        if self.searching {
            return list;
        }
        list.child(add_connection_row(faces, cx))
            .children(self.form.open.then(|| self.connection_form(faces, cx)))
    }

    /// This machine's sessions, or those the search finds, each with the
    /// parts of its open work under it.
    fn sessions(
        &self,
        mut list: Stateful<Div>,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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
        let on = index == self.current && self.open.is_none() && !self.attending();
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

    /// Ghost at the foot of the column, the way a coding agent's
    /// app shows the account: who, the brain it thinks with (which opens the
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

impl Desk {
    /// A group's head: its name, a chevron that says whether it is folded,
    /// and for a connection the link's dot and a `…` that shows on hover
    /// and opens its menu, as a right click does.
    fn group_head(
        &self,
        faces: &Faces,
        group: &Group<'_>,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = group.host.key();
        let fold = key.clone();
        let head = div()
            .id(SharedString::from(key.clone()))
            .group(SharedString::from(key.clone()))
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .mt(px(10.))
            .px(px(10.))
            .rounded(px(6.))
            .cursor_pointer()
            .child(
                ui::text(faces, group.host.label().to_owned())
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
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(&fold, cx)));
        let Host::Remote(name) = group.host else {
            return div().child(head);
        };
        let (menu_for, right_for) = (name.clone(), name.clone());
        let head = head
            .child(div().flex_1())
            .child(ui::dot(link_color(group.link)))
            .child(
                div()
                    .id(SharedString::from(format!("{key}:more")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .rounded(px(5.))
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        ui::mono(faces, "…")
                            .text_color(gpui::transparent_black())
                            .group_hover(SharedString::from(key.clone()), |s| {
                                s.text_color(theme::muted())
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.popup = Popup::Connection(menu_for.clone());
                        cx.notify();
                    })),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    this.popup = Popup::Connection(right_for.clone());
                    cx.notify();
                }),
            );
        let mut wrap = div().relative().child(head);
        if self.popup == Popup::Connection(name.clone()) {
            let gone = name.clone();
            let menu = div()
                .id("connection-menu")
                .occlude()
                .w(px(200.))
                .p(px(6.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .rounded(px(10.))
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.popup = Popup::Closed;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("remove-connection")
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(
                            ui::text(faces, "Remove connection")
                                .text_size(px(13.))
                                .text_color(theme::coral()),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.remove_connection(&gone, cx)),
                        ),
                );
            // Under the head, its right edge on the head's.
            wrap = wrap.child(
                div().absolute().top(px(40.)).right_0().child(deferred(
                    anchored()
                        .anchor(Corner::TopRight)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu),
                )),
            );
        }
        wrap
    }

    /// A character a daemon runs: a dot for whether her day has her awake
    /// (green) or asleep (faint), none while her mind does not say; and her
    /// name. Clicking it opens her.
    fn character_row(
        &self,
        faces: &Faces,
        host: &Host,
        her: &Listed,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on = !self.attending()
            && self
                .open
                .as_ref()
                .is_some_and(|o| o.host == *host && o.id == her.id);
        let dot = match her.awake {
            Some(true) => theme::green(),
            Some(false) => theme::faint(),
            None => gpui::transparent_black(),
        };
        let (host, id) = (host.clone(), her.id.clone());
        div()
            .id(SharedString::from(format!("{}:{}", host.key(), her.id)))
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(ROW))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .when(!on, |d| d.hover(|x| x.bg(theme::hover())))
            .child(ui::dot(dot))
            .child(ui::one_line(
                ui::text(faces, her.card.name.clone())
                    .text_size(px(14.))
                    .text_color(if on { theme::text() } else { theme::text2() }),
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.open_character(&host, &id, cx)))
    }

    /// The form for a connection: its name, its URL, and the name of the
    /// variable its token is in, never the token. Under it, why the last
    /// try was refused.
    fn connection_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let field = |label: &'static str, n: usize| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    ui::mono(faces, label)
                        .text_size(px(10.5))
                        .text_color(theme::faint()),
                )
                .child(
                    div()
                        .px(px(8.))
                        .rounded(px(7.))
                        .bg(theme::bg())
                        .border_1()
                        .border_color(theme::border2())
                        .child(self.form.fields[n].clone()),
                )
        };
        let button = |id: &'static str, label: &'static str, strong: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(26.))
                .px(px(10.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(strong, |d| d.bg(theme::accent()))
                .when(!strong, |d| d.hover(|s| s.bg(theme::hover())))
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(if strong { theme::bg() } else { theme::muted() }),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mx(px(4.))
            .mt(px(4.))
            .p(px(10.))
            .rounded(px(10.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border())
            .child(field("NAME", 0))
            .child(field("URL", 1))
            .child(field("TOKEN VARIABLE", 2))
            .children(self.form.refused.as_ref().map(|why| {
                ui::text(faces, why.clone())
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(theme::coral())
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(button("connection-cancel", "Cancel", false).on_click(
                        cx.listener(|this, _, window, cx| this.toggle_adding(window, cx)),
                    ))
                    .child(
                        button("connection-add", "Connect", true)
                            .on_click(cx.listener(|this, _, _, cx| this.add_connection(cx))),
                    ),
            )
    }
}

/// The row that opens the form for a connection, or closes it.
fn add_connection_row(faces: &Faces, cx: &mut Context<Desk>) -> Stateful<Div> {
    div()
        .id("add-connection")
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .mt(px(12.))
        .px(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(ui::icon("plus", 14.).text_color(theme::muted()))
        .child(
            ui::text(faces, "Add connection")
                .text_size(px(13.))
                .text_color(theme::muted()),
        )
        .on_click(cx.listener(|this, _, window, cx| this.toggle_adding(window, cx)))
}
