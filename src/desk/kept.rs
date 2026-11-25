//! What the desk keeps of its sessions between runs: the index read as the
//! window opens, each session's record written as it goes and read back
//! when it is opened. See `store`.

use gpui::Context;

use super::Desk;
use crate::{
    session::{Session, State},
    sessions::Read,
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
                    let key = self.sessions.take_key();
                    self.sessions.all.push(Session::stored(key, meta));
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
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let missing: Vec<(String, String)> = s
            .meta
            .subagents
            .iter()
            .filter(|(id, _)| {
                !self.sessions.all.iter().any(|c| {
                    c.meta.agent == s.meta.agent && c.meta.id.as_ref().is_some_and(|i| *i.0 == **id)
                })
            })
            .cloned()
            .collect();
        for (id, title) in missing {
            let id = agent_client_protocol::v1::schema::SessionId::new(id);
            self.sessions
                .add_subagent(key, id, title, crate::session::State::Closed);
        }
    }

    /// A new session of the agent of the session `of`, in its project and
    /// worktree, not yet begun.
    pub(crate) fn beside(&mut self, of: u64) -> Option<Session> {
        let key = self.sessions.take_key();
        let o = self.sessions.at(of)?;
        let mut s = Session::new(key, &o.meta.agent, o.meta.folder.clone());
        s.meta.project.clone_from(&o.meta.project);
        s.meta.worktree.clone_from(&o.meta.worktree);
        (s.branch, s.repo) = (o.branch.clone(), o.repo);
        s.scripts.clone_from(&o.scripts);
        if let Some(slot) = self.slot(&s.meta.agent) {
            slot.seed(&mut s);
        }
        Some(s)
    }

    /// The session `old` put away, and `new` in its place in the lists.
    pub(crate) fn replace(&mut self, old: u64, new: &mut Session) {
        if let Some(o) = self.sessions.at_mut(old) {
            new.meta.title.clone_from(&o.meta.title);
            new.meta.flags.order = o.meta.flags.order;
            new.meta.flags.pinned = o.meta.flags.pinned.take();
            o.meta.flags.archived = true;
        }
    }

    /// Write the index of the sessions the agents have, off the window's
    /// thread.
    pub(crate) fn save_index(&self) {
        if self.store_refused.is_some() {
            return;
        }
        let index = Index {
            sessions: self
                .sessions
                .all
                .iter()
                .filter_map(Session::kept)
                .cloned()
                .collect(),
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
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        let Some(id) = s.meta.id.clone() else { return };
        if s.log.is_empty() {
            return;
        }
        let events = std::mem::take(&mut s.log);
        s.recorded = true;
        store::queue(Job::Append(
            store::record_path(&home, &s.meta.agent, &id.0),
            events,
        ));
    }

    /// Write every record and the index, and wait until they are written:
    /// as the window closes.
    pub(crate) fn keep_all(&mut self) {
        // A turn going on as Ghost closes may go on in the agent; the
        // record says where Ghost lost sight of it.
        for s in &mut self.sessions.all {
            if matches!(s.state, crate::session::State::Working) {
                s.note("Ghost closed while this turn was going on; the agent may have gone on with it.");
            }
        }
        let keys: Vec<u64> = self.sessions.all.iter().map(|s| s.key).collect();
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
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        match &s.meta.id {
            Some(id) => {
                let path = store::record_path(&home, &s.meta.agent, &id.0);
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
        let agent = self.sessions.at(key).map(|s| &s.meta.agent);
        let probe = (self.slots.iter())
            .find(|slot| Some(&slot.entry.id) == agent)
            .and_then(|slot| slot.probe.as_ref());
        match self.sessions.read_back(key, events, probe, &self.home) {
            Read::Replayed => {
                self.place(key);
                // A record from before its index named its subagents names them.
                self.subagents_back(key);
            }
            Read::Unrecorded => self.open_unrecorded(key),
            Read::Left => {}
        }
    }

    /// Open the session `key`, which has no record, from its agent. A
    /// subagent opens from its record only: its agent does not load it.
    fn open_unrecorded(&mut self, key: u64) {
        let Some(s) = self.sessions.at_mut(key) else {
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
        let agent = s.meta.agent.clone();
        self.open_when_running(&agent, key);
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
            .all
            .iter()
            .filter(|s| s.meta.agent == agent && s.meta.id.is_some())
            .map(|s| s.key)
            .collect();
        for key in keys {
            self.flush(key);
            if let Some(s) = self.sessions.at_mut(key)
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
                .all
                .iter()
                .filter_map(|s| {
                    Some(store::record_path(
                        &self.home,
                        &s.meta.agent,
                        &s.meta.id.as_ref()?.0,
                    ))
                })
                .collect(),
            worktrees: self
                .sessions
                .all
                .iter()
                .filter_map(|s| s.meta.worktree.as_ref().map(|w| w.path.clone()))
                .collect(),
            projects: self.settings.folders.clone(),
            uids: self
                .sessions
                .all
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
