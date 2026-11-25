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
    session::{Session, State},
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
    /// closed, to be loaded when opened again, and says so in its record;
    /// one opening is broken. The keys of those closed, whose records are
    /// to be written.
    pub fn exited(&mut self, agent: &str, why: &str) -> Vec<u64> {
        let mut closed = Vec::new();
        for s in self.all.iter_mut().filter(|s| s.meta.agent == agent) {
            let open = !matches!(s.state, State::Draft | State::Closed);
            if s.meta.id.is_some() && open {
                // The agent keeps it: opened again, it is loaded.
                s.note(&format!("The agent stopped: {why}"));
                closed.push(s.key);
                s.pending = None;
                s.since = None;
                s.state = State::Closed;
            } else if s.state == State::Opening {
                s.pending = None;
                s.since = None;
                s.state = State::Broken(format!("The agent stopped: {why}"));
            }
        }
        closed
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agent_client_protocol::v1::schema::{RequestPermissionRequest, StopReason};

    use super::*;
    use crate::hub::{Slot, Stage};
    use crate::session::Item;

    /// Sessions of the agents `a` and `b`, each open as `s1`, under keys
    /// 1 and 2.
    fn two() -> Sessions {
        let mut sessions = Sessions::default();
        for agent in ["a", "b"] {
            let key = sessions.take_key();
            let mut s = Session::new(key, agent, "/p".into());
            s.meta.id = Some(SessionId::new("s1"));
            s.state = State::Idle;
            sessions.all.push(s);
        }
        sessions
    }

    fn said(session: &str, update: &str) -> SessionNotification {
        serde_json::from_str(&format!(r#"{{"sessionId":"{session}","update":{update}}}"#)).unwrap()
    }

    fn chunk(text: &str) -> String {
        format!(
            r#"{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{text}"}}}}"#
        )
    }

    #[test]
    fn a_turns_chunks_go_to_the_session_of_their_agent_and_ended_closes_the_turn() {
        let mut sessions = two();
        sessions.at_mut(2).unwrap().said("go");
        assert_eq!(
            sessions.update("b", said("s1", &chunk("Do"))),
            Ok((2, false))
        );
        assert_eq!(
            sessions.update("b", said("s1", &chunk("ne."))),
            Ok((2, false))
        );
        // The same id under another agent is another session.
        assert!(sessions.at(1).unwrap().items.is_empty());
        let s = sessions.at_mut(2).unwrap();
        s.ended(Ok(StopReason::EndTurn));
        assert!(matches!(&s.items[1], Item::Agent(t) if t == "Done."));
        assert!(matches!(
            s.items[2],
            Item::Ended {
                reason: StopReason::EndTurn,
                ..
            }
        ));
        assert_eq!(s.state, State::Idle);
        // One for no session on the desk (the probe's) is handed back.
        assert!(sessions.update("b", said("probe", &chunk("x"))).is_err());
    }

    #[test]
    fn a_permission_asked_waits_in_its_session_until_answered() {
        let mut sessions = two();
        let request: RequestPermissionRequest = serde_json::from_str(
            r#"{"sessionId":"s1","toolCall":{"toolCallId":"t","title":"rm -rf build"},"options":[{"optionId":"ok","name":"Allow","kind":"allow_once"}]}"#,
        )
        .unwrap();
        let option = request.options[0].clone();
        let id = request.session_id.clone();
        let s = sessions.by_id("a", &id).unwrap();
        s.asked(7, request);
        assert_eq!(s.waiting().unwrap().title, "rm -rf build");
        s.answered(7, &option);
        assert!(s.waiting().is_none());
        assert!(sessions.at(2).unwrap().waiting().is_none());
    }

    #[test]
    fn an_agent_stopping_closes_and_notes_its_open_sessions_but_one_let_go_before_does_not() {
        let home = tempfile::tempdir().unwrap();
        let entry =
            serde_json::from_str(r#"{"id":"a","name":"A","version":"1","distribution":{}}"#)
                .unwrap();
        let mut slot = Slot::new(entry, home.path(), Some("/bin/sh -c 'sleep 30'".into()));
        let launch = slot.launch(home.path()).unwrap();
        let agent = acp::runtime()
            .block_on(acp::Agent::start(&launch, |_| {}))
            .unwrap();
        let pid = agent.pid();
        slot.stage = Stage::Running {
            agent: Arc::new(agent),
        };
        assert!(slot.runs(pid));
        assert!(!slot.runs(pid.map(|p| p + 1)));
        assert!(!slot.runs(None));

        let mut sessions = two();
        let key = sessions.take_key();
        let mut opening = Session::new(key, "a", "/p".into());
        opening.state = State::Opening;
        sessions.all.push(opening);
        assert_eq!(sessions.exited("a", "killed"), vec![1]);
        let a = sessions.at(1).unwrap();
        assert_eq!(a.state, State::Closed);
        assert!(matches!(&a.items[..], [Item::Note { trouble: true, .. }]));
        // Kept in its record, to be there when it is read back.
        assert!(
            matches!(&a.log[..], [store::Event::Note { text, trouble: true }] if text == "The agent stopped: killed")
        );
        assert_eq!(
            sessions.at(key).unwrap().state,
            State::Broken("The agent stopped: killed".into())
        );
        assert_eq!(sessions.at(2).unwrap().state, State::Idle);
    }

    #[test]
    fn a_session_its_agent_will_not_resume_is_loaded_without_its_record() {
        let home = tempfile::tempdir().unwrap();
        let mut sessions = two();
        let record = store::record_path(home.path(), "a", "s1");
        store::queue(Job::Append(
            record.clone(),
            vec![store::Event::You("go".into())],
        ));
        let s = sessions.at_mut(1).unwrap();
        s.recorded = true;
        s.said("go");
        let refused = Err(acp::Error::internal_error());
        assert_eq!(
            sessions.loaded(1, true, refused, home.path()),
            Loaded::Refused
        );
        store::settle();
        let s = sessions.at(1).unwrap();
        // Opened again, it is loaded, not resumed, and written afresh.
        assert!(!s.recorded && s.log.is_empty() && s.items.is_empty());
        assert!(!record.exists());
        // A load refused is the end of it.
        let refused = Err(acp::Error::internal_error());
        assert_eq!(
            sessions.loaded(2, false, refused, home.path()),
            Loaded::Stays
        );
        assert!(matches!(sessions.at(2).unwrap().state, State::Broken(_)));
    }

    #[test]
    fn a_subagent_set_to_work_appears_once_under_its_parent() {
        let mut sessions = two();
        let sub =
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1","title":"Read the papers"}"#;
        assert_eq!(sessions.update("a", said("s1", sub)), Ok((1, false)));
        assert_eq!(sessions.update("a", said("s1", sub)), Ok((1, false)));
        let children: Vec<&Session> = (sessions.all.iter())
            .filter(|s| s.parent == Some(1))
            .collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].meta.title, "Read the papers");
        assert_eq!(children[0].meta.agent, "a");
        // Its own updates go to it.
        let child = children[0].key;
        assert_eq!(
            sessions.update("a", said("c1", &chunk("Reading"))),
            Ok((child, false))
        );
    }

    #[test]
    fn a_record_cut_short_is_loaded_from_its_agent_not_resumed() {
        let home = tempfile::tempdir().unwrap();
        let mut sessions = two();
        let update = || store::Event::Update(Box::new(serde_json::from_str(&chunk("Do")).unwrap()));
        let cut = || vec![store::Event::You("go".into()), update()];
        for key in [1, 2] {
            sessions.at_mut(key).unwrap().state = State::Closed;
        }
        let read = sessions.read_back(1, Some(cut()), None, home.path());
        assert_eq!(read, Read::Unrecorded);
        assert!(!sessions.at(1).unwrap().recorded);
        let whole = vec![
            store::Event::You("go".into()),
            update(),
            store::Event::Ended {
                reason: StopReason::EndTurn,
                secs: 1,
            },
        ];
        let read = sessions.read_back(2, Some(whole), None, home.path());
        assert_eq!(read, Read::Replayed);
        let s = sessions.at(2).unwrap();
        assert!(s.recorded && s.items.len() == 3);
        // A subagent has no other copy: what there is of it is read.
        sessions.at_mut(2).unwrap().parent = Some(1);
        let read = sessions.read_back(2, Some(cut()), None, home.path());
        assert_eq!(read, Read::Replayed);
        // One opened meanwhile is left as it is.
        sessions.at_mut(1).unwrap().state = State::Idle;
        let read = sessions.read_back(1, Some(cut()), None, home.path());
        assert_eq!(read, Read::Left);
    }
}
