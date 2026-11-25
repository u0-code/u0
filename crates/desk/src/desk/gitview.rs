//! What git says where a session works, read for the column beside it as
//! t3code reads it: the branch against where it left the main one, one
//! commit, the recent commits, the local branches (one can be checked out,
//! never under another session at work), the branch's pull request and its
//! checks. And a commit message the writer writes, when asked to.

use std::{fmt::Write as _, path::PathBuf};

use gpui::{AppContext as _, Context, Entity, Window};

use gpui::component::{
    diff::{DiffLinePosition, DiffState},
    input::{InputEvent, InputState},
    tree::TreeState,
};

use super::Desk;
use crate::{git::Scope, session::State};

/// The changes in the column beside: which it shows; the commit message
/// being written and whether its form is open; what you say of a file's
/// lines (the line being commented on, the line to write it in, and those
/// said, waiting to go to the agent); whether test files are left out.
/// gpui-kit's view of them and the tree of their files, with what the view
/// holds (session, scope, how many files, how much patch) and whether the
/// tree shows; the words found in them, each line found, and the one on.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about the review, not a state among several"
)]
pub struct Review {
    pub scope: Scope,
    pub message: Entity<InputState>,
    pub committing: bool,
    pub commenting: Option<(PathBuf, usize)>,
    pub comment: Entity<InputState>,
    pub comments: Vec<(PathBuf, usize, String)>,
    pub no_tests: bool,
    pub diff: Entity<DiffState>,
    pub tree: Entity<TreeState>,
    pub shown: Option<(u64, Scope, usize, usize)>,
    pub tree_open: bool,
    pub find: Entity<InputState>,
    pub finding: bool,
    pub hits: Vec<(DiffLinePosition, std::ops::Range<usize>)>,
    pub hit: usize,
    /// A file to bring into view once the view holds it.
    pub reveal: Option<String>,
}

impl Review {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let comment = cx.new(|cx| InputState::new(window, cx).placeholder("What of this line?"));
        cx.subscribe_in(
            &comment,
            window,
            |this, input, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    let text = input.read(cx).value().trim().to_owned();
                    if !text.is_empty() {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                        this.add_comment(&text, cx);
                    }
                }
            },
        )
        .detach();
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Find in the diff"));
        cx.subscribe_in(&find, window, |this, _, e: &InputEvent, _, cx| match e {
            InputEvent::Change => this.find_in_diff(cx),
            InputEvent::PressEnter { .. } => this.step_hit(1, cx),
            _ => {}
        })
        .detach();
        Self {
            scope: Scope::All,
            message: cx.new(|cx| InputState::new(window, cx).placeholder("Commit message")),
            committing: false,
            commenting: None,
            comment,
            comments: Vec::new(),
            no_tests: false,
            diff: cx.new(|cx| DiffState::new([], cx)),
            tree: cx.new(|cx| TreeState::new(cx)),
            shown: None,
            tree_open: false,
            find,
            finding: false,
            hits: Vec::new(),
            hit: 0,
            reveal: None,
        }
    }
}

