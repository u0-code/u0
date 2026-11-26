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

/// A path as typed (`~/a/b`), whole, `~` being `home`.
fn expand(typed: &str, home: &Path) -> PathBuf {
    match typed.strip_prefix('~') {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(typed),
    }
}

/// Whether what is typed is a path to browse.
fn is_path(typed: &str) -> bool {
    typed.starts_with('/') || typed.starts_with('~')
}

/// The folder a path typed is in (`~` being `home`), and the start of the
/// name typed in it.
fn split_path(typed: &str, home: &Path) -> (PathBuf, String) {
    let (dir, start) = typed.rsplit_once('/').unwrap_or((typed, ""));
    (expand(&format!("{dir}/"), home), start.to_lowercase())
}

impl Desk {
    /// Your home on the server a project is added on: on another server,
    /// the folder Ghost's home there is in, as the desk knows no other.
    fn palette_home(&self) -> PathBuf {
        match self.palette.on {
            crate::hub::Host::HERE => Desk::home_path().unwrap_or_default(),
            on => (self.home_on(on).parent())
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        }
    }

    /// Open the page `sub` over what the palette shows, what was typed
    /// kept to come back to. Projects are added on the server of the
    /// session on screen, until another is chosen.
    pub(super) fn palette_page(&mut self, sub: Sub, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.palette.state.read(cx).query(cx).to_string();
        self.palette.pages.push((sub, typed));
        if matches!(sub, Sub::Add | Sub::Clone | Sub::Make) && self.palette.pages.len() == 1 {
            self.palette.on = self.session().map_or(crate::hub::Host::HERE, |s| s.host);
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
                    let shown = format!("{}/", Desk::shown(&folder.folder));
                    self.palette
                        .state
                        .update(cx, |s, cx| s.set_query(shown, window, cx));
                }
            }
            "enter" if sub == Sub::Add && m.platform && is_path(&typed) => {
                self.close_palette(window, cx);
                let at = at_on(self.palette.on, expand(&typed, &self.palette_home()));
                self.add_and_begin(&at, window, cx);
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
            let (dir, _) = split_path(typed, &self.palette_home());
            let at = super::super::Place::new(self.palette.on, dir);
            if !self.palette.folders.contains_key(&at) {
                self.palette.folders.insert(at.clone(), Vec::new());
                self.call(at.host, crate::proto::Call::Folders { dir: at.folder });
            }
        }
    }

    /// Make `p` a project on its server, as t3code adds one: made there
    /// where it is missing, never twice, taken out of the archive; then
    /// its latest session not done, or a new one.
    pub(crate) fn add_and_begin(
        &mut self,
        p: &super::super::Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = p.folder.clone();
        self.call(p.host, crate::proto::Call::MakeFolder { dir });
        if self.prefs(p).archived {
            self.restore_project(p, cx);
        }
        self.adopt(p);
        self.open_project(p, window, cx);
    }

    /// A project's latest session not done, or a new one in it.
    pub(crate) fn open_project(
        &mut self,
        p: &super::super::Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let latest = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.host == p.host
                    && s.meta.project == p.folder
                    && self.listed_now(s)
                    && s.meta.flags.settled.is_none()
            })
            .max_by_key(|s| super::lists::used(s))
            .map(|s| s.key);
        match latest {
            Some(key) => self.show(key, window, cx),
            None => self.new_in(p.clone(), window, cx),
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
                        sub: self.server_chip(p.host).map(str::to_owned),
                        ..Row::new("", self.project_name(&p), Act::NewIn(p))
                    })
                    .collect();
                rows.push(Row::new(
                    "square-dashed",
                    crate::tr("No project"),
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
                    Row::new(
                        "clock",
                        crate::tr_with("Until %{said}", &[("said", &said)]),
                        Act::Snooze(Later::At(at)),
                    ),
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
                            .then(|| (crate::tr("Tagged").to_string(), crate::theme::faint())),
                        ..Row::new("list", format!("#{t}"), Act::Tag(t))
                    })
                    .collect();
                if !word.is_empty() && !word.contains(char::is_whitespace) {
                    let fresh = !rows.iter().any(|r| r.title == format!("#{word}"));
                    if fresh {
                        rows.insert(
                            0,
                            Row::new(
                                "plus",
                                crate::tr_with("Tag it #%{word}", &[("word", &word)]),
                                Act::Tag(word),
                            ),
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
                                .then(|| (crate::tr("Linked").to_string(), crate::theme::faint())),
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
                        .then(|| (crate::tr("Current").to_string(), crate::theme::faint())),
                    ..Row::new("sun", crate::tr(word), Act::Look(look))
                })
                .collect();
                return narrowed(rows, typed);
            }
            Sub::Add if is_path(typed) => {
                let on = self.palette.on;
                let home = self.palette_home();
                let (dir, start) = split_path(typed, &home);
                let at = super::super::Place::new(on, dir.clone());
                let mut rows: Vec<Row> = self
                    .palette
                    .folders
                    .get(&at)
                    .into_iter()
                    .flatten()
                    .filter(|n| n.to_lowercase().starts_with(&start))
                    .map(|n| {
                        let folder = dir.join(n);
                        let shown = Desk::shown(&folder);
                        Row::new("folder", n.clone(), Act::Adopt(at_on(on, folder))).sub(shown)
                    })
                    .collect();
                rows.push(Row {
                    keys: Keystroke::parse("cmd-enter").into_iter().collect(),
                    ..Row::new(
                        "folder-plus",
                        crate::tr_with("Add %{typed}", &[("typed", &typed)]),
                        Act::Adopt(at_on(on, expand(typed, &home))),
                    )
                });
                rows
            }
            Sub::Add => {
                let on = self.palette.on;
                let mut rows = vec![
                    Row::new(
                        "branch",
                        crate::tr("Clone a repository"),
                        Act::Sub(Sub::Clone),
                    ),
                    Row::new("folder-plus", crate::tr("New project"), Act::Sub(Sub::Make)),
                ];
                // macOS's folder picker shows this Mac's folders.
                if on == crate::hub::Host::HERE {
                    rows.insert(
                        0,
                        Row::new("folder", crate::tr("Choose a folder…"), Act::Choose),
                    );
                }
                rows.extend(self.servers_rows());
                // The folders the agents already worked in, a step from
                // being projects.
                for (folder, count, _) in self.found_elsewhere(on).into_iter().take(6) {
                    let worked = if count == 1 {
                        crate::tr("1 session already worked here")
                    } else {
                        crate::tr_with(
                            "%{count} sessions already worked here",
                            &[("count", &count)],
                        )
                    };
                    let shown = Desk::shown(&folder);
                    rows.push(Row::new("folder", shown, Act::Adopt(at_on(on, folder))).sub(worked));
                }
                return narrowed(rows, typed);
            }
            Sub::Clone => {
                let mut rows = Vec::new();
                if let Some(c) = &self.adding.cloning {
                    let share = c.share.map(|s| format!(" · {:.0}%", s * 100.));
                    rows.push(
                        Row::new("x", crate::tr("Stop cloning"), Act::CancelClone).sub(format!(
                            "{}{}",
                            c.what,
                            share.unwrap_or_default()
                        )),
                    );
                } else if let Some(source) = crate::desk::projects::Source::read(typed) {
                    let on = self.palette.on;
                    let dir = source.dir(&self.projects_in(on), self.settings_on(on).host_folders);
                    rows.push(
                        Row::new(
                            "branch",
                            crate::tr_with("Clone %{url}", &[("url", &source.url)]),
                            Act::Clone(typed.to_owned()),
                        )
                        .sub(crate::tr_with(
                            "into %{dir}",
                            &[("dir", &Desk::shown(&dir))],
                        )),
                    );
                }
                rows.push(self.parent_row());
                rows.extend(self.servers_rows());
                rows
            }
            Sub::Make => {
                let mut rows = Vec::new();
                if !typed.is_empty() {
                    let at = Desk::shown(&self.projects_in(self.palette.on));
                    rows.push(
                        Row::new(
                            "folder-plus",
                            crate::tr_with("Make %{typed}", &[("typed", &typed)]),
                            Act::Make(typed.to_owned(), false),
                        )
                        .sub(crate::tr_with(
                            "A repository with a README, in %{at}",
                            &[("at", &at)],
                        )),
                    );
                    if self.on_path.contains(&"gh") {
                        rows.push(
                            Row::new(
                                "globe",
                                crate::tr_with(
                                    "Make %{typed}, and a private repository on GitHub",
                                    &[("typed", &typed)],
                                ),
                                Act::Make(typed.to_owned(), true),
                            )
                            .sub(crate::tr_with(
                                "In %{at}, pushed to GitHub by gh",
                                &[("at", &at)],
                            )),
                        );
                    }
                }
                rows.push(self.parent_row());
                rows.extend(self.servers_rows());
                rows
            }
        };
        Some(vec![(String::new(), rows)])
    }

    /// Where new and cloned projects go; on this Mac, the way to choose
    /// another (macOS's folder picker shows this Mac's).
    fn parent_row(&self) -> Row {
        let on = self.palette.on;
        let choose = if on == crate::hub::Host::HERE {
            Act::ChooseParent
        } else {
            Act::On(on)
        };
        Row::new("folder", crate::tr("Put new projects in…"), choose)
            .sub(Desk::shown(&self.projects_in(on)))
    }

    /// Where a project is added, cloned or made: the server chosen, and a
    /// row for each other server the window holds, to choose it instead.
    fn servers_rows(&self) -> Vec<Row> {
        if self.machines.len() < 2 {
            return Vec::new();
        }
        (self.machines.iter())
            .filter(|m| m.up == crate::hub::Up::On)
            .map(|m| {
                let chosen = m.host == self.palette.on;
                Row {
                    end: chosen
                        .then(|| (crate::tr("Adding here").to_string(), crate::theme::faint())),
                    ..Row::new(
                        "server",
                        crate::tr_with("On %{name}", &[("name", &m.name)]),
                        Act::On(m.host),
                    )
                }
            })
            .collect()
    }
}

/// The folder `folder` on the server `on`.
fn at_on(on: crate::hub::Host, folder: PathBuf) -> super::super::Place {
    super::super::Place::new(on, folder)
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
