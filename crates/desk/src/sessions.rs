//! The sessions on the desk, each by the desk's key for it and found by
//! the id its agent gave it, on the server it is on: two servers' agents
//! may give the same id. Nothing here needs a window.

use std::path::PathBuf;

use agent_client_protocol::{
    schema::MaybeUndefined,
    v1::schema::{SessionId, SessionInfo, SessionUpdate},
};

use crate::{
    acp,
    hub::{Host, Probe},
    proto::{Call, Opened},
    session::{Session, State},
    store,
};

#[derive(Default)]
pub struct Sessions {
    /// Every session on the desk, in the order each came.
    pub all: Vec<Session>,
    /// The last key taken.
    last_key: u64,
}

impl Sessions {
    /// None yet, with keys from a random start: a server outlives the
    /// desk, and another desk may be linked to it, so that what it says
    /// of one desk's session (its turn ended) is not taken for another's.
    pub fn new() -> Self {
        Self {
            all: Vec::new(),
            last_key: crate::access::number() >> 16,
        }
    }

    /// A key nothing on the desk has had: a session's, a terminal's or a
    /// notice's.
    pub fn take_key(&mut self) -> u64 {
        self.last_key += 1;
        self.last_key
    }

    pub fn at(&self, key: u64) -> Option<&Session> {
        self.all.iter().find(|s| s.key == key)
    }

    /// Whether the session `key` asks before it goes on now: asked during
    /// a turn that still goes on, and not answered.
    pub fn asks(&self, key: u64) -> bool {
        self.at(key).and_then(crate::desk::lists::status)
            == Some(crate::desk::lists::Status::Approval)
    }

    /// The draft `here` takes, the newest: a new thread not yet sent, of
    /// any agent, not a subagent's nor a side chat's.
    pub fn draft(&self, here: impl Fn(&Session) -> bool) -> Option<u64> {
        (self.all.iter())
            .filter(|s| s.is_draft() && s.parent.is_none() && s.beside.is_none() && here(s))
            .map(|s| s.key)
            .max()
    }

    pub fn at_mut(&mut self, key: u64) -> Option<&mut Session> {
        self.all.iter_mut().find(|s| s.key == key)
    }

    /// The session `agent` on `host` knows as `id`.
    pub fn by_id(&mut self, host: Host, agent: &str, id: &SessionId) -> Option<&mut Session> {
        self.all
            .iter_mut()
            .find(|s| s.of(host, agent) && s.meta.id.as_ref() == Some(id))
    }

    /// The sessions of `agent` on `host`.
    fn of(&mut self, host: Host, agent: &str) -> impl Iterator<Item = &mut Session> {
        self.all.iter_mut().filter(move |s| s.of(host, agent))
    }

    /// The sessions of `agent` on `host` its CLI runs now outside u0
    /// (`running`): each is marked, and each other not.
    pub fn running(&mut self, host: Host, agent: &str, running: &[SessionId]) {
        for s in self.of(host, agent) {
            s.elsewhere = s.meta.id.as_ref().is_some_and(|id| running.contains(id));
        }
    }

