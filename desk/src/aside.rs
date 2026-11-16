//! The column down the right: where Ghost runs, the context it
//! works in, the work taken on in this session, and the files that work
//! handed over. With a character open, her daemon and its link, her clock
//! and the brain she thinks with, and her work as her daemon keeps it.
//! Work that waits on you shows what it asks, to be answered here, and
//! open work can be stopped (see `answer.rs`). Sections
//! are told apart by full-width hairlines and rows keep one height, the
//! way a coding agent's side panel reads. Nothing here is drawn without
//! data behind it: a section with nothing to say says so, and Outputs is
//! left out until there is a file to show.

use std::path::PathBuf;

use chrono::Utc;
use ghost_link::protocol::WorkState;
use gpui::{
    AnyElement, Context, Corner, Div, FontWeight, SharedString, Stateful, div, prelude::*, px,
};

use crate::{
    answer::{Ask, Whom},
    brains::Mark,
    desk::{At, Desk, Menu, Open, RIGHT, Watched},
    fonts::Faces,
    model::{LinkState, Reach, reason},
    settings::Session,
    theme,
    thread::{self, Output, Source, State, Task},
    ui,
};

/// A row's height in a section.
const ROW: f32 = 31.;
/// Room an icon and its gap take, for a second line to start under the label.
const ICON_ROOM: f32 = 26.;
/// How many pieces of work show before "Show more".
const WORK_ROWS: usize = 5;

/// A label in a row.
fn label(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(13.5))
        .text_color(theme::text2())
}

/// A value at the right of a row.
fn value(faces: &Faces, text: impl Into<SharedString>, color: gpui::Hsla) -> AnyElement {
    ui::mono(faces, text)
        .text_size(px(12.))
        .text_color(color)
        .flex_none()
        .into_any_element()
}