impl Desk {
    /// Read again what git says where the session `key` works: its commits
    /// and branches, and its pull request.
    pub(crate) fn refresh_git(&self, key: u64) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        if !s.repo {
            return;
        }
        self.transport.send(crate::proto::Call::Commits {
            dir: s.meta.folder.clone(),
            key,
        });
        // Without gh there is no answer, and no pull request shown.
        self.transport.send(crate::proto::Call::ReadPr {
            dir: s.meta.folder.clone(),
            key,
        });
    }

    /// Ask git about the folder the session `key` works in: not on another
    /// machine, whose git the desk does not read.
    pub(crate) fn look(&self, key: u64) {
        if let Some(s) = self.sessions.at(key)
            && crate::remote::parse(&s.meta.project).is_none()
        {
            self.transport.send(crate::proto::Call::Look {
                folder: s.meta.folder.clone(),
                key,
            });
        }
    }

    /// What git said of `folder`, where the session `key` works: kept on
    /// it, and in a repository what is not committed there is read.
    pub(crate) fn looked(
        &mut self,
        key: u64,
        folder: &std::path::Path,
        branch: Option<String>,
        repo: bool,
        scripts: Vec<crate::scripts::Script>,
    ) {
        let under = self.home.join("worktrees");
        let Some(s) = self
            .sessions
            .at_mut(key)
            .filter(|s| s.meta.folder == folder)
        else {
            return;
        };
        s.branch = branch;
        s.repo = repo;
        s.scripts = scripts;
        if s.state == State::Draft {
            return;
        }
        // A session in one of Ghost's worktrees works there again.
        if s.meta.worktree.is_none()
            && s.meta.folder.starts_with(&under)
            && let Some(branch) = s.branch.clone()
        {
            s.meta.worktree = Some(crate::git::Worktree {
                repo: s.meta.project.clone(),
                path: s.meta.folder.clone(),
                branch,
            });
            self.save_index();
        }
        if repo {
            self.refresh_diffs(key);
            if self.current == Some(key) {
                self.refresh_git(key);
            }
        }
    }

    /// Find where the diffs of the calls of the session `key` that have
    /// just ended start in their files; not on another machine, whose
    /// files are not here.
    pub(crate) fn place(&mut self, key: u64) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        let calls = std::mem::take(&mut s.placing);
        if !calls.is_empty() && crate::remote::parse(&s.meta.project).is_none() {
            self.transport
                .send(crate::proto::Call::Place { key, calls });
        }
    }

    /// Read again what the session `key` shows changed, as git says it now.
    pub(crate) fn refresh_diffs(&self, key: u64) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        if !s.repo {
            return;
        }
        self.transport.send(crate::proto::Call::Uncommitted {
            dir: s.meta.folder.clone(),
            key,
            no_spacing: self.settings.no_spacing,
        });
        if let Some((scope, _)) = &s.git.scoped {
            self.transport.send(crate::proto::Call::ScopeDiff {
                dir: s.meta.folder.clone(),
                key,
                scope: scope.clone(),
                base: s.git.compare_to.clone(),
                no_spacing: self.settings.no_spacing,
            });
        }
    }

    /// Every other minute, read again the pull requests of the sessions on
    /// branches of their own (their worktrees') and of the one on screen,
    /// as t3code watches its threads' pull requests and their checks.
    pub(crate) fn poll_prs(&mut self) {
        self.pr_tick += 1;
        if !self.pr_tick.is_multiple_of(2) {
            return;
        }
        for s in self
            .sessions
            .all
            .iter()
            .filter(|s| s.repo && (s.meta.worktree.is_some() || Some(s.key) == self.current))
        {
            self.transport.send(crate::proto::Call::ReadPr {
                dir: s.meta.folder.clone(),
                key: s.key,
            });
        }
    }

    /// Ask for the files of `folder` git sees, for the column's Files tab.
    pub(crate) fn list_files(&self, folder: PathBuf) {
        self.transport
            .send(crate::proto::Call::ListFiles { folder });
    }

    /// Ask for what the file at `path` holds, for its tab in the column.
    pub(crate) fn read_file(&self, path: PathBuf) {
        self.transport.send(crate::proto::Call::ReadFile { path });
    }

    /// Show the changes of `scope`; the branch's and a commit's are read
    /// from git first.
    pub(crate) fn ask_scope(&mut self, scope: &Scope) {
        let Some(key) = self.current else { return };
        if !matches!(scope, Scope::Branch | Scope::Commit(_)) {
            return;
        }
        let no_spacing = self.settings.no_spacing;
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.scoped = None;
            let (dir, base) = (s.meta.folder.clone(), s.git.compare_to.clone());
            self.transport.send(crate::proto::Call::ScopeDiff {
                dir,
                key,
                scope: scope.clone(),
                base,
                no_spacing,
            });
        }
    }

    /// Check out `branch` where the session on screen works: not while a
    /// session at work shares that checkout.
    pub fn switch_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        self.branch_action(crate::repo::GitAction::Switch(branch), cx);
    }

    /// Make the branch `name` where the session works, and check it out.
    pub fn new_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim().replace(' ', "-");
        if !name.is_empty() {
            self.branch_action(crate::repo::GitAction::NewBranch(name), cx);
        }
    }

    /// Check out, or make, a branch where the session on screen works:
    /// not while a session is at work in that checkout.
    fn branch_action(&mut self, action: crate::repo::GitAction, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        let Some(s) = self.session() else { return };
        let (key, dir) = (s.key, s.meta.folder.clone());
        let busy =
            self.sessions.all.iter().any(|o| {
                o.meta.folder == dir && matches!(o.state, State::Working | State::Opening)
            });
        if busy {
            self.trouble(
                "The branch is not switched",
                Some("A session is at work in this checkout.".into()),
                None,
            );
            cx.notify();
            return;
        }
        self.transport.send(crate::proto::Call::Git {
            dir,
            action,
            prefix: self.settings.branch_prefix.clone(),
            left_out: Vec::new(),
            key,
        });
        cx.notify();
    }

    /// Begin a session in the worktree the session `key` works in.
    pub fn new_in_worktree(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fresh) = self
            .sessions
            .at(key)
            .is_some_and(|s| s.meta.worktree.is_some())
            .then(|| self.beside(key))
            .flatten()
        else {
            return;
        };
        let new = fresh.key;
        self.sessions.all.push(fresh);
        self.show(new, window, cx);
    }

    /// Have the writer write the commit message for the session `key`;
    /// it goes in the commit form.
    pub fn ask_message(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        s.git.writing_message = true;
        let dir = s.meta.folder.clone();
        self.ask_writer(key, crate::writer::Ask::Commit { dir });
        cx.notify();
    }

    /// Have the writer write the pull request for the session `key`'s
    /// branch; once it has, what is left is committed with the message
    /// in the form, pushed, and the pull request opened.
    pub fn ask_pr(&mut self, key: u64, cx: &mut Context<Self>) {
        let message = self.commit_message(cx);
        self.review.committing = false;
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        s.git.step = Some("Writing the pull request".into());
        let dir = s.meta.folder.clone();
        self.ask_writer(key, crate::writer::Ask::Pr { dir, message });
        cx.notify();
    }

    /// Begin a comment on line `line` of `path`.
    pub fn begin_comment(
        &mut self,
        path: std::path::PathBuf,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review.commenting = Some((path, line));
        self.review
            .comment
            .update(cx, |i, cx| i.set_value("", window, cx));
        window.focus(
            &gpui::Focusable::focus_handle(self.review.comment.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// The comment written, kept with its line.
    pub(crate) fn add_comment(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some((path, line)) = self.review.commenting.take() {
            self.review
                .comments
                .retain(|c| !(c.0 == path && c.1 == line));
            self.review
                .comments
                .push((path, line, text.trim().to_owned()));
        }
        cx.notify();
    }

    pub fn drop_comment(&mut self, path: &std::path::Path, line: usize, cx: &mut Context<Self>) {
        self.review
            .comments
            .retain(|c| !(c.0 == path && c.1 == line));
        cx.notify();
    }

    /// The comments, put in the line as a message to the agent, to be sent
    /// as you see fit.
    pub fn send_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.session().map(|s| s.meta.folder.clone()) else {
            return;
        };
        let mut text = String::from("Comments on the changes:\n");
        for (path, line, said) in std::mem::take(&mut self.review.comments) {
            let path = path
                .strip_prefix(&folder)
                .unwrap_or(&path)
                .display()
                .to_string();
            let _ = writeln!(text, "- {path}:{line} — {said}");
        }
        self.put_line(text.trim_end(), window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }
}
