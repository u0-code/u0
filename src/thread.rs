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
pub mod select;
mod topbar;

use std::path::Path;

use agent_client_protocol::v1::schema::{
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

/// A subagent's mark: the live dot while it works, coral while it waits
/// on you, else the person.
pub(crate) fn sub_mark(sub: &crate::session::Sub) -> Div {
    let place = div()
        .size(px(16.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center();
    if sub.needs_you {
        place.child(ui::dot(theme::coral()))
    } else if sub.working() {
        place.child(ui::live_dot())
    } else {
        place.child(icon("user"))
    }
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

pub(crate) fn select_choices(kind: &SessionConfigKind) -> Option<(String, Choices)> {
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

    /// The mark of the agent `id`: its logo where the home holds one (an
    /// agent on another machine shows its own), else its initial.
    pub(crate) fn mark_of(&self, id: &str) -> Mark {
        let agent = crate::remote::split(id).0;
        if self.registry.logos.contains(agent) {
            return Mark::Logo(agent.to_owned().into());
        }
        Mark::Letter(
            self.name_of(id)
                .chars()
                .find(|c| c.is_alphanumeric())
                .map_or('?', |c| c.to_ascii_uppercase()),
        )
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
            Some(s) => {
                col = col.child(self.fresh(faces, s, cx));
            }
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
    fn turn_header(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let name = self.name_of(&s.meta.agent);
        let mut parts = vec![name.clone()];
        parts.extend(Self::option_name(s, &SessionConfigOptionCategory::Model));
        parts.extend(Self::mode_name(s));
        parts.push(Desk::shown(&s.meta.folder));
        // Branched off another: which, to go back to it.
        let origin = s.meta.from.as_ref().and_then(|id| {
            self.sessions.iter().find(|o| {
                o.meta.agent == s.meta.agent && o.meta.id.as_ref().is_some_and(|i| *i.0 == **id)
            })
        });
        // Scripts run in the new worktree: what they were, where to see them.
        let setup = (!s.setup.is_empty()).then(|| {
            let key = s.key;
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mt(px(10.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::border())
                .child(icon("terminal"))
                .child(
                    ui::text(
                        faces,
                        format!("Set up in its new worktree: {}", s.setup.join(", ")),
                    )
                    .flex_1()
                    .text_size(px(13.))
                    .text_color(theme::text2()),
                )
                .child(
                    ui::button(faces, "setup-show", "Show the terminals", ui::Weight::Quiet)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.drawer = true;
                            cx.notify();
                        })),
                )
                .child(
                    ui::icon_button("setup-done", "x", "Put this away").on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Some(s) = this.session_mut(key) {
                                s.setup.clear();
                            }
                            cx.notify();
                        },
                    )),
                )
        });
        div()
            .flex()
            .flex_col()
            .mb(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 14.))
                    .child(meta(faces, parts.join(" · ")))
                    .children(origin.map(|o| {
                        let back = o.key;
                        div()
                            .id("from")
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .child(ui::icon("branch", 12.).text_color(theme::faint()))
                            .child(
                                meta(faces, format!("from “{}”", ui::fit(&o.meta.title, 32)))
                                    .hover(|st| st.text_color(theme::text2())),
                            )
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.show(back, window, cx)),
                            )
                    })),
            )
            .children(setup)
    }

    /// A session begun, before anything is said: where it will work.
    fn fresh(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let name = &self.name_of(&s.meta.agent);
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .mt(px(140.))
            .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 48.))
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
                        ui::mono(faces, Desk::shown(&s.meta.folder))
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
            .children(self.also_row(faces, s, cx))
            .when(s.repo, |d| {
                d.child(Self::where_toggle(faces, s.wants_worktree, cx))
            })
            .when(s.repo && s.wants_worktree, |d| {
                d.child(self.base_chip(faces, s, cx))
            })
    }

    /// The folders a draft works in beside its own, each can be taken
    /// away, and one more to add: where its agent says it can.
    fn also_row(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let able = self
            .slot(&s.meta.agent)
            .and_then(crate::hub::Slot::caps)
            .is_some_and(|k| k.session_capabilities.additional_directories.is_some());
        if !able && s.meta.also.is_empty() {
            return None;
        }
        let key = s.key;
        let mut row = div()
            .flex()
            .flex_wrap()
            .justify_center()
            .items_center()
            .gap(px(6.));
        for (n, folder) in s.meta.also.iter().enumerate() {
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(2.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(theme::tag_line())
                    .child(meta(faces, "also"))
                    .child(
                        ui::mono(faces, Desk::shown(folder))
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                    )
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("also-x-{n}")),
                            "x",
                            "Not this one",
                        )
                        .size(px(20.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(s) = this.session_mut(key)
                                && n < s.meta.also.len()
                            {
                                s.meta.also.remove(n);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        if able {
            row = row.child(
                div()
                    .id("also-add")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::icon("plus", 12.).text_color(theme::faint()))
                    .child(meta(faces, "folder"))
                    .tooltip(ui::tip("Another folder it works in too"))
                    .on_click(cx.listener(|this, _, _, cx| this.add_folder(cx))),
            );
        }
        Some(row)
    }

    /// What a new worktree begins from: where the folder is, unless
    /// another local branch is chosen, or an open pull request, whose own
    /// branch it then works on.
    fn base_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        use crate::session::Begin;
        let shown = match &s.begin {
            Some(Begin::Branch(b)) => b.clone(),
            Some(Begin::Pr(n, title)) => format!("#{n} {title}"),
            None => s.branch.clone().unwrap_or_else(|| "HEAD".into()),
        };
        let key = s.key;
        let mut wrap = div().relative().child(
            div()
                .id("base")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .child(icon("branch"))
                .child(meta(faces, "from"))
                .child(
                    ui::mono(faces, ui::fit(&shown, 32))
                        .text_size(px(12.))
                        .text_color(theme::text2()),
                )
                .child(ui::icon("chevron-down", 11.).text_color(theme::faint()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.want_offers();
                    this.toggle_popup(crate::desk::Popup::Base, cx);
                })),
        );
        if self.popup == crate::desk::Popup::Base {
            let pick = |n: usize, label: String, on: bool, begin: Begin, cx: &mut Context<Self>| {
                div()
                    .id(SharedString::from(format!("base-{n}")))
                    .flex()
                    .items_center()
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::hover()))
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::one_line(ui::mono(faces, label).text_size(px(12.5))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = this.session_mut(key) {
                            s.begin = Some(begin.clone());
                        }
                        this.popup = crate::desk::Popup::Closed;
                        cx.notify();
                    }))
            };
            let mut list = div().flex().flex_col().w(px(300.)).p(px(6.));
            for (n, b) in s.git.branches.iter().enumerate() {
                let on = match &s.begin {
                    Some(Begin::Branch(f)) => f == b,
                    Some(Begin::Pr(..)) => false,
                    None => Some(b) == s.branch.as_ref(),
                };
                list = list.child(pick(n, b.clone(), on, Begin::Branch(b.clone()), cx));
            }
            let prs = self.prs.get(&s.meta.project).filter(|p| !p.is_empty());
            if let Some(prs) = prs {
                list = list.child(
                    div()
                        .px(px(10.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .child(meta(faces, "Pull requests: on its own branch")),
                );
                for (n, (number, title)) in prs.iter().enumerate() {
                    let on = matches!(&s.begin, Some(Begin::Pr(p, _)) if p == number);
                    list = list.child(pick(
                        s.git.branches.len() + n,
                        format!("#{number} {}", ui::fit(title, 34)),
                        on,
                        Begin::Pr(*number, title.clone()),
                        cx,
                    ));
                }
            }
            wrap = wrap.child(gpui::deferred(
                gpui::anchored().child(
                    div().mt(px(30.)).child(
                        ui::menu("base-menu")
                            .max_h(px(360.))
                            .overflow_y_scroll()
                            .child(list),
                    ),
                ),
            ));
        }
        wrap
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
                // Under a message of yours, on hover: when it was said;
                // branching off before it (after the first); rolling the
                // session back to it, the first included; copying it.
                let group = SharedString::from(format!("{key}-you"));
                let idle = matches!(
                    s.state,
                    crate::session::State::Idle | crate::session::State::Closed
                );
                let first = s.items.iter().position(|x| matches!(x, Item::You(_))) == Some(i);
                let branch = idle && s.fork_point(i).is_some() && self.can_fork(s);
                let back = idle && s.meta.id.is_some() && (branch || first);
                let at = s.key;
                let copy = text.clone();
                let rewinding = self.popup == crate::desk::Popup::Rewind(at, i);
                let tools = div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .invisible()
                    .group_hover(group.clone(), Styled::visible)
                    .when(rewinding, Styled::visible)
                    .children(s.time_of(i).map(|t| {
                        meta(faces, when::said_at(t, std::time::SystemTime::now())).mr(px(6.))
                    }))
                    .when(branch, |d| {
                        d.child(
                            Self::under(
                                &key,
                                "branch",
                                "branch",
                                "Branch off here, to say this again",
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.fork_at(at, i, cx))),
                        )
                    })
                    .when(back, |d| {
                        d.child(
                            Self::under(&key, "rewind", "retry", "Roll back to here").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_popup(crate::desk::Popup::Rewind(at, i), cx);
                                }),
                            ),
                        )
                    })
                    .child(
                        Self::under(&key, "copy", "copy", "Copy").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        }),
                    )
                    .children(rewinding.then(|| Self::rewind_menu(faces, s, i, cx)));
                // A long message shows its first lines, the rest a click away.
                let full_key = format!("{key}:full");
                let long = text.lines().count() > 12 || text.len() > 1500;
                let shown = if long && !self.expanded.contains(&full_key) {
                    let head: String = text.lines().take(10).collect::<Vec<_>>().join("\n");
                    format!("{}…", head.chars().take(1200).collect::<String>())
                } else {
                    text.clone()
                };
                let open = self.expanded.contains(&full_key);
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(4.))
                    .group(group)
                    .child(
                        div()
                            .font_family(faces.ui.clone())
                            .text_color(theme::text())
                            .child(select::plain(format!("{key}-you-words"), shown))
                            .max_w(px(520.))
                            .px(px(16.))
                            .py(px(12.))
                            .rounded(px(BUBBLE_RADIUS))
                            .bg(theme::bubble())
                            .text_size(px(14.))
                            .line_height(px(21.)),
                    )
                    .when(long, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(full_key.clone()))
                                .cursor_pointer()
                                .child(
                                    meta(
                                        faces,
                                        if open {
                                            "Show less"
                                        } else {
                                            "Show full message"
                                        },
                                    )
                                    .hover(|st| st.text_color(theme::text2())),
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.toggle(&full_key, cx)),
                                ),
                        )
                    })
                    .child(tools)
                    .into_any_element()
            }
            Item::Agent(text) => {
                // Under what the agent said, on hover: copy it, answer it,
                // or branch off after it.
                let group = SharedString::from(format!("{key}-said"));
                let idle = matches!(
                    s.state,
                    crate::session::State::Idle | crate::session::State::Closed
                );
                let after = idle && s.through(i).is_some() && self.can_fork(s);
                let at = s.key;
                let (copy, quoted) = (text.clone(), text.clone());
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .group(group.clone())
                    .child(div().px(px(10.)).child(markdown::render(
                        faces,
                        &key,
                        text,
                        &self.expanded,
                        cx,
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(2.))
                            .px(px(6.))
                            .invisible()
                            .group_hover(group, Styled::visible)
                            .child(Self::under(&key, "copy", "copy", "Copy").on_click(
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                },
                            ))
                            .child(
                                Self::under(&key, "choose", "list", "Choose words from it")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.read_from(at, i, window, cx);
                                    })),
                            )
                            .child(
                                Self::under(&key, "quote", "reply", "Quote in your reply")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.quote(&quoted, window, cx);
                                    })),
                            )
                            .when(after, |d| {
                                d.child(
                                    Self::under(&key, "after", "branch", "Branch off after this")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.fork_after(at, i, cx);
                                        })),
                                )
                            })
                            .when(after && s.beside.is_none(), |d| {
                                d.child(
                                    Self::under(
                                        &key,
                                        "beside",
                                        "bulb",
                                        "Ask a side question of it",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.ask_beside(at, cx);
                                        },
                                    )),
                                )
                            })
                            .children(s.time_of(i).map(|t| {
                                meta(faces, when::said_at(t, std::time::SystemTime::now()))
                                    .ml(px(6.))
                            })),
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
                // On hover: copied as a checklist, or kept in the folder.
                let written = steps.iter().fold(String::new(), |mut w, (st, text)| {
                    w.push_str(if *st == kinds::Status::Done {
                        "- [x] "
                    } else {
                        "- [ ] "
                    });
                    w.push_str(text);
                    w.push('\n');
                    w
                });
                let (copy, keep) = (written.clone(), written);
                let group = SharedString::from(format!("{key}-plan"));
                let folder = s.meta.folder.clone();
                boxed()
                    .group(group.clone())
                    .child(
                        step()
                            .px(px(2.))
                            .min_h(px(22.))
                            .child(icon("list"))
                            .child(verb(faces, "Plan"))
                            .child(right(faces, format!("{done} of {} done", steps.len())))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(2.))
                                    .invisible()
                                    .group_hover(group, Styled::visible)
                                    .child(
                                        Self::under(&key, "plan-copy", "copy", "Copy the plan")
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(
                                                    gpui::ClipboardItem::new_string(copy.clone()),
                                                );
                                            }),
                                    )
                                    .child(
                                        Self::under(
                                            &key,
                                            "plan-keep",
                                            "file-plus",
                                            "Keep it as PLAN.md in the folder",
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, _| {
                                                this.write(folder.join("PLAN.md"), keep.clone());
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(div().px(px(4.)).child(drawn::plan(faces, &steps)))
                    .into_any_element()
            }
            // An image, yours under what you said, the agent's in its
            // reply; a click looks at it large.
            Item::Image { image, yours } => {
                let (w, h) = if *yours { (240., 180.) } else { (480., 360.) };
                let at = (s.key, i);
                div()
                    .flex()
                    .px(px(10.))
                    .when(*yours, Styled::justify_end)
                    .child(
                        div()
                            .id(SharedString::from(format!("{key}-image")))
                            .cursor_pointer()
                            .child(
                                gpui::img(image.clone())
                                    .max_w(px(w))
                                    .max_h(px(h))
                                    .rounded(px(8.)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.zoom = Some(at);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            // A subagent set to work: its own session, a click away.
            Item::Child { id, title } => {
                let sub = self
                    .sessions
                    .iter()
                    .find(|c| c.meta.id.as_ref() == Some(id) && c.parent.is_some())
                    .map(|c| c.sub.clone())
                    .unwrap_or_default();
                let id = id.clone();
                step()
                    .id(SharedString::from(format!("{key}-child")))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(sub_mark(&sub))
                    .child(verb(faces, "Subagent"))
                    .child(
                        ui::one_line(ui::text(faces, ui::fit(title, 64)))
                            .w_auto()
                            .min_w(px(0.)),
                    )
                    .child(match (sub.needs_you, sub.working(), sub.time()) {
                        (true, _, _) => right_in(faces, "Needs you", theme::coral()),
                        (_, true, t) => right_in(faces, t.unwrap_or_default(), theme::accent()),
                        (_, false, Some(t)) => right(faces, format!("Done · {t}")),
                        _ => right(faces, "Open"),
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(key) = this
                            .sessions
                            .iter()
                            .find(|c| c.meta.id.as_ref() == Some(&id) && c.parent.is_some())
                            .map(|c| c.key)
                        {
                            this.show(key, window, cx);
                        }
                    }))
                    .into_any_element()
            }
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

    /// The image looked at large, over the whole window: the session's
    /// others before and after it a click away; a click outside closes it.
    pub(crate) fn zoomed(
        &self,
        faces: &Faces,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let (key, item) = self.zoom?;
        let s = self.session_at(key)?;
        let images: Vec<usize> = s
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i, Item::Image { .. }))
            .map(|(n, _)| n)
            .collect();
        let Some(Item::Image { image, .. }) = s.items.get(item) else {
            return None;
        };
        let at = images.iter().position(|n| *n == item)?;
        let size = window.viewport_size();
        let step = |id: &'static str, icon: &str, tip: &'static str, to: Option<usize>| {
            ui::icon_button(id, icon, tip)
                .size(px(36.))
                .rounded_full()
                .bg(theme::panel())
                .text_color(theme::text2())
                .when(to.is_none(), gpui::Styled::invisible)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(to) = to {
                        this.zoom = Some((key, to));
                        cx.notify();
                    }
                }))
        };
        let before = at.checked_sub(1).map(|n| images[n]);
        let after = images.get(at + 1).copied();
        Some(
            div()
                .id("zoom")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(16.))
                .bg(gpui::black().opacity(0.78))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.zoom = None;
                    cx.notify();
                }))
                .child(step(
                    "zoom-before",
                    "chevron-left",
                    "The one before",
                    before,
                ))
                .child(
                    gpui::img(image.clone())
                        .max_w(size.width - px(160.))
                        .max_h(size.height - px(120.))
                        .rounded(px(8.)),
                )
                .child(step("zoom-after", "chevron-right", "The one after", after))
                .child(
                    div()
                        .absolute()
                        .top(px(14.))
                        .right(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            ui::text(faces, format!("{} of {}", at + 1, images.len()))
                                .text_size(px(12.5))
                                .text_color(theme::muted()),
                        )
                        .child(
                            ui::icon_button("zoom-close", "x", "Close")
                                .text_color(theme::text2())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.zoom = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Open the agent's message at `item` of the session `key` to choose
    /// words from: to copy them, or quote them in your reply.
    pub fn read_from(
        &mut self,
        key: u64,
        item: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Item::Agent(text)) = self.session_at(key).and_then(|s| s.items.get(item)) else {
            return;
        };
        let text = text.clone();
        self.reader.update(cx, |i, cx| i.set_text(&text, cx));
        self.reading = Some((key, item));
        window.focus(&gpui::Focusable::focus_handle(self.reader.read(cx), cx));
        cx.notify();
    }

    /// The message open to choose words from, over the thread.
    pub(crate) fn reading_view(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.reading?;
        Some(
            div()
                .id("reading")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .pt(px(72.))
                .bg(gpui::black().opacity(0.45))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reading = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("reading-panel")
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .w(px(680.))
                        .max_h(px(560.))
                        .p(px(14.))
                        .rounded(px(14.))
                        .bg(theme::panel())
                        .border_1()
                        .border_color(theme::border2())
                        .shadow_lg()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    meta(
                                        faces,
                                        "Choose the words, then copy them (⌘C) or quote them.",
                                    )
                                    .flex_1(),
                                )
                                .child(
                                    ui::button(
                                        faces,
                                        "reading-quote",
                                        "Quote in your reply",
                                        ui::Weight::Secondary,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            let chosen = this.reader.read(cx).chosen();
                                            let text = chosen
                                                .unwrap_or_else(|| this.reader.read(cx).text());
                                            this.reading = None;
                                            this.quote(&text, window, cx);
                                        },
                                    )),
                                )
                                .child(ui::icon_button("reading-close", "x", "Close").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.reading = None;
                                        cx.notify();
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .id("reading-text")
                                .overflow_y_scroll()
                                .px(px(4.))
                                .child(self.reader.clone()),
                        ),
                )
                .into_any_element(),
        )
    }

    /// A small button under a message.
    fn under(key: &str, what: &str, icon: &str, tip: &'static str) -> gpui::Stateful<Div> {
        ui::icon_button(SharedString::from(format!("{key}-{what}")), icon, tip)
            .size(px(24.))
            .text_color(theme::faint())
    }

    /// How to take the session back to your message at `item`: keeping
    /// the files as they are, or putting them back as they were before.
    fn rewind_menu(
        faces: &Faces,
        s: &Session,
        item: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = s.key;
        let files = s.repo
            && s.turn_of(item)
                .and_then(|t| s.turns.get(t))
                .is_some_and(|t| t.before.is_some());
        let row = |id: &'static str, label: &'static str, about: &'static str| {
            div()
                .id(id)
                .flex()
                .flex_col()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(7.))
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .child(ui::text(faces, label).text_size(px(13.)))
                .child(
                    ui::text(faces, about)
                        .text_size(px(12.))
                        .text_color(theme::faint()),
                )
        };
        gpui::deferred(
            div().absolute().top(px(28.)).right(px(0.)).child(
                ui::menu("rewind-menu").child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(300.))
                        .p(px(6.))
                        .child(
                            row(
                                "rewind-keep",
                                "Roll back to here",
                                "What came after is left behind; it is in the composer to say again. The files stay as they are.",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.rewind(key, item, false, cx);
                                },
                            )),
                        )
                        .when(files, |d| {
                            d.child(
                                row(
                                    "rewind-files",
                                    "Roll back to here, files too",
                                    "The files go back to how they were before this.",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.rewind(key, item, true, cx),
                                )),
                            )
                        }),
                ),
            ),
        )
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
        let row = row_of(tool, &s.meta.folder);
        let open = self.tool_open(s.key, i, tool);
        let running = matches!(
            call.status,
            ToolCallStatus::Pending | ToolCallStatus::InProgress
        );
        let failed = call.status == ToolCallStatus::Failed;
        let (added, gone) = call.content.iter().fold((0, 0), |(a, g), c| match c {
            ToolCallContent::Diff(d) => {
                let file = kinds::diff(d, None);
                (a + file.added, g + file.gone)
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
        let open_diff = diff_of.clone().filter(|_| !running).map(|path| {
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
        // On hover, at its end: a command copied, or run again in a
        // terminal of yours; an edited file opened in your editor.
        let group = SharedString::from(format!("{key}-call"));
        let mut handy = div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .invisible()
            .group_hover(group.clone(), Styled::visible);
        if call.kind == ToolKind::Execute && !running {
            let (copy, again) = (tool.command_line(), tool.command_line());
            handy = handy
                .child(
                    Self::under(&key, "copy-cmd", "copy", "Copy the command").on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        },
                    ),
                )
                .child(
                    Self::under(&key, "again", "play", "Run it in a terminal of yours").on_click(
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_terminal(Some(&again), window, cx);
                        }),
                    ),
                );
        }
        if let Some(path) = diff_of.clone().filter(|_| !running) {
            handy = handy.child(
                Self::under(&key, "editor", "file", "Open in your editor").on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_named(&path.display().to_string());
                    },
                )),
            );
        }
        let toggle = key.clone();
        let mut line = step()
            .id(SharedString::from(format!("{key}-row")))
            .group(group)
            .rounded(px(8.))
            .child(mark)
            .child(verb(faces, row.verb).when(running, |d| d.text_color(theme::accent())))
            .child(object)
            .when(added + gone > 0 && call.kind != ToolKind::Execute, |d| {
                d.child(ui::counts(faces, added, gone, crate::style::META).gap(px(8.)))
            })
            .children(tail)
            .child(div().flex_1())
            .children(open_diff)
            .child(handy);
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
                    let start = tool.starts.get(diff_n).copied().flatten();
                    diff_n += 1;
                    let (file, more) = kinds::cut(kinds::diff(d, start), kinds::LINES_SHOWN);
                    body = body.child(drawn::hunks(faces, &file, drawn::Fit::Thread, None).when(
                        more > 0,
                        |d| {
                            d.child(
                                div()
                                    .pl(px(60.))
                                    .py(px(2.))
                                    .text_color(theme::quiet())
                                    .child(format!("… {more} more lines")),
                            )
                        },
                    ));
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
