//! The agents switched on, on each server the window holds: their slots
//! filled and started, the sessions each keeps listed, signing in and out,
//! installing and taking away; what you set for an agent from its page (a
//! command line of yours that starts it, model ids of yours), kept in the
//! settings of its server; and its health checked every few minutes, and
//! the registry read again every few hours.

use std::path::PathBuf;

use gpui::{Context, Window};

use super::Desk;
use crate::{
    hub::{self, Host, Slot, Stage},
    proto::Call,
    registry::Entry,
};

impl Desk {
    /// Start the agent `id` on `host` with the command typed on its page,
    /// or, with none, the registry's way; it starts again with it at once.
    pub fn save_command(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        let line = self.pages.command.read(cx).value().trim().to_owned();
        let commands = &mut self.settings_on_mut(host).commands;
        if line.is_empty() {
            commands.remove(id);
        } else {
            commands.insert(id.to_owned(), line.clone());
        }
        if let Some(slot) = self.slot_mut(host, id) {
            slot.command = (!line.is_empty()).then_some(line);
        }
        self.start_again(host, id, cx);
    }

    /// Read this Mac's settings again, where they did not read as u0
    /// started: taken, and the agents they switch on started; or why not,
    /// again.
    pub(crate) fn read_settings_again(&mut self, cx: &mut Context<Self>) {
        let (settings, refused) = crate::settings::Settings::load(&self.home);
        if refused.is_none() {
            self.settings = settings;
            self.fill_slots(Host::HERE);
            self.settings_took(cx);
            self.done(crate::tr("Your settings are read"), None);
        } else {
            self.trouble(
                crate::tr("Your settings still do not read"),
                refused.clone(),
                None,
            );
        }
        self.settings_refused = refused;
        cx.notify();
    }

    /// What runs of the agent `id` on `host` is let go, and it starts
    /// again as the settings there, saved now, say.
    fn start_again(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        self.save_settings_on(host);
        // What runs is let go; it starts again the new way, once the
        // server says it can.
        self.let_go(host, id);
        self.look_here(host);
        cx.notify();
    }

