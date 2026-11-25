//! The middle: the top bar, the session's thread, and the line you type
//! into, drawn as the design has them.
//!
//! What you said sits in a bubble; what the agent said is prose; each tool
//! call is a row (an icon, its kind as a verb, what it was about, how it
//! went) that opens into a framed box with its diff or its output; a
//! question the agent asks is a card with its choices. Every agent is drawn
//! the same way, from the same ACP updates.

mod cards;
mod composer;
pub mod drawer;
pub mod drawn;
mod fold;
pub mod kinds;
mod lines;
mod start;

pub use lines::Line;
pub mod output;
mod topbar;

use std::path::Path;

use agent_client_protocol_schema::v1::{
    Plan, PlanEntryStatus, SessionConfigKind, SessionConfigOptionCategory,
    SessionConfigSelectOptions, ToolCallContent, ToolCallStatus, ToolKind,
};
use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::Desk,
    fonts::Faces,
    markdown,
    session::{Item, Session, Tool, text_of},
    style::BUBBLE_RADIUS,
    theme,
    ui::{self, Mark},
    when,
};

/// The parts of a tool call's row.
struct Row {
    icon: &'static str,
    verb: &'static str,
    object: String,
    kind: Object,
}

/// How what a row is about is set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Object {
    /// A path or a pattern, in mono.
    Mono,
    /// A command: its program in one colour, the rest in another.
    Command,
    /// An address, as a link.
    Link,
    /// Words.
    Plain,
}

/// `title` without the verb an agent may have begun it with, or the
/// backticks around a command.
fn bare(title: &str) -> String {
    let t = title.trim().trim_matches('`');
    for verb in [
        "Read ", "Edit ", "Write ", "Search ", "Grep ", "Run ", "Fetch ", "Delete ", "Move ",
    ] {
        if let Some(rest) = t.strip_prefix(verb) {
            return rest.trim().trim_matches('`').to_owned();
        }
    }
    t.to_owned()
}

/// A path shown relative to the session's folder when it is inside it.
fn rel(path: &Path, folder: &Path) -> String {
    path.strip_prefix(folder)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// An address without its scheme, shortened in the middle when long.
fn short_url(url: &str) -> String {
    let u = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if u.chars().count() <= 60 {
        return u.to_owned();
    }
    let parts: Vec<&str> = u.split('/').collect();
    if parts.len() > 3 {
        format!("{}/{}/…/{}", parts[0], parts[1], parts[parts.len() - 1])
    } else {
        ui::fit(u, 60)
    }
}

fn row_of(tool: &Tool, folder: &Path) -> Row {
    let call = &tool.call;
    let path = call
        .content
        .iter()
        .find_map(|c| match c {
            ToolCallContent::Diff(d) => Some(d.path.clone()),
            _ => None,
        })
        .or_else(|| call.locations.first().map(|l| l.path.clone()));
    let path_or_title = || {
        path.as_deref()
            .map_or_else(|| bare(&call.title), |p| rel(p, folder))
    };
    let created = call
        .content
        .iter()
        .any(|c| matches!(c, ToolCallContent::Diff(d) if d.old_text.is_none()));
    let running = matches!(
        call.status,
        ToolCallStatus::Pending | ToolCallStatus::InProgress
    );
    let (icon, verb, object, kind) = match call.kind {
        ToolKind::Read => ("file", "Read", path_or_title(), Object::Mono),
        ToolKind::Edit if created => ("file-plus", "Created", path_or_title(), Object::Mono),
        ToolKind::Edit => ("pencil", "Edited", path_or_title(), Object::Mono),
        ToolKind::Delete => ("trash", "Deleted", path_or_title(), Object::Mono),
        ToolKind::Move => ("arrow-right", "Moved", bare(&call.title), Object::Mono),
        ToolKind::Search => ("search", "Searched", bare(&call.title), Object::Mono),
        ToolKind::Execute if running => ("terminal", "Running", bare(&call.title), Object::Command),
        ToolKind::Execute => ("terminal", "Ran", bare(&call.title), Object::Command),
        ToolKind::Fetch => (
            "globe",
            "Fetched",
            short_url(&bare(&call.title)),
            Object::Link,
        ),
        // A call to think with a task given is a subagent sent off with it.
        ToolKind::Think if !call.content.is_empty() => (
            "user",
            if running {
                "Running a subagent"
            } else {
                "Ran a subagent"
            },
            call.title.clone(),
            Object::Plain,
        ),
        ToolKind::Think => ("bulb", "Thought about", call.title.clone(), Object::Plain),
        ToolKind::SwitchMode => ("list", "Switched mode", call.title.clone(), Object::Plain),
        _ => ("plug", "Called", call.title.clone(), Object::Plain),
    };
    Row {
        icon,
        verb,
        object,
        kind,
    }
}

/// A command as the design colours it: the program, then its arguments.
pub fn command(faces: &Faces, text: &str, size: f32) -> Div {
    let (program, rest) = text.split_once(' ').unwrap_or((text, ""));
    div()
        .flex()
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(faces.mono.clone())
        .text_size(px(size))
        .child(
            div()
                .flex_none()
                .text_color(theme::code_program())
                .child(program.to_owned()),
        )
        .when(!rest.is_empty(), |d| {
            d.child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_color(theme::code_argument())
                    .child(format!(" {}", ui::fit(rest, 72))),
            )
        })
}

