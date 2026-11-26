//! The PRs and Issues pages, as t3code has them: a project's pull requests
//! or issues listed by state, by your part in them and by GitHub's search;
//! one read whole beside the list; and what is done to one, as `gh` does
//! it (`core::github`).

use std::path::PathBuf;

use gpui::component::{
    diff::{DiffFile, DiffState},
    input::{InputEvent, InputState, TextareaState},
};
use gpui::{AppContext as _, Context, Entity, Window};

use super::{Desk, Page};
use crate::{
    github::{Act, Answer, Ask, Detail, Item, Kind},
    proto::Call,
    session::State,
};

/// A page's tab for the one read: what it is, what was said, its changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Summary,
    Timeline,
    Code,
}

/// Your part in them, as GitHub's search says it.
pub const PARTS: [(&str, &str); 5] = [
    ("All", ""),
    ("Mine", "author:@me"),
    ("Reviewing", "review-requested:@me"),
    ("Assigned", "assignee:@me"),
    ("Mentions", "mentions:@me"),
];

/// A page of pull requests or issues: which, of which project, in which
/// state and by your part in them; the line to search with; what was
/// listed; the one read, its tab, what was said of it oldest first or
/// not, and its changes; the lines to say something, to name a label or a
/// person, to edit its title and words or write a new issue; and what is
/// being done to it.
pub struct Forge {
    pub kind: Kind,
    pub folder: Option<PathBuf>,
    pub state: &'static str,
    pub part: usize,
    pub search: Entity<InputState>,
    pub list: Option<Result<Vec<Item>, String>>,
    pub chosen: Option<u64>,
    pub detail: Option<Result<Box<Detail>, String>>,
    pub tab: Tab,
    pub newest: bool,
    pub diff: Entity<DiffState>,
    pub line: Entity<TextareaState>,
    pub who: Entity<InputState>,
    pub title: Entity<InputState>,
    pub body: Entity<TextareaState>,
    pub editing: bool,
    pub creating: bool,
    pub doing: Option<String>,
    /// The Linear team the project's issues are read from, as typed.
    pub team: Entity<InputState>,
    /// The project's labels and the people who can review or be
    /// assigned, offered under the line to name one.
    pub candidates: Option<(PathBuf, Vec<String>, Vec<String>)>,
    /// What a failed run of the checks printed, by the run's id.
    pub log: Option<(String, Result<String, String>)>,
    /// The repository's templates for a new issue: name and body.
    pub templates: Vec<(String, String)>,
}

impl Forge {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search: label:bug, author:someone, words")
        });
        cx.subscribe(&search, |this: &mut Desk, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.list_forge(cx);
            }
        })
        .detach();
        // What is typed to name a label or a person narrows those offered.
        let who =
            cx.new(|cx| InputState::new(window, cx).placeholder("A label, or someone's login"));
        cx.subscribe(&who, |_: &mut Desk, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let team = cx.new(|cx| InputState::new(window, cx).placeholder("Linear team"));

        cx.subscribe(&team, |this: &mut Desk, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.bind_linear(cx);
            }
        })
        .detach();
        Self {
            kind: Kind::Pr,
            folder: None,
            state: "open",
            part: 0,
            search,
            list: None,
            chosen: None,
            detail: None,
            tab: Tab::Summary,
            newest: false,
            diff: cx.new(|cx| DiffState::new([], cx)),
            line: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 10)
                    .placeholder("Say something")
            }),
            who,
            title: cx.new(|cx| InputState::new(window, cx).placeholder("Title")),
            body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 16)
                    .placeholder("What it is about")
            }),
            editing: false,
            creating: false,
            doing: None,
            team,
            candidates: None,
            log: None,
            templates: Vec::new(),
        }
    }
}

impl Desk {
    /// Open the page of `kind`, on the project of the session on screen.
    pub fn open_forge(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.forge.kind != kind || self.forge.folder.is_none() {
            self.forge.kind = kind;
            self.forge.folder = self.chosen_project();
            self.forge.state = "open";
            self.forge.chosen = None;
            self.forge.detail = None;
            self.show_team(window, cx);
            self.list_forge(cx);
        }
        self.open_page(Page::Forge(kind), window, cx);
    }

    /// Read what the failed run `run` of the checks printed.
    pub(crate) fn read_log(&mut self, run: String, cx: &mut Context<Self>) {
        self.forge.log = Some((run.clone(), Ok("Reading…".into())));
        self.ask_forge(self.forge.kind, Ask::Log(run));
        cx.notify();
    }

