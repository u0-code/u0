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

    /// Look at what of each agent the registry lists is here, for the
    /// pages that show it.
    pub(crate) fn look_here(&self) {
        hub::here(
            self.registry.agents.clone(),
            self.home.clone(),
            self.tx.clone(),
        );
    }

    /// Ask how much memory each running agent holds, for Settings, Usage.
    pub(crate) fn weigh_agents(&self) {
        let running = (self.slots.iter())
            .filter_map(|s| Some((s.entry.id.clone(), s.running()?.pid()?)))
            .collect();
        hub::weigh(running, self.tx.clone());
    }

    /// Every five minutes, look again at the agents not running: one now
    /// installed (or its command now here) is started; one that stopped is
    /// tried again, updated first if it is behind. Every hour, read the
    /// registry again.
    pub(crate) fn check_agents(&mut self, cx: &mut Context<Self>) {
        self.health_tick += 1;
        if self.health_tick.is_multiple_of(5) {
            self.install_due(cx);
            self.look_here();
            let home = self.home.clone();
            for slot in &mut self.slots {
                if matches!(slot.stage, Stage::Missing(_) | Stage::Failed(_)) {
                    slot.stage = Stage::Ready;
                    slot.restage(&home);
                }
            }
            self.start_ready();
        }
        if self.health_tick.is_multiple_of(60) {
            hub::fetch(self.home.clone(), self.tx.clone());
        }
    }

    /// Install the registry's newest of each agent switched on that is not
    /// here, or that failed as an older version: what is here runs as it
    /// is, and is updated only when it does not work. One you start with a
    /// command of yours, or on another machine, is left as it is.
    pub(crate) fn install_due(&mut self, cx: &mut Context<Self>) {
        let due: Vec<String> = self
            .slots
            .iter()
            .filter(|s| crate::remote::split(&s.entry.id).1.is_none() && s.command.is_none())
            .filter(|s| {
                let e = &s.entry;
                match (&s.stage, e.installed(&self.home)) {
                    _ if e.way() == crate::registry::Way::None => false,
                    (Stage::Missing(crate::registry::Missing::Download), None) => true,
                    (Stage::Failed(_), Some(v)) => v != e.version,
                    _ => false,
                }
            })
            .filter(|slot| !self.agent_busy(&slot.entry.id))
            .map(|s| s.entry.id.clone())
            .collect();
        for id in due {
            tracing::info!("{id} is installed at the registry's newest");
            self.install(&id, cx);
        }
    }

    /// Whether something goes on in a session of `agent`: a turn, a sign-in,
    /// a question to you, a subagent at work or a task left running in the
    /// background (a dev server goes with the agent's process).
    pub(crate) fn agent_busy(&self, agent: &str) -> bool {
        use crate::session::State;
        self.sessions.all.iter().any(|s| {
            s.meta.agent == agent
                && (matches!(s.state, State::Working | State::Opening | State::SignIn)
                    || s.waiting().is_some()
                    || (s.state != State::Closed
                        && (s.sub.working() || s.tasks.iter().any(|t| t.ended.is_none()))))
        })
    }
}
