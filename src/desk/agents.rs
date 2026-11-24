//! What you set for an agent from its page: a command line of yours that
//! starts it, model ids of yours; and its health checked every few
//! minutes, and the registry read again every few hours.

use gpui::Context;

use super::Desk;
use crate::hub::{self, Stage};

impl Desk {
    /// Start the agent `id` with the command typed on its page, or, with
    /// none, the registry's way; it starts again with it at once.
    pub fn save_command(&mut self, id: &str, cx: &mut Context<Self>) {
        let line = self.agent_command.read(cx).text().trim().to_owned();
        if line.is_empty() {
            self.settings.commands.remove(id);
        } else {
            self.settings.commands.insert(id.to_owned(), line.clone());
        }
        self.save_settings();
        // What runs is let go; it starts again the new way.
        self.let_go(id);
        let home = self.home.clone();
        if let Some(slot) = self.slot_mut(id) {
            slot.command = (!line.is_empty()).then_some(line);
            slot.restage(&home);
        }
        self.start_ready();
        cx.notify();
    }

    /// A model id of yours for the agent `id`, typed on its page.
    pub fn add_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let model = self.agent_model.read(cx).text().trim().to_owned();
        if model.is_empty() {
            return;
        }
        let list = self.settings.models.entry(id.to_owned()).or_default();
        if !list.contains(&model) {
            list.push(model);
        }
        self.agent_model.update(cx, |i, cx| i.set_text("", cx));
        self.save_settings();
        cx.notify();
    }

    pub fn drop_model(&mut self, id: &str, model: &str, cx: &mut Context<Self>) {
        if let Some(list) = self.settings.models.get_mut(id) {
            list.retain(|m| m != model);
            if list.is_empty() {
                self.settings.models.remove(id);
            }
        }
        self.save_settings();
        cx.notify();
    }

    /// Every five minutes, look again at the agents not running: one now
    /// installed (or its command now here) is started; one that stopped is
    /// tried again. Every six hours, read the registry again.
    pub(crate) fn check_agents(&mut self) {
        self.health_tick += 1;
        if self.health_tick.is_multiple_of(5) {
            let home = self.home.clone();
            for slot in &mut self.slots {
                if matches!(slot.stage, Stage::Missing(_) | Stage::Failed(_)) {
                    slot.stage = Stage::Ready;
                    slot.restage(&home);
                }
            }
            self.start_ready();
        }
        if self.health_tick.is_multiple_of(360) {
            hub::fetch(self.home.clone(), self.tx.clone());
        }
    }
}
