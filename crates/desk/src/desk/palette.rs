//! The command palette (⌘K), as t3code's: gpui-kit's Command in a dialog
//! over the window. With nothing typed, what can be done now and the
//! sessions used last; typed, what answers best (`rank`) in t3code's
//! groups. ⌘P looks through the project's files and ⇧⌘F through the words
//! in them, in the same dialog. And the sessions in the sidebar's order,
//! gone to by number (⌘1 to ⌘9) or one after another.

use std::{collections::HashMap, ops::Range, path::PathBuf, time::SystemTime};

use gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, HighlightStyle,
    InteractiveElement as _, IntoElement as _, Keystroke, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Window, div,
    prelude::FluentBuilder as _, px,
};

use gpui::component::{
    IndexPath, WindowExt as _,
    command::{Command, CommandGroup, CommandItem, CommandState},
    kbd::Kbd,
};

use super::{Desk, lists, rank};
use crate::{fonts::Faces, session::State, theme, ui, when};

mod pages;
use pages::Sub;

/// What a row of the palette does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    /// One of the window's actions, by its name in `keys::ACTIONS`.
    Key(&'static str),
    /// One of the palette's pages, opened over it.
    Sub(Sub),
    /// A folder to make a project, and begin a session in.
    Adopt(PathBuf),
    /// The system's folder picker, for a project.
    Choose,
    /// Where new and cloned projects go, chosen.
    ChooseParent,
    Clone(String),
    CancelClone,
    Make(String, bool),
    Remote(String),
    /// What to type in the palette's line, put there.
    Fill(String),
    CopyThread,
    CopyPromptLink,
    Restart,
    Beside,
    Unread,
    Archive,
    Delete,
    PinProject(PathBuf),
    ProjectSettings(PathBuf),
    Look(crate::settings::Look),
    /// The sessions in `Palette::snoozing`, out of the way until then.
    Snooze(super::Later),
    Tab(crate::pages::Tab),
    /// The settings opened on what their search finds of these words:
    /// every setting, and every key binding.
    FindSetting(String),
    Go(u64),
    /// A project: its latest session not done, else a new one.
    Open(PathBuf),
    NewIn(PathBuf),
    /// A file to open, by its name in the project (with ":line").
    File(String),
    /// A new session in a project on a pull request's own branch, in a
    /// worktree.
    FromPr(PathBuf, u64, String),
    /// A new session in a project about an issue: its number and title
    /// begin what you write.
    FromIssue(PathBuf, u64, String),
    /// A pull request or issue linked to the session on screen, or
    /// unlinked where it was.
    Link(crate::store::Link),
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

/// The palette: whether it is open, what is typed and chosen in it, what
/// it looks through and the words it found there.
pub struct Palette {
    pub open: bool,
    pub state: Entity<CommandState>,
    pub through: Through,
    pub grep: Vec<(String, String)>,
    /// The pages open over its root, each with what was typed under it.
    pub pages: Vec<(Sub, String)>,
    /// The folders in each folder a path typed to add a project is in.
    pub folders: HashMap<PathBuf, Vec<String>>,
    /// How words are looked for (⇧⌘F): case, whole words, a pattern.
    pub how: crate::git::Match,
    /// The sessions its Snooze page puts out of the way.
    pub snoozing: Vec<u64>,
    /// What had the keys before it opened, to have them again.
    pub back_to: Option<gpui::FocusHandle>,
}

impl Palette {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        Self {
            open: false,
            state: cx.new(|cx| CommandState::new(window, cx)),
            through: Through::Everything,
            grep: Vec::new(),
            pages: Vec::new(),
            folders: HashMap::new(),
            how: crate::git::Match::default(),
            snoozing: Vec::new(),
            back_to: None,
        }
    }
}

impl Desk {
    /// The sessions as the sidebar lists them, by project or by status;
    /// none done or put away.
    pub(crate) fn ordered(&self) -> Vec<u64> {
        self.side_order.clone()
    }