    /// The subagent `id` the session `parent` set to work: a session of
    /// its own, under it.
    pub fn add_subagent(&mut self, parent: u64, id: SessionId, title: String, state: State) {
        let key = self.take_key();
        let Some(p) = self.at(parent) else {
            return;
        };
        let mut child = Session::new(key, &p.meta.agent, p.meta.folder.clone());
        child.host = p.host;
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

    /// The sessions of `agent` on `host` that waited to be signed in,
    /// opening again: their keys, to be opened.
    pub fn signed_in(&mut self, host: Host, agent: &str) -> Vec<u64> {
        let waiting = self.of(host, agent).filter(|s| s.state == State::SignIn);
        waiting
            .map(|s| {
                s.state = State::Opening;
                s.key
            })
            .collect()
    }

    /// `agent` on `host` started, or could not (why): the keys of its
    /// sessions that waited for it to open, to be opened now; or, if it
    /// could not, each is broken with why.
    pub fn started(&mut self, host: Host, agent: &str, failed: Option<&str>) -> Vec<u64> {
        let waiting =
            (self.of(host, agent)).filter(|s| s.state == State::Opening && !s.needs_worktree());
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

    /// What `agent` on `host` listed, by project: each session not on the
    /// desk yet (nor deleted, `gone`) is put there, closed; one closed
    /// already takes the agent's title and time.
    pub fn listed(
        &mut self,
        host: Host,
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
            if let Some(s) = self.by_id(host, agent, &info.session_id) {
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
            let mut s = Session::listed(key, agent, info, project);
            s.host = host;
            self.all.push(s);
        }
    }

    /// Lines of the record of the session `id` of `agent` on `host` that
    /// its server wrote, made of what the agent said in it, taken into its
    /// thread: its key. A subagent set to work becomes a session of its
    /// own, under its parent.
    pub fn appended(
        &mut self,
        host: Host,
        agent: &str,
        id: &SessionId,
        events: Vec<store::Event>,
    ) -> Option<u64> {
        let updates = events.iter().filter_map(|e| match e {
            store::Event::Update(u) => Some(&**u),
            _ => None,
        });
        for update in updates {
            let SessionUpdate::SubagentUpdate(u) = update else {
                continue;
            };
            if self.by_id(host, agent, &u.session_id).is_none()
                && let Some(parent) = self.by_id(host, agent, id)
            {
                let up = parent.key;
                let title = match &u.title {
                    MaybeUndefined::Value(t) => t.clone(),
                    _ => crate::tr("A subagent").into(),
                };
                self.add_subagent(up, u.session_id.clone(), title, State::Idle);
            }
            // What it is doing now, for its row, the column and its tab.
            if let Some(child) = self.by_id(host, agent, &u.session_id) {
                child.sub.take(u);
            }
        }
        let s = self.by_id(host, agent, id)?;
        s.meta.updated = std::time::SystemTime::now();
        s.replay(events);
        Some(s.key)
    }

    /// The process of `agent` on `host` stopped, why: each of its sessions
    /// open is closed, to be loaded when opened again, and says so in its
    /// record; one opening is broken. The keys of those closed, whose
    /// records are to be written.
    pub fn exited(&mut self, host: Host, agent: &str, why: &str) -> Vec<u64> {
        let mut closed = Vec::new();
        for s in self.of(host, agent) {
            let open = !matches!(s.state, State::Draft | State::Closed);
            if s.meta.id.is_some() && open {
                // The agent keeps it: opened again, it is loaded.
                s.note(&crate::tr_with(
                    "The agent stopped: %{why}",
                    &[("why", &why)],
                ));
                closed.push(s.key);
                s.pending = None;
                s.since = None;
                s.state = State::Closed;
            } else if s.state == State::Opening {
                s.pending = None;
                s.since = None;
                s.state = State::Broken(
                    crate::tr_with("The agent stopped: %{why}", &[("why", &why)]).into(),
                );
            }
        }
        closed
    }

    /// The session `key` loaded or resumed (`resumed`), or not.
    pub fn loaded(
        &mut self,
        key: u64,
        resumed: bool,
        result: Result<Opened, acp::Error>,
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
            Err(_) if resumed => Loaded::Refused,
            Err(f) => {
                s.state = State::Broken(f.message);
                Loaded::Stays
            }
        }
    }

    /// The record of the closed session `key`, read back (`None`: it has
    /// none); `probe`, what a session of its agent offers. One cut short
    /// is not read: the agent has the rest.
    pub fn read_back(
        &mut self,
        key: u64,
        events: Option<Vec<store::Event>>,
        probe: Option<&Probe>,
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

    /// Forget the record of the session `key`: it is loaded from its agent
    /// again, and written afresh as it comes. What asks the server to drop
    /// it.
    pub fn drop_record(&mut self, key: u64) -> Option<Call> {
        let s = self.at_mut(key)?;
        s.forget();
        s.log.clear();
        s.recorded = false;
        Some(Call::DropRecord {
            agent: s.meta.agent.clone(),
            id: s.meta.id.clone()?,
        })
    }
}

/// How a load or resume of a session came back: what the desk does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    /// It is open: say what you said while it opened, if anything.
    Open(Option<String>),
    /// Its agent would not resume it: its record is to be dropped, and it
    /// opened again, which loads it.
    Refused,
    /// Nothing more: it waits to be signed in, or is broken.
    Stays,
}

/// A session's record read back: what the desk does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// Its thread is built from the record.
    Replayed,
    /// It has no record to go by (or one cut short, to be dropped): it is
    /// opened from its agent.
    Unrecorded,
    /// It was opened meanwhile, or is gone: left as it is.
    Left,
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::v1::schema::{RequestPermissionRequest, StopReason};

