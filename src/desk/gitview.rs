//! What git says where a session works, read for the column beside it as
//! t3code reads it: the branch against where it left the main one, one
//! commit, the recent commits, the local branches (one can be checked out,
//! never under another session at work), the branch's pull request and its
//! checks. And a commit message the agent writes, when asked to.

use std::{collections::HashSet, fmt::Write as _, path::PathBuf};

use gpui::{AppContext as _, Context, Entity, Window};

use super::Desk;
use crate::{
    git::Scope,
    hub,
    input::{Submitted, TextInput},
    session::{Item, State},
};

/// What the agent is asked, to write a commit message.
const WRITE_MESSAGE: &str = "Write a commit message for the changes not committed yet: a short subject line, then a few lines on what changed and why. Reply with the message only, no code block.";

/// The changes in the column beside: which it shows; the commit message
/// being written and whether its form is open; what you say of a file's
/// lines (the line being commented on, the line to write it in, and those
/// said, waiting to go to the agent); the folders folded in the list of
/// files, and whether test files are left out of it.
pub struct Review {
    pub scope: Scope,
    pub message: Entity<TextInput>,
    pub committing: bool,
    pub commenting: Option<(PathBuf, usize)>,
    pub comment: Entity<TextInput>,
    pub comments: Vec<(PathBuf, usize, String)>,
    pub folded: HashSet<String>,
    pub no_tests: bool,
}

impl Review {
    pub(super) fn new(cx: &mut Context<Desk>) -> Self {
        let comment = cx.new(|cx| TextInput::new(cx, "What of this line?").padded(3.));
        cx.subscribe(&comment, |this, _, Submitted(text), cx| {
            this.add_comment(text, cx);
        })
        .detach();
        Self {
            scope: Scope::All,
            message: cx.new(|cx| TextInput::new(cx, "Commit message").padded(4.).keeping()),
            committing: false,
            commenting: None,
            comment,
            comments: Vec::new(),
            folded: HashSet::new(),
            no_tests: false,
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
        hub::commits(s.meta.folder.clone(), key, self.tx.clone());
        // Without gh there is no answer, and no pull request shown.
        hub::pr(s.meta.folder.clone(), key, self.tx.clone());
    }

    /// Ask git about the folder the session `key` works in: not on another
    /// machine, whose git the desk does not read.
    pub(crate) fn look(&self, key: u64) {
        if let Some(s) = self.sessions.at(key)
            && crate::remote::parse(&s.meta.project).is_none()
        {
            hub::look(s.meta.folder.clone(), key, self.tx.clone());
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
        scripts: Vec<super::runs::Script>,
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
        let tx = self.tx.clone();
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        let calls = std::mem::take(&mut s.placing);
        if !calls.is_empty() && crate::remote::parse(&s.meta.project).is_none() {
            hub::place(key, calls, tx);
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
        hub::uncommitted(s.meta.folder.clone(), key, self.tx.clone());
        if let Some((scope, _)) = &s.git.scoped {
            hub::scope_diff(
                s.meta.folder.clone(),
                key,
                scope.clone(),
                s.git.compare_to.clone(),
                self.tx.clone(),
            );
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
            hub::pr(s.meta.folder.clone(), s.key, self.tx.clone());
        }
    }

    /// Show the changes of `scope`; the branch's and a commit's are read
    /// from git first.
    pub(crate) fn ask_scope(&mut self, scope: &Scope) {
        let Some(key) = self.current else { return };
        if !matches!(scope, Scope::Branch | Scope::Commit(_)) {
            return;
        }
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.scoped = None;
            let (dir, base) = (s.meta.folder.clone(), s.git.compare_to.clone());
            hub::scope_diff(dir, key, scope.clone(), base, self.tx.clone());
        }
    }

    /// Check out `branch` where the session on screen works: not while a
    /// session at work shares that checkout.
    pub fn switch_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        self.branch_action(hub::GitAction::Switch(branch), cx);
    }

    /// Make the branch `name` where the session works, and check it out.
    pub fn new_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim().replace(' ', "-");
        if !name.is_empty() {
            self.branch_action(hub::GitAction::NewBranch(name), cx);
        }
    }

    /// Check out, or make, a branch where the session on screen works:
    /// not while a session is at work in that checkout.
    fn branch_action(&mut self, action: hub::GitAction, cx: &mut Context<Self>) {
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
        hub::git_action(
            dir,
            action,
            self.settings.branch_prefix.clone(),
            Vec::new(),
            key,
            self.tx.clone(),
        );
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

    /// Ask the agent of the session `key` to write the commit message; its
    /// answer goes in the commit form. It is a turn of its own, and spends
    /// what a turn spends.
    pub fn ask_message(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.writing_message = true;
        }
        self.send_in(key, WRITE_MESSAGE, cx);
    }

    /// A turn ended in the session `key`: a commit message it was asked for
    /// goes in the commit form.
    pub(crate) fn take_message(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if !s.git.writing_message {
            return;
        }
        s.git.writing_message = false;
        let said = s.items.iter().rev().find_map(|i| match i {
            Item::Agent(t) => Some(t.trim().to_owned()),
            _ => None,
        });
        if let Some(text) = said {
            self.review
                .message
                .update(cx, |m, cx| m.set_text(&text, cx));
            self.review.committing = true;
        }
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
        self.review.comment.update(cx, |i, cx| i.set_text("", cx));
        window.focus(&gpui::Focusable::focus_handle(
            self.review.comment.read(cx),
            cx,
        ));
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
        self.composer
            .input
            .update(cx, |i, cx| i.set_text(text.trim_end(), cx));
        window.focus(&gpui::Focusable::focus_handle(
            self.composer.input.read(cx),
            cx,
        ));
        cx.notify();
    }

    /// A worktree's branch is named for what was first said; once the
    /// session has a title of its own (the agent's, or yours), the branch
    /// takes it, unless it is pushed already.
    pub(crate) fn name_branch(&self, key: u64) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let Some(wt) = &s.meta.worktree else { return };
        let prefix = &self.settings.branch_prefix;
        let name = format!("{prefix}{}", crate::git::slug(&s.meta.title));
        if s.turns.len() <= 1 && wt.branch.starts_with(prefix.as_str()) && wt.branch != name {
            hub::git_action(
                wt.path.clone(),
                hub::GitAction::RenameBranch(name),
                prefix.clone(),
                Vec::new(),
                key,
                self.tx.clone(),
            );
        }
    }
}
