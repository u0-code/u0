//! The column down the right: where Ghost runs, what fills the context of
//! the brains at work here (`aside/context.rs`), the work taken on in this session, and the files that work
//! handed over. With a character open, her daemon and its link, her clock
//! and the brain she thinks with, and her work as her daemon keeps it.
//! Work that waits on you shows what it asks, to be answered here, and
//! open work can be stopped (see `answer.rs`). Sections
//! are told apart by full-width hairlines and rows keep one height, the
//! way a coding agent's side panel reads. Nothing here is drawn without
//! data behind it: a section with nothing to say says so, and Outputs is
//! left out until there is a file to show.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use ghost_link::protocol::{Abilities, Work, WorkState};
use gpui::{
    AnyElement, Context, Corner, Div, FontWeight, SharedString, Stateful, div, prelude::*, px,
};

mod context;

pub(crate) use self::context::context_work;
use crate::{
    answer::{Ask, Whom},
    brains::Mark,
    desk::{At, Desk, Menu, Open, RIGHT, Watched},
    fonts::Faces,
    media::{self, Shown},
    model::{LinkState, Reach, reason},
    sessions::Session,
    theme,
    thread::{self, Output, Source, Task},
    ui,
};

/// A row's height in a section.
const ROW: f32 = 22.5;
/// Room an icon and its gap take, for a second line to start under the label.
const ICON_ROOM: f32 = 26.;

/// A label in a row.
fn label(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(13.))
        .text_color(theme::text2())
}

/// A value at the right of a row.
fn value(faces: &Faces, text: impl Into<SharedString>, color: gpui::Hsla) -> AnyElement {
    ui::mono(faces, text)
        .text_size(px(11.5))
        .text_color(color)
        .flex_none()
        .into_any_element()
}

/// A small action at the right of a row, which does what it says.
fn action(faces: &Faces, id: impl Into<gpui::ElementId>, text: &'static str) -> Stateful<Div> {
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

/// One background task, on one line: what leads it (an icon, or its
/// state's glyph), its brief, and at the right how it stands in a word or
/// a time. Clicking it opens its card in the thread, where it has one.
pub(crate) fn task_row(
    faces: &Faces,
    id: impl Into<gpui::ElementId>,
    lead: AnyElement,
    brief: String,
    right: String,
    right_color: gpui::Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(28.))
        .child(lead)
        .child(ui::one_line(ui::text(faces, brief).text_size(px(13.))))
        .child(
            ui::mono(faces, right)
                .text_size(px(11.5))
                .text_color(right_color)
                .flex_none(),
        )
}

/// How a part stands, at the right of its row: waiting its turn, how long
/// it has run, or where it stands in the desk's word for it.
pub(crate) fn part_standing(part: &Work, now: DateTime<Utc>) -> String {
    let started = part
        .started
        .as_deref()
        .and_then(|t| t.parse::<DateTime<Utc>>().ok());
    match started {
        _ if !thread::under_way(part.state) => thread::state_word(part.state).to_owned(),
        None => "queued".to_owned(),
        Some(t) => thread::span((now - t).num_seconds()),
    }
}

/// The word a brain is shown by in a tag: the name of its model, or of the
/// agent it is, up to the first dash, so that `openai/deepseek-chat` reads
/// `deepseek` and an agent configured as `claude-code` reads `claude`. A
/// tag has the room of a word, and the first one says whose it is.
pub(crate) fn brain_word(brain: &str) -> &str {
    let name = brain.rsplit('/').next().unwrap_or(brain);
    name.split('-')
        .next()
        .filter(|word| !word.is_empty())
        .unwrap_or(name)
}

/// What a part runs on, at the right of its row or its card: a word in a
/// thin frame.
pub(crate) fn brain_tag(faces: &Faces, brain: &str) -> Div {
    ui::mono(faces, brain_word(brain).to_owned())
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme::tag_line())
        .text_size(px(11.5))
        .text_color(theme::faint())
}

