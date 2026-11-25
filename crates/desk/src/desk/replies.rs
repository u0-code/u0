//! What comes back to the window: signing in, and every message from the
//! agents and the work done off the window's thread.

use agent_client_protocol::v1::schema::{ElicitationScope, SessionId, SessionUpdate};
use gpui::{Context, Window};

use super::Desk;
use crate::{
    acp,
    hub::{self, Msg, Slot, Stage},
    proto::{Call, Event},
    session::State,
    sessions::Loaded,
};

impl Desk {
    /// Sign in to `agent` with its method `method`.
    pub fn sign_in(&mut self, agent: &str, method: &str, cx: &mut Context<Self>) {
        use agent_client_protocol::v1::schema::AuthMethod;
        let Some(slot) = self.slot(agent).filter(|s| s.running()) else {
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
            self.transport.send(Call::SignIn {
                agent: agent.to_owned(),
                method: method.to_owned(),
            });
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

    /// How many sessions need you, on the Dock's icon, when it changes.
    pub(crate) fn show_badge(&mut self) {
        let n = self
            .sessions
            .all
            .iter()
            .filter(|s| s.parent.is_none() && (s.asks() || s.state == State::SignIn))
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

    pub(super) fn take(&mut self, msg: Msg, window: &mut Window, cx: &mut Context<Self>) {
        // No window of Ghost's in front: what needs you is notified.
        self.behind = cx.active_window().is_none();
        match msg {
            Msg::Event(e) => self.event(e, window, cx),
            Msg::Attached(attached) => {
                self.composer.reading = self.composer.reading.saturating_sub(1);
                if let Some(attached) = attached {
                    self.add_attached(attached, cx);
                }
                // What waited for its pictures goes now.
                if self.composer.reading == 0
                    && let Some(other) = self.composer.held
                {
                    self.send_typed(other, window, cx);
                }
            }
            Msg::OnPath(found) => self.on_path = found,
            Msg::Keys(Ok(yours)) => {
                cx.clear_key_bindings();
                crate::keys::bind(cx, &yours);
            }
            Msg::Keys(Err(why)) => self.trouble("Couldn't keep the keys", Some(why), None),
            Msg::Notice(Ok(text)) => self.done(text, None),
            Msg::Notice(Err((text, why))) => self.trouble(text, Some(why), None),
        }
        cx.notify();
    }

    /// What the server says.
    fn event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Grepped { words, found } => {
                // Only what answers what is typed now.
                if self.palette.state.read(cx).query(cx).trim() == words {
                    self.palette.grep = found;
                }
            }
            Event::Files { folder, files } => {
                self.files.insert(folder, files);
            }
            Event::FileRead { path, body } => self.file_read(&path, body, window, cx),
            Event::Forge {
                folder,
                kind,
                answer,
            } => self.forge_answered(&folder, kind, answer, cx),
            Event::Prs {
                folder,
                prs,
                issues,
            } => {
                self.prs.insert(folder.clone(), prs);
                self.issues.insert(folder, issues);
            }
            Event::PrText { number, text } => {
                for m in &mut self.composer.mentions {
                    if matches!(m.what, crate::desk::mention::Offer::Pr { number: n, .. } if n == number)
                    {
                        m.text.clone_from(&text);
                    }
                }
            }
            Event::Cloning { what, share } => {
                if let Some(c) = &mut self.adding.cloning {
                    c.what = what;
                    c.share = share.or(c.share);
                }
            }
            Event::Cloned { dir, result } => {
                self.adding.cloning = None;
                match result {
                    Ok(()) => {
                        self.close_palette(window, cx);
                        self.add_and_begin(&dir, window, cx);
                    }
                    Err(why) if why == crate::registry::CANCELLED => {}
                    Err(why) => self.trouble("Couldn't add the project", Some(why), None),
                }
            }
            Event::Folders { dir, names } => {
                self.palette.folders.insert(dir, names);
            }
            Event::Diffed { key, scope, result } => {
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
            Event::Commits {
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
            Event::Pr { key, pr } => {
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
            Event::GitStep { key, text } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.git.step = Some(text);
                }
            }
            Event::Sizes(sizes) => self.pages.sizes = sizes,
            Event::Tidied(said) => self.pages.tidied = Some(said),
            Event::Looked {
                key,
                folder,
                branch,
                repo,
                scripts,
            } => self.looked(key, &folder, branch, repo, scripts),
            Event::TermOpened { id, result } => self.terminal_opened(id, result),
            Event::TermOutput { id, bytes } => {
                if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) {
                    t.printed(&bytes);
                }
            }
            Event::TermEnded { id, how } => {
                if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) {
                    t.ended = Some(how);
                }
                self.setup_ended(id);
            }
            Event::TermClosed { id, busy } => self.terminal_closed(id, busy, cx),
            Event::Hosts(hosts) => self.adding.hosts = hosts,
            Event::Held { folder, held } => {
                self.held.insert(folder, held);
            }
            Event::Placed { key, starts } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    for (id, starts) in starts {
                        s.placed(&id, starts);
                    }
                }
            }
            Event::Uncommitted { key, result } => {
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
            Event::Worktree { key, result } => self.worktree_made(key, result, cx),
            Event::Checkpoint {
                key,
                turn,
                after,
                result,
                then,
            } => self.checkpointed(key, turn, after, result, then),
            Event::Restored {
                key,
                turn,
                undo,
                result,
            } => self.restored(key, turn, undo, result),
            Event::Git {
                key,
                action,
                result,
            } => self.gitted(key, &action, result),
            Event::Notice(Ok(text)) => self.done(text, None),
            Event::Notice(Err((text, why))) => self.trouble(text, Some(why), None),
            Event::Registry(Ok(registry)) => {
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
            Event::Registry(Err(why)) => self.registry_trouble = Some(why),
            Event::Installing { agent, what, share } => {
                if let Some(Stage::Installing(i)) = self.slot_mut(&agent).map(|s| &mut s.stage) {
                    i.what = what;
                    i.share = share;
                }
            }
            Event::Installed { agent, result } => {
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
            Event::Started { agent, result } => {
                self.started(&agent, result);
                self.install_due(cx);
            }
            Event::Opened { key, result } => self.opened(key, result),
            Event::Listed { agent, result } => self.listed(&agent, result),
            Event::Probed { agent, result } => self.probed(&agent, result),
            Event::CliVersion { cli, version } => {
                self.cli_versions.insert(cli, version);
            }
            Event::Elsewhere { agent, folders } => {
                self.elsewhere.insert(agent, folders);
            }
            Event::Forked { from, at, result } => self.forked(from, at, result, window, cx),
            Event::Loaded {
                key,
                resumed,
                result,
            } => self.loaded(key, resumed, result),
            Event::Weighed(mut weights) => {
                // In the order the agents are listed.
                weights.sort_by_key(|(id, ..)| self.slots.iter().position(|s| &s.entry.id == id));
                self.weights = weights;
            }
            Event::Here(here) => self.here = here,
            Event::Deleted { key, result } => match result {
                Ok(()) => self.forget_session(key, false, cx),
                Err(why) => {
                    let title = self
                        .sessions
                        .at(key)
                        .map(|s| crate::ui::fit(&s.meta.title, 40))
                        .unwrap_or_default();
                    self.trouble(format!("Couldn't delete “{title}”"), Some(why), None);
                }
            },
            Event::Ended { key, result } => {
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
                // with another session on screen, a notice leads to it; each
                // as you asked for it.
                let on = &self.settings.notify_on;
                if let Some(s) = self.sessions.at(key)
                    && (if ok { on.finished } else { on.failed })
                {
                    let name = self.name_of(&s.meta.agent);
                    let title = crate::ui::fit(&s.meta.title, 40);
                    if behind {
                        self.tell(
                            key,
                            format!("{name} · {title}"),
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
                if ok {
                    self.suggest_next(key);
                }
                self.refresh_git(key);
                // What was said meanwhile goes now, in order.
                self.say_queued(key, cx);
            }
            Event::Written { key, ask, result } => self.written(key, ask, result, window, cx),
            Event::LoggedOut { agent, result } => {
                if let Some(slot) = self.slot_mut(&agent) {
                    match result {
                        Ok(()) => slot.auth = hub::Auth::Needed("Signed out".into()),
                        Err(why) => tracing::warn!("{agent} could not sign out: {why}"),
                    }
                }
            }
            Event::Said { agent, event } => self.said(&agent, *event, window, cx),
            Event::Appended { agent, id, events } => {
                if let Some(slot) = self.slot_mut(&agent) {
                    slot.used = std::time::Instant::now();
                }
                if let Some(key) = self.sessions.appended(&agent, &id, events) {
                    self.place(key);
                }
            }
            Event::Signed { agent, result } => self.signed(&agent, result),
            Event::Options { key, options } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.options = options;
                }
            }
            Event::Index(index) => self.keep_from(index),
            Event::Record { key, events } => self.record_read(key, events),
            Event::Found { query, keys } => {
                if self.search.input.read(cx).value().trim().to_lowercase() == query {
                    self.search.found = keys.into_iter().collect();
                }
            }
            Event::Refused { key, why } => {
                let name = self
                    .sessions
                    .at(key)
                    .map(|s| self.name_of(&s.meta.agent))
                    .unwrap_or_default();
                self.trouble(format!("{name} kept it as it was"), Some(why), None);
            }
        }
    }

    pub(super) fn started(
        &mut self,
        agent: &str,
        result: Result<
            (
                Option<u32>,
                Box<agent_client_protocol::v1::schema::InitializeResponse>,
            ),
            crate::acp::Error,
        >,
    ) {
        match result {
            Ok((pid, init)) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.known = Some(init);
                    slot.stage = Stage::Running { pid };
                    slot.used = std::time::Instant::now();
                } else {
                    // Switched off while it started.
                    self.transport.send(Call::Stop {
                        agent: agent.to_owned(),
                    });
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
                if self.slot(&agent).is_some_and(Slot::running) {
                    if let Some(m) = mode {
                        self.transport.send(Call::SetMode {
                            agent: agent.clone(),
                            key,
                            session: id.clone(),
                            mode: m,
                        });
                    }
                    for (config, value) in chosen {
                        self.transport.send(Call::SetOption {
                            agent: agent.clone(),
                            key,
                            session: id.clone(),
                            config,
                            value,
                        });
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
        result: Result<crate::proto::Opened, crate::acp::Error>,
    ) {
        match self.sessions.loaded(key, resumed, result) {
            Loaded::Open(said) => {
                // What you said goes in its record before what the agent
                // says to it.
                self.flush(key);
                if let Some(text) = said {
                    self.begin_turn(key, text);
                }
                self.to_bottom();
                self.save_index();
            }
            Loaded::Refused => {
                self.drop_record(key);
                if let Some(agent) = self.sessions.at(key).map(|s| s.meta.agent.clone()) {
                    self.open_when_running(&agent, key);
                }
            }
            Loaded::Stays => {}
        }
    }

    /// The agent `agent` asks you something in the session `key`: you are
    /// told, unless you said not to be or have it on screen, and it is
    /// marked unread.
    fn asks_you(&mut self, agent: &str, key: u64) {
        let Some(title) = self.sessions.at(key).map(|s| s.meta.title.clone()) else {
            return;
        };
        let name = self.name_of(agent);
        let title = crate::ui::fit(&title, 40);
        if !self.settings.notify_on.asks {
            // Not told, as you asked.
        } else if self.behind {
            self.tell(key, format!("{name} · {title}"), "Asks before it goes on");
        } else if self.current != Some(key) {
            self.done(
                format!("{name} asks before it goes on in “{title}”"),
                Some(super::notices::Way::Show(key)),
            );
        }
        if self.current != Some(key)
            && let Some(s) = self.sessions.at_mut(key)
        {
            s.meta.flags.unread = true;
        }
    }

    pub(super) fn said(
        &mut self,
        agent: &str,
        event: acp::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(slot) = self.slot_mut(agent) {
            slot.used = std::time::Instant::now();
        }
        match event {
            acp::Event::Task { session, task } => {
                if let Some(s) = self.sessions.by_id(agent, &SessionId::new(session)) {
                    s.task(task);
                }
            }
            // Said in no session open with it: the probe's, which lists the
            // commands the agent takes.
            acp::Event::Update(n) => {
                if let SessionUpdate::AvailableCommandsUpdate(c) = n.update
                    && let Some(slot) = self.slot_mut(agent)
                {
                    slot.commands = c.available_commands;
                }
            }
            acp::Event::Permission { id, request } => {
                if let Some(s) = self.sessions.by_id(agent, &request.session_id) {
                    let key = s.key;
                    s.asked(id, request);
                    self.asks_you(agent, key);
                } else if self.slot(agent).is_some_and(Slot::running) {
                    self.transport.send(Call::Answer {
                        agent: agent.to_owned(),
                        request: id,
                        answer: acp::Answer::cancelled(false),
                    });
                }
            }
            acp::Event::Elicit { id, request } => {
                let session = match request.scope() {
                    ElicitationScope::Session(s) => Some(s.session_id.clone()),
                    _ => None,
                };
                let key = session
                    .and_then(|sid| self.sessions.by_id(agent, &sid))
                    .map(|s| {
                        s.elicited(id, request);
                        s.key
                    });
                match key {
                    Some(key) => {
                        self.form_asked(agent, key, id, window, cx);
                        self.asks_you(agent, key);
                    }
                    // Asked of no session open here: no one to ask.
                    None => self.transport.send(Call::Answer {
                        agent: agent.to_owned(),
                        request: id,
                        answer: acp::Answer::cancelled(true),
                    }),
                }
            }
            acp::Event::Exited { pid, why } => {
                // One let go while idle ended as it was meant to, and one
                // let go before this one started is not this one's end:
                // one that stops as it starts says so in `Started`.
                let Some(slot) = self.slot_mut(agent).filter(|s| s.runs(pid)) else {
                    return;
                };
                slot.stage = Stage::Failed(why.clone());
                self.transport.send(Call::Stop {
                    agent: agent.to_owned(),
                });
                for key in self.sessions.exited(agent, &why) {
                    self.flush(key);
                }
            }
        }
    }
}