    use agent_client_protocol::v1::schema::SessionNotification;

    use super::*;
    use crate::hub::{Slot, Stage};
    use crate::session::Item;
    use u0_core::records::Records;

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

    #[test]
    fn a_project_finds_its_one_draft_whatever_its_agent_and_not_a_thread_sent() {
        let mut sessions = two();
        let here = |s: &Session| s.meta.project == std::path::Path::new("/p");
        assert_eq!(sessions.draft(here), None);
        let key = sessions.take_key();
        sessions.all.push(Session::new(key, "b", "/p".into()));
        let elsewhere = sessions.take_key();
        sessions.all.push(Session::new(elsewhere, "a", "/q".into()));
        let mut side = Session::new(sessions.take_key(), "a", "/p".into());
        side.beside = Some(1);
        sessions.all.push(side);
        assert_eq!(sessions.draft(here), Some(key));
        sessions
            .at_mut(key)
            .unwrap()
            .items
            .push(Item::You("hi".into()));
        assert_eq!(sessions.draft(here), None);
    }

    fn said(session: &str, update: &str) -> SessionNotification {
        serde_json::from_str(&format!(r#"{{"sessionId":"{session}","update":{update}}}"#)).unwrap()
    }

    fn chunk(text: &str) -> String {
        format!(
            r#"{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{text}"}}}}"#
        )
    }

    /// What `agent` says in a session, as the server writes it and the
    /// desk takes it in; `None` where it went to no session open with it.
    fn heard(
        records: &Records,
        sessions: &mut Sessions,
        agent: &str,
        n: SessionNotification,
    ) -> Option<u64> {
        let (id, events) = records.said(agent, n).ok()?;
        sessions.appended(Host::HERE, agent, &id, events)
    }

    #[test]
    fn a_turn_is_written_as_it_comes_and_its_record_builds_the_same_thread() {
        let home = tempfile::tempdir().unwrap();
        let records = Records::new(home.path().to_path_buf());
        let mut sessions = two();
        let id = SessionId::new("s1");
        records.open("b", id.clone());
        // What you said is the desk's, sent to be written first.
        let s = sessions.at_mut(2).unwrap();
        s.said("go");
        records.append("b", &id, std::mem::take(&mut s.log));
        let done = [chunk("Do"), chunk("ne.")];
        for text in &done {
            let n = said("s1", text);
            assert_eq!(heard(&records, &mut sessions, "b", n), Some(2));
        }
        // The same id under another agent is another session.
        assert!(sessions.at(1).unwrap().items.is_empty());
        let s = sessions.at_mut(2).unwrap();
        s.ended(Ok(StopReason::EndTurn), Vec::new());
        records.append("b", &id, std::mem::take(&mut s.log));
        assert!(matches!(&s.items[1], Item::Agent(t) if t == "Done."));
        assert!(matches!(
            s.items[2],
            Item::Ended {
                reason: StopReason::EndTurn,
                ..
            }
        ));
        assert_eq!(s.state, State::Idle);
        // One for no session open with its agent (the probe's) is handed back.
        assert!(records.said("b", said("probe", &chunk("x"))).is_err());
        // Read back, the record builds the thread the desk has.
        store::settle();
        let mut again = Session::new(9, "b", "/p".into());
        again.replay(store::read(&records.path("b", &id)).unwrap());
        let shown = |s: &Session| {
            (s.items.iter())
                .map(|i| match i {
                    Item::You(t) | Item::Agent(t) => t.clone(),
                    Item::Ended { .. } => "ended".into(),
                    _ => "?".into(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&again), ["go", "Done.", "ended"]);
        assert_eq!(shown(&again), shown(sessions.at(2).unwrap()));
        assert_eq!(again.times.len(), 2, "when you spoke, and when it did");
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
        let s = sessions.by_id(Host::HERE, "a", &id).unwrap();
        s.asked(7, request, crate::acp::Asker::Agent);
        assert_eq!(s.waiting().unwrap().title, "rm -rf build");
        s.answered(7, &option);
        assert!(s.waiting().is_none());
        assert!(sessions.at(2).unwrap().waiting().is_none());
    }

    #[test]
    fn a_session_asks_while_its_turn_waits_on_the_question_and_not_once_it_ended() {
        let mut sessions = two();
        let request: RequestPermissionRequest = serde_json::from_str(
            r#"{"sessionId":"s1","toolCall":{"toolCallId":"t","title":"Go on?"},"options":[{"optionId":"ok","name":"Allow","kind":"allow_once"}]}"#,
        )
        .unwrap();
        let s = sessions.at_mut(1).unwrap();
        s.state = State::Working;
        s.asked(7, request, crate::acp::Asker::Agent);
        assert!(sessions.asks(1));
        // Never answered: its agent gave up, and the turn ended.
        sessions.at_mut(1).unwrap().state = State::Idle;
        assert!(!sessions.asks(1));
        assert!(!sessions.asks(9), "a thread gone asks nothing");
    }

    #[test]
    fn an_agent_stopping_closes_and_notes_its_open_sessions_but_one_let_go_before_does_not() {
        let entry =
            serde_json::from_str(r#"{"id":"a","name":"A","version":"1","distribution":{}}"#)
                .unwrap();
        let mut slot = Slot::new(
            Host::HERE,
            entry,
            None,
            Some("/bin/sh -c 'sleep 30'".into()),
        );
        let pid = Some(4242);
        slot.stage = Stage::Running { pid };
        assert!(slot.runs(pid));
        assert!(!slot.runs(pid.map(|p| p + 1)));
        assert!(!slot.runs(None));

        let mut sessions = two();
        let key = sessions.take_key();
        let mut opening = Session::new(key, "a", "/p".into());
        opening.state = State::Opening;
        sessions.all.push(opening);
        assert_eq!(sessions.exited(Host::HERE, "a", "killed"), vec![1]);
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
        let mut sessions = two();
        let s = sessions.at_mut(1).unwrap();
        s.recorded = true;
        s.said("go");
        let refused = Err(acp::Error::internal_error());
        assert_eq!(sessions.loaded(1, true, refused), Loaded::Refused);
        let drop = sessions.drop_record(1);
        assert!(
            matches!(drop, Some(Call::DropRecord { agent, id }) if agent == "a" && *id.0 == *"s1")
        );
        let s = sessions.at(1).unwrap();
        // Opened again, it is loaded, not resumed, and written afresh.
        assert!(!s.recorded && s.log.is_empty() && s.items.is_empty());
        // A load refused is the end of it.
        let refused = Err(acp::Error::internal_error());
        assert_eq!(sessions.loaded(2, false, refused), Loaded::Stays);
        assert!(matches!(sessions.at(2).unwrap().state, State::Broken(_)));
    }

    #[test]
    fn a_subagent_set_to_work_appears_once_under_its_parent() {
        let home = tempfile::tempdir().unwrap();
        let records = Records::new(home.path().to_path_buf());
        records.open("a", SessionId::new("s1"));
        let mut sessions = two();
        let sub =
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1","title":"Read the papers"}"#;
        for _ in 0..2 {
            let n = said("s1", sub);
            assert_eq!(heard(&records, &mut sessions, "a", n), Some(1));
        }
        let children: Vec<&Session> = (sessions.all.iter())
            .filter(|s| s.parent == Some(1))
            .collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].meta.title, "Read the papers");
        assert_eq!(children[0].meta.agent, "a");
        // Its own updates go to it, written in a record of its own.
        let child = children[0].key;
        let n = said("c1", &chunk("Reading"));
        assert_eq!(heard(&records, &mut sessions, "a", n), Some(child));
    }

    #[test]
    fn a_claude_code_load_puts_each_subagent_under_the_one_that_set_it_to_work() {
        let home = tempfile::tempdir().unwrap();
        let records = Records::new(home.path().to_path_buf());
        let agent = "claude-acp";
        records.open(agent, SessionId::new("p1"));
        let mut sessions = Sessions::default();
        let key = sessions.take_key();
        let mut s = Session::new(key, agent, "/p".into());
        s.meta.id = Some(SessionId::new("p1"));
        s.state = State::Opening;
        sessions.all.push(s);
        let sent: Vec<SessionNotification> = serde_json::from_str(include_str!(
            "../../core/src/fixtures/claude-subagents-load.json"
        ))
        .unwrap();
        for n in sent {
            assert!(heard(&records, &mut sessions, agent, n).is_some());
        }
        let of = |id: &str| {
            (sessions.all.iter())
                .find(|s| s.meta.id.as_ref().is_some_and(|i| &*i.0 == id))
                .unwrap()
        };
        let (parser, docs, spec) = (
            of("a1b2c3d4e5f6a7b8c"),
            of("ab1c2d3e4f5a6b7c8"),
            of("a9f8e7d6c5b4a3f2e"),
        );
        assert_eq!((parser.parent, docs.parent), (Some(key), Some(key)));
        assert_eq!(spec.parent, Some(parser.key));
        // Each done, as its parent said at the load's end; told its task.
        for c in [parser, docs, spec] {
            assert!(!c.sub.working());
            assert_eq!(c.sub.end, Some(crate::session::End::Done));
            assert!(matches!(c.items.first(), Some(Item::Told(_))));
        }
    }

    #[test]
    fn a_record_cut_short_is_loaded_from_its_agent_not_resumed() {
        let mut sessions = two();
        let update = || store::Event::Update(Box::new(serde_json::from_str(&chunk("Do")).unwrap()));
        let cut = || vec![store::Event::You("go".into()), update()];
        for key in [1, 2] {
            sessions.at_mut(key).unwrap().state = State::Closed;
        }
        let read = sessions.read_back(1, Some(cut()), None);
        assert_eq!(read, Read::Unrecorded);
        assert!(!sessions.at(1).unwrap().recorded);
        let whole = vec![
            store::Event::You("go".into()),
            update(),
            store::Event::Ended {
                reason: StopReason::EndTurn,
                secs: 1,
                tokens: Vec::new(),
            },
        ];
        let read = sessions.read_back(2, Some(whole), None);
        assert_eq!(read, Read::Replayed);
        let s = sessions.at(2).unwrap();
        assert!(s.recorded && s.items.len() == 3);
        // A subagent has no other copy: what there is of it is read.
        sessions.at_mut(2).unwrap().parent = Some(1);
        let read = sessions.read_back(2, Some(cut()), None);
        assert_eq!(read, Read::Replayed);
        // One opened meanwhile is left as it is.
        sessions.at_mut(1).unwrap().state = State::Idle;
        let read = sessions.read_back(1, Some(cut()), None);
        assert_eq!(read, Read::Left);
    }

    #[test]
    fn two_servers_sessions_with_one_agent_and_id_are_kept_apart() {
        let (a, b) = (Host::HERE, Host(1));
        let info: SessionInfo =
            serde_json::from_str(r#"{"sessionId":"s1","cwd":"/p","title":"Same"}"#).unwrap();
        let mut sessions = Sessions::default();
        sessions.listed(a, "claude", vec![("/p".into(), info.clone())], &[]);
        sessions.listed(b, "claude", vec![("/p".into(), info.clone())], &[]);
        // Listed again, each is the one it was: none doubled.
        sessions.listed(b, "claude", vec![("/p".into(), info)], &[]);
        assert_eq!(sessions.all.len(), 2);
        let id = SessionId::new("s1");
        let on_b = sessions.by_id(b, "claude", &id).unwrap().key;
        let on_a = sessions.by_id(a, "claude", &id).unwrap().key;
        assert_ne!(on_a, on_b);
        for s in &mut sessions.all {
            s.state = State::Idle;
        }
        // What B's agent says goes to B's session alone.
        let home = tempfile::tempdir().unwrap();
        let records = Records::new(home.path().to_path_buf());
        records.open("claude", id.clone());
        let (id, events) = records.said("claude", said("s1", &chunk("Hi"))).unwrap();
        assert_eq!(sessions.appended(b, "claude", &id, events), Some(on_b));
        assert!(sessions.at(on_a).unwrap().items.is_empty());
        assert!(!sessions.at(on_b).unwrap().items.is_empty());
        // B's agent stopping closes B's session, not A's.
        assert_eq!(sessions.exited(b, "claude", "gone"), vec![on_b]);
        assert_eq!(sessions.at(on_a).unwrap().state, State::Idle);
    }

    #[test]
    fn a_session_running_elsewhere_is_marked_until_its_agent_says_it_no_longer_is() {
        let mut sessions = two();
        let s1 = [SessionId::new("s1")];
        sessions.running(Host::HERE, "a", &s1);
        let marked = |s: &Sessions| s.all.iter().map(|s| s.elsewhere).collect::<Vec<_>>();
        // Only `a`'s: `b`'s s1 is another session.
        assert_eq!(marked(&sessions), vec![true, false]);
        sessions.running(Host::HERE, "a", &[]);
        assert_eq!(marked(&sessions), vec![false, false]);
    }
}
