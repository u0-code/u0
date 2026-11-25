//! The palette's pages, a stack over its root as t3code's: a new session
//! in a project chosen (⌘1 to ⌘9 there), the look, and adding a project:
//! a path typed and browsed (Tab goes into the folder chosen, ⌘Return
//! takes the path as typed), the folder picker, a repository cloned, a
//! new one made, one on another machine, or a folder the agents already
//! worked in. ⌫ with nothing typed goes back.

use std::path::{Path, PathBuf};

use gpui::{Context, Keystroke, Window};

use super::{Act, Desk, Lead, Row, rank};
use crate::settings::Look;

/// A page of the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    NewIn,
    Look,
    Add,
    Clone,
    Make,
    Remote,
}

impl Sub {
    /// What the page says at its head.
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::NewIn => "New session in…",
            Self::Look => "Change appearance",
            Self::Add => "Add project",
            Self::Clone => "Clone a repository",
            Self::Make => "New project",
            Self::Remote => "On another machine",
        }
    }

    /// What its line to type in asks for.
    pub(super) fn placeholder(self) -> &'static str {
        match self {
            Self::NewIn | Self::Look => "Search…",
            Self::Add => "Enter project path (e.g. ~/projects/my-app)",
            Self::Clone => "Repository URL",
            Self::Make => "Project name",
            Self::Remote => "host:/path",
        }
    }
}

/// A path as typed (`~/a/b`), whole.
fn expand(typed: &str) -> PathBuf {
    match typed.strip_prefix('~') {
        Some(rest) => Desk::home_path()
            .unwrap_or_default()
            .join(rest.trim_start_matches('/')),
        None => PathBuf::from(typed),
    }
}

/// Whether what is typed is a path to browse.
fn is_path(typed: &str) -> bool {
    typed.starts_with('/') || typed.starts_with('~')
}

/// The folder a path typed is in, and the start of the name typed in it.
fn split_path(typed: &str) -> (PathBuf, String) {
    let (dir, start) = typed.rsplit_once('/').unwrap_or((typed, ""));
    (expand(&format!("{dir}/")), start.to_lowercase())
}

impl Desk {
    /// Open the page `sub` over what the palette shows, what was typed
    /// kept to come back to.
    pub(super) fn palette_page(&mut self, sub: Sub, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.palette.state.read(cx).query(cx).to_string();
        self.palette.pages.push((sub, typed));
        if sub == Sub::Remote {
            self.transport.send(crate::proto::Call::Hosts);
        }
        self.palette
            .state
            .update(cx, |s, cx| s.set_query("", window, cx));
        cx.notify();
    }