/// A step's row, as the design sets it: 30 high, an icon, the verb in grey,
/// what it was about, and small print at the right.
fn step() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(30.))
        .px(px(10.))
        .text_size(px(14.))
        .line_height(px(22.))
}

fn icon(name: &str) -> gpui::Svg {
    ui::icon(name, 16.).text_color(theme::faint())
}

fn verb(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text).flex_none().text_color(theme::muted())
}

/// Small print at the right of a row. It takes the room left and sets its
/// words at the end of it: GPUI drops a row's gaps when a child is pushed
/// right with an automatic margin.
fn right(faces: &Faces, text: impl Into<SharedString>) -> Div {
    right_in(faces, text, theme::faint())
}

/// Small print at the right, in `color`: amber while running, coral when
/// it failed.
fn right_in(faces: &Faces, text: impl Into<SharedString>, color: gpui::Hsla) -> Div {
    div().flex().flex_1().justify_end().child(
        ui::text(faces, text)
            .flex_none()
            .text_size(px(12.5))
            .line_height(px(18.))
            .text_color(color),
    )
}

fn meta(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(12.5))
        .line_height(px(18.))
        .text_color(theme::faint())
}

fn counts(faces: &Faces, added: usize, gone: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .gap(px(8.))
        .when(added > 0 || gone == 0, |d| {
            d.child(
                ui::mono(faces, format!("+{added}"))
                    .text_size(px(12.5))
                    .text_color(theme::ok()),
            )
        })
        .when(gone > 0, |d| {
            d.child(
                ui::mono(faces, format!("−{gone}"))
                    .text_size(px(12.5))
                    .text_color(theme::coral()),
            )
        })
}

/// The framed box an opened step sits in.
fn boxed() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(12.))
        .pt(px(12.))
        .pb(px(14.))
        .rounded(px(16.))
        .border_1()
        .border_color(theme::border())
}

/// A well of mono text, for output.
pub fn well(faces: &Faces) -> Div {
    div()
        .px(px(14.))
        .py(px(10.))
        .rounded(px(12.))
        .border_1()
        .border_color(theme::border())
        .font_family(faces.mono.clone())
        .text_size(px(13.))
        .line_height(px(19.))
        .text_color(theme::text2())
}

/// A plan's steps as the checklist draws them.
pub fn plan_steps(plan: &Plan) -> Vec<(kinds::Status, String)> {
    plan.entries
        .iter()
        .map(|e| {
            let status = match e.status {
                PlanEntryStatus::Completed => kinds::Status::Done,
                PlanEntryStatus::InProgress => kinds::Status::Doing,
                _ => kinds::Status::Todo,
            };
            (status, e.content.clone())
        })
        .collect()
}

/// The choices of a select option: `(id, name, current)` each, and the
/// name of the current one.
/// A menu's choices: `(id, name, current)` each.
type Choices = Vec<(String, String, bool)>;

