//! A turn's life around the agent: the checkpoints before and after it,
//! undoing it, the worktree a session works in, and committing.

use agent_client_protocol::v1::schema::{ContentBlock, TextContent};
use gpui::{Context, Focusable as _, Window};

use super::{Aside, Desk};
use crate::git::Scope;
use crate::{
    hub::{self, Slot},
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
        self.aside = Aside::Session;
        self.panes.right = true;
        cx.notify();
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
                .update(cx, |m, cx| m.set_text(&title, cx));
            window.focus(&self.review.message.focus_handle(cx));
        }
        cx.notify();
    }

    /// The commit message typed, or the session's title.
    pub fn commit_message(&self, cx: &gpui::App) -> String {
        let typed = self.review.message.read(cx).text();
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
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if s.repo {
            let turn = s.turn_count().saturating_sub(1);
            hub::checkpoint(
                s.meta.folder.clone(),
                format!("{}/{turn}-before", s.uid()),
                key,
                turn,
                false,
                None,
                None,
                Some(text),
                self.settings.no_spacing,
                self.tx.clone(),
            );
        } else {
            self.prompt(key, text);
        }
    }

    /// Say `text` to the agent of the session `key`.
    pub(super) fn prompt(&mut self, key: u64, text: String) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let (agent, id) = (s.meta.agent.clone(), s.meta.id.clone());
        let attached = self
            .sessions
            .at_mut(key)
            .map(|s| std::mem::take(&mut s.attached))
            .unwrap_or_default();
        match (self.slot(&agent).is_some_and(Slot::running), id) {
            (true, Some(id)) => {
                let mut content = vec![ContentBlock::Text(TextContent::new(text))];
                content.extend(attached);
                self.transport.send(Call::Prompt {
                    agent,
                    key,
                    session: id,
                    content,
                });
            }
            _ => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.ended(Err(
                        "The agent is not running; start a new session with it.".into(),
                    ));
                }
            }
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
        hub::checkpoint(
            s.meta.folder.clone(),
            format!("{}/{turn}-after", s.uid()),
            key,
            turn,
            true,
            s.turns[turn].before.clone(),
            s.first().map(str::to_owned),
            None,
            self.settings.no_spacing,
            self.tx.clone(),
        );
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
                s.meta.folder.clone_from(&wt.path);
                s.branch = Some(wt.branch.clone());
                s.scripts = scripts;
                let path = wt.path.clone();
                s.meta.worktree = Some(wt);
                let agent = s.meta.agent.clone();
                // What the repository runs in each new worktree runs now;
                // what it says to wait for, first.
                if !self.run_worktree_scripts(key, &path, cx) {
                    self.open_when_running(&agent, key);
                }
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
        result: Result<hub::Snapshot, String>,
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
        hub::restore(
            s.meta.folder.clone(),
            target,
            name,
            s.first().map(str::to_owned),
            s.key,
            turn,
            undo,
            self.settings.no_spacing,
            self.tx.clone(),
        );
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
    pub fn git_action(&mut self, action: hub::GitAction, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (dir, key) = (s.meta.folder.clone(), s.key);
        let left_out: Vec<String> = s.git.left_out.iter().cloned().collect();
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.step = Some("Working".into());
            s.git.left_out.clear();
        }
        hub::git_action(
            dir,
            action,
            self.settings.branch_prefix.clone(),
            left_out,
            key,
            self.tx.clone(),
        );
        cx.notify();
    }

    pub(super) fn gitted(
        &mut self,
        key: u64,
        action: &hub::GitAction,
        result: Result<String, String>,
    ) {
        let (tx, no_spacing) = (self.tx.clone(), self.settings.no_spacing);
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        s.git.step = None;
        // A branch renamed, or another checked out, is the session's now.
        match (action, &result) {
            (hub::GitAction::RenameBranch(name), Ok(_)) => {
                s.branch = Some(name.clone());
                if let Some(wt) = &mut s.meta.worktree {
                    wt.branch.clone_from(name);
                }
                self.save_index();
                return;
            }
            (hub::GitAction::RenameBranch(_), Err(_)) => return,
            _ => {}
        }
        let looks = matches!(
            (action, &result),
            (
                hub::GitAction::Switch(_) | hub::GitAction::Init | hub::GitAction::NewBranch(_),
                Ok(_),
            )
        );
        let removed = matches!(action, hub::GitAction::RemoveWorktree(_)) && result.is_ok();
        // With no turn of its own taken here, what it shows is what is
        // not committed: read again after a commit, and after another
        // branch is checked out once git has said which.
        if looks {
            hub::look(s.meta.folder.clone(), key, tx);
        } else if s.turns.is_empty() && s.repo && !removed {
            hub::uncommitted(s.meta.folder.clone(), key, no_spacing, tx);
        }
        // Its worktree removed, it is not kept from now on.
        let unkept =
            (removed.then(|| s.meta.id.take()).flatten()).map(|id| (s.meta.agent.clone(), id));
        if removed {
            s.meta.worktree = None;
            s.state =
                State::Broken("Its worktree was removed. Start a new session to go on.".into());
        }
        // A repository made here: its project is read again.
        if matches!(action, hub::GitAction::Init) {
            let project = s.meta.project.clone();
            self.held.remove(&project);
        }
        if let Some((agent, id)) = unkept {
            self.unkeep(agent, id, false);
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
