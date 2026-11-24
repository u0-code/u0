//! A turn's life around the agent: the checkpoints before and after it,
//! undoing it, the worktree a session works in, and committing.

use gpui::{Context, Focusable as _, Window};

use super::{Aside, Desk, Scope};
use crate::{
    hub::{self, Slot},
    session::{Item, State},
};

impl Desk {
    /// Whether the draft on screen works in a worktree of its own; the
    /// choice is remembered for the next.
    pub fn set_worktree(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(key) = self.current
            && let Some(s) = self.session_mut(key)
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
        self.scope = scope;
        self.aside = Aside::Session;
        self.panes.right = true;
        cx.notify();
    }

    /// Open or close the commit form.
    pub fn toggle_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.committing = !self.committing;
        if self.committing {
            let title = self.session().map(|s| s.title.clone()).unwrap_or_default();
            self.message.update(cx, |m, cx| m.set_text(&title, cx));
            window.focus(&self.message.focus_handle(cx));
        }
        cx.notify();
    }

    /// The commit message typed, or the session's title.
    pub fn commit_message(&self, cx: &gpui::App) -> String {
        let typed = self.message.read(cx).text();
        if typed.is_empty() {
            self.session().map(|s| s.title.clone()).unwrap_or_default()
        } else {
            typed
        }
    }

    /// Begin a turn in the session `key` with `text`: in a repository the
    /// files are checkpointed first, so the turn can be undone, and `text`
    /// goes once that is done.
    pub(super) fn begin_turn(&mut self, key: u64, text: String) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        if s.repo {
            let turn = s.turn_count().saturating_sub(1);
            let take = hub::Take {
                dir: s.folder.clone(),
                name: format!("{}/{turn}-before", s.uid()),
                key,
                turn,
                after: false,
                before: None,
                first: None,
                then: Some(text),
            };
            hub::checkpoint(take, self.tx.clone());
        } else {
            self.prompt(key, text);
        }
    }

    /// Say `text` to the agent of the session `key`.
    pub(super) fn prompt(&mut self, key: u64, text: String) {
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        let attached = self
            .session_mut(key)
            .map(|s| std::mem::take(&mut s.attached))
            .unwrap_or_default();
        match (self.slot(&agent).and_then(Slot::running).cloned(), id) {
            (Some(running), Some(id)) => {
                hub::prompt(running, key, id, text, attached, self.tx.clone());
            }
            _ => {
                if let Some(s) = self.session_mut(key) {
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
        let Some(s) = self.session_mut(key) else {
            return;
        };
        if !s.repo || s.turns.is_empty() {
            return;
        }
        let turn = s.turns.len() - 1;
        let take = hub::Take {
            dir: s.folder.clone(),
            name: format!("{}/{turn}-after", s.uid()),
            key,
            turn,
            after: true,
            before: s.turns[turn].before.clone(),
            first: s.first().map(str::to_owned),
            then: None,
        };
        hub::checkpoint(take, self.tx.clone());
    }

    pub(super) fn worktree_made(
        &mut self,
        key: u64,
        result: Result<crate::git::Worktree, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        match result {
            Ok(wt) => {
                s.folder.clone_from(&wt.path);
                s.branch = Some(wt.branch.clone());
                let path = wt.path.clone();
                s.worktree = Some(wt);
                let agent = s.agent.clone();
                self.open_when_running(&agent, key);
                // What the repository runs in each new worktree runs now.
                self.run_worktree_scripts(key, &path, cx);
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
        let Some(s) = self.session_mut(key) else {
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
            Err(why) => s.items.push(Item::Note {
                text: format!(
                    "The files could not be checkpointed, so this turn cannot be undone: {why}"
                ),
                trouble: true,
            }),
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
            s.folder.clone(),
            target,
            name,
            s.first().map(str::to_owned),
            s.key,
            turn,
            undo,
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
            .iter()
            .find(|s| s.key == key)
            .map(|s| self.name_of(&s.agent))
            .unwrap_or_default();
        let Some(s) = self.session_mut(key) else {
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
            Err(why) => s.items.push(Item::Note {
                text: format!("The files could not be put back: {why}"),
                trouble: true,
            }),
        }
    }

    /// Commit, push, open a pull request, or remove the worktree, in the
    /// session on screen.
    pub fn git_action(&mut self, action: hub::GitAction, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (dir, key) = (s.folder.clone(), s.key);
        if let Some(s) = self.session_mut(key) {
            s.git_note = Some(crate::session::GitNote {
                text: "Working…".into(),
                trouble: false,
                url: None,
            });
        }
        hub::git_action(dir, action, key, self.tx.clone());
        cx.notify();
    }

    pub(super) fn gitted(
        &mut self,
        key: u64,
        action: &hub::GitAction,
        result: Result<String, String>,
    ) {
        let tx = self.tx.clone();
        let Some(s) = self.session_mut(key) else {
            return;
        };
        s.git_step = None;
        // A branch renamed, or another checked out, is the session's now.
        match (action, &result) {
            (hub::GitAction::RenameBranch(name), Ok(_)) => {
                s.branch = Some(name.clone());
                if let Some(wt) = &mut s.worktree {
                    wt.branch.clone_from(name);
                }
                self.save_index();
                return;
            }
            (hub::GitAction::RenameBranch(_), Err(_)) => return,
            (hub::GitAction::Switch(_), Ok(_)) => s.look(),
            _ => {}
        }
        let s = self.session_mut(key).expect("found above");
        let removed = matches!(action, hub::GitAction::RemoveWorktree(_)) && result.is_ok();
        // With no turn of its own taken here, what it shows is what is
        // not committed: read again after a commit.
        if s.turns.is_empty() && s.repo && !removed {
            hub::uncommitted(s.folder.clone(), key, tx);
        }
        s.git_note = Some(match result {
            Ok(text) => {
                let url = text.starts_with("https://").then(|| text.clone());
                crate::session::GitNote {
                    text: if url.is_some() {
                        "Pull request opened".into()
                    } else {
                        text
                    },
                    trouble: false,
                    url,
                }
            }
            Err(why) => crate::session::GitNote {
                text: why,
                trouble: true,
                url: None,
            },
        });
        if removed {
            s.worktree = None;
            s.id = None;
            s.state =
                State::Broken("Its worktree was removed. Start a new session to go on.".into());
        }
        self.refresh_git(key);
    }
}