    /// Go to the `n`th session in the sidebar's order; while a menu of
    /// choices is open, choose its `n`th instead.
    pub fn nth_session(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.choose_nth(n, window, cx) {
            return;
        }
        // In the palette's New session in…, the nth project.
        if self.palette.open && self.palette.pages.last().is_some_and(|p| p.0 == Sub::NewIn) {
            return self.run_row(IndexPath::new(n), window, cx);
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
        let to = if ahead {
            self.trail.forward()
        } else {
            self.trail.back()
        };
        let Some(key) = to else { return };
        self.trailing = true;
        self.show(key, window, cx);
        self.trailing = false;
    }

    /// Keys to the next part of the window (F6), or the one before (⇧F6),
    /// round the sidebar, the composer and the terminals, as t3code cycles
    /// its areas; those hidden are passed over.
    pub fn next_area(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        #[derive(PartialEq)]
        enum Area {
            Sidebar,
            Composer,
            Terminals,
        }
        let line = gpui::Focusable::focus_handle(self.composer.input.read(cx), cx);
        let mut areas = Vec::new();
        if self.panes.left {
            areas.push(Area::Sidebar);
        }
        areas.push(Area::Composer);
        if self.drawer.open {
            areas.push(Area::Terminals);
        }
        let now = if line.contains_focused(window, cx) {
            Area::Composer
        } else if self.drawer.focus.contains_focused(window, cx) {
            Area::Terminals
        } else {
            Area::Sidebar
        };
        let at = areas.iter().position(|a| *a == now).unwrap_or(0);
        let n = areas.len();
        match &areas[if back { (at + n - 1) % n } else { (at + 1) % n }] {
            Area::Sidebar => self.side.update(cx, |tree, cx| tree.focus(window, cx)),
            Area::Composer => window.focus(&line, cx),
            Area::Terminals => window.focus(&self.drawer.focus, cx),
        }
        cx.notify();
    }

    /// Look for `words` in the project's files (git grep), for the palette.
    pub(crate) fn search_words(&mut self, words: &str) {
        let Some(folder) = self.palette_folder() else {
            return;
        };
        if words.trim().len() < 2 {
            self.palette.grep.clear();
            return;
        }
        self.transport.send(crate::proto::Call::Grep {
            folder,
            words: words.trim().to_owned(),
            how: self.palette.how,
        });
    }
}

/// What leads a row.
#[derive(Debug, Clone)]
enum Lead {
    Icon(&'static str),
    Mark(ui::Mark),
    /// A project: its folder, and its own icon file if it has one.
    Project(PathBuf, Option<crate::settings::Icon>),
}

/// A row of the palette: what leads it, its title (the characters in
/// `bold` set heavier), what is said under it and at its end, its keys,
/// whether it opens more, and what it does.
#[derive(Debug, Clone)]
struct Row {
    lead: Lead,
    title: String,
    bold: Vec<Range<usize>>,
    sub: Option<String>,
    end: Option<(String, gpui::Hsla)>,
    keys: Vec<Keystroke>,
    act: Act,
    /// The words it is found by beside its title.
    words: Vec<String>,
    /// The session it is, found as sessions are.
    session: Option<u64>,
}

impl Row {
    fn new(icon: &'static str, title: impl Into<String>, act: Act) -> Self {
        Self {
            lead: Lead::Icon(icon),
            title: title.into(),
            bold: Vec::new(),
            sub: None,
            end: None,
            keys: Vec::new(),
            act,
            words: Vec::new(),
            session: None,
        }
    }

    fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }
}

/// The rows of a group past which it shows no more, and its fewer beside
/// matching sessions.
const SESSIONS: usize = 20;
const BESIDE_SESSIONS: usize = 3;
const RECENT: usize = 12;
const SAID: usize = 10;
const SUGGESTED: usize = 3;
const FILES: usize = 200;
/// How tall the palette's list stands at most: t3code's 420, less its
/// line to type in and its foot.
const LIST: f32 = 344.;

/// An action's icon, by its name in `keys::ACTIONS`.
fn icon_of(name: &str) -> &'static str {
    match name {
        "new_session" | "new_here" => "square-pen",
        "new_scratch" => "square-dashed",
        "find" => "search",
        "drawer" => "terminal",
        "model" | "mode" | "effort" => "sliders",
        "stop" => "square",
        "next_session" => "arrow-down",
        "previous_session" => "arrow-up",
        "toggle_sidebar" => "sidebar",
        "settings" => "settings",
        "review" => "list",
        "right_column" => "panel-right",
        "whole_right" => "expand",
        "usage" => "gauge",
        "pull_requests" => "pull-request",
        "issues" => "circle-dot",
        "light_or_dark" => "sun",
        "copy_pr" | "copy_id" => "copy",
        "settle" => "check",
        "pin" => "pin",
        "take_back" => "undo-2",
        "open_folder" => "folder",
        "dictate" => "mic",
        "back" => "arrow-left",
        "ahead" | "next_area" | "next_terminal" => "arrow-right",
        "previous_area" | "previous_terminal" => "arrow-left",
        "go_to_file" => "file",
        "search_words" => "text-search",
        "pair_device" | "show_pairing_code" => "laptop",
        "palette" => "search",
        "open_surface" => "panel-right",
        "reopen_surface" => "undo-2",
        "close_surface" => "x",
        "clear_terminal" => "terminal",
        "previous_turn" => "arrow-up",
        "next_turn" => "arrow-down",
        "full_screen" => "expand",
        "zoom_in" => "plus",
        "zoom_out" | "minimize" => "minus",
        "check_for_updates" => "retry",
        "quit" => "power",
        // A chevron leads a row into a page of its own, at its end; an
        // action with no mark of its own has the plain one.
        _ => "dots",
    }
}

