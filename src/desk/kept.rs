//! What the desk keeps of its sessions between runs: the index read as the
//! window opens, each session's record written as it goes and read back
//! when it is opened. See `store`.

use gpui::Context;

use super::Desk;
use crate::{
    session::Session,
    store::{self, Index, Job},
};

impl Desk {
    /// Put the sessions kept from an earlier run on the desk, closed.
    pub(super) fn keep_from(&mut self, index: Result<Index, String>) {
        match index {
            Ok(index) => {
                for meta in index.sessions {
                    let key = self.take_key();
                    self.sessions.push(Session::stored(key, meta));
                }
            }
            Err(why) => {
                tracing::warn!("{why}");
                self.store_refused = Some(why);
            }
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
        };
        store::queue(Job::Save(self.home.clone(), store::bytes(&index)));
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
        self.save_index();
        store::settle();
    }

    /// Read back the record of the closed session `key`, if it has one: the
    /// thread shows at once, and the agent is asked only when you write.
    pub(crate) fn read_record(&mut self, key: u64) -> bool {
        let home = self.home.clone();
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return false;
        };
        let Some(id) = &s.id else { return false };
        let Some(events) =
            store::read(&store::record_path(&home, &s.agent, &id.0)).filter(|e| !e.is_empty())
        else {
            return false;
        };
        let agent = s.agent.clone();
        let slot = self.slots.iter().find(|x| x.entry.id == agent);
        let s = self
            .sessions
            .iter_mut()
            .find(|s| s.key == key)
            .expect("found above");
        s.items.clear();
        s.replay(events);
        s.recorded = true;
        if s.options.is_empty()
            && let Some(p) = slot.and_then(|x| x.probe.as_ref())
        {
            s.modes.clone_from(&p.modes);
            s.options.clone_from(&p.options);
        }
        true
    }

    /// Drop the record of the session `key`: it is loaded from its agent
    /// again, and written afresh as it comes.
    pub(crate) fn drop_record(&mut self, key: u64) {
        let home = self.home.clone();
        if let Some(s) = self.session_mut(key) {
            if let Some(id) = &s.id {
                store::queue(Job::Remove(store::record_path(&home, &s.agent, &id.0)));
            }
            s.items.clear();
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
            .filter(|slot| {
                !self.sessions.iter().any(|s| {
                    s.agent == slot.entry.id
                        && (matches!(
                            s.state,
                            crate::session::State::Working
                                | crate::session::State::Opening
                                | crate::session::State::SignIn
                        ) || s.waiting().is_some())
                })
            })
            .map(|s| s.entry.id.clone())
            .collect();
        for agent in idle {
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
            if let Some(slot) = self.slot_mut(&agent) {
                slot.stage = crate::hub::Stage::Ready;
            }
            tracing::info!("{agent} was idle and is let go");
        }
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
