//! The sessions on the desk, each by the desk's key for it and found by
//! the id its agent gave it. Nothing here needs a window.

use std::path::{Path, PathBuf};

use agent_client_protocol::{
    schema::MaybeUndefined,
    v1::schema::{SessionId, SessionInfo, SessionNotification, SessionUpdate},
};

use crate::{
    acp,
    hub::{Opened, Probe},
    session::{Item, Session, State},
    store::{self, Job},
};

#[derive(Default)]
pub struct Sessions {
    /// Every session on the desk, in the order each came.
    pub all: Vec<Session>,
    /// The last key taken.
    last_key: u64,
}

impl Sessions {
    /// A key nothing on the desk has had: a session's, a terminal's or a
    /// notice's.
    pub fn take_key(&mut self) -> u64 {
        self.last_key += 1;
        self.last_key
    }

    pub fn at(&self, key: u64) -> Option<&Session> {
        self.all.iter().find(|s| s.key == key)
    }

    pub fn at_mut(&mut self, key: u64) -> Option<&mut Session> {
        self.all.iter_mut().find(|s| s.key == key)
    }

    /// The session `agent` knows as `id`.
    pub fn by_id(&mut self, agent: &str, id: &SessionId) -> Option<&mut Session> {
        self.all
            .iter_mut()
            .find(|s| s.meta.agent == agent && s.meta.id.as_ref() == Some(id))
    }

    /// The subagent `id` the session `parent` set to work: a session of
    /// its own, under it.
    pub fn add_subagent(&mut self, parent: u64, id: SessionId, title: String, state: State) {
        let key = self.take_key();
        let Some(p) = self.at(parent) else {
            return;
        };
        let mut child = Session::new(key, &p.meta.agent, p.meta.folder.clone());
        child.meta.project.clone_from(&p.meta.project);
        (child.branch, child.repo) = (p.branch.clone(), p.repo);
        child.scripts.clone_from(&p.scripts);
        child.meta.id = Some(id);
        child.parent = Some(parent);
        child.meta.title = title;
        child.meta.renamed = true;
        child.state = state;
        self.all.push(child);
    }

    /// The sessions of `agent` that waited to be signed in, opening again:
    /// their keys, to be opened.
    pub fn signed_in(&mut self, agent: &str) -> Vec<u64> {
        let waiting = self
            .all
            .iter_mut()
            .filter(|s| s.meta.agent == agent && s.state == State::SignIn);
        waiting
            .map(|s| {
                s.state = State::Opening;
                s.key
            })
            .collect()
    }

    /// `agent` started, or could not (why): the keys of its sessions that
    /// waited for it to open, to be opened now; or, if it could not, each
    /// is broken with why.
    pub fn started(&mut self, agent: &str, failed: Option<&str>) -> Vec<u64> {
        let waiting = self
            .all
            .iter_mut()
            .filter(|s| s.meta.agent == agent && s.state == State::Opening && !s.needs_worktree());
        match failed {
            None => waiting.map(|s| s.key).collect(),
            Some(why) => {
                for s in waiting {
                    s.state = State::Broken(why.to_owned());
                }
                Vec::new()
            }
        }
    }

    /// What `agent` listed, by project: each session not on the desk yet
    /// (nor deleted, `gone`) is put there, closed; one closed already takes
    /// the agent's title and time.
    pub fn listed(
        &mut self,
        agent: &str,
        found: Vec<(PathBuf, SessionInfo)>,
        gone: &[(String, String)],
    ) {
        for (project, info) in found {
            if gone
                .iter()
                .any(|(a, id)| a == agent && *id == *info.session_id.0)
            {
                continue;
            }
            if let Some(s) = self.by_id(agent, &info.session_id) {
                if s.state == State::Closed {
                    if let Some(t) = info
                        .title
                        .filter(|t| !t.trim().is_empty() && !s.meta.renamed)
                    {
                        s.meta.title = t;
                    }
                    if let Some(t) = info.updated_at.as_deref().and_then(crate::when::parse) {
                        s.meta.updated = t;
                    }
                }
                continue;
            }
            let key = self.take_key();
            self.all.push(Session::listed(key, agent, info, project));
        }
    }

