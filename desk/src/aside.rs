//! The column down the right: where the solver runs, the context it works
//! in, the work taken on in this session, and the files that work handed
//! over. With a character open, her daemon and its link, her clock and the
//! brain she thinks with, and her work as her daemon keeps it. Sections
//! are told apart by full-width hairlines and rows keep one height, the
//! way a coding agent's side panel reads. Nothing here is drawn without
//! data behind it: a section with nothing to say says so, and Outputs is
//! left out until there is a file to show.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use ghost_link::protocol::{Work, WorkState};
use gpui::{
    AnyElement, Context, Corner, Div, FontWeight, SharedString, Stateful, Svg, div, prelude::*, px,
};

use crate::{
    desk::{At, Desk, Menu, Open, RIGHT},
    fonts::Faces,
    model::LinkState,
    settings::Session,
    sidebar::link_color,
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
fn line(icon: Option<Svg>, label: Div, right: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .children(icon.map(gpui::Styled::flex_none))
        .child(ui::one_line(label))
        .children(right)
}

/// A row of one line.
fn row(icon: Option<Svg>, label: Div, right: Option<AnyElement>) -> Div {
    line(icon, label, right).h(px(ROW))
}

/// A row with a second line of small print under its label.
fn row_with(icon: Option<Svg>, label: Div, right: Option<AnyElement>, under: Div) -> Div {
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
fn row_icon(name: &str) -> Svg {
    ui::icon(name, 16.).text_color(theme::muted())
}

/// Where a piece of work stands, as a glyph: a ring while it is open, a
/// dot once it has ended, each in its state's colour.
fn glyph(state: State) -> Div {
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
fn state_of(state: WorkState) -> State {
    match state {
        WorkState::Open | WorkState::Running => State::Running,
        WorkState::Waiting => State::Waiting,
        WorkState::Finished => State::Finished,
        WorkState::Blocked | WorkState::GaveUp => State::Blocked,
        WorkState::Dropped | WorkState::Other => State::Dropped,
    }
}

/// The daemon: what the desk calls it, how the link to it stands, where it
/// is, and why it cannot be reached when it cannot.
fn daemon_row(faces: &Faces, name: &str, url: &str, link: &LinkState) -> Div {
    let host = url
        .trim_start_matches("ws://")
        .trim_start_matches("wss://")
        .to_owned();
    let word = match link {
        LinkState::Connected => "connected",
        LinkState::Connecting => "connecting",
        LinkState::Lost(_) => "unreachable",
    };
    let mut r = row_with(
        Some(row_icon("server")),
        label(faces, name.to_owned()),
        Some(value(faces, word, link_color(link))),
        under(faces, host),
    );
    if let LinkState::Lost(why) = link {
        r = r.child(
            ui::text(faces, why.clone())
                .text_size(px(12.))
                .line_height(px(17.))
                .text_color(theme::coral())
                .pl(px(ICON_ROOM))
                .pt(px(2.)),
        );
    }
    r
}

impl Desk {
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
            let environment = her_environment(faces, open);
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

    /// Her work as her daemon keeps it, most recently active first: five,
    /// then the rest when asked for.
    fn her_work(&self, faces: &Faces, work: &[Work], cx: &mut Context<Self>) -> Div {
        if work.is_empty() {
            return div().child(
                ui::text(faces, "Nothing in hand, and nothing ended lately.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        let key = "more:her".to_owned();
        let all = self.expanded.contains(&key);
        let (count, hidden) = thread::shown(work.len(), WORK_ROWS, all);
        let mut list = div().flex().flex_col().gap(px(2.));
        for w in work.iter().take(count) {
            list = list.child(her_work_row(faces, w));
        }
        if hidden > 0 || (all && work.len() > WORK_ROWS) {
            let more = if hidden > 0 {
                format!("Show {hidden} more")
            } else {
                "Show fewer".to_owned()
            };
            list = list.child(
                div()
                    .id("her-work-more")
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

    /// Where the solver runs: the machine its work uses, the daemon, the
    /// brain, and whether the desk's channel is registered.
    fn environment(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .child(self.machine_row(faces, cx))
            .child(daemon_row(
                faces,
                "Daemon",
                &self.settings.solver_url,
                &self.solver.link,
            ))
            .child(self.brain_row(faces, cx))
            .child(row(
                Some(row_icon("plug")),
                label(faces, "Desk"),
                Some(if self.solver.desk_online {
                    value(faces, "registered", theme::green())
                } else {
                    value(faces, "not yet", theme::muted())
                }),
            ))
    }

    /// The machine the solver's work runs on. On this one, its home, which
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
        let (icon, name) = provider.as_ref().map_or_else(
            || {
                (
                    "openai",
                    told.unwrap_or_else(|| "No brain chosen".to_owned()),
                )
            },
            |p| (p.icon(), p.label()),
        );
        let change = action(faces, "change-brain", "Change")
            .on_click(cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, At::Aside, cx)));
        row(
            Some(ui::icon(icon, 16.).text_color(if provider.is_some() {
                theme::brand(icon)
            } else {
                theme::ghost_line()
            })),
            label(faces, name),
            Some(change.into_any_element()),
        )
        .relative()
        .children(
            self.menu_over(At::Aside, Corner::TopRight, faces, cx)
                .map(|menu| div().absolute().top(px(ROW)).right_0().child(menu)),
        )
    }

    /// What the solver knows of its own life and of you: the journal, its
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
        let operator = self.solver.you(self.who()).map_or_else(
            || "not yet known".to_owned(),
            |p| format!("{} · {} lines", self.who(), p.heard),
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

/// Where she is: her daemon and its link, her clock and her city, and the
/// brain she thinks with, as her daemon says.
fn her_environment(faces: &Faces, open: &Open) -> Div {
    let status = open.her.status.as_ref();
    let dash = || "—".to_owned();
    div()
        .flex()
        .flex_col()
        .child(daemon_row(
            faces,
            open.host.label(),
            &open.url,
            &open.her.link,
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
            let icon = think.as_deref().map_or("openai", brain_icon);
            row(
                Some(ui::icon(icon, 16.).text_color(theme::brand(icon))),
                label(faces, "Brain"),
                Some(value(faces, think.unwrap_or_else(dash), theme::muted())),
            )
        })
}

/// The mark for a brain, from the daemon's word for it.
fn brain_icon(think: &str) -> &'static str {
    let think = think.to_lowercase();
    if think.contains("claude") || think.contains("anthropic") {
        "claude"
    } else if think.contains("deepseek") {
        "deepseek"
    } else {
        "openai"
    }
}

/// A piece of her work: where it stands, its brief, how far it has gone
/// and when it last moved, and what it asked while it waits to hear.
fn her_work_row(faces: &Faces, w: &Work) -> Div {
    let mut figures = format!("{} steps", w.steps);
    if !w.parts.is_empty() {
        let open = w.parts.iter().filter(|p| !p.state.ended()).count();
        figures = format!("{figures} · {} parts, {open} open", w.parts.len());
    }
    if let Some(last) = w
        .last
        .as_deref()
        .and_then(|t| t.parse::<DateTime<Utc>>().ok())
    {
        let ago = thread::span((Utc::now() - last).num_seconds());
        figures = format!("{figures} · {ago} ago");
    }
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .py(px(6.))
        .child(div().pt(px(5.)).child(glyph(state_of(w.state))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w(px(0.))
                .child(ui::one_line(
                    ui::text(faces, w.brief.clone()).text_size(px(13.5)),
                ))
                .child(ui::one_line(
                    ui::mono(faces, figures)
                        .text_size(px(11.5))
                        .text_color(theme::muted()),
                ))
                .children(w.question.as_ref().map(|q| {
                    ui::one_line(
                        ui::text(faces, q.clone())
                            .text_size(px(12.5))
                            .text_color(theme::purple()),
                    )
                })),
        )
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