/// A small action at the right of a row, which does what it says.
fn action(faces: &Faces, id: &'static str, text: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(22.))
        .px(px(7.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(
            ui::text(faces, text)
                .text_size(px(12.5))
                .text_color(theme::muted()),
        )
}

/// The one line of a row: an icon if it has one, the label, and what is
/// on the right.
fn line(icon: Option<AnyElement>, label: Div, right: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .children(icon)
        .child(ui::one_line(label))
        .children(right)
}

/// A row of one line.
fn row(icon: Option<AnyElement>, label: Div, right: Option<AnyElement>) -> Div {
    line(icon, label, right).h(px(ROW))
}

/// A row with a second line of small print under its label.
fn row_with(icon: Option<AnyElement>, label: Div, right: Option<AnyElement>, under: Div) -> Div {
    let indent = if icon.is_some() { ICON_ROOM } else { 0. };
    div()
        .flex()
        .flex_col()
        .py(px(5.))
        .child(line(icon, label, right).h(px(22.)))
        .child(under.pl(px(indent)))
}

/// Small print under a label.
fn under(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::one_line(
        ui::mono(faces, text)
            .text_size(px(11.5))
            .text_color(theme::faint()),
    )
}

/// A muted icon at the head of a row.
fn row_icon(name: &str) -> AnyElement {
    ui::icon(name, 16.)
        .text_color(theme::muted())
        .flex_none()
        .into_any_element()
}

/// Where a piece of work stands, as a glyph: a ring while it is open, a
/// dot once it has ended, each in its state's colour.
pub(crate) fn glyph(state: State) -> Div {
    match state {
        State::Running | State::Waiting => div()
            .flex_none()
            .size(px(9.))
            .rounded_full()
            .border_2()
            .border_color(state.color()),
        _ => ui::dot(state.color()).size(px(7.)).m(px(1.)),
    }
}

/// Where a piece of work her daemon keeps stands, in the desk's colours: a
/// piece resting between sittings is still under way.
pub(crate) fn state_of(state: WorkState) -> State {
    match state {
        WorkState::Open | WorkState::Running => State::Running,
        WorkState::Waiting => State::Waiting,
        WorkState::Finished => State::Finished,
        WorkState::Blocked | WorkState::GaveUp => State::Blocked,
        WorkState::Dropped | WorkState::Other => State::Dropped,
    }
}

/// Whose daemon a connection card is for, so its buttons know what to do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Whose {
    /// The solver's: on this machine, the core the desk looks after.
    Solver,
    /// The open character's.
    Hers,
}

/// How a link stands, in a small pill: a dot when it is up, a ring while
/// it is not. Down is grey, not red: it is a state to act on, and the card
/// says why and what to do.
fn pill(faces: &Faces, link: &LinkState, reach: &Reach) -> AnyElement {
    let (word, color, up) = match (link, &reach.why) {
        (LinkState::Connected, _) => ("online", theme::green(), true),
        (LinkState::Connecting, None) => ("connecting", theme::muted(), false),
        (LinkState::Connecting, Some(_)) => ("retrying", theme::muted(), false),
        (LinkState::Lost(_), _) => ("offline", theme::faint(), false),
    };
    let mark = if up {
        ui::dot(color).size(px(7.))
    } else {
        div()
            .flex_none()
            .size(px(7.))
            .rounded_full()
            .border_1()
            .border_color(color)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(20.))
        .px(px(8.))
        .rounded_full()
        .border_1()
        .border_color(theme::border())
        .bg(theme::well())
        .child(mark)
        .child(ui::mono(faces, word).text_size(px(11.)).text_color(if up {
            theme::text2()
        } else {
            theme::muted()
        }))
        .into_any_element()
}

/// A button in a card: an outline, an icon and what it does.
fn button(faces: &Faces, id: String, icon: &str, text: &'static str) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .h(px(24.))
        .px(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme::border2())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(ui::icon(icon, 13.).text_color(theme::muted()))
        .child(
            ui::text(faces, text)
                .text_size(px(12.5))
                .text_color(theme::text2()),
        )
}

/// The small print under a daemon's name: where it is, and for how long
/// it has been up or down.
fn reach_line(host: &str, link: &LinkState, reach: &Reach) -> String {
    let span = thread::span(i64::try_from(reach.since.elapsed().as_secs()).unwrap_or(i64::MAX));
    match link {
        LinkState::Connected => format!("{host} · up {span}"),
        _ if reach.why.is_none() => host.to_owned(),
        _ if reach.ever => format!("{host} · down {span}"),
        _ => format!("{host} · never reached"),
    }
}

impl Desk {
    /// A daemon's connection, as a card: what the desk calls it, a pill
    /// for how it stands, where it is and for how long. While it cannot be
    /// reached, why, in words, and what can be done: try again now, start
    /// the core when it is the one on this machine, and the error itself
    /// under Details.
    fn daemon_card(
        &self,
        faces: &Faces,
        whose: Whose,
        name: &str,
        url: &str,
        watched: &Watched,
        cx: &mut Context<Self>,
    ) -> Div {
        let host = url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .trim_end_matches('/')
            .to_owned();
        let (link, reach) = (&watched.link, &watched.reach);
        let card = row_with(
            Some(row_icon("server")),
            label(faces, name.to_owned()),
            Some(pill(faces, link, reach)),
            under(faces, reach_line(&host, link, reach)),
        );
        let Some(why) = reach.why.as_ref().filter(|_| *link != LinkState::Connected) else {
            return card;
        };
        let tag = match whose {
            Whose::Solver => "solver",
            Whose::Hers => "hers",
        };
        let key = format!("link:{url}");
        let open = self.expanded.contains(&key);
        let retrying = *link == LinkState::Connecting;
        let tries = match reach.tries {
            1 => "Tried once".to_owned(),
            n => format!("Tried {n} times"),
        };
        let next = if retrying {
            "trying again now"
        } else {
            "trying again on its own"
        };
        let retry = button(faces, format!("{tag}-retry"), "retry", "Retry now").on_click(
            cx.listener(move |this, _, _, cx| match whose {
                Whose::Solver => this.retry_solver(cx),
                Whose::Hers => this.retry_her(cx),
            }),
        );
        let start = (whose == Whose::Solver && self.core_is_local()).then(|| {
            let word = if self.deployment.started() {
                "Restart core"
            } else {
                "Start core"
            };
            button(faces, format!("{tag}-start"), "power", word)
                .on_click(cx.listener(|this, _, _, cx| this.start_core(cx)))
        });
        let details = div()
            .id(SharedString::from(format!("{tag}-details")))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(3.))
            .h(px(24.))
            .px(px(4.))
            .cursor_pointer()
            .child(
                ui::text(faces, "Details")
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
                    11.,
                )
                .text_color(theme::faint()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)));
        card.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pl(px(ICON_ROOM))
                .pt(px(6.))
                .pb(px(2.))
                .child(
                    ui::text(faces, reason(why, &host))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(theme::text2()),
                )
                .child(
                    ui::mono(faces, format!("{tries} · {next}"))
                        .text_size(px(11.5))
                        .text_color(theme::faint()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .child(retry)
                        .children(start)
                        .child(details),
                )
                .when(open, |d| {
                    d.child(
                        ui::mono(faces, why.clone())
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::muted())
                            .p(px(8.))
                            .rounded(px(6.))
                            .bg(theme::well())
                            .border_1()
                            .border_color(theme::border()),
                    )
                }),
        )
    }

    pub(crate) fn right(&self, faces: &Faces, cx: &mut Context<Self>) -> AnyElement {
        let column = div()
            .id("aside")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(RIGHT))
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme::border());
        if let Some(open) = &self.open {
            let work = &open.her.work;
            let environment = self.her_environment(faces, open, cx);
            let body = self.her_work(faces, work, cx);
            return column
                .child(self.section(faces, "Environment", None, None, environment, cx))
                .child(self.section(
                    faces,
                    "Work",
                    None,
                    (!work.is_empty()).then(|| work.len().to_string()),
                    body,
                    cx,
                ))
                .into_any_element();
        }
        let session = self.session();
        let tasks = thread::session_tasks(&self.solver.journal, &session.id);
        let outputs = thread::outputs(&self.solver.journal, &session.id);
        let environment = self.environment(faces, cx);
        let context = self.context(faces);
        let work = self.work(faces, &session, &tasks, cx);
        column
            .child(self.section(faces, "Environment", None, None, environment, cx))
            .child(self.section(faces, "Context", Some("section:context"), None, context, cx))
            .child(self.section(
                faces,
                "Work",
                None,
                (!tasks.is_empty()).then(|| tasks.len().to_string()),
                work,
                cx,
            ))
            .when(!outputs.is_empty(), |d| {
                let count = outputs.len().to_string();
                let body = self.outputs(faces, &outputs, cx);
                d.child(self.section(faces, "Outputs", None, Some(count), body, cx))
            })
            .into_any_element()
    }

    /// A section: its head (with a chevron if it folds, and an aside on
    /// the right), its body, and a hairline under it.
    fn section(
        &self,
        faces: &Faces,
        title: &'static str,
        fold: Option<&'static str>,
        aside: Option<String>,
        body: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = fold.is_none_or(|k| !self.collapsed.contains(k));
        let head = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(24.))
            .child(
                ui::text(faces, title)
                    .text_size(px(13.5))
                    .font_weight(FontWeight::MEDIUM),
            )
            .children(fold.map(|_| {
                ui::icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.,
                )
                .text_color(theme::faint())
            }))
            .child(div().flex_1())
            .children(aside.map(|a| ui::mono(faces, a).text_size(px(11.5))));
        let head: AnyElement = match fold {
            Some(key) => head
                .id(key)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(key, cx)))
                .into_any_element(),
            None => head.into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(16.))
            .py(px(14.))
            .border_b_1()
            .border_color(theme::border())
            .child(head)
            .when(open, |d| d.child(body.mt(px(6.))))
    }

    /// Where Ghost runs: the machine its work uses, the daemon,
    /// and the brain.
    fn environment(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .child(self.machine_row(faces, cx))
            .child(self.daemon_card(
                faces,
                Whose::Solver,
                "Daemon",
                &self.settings.solver_url,
                &self.solver,
                cx,
            ))
            .child(self.brain_row(faces, cx))
    }

    /// The machine Ghost's work runs on. On this one, its home, which
    /// Open shows in Finder; anywhere else, what the deployment names.
    fn machine_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        if !self.abilities.local() {
            let what = match (&self.abilities.machine, self.abilities.work) {
                (Some(m), true) => m.clone(),
                _ => "none".to_owned(),
            };
            return row(
                Some(row_icon("laptop")),
                label(faces, "Machine"),
                Some(value(faces, what, theme::muted())),
            );
        }
        let home = self.deployment.home();
        let you = std::env::var_os("HOME").map(PathBuf::from);
        let shown = ui::short_path(&home, you.as_deref());
        let open = action(faces, "open-machine", "Open")
            .on_click(cx.listener(move |this, _, _, cx| this.reveal(&home, cx)));
        row_with(
            Some(row_icon("laptop")),
            label(faces, "This machine"),
            Some(open.into_any_element()),
            under(faces, shown),
        )
    }

    /// The brain it thinks with, by its mark; Change opens the brain menu
    /// here. When the deployment's file names none the desk knows, the
    /// daemon's own word for it stands.
    fn brain_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let provider = self.current_provider();
        let told = self
            .solver
            .status
            .as_ref()
            .map(|s| s.think.clone())
            .filter(|t| !t.is_empty());
        let (mark, name) = match (&provider, told) {
            (Some(p), _) => (ui::mark(faces, p.mark(), 16.), p.label()),
            (None, Some(told)) => (ui::mark(faces, Mark::from_words(&told), 16.), told),
            (None, None) => (row_icon("plug"), "No brain chosen".to_owned()),
        };
        let change =
            action(faces, "change-brain", "Change").on_click(cx.listener(|this, _, window, cx| {
                this.open_menu(Menu::Brains, At::Aside, window, cx);
            }));
        row(
            Some(mark),
            label(faces, name),
            Some(change.into_any_element()),
        )
        .relative()
        .children(
            self.menu_over(At::Aside, Corner::TopRight, faces, cx)
                .map(|menu| div().absolute().top(px(ROW)).right_0().child(menu)),
        )
    }

    /// What Ghost knows of its own life and of you: the journal, its
    /// tempo, your sessions, and you as its operator.
    fn context(&self, faces: &Faces) -> Div {
        let status = self.solver.status.as_ref();
        let pair = |name: &'static str, v: String| {
            row(
                None,
                label(faces, name),
                Some(value(faces, v, theme::text())),
            )
        };
        let dash = || "—".to_owned();
        let operator = self.solver.heard_from(self.who()).map_or_else(
            || "not yet known".to_owned(),
            |heard| format!("{} · {heard} lines", self.who()),
        );
        div()
            .flex()
            .flex_col()
            .child(pair(
                "Journal",
                status.map_or_else(dash, |s| format!("{} events", s.journal)),
            ))
            .child(pair(
                "Tempo",
                status.map_or_else(dash, |s| format!("{:.2}", s.tempo)),
            ))
            .child(pair("Sessions", self.settings.sessions.len().to_string()))
            .child(pair("Operator", operator))
    }

    /// The work taken on in this session, newest first: five, then the
    /// rest when asked for.
    fn work(
        &self,
        faces: &Faces,
        session: &Session,
        tasks: &[Task<'_>],
        cx: &mut Context<Self>,
    ) -> Div {
        if tasks.is_empty() {
            return div().child(
                ui::text(faces, "Nothing taken on in this session.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        let key = format!("more:{}", session.id);
        let all = self.expanded.contains(&key);
        let (count, hidden) = thread::shown(tasks.len(), WORK_ROWS, all);
        let mut list = div().flex().flex_col().gap(px(2.));
        for t in tasks.iter().rev().take(count) {
            list = list.child(work_row(faces, t, cx));
            // What its daemon says of it, when it says: what it asks, and
            // a way to stop it while it is open.
            if let Some(w) = self.solver.work.iter().find(|w| w.id == t.id) {
                let ask = Ask::of(Whom::Solver, w);
                if ask.open {
                    list = list.child(self.ask_controls(faces, &ask, cx).pl(px(19.)).pb(px(6.)));
                }
            }
        }
        let more = if hidden > 0 {
            Some(format!("Show {hidden} more"))
        } else if all && tasks.len() > WORK_ROWS {
            Some("Show fewer".to_owned())
        } else {
            None
        };
        if let Some(more) = more {
            list = list.child(
                div()
                    .id("work-more")
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, more)
                            .text_size(px(12.5))
                            .text_color(theme::muted()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx))),
            );
        }
        list
    }

    /// The files this session's work handed over; each shows in Finder.
    fn outputs(&self, faces: &Faces, outputs: &[Output], cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col();
        for (i, o) in outputs.iter().enumerate() {
            let path = self.output_path(o);
            list = list.child(
                row(
                    Some(row_icon("file")),
                    label(faces, o.name().to_owned()),
                    Some(value(faces, o.kind(), theme::faint())),
                )
                .id(("output", i))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.reveal(&path, cx))),
            );
        }
        list
    }

    /// Where a handed-over file is on this machine: a report names a path
    /// on the solver's machine (relative ones are under its home), and a
    /// kept file is in the ghost's `media/`.
    fn output_path(&self, o: &Output) -> PathBuf {
        match o.source {
            Source::Report => {
                let p = PathBuf::from(&o.file);
                if p.is_absolute() {
                    p
                } else {
                    self.deployment.home().join(p)
                }
            }
            Source::Kept => self.deployment.ghost_dir().join("media").join(&o.file),
        }
    }
}