fn select_choices(kind: &SessionConfigKind) -> Option<(String, Choices)> {
    let SessionConfigKind::Select(select) = kind else {
        return None;
    };
    let all: Vec<_> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(o) => o.iter().collect(),
        SessionConfigSelectOptions::Grouped(g) => g.iter().flat_map(|g| g.options.iter()).collect(),
        _ => return None,
    };
    let current = all
        .iter()
        .find(|o| o.value == select.current_value)
        .map_or_else(|| select.current_value.0.to_string(), |o| o.name.clone());
    let choices = all
        .iter()
        .map(|o| {
            (
                o.value.0.to_string(),
                o.name.clone(),
                o.value == select.current_value,
            )
        })
        .collect();
    Some((current, choices))
}

impl Desk {
    /// An agent's name as the desk shows it.
    pub(crate) fn name_of(&self, id: &str) -> String {
        let (agent, host) = crate::remote::split(id);
        let registry = self.entry(agent).map_or(agent, |e| e.name.as_str());
        let name = theme::agent_name(agent, registry).into_owned();
        match host {
            Some(host) => format!("{name} on {host}"),
            None => name,
        }
    }

    /// The name of the value chosen for the session's option in `category`.
    pub(crate) fn option_name(
        s: &Session,
        category: &SessionConfigOptionCategory,
    ) -> Option<String> {
        s.options
            .iter()
            .find(|o| o.category.as_ref() == Some(category))
            .and_then(|o| select_choices(&o.kind))
            .map(|(current, _)| current)
    }

    /// The session's mode, by name, from its options or its modes.
    pub(crate) fn mode_name(s: &Session) -> Option<String> {
        Self::option_name(s, &SessionConfigOptionCategory::Mode).or_else(|| {
            let m = s.modes.as_ref()?;
            m.available_modes
                .iter()
                .find(|x| x.id == m.current_mode_id)
                .map(|x| x.name.clone())
        })
    }

    pub(crate) fn thread(
        &self,
        faces: &Faces,
        column: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !self.lines.is_empty() {
            return self.thread_list(column, cx).into_any_element();
        }
        let mut col = div().flex().flex_col().w(px(column)).py(px(28.));
        match self.session() {
            None => col = col.child(self.start(faces, column, cx)),
            Some(s) => col = col.child(Self::fresh(faces, s, &self.name_of(&s.agent), cx)),
        }
        div()
            .id("thread")
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(col)
            .into_any_element()
    }

    /// The line over the first turn: who, with what, where.
    fn turn_header(&self, faces: &Faces, s: &Session) -> Div {
        let name = self.name_of(&s.agent);
        let mut parts = vec![name.clone()];
        parts.extend(Self::option_name(s, &SessionConfigOptionCategory::Model));
        parts.extend(Self::mode_name(s));
        parts.push(Desk::shown(&s.folder));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .mb(px(12.))
            .child(ui::mark(faces, &Mark::of(&s.agent, &name), 14.))
            .child(meta(faces, parts.join(" · ")))
    }

