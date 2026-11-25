//! The command palette (⌘K), as t3code's: what can be done, the sessions
//! to go to, the projects to begin one in, found by what is typed and
//! grouped; ↑ and ↓ choose, Return does it. And the sessions in the
//! sidebar's order, gone to by number (⌘1 to ⌘9) or one after another
//! (⌘[ and ⌘]).

use std::{collections::HashMap, path::PathBuf};

use gpui::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Desk, Page, Popup};
use crate::{
    fonts::Faces,
    input::{Cancelled, Changed, Stepped, Submitted, TextInput},
    session::State,
    theme, ui,
};

/// What a line of the palette does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    /// One of the window's actions, by its name in `keys::ACTIONS`.
    Key(&'static str),
    AddProject,
    Grouping,
    CopyThread,
    Restart,
    Beside,
    Look(crate::settings::Look),
    Tab(crate::pages::Tab),
    Go(u64),
    NewIn(PathBuf),
    /// A file to open in your editor, by its name (with ":line").
    File(String),
    /// A new session in a project on a pull request's own branch, in a
    /// worktree.
    FromPr(PathBuf, u64, String),
    /// A new session in a project about an issue: its number and title
    /// begin what you write.
    FromIssue(PathBuf, u64, String),
}

/// What the palette looks through: everything, the project's files
/// (⌘P), or the words in them (⇧⌘F).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Through {
    #[default]
    Everything,
    Files,
    Words,
}

/// The palette: whether it is open, its line, the line of it Return
/// takes, what it looks through and the words it found there.
pub struct Palette {
    pub open: bool,
    pub input: Entity<TextInput>,
    pub at: usize,
    pub through: Through,
    pub grep: Vec<(String, String)>,
}

