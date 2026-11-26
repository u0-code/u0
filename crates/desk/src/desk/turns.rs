//! A turn's life around the agent: the checkpoints before and after it,
//! undoing it, the worktree a session works in, and committing.

use agent_client_protocol::v1::schema::{ContentBlock, TextContent};
use gpui::{Context, Focusable as _, Window};

use super::{Desk, Surface};
use crate::git::Scope;
use crate::{
    hub::Slot,
    proto::Call,
    session::{Item, State},
};

impl Desk {
    /// Whether the draft on screen works in a worktree of its own; the
    /// choice is remembered for the next.
    pub fn set_worktree(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(key) = self.current
            && let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
        {
            s.wants_worktree = on && s.repo;
        }
        self.settings.worktree = on;
        self.save_settings();
        cx.notify();
    }

    /// Show `scope`'s changes in the right column, and its first file.
    pub fn show_changes(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.ask_scope(&scope);
        self.review.scope = scope;
        self.open_surface(Surface::Diff, cx);
    }

    /// Open or close the commit form.
    pub fn toggle_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review.committing = !self.review.committing;
        if self.review.committing {
            let title = self
                .session()
                .map(|s| s.meta.title.clone())
                .unwrap_or_default();
            self.review
                .message
                .update(cx, |m, cx| m.set_value(&title, window, cx));
            window.focus(&self.review.message.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// The commit message typed, or the session's title.
    pub fn commit_message(&self, cx: &gpui::App) -> String {
        let typed = self.review.message.read(cx).value().trim().to_owned();
        if typed.is_empty() {
            self.session()
                .map(|s| s.meta.title.clone())
                .unwrap_or_default()
        } else {
            typed
        }
    }

    /// Begin a turn in the session `key` with `text`: in a repository the
    /// files are checkpointed first, so the turn can be undone, and `text`
    /// goes once that is done.
    pub(super) fn begin_turn(&mut self, key: u64, text: String) {
        // What it thought you would say next is said, or not, now.
        self.composer.suggested.take_if(|(k, _)| *k == key);
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if s.repo {
            let turn = s.turn_count().saturating_sub(1);
            let call = Call::Checkpoint {
                dir: s.meta.folder.clone(),
                name: format!("{}/{turn}-before", s.uid()),
                key,
                turn,
                after: false,
                before: None,
                first: None,
                then: Some(text),
                no_spacing: self.settings.no_spacing,
            };
            let host = s.host;
            self.call(host, call);
        } else {
            self.prompt(key, text);
        }
    }

    /// Say `text` to the agent of the session `key`.
    pub(super) fn prompt(&mut self, key: u64, text: String) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let (host, agent, id) = (s.host, s.meta.agent.clone(), s.meta.id.clone());
        let attached = self
            .sessions
            .at_mut(key)
            .map(|s| std::mem::take(&mut s.attached))
            .unwrap_or_default();
        if let (true, Some(id)) = (self.slot(host, &agent).is_some_and(Slot::running), id) {
            let mut content = vec![ContentBlock::Text(TextContent::new(text))];
            content.extend(attached);
            let call = Call::Prompt {
                agent,
                key,
                session: id,
                content,
            };
            self.call(host, call);
        } else {
            if let Some(s) = self.sessions.at_mut(key) {
                s.ended(
                    Err("The agent is not running; start a new session with it.".into()),
                    Vec::new(),
                );
            }
            self.flush(key);
        }
    }

    /// A turn ended: in a repository the files are checkpointed again, and
    /// what the turn changed is read.
    pub(super) fn end_turn(&mut self, key: u64) {
        self.flush(key);
        self.save_index();
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if !s.repo || s.turns.is_empty() {
            return;
        }
        let turn = s.turns.len() - 1;
        let call = Call::Checkpoint {
            dir: s.meta.folder.clone(),
            name: format!("{}/{turn}-after", s.uid()),
            key,
            turn,
            after: true,
            before: s.turns[turn].before.clone(),
            first: s.first().map(str::to_owned),
            then: None,
            no_spacing: self.settings.no_spacing,
        };
        let host = s.host;
        self.call(host, call);
    }

    pub(super) fn worktree_made(
        &mut self,
        key: u64,
        result: Result<(crate::git::Worktree, Vec<crate::scripts::Script>), String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        match result {
            Ok((wt, scripts)) => {
                // The branch is named by the writer for what was asked.
                let said = s.pending.clone().unwrap_or_default();
                s.meta.folder.clone_from(&wt.path);
                s.branch = Some(wt.branch.clone());
                s.scripts = scripts;
                let path = wt.path.clone();
                s.meta.worktree = Some(wt);
                let (host, agent, project) = (s.host, s.meta.agent.clone(), s.meta.project.clone());
                let semantic = self.settings_on(host).of(&project).branch_kind;
                // What the repository runs in each new worktree runs now;
                // what it says to wait for, first.
                if !self.run_worktree_scripts(key, &path, cx) {
                    self.open_when_running(host, &agent, key);
                }
                self.ask_writer(key, crate::writer::Ask::Branch { said, semantic });
            }
            Err(why) => {
                s.pending = None;
                s.since = None;
                s.state = State::Broken(format!("A worktree could not be made: {why}"));
            }
        }
    }

    pub(super) fn checkpointed(
        &mut self,
        key: u64,
        turn: usize,
        after: bool,
        result: Result<crate::repo::Snapshot, String>,
        then: Option<String>,
    ) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        while s.turns.len() <= turn {
            s.turns.push(crate::session::Turn::default());
        }
        match result {
            Ok(snap) => {
                let t = &mut s.turns[turn];
                if after {
                    t.after = Some(snap.commit);
                    t.diff = snap.turn;
                    if snap.total.is_some() {
                        s.total = snap.total;
                    }
                } else {
                    t.before = Some(snap.commit);
                }
            }
            Err(why) => self.trouble(
                "This turn can't be undone",
                Some(format!("Its files could not be checkpointed: {why}")),
                None,
            ),
        }
        if let Some(text) = then {
            self.prompt(key, text);
        }
    }