/// One part a task handed out, under the task's row and set in by the room
/// of its icon, smaller than the task: its glyph, its brief, and how it
/// stands. A coding agent's panel shows the sub-agents of a task this way.
pub(crate) fn part_row(
    faces: &Faces,
    id: impl Into<gpui::ElementId>,
    part: &Work,
    now: DateTime<Utc>,
) -> Stateful<Div> {
    // A part waiting its turn is a quiet ring: nothing of it runs yet. A
    // part waits on its work, never on you, so it is never drawn waiting.
    let lead = match (part.state, &part.started) {
        (state, None) if !state.ended() => div()
            .flex_none()
            .size(px(9.))
            .rounded_full()
            .border_2()
            .border_color(theme::faint()),
        (WorkState::Waiting, Some(_)) => glyph(WorkState::Running),
        (state, _) => glyph(state),
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(24.))
        .pl(px(ICON_ROOM))
        .child(lead)
        .child(ui::one_line(
            ui::text(faces, part.brief.clone())
                .text_size(px(12.5))
                .text_color(theme::text2()),
        ))
        .children(part.brain.as_deref().map(|brain| brain_tag(faces, brain)))
        .child(
            ui::mono(faces, part_standing(part, now))
                .text_size(px(11.5))
                .text_color(theme::faint())
                .flex_none(),
        )
}

/// What leads a task still going: the terminal, as it runs on a machine.
pub(crate) fn running_lead() -> AnyElement {
    row_icon("terminal")
}

/// How a task still going stands, at the right of its row: how long it
/// has run, or that it waits on you.
pub(crate) fn open_standing(
    state: WorkState,
    started: Option<DateTime<Utc>>,
) -> (String, gpui::Hsla) {
    match state {
        WorkState::Waiting => (thread::state_word(state).to_owned(), theme::accent()),
        _ => (
            started.map_or_else(String::new, |t| {
                thread::span((Utc::now() - t).num_seconds())
            }),
            theme::muted(),
        ),
    }
}

