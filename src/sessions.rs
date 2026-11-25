//! The sessions on the desk, each by the desk's key for it and found by
//! the id its agent gave it. Nothing here needs a window.

use agent_client_protocol::v1::schema::SessionId;

use crate::session::{Session, State};

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
}