impl Desk {
    /// Open the palette to look through `through` (⌘K everything, ⌘P
    /// files, ⇧⌘F words); its own key again closes it, another's changes
    /// what it looks through.
    pub fn open_palette(&mut self, through: Through, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.open && self.palette.through == through {
            self.close_palette(window, cx);
            return;
        }
        if through != Through::Everything
            && let Some(folder) = self.palette_folder()
            && !self.files.contains_key(&folder)
        {
            self.files.insert(folder.clone(), Vec::new());
            self.transport
                .send(crate::proto::Call::ListFiles { folder });
        }
        self.palette.through = through;
        self.palette.grep.clear();
        self.palette.pages.clear();
        self.palette.folders.clear();
        self.palette
            .state
            .update(cx, |s, cx| s.set_query("", window, cx));
        if !self.palette.open {
            self.palette.open = true;
            self.palette.back_to = window.focused(cx);
            self.want_offers();
            self.close_settings(window, cx);
            let desk = cx.entity();
            let shown = cx.new(|cx| super::Over::new(desk.clone(), Self::palette_view, cx));
            window.open_dialog(cx, move |dialog, window, _| {
                let desk = desk.clone();
                dialog
                    .w(px(576.))
                    .margin_top((window.viewport_size().height * 0.1).max(px(52.)))
                    .p_0()
                    .rounded(px(16.))
                    .border_color(theme::text().opacity(0.08))
                    .close_button(false)
                    .keyboard(false)
                    .overlay_closable(true)
                    .on_close(move |_, window, cx| {
                        desk.update(cx, |d, cx| d.palette_closed(window, cx));
                    })
                    .child(shown.clone())
            });
        }
        // Once the dialog is drawn, which takes the keys as it opens.
        let state = self.palette.state.clone();
        window.defer(cx, move |window, cx| {
            state.update(cx, |s, cx| s.focus(window, cx));
        });
        cx.notify();
    }

