//! What Ghost has an agent write for you (`writer`): asked of the writer
//! you chose in the settings, or else of the session's own agent, and put
//! where it was asked for when it comes back.

use gpui::{Context, Window};

use super::{Desk, lists::Waiting};
use crate::{
    proto::Call,
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
        let model = (self.settings.writer.agent.as_ref() == Some(&agent))
            .then(|| self.settings.writer.model.clone())
            .flatten();
        let yours = match &ask {
            Ask::Commit { .. } => self.settings.writer.changes.clone(),
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

    /// What the writer wrote for the session `key`, put where it was asked
    /// for; or why it did not write it.
    pub(super) fn written(
        &mut self,
        key: u64,
        ask: &Ask,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match result {
            Ok(text) => text,
            Err(why) => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.git.writing_message = false;
                }
                self.trouble("The writer couldn't write it", Some(why), None);
                return;
            }
        };
        match ask {
            Ask::Commit { .. } => {
                let Some(s) = self.sessions.at_mut(key) else {
                    return;
                };
                s.git.writing_message = false;
                self.review
                    .message
                    .update(cx, |m, cx| m.set_value(&text, window, cx));
                self.review.committing = true;
            }
        }
    }
}