impl Desk {
    /// Where she is: her daemon and its link, her clock and her city, and
    /// the brain she thinks with, as her daemon says.
    fn her_environment(&self, faces: &Faces, open: &Open, cx: &mut Context<Self>) -> Div {
        let status = open.her.status.as_ref();
        let dash = || "—".to_owned();
        div()
            .flex()
            .flex_col()
            .child(self.daemon_card(
                faces,
                Whose::Hers,
                open.host.label(),
                &open.url,
                &open.her,
                cx,
            ))
            .child(row_with(
                Some(row_icon("ghost")),
                label(faces, "Her clock"),
                Some(value(
                    faces,
                    status.map_or_else(dash, crate::model::Status::hhmm),
                    theme::text(),
                )),
                under(faces, status.map_or_else(dash, |s| s.city.clone())),
            ))
            .child({
                let think = status.map(|s| s.think.clone()).filter(|t| !t.is_empty());
                let mark = think.as_deref().map_or_else(
                    || row_icon("plug"),
                    |t| ui::mark(faces, Mark::from_words(t), 16.),
                );
                row(
                    Some(mark),
                    label(faces, "Brain"),
                    Some(value(faces, think.unwrap_or_else(dash), theme::muted())),
                )
            })
    }
}

/// A piece of work: where it stands, its brief, and in figures how far it
/// has gone and when it last moved. Clicking it opens its card in the
/// thread.
fn work_row(faces: &Faces, t: &Task<'_>, cx: &mut Context<Desk>) -> Stateful<Div> {
    let ago = thread::span((Utc::now() - t.changed()).num_seconds());
    let figures = format!("{} · {ago} ago", t.figures());
    let key = format!("task:{}", t.id);
    div()
        .id(SharedString::from(format!("work-{}", t.id)))
        .flex()
        .items_start()
        .gap(px(10.))
        .py(px(6.))
        .cursor_pointer()
        .child(div().pt(px(5.)).child(glyph(t.state())))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w(px(0.))
                .child(ui::one_line(
                    ui::text(faces, t.brief.clone()).text_size(px(13.5)),
                ))
                .child(ui::one_line(
                    ui::mono(faces, figures)
                        .text_size(px(11.5))
                        .text_color(theme::muted()),
                )),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
}
