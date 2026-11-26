//! The agents switched on: their slots filled and started, the sessions
//! each keeps listed, signing in and out, installing and taking away;
//! what you set for an agent from its page (a command line of yours that
//! starts it, model ids of yours); and its health checked every few
//! minutes, and the registry read again every few hours.

use std::path::PathBuf;

use gpui::{Context, Window};

use super::Desk;
use crate::{
    hub::{self, Slot, Stage},
    proto::Call,
    registry::Entry,
};

impl Desk {
    /// Start the agent `id` with the command typed on its page, or, with
    /// none, the registry's way; it starts again with it at once.
    pub fn save_command(&mut self, id: &str, cx: &mut Context<Self>) {
        let line = self.pages.command.read(cx).value().trim().to_owned();
        if line.is_empty() {
            self.settings.commands.remove(id);
        } else {
            self.settings.commands.insert(id.to_owned(), line.clone());
        }
        if let Some(slot) = self.slot_mut(id) {
            slot.command = (!line.is_empty()).then_some(line);
        }
        self.start_again(id, cx);
    }

    /// Read the settings again, where they did not read as Ghost started:
    /// taken, and the agents they switch on started; or why not, again.
    pub(crate) fn read_settings_again(&mut self, cx: &mut Context<Self>) {
        let (settings, refused) = crate::settings::Settings::load(&self.home);
        if refused.is_none() {
            self.settings = settings;
            self.fill_slots();
            self.settings_took(cx);
            self.done("Your settings are read", None);
        } else {
            self.trouble("Your settings still do not read", refused.clone(), None);
        }
        self.settings_refused = refused;
        cx.notify();
    }

    /// What runs of the agent `id` is let go, and it starts again as the
    /// settings, saved now, say.
    fn start_again(&mut self, id: &str, cx: &mut Context<Self>) {
        self.save_settings();
        // What runs is let go; it starts again the new way, once the
        // server says it can.
        self.let_go(id);
        self.look_here();
        cx.notify();
    }

    /// The variable typed on the agent `id`'s page, kept in the Keychain
    /// where it is marked secret; it starts again with it.
    pub fn add_var(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.pages.var_name.read(cx).value().trim().to_owned();
        let value = self.pages.var_value.read(cx).value().to_string();
        if name.is_empty() {
            return;
        }
        let secret = self.pages.var_secret;
        if secret {
            self.transport.send(Call::Secret {
                account: crate::secrets::env_account(id, &name),
                value: Some(value.clone()),
            });
        }
        let vars = self.settings.env.entry(id.to_owned()).or_default();
        vars.insert(name, (!secret).then_some(value));
        for input in [&self.pages.var_name, &self.pages.var_value] {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.start_again(id, cx);
    }

    /// Take the variable `name` from the agent `id`, and out of the
    /// Keychain where it was kept there.
    pub fn drop_var(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(vars) = self.settings.env.get_mut(id) else {
            return;
        };
        if let Some(None) = vars.remove(name) {
            self.transport.send(Call::Secret {
                account: crate::secrets::env_account(id, name),
                value: None,
            });
        }
        if vars.is_empty() {
            self.settings.env.remove(id);
        }
        self.start_again(id, cx);
    }

    /// A model id of yours for the agent `id`, typed on its page.
    pub fn add_model(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.pages.model.read(cx).value().trim().to_owned();
        if model.is_empty() {
            return;
        }
        let list = self.settings.models.entry(id.to_owned()).or_default();
        if !list.contains(&model) {
            list.push(model);
        }
        self.pages
            .model
            .update(cx, |i, cx| i.set_value("", window, cx));
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
        self.transport.send(Call::Here {
            agents: self.registry.agents.clone(),
            commands: self.settings.commands.clone(),
        });
    }

    /// Ask what Ghost runs, and which are the agents, for Settings ›
    /// Diagnostics.
    pub(crate) fn weigh_agents(&self) {
        self.transport.send(Call::Weigh);
    }

    /// Ask what Ghost runs and what its log says went wrong.
    pub(crate) fn diagnose(&self) {
        self.weigh_agents();
        self.transport.send(Call::Troubles);
    }

    /// Ask the process `pid`, one Ghost runs, to stop (TERM), and look
    /// again.
    pub(crate) fn stop_process(&self, pid: u32) {
        self.transport.send(Call::Signal {
            pid,
            signal: "TERM".into(),
        });
        self.weigh_agents();
    }

    /// Every five minutes, look again at the agents not running: one now
    /// installed (or its command now here) is started; one that stopped is
    /// tried again, updated first if it is behind. Every hour, read the
    /// registry again.
    pub(crate) fn check_agents(&mut self) {
        self.health_tick += 1;
        if self.health_tick.is_multiple_of(5) {
            // What the server says is here restages them, and starts those
            // that can start.
            self.look_here();
        }
        if self.settings.check_agents && self.health_tick.is_multiple_of(60) {
            self.transport.send(Call::Fetch);
        }
    }

    /// Install the registry's newest of each agent switched on that is not
    /// here, or that failed as an older version: what is here runs as it
    /// is, and is updated only when it does not work. One you start with a
    /// command of yours is left as it is.
    pub(crate) fn install_due(&mut self, cx: &mut Context<Self>) {
        let due: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.command.is_none())
            .filter(|s| {
                let e = &s.entry;
                let installed = self.here.get(&e.id).and_then(|h| h.installed.as_ref());
                match (&s.stage, installed) {
                    _ if e.way() == crate::registry::Way::None => false,
                    (Stage::Missing(crate::registry::Missing::Download), None) => true,
                    (Stage::Failed(_), Some(v)) => *v != e.version,
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
                    || s.asks()
                    || (s.state != State::Closed
                        && (s.sub.working() || s.tasks.iter().any(|t| t.ended.is_none()))))
        })
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.registry
            .agents
            .iter()
            .find(|e| e.id == id)
            .or_else(|| self.slot(id).map(|s| &s.entry))
    }