    /// Begin the new issue's words with the template `n`.
    pub(crate) fn use_template(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, body)) = self.forge.templates.get(n) {
            let body = body.clone();
            self.forge
                .body
                .update(cx, |b, cx| b.set_value(body, window, cx));
        }
    }

    /// Ask `ask` of the page's project's pull requests or issues (`kind`),
    /// of its Linear team where it is bound to one.
    fn ask_forge(&self, kind: Kind, ask: Ask) {
        let Some(folder) = self.forge.folder.clone() else {
            return;
        };
        let linear = self
            .settings
            .prefs
            .get(&folder)
            .and_then(|p| p.linear.clone());
        self.transport.send(Call::Forge {
            folder,
            kind,
            ask,
            linear,
        });
    }

    /// Ask for the list the page's choices pick.
    pub(crate) fn list_forge(&mut self, cx: &mut Context<Self>) {
        let words = self.forge.search.read(cx).value().trim().to_owned();
        let search = format!("{} {words}", PARTS[self.forge.part].1)
            .trim()
            .to_owned();
        self.forge.list = None;
        let state = self.forge.state.to_owned();
        self.ask_forge(self.forge.kind, Ask::List { state, search });
        cx.notify();
    }

    /// Bind the page's project's issues to the Linear team typed (its key),
    /// or, with none, to its repository's host; and list them again.
    pub(crate) fn bind_linear(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.forge.folder.clone() else {
            return;
        };
        let team = self.forge.team.read(cx).value().trim().to_uppercase();
        self.settings.prefs.entry(folder).or_default().linear = (!team.is_empty()).then_some(team);
        self.save_settings();
        self.list_forge(cx);
    }

    /// Show the Linear team the page's project is bound to in its field.
    pub(crate) fn show_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let team = self
            .forge
            .folder
            .as_ref()
            .and_then(|f| self.settings.prefs.get(f)?.linear.clone());
        self.forge.team.update(cx, |i, cx| {
            i.set_value(team.unwrap_or_default(), window, cx);
        });
    }

    /// Read the one numbered `n` whole, and a pull request's changes.
    pub fn read_forge(&mut self, n: u64, cx: &mut Context<Self>) {
        self.forge.chosen = Some(n);
        self.forge.log = None;
        if self.forge.candidates.as_ref().map(|c| &c.0) != self.forge.folder.as_ref() {
            self.ask_forge(self.forge.kind, Ask::Candidates);
        }
        self.forge.detail = None;
        self.forge.editing = false;
        self.forge.creating = false;
        self.forge.diff.update(cx, |d, cx| d.set_files([], cx));
        let kind = self.forge.kind;
        self.ask_forge(kind, Ask::View(n));
        if kind == Kind::Pr {
            self.ask_forge(kind, Ask::Diff(n));
        }
        cx.notify();
    }

    /// Do `act` to the one read.
    pub fn act_forge(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(n) = self.forge.chosen else {
            return;
        };
        self.forge.doing = Some(
            format!("{act:?}")
                .split(['(', ' ', '{'])
                .next()
                .unwrap_or("")
                .to_owned(),
        );
        self.ask_forge(self.forge.kind, Ask::Act(n, act));
        cx.notify();
    }

    /// Say what is on the line about the one read: a comment, or with a
    /// verdict a review.
    pub fn say_forge(
        &mut self,
        verdict: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let body = self.forge.line.read(cx).value().trim().to_owned();
        if body.is_empty() && verdict != Some("approve") {
            return;
        }
        self.forge
            .line
            .update(cx, |l, cx| l.set_value("", window, cx));
        let act = match verdict {
            Some(v) => Act::Review {
                verdict: v.to_owned(),
                body,
            },
            None => Act::Comment(body),
        };
        self.act_forge(act, cx);
    }

    /// Begin editing the one read's title and words, or a new issue.
    pub fn edit_forge(&mut self, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (title, body) = match (&self.forge.detail, new) {
            (Some(Ok(d)), false) => (d.item.title.clone(), d.body.clone()),
            _ => (String::new(), String::new()),
        };
        self.forge
            .title
            .update(cx, |i, cx| i.set_value(&title, window, cx));
        self.forge
            .body
            .update(cx, |i, cx| i.set_value(&body, window, cx));
        self.forge.editing = !new;
        self.forge.creating = new;
        if new {
            self.ask_forge(Kind::Issue, Ask::Templates);
        }
        cx.notify();
    }

    /// Save the title and words edited, or make the new issue.
    pub fn save_forge(&mut self, cx: &mut Context<Self>) {
        let title = self.forge.title.read(cx).value().trim().to_owned();
        let body = self.forge.body.read(cx).value().to_string();
        if title.is_empty() {
            return;
        }
        if self.forge.creating {
            self.forge.creating = false;
            self.forge.doing = Some("Create".into());
            self.ask_forge(Kind::Issue, Ask::Create { title, body });
        } else {
            self.forge.editing = false;
            self.act_forge(Act::Edit { title, body }, cx);
        }
        cx.notify();
    }

    /// Begin a session on the pull request `n`: on its own branch in a
    /// worktree, or (`here`) checked out where the project is.
    pub fn start_from_pr(
        &mut self,
        folder: PathBuf,
        n: u64,
        title: String,
        here: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if here {
            self.forge.folder = Some(folder.clone());
            self.forge.chosen = Some(n);
            self.act_forge(Act::Checkout, cx);
        }
        self.new_in(folder, window, cx);
        if let Some(key) = self.current
            && let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
            && !here
        {
            s.wants_worktree = true;
            s.begin = Some(crate::session::Begin::Pr(n, title));
        }
    }

    /// Begin a session about the issue `n`: its number and title, and with
    /// `triage` what is asked of it, begin what you write.
    pub fn start_from_issue(
        &mut self,
        folder: PathBuf,
        n: u64,
        title: &str,
        triage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_in(folder, window, cx);
        let line = if triage {
            format!(
                "Triage issue #{n}: {title}\n\nRead it (gh issue view {n} --comments), find where in the code it comes from and why, and say how to fix it and how big the fix is. Change nothing yet."
            )
        } else {
            format!("Issue #{n}: {title}\n\n")
        };
        self.put_line(&line, window, cx);
    }

    /// Show the pull request `n` of the session's project on the PRs page,
    /// when the session works in a repository: true when it does.
    pub fn show_pr(&mut self, n: u64, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(folder) = self
            .session()
            .filter(|s| s.repo)
            .map(|s| s.meta.project.clone())
        else {
            return false;
        };
        self.forge.folder = Some(folder);
        self.forge.kind = Kind::Pr;
        self.forge.state = "all";
        self.list_forge(cx);
        self.read_forge(n, cx);
        self.open_page(Page::Forge(Kind::Pr), window, cx);
        true
    }

    /// What came back of a project's pull requests or issues.
    pub(super) fn forge_answered(
        &mut self,
        folder: &PathBuf,
        kind: Kind,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        if self.forge.folder.as_ref() != Some(folder) || self.forge.kind != kind {
            return;
        }
        match answer {
            Answer::Listed(list) => self.forge.list = Some(list),
            Answer::Viewed(n, detail) if self.forge.chosen == Some(n) => {
                self.forge.detail = Some(detail);
            }
            Answer::Diffed(n, Ok(patch)) if self.forge.chosen == Some(n) => {
                let parsed = DiffFile::parse(&patch).unwrap_or_default();
                self.forge.diff.update(cx, |d, cx| d.set_files(parsed, cx));
            }
            Answer::Candidates(Ok((labels, people))) => {
                self.forge.candidates = Some((folder.clone(), labels, people));
            }
            Answer::Logged(run, log) => self.forge.log = Some((run, log)),
            Answer::Templates(templates) => self.forge.templates = templates,
            Answer::Acted(n, result) => {
                let what = self.forge.doing.take().unwrap_or_default();
                match result {
                    Ok(said) => self.done(
                        format!(
                            "{what}: done{}",
                            said.lines()
                                .last()
                                .map(|l| format!(" — {l}"))
                                .unwrap_or_default()
                        ),
                        None,
                    ),
                    Err(e) => self.trouble(format!("{what} didn't go through"), Some(e), None),
                }
                // Read again what it did.
                self.list_forge(cx);
                if let Some(n) = n.or(self.forge.chosen) {
                    self.read_forge(n, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

impl Desk {
    /// Keep `token` for `host`, or, with none, take it out of the Keychain.
    pub(crate) fn set_token(
        &mut self,
        host: crate::forge::Host,
        token: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(seen) = self.pages.forges.iter_mut().find(|s| s.host == host) {
            seen.token = token.is_some();
        }
        self.transport.send(Call::Secret {
            account: host.account(),
            value: token,
        });
        cx.notify();
    }
}
