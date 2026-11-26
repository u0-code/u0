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
mod form;
pub mod kinds;
mod later;
mod lines;
mod modes;
mod picker;
mod start;

pub use lines::{Find, Line};
pub use picker::Picker;
pub mod output;
mod topbar;

use std::{path::Path, sync::Arc};

use agent_client_protocol::v1::schema::{
    Plan, PlanEntryStatus, SessionConfigKind, SessionConfigOptionCategory,
    SessionConfigSelectOptions, ToolCallContent, ToolCallStatus, ToolKind,
};
use gpui::component::{
    bubble::{Bubble, BubbleContent, BubbleVariant},
    button::Button,
    input::Textarea,
    menu::{PopupMenu, PopupMenuItem},
    message::MessageAlignment,
    popover::Popover,
    scroll::ScrollableElement as _,
};
use gpui::{AnyElement, Context, Div, Entity, SharedString, div, prelude::*, px};

use crate::{
    desk::Desk,
    fonts::Faces,
    git, markdown,
    session::{Format, Item, Picture, Session, Tool, text_of},
    style::BUBBLE_RADIUS,
    theme,
    ui::{self, Mark},
    when,
};

/// A picture of the session's as GPUI draws it: decoded once, and known
/// after that by its hash, as GPUI keeps an image of its own.
fn picture(picture: &Arc<Picture>) -> gpui::ImageSource {
    let picture = picture.clone();
    gpui::ImageSource::Custom(Arc::new(move |window, cx| {
        window.use_asset::<Decoded>(&picture, cx)
    }))
}

/// Pictures decoded for drawing.
enum Decoded {}

impl gpui::Asset for Decoded {
    type Source = Arc<Picture>;
    type Output = Result<Arc<gpui::RenderImage>, gpui::ImageCacheError>;

    fn load(
        picture: Self::Source,
        cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        let format = match picture.format {
            Format::Png => gpui::ImageFormat::Png,
            Format::Jpeg => gpui::ImageFormat::Jpeg,
            Format::Gif => gpui::ImageFormat::Gif,
            Format::Webp => gpui::ImageFormat::Webp,
            Format::Svg => gpui::ImageFormat::Svg,
            Format::Bmp => gpui::ImageFormat::Bmp,
            Format::Tiff => gpui::ImageFormat::Tiff,
            Format::Ico => gpui::ImageFormat::Ico,
        };
        async move {
            gpui::Image::from_bytes(format, picture.bytes.clone())
                .to_image_data(renderer)
                .map_err(Into::into)
        }
    }
}

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
    // A change to the files is said in the past only once it is made: until
    // then it waits for you, or is being made.
    let changed = |wants: &'static str, doing: &'static str, done: &'static str| match call.status {
        ToolCallStatus::Pending => wants,
        ToolCallStatus::InProgress => doing,
        _ => done,
    };
    let (icon, verb, object, kind) = match call.kind {
        ToolKind::Read => ("file", "Read", path_or_title(), Object::Mono),
        ToolKind::Edit if created => (
            "file-plus",
            changed("Wants to create", "Creating", "Created"),
            path_or_title(),
            Object::Mono,
        ),
        ToolKind::Edit => (
            "pencil",
            changed("Wants to edit", "Editing", "Edited"),
            path_or_title(),
            Object::Mono,
        ),
        ToolKind::Delete => (
            "trash",
            changed("Wants to delete", "Deleting", "Deleted"),
            path_or_title(),
            Object::Mono,
        ),
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
                    .truncate()
                    .text_color(theme::code_argument())
                    .child(format!(" {rest}")),
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
        .text_size(px(crate::style::BODY))
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
            .text_size(px(crate::style::CAPTION))
            .line_height(px(18.))
            .text_color(color),
    )
}

fn meta(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(crate::style::CAPTION))
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