    /// The variable typed on the page of the agent `id` on `host`, kept in
    /// that server's Keychain where it is marked secret; it starts again
    /// with it.
    pub fn add_var(&mut self, host: Host, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.pages.var_name.read(cx).value().trim().to_owned();
        let value = self.pages.var_value.read(cx).value().to_string();
        if name.is_empty() {
            return;
        }
        let secret = self.pages.var_secret;
        if secret {
            self.call(
                host,
                Call::Secret {
                    account: crate::secrets::env_account(id, &name),
                    value: Some(value.clone()),
                },
            );
        }
        let vars = (self.settings_on_mut(host).env)
            .entry(id.to_owned())
            .or_default();
        vars.insert(name, (!secret).then_some(value));
        for input in [&self.pages.var_name, &self.pages.var_value] {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.start_again(host, id, cx);
    }

    /// Take the variable `name` from the agent `id` on `host`, and out of
    /// that server's Keychain where it was kept there.
    pub fn drop_var(&mut self, host: Host, id: &str, name: &str, cx: &mut Context<Self>) {
        let env = &mut self.settings_on_mut(host).env;
        let Some(vars) = env.get_mut(id) else {
            return;
        };
        let secret = matches!(vars.remove(name), Some(None));
        if vars.is_empty() {
            env.remove(id);
        }
        if secret {
            self.call(
                host,
                Call::Secret {
                    account: crate::secrets::env_account(id, name),
                    value: None,
                },
            );
        }
        self.start_again(host, id, cx);
    }

    /// A model id of yours for the agent `id` on `host`, typed on its page.
    pub fn add_model(&mut self, host: Host, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.pages.model.read(cx).value().trim().to_owned();
        if model.is_empty() {
            return;
        }
        let list = (self.settings_on_mut(host).models)
            .entry(id.to_owned())
            .or_default();
        if !list.contains(&model) {
            list.push(model);
        }
        self.pages
            .model
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.save_settings_on(host);
        cx.notify();
    }

    pub fn drop_model(&mut self, host: Host, id: &str, model: &str, cx: &mut Context<Self>) {
        let models = &mut self.settings_on_mut(host).models;
        if let Some(list) = models.get_mut(id) {
            list.retain(|m| m != model);
            if list.is_empty() {
                models.remove(id);
            }
        }
        self.save_settings_on(host);
        cx.notify();
    }

    /// Look at what of each agent the registry lists is on `host`, for the
    /// pages that show it.
    pub(crate) fn look_here(&self, host: Host) {
        self.call(
            host,
            Call::Here {
                agents: self.registry.agents.clone(),
                commands: self.settings_on(host).commands.clone(),
            },
        );
    }

    /// Ask what u0 runs on `host`, and which are the agents, for
    /// Settings › Diagnostics.
    pub(crate) fn weigh_agents(&self, host: Host) {
        self.call(host, Call::Weigh);
    }

    /// Ask what u0 runs on `host` and what its log says went wrong.
    pub(crate) fn diagnose(&self, host: Host) {
        self.weigh_agents(host);
        self.call(host, Call::Troubles);
    }

    /// Ask the process `pid`, one u0 runs on `host`, to stop (TERM),
    /// and look again.
    pub(crate) fn stop_process(&self, host: Host, pid: u32) {
        self.call(
            host,
            Call::Signal {
                pid,
                signal: "TERM".into(),
            },
        );
        self.weigh_agents(host);
    }

    /// Every five minutes, look again at the agents not running: one now
    /// installed (or its command now here) is started; one that stopped is
    /// tried again, updated first if it is behind. Every hour, read the
    /// registry again.
    pub(crate) fn check_agents(&mut self) {
        self.health_tick += 1;
        if self.health_tick.is_multiple_of(5) {
            // What each server says is there restages them, and starts
            // those that can start.
            for m in &self.machines {
                self.look_here(m.host);
            }
        }
        if self.settings.check_agents && self.health_tick.is_multiple_of(60) {
            self.call(Host::HERE, Call::Fetch);
        }
    }

    /// Install the registry's newest of each agent switched on on `host`
    /// that is not there, or that failed as an older version: what is
    /// there runs as it is, and is updated only when it does not work. One
    /// you start with a command of yours is left as it is.
    pub(crate) fn install_due(&mut self, host: Host, cx: &mut Context<Self>) {
        let due: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.host == host && s.command.is_none())
            .filter(|s| {
                let e = &s.entry;
                let installed = self.here_on(host, &e.id).and_then(|h| h.installed.as_ref());
                match (&s.stage, installed) {
                    _ if e.way() == crate::registry::Way::None => false,
                    (Stage::Missing(crate::registry::Missing::Download), None) => true,
                    (Stage::Failed(_), Some(v)) => *v != e.version,
                    _ => false,
                }
            })
            .filter(|slot| !self.agent_busy(host, &slot.entry.id))
            .map(|s| s.entry.id.clone())
            .collect();
        for id in due {
            tracing::info!("{id} is installed at the registry's newest");
            self.install(host, &id, cx);
        }
    }

    /// Whether something goes on in a session of `agent` on `host`: a
    /// turn, a sign-in, a question to you, a subagent at work or a task
    /// left running in the background (a dev server goes with the agent's
    /// process).
    pub(crate) fn agent_busy(&self, host: Host, agent: &str) -> bool {
        use crate::session::State;
        self.sessions.all.iter().any(|s| {
            s.of(host, agent)
                && (matches!(s.state, State::Working | State::Opening | State::SignIn)
                    || s.asks()
                    || (s.state != State::Closed
                        && (s.sub.working() || s.tasks.iter().any(|t| t.ended.is_none()))))
        })
    }

