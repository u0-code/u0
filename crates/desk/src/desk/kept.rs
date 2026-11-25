//! What the desk keeps of its sessions between runs, through the server,
//! which writes it: the index sent as the window opens, each session's
//! record sent as it goes and read back when it is opened. See `store`.

use agent_client_protocol::v1::schema::SessionId;
use gpui::Context;

use super::Desk;
use crate::{
    proto::Call,
    session::{Session, State},
    sessions::Read,
    store::{self, Index},
};

impl Desk {
    /// Put the sessions kept from an earlier run on the desk, closed.
    pub(super) fn keep_from(&mut self, index: Result<Index, String>) {
        match index {
            Ok(index) => {
                self.gone = index.gone;
                self.composer.stashed = (index.stashed.into_iter())
                    .map(|text| super::mention::Stashed {
                        text,
                        attached: Vec::new(),
                    })
                    .collect();
                for meta in index.sessions {
                    let key = self.sessions.take_key();
                    if let Some(id) = &meta.id {
                        self.kept
                            .insert((meta.agent.clone(), id.clone()), meta.clone());
                    }
                    self.sessions.all.push(Session::stored(key, meta));
                    self.subagents_back(key);
                }
                self.settle_idle();
            }
            Err(why) => self.store_refused = Some(why),
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

    /// Send the server each session's meta that is not as it has it.
    pub(crate) fn save_index(&mut self) {
        if self.store_refused.is_some() {
            return;
        }
        let mut changed = Vec::new();
        for meta in self.sessions.all.iter().filter_map(Session::kept) {
            let id = meta.id.clone().expect("a kept session has an id");
            let at = (meta.agent.clone(), id);
            if self.kept.get(&at) != Some(meta) {
                self.kept.insert(at, meta.clone());
                changed.push(meta.clone());
            }
        }
        if !changed.is_empty() {
            self.transport.send(Call::Keep(changed));
        }
    }

    /// Have the server forget the session `id` of `agent`, and its record;
    /// `keep_out`: its agent keeps it, and its list is not to bring it back.
    pub(crate) fn unkeep(&mut self, agent: String, id: SessionId, keep_out: bool) {
        self.kept.remove(&(agent.clone(), id.clone()));
        self.transport.send(Call::Forget {
            agent,
            id,
            keep_out,
        });
    }

    /// Send the server what you put aside from the composer: its text.
    pub(crate) fn save_stash(&self) {
        if self.store_refused.is_none() {
            self.transport.send(Call::Stash(
                (self.composer.stashed.iter())
                    .map(|s| s.text.clone())
                    .collect(),
            ));
        }
    }

    /// Send what happened in the session `key` since its record was last
    /// sent; a session its agent has not named yet keeps it until then.
    pub(crate) fn flush(&mut self, key: u64) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        let Some(id) = s.meta.id.clone() else { return };
        if s.log.is_empty() {
            return;
        }
        let events = std::mem::take(&mut s.log);
        s.recorded = true;
        let agent = s.meta.agent.clone();
        self.transport.send(Call::Append { agent, id, events });
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
        crate::hub::Transport::settle();
    }

    /// Ask the server for the record of the closed session `key` (see
    /// `record_read`).
    pub(crate) fn read_record(&mut self, key: u64) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        match s.meta.id.clone() {
            Some(id) => {
                // Taken to have one until it is read, so it is read once.
                s.recorded = true;
                let agent = s.meta.agent.clone();
                self.transport.send(Call::Read { key, agent, id });
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
        match self.sessions.read_back(key, events, probe) {
            Read::Replayed => {
                self.place(key);
                // A record from before its index named its subagents names them.
                self.subagents_back(key);
            }
            // One cut short is dropped: the agent has the rest.
            Read::Unrecorded => {
                self.drop_record(key);
                self.open_unrecorded(key);
            }
            Read::Left => {}
        }
    }

    /// Drop the record of the session `key`: it is loaded from its agent
    /// again, and written afresh as it comes.
    pub(crate) fn drop_record(&mut self, key: u64) {
        if let Some(call) = self.sessions.drop_record(key) {
            self.transport.send(call);
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
        const IDLE: std::time::Duration = std::time::Duration::from_mins(10);
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
        self.transport.send(crate::proto::Call::Stop {
            agent: agent.to_owned(),
        });
        if let Some(slot) = self.slot_mut(agent) {
            slot.stage = crate::hub::Stage::Ready;
        }
    }

    /// Measure what Ghost keeps on this machine.
    pub fn measure(&self) {
        self.transport.send(Call::Sizes);
    }

    /// Tidy `what` around the sessions kept.
    pub fn tidy(&mut self, what: crate::tidy::Tidy, cx: &mut Context<Self>) {
        let kept = crate::tidy::Kept {
            records: self
                .sessions
                .all
                .iter()
                .filter_map(|s| Some((s.meta.agent.clone(), s.meta.id.as_ref()?.0.to_string())))
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
        self.pages.tidied = None;
        self.transport.send(Call::Tidy { what, kept });
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