/// A well of mono text, for output: on the well's ground, as Claude sets
/// a call's IN and OUT.
pub fn well(faces: &Faces) -> Div {
    div()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(theme::well())
        .font_family(faces.mono.clone())
        .text_size(px(crate::style::LABEL))
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
        self.entry(id).map_or(id, |e| e.name.as_str()).to_owned()
    }

    /// The mark of the agent `id`: its logo where the home holds one (an
    /// account of an agent shows its agent's), else its initial.
    pub(crate) fn mark_of(&self, id: &str) -> Mark {
        let agent = crate::registry::instance(id).0;
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
        let fresh = self.session();
        col = col
            .child(self.start(
                faces,
                column,
                fresh.is_some() || self.unbegun().is_some(),
                cx,
            ))
            .children(fresh.and_then(|s| Self::also_row(faces, s, cx)));
        // The card in the middle of the page, as Claude's.
        div()
            .id("thread")
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .flex_1()
            .min_h(px(0.))
            .child(col)
            .overflow_y_scrollbar()
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
        let origin = self.origin_of(s).and_then(|k| self.sessions.at(k));
        // Scripts run in the new worktree: what they were, whether the
        // session still waits for them, where to see them.
        let setup = (!s.setup.is_empty()).then(|| {
            let key = s.key;
            let waiting = !s.holding.is_empty();
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
                .child(if waiting {
                    gpui::component::spinner::Spinner::new()
                        .color(theme::faint())
                        .into_any_element()
                } else {
                    icon("check").into_any_element()
                })
                .child(
                    ui::text(
                        faces,
                        if waiting {
                            crate::tr_with(
                                "Setting up its new worktree: %{steps}",
                                &[("steps", &s.setup.join(", "))],
                            )
                        } else {
                            crate::tr_with(
                                "Its new worktree is set up: %{steps}",
                                &[("steps", &s.setup.join(", "))],
                            )
                        },
                    )
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(crate::style::LABEL))
                    .text_color(theme::text2()),
                )
                .child(
                    ui::button(
                        "setup-show",
                        crate::tr("Show the terminals"),
                        ui::Weight::Quiet,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer.open = true;
                        cx.notify();
                    })),
                )
                .when(waiting, |d| {
                    d.child(
                        ui::button("setup-cancel", crate::tr("Cancel"), ui::Weight::Quiet)
                            .tooltip(crate::tr("Stop setting it up, and begin"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.cancel_setup(key, cx)),
                            ),
                    )
                })
                .child(
                    ui::icon_button("setup-done", "x", crate::tr("Put this away")).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if let Some(s) = this.sessions.at_mut(key) {
                                s.setup.clear();
                            }
                            cx.notify();
                        }),
                    ),
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
                        let (back, here) = (o.key, s.key);
                        div()
                            .id("from")
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .child(ui::icon("branch", 12.).text_color(theme::faint()))
                            .child(
                                ui::one_line(meta(
                                    faces,
                                    crate::tr_with("from “%{title}”", &[("title", &o.meta.title)]),
                                ))
                                .max_w(px(320.))
                                .hover(|st| st.text_color(theme::text2())),
                            )
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.show(back, window, cx)),
                            )
                            .child(
                                ui::button(
                                    "bring-back",
                                    crate::tr("Bring it back"),
                                    ui::Weight::Quiet,
                                )
                                .tooltip(crate::tr("Name this branch in what you write there"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.bring_back(here, window, cx);
                                    },
                                )),
                            )
                    })),
            )
            .children(setup)
    }

    /// The folders a draft works in beside its own (the composer's + adds
    /// one), each can be taken away.
    fn also_row(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        if s.meta.also.is_empty() {
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
                    .child(meta(faces, crate::tr("also")))
                    .child(
                        ui::mono(faces, Desk::shown(folder))
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::text2()),
                    )
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("also-x-{n}")),
                            "x",
                            crate::tr("Not this one"),
                        )
                        .size(px(20.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(s) = this.sessions.at_mut(key)
                                && n < s.meta.also.len()
                            {
                                s.meta.also.remove(n);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        Some(row)
    }

    /// What a new worktree begins from: where the folder is, unless
    /// another local branch is chosen, or an open pull request, whose own
    /// branch it then works on.
    fn base_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        use crate::session::Begin;
        let shown = match &s.begin {
            Some(Begin::Branch(b)) => b.clone(),
            Some(Begin::Pr(n, title)) => format!("#{n} {title}"),
            None => s.branch.clone().unwrap_or_else(|| "HEAD".into()),
        };
        let chip = ui::opener("base")
            .h(px(26.))
            .px(px(8.))
            .child(icon("branch"))
            .child(meta(faces, crate::tr("from")))
            .child(
                ui::one_line(ui::mono(faces, shown).text_size(px(crate::style::CAPTION)))
                    .max_w(px(240.))
                    .text_color(theme::text2()),
            )
            .child(ui::icon("chevron-down", 11.).text_color(theme::faint()));
        let key = s.key;
        self.menu(
            crate::desk::Popup::Base,
            chip,
            move |d, menu, desk, _| d.base_menu(key, menu, desk),
            cx,
        )
    }

    /// The local branches a new worktree can begin from, then the open
    /// pull requests (asked for as it opens).
    fn base_menu(&self, key: u64, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        use crate::session::Begin;
        let Some(s) = self.sessions.at(key) else {
            return menu;
        };
        let pick = |label: String, on: bool, begin: Begin| {
            Self::menu_item(desk, label, move |this, _, cx| {
                if let Some(s) = this.sessions.at_mut(key) {
                    s.begin = Some(begin.clone());
                }
                cx.notify();
            })
            .checked(on)
        };
        for b in &s.git.branches {
            let on = match &s.begin {
                Some(Begin::Branch(f)) => f == b,
                Some(Begin::Pr(..)) => false,
                None => Some(b) == s.branch.as_ref(),
            };
            menu = menu.item(pick(b.clone(), on, Begin::Branch(b.clone())));
        }
        let prs = (self.prs.get(&crate::desk::Place::project_of(s))).filter(|p| !p.is_empty());
        if let Some(prs) = prs {
            menu = menu
                .separator()
                .label(crate::tr("Pull requests: on its own branch"));
            for (number, title) in prs {
                let on = matches!(&s.begin, Some(Begin::Pr(p, _)) if p == number);
                menu = menu.item(pick(
                    format!("#{number} {}", ui::fit(title, 34)),
                    on,
                    Begin::Pr(*number, title.clone()),
                ));
            }
        }
        menu.min_w(px(288.))
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
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .invisible()
                    .group_hover(group.clone(), Styled::visible)
                    .when(rewinding, Styled::visible)
                    .children(s.time_of(i).map(|t| {
                        meta(
                            faces,
                            when::said_at(t, std::time::SystemTime::now(), self.settings.twelve),
                        )
                        .mr(px(6.))
                    }))
                    .when(branch, |d| {
                        d.child(
                            Self::under(
                                &key,
                                "branch",
                                "branch",
                                crate::tr("Branch off here, to say this again"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.fork_at(at, i, cx))),
                        )
                    })
                    .when(back, |d| {
                        let faces = faces.clone();
                        d.child(
                            self.menu(
                                crate::desk::Popup::Rewind(at, i),
                                Self::under(
                                    &key,
                                    "rewind",
                                    "retry",
                                    crate::tr("Roll back to here"),
                                ),
                                move |d, menu, desk, _| d.rewind_menu(&faces, at, i, menu, desk),
                                cx,
                            )
                            .anchor(gpui::Anchor::TopRight),
                        )
                    })
                    .child(
                        Self::under(&key, "copy", "copy", crate::tr("Copy")).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    copy.clone(),
                                ));
                            },
                        ),
                    );
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
                        Bubble::new()
                            .alignment(MessageAlignment::End)
                            .with_variant(BubbleVariant::Secondary)
                            .max_w(px(520.))
                            .content(
                                BubbleContent::new()
                                    .font_family(faces.ui.clone())
                                    .text_color(theme::text())
                                    .bg(theme::bubble())
                                    .px(px(16.))
                                    .py(px(12.))
                                    .rounded(px(BUBBLE_RADIUS))
                                    .text_size(px(crate::style::BODY))
                                    .line_height(px(21.)),
                            )
                            .child(gpui::base::SelectableText::new(
                                SharedString::from(format!("{key}-you-words")),
                                shown,
                            )),
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
                                            crate::tr("Show less")
                                        } else {
                                            crate::tr("Show full message")
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
                // What is still streaming in shows whole paragraphs only,
                // where that is asked for.
                let streaming = self.settings.paragraphs
                    && i + 1 == s.items.len()
                    && matches!(s.state, crate::session::State::Working);
                let streamed = |t: &str| -> String {
                    if streaming {
                        t[..t.rfind("\n\n").unwrap_or(0)].to_owned()
                    } else {
                        t.to_owned()
                    }
                };
                // Under what the agent said, on hover: copy it, answer it,
                // or branch off after it.
                let group = SharedString::from(format!("{key}-said"));
                let idle = matches!(
                    s.state,
                    crate::session::State::Idle | crate::session::State::Closed
                );
                let after = idle && s.through(i).is_some() && self.can_fork(s);
                let latest = !s.items[i + 1..].iter().any(|x| matches!(x, Item::Agent(_)));
                let at = s.key;
                let (copy, quoted) = (text.clone(), text.clone());
                // When it was said is in each icon's tip, as Claude keeps
                // the row to its icons.
                let said = s
                    .time_of(i)
                    .map(|t| when::said_at(t, std::time::SystemTime::now(), self.settings.twelve));
                let tip = |t: &str| {
                    let t = crate::tr(t);
                    said.as_ref()
                        .map_or_else(|| t.to_string(), |w| format!("{t} · {w}"))
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .group(group.clone())
                    .child(div().px(px(10.)).child(markdown::render(
                        faces,
                        &key,
                        &streamed(text),
                        cx,
                    )))
                    .child(
                        div()
                            .flex()
                            .px(px(6.))
                            // On the latest answer always, as Claude keeps
                            // it; on the others, on hover.
                            .when(!latest, |d| {
                                d.invisible().group_hover(group, Styled::visible)
                            })
                            .child(Self::under(&key, "copy", "copy", tip("Copy")).on_click(
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                },
                            ))
                            .child(
                                Self::under(&key, "choose", "list", tip("Choose words from it"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.read_from(at, i, window, cx);
                                    })),
                            )
                            .child(
                                Self::under(&key, "quote", "reply", tip("Quote in your reply"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.quote(&quoted, Some((at, i)), window, cx);
                                    })),
                            )
                            .when(after, |d| {
                                d.child(
                                    Self::under(
                                        &key,
                                        "after",
                                        "branch",
                                        tip("Branch off after this"),
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.fork_after(at, i, cx);
                                        },
                                    )),
                                )
                            })
                            .when(after && s.beside.is_none(), |d| {
                                d.child(
                                    Self::under(
                                        &key,
                                        "beside",
                                        "bulb",
                                        tip("Ask a side question of it"),
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.ask_beside(at, cx);
                                        },
                                    )),
                                )
                            }),
                    )
                    .into_any_element()
            }
            Item::Thought { text, secs } => {
                let open = self.expanded.contains(&key);
                let toggle = key.clone();
                let label = match secs {
                    Some(n) => crate::tr_with("Thought for %{n}s", &[("n", &n)]),
                    None => crate::tr("Thinking"),
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
                                    ui::text(faces, text.clone())
                                        .text_size(px(crate::style::BODY))
                                        .line_height(px(21.))
                                        .text_color(theme::muted()),
                                ),
                        )
                    })
                    .into_any_element()
            }
            Item::Tool(tool) => self.tool(faces, s, i, tool, cx).into_any_element(),
            Item::Asked(asked) => self.asked(faces, s, &key, asked, cx).into_any_element(),
            // What it asked for: answered, a quiet row with the answer;
            // waiting, where to answer it.
            Item::Elicited(e) => step()
                .child(ui::icon("user", 16.).text_color(theme::faint()))
                .child(verb(
                    faces,
                    match &e.answered {
                        Some(said) => format!("{} · {said}", e.message),
                        None => crate::tr_with(
                            "%{message} · answer it below",
                            &[("message", &e.message)],
                        )
                        .to_string(),
                    },
                ))
                .into_any_element(),
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
                            .child(verb(faces, crate::tr("Plan")))
                            .child(right(
                                faces,
                                crate::tr_with(
                                    "%{done} of %{all} done",
                                    &[("done", &done), ("all", &steps.len())],
                                ),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(2.))
                                    .invisible()
                                    .group_hover(group, Styled::visible)
                                    .child(
                                        Self::under(
                                            &key,
                                            "plan-copy",
                                            "copy",
                                            crate::tr("Copy the plan"),
                                        )
                                        .on_click(
                                            move |_, _, cx| {
                                                cx.write_to_clipboard(
                                                    gpui::ClipboardItem::new_string(copy.clone()),
                                                );
                                            },
                                        ),
                                    )
                                    .child(
                                        Self::under(
                                            &key,
                                            "plan-keep",
                                            "file-plus",
                                            crate::tr("Keep it as PLAN.md in the folder"),
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
            // reply, as high as its row is taken to be before the picture
            // is read; a click looks at it large.
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
                                gpui::img(picture(image))
                                    .max_w(px(w))
                                    .h(px(h))
                                    .rounded(px(8.)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.zoom = Some(at);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            // A file that went with what you said, under it, as it was in
            // the composer; a click opens it.
            Item::File { name, uri } => {
                use gpui::component::Sizable as _;
                use gpui::component::attachment::{
                    Attachment, AttachmentContent, AttachmentMedia, AttachmentTitle,
                };
                let uri = uri.clone();
                div()
                    .flex()
                    .justify_end()
                    .px(px(10.))
                    .child(
                        Attachment::new()
                            .id(SharedString::from(format!("{key}-file")))
                            .small()
                            .media(AttachmentMedia::new().child(ui::icon("file", 16.)))
                            .content(
                                AttachmentContent::new().title(AttachmentTitle::new(name.clone())),
                            )
                            .tooltip(uri.clone())
                            .on_click(move |_, _, cx| {
                                if uri.starts_with("file:") {
                                    cx.open_url(&uri);
                                }
                            }),
                    )
                    .into_any_element()
            }
            // A subagent set to work: its own session, a click away.
            Item::Child { id, title } => {
                let child = self
                    .sessions
                    .all
                    .iter()
                    .find(|c| c.meta.id.as_ref() == Some(id) && c.parent.is_some());
                let sub = child.map(|c| c.sub.clone()).unwrap_or_default();
                let child = child.map(|c| c.key);
                step()
                    .id(SharedString::from(format!("{key}-child")))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(sub_mark(&sub))
                    .child(verb(faces, crate::tr("Subagent")))
                    .child(
                        ui::one_line(ui::text(faces, title.clone()))
                            .w_auto()
                            .min_w(px(0.)),
                    )
                    .child(match (sub.needs_you, sub.working(), sub.time()) {
                        (true, _, _) => right_in(faces, crate::tr("Needs you"), theme::coral()),
                        (_, true, t) => right_in(faces, t.unwrap_or_default(), theme::accent()),
                        (_, false, Some(t)) => {
                            right(faces, crate::tr_with("Done · %{took}", &[("took", &t)]))
                        }
                        _ => right(faces, crate::tr("Open")),
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(key) = child {
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
            } => self.trouble_note(s, i, text, cx).into_any_element(),
            Item::Note { text, .. } => ui::text(faces, text.clone())
                .px(px(10.))
                .text_size(px(crate::style::LABEL))
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
        let s = self.sessions.at(key)?;
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
            ui::icon_button(id, icon, crate::tr(tip))
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
                    gpui::img(picture(image))
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
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "%{n} of %{all}",
                                    &[("n", &(at + 1)), ("all", &images.len())],
                                ),
                            )
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::muted()),
                        )
                        .child(
                            ui::icon_button("zoom-close", "x", crate::tr("Close"))
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
        let Some(Item::Agent(text)) = self.sessions.at(key).and_then(|s| s.items.get(item)) else {
            return;
        };
        let text = text.clone();
        self.reader
            .update(cx, |i, cx| i.set_value(&text, window, cx));
        self.reading = Some((key, item));
        window.focus(&gpui::Focusable::focus_handle(self.reader.read(cx), cx), cx);
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
                                        crate::tr("Choose the words, then copy them or quote them"),
                                    )
                                    .flex_1(),
                                )
                                .child(
                                    ui::button(
                                        "reading-quote",
                                        crate::tr("Quote in your reply"),
                                        ui::Weight::Secondary,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            let reader = this.reader.read(cx);
                                            let chosen = reader.selected_value();
                                            let text = if chosen.is_empty() {
                                                reader.value().to_string()
                                            } else {
                                                chosen.to_string()
                                            };
                                            let from = this.reading.take();
                                            this.quote(&text, from, window, cx);
                                        },
                                    )),
                                )
                                .child(
                                    ui::icon_button("reading-close", "x", crate::tr("Close"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.reading = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id("reading-text")
                                .overflow_y_scroll()
                                .px(px(4.))
                                .child(
                                    Textarea::new(&self.reader).appearance(false).readonly(true),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// A small button under a message.
    fn under(key: &str, what: &str, icon: &str, tip: impl Into<SharedString>) -> Button {
        ui::icon_button(SharedString::from(format!("{key}-{what}")), icon, tip)
            .size(px(24.))
            .text_color(theme::faint())
    }

    /// How to take the session back to your message at `item`: keeping
    /// the files as they are, or putting them back as they were before.
    fn rewind_menu(
        &self,
        faces: &Faces,
        key: u64,
        item: usize,
        menu: PopupMenu,
        desk: &Entity<Self>,
    ) -> PopupMenu {
        let files = self.sessions.at(key).is_some_and(|s| {
            s.repo
                && s.turn_of(item)
                    .and_then(|t| s.turns.get(t))
                    .is_some_and(|t| t.before.is_some())
        });
        let row = |label: &'static str, about: &'static str, files: bool| {
            let faces = faces.clone();
            PopupMenuItem::element(move |_, _| {
                ui::described(&faces, crate::tr(label), Some(crate::tr(about))).w(px(268.))
            })
            .accessibility_label(crate::tr(label))
            .on_click(Self::does(desk, move |this, window, cx| {
                this.rewind(key, item, files, window, cx);
            }))
        };
        menu.item(row(
            "Roll back to here",
            "What came after is left behind; it is in the composer to say again. The files stay as they are.",
            false,
        ))
        .when(files, |m| {
            m.item(row(
                "Roll back to here, files too",
                "The files go back to how they were before this.",
                true,
            ))
        })
    }

    /// A folded stretch of the thread, as Claude sets it: what was done in
    /// it in one muted line, a chevron at its end. A click opens or folds
    /// it; `tip` says more on hover.
    fn fold_row(
        faces: &Faces,
        key: String,
        words: &str,
        tip: Option<String>,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle = key.clone();
        div()
            .id(SharedString::from(key))
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .py(px(2.))
            .cursor_pointer()
            .child(
                ui::one_line(
                    ui::text(faces, words.to_owned())
                        .text_size(px(crate::style::PROSE))
                        .text_color(theme::muted()),
                )
                .w_auto()
                .min_w(px(0.)),
            )
            .child(
                ui::icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    14.,
                )
                .flex_none()
                .text_color(theme::faint()),
            )
            .when_some(tip, |d, tip| d.tooltip(ui::tip(tip)))
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
                let file = git::edit_diff(d, None);
                (a + file.added, g + file.gone)
            }
            _ => (a, g),
        });

        let object: AnyElement = match row.kind {
            Object::Command => command(faces, &row.object, crate::style::LABEL).into_any_element(),
            Object::Mono => ui::one_line(
                ui::mono(faces, row.object.clone())
                    .text_size(px(crate::style::LABEL))
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
                ui::one_line(ui::text(faces, row.object.clone()).text_color(theme::text()))
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
            tool.running()
                .map(|t| right_in(faces, when::took(t), theme::accent()).into_any_element())
        } else if failed {
            Some(right_in(faces, crate::tr("failed"), theme::coral()).into_any_element())
        } else if call.kind == ToolKind::Execute {
            tool.took_said()
                .map(|t| right(faces, when::took(t)).into_any_element())
        } else if call.kind == ToolKind::Search {
            call.content.iter().find_map(|c| match c {
                ToolCallContent::Content(x) => {
                    let n = text_of(&x.content)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .count();
                    let said = if n == 1 {
                        crate::tr("1 match")
                    } else {
                        crate::tr_with("%{n} matches", &[("n", &n)])
                    };
                    Some(right(faces, said).into_any_element())
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
                SharedString::from(format!("{key}-open-diff")),
                crate::tr("Open diff"),
                ui::Weight::Quiet,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.reveal_in_diff(&path, cx);
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
                    Self::under(&key, "copy-cmd", "copy", crate::tr("Copy the command")).on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        },
                    ),
                )
                .child(
                    Self::under(
                        &key,
                        "again",
                        "play",
                        crate::tr("Run it in a terminal of yours"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_terminal(Some(&again), window, cx);
                    })),
                );
        }
        if let Some(path) = diff_of.clone().filter(|_| !running) {
            handy = handy.child(
                Self::under(&key, "editor", "file", crate::tr("Open in your editor")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_named(&path.display().to_string());
                    }),
                ),
            );
        }
        let toggle = key.clone();
        let mut line = step()
            .id(SharedString::from(format!("{key}-row")))
            .group(group)
            .rounded(px(8.))
            .child(mark)
            .child(
                verb(faces, crate::tr(row.verb)).when(running, |d| d.text_color(theme::accent())),
            )
            .child(object)
            .when(added + gone > 0 && call.kind != ToolKind::Execute, |d| {
                d.child(ui::counts(faces, added, gone, crate::style::CAPTION).gap(px(8.)))
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

        // Opened, a call is its line and under it what it did, on the
        // well's ground: one surface, not a box round a box.
        let mut body = div().flex().flex_col().gap(px(6.)).child(line);
        let mut diff_n = 0;
        for (j, c) in call.content.iter().enumerate() {
            match c {
                ToolCallContent::Diff(d) => {
                    let start = tool.starts.get(diff_n).copied().flatten();
                    diff_n += 1;
                    let file = git::edit_diff(d, start);
                    body = body
                        .child(self.edit_view(&format!("{key}-{j}"), &file, cx))
                        .child(
                            div().flex().justify_end().child(
                                ui::button(
                                    SharedString::from(format!("{key}-{j}-in-diff")),
                                    crate::tr("Open in the Diff tab"),
                                    ui::Weight::Quiet,
                                )
                                .h(px(22.))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.show_changes(crate::git::Scope::Call(i), cx);
                                    },
                                )),
                            ),
                        );
                }
                ToolCallContent::Content(x) => {
                    let text = text_of(&x.content);
                    if !text.trim().is_empty() {
                        body = body.child(well(faces).child(output::printed(
                            faces,
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
    fn an_edit_is_said_in_the_past_only_once_it_is_made() {
        let verb = |status: &str| {
            let call = serde_json::from_str(&format!(
                r#"{{"toolCallId":"e","title":"Edit a.rs","kind":"edit","status":"{status}"}}"#
            ))
            .unwrap();
            let tool = Tool {
                call,
                started: None,
                took: None,
                starts: Vec::new(),
            };
            row_of(&tool, Path::new("/r")).verb
        };
        assert_eq!(verb("pending"), "Wants to edit");
        assert_eq!(verb("in_progress"), "Editing");
        assert_eq!(verb("completed"), "Edited");
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
