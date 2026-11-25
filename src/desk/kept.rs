//! What the desk keeps of its sessions between runs: the index read as the
//! window opens, each session's record written as it goes and read back
//! when it is opened. See `store`.

use gpui::Context;

use super::Desk;
use crate::{
    session::{Session, State},
    store::{self, Index, Job},
};

impl Desk {
    /// Put the sessions kept from an earlier run on the desk, closed.
    pub(super) fn keep_from(&mut self, index: Result<Index, String>) {
        match index {
            Ok(index) => {
                self.gone = index.gone;
                self.stashed = index.stashed;
                for meta in index.sessions {
                    let key = self.take_key();
                    self.sessions.push(Session::stored(key, meta));
                    self.subagents_back(key);
                }
            }
            Err(why) => {
                tracing::warn!("{why}");
                self.store_refused = Some(why);
            }
        }
    }

    /// The subagents the session `key` set to work that are not on the desk:
    /// closed, under it, read from their records when opened.
    fn subagents_back(&mut self, key: u64) {
        let Some(s) = self.session_at(key) else {
            return;
        };
        let missing: Vec<(String, String)> = s
            .subagents
            .iter()
            .filter(|(id, _)| {
                !self
                    .sessions
                    .iter()
                    .any(|c| c.agent == s.agent && c.id.as_ref().is_some_and(|i| *i.0 == **id))
            })
            .cloned()
            .collect();
        for (id, title) in missing {
            let id = agent_client_protocol::v1::schema::SessionId::new(id);
            self.add_subagent(key, id, title, crate::session::State::Closed);
        }
    }

    /// The subagent `id` the session `parent` set to work: a session of
    /// its own, under it.
    pub(crate) fn add_subagent(
        &mut self,
        parent: u64,
        id: agent_client_protocol::v1::schema::SessionId,
        title: String,
        state: crate::session::State,
    ) {
        let key = self.take_key();
        let Some(p) = self.session_at(parent) else {
            return;
        };
        let mut child = Session::new(key, &p.agent, p.folder.clone());
        child.project.clone_from(&p.project);
        (child.branch, child.repo) = (p.branch.clone(), p.repo);
        child.id = Some(id);
        child.parent = Some(parent);
        child.title = title;
        child.renamed = true;
        child.state = state;
        self.sessions.push(child);
    }

    /// A new session of the agent of the session `of`, in its project and
    /// worktree, not yet begun.
    pub(crate) fn beside(&mut self, of: u64) -> Option<Session> {
        let key = self.take_key();
        let o = self.session_at(of)?;
        let mut s = Session::new(key, &o.agent, o.folder.clone());
        s.project.clone_from(&o.project);
        s.worktree.clone_from(&o.worktree);
        (s.branch, s.repo) = (o.branch.clone(), o.repo);
        if let Some(slot) = self.slot(&s.agent) {
            slot.seed(&mut s);
        }
        Some(s)
    }

    /// The session `old` put away, and `new` in its place in the lists.
    pub(crate) fn replace(&mut self, old: u64, new: &mut Session) {
        if let Some(o) = self.session_mut(old) {
            new.title.clone_from(&o.title);
            new.flags.order = o.flags.order;
            new.flags.pinned = o.flags.pinned.take();
            o.flags.archived = true;
        }
    }

    /// Write the index of the sessions the agents have, off the window's
    /// thread.
    pub(crate) fn save_index(&self) {
        if self.store_refused.is_some() {
            return;
        }
        let index = Index {
            sessions: self.sessions.iter().filter_map(Session::meta).collect(),
            gone: self.gone.clone(),
            stashed: self.stashed.clone(),
        };
        store::queue(Job::Write(
            store::index_path(&self.home),
            store::bytes(&index),
        ));
    }

    /// Write what happened in the session `key` since its record was last
    /// written; a session its agent has not named yet keeps it until then.
    pub(crate) fn flush(&mut self, key: u64) {
        let home = self.home.clone();
        let Some(s) = self.session_mut(key) else {
            return;
        };
        let Some(id) = s.id.clone() else { return };
        if s.log.is_empty() {
            return;
        }
        let events = std::mem::take(&mut s.log);
        s.recorded = true;
        store::queue(Job::Append(
            store::record_path(&home, &s.agent, &id.0),
            events,
        ));
    }

    /// Write every record and the index, and wait until they are written:
    /// as the window closes.
    pub(crate) fn keep_all(&mut self) {
        // A turn going on as Ghost closes may go on in the agent; the
        // record says where Ghost lost sight of it.
        for s in &mut self.sessions {
            if matches!(s.state, crate::session::State::Working) {
                s.note("Ghost closed while this turn was going on; the agent may have gone on with it.");
            }
        }
        let keys: Vec<u64> = self.sessions.iter().map(|s| s.key).collect();
        for key in keys {
            self.flush(key);
        }
        // Ports brought here go with Ghost.
        for (_, tunnel) in &mut self.tunnels {
            let _ = tunnel.kill();
        }
        self.save_index();
        store::settle();
    }

    /// Read back the record of the closed session `key`, off the window's
    /// thread (see `record_read`).
    pub(crate) fn read_record(&mut self, key: u64) {
        let (home, tx) = (self.home.clone(), self.tx.clone());
        let Some(s) = self.session_mut(key) else {
            return;
        };
        match &s.id {
            Some(id) => {
                let path = store::record_path(&home, &s.agent, &id.0);
                // Taken to have one until it is read, so it is read once.
                s.recorded = true;
                crate::hub::read_record(path, key, tx);
            }
            None => self.open_unrecorded(key),
        }
    }

