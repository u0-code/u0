//! What comes back to the window: signing in, how each server's link
//! stands, and every message from the agents and the work done off the
//! window's thread, each from the server it came from.

use agent_client_protocol::v1::schema::{ElicitationScope, SessionId, SessionUpdate};
use gpui::{Context, Window};

use super::Desk;
use crate::{
    acp,
    hub::{self, Host, Msg, Slot, Stage},
    proto::{Call, Event},
    session::State,
    sessions::Loaded,
};

impl Desk {
    /// Sign in to `agent` on `host` with its method `method`.
    pub fn sign_in(&mut self, host: Host, agent: &str, method: &str, cx: &mut Context<Self>) {
        use agent_client_protocol::v1::schema::AuthMethod;
        let Some(slot) = self.slot(host, agent).filter(|s| s.running()) else {
            return;
        };
        let terminal = slot.sign_ins().find_map(|m| match m {
            AuthMethod::Terminal(t) if &*t.id.0 == method => Some(t.clone()),
            _ => None,
        });
        if let Some(t) = terminal {
            // A login typed in a terminal: once it is done there, opening
            // the session is tried again from its card. On another server
            // it is typed in a terminal there, through its own program.
            if host != Host::HERE {
                return self.sign_in_agent_there(host, agent, cx);
            }
            if let Ok(mut launch) = slot.launch(&self.home) {
                launch.args.extend(t.args);
                launch.env.extend(t.env);
                if let Err(e) =
                    crate::macos::in_terminal(&launch.program, &launch.args, &launch.env)
                {
                    let name = self.name_of(agent);
                    self.trouble(
                        crate::tr_with(
                            "Couldn't open a terminal to sign in to %{name}",
                            &[("name", &name)],
                        ),
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        } else {
            self.signing = Some((host, agent.to_owned()));
            self.call(
                host,
                Call::SignIn {
                    agent: agent.to_owned(),
                    method: method.to_owned(),
                },
            );
        }
        cx.notify();
    }

    /// Try again to open the sessions of `agent` on `host` that wait to be
    /// signed in.
    pub fn retry_open(&mut self, host: Host, agent: &str, cx: &mut Context<Self>) {
        for key in self.sessions.signed_in(host, agent) {
            self.open_when_running(host, agent, key);
        }
        cx.notify();
    }

    pub(super) fn signed(&mut self, host: Host, agent: &str, result: Result<(), String>) {
        self.signing = None;
        match result {
            Ok(()) => {
                for key in self.sessions.signed_in(host, agent) {
                    self.open_when_running(host, agent, key);
                }
            }
            Err(why) => {
                let name = self.name_of(agent);
                self.trouble(
                    crate::tr_with("Signing in to %{name} failed", &[("name", &name)]),
                    Some(why),
                    Some(super::notices::Way::Agent(host, agent.to_owned())),
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
    /// (`caffeinate`, for as long as u0 runs); otherwise it may.
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
        // No window of u0's in front: what needs you is notified.
        self.behind = cx.active_window().is_none();
        match msg {
            Msg::Event(host, e) => self.event(host, e, window, cx),
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
            Msg::Update(update) => self.updated(update, cx),
            Msg::Keys(Ok(yours)) => {
                cx.clear_key_bindings();
                crate::keys::bind(cx, &yours);
            }
            Msg::Keys(Err(why)) => {
                self.trouble(crate::tr("Couldn't keep the keys"), Some(why), None);
            }
            Msg::Notice(Ok(text)) => self.done(text, None),
            Msg::Notice(Err((text, why))) => self.trouble(text, Some(why), None),
            Msg::Link(host, state) => self.linked(host, state),
            Msg::Forwarded {
                host,
                address,
                result,
            } => self.forwarded(host, &address, result, cx),
            Msg::Paired(paired) => {
                self.pages.pairing = false;
                self.read_servers();
                self.hold_servers();
                match paired {
                    Ok(crate::link::Paired {
                        name,
                        back,
                        waiting,
                    }) => {
                        self.pages
                            .pair_link
                            .update(cx, |i, cx| i.set_value("", window, cx));
                        match back {
                            None if waiting => self.done(
                                crate::tr_with(
                                    "Paired with %{name}: it comes into this window once it is let in there, under Settings › Devices",
                                    &[("name", &name)],
                                ),
                                None,
                            ),
                            None => self.done(
                                crate::tr_with(
                                    "Paired with %{name}: its projects are down the left",
                                    &[("name", &name)],
                                ),
                                None,
                            ),
                            Some(Ok(theirs)) => self.done(
                                crate::tr_with(
                                    "Paired with %{name} both ways: it controls this Mac with the key %{key}",
                                    &[("name", &name), ("key", &crate::key::fingerprint(&theirs))],
                                ),
                                None,
                            ),
                            Some(Err(why)) => self.trouble(
                                crate::tr_with("Paired with %{name} one way only", &[("name", &name)]),
                                Some(
                                    crate::tr_with("It could not reach this Mac back: %{why}", &[("why", &why)])
                                        .into(),
                                ),
                                None,
                            ),
                        }
                    }
                    Err(why) => self.trouble(crate::tr("Couldn't pair"), Some(why), None),
                }
            }
        }
        cx.notify();
    }

    /// How the link to the server `host` stands. Reached the first time,
    /// or come back without all said meanwhile (it restarted, or too much
    /// was said), what the desk has of it is read afresh; dropped, it is
    /// shown out of reach, what the desk has of it still read, and it is
    /// reached again on its own.
    pub(crate) fn linked(&mut self, host: Host, state: crate::link::State) {
        use crate::link::State;
        let Some(m) = self.machine_mut(host) else {
            return;
        };
        let name = m.name.clone();
        match state {
            State::Up(welcome) => {
                let was = std::mem::replace(&mut m.up, hub::Up::On);
                m.home = welcome.home;
                m.version = Some(welcome.version);
                m.scope = welcome.scope;
                if welcome.resumed {
                    if matches!(was, hub::Up::Lost(_)) {
                        self.done(crate::tr_with("%{name} is back", &[("name", &name)]), None);
                    }
                    return;
                }
                // This Mac's settings are read from its home, which its
                // service shares.
                if host != Host::HERE {
                    (m.settings, m.refused) = match welcome.settings {
                        Ok(s) => (*s, None),
                        Err(why) => (crate::settings::Settings::default(), Some(why)),
                    };
                }
                self.afresh(host);
            }
            State::Lost(why) => {
                let was = std::mem::replace(&mut m.up, hub::Up::Lost(why.clone()));
                if was == hub::Up::On {
                    self.trouble(
                        crate::tr_with("Lost %{name}; reaching it again", &[("name", &name)]),
                        Some(why),
                        None,
                    );
                }
            }
            State::Refused(why) => {
                m.up = hub::Up::Refused(why.clone());
                self.trouble(
                    crate::tr_with("%{name} turned this Mac away", &[("name", &name)]),
                    Some(why),
                    None,
                );
            }
        }
    }

    /// What the desk has of the server `host` read from it again: as it is
    /// first reached, and as it comes back restarted. Its sessions on the
    /// desk stay, those that were open closed (their agents went with the
    /// server), to be opened again when written in.
    pub(crate) fn afresh(&mut self, host: Host) {
        for s in self.sessions.all.iter_mut().filter(|s| s.host == host) {
            if !matches!(s.state, State::Draft | State::Closed) {
                s.state = State::Closed;
                s.pending = None;
                s.since = None;
            }
        }
        self.forget_host(host, false);
        self.call(host, Call::Watch);
        if host == Host::HERE {
            self.call(host, Call::Fetch);
        }
        self.fill_slots(host);
        self.look_here(host);
    }

    /// Let go of what the desk has of the server `host`: its agents, what
    /// it read of its projects and its terminals; and, `sessions`, its
    /// sessions too.
    pub(crate) fn forget_host(&mut self, host: Host, sessions: bool) {
        let gone: Vec<u64> = (self.sessions.all.iter())
            .filter(|s| s.host == host && sessions)
            .map(|s| s.key)
            .collect();
        self.sessions.all.retain(|s| !gone.contains(&s.key));
        if self.current.is_some_and(|k| gone.contains(&k)) {
            self.current = None;
        }
        self.kept.retain(|(h, ..), _| *h != host);
        self.waiting.retain(|(h, ..)| *h != host);
        self.held.retain(|p, _| p.host != host);
        self.files.retain(|p, _| p.host != host);
        self.slots.retain(|s| s.host != host);
        self.drawer.terminals.retain(|t| t.host != host);
    }

    /// What the server `host` says.
    fn event(&mut self, host: Host, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        let at = |folder| super::Place::new(host, folder);
        match event {
            Event::Grepped { words, found } => {
                // Only what answers what is typed now.
                if self.palette.state.read(cx).query(cx).trim() == words {
                    self.palette.grep = found;
                }
            }
            Event::Files { folder, files } => {
                self.files.insert(at(folder), files);
            }
            Event::FileRead { path, body } => self.file_read(&path, body, window, cx),
            Event::Forge {
                folder,
                kind,
                answer,
            } => self.forge_answered(&at(folder), kind, answer, window, cx),
            Event::Prs {
                folder,
                prs,
                issues,
            } => {
                self.prs.insert(at(folder.clone()), prs);
                self.issues.insert(at(folder), issues);
                self.menu_changed += 1;
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
                let url = self.adding.url.take();
                match result {
                    Ok(()) => {
                        self.close_palette(window, cx);
                        self.add_and_begin(&at(dir), window, cx);
                    }
                    Err(why) if why == crate::registry::CANCELLED => {}
                    Err(why) => self.trouble(
                        crate::tr("Couldn't add the project"),
                        Some(why),
                        url.map(|u| super::notices::Way::Clone(u, at(dir))),
                    ),
                }
            }
            Event::Folders { dir, names } => {
                self.palette.folders.insert(at(dir), names);
            }
            Event::Diffed { key, scope, result } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    match result {
                        Ok(files) => s.git.scoped = Some((scope, files)),
                        Err(why) => {
                            s.git.scoped = Some((scope, Vec::new()));
                            self.trouble(crate::tr("Couldn't read the changes"), Some(why), None);
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
                let ended = |p: &Option<crate::git::Pr>| {
                    p.as_ref()
                        .is_some_and(|p| p.state == "merged" || p.state == "closed")
                };
                let on = self.settings_on(host).settle_merged;
                if let Some(s) = self.sessions.at_mut(key) {
                    // Its pull request merged or closed just now, while it
                    // waits on nothing: the session is done, unless you said
                    // it never is.
                    let done = on
                        && ended(&pr)
                        && !ended(&s.git.pr)
                        && !s.meta.flags.kept
                        && matches!(s.state, State::Idle | State::Closed);
                    s.git.pr = pr;
                    if done && s.meta.flags.settled.is_none() {
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
            // What the settings dialog shows of the server it is on.
            _ if host != self.pages.on && Self::of_dialog(&event) => {}
            Event::Sizes(sizes) => self.pages.sizes = sizes,
            Event::Tidied(said) => self.pages.tidied = Some(said),
            // What `u0 thread` waits for: the window shows the turn as
            // any task's (`Event::TaskTurn`).
            Event::Turned { .. } => {}
            Event::Looked {
                key,
                folder,
                branch,
                repo,
                repos,
                scripts,
            } => self.looked(key, &folder, branch, repo, repos, scripts),
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

            Event::Forges(seen) => self.pages.forges = seen,
            Event::Devices { found, missing } => self.devices_listed(found, missing),
            Event::Screen { id, picture } => self.device_screen(id, picture),
            Event::Held { folder, held } => {
                self.held.insert(at(folder), held);
            }
            Event::Placed { key, starts } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    for (id, starts) in starts {
                        s.placed(&id, starts);
                    }
                }
            }
            Event::Uncommitted { key, result } => {
                if let Some(s) = self.sessions.at_mut(key) {
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
            // The registry is this Mac's: each server installs from it.
            Event::Registry(_) if host != Host::HERE => {}
            Event::Registry(Ok(registry)) => {
                self.registry = registry;
                self.registry_read = Some(std::time::SystemTime::now());
                self.registry_trouble = None;
                // A newer version of an agent replaces its entry; one
                // running keeps running as it is.
                for slot in &mut self.slots {
                    if let Some(e) = self.registry.agents.iter().find(|e| e.id == slot.entry.id) {
                        slot.entry = e.clone();
                    }
                }
                let hosts: Vec<Host> = self.machines.iter().map(|m| m.host).collect();
                for host in hosts {
                    self.fill_slots(host);
                    self.install_due(host, cx);
                    self.look_here(host);
                }
            }
            Event::Registry(Err(why)) => self.registry_trouble = Some(why),
            Event::Installing { agent, what, share } => {
                if let Some(Stage::Installing(i)) =
                    self.slot_mut(host, &agent).map(|s| &mut s.stage)
                {
                    i.what = what;
                    i.share = share;
                }
            }
            Event::Installed { agent, result } => {
                let runs = (self.here_on(host, &agent)).is_some_and(|h| h.missing.is_none());
                let name = self.name_of(&agent);
                if let Some(slot) = self.slot_mut(host, &agent) {
                    match result {
                        // An update that did not come leaves the version
                        // here running, and says why.
                        Err(why) if why != crate::registry::CANCELLED && runs => {
                            slot.stage = Stage::Ready;
                            self.trouble(
                                crate::tr_with("%{name} was not updated", &[("name", &name)]),
                                Some(why),
                                Some(super::notices::Way::Agent(host, agent.clone())),
                            );
                        }
                        Err(why) if why != crate::registry::CANCELLED => {
                            slot.stage = Stage::Failed(why);
                        }
                        _ => slot.stage = Stage::Ready,
                    }
                }
                self.look_here(host);
                self.start_ready(host);
            }
            Event::Started { agent, result } => {
                self.started(host, &agent, result);
                self.install_due(host, cx);
            }
            Event::Opened { key, result } => self.opened(key, result),
            Event::Listed { agent, result } => {
                let settled = self.listed(host, &agent, result);
                self.on_settle(&settled, cx);
            }
            Event::Probed { agent, result } => self.probed(host, &agent, result),
            Event::CliVersion { cli, version } => {
                if let Some(m) = self.machine_mut(host) {
                    m.cli_versions.insert(cli, version);
                }
            }
            Event::Elsewhere { agent, folders } => {
                if let Some(m) = self.machine_mut(host) {
                    m.elsewhere.insert(agent, folders);
                }
            }
            Event::Forked { from, at, result } => self.forked(from, at, result, window, cx),
            Event::Loaded {
                key,
                resumed,
                result,
            } => self.loaded(key, resumed, result, cx),
            Event::Weighed { agents, processes } => {
                let items = crate::pages::process_tree(&processes.procs);
                (self.pages.process_tree).update(cx, |t, cx| t.set_items(items, cx));
                self.pages.processes = (agents, processes);
            }
            Event::Troubles(lines) => self.pages.troubles = lines,
            Event::Telemetry(batch) => self.pages.sent = Some(batch),
            // What the server says is here: each agent restaged, those
            // that can start started, those not here installed.
            Event::Here(here) => {
                let Some(m) = self.machines.iter_mut().find(|m| m.host == host) else {
                    return;
                };
                m.here.extend(here);
                for slot in self.slots.iter_mut().filter(|s| s.host == host) {
                    slot.restage(m.here.get(&slot.entry.id));
                }
                self.install_due(host, cx);
                self.start_ready(host);
            }
            Event::Deleted { key, result } => match result {
                Ok(()) => self.forget_session(key, false, cx),
                Err(why) => {
                    let title = self
                        .sessions
                        .at(key)
                        .map(|s| crate::ui::fit(&s.meta.title, 40))
                        .unwrap_or_default();
                    self.trouble(
                        crate::tr_with("Couldn't delete “%{title}”", &[("title", &title)]),
                        Some(why),
                        None,
                    );
                }
            },
            Event::Ended {
                key,
                result,
                tokens,
            } => {
                // A turn refused for want of a sign-in marks the agent: the
                // composer then offers its sign-in.
                let agent = self.sessions.at(key).map(|s| s.meta.agent.clone());
                if let Some(slot) = agent.and_then(|a| self.slot_mut(host, &a)) {
                    slot.auth = match &result {
                        Err(f) if f.code == crate::acp::ErrorCode::AUTH_REQUIRED => {
                            hub::Auth::Needed(f.message.clone())
                        }
                        Ok(_) => hub::Auth::Ready,
                        Err(_) => slot.auth.clone(),
                    };
                }
                let ok = result.is_ok();
                // A turn a limit stopped goes on as it resets, where the
                // agent says when.
                let resets = (result.as_ref().err())
                    .and_then(|f| crate::usage::resets(f, std::time::SystemTime::now()))
                    .filter(|_| {
                        let project = self.sessions.at(key).map(super::Place::project_of);
                        self.settings_in(project.as_ref()).resume_at_reset
                    });
                if let Some(s) = self.sessions.at_mut(key) {
                    s.ended(result.map_err(|f| f.message), tokens);
                }
                if let Some(at) = resets {
                    self.retry_at(key, at, cx);
                }
                self.stopping = None;
                self.end_turn(key);
                self.turn_over(key, ok, cx);
            }
            Event::Tasks(list) => {
                self.tasks.list = list;
                cx.notify();
            }
            // Changed there by one of u0's tools: taken as they are.
            Event::Settings(settings) => {
                *self.settings_on_mut(host) = *settings;
                if host == Host::HERE {
                    self.settings_took(cx);
                }
                cx.notify();
            }
            Event::Workflows { flows, runs } => {
                (self.flows.list, self.flows.runs) = (flows, runs);
                cx.notify();
            }
            Event::TaskTurn {
                meta,
                record,
                ended,
            } => self.task_turn(host, &meta, record, ended, cx),
            Event::Organized(meta) => self.organized(host, &meta, cx),
            // Answered by one of u0's tools, as you allowed.
            Event::Answered {
                agent,
                session,
                request,
                option,
            } => {
                if let Some(s) = self.sessions.by_id(host, &agent, &session) {
                    s.answered(request, &option);
                    cx.notify();
                }
            }
            Event::Written { key, ask, result } => self.written(key, ask, result, window, cx),
            Event::LoggedOut { agent, result } => {
                if let Some(slot) = self.slot_mut(host, &agent) {
                    match result {
                        Ok(()) => slot.auth = hub::Auth::Needed(crate::tr("Signed out").into()),
                        Err(why) => tracing::warn!("{agent} could not sign out: {why}"),
                    }
                }
            }
            Event::Said { agent, event } => self.said(host, &agent, *event, window, cx),
            Event::Appended { agent, id, events } => {
                if let Some(slot) = self.slot_mut(host, &agent) {
                    slot.used = std::time::Instant::now();
                }
                if let Some(key) = self.sessions.appended(host, &agent, &id, events) {
                    self.place(key);
                }
            }
            Event::Signed { agent, result } => self.signed(host, &agent, result),
            Event::Options { key, options } => {
                if let Some(s) = self.sessions.at_mut(key) {
                    s.options = options;
                }
            }
            Event::Index(index) => self.keep_from(host, index),
            Event::Offered { link, made, grant } => {
                self.pages.offered = Some((link, made, grant));
            }
            Event::Shared(shared) => {
                self.pages.shared = Some(*shared);
                // A server here may have paired back with a Mac.
                self.read_servers();
            }
            Event::Record { key, events } => self.record_read(key, events),
            Event::Used(used) => self.used.extend(used),
            // Each server finds in its own sessions.
            Event::Found { query, keys } => {
                if self.search.input.read(cx).value().trim().to_lowercase() == query {
                    self.search.found.extend(keys);
                }
            }
            Event::Refused { key, why } => {
                let name = self
                    .sessions
                    .at(key)
                    .map(|s| self.name_of(&s.meta.agent))
                    .unwrap_or_default();
                self.trouble(
                    crate::tr_with("%{name} kept it as it was", &[("name", &name)]),
                    Some(why),
                    None,
                );
            }
        }
    }

    /// A turn in the session `key` is over (`ok`, or it failed): it is
    /// unread if you looked elsewhere, you are told as you asked, and what
    /// was said meanwhile goes now.
    pub(super) fn turn_over(&mut self, key: u64, ok: bool, cx: &mut Context<Self>) {
        let away = self.current != Some(key);
        let behind = self.behind;
        // A turn that ended while you looked elsewhere is unread.
        if let Some(s) = self.sessions.at_mut(key).filter(|_| away || behind) {
            s.meta.flags.unread = true;
            self.save_index();
        }
        // With u0 behind other windows, macOS says so; in front, with
        // another session on screen, a notice leads to it; each as you
        // asked for it.
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
                    &crate::tr(if ok { "Finished its turn" } else { "Stopped" }),
                );
            } else if away && s.parent.is_none() {
                self.done(
                    crate::tr_with(
                        if ok {
                            "%{name} finished “%{title}”"
                        } else {
                            "%{name} stopped in “%{title}”"
                        },
                        &[("name", &name), ("title", &title)],
                    ),
                    Some(super::notices::Way::Show(key)),
                );
            }
        }
        if ok {
            self.suggest_next(key);
        }
        self.refresh_git(key);
        // What was said meanwhile goes now, in order.
        self.say_queued(key, cx);
    }

    /// Whether `event` is of what the settings dialog shows of the server
    /// it is on: another server's is not shown.
    fn of_dialog(event: &Event) -> bool {
        matches!(
            event,
            Event::Sizes(_)
                | Event::Tidied(_)
                | Event::Forges(_)
                | Event::Weighed { .. }
                | Event::Troubles(_)
                | Event::Telemetry(_)
                | Event::Shared(_)
                | Event::Offered { .. }
                | Event::Tasks(_)
                | Event::Workflows { .. }
        )
    }

    pub(super) fn started(
        &mut self,
        host: Host,
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
                if let Some(slot) = self.slot_mut(host, agent) {
                    slot.known = Some(init);
                    slot.stage = Stage::Running { pid };
                    slot.used = std::time::Instant::now();
                } else {
                    // Switched off while it started.
                    self.call(
                        host,
                        Call::Stop {
                            agent: agent.to_owned(),
                        },
                    );
                }
                for key in self.sessions.started(host, agent, None) {
                    self.open_when_running(host, agent, key);
                }
                // What waited for it to start, now.
                let (now, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting)
                    .into_iter()
                    .partition(|(h, a, _)| *h == host && a == agent);
                self.waiting = wait;
                for (.., w) in now {
                    self.do_waiting(w);
                }
                self.list_sessions(host, agent);
                self.probe_agent(host, agent);
            }
            Err(f) => {
                if let Some(slot) = self.slot_mut(host, agent) {
                    slot.stage = Stage::Failed(f.message.clone());
                }
                self.sessions.started(host, agent, Some(&f.message));
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
                let (host, agent) = (s.host, s.meta.agent.clone());
                let id = r.session_id.clone();
                if self.slot(host, &agent).is_some_and(Slot::running) {
                    if let Some(m) = mode {
                        let call = Call::SetMode {
                            agent: agent.clone(),
                            key,
                            session: id.clone(),
                            mode: m,
                        };
                        self.call(host, call);
                    }
                    for (config, value) in chosen {
                        let call = Call::SetOption {
                            agent: agent.clone(),
                            key,
                            session: id.clone(),
                            config,
                            value,
                        };
                        self.call(host, call);
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
                let (host, agent) = (s.host, s.meta.agent.clone());
                if let Some(slot) = self.slot_mut(host, &agent) {
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

    /// What `agent` on `host` listed: each session not on the desk yet is
    /// put there, closed; one closed already takes the agent's title and
    /// time. Those it settled for being left come back.
    pub(super) fn listed(
        &mut self,
        host: Host,
        agent: &str,
        result: Result<
            Vec<(
                std::path::PathBuf,
                agent_client_protocol::v1::schema::SessionInfo,
            )>,
            String,
        >,
    ) -> Vec<u64> {
        let found = match result {
            Ok(found) => found,
            Err(why) => {
                tracing::warn!("{agent} could not list its sessions: {why}");
                return Vec::new();
            }
        };
        let gone = self
            .machine(host)
            .map(|m| m.gone.clone())
            .unwrap_or_default();
        self.sessions.listed(host, agent, found, &gone);
        let settled = self.settle_idle();
        self.save_index();
        settled
    }

    pub(super) fn probed(
        &mut self,
        host: Host,
        agent: &str,
        result: Result<
            Box<agent_client_protocol::v1::schema::NewSessionResponse>,
            crate::acp::Error,
        >,
    ) {
        let Some(at) = (self.slots.iter()).position(|s| s.host == host && s.entry.id == agent)
        else {
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
                for s in
                    self.sessions.all.iter_mut().filter(|s| {
                        s.of(host, agent) && s.meta.id.is_none() && s.options.is_empty()
                    })
                {
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
        cx: &mut Context<Self>,
    ) {
        match self.sessions.loaded(key, resumed, result) {
            Loaded::Open(said) => {
                // What you said goes in its record before what the agent
                // says to it.
                self.flush(key);
                if let Some(text) = said {
                    self.begin_turn(key, text);
                }
                self.to_bottom(cx);
                self.save_index();
            }
            Loaded::Refused => {
                self.drop_record(key);
                if let Some((host, agent)) =
                    (self.sessions.at(key)).map(|s| (s.host, s.meta.agent.clone()))
                {
                    self.open_when_running(host, &agent, key);
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
            self.tell(
                key,
                format!("{name} · {title}"),
                &crate::tr("Asks before it goes on"),
            );
        } else if self.current != Some(key) {
            self.done(
                crate::tr_with(
                    "%{name} asks before it goes on in “%{title}”",
                    &[("name", &name), ("title", &title)],
                ),
                Some(super::notices::Way::Asks(key)),
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
        host: Host,
        agent: &str,
        event: acp::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(slot) = self.slot_mut(host, agent) {
            slot.used = std::time::Instant::now();
        }
        match event {
            acp::Event::Task { session, task } => {
                if let Some(s) = self.sessions.by_id(host, agent, &SessionId::new(session)) {
                    s.task(task);
                }
            }
            // Said in no session open with it: the probe's, which lists the
            // commands the agent takes.
            acp::Event::Update(n) => {
                if let SessionUpdate::AvailableCommandsUpdate(c) = n.update
                    && let Some(slot) = self.slot_mut(host, agent)
                {
                    slot.commands = c.available_commands;
                }
            }
            acp::Event::Permission { id, request } => {
                if let Some(s) = self.sessions.by_id(host, agent, &request.session_id) {
                    let key = s.key;
                    s.asked(id, request);
                    self.asks_you(agent, key);
                } else if self.slot(host, agent).is_some_and(Slot::running) {
                    self.call(
                        host,
                        Call::Answer {
                            agent: agent.to_owned(),
                            request: id,
                            answer: acp::Answer::cancelled(false),
                        },
                    );
                }
            }
            acp::Event::Elicit { id, request } => {
                let session = match request.scope() {
                    ElicitationScope::Session(s) => Some(s.session_id.clone()),
                    _ => None,
                };
                let key = session
                    .and_then(|sid| self.sessions.by_id(host, agent, &sid))
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
                    None => self.call(
                        host,
                        Call::Answer {
                            agent: agent.to_owned(),
                            request: id,
                            answer: acp::Answer::cancelled(true),
                        },
                    ),
                }
            }
            acp::Event::Exited { pid, why } => {
                // One let go while idle ended as it was meant to, and one
                // let go before this one started is not this one's end:
                // one that stops as it starts says so in `Started`.
                let Some(slot) = self.slot_mut(host, agent).filter(|s| s.runs(pid)) else {
                    return;
                };
                slot.stage = Stage::Failed(why.clone());
                self.call(
                    host,
                    Call::Stop {
                        agent: agent.to_owned(),
                    },
                );
                for key in self.sessions.exited(host, agent, &why) {
                    self.flush(key);
                }
            }
        }
    }
}
