//! The record of each session open with its agent, written by the server
//! as it comes (see `store`).
//!
//! A record is made in two places, each part once, where it happens: what
//! the agent says (each update, and when its message began) here, as it
//! arrives; what the desk says (what you said, where a turn ended, a note)
//! on the desk, which sends it to be written. Here each open session is
//! held as a [`Session`], built by the same reducer the desk builds its
//! thread with, so that what is made here follows what the desk has.

use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use agent_client_protocol::v1::schema::{SessionId, SessionNotification, SessionUpdate};

use crate::{
    session::Session,
    store::{self, Event, Job},
};

pub struct Records {
    home: PathBuf,
    /// The sessions open with their agents, by agent and id.
    open: Mutex<HashMap<(String, SessionId), Session>>,
}

impl Records {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            open: Mutex::default(),
        }
    }

    /// The session `id` of `agent` is open with it: what it says in it is
    /// written from now on.
    pub fn open(&self, agent: &str, id: SessionId) {
        let thread = Session::new(0, agent, PathBuf::new());
        (self.open.lock().expect("records")).insert((agent.to_owned(), id), thread);
    }

    /// The sessions of `agent` are open with it no more: it stopped.
    pub fn close(&self, agent: &str) {
        (self.open.lock().expect("records")).retain(|(a, _), _| a != agent);
    }

    /// The session `id` of `agent` is gone, and its record.
    pub fn forget(&self, agent: &str, id: &SessionId) {
        (self.open.lock().expect("records")).remove(&(agent.to_owned(), id.clone()));
        store::queue(Job::Remove(self.path(agent, id)));
    }

    /// What `agent` said in one of its sessions: the lines of its record
    /// that makes, written; a subagent it set to work is open from then
    /// on. One said in a session not open here (the one it was opened in
    /// to be read) is handed back.
    pub fn said(
        &self,
        agent: &str,
        n: SessionNotification,
    ) -> Result<(SessionId, Vec<Event>), Box<SessionNotification>> {
        let mut open = self.open.lock().expect("records");
        let at = (agent.to_owned(), n.session_id.clone());
        let Some(thread) = open.get_mut(&at) else {
            return Err(Box::new(n));
        };
        let child = match &n.update {
            SessionUpdate::SubagentUpdate(u) => Some(u.session_id.clone()),
            _ => None,
        };
        thread.apply(n.update);
        let events = std::mem::take(&mut thread.log);
        if let Some(child) = child {
            (open.entry((agent.to_owned(), child)))
                .or_insert_with(|| Session::new(0, agent, PathBuf::new()));
        }
        self.write(agent, &at.1, events.clone());
        Ok((at.1, events))
    }

    /// Lines the desk made of the session `id` of `agent`, written after
    /// those before them.
    pub fn append(&self, agent: &str, id: &SessionId, events: Vec<Event>) {
        let mut open = self.open.lock().expect("records");
        if let Some(thread) = open.get_mut(&(agent.to_owned(), id.clone())) {
            thread.replay(events.clone());
        }
        self.write(agent, id, events);
    }

    fn write(&self, agent: &str, id: &SessionId, events: Vec<Event>) {
        if !events.is_empty() {
            store::queue(Job::Append(self.path(agent, id), events));
        }
    }

    /// Where the record of the session `id` of `agent` is.
    pub fn path(&self, agent: &str, id: &SessionId) -> PathBuf {
        store::record_path(&self.home, agent, &id.0)
    }
}