    /// The record of the session `key`, read back: the thread shows at once,
    /// and the agent is asked only when you write. A session without one
    /// comes from its agent. One opened meanwhile is left as it is.
    pub(crate) fn record_read(&mut self, key: u64, events: Option<Vec<store::Event>>) {
        let Some(s) = self.session_at(key).filter(|s| s.state == State::Closed) else {
            return;
        };
        let agent = s.agent.clone();
        // A subagent has no other copy; any other is loaded from its agent.
        let cut = |e: &[store::Event]| s.parent.is_none() && store::cut_short(e);
        let Some(events) = events.filter(|e| !e.is_empty()) else {
            self.open_unrecorded(key);
            return;
        };
        if cut(&events) {
            self.drop_record(key);
            self.open_unrecorded(key);
            return;
        }
        let slot = self.slots.iter().find(|x| x.entry.id == agent);
        let s = self
            .sessions
            .iter_mut()
            .find(|s| s.key == key)
            .expect("found above");
        s.forget();
        s.replay(events);
        s.recorded = true;
        if s.options.is_empty()
            && let Some(p) = slot.and_then(|x| x.probe.as_ref())
        {
            s.modes.clone_from(&p.modes);
            s.options.clone_from(&p.options);
        }
        self.place(key);
        // A record from before its index named its subagents names them.
        self.subagents_back(key);
    }

    /// Open the session `key`, which has no record, from its agent. A
    /// subagent opens from its record only: its agent does not load it.
    fn open_unrecorded(&mut self, key: u64) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        s.recorded = false;
        if s.parent.is_some() {
            return;
        }
        s.forget();
        s.plan = None;
        s.turns.clear();
        s.total = None;
        s.redo = None;
        s.state = State::Opening;
        let agent = s.agent.clone();
        self.open_when_running(&agent, key);
    }

    /// Drop the record of the session `key`: it is loaded from its agent
    /// again, and written afresh as it comes.
    pub(crate) fn drop_record(&mut self, key: u64) {
        let home = self.home.clone();
        if let Some(s) = self.session_mut(key) {
            if let Some(id) = &s.id {
                store::queue(Job::Remove(store::record_path(&home, &s.agent, &id.0)));
            }
            s.forget();
            s.log.clear();
            s.recorded = false;
        }
    }

    /// Let go of each agent idle for a while, with nothing going on in its
    /// sessions: its sessions close (their records are written) and it is
    /// started again, and they resumed, when one is written in.
    pub(crate) fn release_idle(&mut self, cx: &mut Context<Self>) {
        const IDLE: std::time::Duration = std::time::Duration::from_secs(10 * 60);
        let idle: Vec<String> = self
            .slots
            .iter()
            .filter(|s| matches!(s.stage, crate::hub::Stage::Running { .. }))
            .filter(|s| s.used.elapsed() >= IDLE)
            .filter(|slot| !self.agent_busy(&slot.entry.id))
            .map(|s| s.entry.id.clone())
            .collect();
        for agent in idle {
            self.let_go(&agent);
            tracing::info!("{agent} was idle and is let go");
        }
        cx.notify();
    }

    /// Let go of `agent`'s process: its sessions' records are written and
    /// they close, to be resumed when one is written in.
    pub(crate) fn let_go(&mut self, agent: &str) {
        let keys: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| s.agent == agent && s.id.is_some())
            .map(|s| s.key)
            .collect();
        for key in keys {
            self.flush(key);
            if let Some(s) = self.session_mut(key)
                && s.state == crate::session::State::Idle
            {
                s.state = crate::session::State::Closed;
                s.recorded = true;
            }
        }
        if let Some(slot) = self.slot_mut(agent) {
            slot.stage = crate::hub::Stage::Ready;
        }
    }

    /// Measure what Ghost keeps on this machine.
    pub fn measure(&self) {
        crate::hub::sizes(self.home.clone(), self.tx.clone());
    }

    /// Tidy `what` around the sessions kept.
    pub fn tidy(&mut self, what: crate::tidy::Tidy, cx: &mut Context<Self>) {
        let kept = crate::tidy::Kept {
            records: self
                .sessions
                .iter()
                .filter_map(|s| Some(store::record_path(&self.home, &s.agent, &s.id.as_ref()?.0)))
                .collect(),
            worktrees: self
                .sessions
                .iter()
                .filter_map(|s| s.worktree.as_ref().map(|w| w.path.clone()))
                .collect(),
            repos: self
                .settings
                .folders
                .iter()
                .filter(|f| crate::git::root(f).is_some())
                .cloned()
                .collect(),
            uids: self
                .sessions
                .iter()
                .filter(|s| !s.turns.is_empty())
                .map(Session::uid)
                .collect(),
        };
        self.tidied = None;
        crate::hub::tidy(self.home.clone(), what, kept, self.tx.clone());
        cx.notify();
    }

    /// Keep everything as the app quits.
    pub(super) fn keep_on_quit(cx: &mut Context<Self>) {
        cx.on_app_quit(|this, _| {
            this.keep_all();
            async {}
        })
        .detach();
    }
}