    /// Open the palette over everything (⌘K), or close it.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(Through::Everything, window, cx);
    }

    /// Escape in the palette: from the files or their words back to
    /// everything, else closed.
    pub(crate) fn palette_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.through == Through::Everything {
            self.close_palette(window, cx);
        } else {
            self.open_palette(Through::Everything, window, cx);
        }
    }

    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.open {
            window.close_dialog(cx);
        }
        self.palette_closed(window, cx);
    }

    /// The palette's dialog has gone, closed here or by a click beside it:
    /// the keys go back where they were, else to the composer.
    fn palette_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette.open {
            return;
        }
        self.palette.open = false;
        let back = self
            .palette
            .back_to
            .take()
            .unwrap_or_else(|| gpui::Focusable::focus_handle(self.composer.input.read(cx), cx));
        window.focus(&back, cx);
        cx.notify();
    }

    /// The folder the files are looked through in: the session's, else
    /// the project a new session would begin in.
    fn palette_folder(&self) -> Option<PathBuf> {
        self.session()
            .map(|s| s.meta.folder.clone())
            .or_else(|| self.chosen_project())
    }

    /// Open a file found in its tab in the column beside the session, as
    /// t3code opens it; with no session, in your editor.
    fn open_file(&mut self, name: &str, cx: &mut Context<Self>) {
        let path = PathBuf::from(name.split(':').next().unwrap_or(name));
        if self.session().is_some() {
            self.open_surface(super::Surface::File(path), cx);
        } else {
            self.open_named(name);
        }
    }

    /// What can be done now, in t3code's order, then Ghost's own.
    fn actions(&self, cx: &gpui::App) -> Vec<Row> {
        let key = |name: &'static str, title: &str| Row {
            keys: crate::keys::keys_of(cx, name),
            ..Row::new(icon_of(name), title, Act::Key(name))
        };
        let mut rows = Vec::new();
        if let Some(project) = self.chosen_project() {
            let name = self.project_name(&project);
            let title = format!("New session in {name}");
            let from = title.chars().count() - name.chars().count();
            rows.push(Row {
                bold: std::iter::once(from..title.chars().count()).collect(),
                keys: crate::keys::keys_of(cx, "new_here"),
                ..Row::new("square-pen", title, Act::NewIn(project))
            });
        }
        rows.push(Row {
            keys: crate::keys::keys_of(cx, "new_session"),
            ..Row::new("square-pen", "New session in…", Act::Sub(Sub::NewIn))
        });
        rows.push(key("new_scratch", "New session without a project"));
        let s = self.session();
        if let Some(s) = s {
            rows.push(key("find", "Find in the session"));
            rows.push(Row::new("copy", "Copy the conversation", Act::CopyThread));
            rows.push(Row::new(
                "copy",
                "Copy a link to what you are writing",
                Act::CopyPromptLink,
            ));
            if s.git.pr.is_some() {
                rows.push(key("copy_pr", "Copy the pull request's address"));
            }
            rows.push(Row::new("retry", "Restart the session", Act::Restart));
            let pinned = s.meta.flags.pinned.is_some();
            rows.push(key("pin", if pinned { "Unpin" } else { "Pin" }));
            let settled = s.meta.flags.settled.is_some();
            rows.push(key("settle", if settled { "Un-settle" } else { "Settle" }));
            if !s.meta.flags.unread {
                rows.push(Row::new("bell", "Mark unread", Act::Unread));
            }
            rows.push(Row::new("bulb", "Ask a side question", Act::Beside));
            if s.repo {
                rows.push(Row::new(
                    "pull-request",
                    "Link a pull request or issue",
                    Act::Sub(Sub::Link),
                ));
            }
            if !matches!(s.state, State::Working | State::Opening) {
                rows.push(Row::new("archive", "Archive the session", Act::Archive));
                rows.push(Row::new("trash", "Delete the session", Act::Delete));
            }
        }
        rows.push(key("go_to_file", "Go to file"));
        rows.push(key("search_words", "Search project contents"));
        rows.push(Row::new("folder-plus", "New project", Act::Sub(Sub::Make)));
        rows.push(Row::new("folder", "Add project", Act::Sub(Sub::Add)));
        rows.push(Row::new("sun", "Change appearance", Act::Sub(Sub::Look)));
        rows.push(key("next_area", "Focus next area"));
        rows.push(key("settings", "Open settings"));
        if let Some(project) = self.chosen_project() {
            let pinned = self.prefs(&project).pinned;
            let name = self.project_name(&project);
            rows.push(
                Row::new(
                    "pin",
                    if pinned {
                        "Unpin project"
                    } else {
                        "Pin project"
                    },
                    Act::PinProject(project.clone()),
                )
                .sub(name.clone()),
            );
            rows.push(
                Row::new(
                    "settings",
                    "Project settings",
                    Act::ProjectSettings(project),
                )
                .sub(name),
            );
        }
        // Ghost's own, which t3code has no row for.
        let listed = [
            "palette",
            "minimize",
            "quit",
            "new_session",
            "new_scratch",
            "find",
            "copy_pr",
            "pin",
            "settle",
            "go_to_file",
            "search_words",
            "next_area",
            "settings",
        ];
        for (name, does, _) in crate::keys::ACTIONS {
            if !listed.contains(&name) && (s.is_some() || !needs_session(name)) {
                rows.push(key(name, does));
            }
        }
        rows
    }

    /// A session as the palette lists it: its project, branch and agent
    /// under its title; where it stands and when you last used it at the
    /// end.
    fn found_row(&self, s: &crate::session::Session, now: SystemTime) -> Row {
        let mut under = vec![self.project_name(&s.meta.project)];
        under.extend(s.branch.clone());
        under.push(self.name_of(&s.meta.agent));
        let end = if self.current == Some(s.key) {
            ("Current session".to_owned(), theme::faint())
        } else {
            match lists::status(s) {
                Some(st) => (st.word().to_owned(), st.color()),
                None => (said_ago(lists::used(s), now), theme::faint()),
            }
        };
        Row {
            lead: Lead::Mark(self.mark_of(&s.meta.agent)),
            sub: Some(under.join(" · ")),
            end: Some(end),
            session: Some(s.key),
            ..Row::new("", s.meta.title.clone(), Act::Go(s.key))
        }
    }

    /// The palette's groups for what is typed, each a heading and its
    /// rows, in order.
    fn palette_groups(&self, cx: &gpui::App) -> Vec<(String, Vec<Row>)> {
        let typed = self.palette.state.read(cx).query(cx).trim().to_owned();
        let now = SystemTime::now();
        match self.palette.through {
            Through::Files => {
                let folder = self.palette_folder();
                let name = folder
                    .as_deref()
                    .map_or_else(|| "Files".to_owned(), |f| self.project_name(f));
                let files = folder.as_ref().and_then(|f| self.files.get(f));
                let typed = typed.trim_start_matches('@').trim_start_matches("./");
                let rows = super::rank::paths(files.into_iter().flatten(), typed)
                    .into_iter()
                    .take(FILES)
                    .map(|(f, at)| {
                        let (folder, file) = f.rsplit_once('/').unwrap_or(("", f.as_str()));
                        // The letters in its name are drawn bold.
                        let skip = folder.chars().count() + usize::from(!folder.is_empty());
                        Row {
                            bold: at
                                .into_iter()
                                .filter_map(|n| n.checked_sub(skip))
                                .map(|n| n..n + 1)
                                .collect(),
                            ..Row::new("file", file, Act::File(f.clone())).sub(f.as_str())
                        }
                    })
                    .collect();
                return vec![(name, rows)];
            }
            Through::Words => {
                let rows = self
                    .palette
                    .grep
                    .iter()
                    .map(|(at, text)| {
                        Row::new("file", at.clone(), Act::File(at.clone())).sub(text.trim())
                    })
                    .collect();
                return vec![("Found".to_owned(), rows)];
            }
            Through::Everything => {}
        }
        if let Some(&(sub, _)) = self.palette.pages.last()
            && let Some(groups) = self.page_groups(sub, &typed)
        {
            return groups;
        }
        let sessions = self.sessions.all.iter().filter(|s| {
            s.parent.is_none()
                && s.beside.is_none()
                && !s.meta.flags.archived
                && !self.deleting(s.key)
        });
        // Only actions, after ">".
        let (only_actions, typed) = match typed.strip_prefix('>') {
            Some(rest) => (true, rest.trim().to_owned()),
            None => (false, typed),
        };
        if typed.is_empty() {
            let mut groups = vec![("Actions".to_owned(), self.actions(cx))];
            if !only_actions {
                let mut recent: Vec<&crate::session::Session> = sessions
                    .filter(|s| Some(s.key) != self.current && s.state != State::Draft)
                    .collect();
                recent.sort_by_key(|s| std::cmp::Reverse(lists::used(s)));
                let rows = recent
                    .into_iter()
                    .take(RECENT)
                    .map(|s| self.found_row(s, now))
                    .collect();
                groups.push(("Recent sessions".to_owned(), rows));
            }
            return groups;
        }
        let query = rank::Query::new(&typed);
        let mut groups: Vec<(&str, Vec<Row>)> = vec![("Actions", self.actions(cx))];
        if !only_actions {
            let mut suggested: Vec<Row> = self
                .projects()
                .into_iter()
                .map(|p| {
                    let title = format!("New session in {}", self.project_name(&p));
                    Row::new("square-pen", title, Act::NewIn(p))
                })
                .collect();
            for (look, word) in [
                (crate::settings::Look::System, "System"),
                (crate::settings::Look::Light, "Light"),
                (crate::settings::Look::Dark, "Dark"),
            ] {
                suggested.push(Row::new(
                    "sun",
                    format!("Change appearance: {word}"),
                    Act::Look(look),
                ));
            }
            groups.push(("Suggestions", suggested));
            groups.push((
                "Sessions",
                sessions.clone().map(|s| self.found_row(s, now)).collect(),
            ));
            let projects = self
                .projects()
                .into_iter()
                .map(|p| Row {
                    lead: Lead::Project(p.clone(), self.icon_of(&p)),
                    words: vec![p.display().to_string()],
                    ..Row::new("", self.project_name(&p), Act::Open(p.clone())).sub(Desk::shown(&p))
                })
                .collect();
            groups.push(("Projects", projects));
            let settings = crate::pages::Tab::ALL
                .map(|tab| Row::new(tab.icon(), tab.title(), Act::Tab(tab)).sub("Settings"))
                .into_iter()
                .map(|r| Row {
                    words: vec!["settings".to_owned()],
                    ..r
                })
                // What the settings' own search finds of the words, which
                // reads every setting and key binding where it is drawn.
                .chain(std::iter::once(Row {
                    words: vec![typed.clone(), "settings".to_owned(), "keys".to_owned()],
                    ..Row::new(
                        "search",
                        format!("Search the settings and keys for “{typed}”"),
                        Act::FindSetting(typed.clone()),
                    )
                }))
                .collect();

            groups.push(("Settings", settings));
            groups.extend(self.github_rows());
        }
        let ranked = rank::rank(groups, |row, typos| {
            let Some(key) = row.session else {
                let words: Vec<&str> = row.words.iter().map(String::as_str).collect();
                return rank::terms(&query, &row.title, &words, typos);
            };
            let s = self.sessions.at(key)?;
            rank::session(
                &query,
                &s.meta.title,
                s.git.pr.as_ref().map(|p| p.number),
                s.branch.as_deref(),
                &self.project_name(&s.meta.project),
                lists::used(s),
                now,
                typos,
            )
        });
        let any_session = ranked.iter().any(|(g, _)| *g == "Sessions");
        let mut listed: Vec<u64> = Vec::new();
        let mut out: Vec<(String, Vec<Row>)> = ranked
            .into_iter()
            .map(|(group, rows)| {
                let most = match group {
                    "Sessions" => SESSIONS,
                    "Suggestions" | "Pull requests" | "Issues" => SUGGESTED,
                    _ if any_session => BESIDE_SESSIONS,
                    _ => usize::MAX,
                };
                let mut rows: Vec<Row> = rows.into_iter().take(most).map(|(r, _)| r).collect();
                // The session open sinks to the foot of its group.
                rows.sort_by_key(|r| r.session.is_some() && r.session == self.current);
                listed.extend(rows.iter().filter_map(|r| r.session));
                (group.to_owned(), rows)
            })
            .collect();
        // What was said in a session here, last, for those not listed.
        let words = typed.to_lowercase();
        if !only_actions && words.chars().count() >= 3 {
            let said: Vec<Row> = self
                .sessions
                .all
                .iter()
                .filter(|s| !listed.contains(&s.key) && s.parent.is_none())
                .filter_map(|s| {
                    let line = s.items.iter().find_map(|i| match i {
                        crate::session::Item::You(t) | crate::session::Item::Agent(t) => {
                            t.lines().find(|l| l.to_lowercase().contains(&words))
                        }
                        _ => None,
                    })?;
                    let mut row = self.found_row(s, now);
                    row.sub = Some(ui::fit(line.trim(), 80));
                    Some(row)
                })
                .take(SAID)
                .collect();
            if !said.is_empty() {
                out.push(("In messages".to_owned(), said));
            }
        }
        out
    }

    /// The open pull requests and issues of the project on screen, to
    /// begin a session from.
    fn github_rows(&self) -> Vec<(&'static str, Vec<Row>)> {
        let Some(project) = self.session().map(|s| s.meta.project.clone()) else {
            return Vec::new();
        };
        let of = |map: &HashMap<PathBuf, Vec<(u64, String)>>| {
            map.get(&project).cloned().unwrap_or_default()
        };
        let prs = of(&self.prs)
            .into_iter()
            .map(|(n, title)| {
                Row::new(
                    "branch",
                    format!("#{n} {title}"),
                    Act::FromPr(project.clone(), n, title),
                )
                .sub("A session on its branch")
            })
            .collect();
        let issues = of(&self.issues)
            .into_iter()
            .map(|(n, title)| {
                Row::new(
                    "alert",
                    format!("#{n} {title}"),
                    Act::FromIssue(project.clone(), n, title),
                )
                .sub("A session about it")
            })
            .collect();
        vec![("Pull requests", prs), ("Issues", issues)]
    }

    /// Do the row at `at` of the palette, as it lists now.
    fn run_row(&mut self, at: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self
            .palette_groups(cx)
            .into_iter()
            .nth(at.section)
            .and_then(|(_, rows)| rows.into_iter().nth(at.row))
        else {
            return;
        };
        // What leaves the palette open: a page, what is typed, a clone
        // going on.
        match &row.act {
            Act::Sub(sub) => return self.palette_page(*sub, window, cx),
            Act::Fill(text) => {
                let text = text.clone();
                return self
                    .palette
                    .state
                    .update(cx, |s, cx| s.set_query(text, window, cx));
            }
            Act::ChooseParent => return Self::choose_parent(cx),
            Act::Clone(url) => return self.start_clone(url, cx),
            Act::CancelClone => return self.cancel_clone(cx),
            _ => {}
        }
        self.close_palette(window, cx);
        match row.act {
            Act::Sub(_) | Act::Fill(_) | Act::ChooseParent | Act::Clone(_) | Act::CancelClone => {}
            Act::Adopt(folder) => self.add_and_begin(&folder, window, cx),
            Act::Choose => Self::add_project(window, cx),
            Act::Make(name, publish) => self.create_project(&name, publish, cx),
            Act::Remote(typed) => self.add_remote(&typed, cx),
            Act::Key(name) => {
                if let Some(action) = crate::keys::action(name) {
                    window.dispatch_action(action, cx);
                }
            }
            Act::CopyThread => {
                if let Some(key) = self.current {
                    self.copy_thread(key, cx);
                }
            }
            Act::CopyPromptLink => self.copy_prompt_link(cx),
            Act::Restart => self.restart(window, cx),
            Act::Beside => {
                if let Some(key) = self.current {
                    self.ask_beside(key, cx);
                }
            }
            Act::Unread | Act::Archive | Act::Delete => {
                if let Some(key) = self.current {
                    match row.act {
                        Act::Unread => self.mark_unread(key, cx),
                        Act::Archive => self.archive(key, cx),
                        _ => self.ask_delete(vec![key], window, cx),
                    }
                }
            }
            Act::PinProject(folder) => self.toggle_project_pin(&folder, cx),
            Act::ProjectSettings(folder) => self.project_settings(&folder, window, cx),
            Act::Look(look) => {
                self.settings.look = look;
                self.save_settings();
            }
            Act::Snooze(later) => {
                let keys = std::mem::take(&mut self.palette.snoozing);
                self.snooze(&keys, later, cx);
            }
            Act::Tab(tab) => self.open_tab(tab, window, cx),
            Act::FindSetting(words) => {
                self.open_tab(self.pages.tab, window, cx);
                self.pages
                    .settings
                    .update(cx, |s, cx| s.set_query(words, window, cx));
            }
            Act::File(name) => self.open_file(&name, cx),
            Act::Go(key) => self.show(key, window, cx),
            Act::Open(folder) => self.open_project(&folder, window, cx),
            Act::NewIn(folder) => self.new_in(folder, window, cx),
            Act::FromPr(folder, n, title) => {
                self.start_from_pr(folder, n, title, false, window, cx);
            }
            Act::FromIssue(folder, n, title) => {
                self.start_from_issue(folder, n, &title, false, window, cx);
            }
            Act::Link(link) => {
                if let Some(key) = self.current {
                    self.link(key, link);
                }
            }
        }
        cx.notify();
    }

    /// The palette as its dialog draws it: gpui-kit's Command, given the
    /// rows already found and in order, and t3code's foot of keys.
    fn palette_view(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let faces = crate::fonts::faces(cx).clone();
        let desk = cx.entity();
        let page = self.palette.pages.last().map(|p| p.0);
        let (placeholder, empty) = match self.palette.through {
            Through::Everything if let Some(sub) = page => (sub.placeholder(), "Nothing here."),
            Through::Everything => (
                "Search commands, projects, and sessions…",
                "No matching commands, projects, or sessions.",
            ),
            Through::Files => ("Search files…", "No matching files."),
            Through::Words => ("Search the project's files…", "No matching words."),
        };
        let mut command = Command::new(&self.palette.state)
            .filterable(false)
            .bordered(false)
            .placeholder(placeholder)
            .max_h(px(LIST))
            .on_query({
                let desk = desk.clone();
                move |query, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.palette_typed(query);
                        cx.notify();
                    });
                }
            })
            .on_confirm({
                let desk = desk.clone();
                move |at, window, cx| {
                    desk.update(cx, |d, cx| d.run_row(at, window, cx));
                }
            })
            .empty(move |_, _, _| {
                div()
                    .py(px(40.))
                    .w_full()
                    .text_center()
                    .text_size(px(crate::style::BODY))
                    .text_color(theme::muted())
                    .child(empty)
            })
            .footer({
                let through = self.palette.through;
                move |_, _, _| foot(through, page.is_some())
            });
        // A page says at its head what it is, and goes back when clicked.
        if let Some(sub) = page {
            command = command.header(move |_, _, _| {
                let desk = desk.clone();
                div()
                    .id("palette-back")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(12.))
                    .pt(px(10.))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .child(ui::icon("chevron-left", 12.))
                    .child(sub.title())
                    .on_click(move |_, window, cx| {
                        desk.update(cx, |d, cx| d.palette_pop(window, cx));
                    })
            });
        }
        // Words are looked for in their case, whole, or as a pattern.
        if self.palette.through == Through::Words && page.is_none() {
            let (desk, how) = (cx.entity(), self.palette.how);
            command = command.header(move |_, _, _| match_toggles(&desk, how));
        }
        for (heading, rows) in self.palette_groups(cx) {
            let items = rows.into_iter().map(|row| {
                let faces = faces.clone();
                CommandItem::new()
                    .label(row.title.clone())
                    .child(move |_, _| row_view(&faces, &row))
            });
            // A page's rows go under its head, not a group's.
            let group = CommandGroup::new().items(items);
            command = command.group(if heading.is_empty() {
                group
            } else {
                group.label(heading)
            });
        }
        command.into_any_element()
    }
}

