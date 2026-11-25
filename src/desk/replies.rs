//! What comes back to the window: signing in, and every message from the
//! agents and the work done off the window's thread.

use agent_client_protocol::v1::schema::{SessionId, SessionUpdate};
use gpui::Context;

use super::Desk;
use crate::{
    acp::Event,
    hub::{self, Msg, Slot, Stage},
    session::State,
    sessions::Loaded,
};

impl Desk {
    /// Sign in to `agent` with its method `method`.
    pub fn sign_in(&mut self, agent: &str, method: &str, cx: &mut Context<Self>) {
        use agent_client_protocol::v1::schema::AuthMethod;
        let Some(slot) = self.slot(agent) else { return };
        let Some(running) = slot.running().cloned() else {
            return;
        };
        let terminal = slot.sign_ins().find_map(|m| match m {
            AuthMethod::Terminal(t) if &*t.id.0 == method => Some(t.clone()),
            _ => None,
        });
        let launch = slot.launch(&self.home);
        if let Some(t) = terminal {
            // A login typed in a terminal: once it is done there, opening
            // the session is tried again from its card.
            if let Ok(mut launch) = launch {
                launch.args.extend(t.args);
                launch.env.extend(t.env);
                if let Err(e) =
                    crate::macos::in_terminal(&launch.program, &launch.args, &launch.env)
                {
                    let name = self.name_of(agent);
                    self.trouble(
                        format!("Couldn't open a terminal to sign in to {name}"),
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        } else {
            self.signing = Some(agent.to_owned());
            hub::authenticate(
                running,
                agent.to_owned(),
                method.to_owned(),
                self.tx.clone(),
            );
        }
        cx.notify();
    }

    /// Try again to open the sessions of `agent` that wait to be signed in.
    pub fn retry_open(&mut self, agent: &str, cx: &mut Context<Self>) {
        for key in self.sessions.signed_in(agent) {
            self.open_when_running(agent, key);
        }
        cx.notify();
    }

    pub(super) fn signed(&mut self, agent: &str, result: Result<(), String>) {
        self.signing = None;
        match result {
            Ok(()) => {
                for key in self.sessions.signed_in(agent) {
                    self.open_when_running(agent, key);
                }
            }
            Err(why) => {
                let name = self.name_of(agent);
                self.trouble(
                    format!("Signing in to {name} failed"),
                    Some(why),
                    Some(super::notices::Way::Agent(agent.to_owned())),
                );
            }
        }
    }

    /// Say through macOS what a session did, as you asked it to.
    fn tell(&self, title: &str, body: &str) {
        match self.settings.notify {
            crate::settings::Notify::Off => {}
            crate::settings::Notify::Banner => hub::notify(title, body, false),
            crate::settings::Notify::BannerAndSound => hub::notify(title, body, true),
        }
    }

    /// How many sessions need you, on the Dock's icon, when it changes.
    pub(crate) fn show_badge(&mut self) {
        let n = self
            .sessions
            .all
            .iter()
            .filter(|s| s.parent.is_none() && (s.waiting().is_some() || s.state == State::SignIn))
            .count();
        if n != self.badge {
            self.badge = n;
            crate::macos::badge(n);
        }
    }

    /// While an agent works, and you asked for it, the Mac does not sleep
    /// (`caffeinate`, for as long as Ghost runs); otherwise it may.
    pub(crate) fn stay_awake(&mut self) {
        let working = self.settings.awake
            && self
                .sessions
                .all
                .iter()
                .any(|s| matches!(s.state, State::Working | State::Opening));
        match (&mut self.awake, working) {
            (None, true) => {
                self.awake = std::process::Command::new("caffeinate")
                    .args(["-i", "-w", &std::process::id().to_string()])
                    .spawn()
                    .map_err(|e| tracing::warn!("caffeinate could not be started: {e}"))
                    .ok();
            }
            (Some(child), false) => {
                let _ = child.kill();
                let _ = child.wait();
                self.awake = None;
            }
            _ => {}
        }
    }

    // What comes back.

    pub(super) fn take(&mut self, msg: Msg, cx: &mut Context<Self>) {
        // No window of Ghost's in front: what needs you is notified.
        self.behind = cx.active_window().is_none();
        match msg {
            Msg::Registry(Ok(registry)) => {
                self.registry = registry;
                self.registry_read = Some(std::time::SystemTime::now());
                self.registry_trouble = None;
                // A newer version of an agent replaces its entry; one
                // running keeps running as it is.
                for slot in &mut self.slots {
                    if let Some(e) = self.registry.agents.iter().find(|e| e.id == slot.entry.id) {
                        slot.entry = e.clone();
                        slot.restage(&self.home);
                    }
                }
                self.fill_slots();
                self.install_due(cx);
                self.look_here();
            }
            Msg::Registry(Err(why)) => self.registry_trouble = Some(why),
            Msg::Installing { agent, what, share } => {
                if let Some(Stage::Installing(i)) = self.slot_mut(&agent).map(|s| &mut s.stage) {
                    i.what = what;
                    i.share = share;
                }
            }
            Msg::Installed { agent, result } => {
                let home = self.home.clone();
                let name = self.name_of(&agent);
                if let Some(slot) = self.slot_mut(&agent) {
                    match result {
                        // An update that did not come leaves the version
                        // here running, and says why.
                        Err(why)
                            if why != crate::registry::CANCELLED && slot.launch(&home).is_ok() =>
                        {
                            slot.stage = Stage::Ready;
                            self.trouble(
                                format!("{name} was not updated"),
                                Some(why),
                                Some(super::notices::Way::Agent(agent.clone())),
                            );
                        }
                        Err(why) if why != crate::registry::CANCELLED => {
                            slot.stage = Stage::Failed(why);
                        }
                        _ => {
                            slot.stage = Stage::Ready;
                            slot.restage(&home);
                        }
                    }
                }
                self.look_here();
                self.start_ready();
            }
            Msg::Started { agent, result } => {
                self.started(&agent, result);
                self.install_due(cx);
            }
            Msg::Opened { key, result } => self.opened(key, result),
            Msg::Listed { agent, result } => self.listed(&agent, result),
            Msg::Probed { agent, result } => self.probed(&agent, result),
            Msg::Terminal(id) => {
                if let Some(t) = self.terminals.iter().find(|t| t.id == id) {
                    t.changed.store(false, std::sync::atomic::Ordering::Release);
                }
            }
            Msg::TerminalOpened { id, result } => self.terminal_opened(id, result),
            Msg::TerminalEnded(id) => {
                if let Some(t) = self.terminals.iter_mut().find(|t| t.id == id) {
                    t.end();
                }
                self.setup_ended(id);
            }
            Msg::CliVersion { cli, version } => {
                self.cli_versions.insert(cli, version);
            }
            Msg::Elsewhere { agent, folders } => {
                self.elsewhere.insert(agent, folders);
            }
            Msg::Forked { from, at, result } => self.forked(from, at, result, cx),
            Msg::Picture(Some(picture)) => self.add_picture(picture, cx),
            Msg::Picture(None) => {}
            Msg::Grepped { words, found } => {
                // Only what answers what is typed now.
                if self.palette_input.read(cx).text().trim() == words {
                    self.grep = found;
                }
            }
            Msg::Files { folder, files } => {
                self.files.insert(folder, files);
            }
            Msg::Prs {
                folder,
                prs,
                issues,
            } => {
                self.prs.insert(folder.clone(), prs);
                self.issues.insert(folder, issues);
            }
            Msg::PrText { number, text } => {
                for m in &mut self.mentions {
                    if matches!(m.what, crate::desk::mention::Offer::Pr { number: n, .. } if n == number)
                    {
                        m.text.clone_from(&text);
                    }
                }
            }
            Msg::Cloning { what, share } => {
                if let Some(c) = &mut self.cloning {
                    c.what = what;
                    c.share = share.or(c.share);
                }
            }
            Msg::Cloned { dir, result } => {
                self.cloning = None;
                match result {
                    Ok(()) => {
                        self.new_name.update(cx, |i, cx| i.set_text("", cx));
                        self.adopt(&dir);
                    }
                    Err(why) if why == crate::registry::CANCELLED => {}
                    Err(why) => self.adding_trouble = Some(why),
                }
            }
            Msg::Diffed { key, scope, result } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    match result {
                        Ok(files) => s.git.scoped = Some((scope, files)),
                        Err(why) => {
                            s.git.scoped = Some((scope, Vec::new()));
                            self.trouble("Couldn't read the changes", Some(why), None);
                        }
                    }
                }
            }
            Msg::Commits {
                key,
                commits,
                branches,
                upstream,
            } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.git.commits = commits;
                    s.git.branches = branches;
                    s.git.upstream = upstream;
                }
            }
            Msg::Pr { key, pr } => {
                let merged = pr.as_ref().is_some_and(|p| p.state == "merged");
                if let Some(s) = self.sessions.at_mut(key) {
                    s.git.pr = pr;
                    // Its pull request merged: the session is done.
                    if merged && s.meta.flags.settled.is_none() {
                        s.meta.flags.settled =
                            Some(crate::store::secs(std::time::SystemTime::now()));
                        self.save_index();
                    }
                }
            }
            Msg::GitStep { key, text } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.git.step = Some(text);
                }
            }
            Msg::Sizes(sizes) => self.sizes = sizes,
            Msg::Tidied(said) => self.tidied = Some(said),
            Msg::Found { query, keys } => {
                if self.searching && self.search.read(cx).text().to_lowercase() == query {
                    self.found = keys.into_iter().collect();
                }
            }
            Msg::Loaded {
                key,
                resumed,
                result,
            } => self.loaded(key, resumed, result),
            Msg::Looked {
                key,
                folder,
                branch,
                repo,
                scripts,
            } => self.looked(key, &folder, branch, repo, scripts),
            Msg::Weighed(weights) => self.weights = weights,
            Msg::Hosts(hosts) => self.hosts = hosts,
            Msg::Here(here) => self.here = here,
            Msg::OnPath(found) => self.on_path = found,
            Msg::Keys(Ok(yours)) => {
                cx.clear_key_bindings();
                crate::keys::bind(cx, &yours);
            }
            Msg::Keys(Err(why)) => self.trouble("Couldn't keep the keys", Some(why), None),
            Msg::Held { folder, held } => {
                self.held.insert(folder, held);
            }
            Msg::Notice(Ok(text)) => self.done(text, None),
            Msg::Notice(Err((text, why))) => self.trouble(text, Some(why), None),
            Msg::Placed { key, starts } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    for (id, starts) in starts {
                        s.placed(&id, starts);
                    }
                }
            }
            Msg::Record { key, events } => self.record_read(key, events),
            Msg::Uncommitted { key, result } => {
                if let Some(s) = self.sessions.at_mut(key)
                    && s.turns.is_empty()
                {
                    match result {
                        Ok(files) => s.total = Some(files),
                        Err(why) => {
                            tracing::warn!("what is not committed could not be read: {why}");
                        }
                    }
                }
            }
            Msg::Deleted { key, result } => match result {
                Ok(()) => self.forget_session(key, cx),
                Err(why) => {
                    let title = self
                        .sessions
                        .at(key)
                        .map(|s| crate::ui::fit(&s.meta.title, 40))
                        .unwrap_or_default();
                    self.trouble(format!("Couldn't delete “{title}”"), Some(why), None);
                }
            },
            Msg::Ended { key, result } => {
                // A turn refused for want of a sign-in marks the agent: the
                // composer then offers its sign-in.
                let agent = self.sessions.at(key).map(|s| s.meta.agent.clone());
                if let Some(slot) = agent.and_then(|a| self.slot_mut(&a)) {
                    slot.auth = match &result {
                        Err(f) if f.code == crate::acp::ErrorCode::AUTH_REQUIRED => {
                            hub::Auth::Needed(f.message.clone())
                        }
                        Ok(_) => hub::Auth::Ready,
                        Err(_) => slot.auth.clone(),
                    };
                }
                let away = self.current != Some(key);
                let behind = self.behind;
                let ok = result.is_ok();
                if let Some(s) = self.sessions.at_mut(key) {
                    s.ended(result.map_err(|f| f.message));
                    // A turn that ended while you looked elsewhere is unread.
                    s.meta.flags.unread |= away || behind;
                }

                // With Ghost behind other windows, macOS says so; in front,
                // with another session on screen, a notice leads to it.
                if let Some(s) = self.sessions.at(key) {
                    let name = self.name_of(&s.meta.agent);
                    let title = crate::ui::fit(&s.meta.title, 40);
                    if behind {
                        self.tell(
                            &format!("{name} · {title}"),
                            if ok { "Finished its turn" } else { "Stopped" },
                        );
                    } else if away && s.parent.is_none() {
                        self.done(
                            format!(
                                "{name} {} “{title}”",
                                if ok { "finished" } else { "stopped in" }
                            ),
                            Some(super::notices::Way::Show(key)),
                        );
                    }
                }
                self.stopping = None;
                self.end_turn(key);
                self.take_message(key, cx);
                self.name_branch(key);
                self.refresh_git(key);
                // What was said meanwhile goes now, in order.
                self.say_queued(key, cx);
            }
            Msg::LoggedOut { agent, result } => {
                if let Some(slot) = self.slot_mut(&agent) {
                    match result {
                        Ok(()) => slot.auth = hub::Auth::Needed("Signed out".into()),
                        Err(why) => tracing::warn!("{agent} could not sign out: {why}"),
                    }
                }
            }
            Msg::Worktree { key, result } => self.worktree_made(key, result, cx),
            Msg::Checkpoint {
                key,
                turn,
                after,
                result,
                then,
            } => self.checkpointed(key, turn, after, result, then),
            Msg::Restored {
                key,
                turn,
                undo,
                result,
            } => self.restored(key, turn, undo, result),
            Msg::Git {
                key,
                action,
                result,
            } => self.gitted(key, &action, result),
            Msg::Said { agent, event } => self.said(&agent, *event),
            Msg::Signed { agent, result } => self.signed(&agent, result),
            Msg::Options { key, options } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.options = options;
                }
            }
            Msg::Refused { key, why } => {
                let name = self
                    .sessions
                    .at(key)
                    .map(|s| self.name_of(&s.meta.agent))
                    .unwrap_or_default();
                self.trouble(format!("{name} kept it as it was"), Some(why), None);
            }
        }
        cx.notify();
    }

    pub(super) fn started(
        &mut self,
        agent: &str,
        result: Result<
            (
                std::sync::Arc<crate::acp::Agent>,
                Box<agent_client_protocol::v1::schema::InitializeResponse>,
            ),
            crate::acp::Error,
        >,
    ) {
        match result {
            Ok((running, init)) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.known = Some(init);
                    slot.stage = Stage::Running { agent: running };
                    slot.used = std::time::Instant::now();
                }
                for key in self.sessions.started(agent, None) {
                    self.open_when_running(agent, key);
                }
                // What waited for it to start, now.
                let (now, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting)
                    .into_iter()
                    .partition(|(a, _)| a == agent);
                self.waiting = wait;
                for (_, w) in now {
                    self.do_waiting(w);
                }
                self.list_sessions(agent);
                self.probe_agent(agent);
            }
            Err(f) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Failed(f.message.clone());
                }
                self.sessions.started(agent, Some(&f.message));
            }
        }
    }

    pub(super) fn opened(
        &mut self,
        key: u64,
        result: Result<
            Box<agent_client_protocol::v1::schema::NewSessionResponse>,
            crate::acp::Error,
        >,
    ) {
        let tx = self.tx.clone();
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        match result {
            Ok(r) => {
                s.meta.id = Some(r.session_id.clone());
                s.modes = r.modes;
                s.options = r.config_options.unwrap_or_default();
                s.state = State::Idle;
                // What was chosen before it was opened is set on it now,
                // ahead of the first thing said.
                let chosen = std::mem::take(&mut s.chosen);
                let mode = s.chosen_mode.take();
                for (config, value) in &chosen {
                    s.choose(config, value);
                }
                s.chosen.clear();
                if let (Some(m), Some(modes)) = (&mode, &mut s.modes) {
                    modes.current_mode_id =
                        agent_client_protocol::v1::schema::SessionModeId::new(m.clone());
                }
                let agent = s.meta.agent.clone();
                let id = r.session_id.clone();
                if let Some(running) = self.slot(&agent).and_then(Slot::running).cloned() {
                    if let Some(m) = mode {
                        hub::set_mode(running.clone(), key, id.clone(), m, tx.clone());
                    }
                    for (config, value) in chosen {
                        hub::set_option(
                            running.clone(),
                            key,
                            id.clone(),
                            config,
                            value,
                            tx.clone(),
                        );
                    }
                }
                self.flush(key);
                self.save_index();
                let s = self.sessions.at_mut(key).expect("just opened");
                if let Some(text) = s.pending.take() {
                    s.state = State::Working;
                    self.begin_turn(key, text);
                }
            }
            // The text waits: once signed in, the session opens and it goes.
            Err(f) if f.code == crate::acp::ErrorCode::AUTH_REQUIRED => {
                s.state = State::SignIn;
                let agent = s.meta.agent.clone();
                if let Some(slot) = self.slot_mut(&agent) {
                    slot.auth = hub::Auth::Needed(f.message);
                }
            }
            Err(f) => {
                s.pending = None;
                s.since = None;
                s.state = State::Broken(f.message);
            }
        }
    }

    /// What `agent` listed: each session not on the desk yet is put there,
    /// closed; one closed already takes the agent's title and time.
    pub(super) fn listed(
        &mut self,
        agent: &str,
        result: Result<
            Vec<(
                std::path::PathBuf,
                agent_client_protocol::v1::schema::SessionInfo,
            )>,
            String,
        >,
    ) {
        let found = match result {
            Ok(found) => found,
            Err(why) => {
                tracing::warn!("{agent} could not list its sessions: {why}");
                return;
            }
        };
        self.sessions.listed(agent, found, &self.gone);
        self.settle_idle();
        self.save_index();
    }

    pub(super) fn probed(
        &mut self,
        agent: &str,
        result: Result<
            Box<agent_client_protocol::v1::schema::NewSessionResponse>,
            crate::acp::Error,
        >,
    ) {
        let Some(at) = self.slots.iter().position(|s| s.entry.id == agent) else {
            return;
        };
        match result {
            Ok(r) => {
                let slot = &mut self.slots[at];
                slot.probe = Some(hub::Probe {
                    modes: r.modes,
                    options: r.config_options.unwrap_or_default(),
                });
                slot.auth = hub::Auth::Ready;
                let slot = &self.slots[at];
                for s in self.sessions.all.iter_mut().filter(|s| {
                    s.meta.agent == agent && s.meta.id.is_none() && s.options.is_empty()
                }) {
                    slot.seed(s);
                }
            }
            Err(f) if f.code == crate::acp::ErrorCode::AUTH_REQUIRED => {
                self.slots[at].auth = hub::Auth::Needed(f.message);
            }
            Err(f) => tracing::warn!("{agent} would not open a session to be read: {}", f.message),
        }
    }

    pub(super) fn loaded(
        &mut self,
        key: u64,
        resumed: bool,
        result: Result<hub::Opened, crate::acp::Error>,
    ) {
        match self.sessions.loaded(key, resumed, result, &self.home) {
            Loaded::Open(said) => {
                if let Some(text) = said {
                    self.begin_turn(key, text);
                }
                self.to_bottom();
                self.flush(key);
                self.save_index();
            }
            Loaded::Refused => {
                if let Some(agent) = self.sessions.at(key).map(|s| s.meta.agent.clone()) {
                    self.open_when_running(&agent, key);
                }
            }
            Loaded::Stays => {}
        }
    }

    pub(super) fn said(&mut self, agent: &str, event: Event) {
        if let Some(slot) = self.slot_mut(agent) {
            slot.used = std::time::Instant::now();
        }
        match event {
            Event::Task { session, task } => {
                if let Some(s) = self.sessions.by_id(agent, &SessionId::new(session)) {
                    s.task(task);
                }
            }
            Event::Update(n) => match self.sessions.update(agent, n) {
                Ok((key, titled)) => {
                    // A long turn is written as it goes, not only at its end.
                    let long = self.sessions.at(key).is_some_and(|s| s.log.len() >= 400);
                    self.place(key);
                    if long {
                        self.flush(key);
                    }
                    // A title come from the agent names the worktree's branch.
                    if titled {
                        self.name_branch(key);
                    }
                }
                // The probe's session lists the commands the agent takes.
                Err(update) => {
                    if let SessionUpdate::AvailableCommandsUpdate(c) = *update
                        && let Some(slot) = self.slot_mut(agent)
                    {
                        slot.commands = c.available_commands;
                    }
                }
            },
            Event::Permission { id, request } => {
                if let Some(s) = self.sessions.by_id(agent, &request.session_id) {
                    let (key, title) = (s.key, s.meta.title.clone());
                    s.asked(id, request);
                    if self.behind {
                        let name = self.name_of(agent);
                        self.tell(
                            &format!("{name} · {}", crate::ui::fit(&title, 40)),
                            "Asks before it goes on",
                        );
                    }
                    if self.current != Some(key)
                        && let Some(s) = self.sessions.at_mut(key)
                    {
                        s.meta.flags.unread = true;
                    }
                } else if let Some(running) = self.slot(agent).and_then(Slot::running) {
                    hub::answer(running, id, None);
                }
            }
            Event::Exited { pid, why } => {
                // One let go while idle ended as it was meant to, and one
                // let go before this one started is not this one's end:
                // one that stops as it starts says so in `Started`.
                let Some(slot) = self.slot_mut(agent).filter(|s| s.runs(pid)) else {
                    return;
                };
                slot.stage = Stage::Failed(why.clone());
                for key in self.sessions.exited(agent, &why) {
                    self.flush(key);
                }
            }
        }
    }
}