    /// Put the files back as they were before turn `turn`, or, after that,
    /// as they were again.
    pub fn undo(&mut self, turn: usize, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let target = match &s.redo {
            Some((t, back)) if *t == turn => Some((back.clone(), false)),
            _ => s
                .turns
                .get(turn)
                .and_then(|t| t.before.clone())
                .map(|b| (b, true)),
        };
        let Some((target, undo)) = target else { return };
        let name = format!("{}/{turn}-{}", s.uid(), if undo { "undo" } else { "redo" });
        let call = Call::Restore {
            dir: s.meta.folder.clone(),
            target,
            name,
            first: s.first().map(str::to_owned),
            key: s.key,
            turn,
            undo,
            no_spacing: self.settings.no_spacing,
        };
        self.call(s.host, call);
        cx.notify();
    }

    pub(super) fn restored(
        &mut self,
        key: u64,
        turn: usize,
        undo: bool,
        result: Result<(String, Option<Vec<crate::git::FileDiff>>), String>,
    ) {
        let name = self
            .sessions
            .at(key)
            .map(|s| self.name_of(&s.meta.agent))
            .unwrap_or_default();
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        match result {
            Ok((back, total)) => {
                s.total = total;
                if undo {
                    s.redo = Some((turn, back));
                    s.items.push(Item::Note {
                        text: format!(
                            "The files are back as they were before turn {}. {name} still remembers that turn: tell it what changed.",
                            turn + 1
                        ),
                        trouble: false,
                    });
                } else {
                    s.redo = None;
                    s.items.push(Item::Note {
                        text: format!("Turn {} is back in the files.", turn + 1),
                        trouble: false,
                    });
                }
            }
            Err(why) => self.trouble("Couldn't put the files back", Some(why), None),
        }
    }

    /// Commit, push, open a pull request, or remove the worktree, in the
    /// session on screen.
    pub fn git_action(&mut self, action: crate::repo::GitAction, cx: &mut Context<Self>) {
        if let Some(key) = self.current {
            self.git_in(key, action, cx);
        }
    }

    /// Do `action` where the session `key` works.
    pub(crate) fn git_in(
        &mut self,
        key: u64,
        action: crate::repo::GitAction,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let (host, dir) = (s.host, s.meta.folder.clone());
        let left_out: Vec<String> = s.git.left_out.iter().cloned().collect();
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.step = Some("Working".into());
            s.git.left_out.clear();
        }
        let call = Call::Git {
            dir,
            action,
            prefix: self.settings_on(host).branch_prefix.clone(),
            left_out,
            key,
        };
        self.call(host, call);
        cx.notify();
    }

    pub(super) fn gitted(
        &mut self,
        key: u64,
        action: &crate::repo::GitAction,
        result: Result<String, String>,
    ) {
        let no_spacing = self.settings.no_spacing;
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        s.git.step = None;
        // A branch renamed, or another checked out, is the session's now.
        match (action, &result) {
            (crate::repo::GitAction::RenameBranch(name), Ok(_)) => {
                s.branch = Some(name.clone());
                if let Some(wt) = &mut s.meta.worktree {
                    wt.branch.clone_from(name);
                }
                self.save_index();
                return;
            }
            (crate::repo::GitAction::RenameBranch(_), Err(_)) => return,
            _ => {}
        }
        let looks = matches!(
            (action, &result),
            (
                crate::repo::GitAction::Switch(_)
                    | crate::repo::GitAction::Init
                    | crate::repo::GitAction::NewBranch(_),
                Ok(_),
            )
        );
        let removed = matches!(action, crate::repo::GitAction::RemoveWorktree(_)) && result.is_ok();
        // With no turn of its own taken here, what it shows is what is
        // not committed: read again after a commit, and after another
        // branch is checked out once git has said which.
        let host = s.host;
        if looks {
            let folder = s.meta.folder.clone();
            self.call(host, Call::Look { folder, key });
        } else if s.turns.is_empty() && s.repo && !removed {
            let dir = s.meta.folder.clone();
            self.call(
                host,
                Call::Uncommitted {
                    dir,
                    key,
                    since: None,
                    no_spacing,
                },
            );
        }
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        // Its worktree removed, it is not kept from now on.
        let unkept =
            (removed.then(|| s.meta.id.take()).flatten()).map(|id| (s.meta.agent.clone(), id));
        if removed {
            s.meta.worktree = None;
            s.state =
                State::Broken("Its worktree was removed. Start a new session to go on.".into());
        }
        // A repository made here: its project is read again.
        if matches!(action, crate::repo::GitAction::Init) {
            let project = super::Place::project_of(s);
            self.held.remove(&project);
        }
        if let Some((agent, id)) = unkept {
            self.unkeep(host, agent, id, false);
        }
        match result {
            Ok(text) if text.starts_with("https://") => {
                self.done("Pull request opened", Some(super::notices::Way::Open(text)));
            }
            Ok(text) => self.done(text, None),
            Err(why) => self.trouble(format!("Couldn't {}", action.doing()), Some(why), None),
        }
        self.refresh_git(key);
    }
}