/// How words are looked for (⇧⌘F): in their case, whole, as a pattern;
/// each a toggle that looks again.
fn match_toggles(desk: &Entity<Desk>, how: crate::git::Match) -> gpui::Div {
    let toggle = |id: &'static str,
                  label: &'static str,
                  tip: &'static str,
                  on: bool,
                  flip: fn(&mut crate::git::Match)| {
        let desk = desk.clone();
        ui::button(
            id,
            label,
            if on {
                ui::Weight::Secondary
            } else {
                ui::Weight::Quiet
            },
        )
        .h(px(24.))
        .px(px(6.))
        .tooltip(tip)
        .on_click(move |_, _, cx| {
            desk.update(cx, |d, cx| {
                flip(&mut d.palette.how);
                let words = d.palette.state.read(cx).query(cx).to_string();
                d.search_words(&words);
                cx.notify();
            });
        })
    };
    div()
        .flex()
        .justify_end()
        .gap(px(2.))
        .px(px(12.))
        .pt(px(8.))
        .child(toggle("match-case", "Aa", "Match case", how.case, |h| {
            h.case = !h.case;
        }))
        .child(toggle("match-word", "ab", "Whole words", how.word, |h| {
            h.word = !h.word;
        }))
        .child(toggle(
            "match-regex",
            ".*",
            "A regular expression",
            how.regex,
            |h| h.regex = !h.regex,
        ))
}