    /// What the registry says of the agent `id`, or, for one it no longer
    /// lists, what its slot does.
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.registry
            .agents
            .iter()
            .find(|e| e.id == id)
            .or_else(|| {
                self.slots
                    .iter()
                    .find(|s| s.entry.id == id)
                    .map(|s| &s.entry)
            })
    }

    /// The slots of the agents switched on on `host`.
    pub fn slots_on(&self, host: Host) -> impl Iterator<Item = &Slot> {
        self.slots.iter().filter(move |s| s.host == host)
    }

    /// The slot of the agent `id` on `host`.
    pub fn slot(&self, host: Host, id: &str) -> Option<&Slot> {
        self.slots
            .iter()
            .find(|s| s.host == host && s.entry.id == id)
    }

    pub(crate) fn slot_mut(&mut self, host: Host, id: &str) -> Option<&mut Slot> {
        self.slots
            .iter_mut()
            .find(|s| s.host == host && s.entry.id == id)
    }

    /// Make the slots on `host` match the agents switched on there,
    /// keeping those already there as they are.
    pub(crate) fn fill_slots(&mut self, host: Host) {
        let (mut old, others): (Vec<Slot>, Vec<Slot>) = std::mem::take(&mut self.slots)
            .into_iter()
            .partition(|s| s.host == host);
        self.slots = others;
        let settings = self.settings_on(host);
        let mut filled = Vec::new();
        for id in &settings.enabled {
            if let Some(at) = old.iter().position(|s| &s.entry.id == id) {
                filled.push(old.remove(at));
            } else if let Some(entry) = crate::registry::entry_of(&self.registry, id) {
                filled.push(Slot::new(
                    host,
                    entry,
                    // An account beside the first is installed as its agent is.
                    (self.here_on(host, id))
                        .or_else(|| self.here_on(host, crate::registry::instance(id).0)),
                    settings.commands.get(id).cloned(),
                ));
            }
        }
        // This Mac's agents first, in the order each server lists them.
        self.slots.extend(filled);
        self.slots.sort_by_key(|s| s.host);
        // Those switched off are let go.
        for slot in old {
            self.call(
                host,
                Call::Stop {
                    agent: slot.entry.id,
                },
            );
        }
        self.start_ready(host);
    }

    /// Start each agent switched on on `host` that can run and has not run
    /// yet, so that the sessions it keeps are listed. One let go after
    /// running idle starts again when it is wanted, not here.
    pub(crate) fn start_ready(&mut self, host: Host) {
        let ready: Vec<String> = (self.slots.iter())
            .filter(|s| s.host == host && matches!(s.stage, Stage::Ready) && s.known.is_none())
            .map(|s| s.entry.id.clone())
            .collect();
        for id in ready {
            self.start_for(host, &id);
        }
    }

    /// Start `agent` on `host` if it is not running or starting: something
    /// waits on it.
    pub(crate) fn start_for(&mut self, host: Host, agent: &str) {
        let env = (self.settings_on(host).env.get(agent))
            .cloned()
            .unwrap_or_default();
        if let Some(slot) = self.slot_mut(host, agent)
            && matches!(slot.stage, Stage::Ready)
        {
            slot.stage = Stage::Starting;
            let call = Call::Start {
                entry: slot.entry.clone(),
                command: slot.command.clone(),
                env,
            };
            self.call(host, call);
        }
    }

    /// Ask `agent` on `host`, if it is running and can list them, for the
    /// sessions it keeps in the projects there.
    pub(crate) fn list_sessions(&self, host: Host, agent: &str) {
        let Some(slot) = self.slot(host, agent) else {
            return;
        };
        let Some(caps) = slot.caps().filter(|_| slot.running()) else {
            return;
        };
        if caps.session_capabilities.list.is_none() {
            return;
        }
        self.call(
            host,
            Call::List {
                agent: agent.to_owned(),
                places: self.settings_on(host).folders.clone(),
                under: self.worktree_folders(host),
            },
        );
    }

    /// Read what a session of `agent` on `host` offers before one is
    /// opened, and whether it is signed in, by opening one and deleting it
    /// again.
    pub(crate) fn probe_agent(&self, host: Host, agent: &str) {
        if let Some((dir, delete, close)) = self.aside_place(host, agent) {
            self.call(
                host,
                Call::Probe {
                    agent: agent.to_owned(),
                    dir,
                    delete,
                    close,
                },
            );
        }
    }

    /// Where a session of `agent` on `host` that is not yours is opened,
    /// while it runs: u0's home there, out of every project; and
    /// whether it can then be deleted, or else closed.
    pub(crate) fn aside_place(&self, host: Host, agent: &str) -> Option<(PathBuf, bool, bool)> {
        let slot = self.slot(host, agent)?;
        let caps = &slot.caps().filter(|_| slot.running())?.session_capabilities;
        Some((
            self.home_on(host).to_path_buf(),
            caps.delete.is_some(),
            caps.close.is_some(),
        ))
    }

    /// Sign in to `agent` on `host` the way it offers: its own method when
    /// it lists one, else its own program in a terminal, where it signs
    /// in: on this Mac, Terminal's; on another server, one of u0's
    /// there.
    pub fn sign_in_agent(&mut self, host: Host, agent: &str, cx: &mut Context<Self>) {
        let Some(slot) = self.slot(host, agent) else {
            return;
        };
        let method = slot.sign_ins().next().map(|m| m.id().0.to_string());
        if let Some(method) = method {
            self.sign_in(host, agent, &method, cx);
            return;
        }
        if host != Host::HERE {
            return self.sign_in_agent_there(host, agent, cx);
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

    /// Sign in to `agent` on the server `host`, elsewhere: its own program
    /// there (the CLI its adapter drives, as the server says), run in one
    /// of u0's terminals there, beside the session on screen.
    pub(crate) fn sign_in_agent_there(&mut self, host: Host, agent: &str, cx: &mut Context<Self>) {
        let cli = self.here_on(host, agent).and_then(|h| h.cli.clone());
        let name = self.name_of(agent);
        match cli {
            Some(cli) => {
                let line = shell_words::quote(&cli.to_string_lossy()).into_owned();
                self.terminal_on(host, &line, cx);
            }
            None => self.trouble(
                crate::tr_with("Sign in to %{name} on its server", &[("name", &name)]),
                Some(crate::tr("u0 there does not know the program it signs in with.").into()),
                None,
            ),
        }
    }

    /// Read again whether `agent` on `host` is signed in.
    pub fn recheck_auth(&mut self, host: Host, agent: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.slot_mut(host, agent) {
            slot.auth = hub::Auth::Unknown;
        }
        self.probe_agent(host, agent);
        cx.notify();
    }

    /// Sign `agent` on `host` out, when it can be.
    pub fn log_out(&mut self, host: Host, agent: &str, cx: &mut Context<Self>) {
        if self.slot(host, agent).is_some_and(Slot::running) {
            self.call(
                host,
                Call::SignOut {
                    agent: agent.to_owned(),
                },
            );
        }
        cx.notify();
    }

    /// Whether `agent` on `host` can delete the sessions it keeps, as it
    /// said when it last started.
    pub fn can_delete(&self, host: Host, agent: &str) -> bool {
        self.slot(host, agent)
            .and_then(Slot::caps)
            .is_some_and(|k| k.session_capabilities.delete.is_some())
    }

    /// Install the agent `id` on `host` from its archive, or update it to
    /// the registry's version.
    pub fn install(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        // One running is let go first; it starts again once installed.
        if matches!(
            self.slot(host, id).map(|s| &s.stage),
            Some(Stage::Running { .. })
        ) {
            self.let_go(host, id);
        }
        if let Some(slot) = self.slot_mut(host, id) {
            slot.stage = Stage::Installing(hub::Install {
                what: crate::tr("Starting").into(),
                share: None,
            });
            let call = Call::Install {
                entry: slot.entry.clone(),
            };
            self.call(host, call);
        }
        cx.notify();
    }

    /// Stop the install of `id` going on on `host`; nothing of it is kept.
    pub fn cancel_install(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        if let Some(Stage::Installing(_)) = self.slot(host, id).map(|s| &s.stage) {
            self.call(
                host,
                Call::CancelInstall {
                    agent: id.to_owned(),
                },
            );
            cx.notify();
        }
    }

    /// Take the download of the agent `id` off `host`, once you say so; it
    /// stops first if it is running.
    pub fn uninstall(&mut self, host: Host, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        let name = self.name_of(id);
        let (yes, no) = (crate::tr("Uninstall"), crate::tr("Cancel"));
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &crate::tr_with("Uninstall %{name}?", &[("name", &name)]),
            Some(&crate::tr_with(
                "Its download in %{folder} is removed. Its sessions and sign-in stay with %{name}, and it can be installed again.",
                &[
                    ("folder", &Desk::shown(&self.home_on(host).join("agents").join(id))),
                    ("name", &name),
                ],
            )),
            &[yes.as_ref(), no.as_ref()],
            cx,
        );
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    // One running stops first.
                    this.call(host, Call::Stop { agent: id.clone() });
                    if let Some(slot) = this.slot_mut(host, &id) {
                        slot.stage = Stage::Ready;
                    }
                    this.call(host, Call::Uninstall { entry });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Look again for what an agent on `host` needs (a runner installed
    /// meanwhile).
    pub fn recheck(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        self.call(
            host,
            Call::Stop {
                agent: id.to_owned(),
            },
        );
        if let Some(slot) = self.slot_mut(host, id) {
            slot.stage = Stage::Ready;
        }
        self.look_here(host);
        cx.notify();
    }

    /// Switch the agent `id` on `host` on or off.
    pub fn toggle_agent(&mut self, host: Host, id: &str, cx: &mut Context<Self>) {
        self.settings_on_mut(host).toggle(id);
        self.save_settings_on(host);
        self.fill_slots(host);
        cx.notify();
    }

    /// Add another account of `agent` on `host`, named as typed: the agent
    /// switched on once more under an id of its own, its CLI pointed at a
    /// folder of its own in u0's home there (by the variable
    /// `registry::KNOWN` names), so that it signs in apart; it starts with
    /// the variables `agent` has.
    pub fn add_account(
        &mut self,
        host: Host,
        agent: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.pages.account.read(cx).value().to_string();
        let Some(id) = crate::registry::account_id(agent, &typed) else {
            return;
        };
        let Some(var) = crate::registry::known(agent).and_then(|k| k.4) else {
            return;
        };
        if self.settings_on(host).enabled.contains(&id) {
            self.trouble(
                crate::tr_with(
                    "There is an account named “%{name}” already",
                    &[("name", &typed.trim())],
                ),
                None,
                None,
            );
            return;
        }
        let folder = self.home_on(host).join("accounts").join(&id);
        let settings = self.settings_on_mut(host);
        let mut env = settings.env.get(agent).cloned().unwrap_or_default();
        env.insert(var.to_owned(), Some(folder.display().to_string()));
        settings.env.insert(id.clone(), env);
        if let Some(command) = settings.commands.get(agent).cloned() {
            settings.commands.insert(id.clone(), command);
        }
        settings.toggle(&id);
        self.save_settings_on(host);
        self.fill_slots(host);
        self.pages
            .account
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.open_page(super::Page::Agent(host, id), window, cx);
    }

    /// Take the account `id` on `host` away: switched off, and its
    /// variables and command forgotten. Its folder, with its sign-in,
    /// stays.
    pub fn drop_account(
        &mut self,
        host: Host,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = crate::registry::instance(id).0.to_owned();
        let settings = self.settings_on_mut(host);
        settings.enabled.retain(|e| e != id);
        settings.env.remove(id);
        settings.commands.remove(id);
        self.save_settings_on(host);
        self.fill_slots(host);
        self.open_page(super::Page::Agent(host, agent), window, cx);
    }

    /// Read the registry again.
    pub fn refresh_registry(&mut self, cx: &mut Context<Self>) {
        self.call(Host::HERE, Call::Fetch);
        self.registry_trouble = None;
        cx.notify();
    }
}