    /// An update `agent` sent: the key of the session it went to, and
    /// whether it named it; or, when it went to none of them (the probe's
    /// session), the update. A subagent set to work becomes a session of
    /// its own, under its parent.
    pub fn update(
        &mut self,
        agent: &str,
        n: SessionNotification,
    ) -> Result<(u64, bool), Box<SessionUpdate>> {
        if let SessionUpdate::SubagentUpdate(u) = &n.update
            && self.by_id(agent, &u.session_id).is_none()
            && let Some(parent) = self.by_id(agent, &n.session_id)
        {
            let up = parent.key;
            let title = match &u.title {
                MaybeUndefined::Value(t) => t.clone(),
                _ => "A subagent".into(),
            };
            self.add_subagent(up, u.session_id.clone(), title, State::Idle);
        }
        // What it is doing now, for its row, the column and its tab.
        if let SessionUpdate::SubagentUpdate(u) = &n.update
            && let Some(child) = self.by_id(agent, &u.session_id)
        {
            child.sub.take(u);
        }
        let Some(s) = self.by_id(agent, &n.session_id) else {
            return Err(Box::new(n.update));
        };
        let titled = matches!(n.update, SessionUpdate::SessionInfoUpdate(_));
        s.apply(n.update);
        Ok((s.key, titled))
    }

    /// The process of `agent` stopped, why: each of its sessions open is
    /// closed, to be loaded when opened again, and one opening is broken.
    pub fn exited(&mut self, agent: &str, why: &str) {
        for s in self.all.iter_mut().filter(|s| s.meta.agent == agent) {
            let open = !matches!(s.state, State::Draft | State::Closed);
            if s.meta.id.is_some() && open {
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

    /// The session `key` loaded or resumed (`resumed`), or not; its record
    /// is in `home`.
    pub fn loaded(
        &mut self,
        key: u64,
        resumed: bool,
        result: Result<Opened, acp::Error>,
        home: &Path,
    ) -> Loaded {
        let Some(s) = self.at_mut(key) else {
            return Loaded::Stays;
        };
        match result {
            Ok(r) => {
                s.modes = r.modes;
                s.options = r.options.unwrap_or_default();
                s.state = State::Idle;
                let pending = s.pending.take();
                if let Some(text) = &pending {
                    s.said(text);
                }
                Loaded::Open(pending)
            }
            Err(f) if f.code == acp::ErrorCode::AUTH_REQUIRED => {
                s.state = State::SignIn;
                Loaded::Stays
            }
            // The agent would not resume it: it is loaded, said again.
            Err(_) if resumed => {
                self.drop_record(key, home);
                Loaded::Refused
            }
            Err(f) => {
                s.state = State::Broken(f.message);
                Loaded::Stays
            }
        }
    }

    /// The record of the closed session `key`, read back (`None`: it has
    /// none); `probe`, what a session of its agent offers. One cut short
    /// is dropped from `home`: the agent has the rest.
    pub fn read_back(
        &mut self,
        key: u64,
        events: Option<Vec<store::Event>>,
        probe: Option<&Probe>,
        home: &Path,
    ) -> Read {
        let Some(s) = self.at(key).filter(|s| s.state == State::Closed) else {
            return Read::Left;
        };
        // A subagent has no other copy; any other is loaded from its agent.
        let cut = |e: &[store::Event]| s.parent.is_none() && store::cut_short(e);
        let Some(events) = events.filter(|e| !e.is_empty()) else {
            return Read::Unrecorded;
        };
        if cut(&events) {
            self.drop_record(key, home);
            return Read::Unrecorded;
        }
        let s = self.at_mut(key).expect("found above");
        s.forget();
        s.replay(events);
        s.recorded = true;
        if s.options.is_empty()
            && let Some(p) = probe
        {
            s.modes.clone_from(&p.modes);
            s.options.clone_from(&p.options);
        }
        Read::Replayed
    }

    /// Drop the record of the session `key` from `home`: it is loaded from
    /// its agent again, and written afresh as it comes.
    pub fn drop_record(&mut self, key: u64, home: &Path) {
        if let Some(s) = self.at_mut(key) {
            if let Some(id) = &s.meta.id {
                store::queue(Job::Remove(store::record_path(home, &s.meta.agent, &id.0)));
            }
            s.forget();
            s.log.clear();
            s.recorded = false;
        }
    }
}

/// How a load or resume of a session came back: what the desk does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    /// It is open: say what you said while it opened, if anything.
    Open(Option<String>),
    /// Its agent would not resume it: its record is dropped, and it is to
    /// be opened again, which loads it.
    Refused,
    /// Nothing more: it waits to be signed in, or is broken.
    Stays,
}

/// A session's record read back: what the desk does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// Its thread is built from the record.
    Replayed,
    /// It has no record to go by: it is opened from its agent.
    Unrecorded,
    /// It was opened meanwhile, or is gone: left as it is.
    Left,
}