    pub fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.entry.id == id)
    }

    pub(crate) fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.entry.id == id)
    }

    /// Make the slots match the agents switched on, keeping those already
    /// there as they are.
    pub(crate) fn fill_slots(&mut self) {
        let mut old: Vec<Slot> = std::mem::take(&mut self.slots);
        for id in &self.settings.enabled {
            if let Some(at) = old.iter().position(|s| &s.entry.id == id) {
                self.slots.push(old.remove(at));
            } else if let Some(entry) = crate::registry::entry_of(&self.registry, id) {
                self.slots.push(Slot::new(
                    entry,
                    // An account beside the first is installed as its agent is.
                    (self.here.get(id)).or_else(|| self.here.get(crate::registry::instance(id).0)),
                    self.settings.commands.get(id).cloned(),
                ));
            }
        }
        // Those switched off are let go.
        for slot in old {
            self.transport.send(Call::Stop {
                agent: slot.entry.id,
            });
        }
        self.start_ready();
    }

    /// Start each agent switched on that can run and has not run yet, so
    /// that the sessions it keeps are listed. One let go after running
    /// idle starts again when it is wanted, not here.
    pub(crate) fn start_ready(&mut self) {
        for slot in &mut self.slots {
            if matches!(slot.stage, Stage::Ready) && slot.known.is_none() {
                slot.stage = Stage::Starting;
                self.transport.send(Call::Start {
                    entry: slot.entry.clone(),
                    command: slot.command.clone(),
                    env: self
                        .settings
                        .env
                        .get(&slot.entry.id)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }
    }

    /// Start `agent` if it is not running or starting: something waits on
    /// it.
    pub(crate) fn start_for(&mut self, agent: &str) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.entry.id == agent)
            && matches!(slot.stage, Stage::Ready)
        {
            slot.stage = Stage::Starting;
            self.transport.send(Call::Start {
                entry: slot.entry.clone(),
                command: slot.command.clone(),
                env: self
                    .settings
                    .env
                    .get(&slot.entry.id)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }

    /// Ask `agent`, if it is running and can list them, for the sessions
    /// it keeps in the projects.
    pub(crate) fn list_sessions(&self, agent: &str) {
        let Some(slot) = self.slot(agent) else { return };
        let Some(caps) = slot.caps().filter(|_| slot.running()) else {
            return;
        };
        if caps.session_capabilities.list.is_none() {
            return;
        }
        let places = self.settings.folders.clone();
        self.transport.send(Call::List {
            agent: agent.to_owned(),
            places,
            under: self
                .settings
                .worktree_folders(self.transport.home_there(&self.home)),
        });
    }

    /// Read what a session of `agent` offers before one is opened, and
    /// whether it is signed in, by opening one and deleting it again.
    pub(crate) fn probe_agent(&self, agent: &str) {
        if let Some((dir, delete, close)) = self.aside_place(agent) {
            self.transport.send(Call::Probe {
                agent: agent.to_owned(),
                dir,
                delete,
                close,
            });
        }
    }

    /// Where a session of `agent` that is not yours is opened, while it
    /// runs: Ghost's home, out of every project; and whether it can then
    /// be deleted, or else closed.
    pub(crate) fn aside_place(&self, agent: &str) -> Option<(PathBuf, bool, bool)> {
        let slot = self.slot(agent)?;
        let caps = &slot.caps().filter(|_| slot.running())?.session_capabilities;
        Some((
            self.home.clone(),
            caps.delete.is_some(),
            caps.close.is_some(),
        ))
    }

    /// Sign in to `agent` the way it offers: its own method when it lists
    /// one, else its own program in a terminal, where it signs in.
    pub fn sign_in_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        let Some(slot) = self.slot(agent) else { return };
        let method = slot.sign_ins().next().map(|m| m.id().0.to_string());
        if let Some(method) = method {
            self.sign_in(agent, &method, cx);
            return;
        }
        match slot.entry.own_cli(&self.home) {
            Some(cli) => {
                if let Err(e) = crate::macos::in_terminal(&cli, &[], &[]) {
                    tracing::warn!("a terminal could not be opened: {e}");
                }
            }
            None => {
                if let Some(site) = &slot.entry.website {
                    cx.open_url(site);
                }
            }
        }
    }

    /// Read again whether `agent` is signed in.
    pub fn recheck_auth(&mut self, agent: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.slot_mut(agent) {
            slot.auth = hub::Auth::Unknown;
        }
        self.probe_agent(agent);
        cx.notify();
    }

    /// Sign `agent` out, when it can be.
    pub fn log_out(&mut self, agent: &str, cx: &mut Context<Self>) {
        if self.slot(agent).is_some_and(Slot::running) {
            self.transport.send(Call::SignOut {
                agent: agent.to_owned(),
            });
        }
        cx.notify();
    }

    /// Whether `agent` can delete the sessions it keeps, as it said when it
    /// last started.
    pub fn can_delete(&self, agent: &str) -> bool {
        self.slot(agent)
            .and_then(Slot::caps)
            .is_some_and(|k| k.session_capabilities.delete.is_some())
    }

    /// Install the agent `id` from its archive, or update it to the
    /// registry's version.
    pub fn install(&mut self, id: &str, cx: &mut Context<Self>) {
        // One running is let go first; it starts again once installed.
        if matches!(self.slot(id).map(|s| &s.stage), Some(Stage::Running { .. })) {
            self.let_go(id);
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.entry.id == id) {
            slot.stage = Stage::Installing(hub::Install {
                what: "Starting".into(),
                share: None,
            });
            self.transport.send(Call::Install {
                entry: slot.entry.clone(),
            });
        }
        cx.notify();
    }

    /// Stop the install of `id` going on; nothing of it is kept.
    pub fn cancel_install(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(Stage::Installing(_)) = self.slot(id).map(|s| &s.stage) {
            self.transport.send(Call::CancelInstall {
                agent: id.to_owned(),
            });
            cx.notify();
        }
    }

    /// Take the agent `id`'s download off this machine, once you say so;
    /// it stops first if it is running.
    pub fn uninstall(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        let name = self.name_of(id);
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Uninstall {name}?"),
            Some(&format!(
                "Its download in {} is removed. Its sessions and sign-in stay with {name}, and it can be installed again.",
                Desk::shown(&self.home.join("agents").join(id))
            )),
            &["Uninstall", "Cancel"],
            cx,
        );
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    // One running stops first.
                    this.transport.send(Call::Stop { agent: id.clone() });
                    if let Some(slot) = this.slot_mut(&id) {
                        slot.stage = Stage::Ready;
                    }
                    this.transport.send(Call::Uninstall { entry });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Look again for what an agent needs (a runner installed meanwhile).
    pub fn recheck(&mut self, id: &str, cx: &mut Context<Self>) {
        self.transport.send(Call::Stop {
            agent: id.to_owned(),
        });
        if let Some(slot) = self.slot_mut(id) {
            slot.stage = Stage::Ready;
        }
        self.look_here();
        cx.notify();
    }

    /// Switch the agent `id` on or off.
    pub fn toggle_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        self.settings.toggle(id);
        self.save_settings();
        self.fill_slots();
        cx.notify();
    }

    /// Add another account of `agent`, named as typed: the agent switched
    /// on once more under an id of its own, its CLI pointed at a folder of
    /// its own (by the variable `registry::KNOWN` names), so that it signs
    /// in apart; it starts with the variables `agent` has.
    pub fn add_account(&mut self, agent: &str, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.pages.account.read(cx).value().to_string();
        let Some(id) = crate::registry::account_id(agent, &typed) else {
            return;
        };
        let Some(var) = crate::registry::known(agent).and_then(|k| k.4) else {
            return;
        };
        if self.settings.enabled.contains(&id) {
            self.trouble(
                format!("There is an account named “{}” already", typed.trim()),
                None,
                None,
            );
            return;
        }
        let mut env = self.settings.env.get(agent).cloned().unwrap_or_default();
        let folder = self.home.join("accounts").join(&id);
        env.insert(var.to_owned(), Some(folder.display().to_string()));
        self.settings.env.insert(id.clone(), env);
        if let Some(command) = self.settings.commands.get(agent).cloned() {
            self.settings.commands.insert(id.clone(), command);
        }
        self.settings.toggle(&id);
        self.save_settings();
        self.fill_slots();
        self.pages
            .account
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.open_page(super::Page::Agent(id), window, cx);
    }

    /// Take the account `id` away: switched off, and its variables and
    /// command forgotten. Its folder, with its sign-in, stays.
    pub fn drop_account(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let agent = crate::registry::instance(id).0.to_owned();
        self.settings.enabled.retain(|e| e != id);
        self.settings.env.remove(id);
        self.settings.commands.remove(id);
        self.save_settings();
        self.fill_slots();
        self.open_page(super::Page::Agent(agent), window, cx);
    }

    /// Read the registry again.
    pub fn refresh_registry(&mut self, cx: &mut Context<Self>) {
        self.transport.send(Call::Fetch);
        self.registry_trouble = None;
        cx.notify();
    }
}
