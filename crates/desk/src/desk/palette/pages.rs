//! The palette's pages, a stack over its root as t3code's: a new session
//! in a project chosen (⌘1 to ⌘9 there), the look, and adding a project:
//! a path typed and browsed (Tab goes into the folder chosen, ⌘Return
//! takes the path as typed), the folder picker, a repository cloned, a
//! new one made, or a folder the agents already worked in. ⌫ with
//! nothing typed goes back.

use std::path::{Path, PathBuf};

use gpui::{Context, Keystroke, Window};

use super::{Act, Desk, Lead, Row, rank};
use crate::desk::Later;
use crate::settings::Look;
use crate::{github::Kind, store::Link};

/// A page of the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    NewIn,
    Look,
    Add,
    Clone,
    Make,
    /// Until when the sessions in `Palette::snoozing` are out of the way.
    Snooze,
    /// A pull request or issue of the project, linked to the session.
    Link,
    /// Tags of the session on screen: those used anywhere, or a new one.
    Tag,
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

            Self::Snooze => "Snooze until…",
            Self::Link => "Link a pull request or issue",
            Self::Tag => "Tag the session",
        }
    }

    /// What its line to type in asks for.
    pub(super) fn placeholder(self) -> &'static str {
        match self {
            Self::NewIn | Self::Look | Self::Link => "Search…",
            Self::Tag => "A tag, or one used before",
            Self::Add => "Enter project path (e.g. ~/projects/my-app)",
            Self::Clone => "Repository URL",
            Self::Make => "Project name",

            Self::Snooze => "In 3h, in 2 days, at 5pm…",
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

    /// Snooze the sessions `keys` until a time you type, or one of the
    /// presets: the palette's page for it.
    pub fn snooze_page(&mut self, keys: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette.open {
            self.toggle_palette(window, cx);
        }
        self.palette.snoozing = keys;
        self.palette_page(Sub::Snooze, window, cx);
    }

    /// The palette's New session in…, over what it shows.
    pub fn new_in_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_page(Sub::NewIn, window, cx);
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

    /// Make `folder` a project, as t3code adds one: made where it is
    /// missing, never twice, taken out of the archive; then its latest
    /// session not done, or a new one.
    pub(crate) fn add_and_begin(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = std::fs::create_dir_all(folder) {
            self.trouble("Couldn't make the folder", Some(e.to_string()), None);
            return;
        }
        if self.prefs(folder).archived {
            self.restore_project(folder, cx);
        }
        self.adopt(folder);
        self.open_project(folder, window, cx);
    }

    /// A project's latest session not done, or a new one in it.
    pub(crate) fn open_project(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let latest = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.meta.project == folder && self.listed_now(s) && s.meta.flags.settled.is_none()
            })
            .max_by_key(|s| super::lists::used(s))
            .map(|s| s.key);
        match latest {
            Some(key) => self.show(key, window, cx),
            None => self.new_in(folder.to_path_buf(), window, cx),
        }
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
                        lead: Lead::Project(self.icon_of(&p)),
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
            Sub::Snooze => {
                let now = std::time::SystemTime::now();
                let mut rows: Vec<Row> = self
                    .snoozes()
                    .into_iter()
                    .map(|(word, later)| Row::new("clock", word, Act::Snooze(later)))
                    .collect();
                let Some(at) = crate::when::ahead(typed, now) else {
                    return narrowed(rows, typed);
                };
                let said = crate::when::said_at(at, now, self.settings.twelve);
                rows.insert(
                    0,
                    Row::new("clock", format!("Until {said}"), Act::Snooze(Later::At(at))),
                );
                rows
            }
            Sub::Tag => {
                let on = self
                    .session()
                    .map(|s| s.meta.tags.clone())
                    .unwrap_or_default();
                let mut all: Vec<String> = (self.sessions.all.iter())
                    .flat_map(|s| s.meta.tags.iter().cloned())
                    .collect();
                all.sort();
                all.dedup();
                let word = typed.trim().trim_start_matches('#').to_lowercase();
                let mut rows: Vec<Row> = all
                    .into_iter()
                    .map(|t| Row {
                        end: on
                            .contains(&t)
                            .then(|| ("Tagged".to_owned(), crate::theme::faint())),
                        ..Row::new("list", format!("#{t}"), Act::Tag(t))
                    })
                    .collect();
                if !word.is_empty() && !word.contains(char::is_whitespace) {
                    let fresh = !rows.iter().any(|r| r.title == format!("#{word}"));
                    if fresh {
                        rows.insert(
                            0,
                            Row::new("plus", format!("Tag it #{word}"), Act::Tag(word)),
                        );
                    }
                }
                return narrowed(rows, typed);
            }
            Sub::Link => {
                let linked = self
                    .session()
                    .map(|s| s.meta.links.clone())
                    .unwrap_or_default();
                let rows = self
                    .github_rows()
                    .into_iter()
                    .flat_map(|(_, rows)| rows)
                    .filter_map(|row| {
                        let (kind, n, title) = match &row.act {
                            Act::FromPr(_, n, t) => (Kind::Pr, *n, t.clone()),
                            Act::FromIssue(_, n, t) => (Kind::Issue, *n, t.clone()),
                            _ => return None,
                        };
                        let link = Link {
                            kind,
                            number: n,
                            title,
                        };
                        Some(Row {
                            end: linked
                                .contains(&link)
                                .then(|| ("Linked".to_owned(), crate::theme::faint())),
                            sub: None,
                            act: Act::Link(link),
                            ..row
                        })
                    })
                    .collect();
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
                } else if let Some(source) = crate::desk::projects::Source::read(typed) {
                    let dir = source.dir(&self.projects_in(), self.settings.host_folders);
                    rows.push(
                        Row::new(
                            "branch",
                            format!("Clone {}", source.url),
                            Act::Clone(typed.to_owned()),
                        )
                        .sub(format!("into {}", Desk::shown(&dir))),
                    );
                }
                rows.push(self.parent_row());
                rows
            }
            Sub::Make => {
                let mut rows = Vec::new();
                if !typed.is_empty() {
                    let at = Desk::shown(&self.projects_in());
                    rows.push(
                        Row::new(
                            "folder-plus",
                            format!("Make {typed}"),
                            Act::Make(typed.to_owned(), false),
                        )
                        .sub(format!("A repository with a README, in {at}")),
                    );
                    if self.on_path.contains(&"gh") {
                        rows.push(
                            Row::new(
                                "globe",
                                format!("Make {typed}, and a private repository on GitHub"),
                                Act::Make(typed.to_owned(), true),
                            )
                            .sub(format!("In {at}, pushed to GitHub by gh")),
                        );
                    }
                }
                rows.push(self.parent_row());
                rows
            }
        };
        Some(vec![(String::new(), rows)])
    }

    /// Where new and cloned projects go, and the way to choose another.
    fn parent_row(&self) -> Row {
        Row::new("folder", "Put new projects in…", Act::ChooseParent)
            .sub(Desk::shown(&self.projects_in()))
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