impl Palette {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(cx, "Go to a session, or do something")
                .padded(4.)
                .submitting_empty()
        });
        cx.subscribe(&input, |this, input, _: &Changed, cx| {
            this.palette.at = 0;
            if this.palette.through == Through::Words {
                let words = input.read(cx).text();
                this.search_words(&words);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, Stepped(by), cx| {
            this.step_palette(*by, cx);
        })
        .detach();
        cx.subscribe_in(&input, window, |this, _, Submitted(_), window, cx| {
            this.run_act(this.palette.at, window, cx);
        })
        .detach();
        cx.subscribe_in(&input, window, |this, _, _: &Cancelled, window, cx| {
            this.toggle_palette(window, cx);
        })
        .detach();
        Self {
            open: false,
            input,
            at: 0,
            through: Through::Everything,
            grep: Vec::new(),
        }
    }
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
    /// The sessions as the sidebar lists them, by project or by status;
    /// none done or put away.
    pub(crate) fn ordered(&self) -> Vec<u64> {
        let lists: Vec<Vec<&crate::session::Session>> = if self.settings.by_status {
            self.by_status_groups(None)
                .0
                .into_iter()
                .map(|(_, v)| v)
                .collect()
        } else {
            self.projects().iter().map(|f| self.in_project(f)).collect()
        };
        lists.into_iter().flatten().map(|s| s.key).collect()
    }

    /// Go to the `n`th session in the sidebar's order; while a menu of
    /// choices is open, choose its `n`th instead.
    pub fn nth_session(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.choose_nth(n, cx) {
            return;
        }
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

    /// Copy what `of` reads from the session on screen, if it has it.
    pub fn copy_of_session(
        &mut self,
        of: impl Fn(&crate::session::Session) -> Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(text) = self.session().and_then(of) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.done("Copied", None);
            cx.notify();
        }
    }

    /// Back to the session shown before (or ahead again), as a browser
    /// goes back.
    pub fn step_trail(&mut self, ahead: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (from, to) = if ahead {
            (&mut self.ahead, &mut self.trail)
        } else {
            (&mut self.trail, &mut self.ahead)
        };
        let Some(key) = from.pop() else { return };
        if let Some(now) = self.current {
            to.push(now);
        }
        self.trailing = true;
        self.show(key, window, cx);
        self.trailing = false;
    }

    /// Keys to the next part of the window (F6): the line you write in,
    /// the terminals when their drawer is open, and back.
    pub fn next_area(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_line =
            gpui::Focusable::focus_handle(self.composer.input.read(cx), cx).is_focused(window);
        if in_line && self.drawer.open {
            window.focus(&self.drawer.focus);
        } else {
            window.focus(&gpui::Focusable::focus_handle(
                self.composer.input.read(cx),
                cx,
            ));
        }
        cx.notify();
    }

    /// Open the model's menu in the composer (⌘⇧M).
    pub fn open_model(&mut self, cx: &mut Context<Self>) {
        self.open_option(
            &agent_client_protocol::v1::schema::SessionConfigOptionCategory::Model,
            cx,
        );
    }

    /// Open the menu of the session's option of the kind `which` (its
    /// model, mode, or how hard it thinks); for a mode, its modes where it
    /// has no such option.
    pub fn open_option(
        &mut self,
        which: &agent_client_protocol::v1::schema::SessionConfigOptionCategory,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.session() else { return };
        let found = s
            .options
            .iter()
            .find(|o| o.category.as_ref() == Some(which));
        self.popup = match found {
            Some(o) => Popup::Option(o.id.0.to_string()),
            None if *which
                == agent_client_protocol::v1::schema::SessionConfigOptionCategory::Mode
                && s.modes.is_some() =>
            {
                Popup::Mode
            }
            None => return,
        };
        cx.notify();
    }

    /// Open the palette to look through `through` (⌘P files, ⇧⌘F words).
    pub fn open_palette(&mut self, through: Through, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.session() {
            self.transport.send(crate::proto::Call::ListFiles {
                folder: s.meta.folder.clone(),
            });
        }
        self.palette.open = false;
        self.toggle_palette(window, cx);
        self.palette.through = through;
        self.palette.grep.clear();
        let says = match through {
            Through::Files => "Go to a file",
            Through::Words => "Search the project's files",
            Through::Everything => "Go to anything",
        };
        self.palette
            .input
            .update(cx, |i, cx| i.set_placeholder(says, cx));
    }

    /// Look for `words` in the project's files (git grep), for the palette.
    pub(crate) fn search_words(&mut self, words: &str) {
        let Some(s) = self.session() else { return };
        if words.trim().len() < 2 {
            self.palette.grep.clear();
            return;
        }
        self.transport.send(crate::proto::Call::Grep {
            folder: s.meta.folder.clone(),
            words: words.trim().to_owned(),
        });
    }

    /// Open the palette, or close it.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette.through = Through::Everything;
        self.palette
            .input
            .update(cx, |i, cx| i.set_placeholder("Go to anything", cx));
        self.palette.open = !self.palette.open;
        self.palette.at = 0;
        if self.palette.open {
            self.want_offers();
            self.palette.input.update(cx, |i, cx| i.set_text("", cx));
            window.focus(&self.palette.input.focus_handle(cx));
        } else {
            window.focus(&self.composer.input.focus_handle(cx));
        }
        cx.notify();
    }

    /// Move the palette's choice up (below zero) or down.
    pub(crate) fn step_palette(&mut self, by: isize, cx: &mut Context<Self>) {
        let query = self.palette.input.read(cx).text();
        let last = self.lines(&query, cx).len().saturating_sub(1);
        self.palette.at = self.palette.at.saturating_add_signed(by).min(last);
        cx.notify();
    }

    /// The lines the palette offers for what is typed: with nothing typed
    /// what can be done leads, else the sessions found.
    fn lines(&self, query: &str, cx: &gpui::App) -> Vec<Line> {
        let q = query.trim().to_lowercase();
        // The project's files, or what was found in them, alone.
        let file = |name: String, hint: String| Line {
            group: "Files",
            label: name.clone(),
            hint,
            act: Act::File(name),
            agent: None,
            icon: "file",
            live: false,
        };
        match self.palette.through {
            Through::Files => {
                let project = self
                    .session()
                    .map(|s| s.meta.folder.clone())
                    .unwrap_or_default();
                return super::mention::ranked(self.files.get(&project), &q)
                    .into_iter()
                    .take(LINES)
                    .map(|f| file(f.clone(), String::new()))
                    .collect();
            }
            Through::Words => {
                return self
                    .palette
                    .grep
                    .iter()
                    .take(LINES)
                    .map(|(at, text)| {
                        let mut l = file(at.clone(), ui::fit(text.trim(), 48));
                        l.group = "Found";
                        l
                    })
                    .collect();
            }
            Through::Everything => {}
        }
        let found = |text: &str| q.is_empty() || text.to_lowercase().contains(&q);
        let act = |label: &str, act: Act, icon: &'static str| Line {
            group: "Actions",
            label: label.to_owned(),
            hint: String::new(),
            act,
            agent: None,
            icon,
            live: false,
        };
        // The window's actions, then what only the palette does.
        let actions: Vec<Line> = crate::keys::ACTIONS
            .iter()
            .filter(|(name, _, _)| *name != "palette")
            .map(|(name, does, _)| Line {
                hint: crate::keys::keys_of(cx, name).unwrap_or_default(),
                ..act(does, Act::Key(name), "chevron-right")
            })
            .chain([
                act("Add a project", Act::AddProject, "folder"),
                act("Group sessions by project or status", Act::Grouping, "list"),
                act("Copy this conversation", Act::CopyThread, "copy"),
                act("Restart the session", Act::Restart, "retry"),
                act("Ask a side question", Act::Beside, "bulb"),
                act(
                    "Look: dark",
                    Act::Look(crate::settings::Look::Dark),
                    "settings",
                ),
                act(
                    "Look: light",
                    Act::Look(crate::settings::Look::Light),
                    "settings",
                ),
                act(
                    "Look: as the system",
                    Act::Look(crate::settings::Look::System),
                    "settings",
                ),
            ])
            .chain(crate::pages::Tab::ALL.map(|tab| {
                act(
                    &format!("Settings: {}", tab.title()),
                    Act::Tab(tab),
                    "settings",
                )
            }))
            .filter(|l| found(&l.label))
            .collect();
        let mut sessions = Vec::new();
        for (n, key) in self.ordered().into_iter().enumerate() {
            let Some(s) = self.sessions.at(key) else {
                continue;
            };
            let project = self.project_name(&s.meta.project);
            // Found by what was said in it too, where its thread is here.
            let said = q.len() >= 3
                && s.items.iter().any(|i| match i {
                    crate::session::Item::You(t) | crate::session::Item::Agent(t) => {
                        t.to_lowercase().contains(&q)
                    }
                    _ => false,
                });
            if !found(&s.meta.title) && !found(&project) && !said {
                continue;
            }
            let hint = if said && !found(&s.meta.title) {
                format!("{project} · said in it")
            } else if n < 9 {
                format!("{project}  ⌘{}", n + 1)
            } else {
                project
            };
            sessions.push(Line {
                group: "Sessions",
                label: s.meta.title.clone(),
                hint,
                act: Act::Go(key),
                agent: Some(s.meta.agent.clone()),
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
        // The open pull requests and issues of the project on screen, to
        // begin a session from.
        let project = self.session().map(|s| s.meta.project.clone());
        let mut github = Vec::new();
        if let Some(project) = project {
            let of = |map: &HashMap<PathBuf, Vec<(u64, String)>>| {
                map.get(&project).cloned().unwrap_or_default()
            };
            for (n, title) in of(&self.prs) {
                let label = format!("#{n} {title}");
                if found(&label) {
                    github.push(Line {
                        group: "Pull requests",
                        label,
                        hint: "a session on its branch".into(),
                        act: Act::FromPr(project.clone(), n, title),
                        agent: None,
                        icon: "branch",
                        live: false,
                    });
                }
            }
            for (n, title) in of(&self.issues) {
                let label = format!("#{n} {title}");
                if found(&label) {
                    github.push(Line {
                        group: "Issues",
                        label,
                        hint: "a session about it".into(),
                        act: Act::FromIssue(project.clone(), n, title),
                        agent: None,
                        icon: "alert",
                        live: false,
                    });
                }
            }
        }
        let mut all = if q.is_empty() {
            let mut all = actions;
            all.extend(sessions.into_iter().take(5));
            all
        } else {
            let mut all = sessions;
            all.extend(actions);
            all.extend(github);
            all
        };
        all.extend(projects);
        all.truncate(LINES);
        all
    }

    /// Do the line `n` of the palette.
    pub fn run_act(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.palette.input.read(cx).text();
        let Some(line) = self.lines(&query, cx).into_iter().nth(n) else {
            return;
        };
        self.palette.open = false;
        window.focus(&self.composer.input.focus_handle(cx));
        match line.act {
            Act::Key(name) => {
                if let Some(action) = crate::keys::action(name) {
                    window.dispatch_action(action, cx);
                }
            }
            Act::AddProject => self.open_page(Page::NewProject, cx),
            Act::CopyThread => {
                if let Some(key) = self.current {
                    self.copy_thread(key, cx);
                }
            }
            Act::Restart => self.restart(window, cx),
            Act::Look(look) => {
                self.settings.look = look;
                self.save_settings();
            }
            Act::Beside => {
                if let Some(key) = self.current {
                    self.ask_beside(key, cx);
                }
            }
            Act::Tab(tab) => self.open_tab(tab, cx),
            Act::File(name) => self.open_named(&name),
            Act::Grouping => self.toggle_grouping(cx),
            Act::Go(key) => self.show(key, window, cx),
            Act::NewIn(folder) => self.new_in(folder, window, cx),
            Act::FromPr(folder, n, title) => {
                self.new_in(folder, window, cx);
                if let Some(key) = self.current
                    && let Some(s) = self.sessions.at_mut(key)
                    && s.state == State::Draft
                {
                    s.wants_worktree = true;
                    s.begin = Some(crate::session::Begin::Pr(n, title));
                }
            }
            Act::FromIssue(folder, n, title) => {
                self.new_in(folder, window, cx);
                self.composer.input.update(cx, |i, cx| {
                    i.set_text(&format!("Issue #{n}: {title}\n\n"), cx);
                });
            }
        }
        cx.notify();
    }

    /// The palette, over the window's top.
    pub(crate) fn palette_view(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.palette.open {
            return None;
        }
        let query = self.palette.input.read(cx).text();
        let mut list = div().flex().flex_col().p(px(6.));
        let mut group = "";
        for (n, line) in self.lines(&query, cx).into_iter().enumerate() {
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
                Some(agent) => ui::mark(faces, &self.mark_of(agent), 16.),
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
                    .when(n == self.palette.at, |d| d.bg(theme::panel2()))
                    .when(n != self.palette.at, |d| {
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
                                .child(div().flex_1().child(self.palette.input.clone()))
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
