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

    /// How much memory each running agent holds (ps), for Settings, Usage.
    pub(crate) fn weigh_agents(&mut self) {
        self.weights = self
            .slots
            .iter()
            .filter_map(|s| {
                let pid = s.running()?.pid()?;
                let out = std::process::Command::new("ps")
                    .args(["-o", "rss=", "-g", &pid.to_string()])
                    .output()
                    .ok()?;
                let kb: u64 = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<u64>().ok())
                    .sum();
                Some((s.entry.id.clone(), pid, kb))
            })
            .collect();
    }

    /// Every five minutes, look again at the agents not running: one now
    /// installed (or its command now here) is started; one that stopped is
    /// tried again. Every hour, read the registry again, and keep each
    /// agent current with it.
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
        if self.health_tick.is_multiple_of(60) {
            hub::fetch(self.home.clone(), self.tx.clone());
        }
    }

    /// Install each agent switched on that is not installed yet, or not at
    /// the registry's version, and (a package) one installed a day ago or
    /// more, so that what it depends on, its own CLI, is the newest it
    /// allows: as the registry is read, while nothing goes on in its
    /// sessions. One running is let go first and starts again installed;
    /// its sessions resume when written in.
    pub(crate) fn keep_current(&mut self, cx: &mut Context<Self>) {
        const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
        let due: Vec<String> = self
            .slots
            .iter()
            .filter(|s| crate::remote::split(&s.entry.id).1.is_none() && s.command.is_none())
            .filter(|s| !matches!(s.stage, Stage::Installing(_) | Stage::Starting))
            .filter(|s| {
                let e = &s.entry;
                let installed = e.installed(&self.home);
                match e.way() {
                    crate::registry::Way::None => false,
                    _ if installed.as_deref() != Some(e.version.as_str()) => true,
                    crate::registry::Way::Archive => false,
                    crate::registry::Way::Runner { .. } => e
                        .installed_at(&self.home)
                        .and_then(|t| t.elapsed().ok())
                        .is_none_or(|age| age >= DAY),
                }
            })
            .filter(|slot| !self.agent_busy(&slot.entry.id))
            .map(|s| s.entry.id.clone())
            .collect();
        for id in due {
            tracing::info!("{id} is installed again, to be current");
            self.install(&id, cx);
        }
    }

    /// Whether something goes on in a session of `agent`: a turn, a sign-in,
    /// or a question to you.
    pub(crate) fn agent_busy(&self, agent: &str) -> bool {
        self.sessions.iter().any(|s| {
            s.agent == agent
                && (matches!(
                    s.state,
                    crate::session::State::Working
                        | crate::session::State::Opening
                        | crate::session::State::SignIn
                ) || s.waiting().is_some())
        })
    }
}