    /// Back from the page on top, to what was typed before it.
    pub(crate) fn palette_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, typed)) = self.palette.pages.pop() {
            self.palette
                .state
                .update(cx, |s, cx| s.set_query(typed, window, cx));
            cx.notify();
        }
    }

    /// Add a project from the palette: open it, or open the palette at it.
    pub fn add_project_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette.open {
            self.toggle_palette(window, cx);
        }
        self.palette_page(Sub::Add, window, cx);
    }

    /// Keys the palette's pages take before its line does: ⌫ with
    /// nothing typed goes back; in Add project, Tab goes into the folder
    /// chosen and ⌘Return takes the path typed. Whether one was taken.
    pub(crate) fn palette_keys(
        &mut self,
        k: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(&(sub, _)) = self.palette.pages.last().filter(|_| self.palette.open) else {
            return false;
        };
        let typed = self.palette.state.read(cx).query(cx).to_string();
        let m = &k.modifiers;
        match k.key.as_str() {
            "backspace" if typed.is_empty() && !m.modified() => self.palette_pop(window, cx),
            "tab" if sub == Sub::Add && is_path(&typed) && !m.modified() => {
                let chosen = self.palette.state.read(cx).selected_index();
                let row = chosen.and_then(|at| {
                    self.page_groups(sub, &typed)?
                        .into_iter()
                        .nth(at.section)?
                        .1
                        .into_iter()
                        .nth(at.row)
                });
                if let Some(Row {
                    act: Act::Adopt(folder),
                    ..
                }) = row
                {
                    let shown = format!("{}/", Desk::shown(&folder));
                    self.palette
                        .state
                        .update(cx, |s, cx| s.set_query(shown, window, cx));
                }
            }
            "enter" if sub == Sub::Add && m.platform && is_path(&typed) => {
                self.close_palette(window, cx);
                self.add_and_begin(&expand(&typed), window, cx);
            }
            _ => return false,
        }
        true
    }

    /// What is typed changed: look in a folder typed for the folders in
    /// it, and through the project's files for words.
    pub(super) fn palette_typed(&mut self, typed: &str) {
        if self.palette.through == super::Through::Words {
            self.search_words(typed);
        }
        if self.palette.pages.last().is_some_and(|p| p.0 == Sub::Add) && is_path(typed) {
            let (dir, _) = split_path(typed);
            if !self.palette.folders.contains_key(&dir) {
                self.palette.folders.insert(dir.clone(), Vec::new());
                self.transport.send(crate::proto::Call::Folders { dir });
            }
        }
    }

    /// Make `folder` a project and begin a session in it.
    pub(crate) fn add_and_begin(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adopt(folder);
        self.new_in(folder.to_path_buf(), window, cx);
    }

    /// The rows of the page `sub` for what is typed; none where a page
    /// that narrows its rows has none that answer, and the root's search
    /// is shown instead.
    pub(super) fn page_groups(&self, sub: Sub, typed: &str) -> Option<Vec<(String, Vec<Row>)>> {
        let rows = match sub {
            Sub::NewIn => {
                let mut rows: Vec<Row> = self
                    .projects()
                    .into_iter()
                    .enumerate()
                    .map(|(n, p)| Row {
                        lead: Lead::Project(
                            p.clone(),
                            self.held.get(&p).and_then(|h| h.icon.clone()),
                        ),
                        keys: (n < 9)
                            .then(|| crate::keys::numbered(n + 1))
                            .into_iter()
                            .collect(),
                        ..Row::new("", self.project_name(&p), Act::NewIn(p))
                    })
                    .collect();
                rows.push(Row::new(
                    "square-dashed",
                    "No project",
                    Act::Key("new_scratch"),
                ));
                return narrowed(rows, typed);
            }
            Sub::Look => {
                let rows = [
                    (Look::System, "System"),
                    (Look::Light, "Light"),
                    (Look::Dark, "Dark"),
                ]
                .into_iter()
                .map(|(look, word)| Row {
                    end: (self.settings.look == look)
                        .then(|| ("Current".to_owned(), crate::theme::faint())),
                    ..Row::new("sun", word, Act::Look(look))
                })
                .collect();
                return narrowed(rows, typed);
            }
            Sub::Add if is_path(typed) => {
                let (dir, start) = split_path(typed);
                let mut rows: Vec<Row> = self
                    .palette
                    .folders
                    .get(&dir)
                    .into_iter()
                    .flatten()
                    .filter(|n| n.to_lowercase().starts_with(&start))
                    .map(|n| {
                        let folder = dir.join(n);
                        Row::new("folder", n.clone(), Act::Adopt(folder.clone()))
                            .sub(Desk::shown(&folder))
                    })
                    .collect();
                rows.push(Row {
                    keys: Keystroke::parse("cmd-enter").into_iter().collect(),
                    ..Row::new(
                        "folder-plus",
                        format!("Add {typed}"),
                        Act::Adopt(expand(typed)),
                    )
                });
                rows
            }
            Sub::Add => {
                let mut rows = vec![
                    Row::new("folder", "Choose a folder…", Act::Choose),
                    Row::new("branch", "Clone a repository", Act::Sub(Sub::Clone)),
                    Row::new("folder-plus", "New project", Act::Sub(Sub::Make)),
                    Row::new("server", "On another machine", Act::Sub(Sub::Remote)),
                ];
                // The folders the agents already worked in, a step from
                // being projects.
                for (folder, count, _) in self.found_elsewhere().into_iter().take(6) {
                    let s = if count == 1 { "" } else { "s" };
                    rows.push(
                        Row::new("folder", Desk::shown(&folder), Act::Adopt(folder))
                            .sub(format!("{count} session{s} already worked here")),
                    );
                }
                return narrowed(rows, typed);
            }
            Sub::Clone => {
                let mut rows = Vec::new();
                if let Some(c) = &self.adding.cloning {
                    let share = c.share.map(|s| format!(" · {:.0}%", s * 100.));
                    rows.push(Row::new("x", "Stop cloning", Act::CancelClone).sub(format!(
                        "{}{}",
                        c.what,
                        share.unwrap_or_default()
                    )));
                } else if let Some(name) = crate::desk::projects::cloned_name(typed) {
                    rows.push(
                        Row::new(
                            "branch",
                            format!("Clone {typed}"),
                            Act::Clone(typed.to_owned()),
                        )
                        .sub(format!(
                            "into {}",
                            Desk::shown(&self.adding.parent.join(name))
                        )),
                    );
                }
                rows.push(self.parent_row());
                rows
            }
            Sub::Make => {
                let mut rows = Vec::new();
                if !typed.is_empty() {
                    rows.push(
                        Row::new(
                            "folder-plus",
                            format!("Make {typed}"),
                            Act::Make(typed.to_owned()),
                        )
                        .sub(format!(
                            "A repository with a README, in {}",
                            Desk::shown(&self.adding.parent)
                        )),
                    );
                }
                rows.push(self.parent_row());
                rows
            }
            Sub::Remote => {
                let mut rows = Vec::new();
                if crate::desk::projects::remote_typed(typed).is_some() {
                    rows.push(
                        Row::new(
                            "server",
                            format!("Add {typed}"),
                            Act::Remote(typed.to_owned()),
                        )
                        .sub("Over SSH with your key; the agent runs there by npx or uvx"),
                    );
                }
                for host in self.adding.hosts.iter().filter(|h| h.starts_with(typed)) {
                    rows.push(
                        Row::new("server", host.clone(), Act::Fill(format!("{host}:/")))
                            .sub("From ~/.ssh/config"),
                    );
                }
                rows
            }
        };
        Some(vec![(String::new(), rows)])
    }

    /// Where new and cloned projects go, and the way to choose another.
    fn parent_row(&self) -> Row {
        Row::new("folder", "Put new projects in…", Act::ChooseParent)
            .sub(Desk::shown(&self.adding.parent))
    }
}

/// The rows of a page that answer what is typed, best first; none where
/// none does.
fn narrowed(rows: Vec<Row>, typed: &str) -> Option<Vec<(String, Vec<Row>)>> {
    let query = rank::Query::new(typed);
    if query.is_empty() {
        return Some(vec![(String::new(), rows)]);
    }
    let found = rank::rank(vec![((), rows)], |row, typos| {
        let words: Vec<&str> = row.words.iter().map(String::as_str).collect();
        rank::terms(&query, &row.title, &words, typos)
    });
    let rows: Vec<Row> = found
        .into_iter()
        .flat_map(|((), r)| r)
        .map(|(r, _)| r)
        .collect();
    (!rows.is_empty()).then(|| vec![(String::new(), rows)])
}