/// The ended tasks in one line, by how each ended: `3 finished · 1
/// blocked`.
pub(crate) fn ended_summary(ended: impl IntoIterator<Item = WorkState>) -> String {
    let words: Vec<&str> = ended.into_iter().map(thread::state_word).collect();
    [WorkState::Finished, WorkState::Blocked, WorkState::Dropped]
        .map(thread::state_word)
        .into_iter()
        .filter_map(|word| {
            let n = words.iter().filter(|w| **w == word).count();
            (n > 0).then(|| format!("{n} {word}"))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The row that folds the ended tasks away, and opens them.
pub(crate) fn ended_row(
    faces: &Faces,
    id: &'static str,
    summary: String,
    open: bool,
    cx: &mut Context<Desk>,
    key: String,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(28.))
        .cursor_pointer()
        .child(row_icon("check"))
        .child(ui::one_line(
            ui::text(faces, summary)
                .text_size(px(13.5))
                .text_color(theme::text2()),
        ))
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                11.,
            )
            .text_color(theme::faint())
            .flex_none(),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
}

/// Where a piece of work stands, as a mark in a 16 square: a dot while it
/// runs and a ring while it waits, both amber, the only places amber is
/// used; a grey tick once it is done, a coral ring with a bar when it is
/// blocked, a dash when it was dropped.
pub(crate) fn glyph(state: WorkState) -> Div {
    let mark = match state {
        WorkState::Open | WorkState::Running => {
            ui::dot(theme::accent()).size(px(7.)).into_any_element()
        }
        WorkState::Waiting => div()
            .size(px(9.))
            .rounded_full()
            .border_1()
            .border_color(theme::accent())
            .into_any_element(),
        WorkState::Finished => ui::icon("check", 16.)
            .text_color(theme::muted())
            .into_any_element(),
        WorkState::Blocked | WorkState::GaveUp => ui::icon("alert", 14.)
            .text_color(theme::coral())
            .into_any_element(),
        WorkState::Dropped | WorkState::Other => ui::icon("minus", 14.)
            .text_color(theme::quiet())
            .into_any_element(),
    };
    div()
        .flex()
        .flex_none()
        .size(px(16.))
        .items_center()
        .justify_center()
        .child(mark)
}

/// How a link stands, in a few words and a colour, and whether it is up.
/// Down is grey, not red: it is a state to act on, and the card says why
/// and what to do. A core the desk has just started is starting, not down.
fn standing(link: &LinkState, reach: &Reach, starting: bool) -> (&'static str, gpui::Hsla, bool) {
    match (link, &reach.why) {
        (LinkState::Connected, _) => ("online", theme::green(), true),
        _ if starting => ("starting", theme::muted(), false),
        (LinkState::Connecting, None) => ("connecting", theme::muted(), false),
        (LinkState::Connecting, Some(_)) => ("retrying", theme::muted(), false),
        (LinkState::Lost(_), _) => ("offline", theme::faint(), false),
    }
}

/// A link's standing in a small pill: a dot when it is up, a ring while
/// it is not.
fn pill(faces: &Faces, word: &'static str, color: gpui::Hsla, up: bool) -> AnyElement {
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

/// The small print under a daemon's name: where it is, and for how long
/// it has been up or down.
fn reach_line(host: &str, link: &LinkState, reach: &Reach, starting: bool) -> String {
    let span = thread::span(i64::try_from(reach.since.elapsed().as_secs()).unwrap_or(i64::MAX));
    match link {
        LinkState::Connected => format!("{host} · up {span}"),
        _ if starting => format!("{host} · starting"),
        _ if reach.why.is_none() => host.to_owned(),
        _ if reach.ever => format!("{host} · down {span}"),
        _ => format!("{host} · never reached"),
    }
}

impl Desk {
    /// A daemon's connection, as a card: what the desk calls it, a pill
    /// for how it stands, where it is and for how long. While it cannot be
    /// reached, one line on why, and a row of quiet actions: try again
    /// now, start the core when it is the one on this machine, and the
    /// error itself under Details. A core the desk has just started shows
    /// as starting, with nothing to act on yet.
    fn daemon_card(
        &self,
        faces: &Faces,
        whom: &Whom,
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
        // The solver's daemon is the core on this machine the desk looks
        // after; any other card is the open character's.
        let solver = *whom == Whom::Solver;
        let starting = solver && self.core_starting();
        let (word, color, up) = standing(link, reach, starting);
        let card = row_with(
            Some(row_icon("server")),
            label(faces, name.to_owned()),
            Some(pill(faces, word, color, up)),
            under(faces, reach_line(&host, link, reach, starting)),
        );
        let Some(why) = reach.why.as_ref().filter(|_| !up && !starting) else {
            return card;
        };
        let tag = if solver { "solver" } else { "hers" };
        let key = format!("link:{url}");
        let open = self.expanded.contains(&key);
        let retry = action(
            faces,
            SharedString::from(format!("{tag}-retry")),
            "Retry now",
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if solver {
                this.retry_solver(cx);
            } else {
                this.retry_her(cx);
            }
        }));
        let start = (solver && self.core_is_local()).then(|| {
            let word = if self.deployment.started() {
                "Restart core"
            } else {
                "Start core"
            };
            action(faces, SharedString::from(format!("{tag}-start")), word)
                .on_click(cx.listener(|this, _, _, cx| this.start_core(cx)))
        });
        let details = action(
            faces,
            SharedString::from(format!("{tag}-details")),
            "Details",
        )
        .gap(px(3.))
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
                .gap(px(4.))
                .pl(px(ICON_ROOM))
                .pt(px(4.))
                .pb(px(2.))
                .child(
                    ui::text(faces, reason(why))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(theme::text2()),
                )
                .child(
                    // The actions start where the text does: each has its
                    // own inner space, so the row steps back by that much.
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(2.))
                        .ml(px(-7.))
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
                            .mt(px(2.))
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
            let place = open.place(self.who());
            let context = Self::context(faces, &open.her, &place);
            let body = self.her_work(faces, work, cx);
            return column
                .child(self.section(faces, "Environment", None, None, environment, cx))
                .child(self.section(faces, "Context", Some("section:context"), None, context, cx))
                .child(self.section(
                    faces,
                    "Background tasks",
                    None,
                    Self::open_count(work.iter().filter(|w| !w.state.ended()).count()),
                    body,
                    cx,
                ))
                .into_any_element();
        }
        let session = self.session();
        let tasks = thread::session_tasks(&self.solver.journal, &self.solver.work, &session.id);
        let outputs = thread::outputs(&self.solver.journal, &session.id);
        let environment = self.environment(faces, cx);
        let place = thread::place_key(&session.id);
        let context = Self::context(faces, &self.solver, &place);
        let work = self.work(faces, &session, &tasks, cx);
        column
            .child(self.section(faces, "Environment", None, None, environment, cx))
            .child(self.section(faces, "Context", Some("section:context"), None, context, cx))
            .child(self.section(
                faces,
                "Background tasks",
                None,
                Self::open_count(tasks.iter().filter(|t| t.open()).count()),
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
                &Whom::Solver,
                "Daemon",
                &self.settings.solver_url,
                &self.solver,
                cx,
            ))
            .child(self.brain_row(faces, cx))
    }

    /// The machine Ghost's work runs on, as its core says. A directory on
    /// this computer, which Open shows in Finder; a directory on the
    /// computer of a core elsewhere, named, with its path as text; anything
    /// else, what the core names; before the core has said, a dash.
    fn machine_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let status = self.solver.status.as_ref();
        let abilities = status.map(|s| &s.said.abilities);
        let dir = status
            .and_then(crate::model::Status::machine_dir)
            .filter(|_| abilities.is_some_and(Abilities::local));
        let (Some(status), Some(home)) = (status, dir) else {
            let what = match abilities {
                None => "—".to_owned(),
                Some(Abilities {
                    work: true,
                    machine: Some(m),
                    ..
                }) => m.spec.clone(),
                Some(_) => "none".to_owned(),
            };
            return row(
                Some(row_icon("laptop")),
                label(faces, "Machine"),
                Some(value(faces, what, theme::muted())),
            );
        };
        if !status.here {
            // On its core's computer: named, and the path as that
            // computer has it, which means nothing to Finder here.
            let host = status.said.core.as_ref().and_then(|c| c.host.clone());
            return row_with(
                Some(row_icon("laptop")),
                label(faces, "Machine"),
                Some(value(
                    faces,
                    host.unwrap_or_else(|| "elsewhere".to_owned()),
                    theme::muted(),
                )),
                under(faces, home.display().to_string()),
            );
        }
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
            .map(|s| s.said.think.clone())
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

    /// How many tasks are still going, beside the section's name; none
    /// when none is.
    fn open_count(open: usize) -> Option<String> {
        (open > 0).then(|| open.to_string())
    }

    /// The background tasks of this session: those still going, newest
    /// first, each with what it asks and a way to stop it; then those that
    /// have ended, folded into one line that opens.
    fn work(
        &self,
        faces: &Faces,
        session: &Session,
        tasks: &[Task<'_>],
        cx: &mut Context<Self>,
    ) -> Div {
        if tasks.is_empty() {
            return div().child(
                ui::text(faces, "Nothing running in the background.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        let mut list = div().flex().flex_col();
        for t in tasks.iter().rev().filter(|t| t.open()) {
            let (right, color) = open_standing(t.state(), Some(t.started));
            let key = format!("task:{}", t.id);
            list = list.child(
                task_row(
                    faces,
                    SharedString::from(format!("work-{}", t.id)),
                    running_lead(),
                    t.brief.clone(),
                    right,
                    color,
                )
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx))),
            );
            // What its daemon says of it, when it says: the parts it
            // handed out, each of which opens in the thread inside the
            // task's card; what it asks; and a way to stop it while it is
            // open.
            if let Some(w) = t.work {
                let now = Utc::now();
                for p in &w.parts {
                    let keys = [format!("task:{}", t.id), format!("part:{}", p.id)];
                    list = list.child(
                        part_row(faces, SharedString::from(format!("part-{}", p.id)), p, now)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.expanded.extend(keys.iter().cloned());
                                cx.notify();
                            })),
                    );
                }
                let ask = Ask::of(Whom::Solver, w);
                if ask.open {
                    list = list.child(
                        self.ask_controls(faces, &ask, cx)
                            .pl(px(ICON_ROOM))
                            .pb(px(6.)),
                    );
                }
            }
        }
        let ended: Vec<&Task<'_>> = tasks.iter().rev().filter(|t| !t.open()).collect();
        if ended.is_empty() {
            return list;
        }
        let summary = ended_summary(ended.iter().map(|t| t.state()));
        let key = format!("more:{}", session.id);
        let open = self.expanded.contains(&key);
        list = list.child(ended_row(faces, "work-ended", summary, open, cx, key));
        if open {
            for t in ended {
                let ago = format!(
                    "{} ago",
                    thread::span((Utc::now() - t.changed()).num_seconds())
                );
                let key = format!("task:{}", t.id);
                list = list.child(
                    task_row(
                        faces,
                        SharedString::from(format!("work-{}", t.id)),
                        glyph(t.state()).into_any_element(),
                        t.brief.clone(),
                        ago,
                        theme::faint(),
                    )
                    .pl(px(4.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx))),
                );
            }
        }
        list
    }

    /// The files this session's work handed over. A report names a path
    /// on its core's computer (relative ones are under its machine's
    /// directory, as the core says it): it shows in Finder when that is
    /// this computer, and otherwise says where it is. A kept file is asked
    /// of the core like any image of hers, and opens from the copy kept
    /// here.
    fn outputs(&self, faces: &Faces, outputs: &[Output], cx: &mut Context<Self>) -> Div {
        let status = self.solver.status.as_ref();
        let here = status.is_some_and(|s| s.here);
        let mut list = div().flex().flex_col();
        for (i, o) in outputs.iter().enumerate() {
            // What a click does with it, in Finder or in its viewer, or
            // what is said under it instead. A kept file on its way from
            // the core is a plain row until it is here.
            let (click, note) = match o.source {
                Source::Report => match status.and_then(|s| s.locate(&o.file, None)) {
                    Some(path) if here => (Some((path, true)), None),
                    Some(path) => (None, Some(path.display().to_string())),
                    None => (None, None),
                },
                Source::Kept => match self.solver.media().map(|m| media::shown(&m, &o.file)) {
                    Ok(Shown::Here(path)) => (Some((path, false)), None),
                    Ok(Shown::Loading) => (None, None),
                    Ok(Shown::Failed(why)) => (None, Some(why)),
                    Err(why) => (None, Some(why.to_owned())),
                },
            };
            let (icon, name, kind) = (
                Some(row_icon("file")),
                label(faces, o.name().to_owned()),
                Some(value(faces, o.kind(), theme::faint())),
            );
            list = list.child(match (click, note) {
                (Some((path, in_finder)), _) => row(icon, name, kind)
                    .id(("output", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if in_finder {
                            this.reveal(&path, cx);
                        } else {
                            cx.open_with_system(&path);
                        }
                    }))
                    .into_any_element(),
                (None, Some(note)) => {
                    row_with(icon, name, kind, under(faces, note)).into_any_element()
                }
                (None, None) => row(icon, name, kind).into_any_element(),
            });
        }
        list
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
                &Whom::Her,
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
                under(faces, status.map_or_else(dash, |s| s.said.city.clone())),
            ))
            .child({
                let think = status
                    .map(|s| s.said.think.clone())
                    .filter(|t| !t.is_empty());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn part(state: WorkState, started: Option<&str>) -> Work {
        serde_json::from_value(serde_json::json!({
            "id": "w/1",
            "brief": "Read the reviews",
            "state": state,
            "started": started,
        }))
        .unwrap()
    }

    #[test]
    fn ended_work_is_counted_by_how_it_ended_in_the_words_shown_for_it() {
        let ended = [
            WorkState::Dropped,
            WorkState::Finished,
            WorkState::GaveUp,
            WorkState::Blocked,
            WorkState::Finished,
        ];
        assert_eq!(ended_summary(ended), "2 finished · 2 blocked · 1 dropped");
        assert_eq!(ended_summary([WorkState::Finished]), "1 finished");
    }

    #[test]
    fn a_brain_is_tagged_by_the_first_word_of_its_model_or_agent() {
        assert_eq!(brain_word("openai/deepseek-chat"), "deepseek");
        assert_eq!(brain_word("anthropic/claude-sonnet-5"), "claude");
        assert_eq!(brain_word("claude-code"), "claude");
        assert_eq!(brain_word("codex"), "codex");
        assert_eq!(brain_word("ollama/qwen3:8b"), "qwen3:8b");
        assert_eq!(
            brain_word("-odd"),
            "-odd",
            "a name with no first word is kept whole"
        );
    }

    #[test]
    fn a_part_reads_queued_until_it_starts_then_its_time_then_how_it_ended() {
        let now: DateTime<Utc> = "2026-11-17T12:00:00Z".parse().unwrap();
        assert_eq!(part_standing(&part(WorkState::Open, None), now), "queued");
        let running = part(WorkState::Running, Some("2026-11-17T11:58:00Z"));
        assert_eq!(
            part_standing(&running, now),
            thread::span(120),
            "how long it has run"
        );
        assert_eq!(
            part_standing(&part(WorkState::Waiting, None), now),
            "waiting"
        );
        assert_eq!(
            part_standing(&part(WorkState::Finished, None), now),
            "finished"
        );
        assert_eq!(
            part_standing(&part(WorkState::GaveUp, None), now),
            "blocked"
        );
        assert_eq!(
            part_standing(&part(WorkState::Dropped, None), now),
            "dropped"
        );
    }
}
