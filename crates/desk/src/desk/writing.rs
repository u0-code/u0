//! What Ghost has an agent write for you (`writer`): asked of the writer
//! you chose in the settings, or else of the session's own agent, and put
//! where it was asked for when it comes back: a session's title, its
//! worktree's branch, a commit message, a pull request, what you might
//! say next.

use gpui::{Context, Window};

use super::{Desk, lists::Waiting};
use crate::{
    proto::Call,
    repo::GitAction,
    session::Item,
    writer::{Ask, Order},
};

impl Desk {
    /// Have the writer write what `ask` asks for the session `key`: now,
    /// or once its agent runs.
    pub(crate) fn ask_writer(&mut self, key: u64, ask: Ask) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let agent = self.writer_of(s);
        let w = &self.settings.writer;
        let model = (w.agent.as_ref() == Some(&agent))
            .then(|| w.model.clone())
            .flatten();
        let yours = match &ask {
            Ask::Title { .. } => w.titles.clone(),
            Ask::Branch { .. } => w.branches.clone(),
            Ask::Commit { .. } | Ask::Pr { .. } => w.changes.clone(),
            Ask::Next { .. } => String::new(),
            Ask::Transcript { .. } => w.transcripts.clone(),
        };
        let Some((dir, delete, close)) = self.aside_place(&agent) else {
            self.waiting.push((agent.clone(), Waiting::Write(key, ask)));
            self.start_for(&agent);
            return;
        };
        self.transport.send(Call::Write(Order {
            agent,
            model,
            key,
            ask,
            yours,
            dir,
            delete,
            close,
        }));
    }

    /// The agent that writes for `s`: the writer you chose, while it is
    /// switched on, or else its own.
    pub(crate) fn writer_of(&self, s: &crate::session::Session) -> String {
        (self.settings.writer.agent.clone())
            .filter(|a| self.slot(a).is_some())
            .unwrap_or_else(|| s.meta.agent.clone())
    }

    /// Have the writer title the session `key` again, from what you said
    /// in it; from its title, while what was said is not read yet.
    pub fn retitle(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let said: Vec<&str> = (s.items.iter())
            .filter_map(|i| match i {
                Item::You(t) => Some(t.as_str()),
                _ => None,
            })
            .take(3)
            .collect();
        let said = if said.is_empty() {
            s.meta.title.clone()
        } else {
            said.join("\n\n")
        };
        self.ask_writer(key, Ask::Title { said });
        cx.notify();
    }

    /// After a turn of the session `key` that ended well, what you might
    /// say next, when you asked for it.
    pub(crate) fn suggest_next(&mut self, key: u64) {
        let Some(s) = self.sessions.at(key).filter(|s| s.parent.is_none()) else {
            return;
        };
        if !self.settings.writer.suggest {
            return;
        }
        let last = |you: bool| {
            s.items.iter().rev().find_map(|i| match i {
                Item::You(t) if you => Some(t.clone()),
                Item::Agent(t) if !you => Some(t.clone()),
                _ => None,
            })
        };
        if let (Some(said), Some(answer)) = (last(true), last(false)) {
            self.ask_writer(key, Ask::Next { said, answer });
        }
    }

    /// What the writer wrote for the session `key`, put where it was asked
    /// for; or why it did not write it.
    pub(super) fn written(
        &mut self,
        key: u64,
        ask: Ask,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.sessions.at_mut(key) {
            s.git.writing_message &= !matches!(ask, Ask::Commit { .. });
            if matches!(ask, Ask::Pr { .. }) {
                s.git.step = None;
            }
        }
        let text = match result {
            // A suggestion that did not come is no news.
            Err(_) if matches!(ask, Ask::Next { .. }) => return,
            Err(why) => {
                // What you dictated goes in as it was heard all the same.
                if let Ask::Transcript { heard } = &ask {
                    self.cleaned(heard, heard, window, cx);
                }
                let what = match ask {
                    Ask::Title { .. } => "a title",
                    Ask::Branch { .. } => "the branch's name",
                    Ask::Commit { .. } => "the commit message",
                    Ask::Pr { .. } => "the pull request",
                    Ask::Next { .. } => "what to say next",
                    Ask::Transcript { .. } => "what you dictated, cleaned up",
                };
                self.trouble(format!("The writer couldn't write {what}"), Some(why), None);
                return;
            }
            Ok(text) => text,
        };
        match ask {
            Ask::Title { .. } => {
                let Some(s) = self.sessions.at_mut(key) else {
                    return;
                };
                if text.is_empty() || s.meta.title == text {
                    self.done("The title already fits", None);
                    return;
                }
                s.meta.title = text;
                s.meta.renamed = true;
                self.save_index();
            }
            Ask::Branch { semantic, .. } => {
                let Some(wt) = self.sessions.at(key).and_then(|s| s.meta.worktree.as_ref()) else {
                    return;
                };
                let prefix = &self.settings.branch_prefix;
                let name = if semantic {
                    text
                } else {
                    format!("{prefix}{text}")
                };
                if wt.branch != name {
                    self.transport.send(Call::Git {
                        dir: wt.path.clone(),
                        action: GitAction::RenameBranch(name),
                        prefix: prefix.clone(),
                        left_out: Vec::new(),
                        key,
                    });
                }
            }
            Ask::Commit { .. } => {
                self.review
                    .message
                    .update(cx, |m, cx| m.set_value(&text, window, cx));
                self.review.committing = true;
            }
            Ask::Pr { message, .. } => {
                let (title, body) = text.split_once('\n').unwrap_or((&text, ""));
                let action = GitAction::PullRequest {
                    message,
                    title: title.trim().trim_start_matches("# ").to_owned(),
                    body: body.trim().to_owned(),
                };
                self.git_in(key, action, cx);
            }
            Ask::Next { .. } => {
                if !text.is_empty() {
                    self.composer.suggested = Some((key, text));
                }
            }
            Ask::Transcript { heard } => self.cleaned(&heard, &text, window, cx),
        }
        cx.notify();
    }
}
