//! What comes back to the window: signing in, and every message from the
//! agents and the work done off the window's thread.

use agent_client_protocol_schema::v1::SessionUpdate;
use gpui::Context;

use super::Desk;
use crate::{
    acp::Event,
    hub::{self, Msg, Slot, Stage},
    session::{Item, Session, State},
};

impl Desk {
    /// Sign in to `agent` with its method `method`.
    pub fn sign_in(&mut self, agent: &str, method: &str, cx: &mut Context<Self>) {
        use agent_client_protocol_schema::v1::AuthMethod;
        let Some(slot) = self.slot(agent) else { return };
        let Stage::Running {
            agent: running,
            init,
        } = &slot.stage
        else {
            return;
        };
        let terminal = init.auth_methods.iter().find_map(|m| match m {
            AuthMethod::Terminal(t) if &*t.id.0 == method => Some(t.clone()),
            _ => None,
        });
        let running = running.clone();
        let launch = slot.entry.launch(&self.home);
        if let Some(t) = terminal {
            // A login typed in a terminal: once it is done there, opening
            // the session is tried again from its card.
            if let Ok(mut launch) = launch {
                launch.args.extend(t.args);
                launch.env.extend(t.env);
                if let Err(e) = hub::in_terminal(&launch.program, &launch.args, &launch.env) {
                    self.note_sessions(agent, &format!("A terminal could not be opened: {e}"));
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
        let keys: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| s.agent == agent && s.state == State::SignIn)
            .map(|s| s.key)
            .collect();
        for key in keys {
            if let Some(s) = self.session_mut(key) {
                s.state = State::Opening;
            }
            self.open_when_running(agent, key);
        }
        cx.notify();
    }

    pub(super) fn signed(&mut self, agent: &str, result: Result<(), String>) {
        self.signing = None;
        match result {
            Ok(()) => {
                let keys: Vec<u64> = self
                    .sessions
                    .iter()
                    .filter(|s| s.agent == agent && s.state == State::SignIn)
                    .map(|s| s.key)
                    .collect();
                for key in keys {
                    if let Some(s) = self.session_mut(key) {
                        s.state = State::Opening;
                    }
                    self.open_when_running(agent, key);
                }
            }
            Err(why) => self.note_sessions(agent, &format!("Signing in failed: {why}")),
        }
    }

    /// Put a line of trouble in each session of `agent` waiting to sign in.
    pub(super) fn note_sessions(&mut self, agent: &str, text: &str) {
        for s in self
            .sessions
            .iter_mut()
            .filter(|s| s.agent == agent && s.state == State::SignIn)
        {
            s.items.push(Item::Note {
                text: text.to_owned(),
                trouble: true,
            });
        }
    }

    // What comes back.

    pub(super) fn take(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Registry(Ok(registry)) => {
                self.registry = registry;
                self.registry_read = Some(std::time::SystemTime::now());
                self.registry_trouble = None;
                // A newer version of an agent replaces its entry; one
                // running keeps running as it is.
                for slot in &mut self.slots {
                    if let Some(e) = self.registry.agents.iter().find(|e| e.id == slot.entry.id) {
                        let stage = match &slot.stage {
                            Stage::Missing(_) | Stage::Ready | Stage::Failed(_) => {
                                Slot::new(e.clone(), &self.home).stage
                            }
                            other => other.clone(),
                        };
                        *slot = Slot {
                            entry: e.clone(),
                            stage,
                        };
                    }
                }
                self.fill_slots();
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
                if let Some(slot) = self.slot_mut(&agent) {
                    slot.stage = match result {
                        Err(why) if why != crate::registry::CANCELLED => Stage::Failed(why),
                        _ => Slot::new(slot.entry.clone(), &home).stage,
                    };
                }
                self.start_ready();
            }
            Msg::Started { agent, result } => self.started(&agent, result),
            Msg::Opened { key, result } => self.opened(key, result),
            Msg::Listed { agent, result } => self.listed(&agent, result),
            Msg::Loaded { key, result } => self.loaded(key, result),
            Msg::Uncommitted { key, result } => {
                if let Some(s) = self.session_mut(key)
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
                    if let Some(s) = self.session_mut(key) {
                        s.items.push(Item::Note {
                            text: format!("It could not be deleted: {why}"),
                            trouble: true,
                        });
                    }
                }
            },
            Msg::Ended { key, result } => {
                if let Some(s) = self.session_mut(key) {
                    s.ended(result);
                }
                self.end_turn(key);
            }
            Msg::Worktree { key, result } => self.worktree_made(key, result),
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
                if let Some(s) = self.session_mut(key) {
                    s.options = options;
                }
            }
            Msg::Refused { key, why } => {
                if let Some(s) = self.session_mut(key) {
                    s.items.push(Item::Note {
                        text: why,
                        trouble: true,
                    });
                }
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
                Box<agent_client_protocol_schema::v1::InitializeResponse>,
            ),
            crate::acp::Failure,
        >,
    ) {
        let waiting: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| s.agent == agent && s.state == State::Opening && s.worktree_ready())
            .map(|s| s.key)
            .collect();
        match result {
            Ok((running, init)) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Running {
                        agent: running,
                        init,
                    };
                }
                for key in waiting {
                    self.open_when_running(agent, key);
                }
                self.list_sessions(agent);
            }
            Err(f) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Failed(f.message.clone());
                }
                for key in waiting {
                    if let Some(s) = self.session_mut(key) {
                        s.state = State::Broken(f.message.clone());
                    }
                }
            }
        }
    }

    pub(super) fn opened(
        &mut self,
        key: u64,
        result: Result<
            Box<agent_client_protocol_schema::v1::NewSessionResponse>,
            crate::acp::Failure,
        >,
    ) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        match result {
            Ok(r) => {
                s.id = Some(r.session_id.clone());
                s.modes = r.modes;
                s.options = r.config_options.unwrap_or_default();
                s.state = State::Idle;
                if let Some(text) = s.pending.take() {
                    s.state = State::Working;
                    self.begin_turn(key, text);
                }
            }
            // The text waits: once signed in, the session opens and it goes.
            Err(f) if f.code == Some(crate::acp::AUTH_REQUIRED) => s.state = State::SignIn,
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
                agent_client_protocol_schema::v1::SessionInfo,
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
        for (project, info) in found {
            if let Some(s) = self
                .sessions
                .iter_mut()
                .find(|s| s.agent == agent && s.id.as_ref() == Some(&info.session_id))
            {
                if s.state == State::Closed {
                    if let Some(t) = info.title.filter(|t| !t.trim().is_empty()) {
                        s.title = t;
                    }
                    if let Some(t) = info.updated_at.as_deref().and_then(crate::when::parse) {
                        s.updated = t;
                    }
                }
                continue;
            }
            let key = self.take_key();
            self.sessions
                .push(Session::listed(key, agent, info, project));
        }
    }

    pub(super) fn loaded(
        &mut self,
        key: u64,
        result: Result<
            Box<agent_client_protocol_schema::v1::LoadSessionResponse>,
            crate::acp::Failure,
        >,
    ) {
        let under = self.home.join("worktrees");
        let tx = self.tx.clone();
        let Some(s) = self.session_mut(key) else {
            return;
        };
        match result {
            Ok(r) => {
                s.modes = r.modes;
                s.options = r.config_options.unwrap_or_default();
                s.state = State::Idle;
                s.look();
                // A session in one of Ghost's worktrees works there again.
                if s.worktree.is_none()
                    && s.folder.starts_with(&under)
                    && let Some(branch) = s.branch.clone()
                {
                    s.worktree = Some(crate::git::Worktree {
                        repo: s.project.clone(),
                        path: s.folder.clone(),
                        branch,
                    });
                }
                if s.repo {
                    hub::uncommitted(s.folder.clone(), key, tx);
                }
                if let Some(text) = s.pending.take() {
                    s.said(&text);
                    self.begin_turn(key, text);
                }
                self.thread_scroll.scroll_to_bottom();
            }
            Err(f) if f.code == Some(crate::acp::AUTH_REQUIRED) => s.state = State::SignIn,
            Err(f) => s.state = State::Broken(f.message),
        }
    }

    pub(super) fn said(&mut self, agent: &str, event: Event) {
        match event {
            Event::Update(n) => {
                let Some(s) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.agent == agent && s.id.as_ref() == Some(&n.session_id))
                else {
                    return;
                };
                let title = matches!(n.update, SessionUpdate::SessionInfoUpdate(_));
                s.apply(n.update);
                if !title {
                    self.thread_scroll_follow();
                }
            }
            Event::Permission { id, request } => {
                if let Some(s) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.agent == agent && s.id.as_ref() == Some(&request.session_id))
                {
                    s.asked(id, request);
                    self.thread_scroll_follow();
                } else if let Some(running) = self.slot(agent).and_then(Slot::running) {
                    hub::answer(running, &id, None);
                }
            }
            Event::Exited(why) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Failed(why.clone());
                }
                for s in self.sessions.iter_mut().filter(|s| s.agent == agent) {
                    let open = !matches!(s.state, State::Draft | State::Closed);
                    if s.id.is_some() && open {
                        // The agent keeps it: opened again, it is loaded.
                        s.items.push(Item::Note {
                            text: format!("The agent stopped: {why}"),
                            trouble: true,
                        });
                        s.pending = None;
                        s.since = None;
                        s.state = State::Closed;
                    } else if s.state == State::Opening {
                        s.pending = None;
                        s.since = None;
                        s.state = State::Broken(format!("The agent stopped: {why}"));
                    }
                }
            }
        }
    }
}