/// Whether the action named `name` needs a session on screen to do
/// anything.
fn needs_session(name: &str) -> bool {
    matches!(
        name,
        "model" | "mode" | "effort" | "stop" | "review" | "copy_id" | "open_folder"
    )
}

/// "just now", "5m ago", "3h ago", as t3code dates a row.
fn said_ago(t: SystemTime, now: SystemTime) -> String {
    match when::ago(t, now).as_str() {
        "now" => "just now".to_owned(),
        ago if ago.ends_with(['m', 'h']) => format!("{ago} ago"),
        ago => ago.to_owned(),
    }
}

/// A row as t3code draws it: its icon, the title over what is said under
/// it, then what stands at its end, its keys, and a chevron where it
/// opens more.
fn row_view(faces: &Faces, row: &Row) -> gpui::Div {
    let lead = match &row.lead {
        Lead::Icon(icon) => ui::icon(icon, 16.)
            .text_color(theme::muted())
            .into_any_element(),
        Lead::Mark(mark) => ui::mark(faces, mark, 16.),
        Lead::Project(folder, icon) => super::projects::project_icon(folder, icon.clone(), 16.),
    };
    let title = row.title.clone();
    let bold: Vec<(Range<usize>, HighlightStyle)> = row
        .bold
        .iter()
        .map(|r| {
            let byte = |c: usize| title.char_indices().nth(c).map_or(title.len(), |(b, _)| b);
            (
                byte(r.start)..byte(r.end),
                HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    color: Some(theme::text()),
                    ..HighlightStyle::default()
                },
            )
        })
        .collect();
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .w_full()
        .min_w(px(0.))
        .font_family(faces.ui.clone())
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(px(20.))
                .child(lead),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(crate::style::BODY))
                        .line_height(px(20.))
                        // Regular, as t3code's: only what was typed stands
                        // out, and a project's name.
                        .font_weight(gpui::FontWeight::NORMAL)
                        // A file's letters typed stand out of the rest.
                        .text_color(if matches!(row.act, Act::File(_)) && !row.bold.is_empty() {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .truncate()
                        .child(StyledText::new(title).with_highlights(bold)),
                )
                .children(row.sub.clone().map(|sub| {
                    div()
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(16.))
                        .text_color(theme::muted().opacity(0.7))
                        .truncate()
                        .child(sub)
                })),
        )
        .children(row.end.clone().map(|(end, color)| {
            div()
                .flex_none()
                .min_w(px(48.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(crate::style::CAPTION))
                .text_color(color.opacity(0.85))
                .child(end)
        }))
        .when(!row.keys.is_empty(), |d| {
            d.child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(20.))
                    .items_center()
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::faint())
                    .child(SharedString::from(crate::keys::written(&row.keys))),
            )
        })
        .when(matches!(row.act, Act::Sub(_)), |d| {
            d.child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(20.))
                    .items_center()
                    .child(ui::icon("chevron-right", 14.)),
            )
        })
}

/// The palette's foot: the keys that move, choose and leave, as t3code
/// says them.
fn foot(through: Through, page: bool) -> gpui::Div {
    let hint = |keys: &[&'static str], what: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(keys.iter().map(|k| {
                Kbd::label(*k)
                    .text_size(px(crate::style::CAPTION))
                    .py(px(4.))
            }))
            .child(div().ml(px(2.)).child(what))
    };
    let (enter, esc) = match through {
        Through::Everything => ("Select", "Close"),
        Through::Files | Through::Words => ("Open file", "Back"),
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .py(px(10.))
        .border_t_1()
        .border_color(theme::text().opacity(0.07))
        .bg(theme::text().opacity(0.025))
        .text_size(px(crate::style::BODY))
        .text_color(theme::muted())
        .child(hint(&["↑", "↓"], "Navigate"))
        .child(hint(&["Enter"], enter))
        .child(hint(&["Esc"], esc))
        .when(page, |d| d.child(hint(&["⌫"], "Back")))
}