    /// A session begun, before anything is said: where it will work.
    fn fresh(faces: &Faces, s: &Session, name: &str, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .mt(px(140.))
            .child(ui::mark(faces, &Mark::of(&s.agent, name), 48.))
            .child(
                ui::text(faces, format!("New session with {name}"))
                    .text_size(px(22.))
                    .line_height(px(30.))
                    .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(
                div()
                    .id("folder")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(theme::tag_line())
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::icon("folder", 14.).text_color(theme::faint()))
                    .child(
                        ui::mono(faces, Desk::shown(&s.folder))
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                    )
                    .children(s.branch.clone().map(|b| {
                        ui::mono(faces, b)
                            .text_size(px(12.))
                            .text_color(theme::faint())
                    }))
                    .child(ui::icon("chevron-down", 12.).text_color(theme::faint()))
                    .tooltip(ui::tip("The folder it works in"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
            )
            .when(s.repo, |d| {
                d.child(Self::where_toggle(faces, s.wants_worktree, cx))
            })
    }

    /// Whether the session works in the folder itself or in a worktree of
    /// its own, on a new branch: two halves of one control.
    fn where_toggle(faces: &Faces, worktree: bool, cx: &mut Context<Self>) -> Div {
        let to = |to: bool| -> ui::Pick {
            Box::new(cx.listener(move |this, _, _, cx| this.set_worktree(to, cx)))
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(ui::segmented(
                faces,
                "where",
                vec![
                    ("In this folder", !worktree, to(false)),
                    ("New worktree", worktree, to(true)),
                ],
            ))
            .child(
                ui::text(
                    faces,
                    if worktree {
                        "A worktree of its own on a new branch: your folder is left as it is."
                    } else {
                        "It works in your folder as it is. Each turn is checkpointed and can be undone."
                    },
                )
                .text_size(px(12.5))
                .text_color(theme::faint()),
            )
    }

    fn item(
        &self,
        faces: &Faces,
        s: &Session,
        i: usize,
        item: &Item,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("{}-{i}", s.key);
        match item {
            Item::You(text) => {
                // Beside a message of yours after the first, on hover: a new
                // session with what was said before it, to say it again.
                let group = SharedString::from(format!("{key}-you"));
                let branch = s.fork_point(i).is_some()
                    && matches!(
                        s.state,
                        crate::session::State::Idle | crate::session::State::Closed
                    )
                    && self.can_fork(s);
                let at = s.key;
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .group(group.clone())
                    .when(branch, |d| {
                        d.child(
                            div().invisible().group_hover(group, Styled::visible).child(
                                ui::icon_button(
                                    SharedString::from(format!("{key}-branch")),
                                    "branch",
                                    "Branch off here, to say this again",
                                )
                                .size(px(24.))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.fork_at(at, i, cx);
                                    },
                                )),
                            ),
                        )
                    })
                    .child(
                        ui::text(faces, text.clone())
                            .max_w(px(520.))
                            .px(px(16.))
                            .py(px(12.))
                            .rounded(px(BUBBLE_RADIUS))
                            .bg(theme::bubble())
                            .text_size(px(14.))
                            .line_height(px(21.)),
                    )
                    .into_any_element()
            }
            Item::Agent(text) => {
                // What the agent said, copied whole from beside it on hover.
                let group = SharedString::from(format!("{key}-said"));
                let copy = text.clone();
                div()
                    .relative()
                    .group(group.clone())
                    .px(px(10.))
                    .child(markdown::render(faces, &key, text, &self.expanded, cx))
                    .child(
                        div()
                            .absolute()
                            .bottom(px(-22.))
                            .left(px(6.))
                            .invisible()
                            .group_hover(group, Styled::visible)
                            .child(
                                ui::icon_button(
                                    SharedString::from(format!("{key}-copy")),
                                    "copy",
                                    "Copy",
                                )
                                .size(px(22.))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                }),
                            ),
                    )
                    .into_any_element()
            }
            Item::Thought { text, secs } => {
                let open = self.expanded.contains(&key);
                let toggle = key.clone();
                let label = match secs {
                    Some(n) => format!("Thought for {n}s"),
                    None => "Thinking".into(),
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        step()
                            .id(SharedString::from(key.clone()))
                            .cursor_pointer()
                            .rounded(px(8.))
                            .hover(|st| st.bg(theme::hover()))
                            .child(icon(if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            }))
                            .child(
                                verb(faces, label)
                                    .when(secs.is_none(), |d| d.text_color(theme::accent())),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
                    )
                    .when(open, |d| {
                        d.child(
                            div()
                                .ml(px(33.))
                                .pl(px(12.))
                                .border_l_1()
                                .border_color(theme::tag_line())
                                .child(
                                    ui::serif(faces, text.clone())
                                        .italic()
                                        .text_size(px(14.))
                                        .line_height(px(21.))
                                        .text_color(theme::muted()),
                                ),
                        )
                    })
                    .into_any_element()
            }
            Item::Tool(tool) => self.tool(faces, s, i, tool, cx).into_any_element(),
            Item::Asked(asked) => self.asked(faces, s, &key, asked, cx).into_any_element(),
            Item::Plan(plan) => {
                let steps = plan_steps(plan);
                let done = steps
                    .iter()
                    .filter(|(st, _)| *st == kinds::Status::Done)
                    .count();
                boxed()
                    .child(
                        step()
                            .px(px(2.))
                            .min_h(px(22.))
                            .child(icon("list"))
                            .child(verb(faces, "Plan"))
                            .child(right(faces, format!("{done} of {} done", steps.len()))),
                    )
                    .child(div().px(px(4.)).child(drawn::plan(faces, &steps)))
                    .into_any_element()
            }
            Item::Image(image) => div()
                .px(px(10.))
                .child(
                    gpui::img(image.clone())
                        .max_w(px(480.))
                        .max_h(px(360.))
                        .rounded(px(8.)),
                )
                .into_any_element(),
            // What went wrong in the session is a row of its own; anything
            // else it notes reads as a quiet line.
            Item::Note {
                text,
                trouble: true,
            } => ui::trouble(faces, text.clone()).into_any_element(),
            Item::Note { text, .. } => ui::text(faces, text.clone())
                .px(px(10.))
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(theme::muted())
                .into_any_element(),
            Item::Ended { reason, secs } => self
                .ended(faces, s, i, *reason, *secs, cx)
                .into_any_element(),
        }
    }

    /// A folded stretch of the thread, as one line: a chevron, and what
    /// was done in it. A click opens or folds it.
    fn fold_row(
        faces: &Faces,
        key: String,
        words: &str,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle = key.clone();
        step()
            .id(SharedString::from(key))
            .cursor_pointer()
            .rounded(px(8.))
            .hover(|st| st.bg(theme::hover()))
            .child(icon(if open {
                "chevron-down"
            } else {
                "chevron-right"
            }))
            .child(
                ui::one_line(verb(faces, ui::fit(words, 90)))
                    .w_auto()
                    .min_w(px(0.)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
            .into_any_element()
    }

    /// Whether the call at `i` is open: an edit and a command with output
    /// start open, the rest closed; a click turns either over.
    fn tool_open(&self, session: u64, i: usize, tool: &Tool) -> bool {
        let call = &tool.call;
        if !has_body(tool) {
            return false;
        }
        let first = match call.kind {
            ToolKind::Edit | ToolKind::Execute => true,
            _ => call.status == ToolCallStatus::Failed,
        };
        first != self.expanded.contains(&format!("{session}-{i}"))
    }

    fn tool(
        &self,
        faces: &Faces,
        s: &Session,
        i: usize,
        tool: &Tool,
        cx: &mut Context<Self>,
    ) -> Div {
        let call = &tool.call;
        let key = format!("{}-{i}", s.key);
        let row = row_of(tool, &s.folder);
        let open = self.tool_open(s.key, i, tool);
        let running = matches!(
            call.status,
            ToolCallStatus::Pending | ToolCallStatus::InProgress
        );
        let failed = call.status == ToolCallStatus::Failed;
        let (added, gone) = call.content.iter().fold((0, 0), |(a, g), c| match c {
            ToolCallContent::Diff(d) => {
                let (gone, added) = kinds::counted(&kinds::diff(
                    d.old_text.as_deref().unwrap_or(""),
                    &d.new_text,
                ));
                (a + added, g + gone)
            }
            _ => (a, g),
        });

        let object: AnyElement = match row.kind {
            Object::Command => command(faces, &row.object, 13.5).into_any_element(),
            Object::Mono => ui::one_line(
                ui::mono(faces, ui::fit(&row.object, 72))
                    .text_size(px(13.5))
                    .text_color(theme::text()),
            )
            .w_auto()
            .min_w(px(0.))
            .into_any_element(),
            Object::Link => {
                ui::one_line(ui::text(faces, row.object.clone()).text_color(theme::link()))
                    .w_auto()
                    .min_w(px(0.))
                    .into_any_element()
            }
            Object::Plain => {
                ui::one_line(ui::text(faces, ui::fit(&row.object, 72)).text_color(theme::text()))
                    .w_auto()
                    .min_w(px(0.))
                    .into_any_element()
            }
        };
        let mark: AnyElement = if running {
            div().mx(px(2.)).child(ui::live_dot()).into_any_element()
        } else {
            icon(row.icon)
                .when(failed, |i| i.text_color(theme::coral()))
                .into_any_element()
        };
        let tail: Option<AnyElement> = if running {
            Some(
                right_in(faces, when::took(tool.started.elapsed()), theme::accent())
                    .into_any_element(),
            )
        } else if failed {
            Some(right_in(faces, "failed", theme::coral()).into_any_element())
        } else if call.kind == ToolKind::Execute {
            tool.took
                .map(|t| right(faces, when::took_short(t)).into_any_element())
        } else if call.kind == ToolKind::Search {
            call.content.iter().find_map(|c| match c {
                ToolCallContent::Content(x) => {
                    let n = text_of(&x.content)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .count();
                    let word = if n == 1 { "match" } else { "matches" };
                    Some(right(faces, format!("{n} {word}")).into_any_element())
                }
                _ => None,
            })
        } else {
            None
        };

        // An edit opens its file's diff in the column beside, as it stands.
        let diff_of = call.content.iter().find_map(|c| match c {
            ToolCallContent::Diff(d) => Some(d.path.clone()),
            _ => None,
        });
        let open_diff = diff_of.filter(|_| !running).map(|path| {
            ui::button(
                faces,
                SharedString::from(format!("{key}-open-diff")),
                "Open diff",
                ui::Weight::Quiet,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.show_aside(crate::desk::Aside::File(path.clone()), cx);
            }))
        });
        let toggle = key.clone();
        let mut line = step()
            .id(SharedString::from(format!("{key}-row")))
            .rounded(px(8.))
            .child(mark)
            .child(verb(faces, row.verb).when(running, |d| d.text_color(theme::accent())))
            .child(object)
            .when(added + gone > 0 && call.kind != ToolKind::Execute, |d| {
                d.child(counts(faces, added, gone))
            })
            .children(tail)
            .children(open_diff.map(|b| div().flex_1().flex().justify_end().child(b)));
        if has_body(tool) {
            line = line
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
            if !open {
                line = line.hover(|st| st.bg(theme::hover()));
            }
        }
        if !open {
            return div().child(line);
        }

        let mut body = boxed().when(failed, |d| d.border_color(theme::coral().opacity(0.35)));
        body = body.child(line.px(px(2.)).min_h(px(22.)));
        let mut diff_n = 0;
        for (j, c) in call.content.iter().enumerate() {
            match c {
                ToolCallContent::Diff(d) => {
                    let lines = kinds::diff(d.old_text.as_deref().unwrap_or(""), &d.new_text);
                    let start = tool.starts.get(diff_n).copied().flatten();
                    diff_n += 1;
                    body = body.child(drawn::diff(faces, &lines, start, drawn::Fit::Thread));
                }
                ToolCallContent::Content(x) => {
                    let text = text_of(&x.content);
                    if !text.trim().is_empty() {
                        body = body.child(well(faces).px(px(4.)).child(output::printed(
                            faces,
                            &format!("{key}-{j}"),
                            &text,
                            self.expanded.contains(&format!("{key}-all")),
                        )));
                    }
                }
                _ => {}
            }
        }
        div().child(body)
    }
}

/// Whether a call has anything to open: a diff, or output with words in it.
fn has_body(tool: &Tool) -> bool {
    tool.call.content.iter().any(|c| match c {
        ToolCallContent::Diff(_) => true,
        ToolCallContent::Content(x) => !text_of(&x.content).trim().is_empty(),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_own_verb_and_backticks_are_taken_off_a_title() {
        assert_eq!(bare("Read src/lib.rs"), "src/lib.rs");
        assert_eq!(bare("`cargo test`"), "cargo test");
        assert_eq!(bare("Run `ls -la`"), "ls -la");
        assert_eq!(bare("put_object"), "put_object");
    }

    #[test]
    fn a_long_address_keeps_its_host_and_its_end() {
        assert_eq!(
            short_url("https://docs.aws.amazon.com/AmazonS3/latest/API/ErrorResponses.html"),
            "docs.aws.amazon.com/AmazonS3/latest/API/ErrorResponses.html"
        );
        assert_eq!(
            short_url(
                "https://docs.example.com/a/very/long/path/that/keeps/going/on/and/on/ErrorResponses.html"
            ),
            "docs.example.com/a/…/ErrorResponses.html"
        );
    }
}
