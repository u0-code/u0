//! The Ghost protocol: what the desk asks of the server (a [`Call`]) and
//! what the server says back (an [`Event`]): each answer, and what an agent
//! says on its own. Serde types and nothing else.
//!
//! Each answer names what it answers (the session's key, the agent's id),
//! so a call has no id of its own, and none waits for a reply. What goes
//! over the wire to an agent is the ACP schema's, carried here as it is,
//! never mirrored.

use std::{collections::HashMap, path::PathBuf};

use agent_client_protocol::v1::schema::{
    ContentBlock, ForkSessionResponse, InitializeResponse, MessageId, NewSessionResponse,
    RequestPermissionOutcome, SessionConfigOption, SessionId, SessionInfo, SessionModeState,
    StopReason,
};
use serde::{Deserialize, Serialize};

use crate::{
    acp::{self, Error},
    registry::{self, Entry},
};

/// What the desk asks of the server. An agent is named by its registry id
/// (`agent`), a session on the desk by its key, and in its agent by its id
/// (`session`).
#[derive(Debug, Serialize, Deserialize)]
pub enum Call {
    /// Read the newest registry.
    Fetch,
    /// Install `entry` from its archive, or update it to its version.
    Install {
        entry: Entry,
    },
    /// Stop the install of `agent` going on; nothing of it is kept.
    CancelInstall {
        agent: String,
    },
    /// Start `entry`'s program (by `command`, a command line of yours,
    /// where you gave one) and say hello to it.
    Start {
        entry: Entry,
        command: Option<String>,
    },
    /// Let go of `agent`: its process stops once nothing more is asked of
    /// it.
    Stop {
        agent: String,
    },
    /// Stop `agent` at once, and all it started: when it would not stop a
    /// turn.
    Kill {
        agent: String,
    },
    /// Open a new session in `folder`, with the folders `also` beside it.
    Open {
        agent: String,
        key: u64,
        folder: PathBuf,
        also: Vec<PathBuf>,
    },
    /// The sessions `agent` keeps in each of `places` (where it works, and
    /// the project that is) and in Ghost's worktrees of them.
    List {
        agent: String,
        places: Vec<(PathBuf, PathBuf)>,
    },
    /// Open again a session the agent keeps: resumed, without what was
    /// said in it, or loaded, with all that was said in it again.
    Reopen {
        agent: String,
        key: u64,
        session: SessionId,
        folder: PathBuf,
        also: Vec<PathBuf>,
        resumed: bool,
    },
    /// Open a session in `dir` to read what a session offers, then delete
    /// it (or close it) again.
    Probe {
        agent: String,
        dir: PathBuf,
        delete: bool,
        close: bool,
    },
    /// Branch `session` off into a new one, whole or before an item of its
    /// thread (`upto`: the item, and the last message the fork keeps).
    Fork {
        agent: String,
        from: u64,
        session: SessionId,
        folder: PathBuf,
        upto: Option<(usize, MessageId)>,
    },
    Delete {
        agent: String,
        key: u64,
        session: SessionId,
    },
    /// Say `content` in `session`; the turn's end comes back.
    Prompt {
        agent: String,
        key: u64,
        session: SessionId,
        content: Vec<ContentBlock>,
    },
    /// Stop a task going on in the background, as the `JetBrains`
    /// extension of ACP asks it.
    StopTask {
        agent: String,
        session: SessionId,
        task: String,
    },
    /// Stop the turn going on in `session`.
    Cancel {
        agent: String,
        session: SessionId,
    },
    /// Answer the question `agent` asked under `request`.
    Answer {
        agent: String,
        request: u64,
        outcome: RequestPermissionOutcome,
    },
    SetMode {
        agent: String,
        key: u64,
        session: SessionId,
        mode: String,
    },
    /// Choose `value` for the option `config` (a model, a level of
    /// thought).
    SetOption {
        agent: String,
        key: u64,
        session: SessionId,
        config: String,
        value: String,
    },
    /// Sign in with the agent's method `method`: the agent itself does it,
    /// often in your browser.
    SignIn {
        agent: String,
        method: String,
    },
    SignOut {
        agent: String,
    },
    /// How much memory each agent running holds.
    Weigh,
    /// What of each of `agents` is here.
    Here {
        agents: Vec<Entry>,
    },
    /// What `cli` says its version is.
    CliVersion {
        cli: PathBuf,
    },
}

/// What the server sends back.
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Registry(Result<registry::Registry, String>),
    Installing {
        agent: String,
        what: String,
        share: Option<f32>,
    },
    Installed {
        agent: String,
        result: Result<(), String>,
    },
    /// An agent started, with its process's id and what it said of itself,
    /// or why it did not.
    Started {
        agent: String,
        result: Result<(Option<u32>, Box<InitializeResponse>), Error>,
    },
    /// Something an agent said on its own.
    Said {
        agent: String,
        event: Box<acp::Event>,
    },
    Opened {
        key: u64,
        result: Result<Box<NewSessionResponse>, Error>,
    },
    /// The sessions `agent` keeps in the projects, each with its project.
    Listed {
        agent: String,
        result: Result<Vec<(PathBuf, SessionInfo)>, String>,
    },
    /// The folders `agent` has sessions in that are not projects, with how
    /// many in each.
    Elsewhere {
        agent: String,
        folders: Vec<(PathBuf, usize)>,
    },
    /// A session the agent keeps was loaded: what was said in it came
    /// before this, as updates.
    Loaded {
        key: u64,
        /// Resumed from the desk's record, not loaded with what was said.
        resumed: bool,
        result: Result<Opened, Error>,
    },
    /// What a session of `agent` offers before it is opened, or why the
    /// agent would not open one.
    Probed {
        agent: String,
        result: Result<Box<NewSessionResponse>, Error>,
    },
    /// A session branched off the session `from` (before the item `at` of
    /// its thread, or whole), or why it was not.
    Forked {
        from: u64,
        at: Option<usize>,
        result: Result<Box<ForkSessionResponse>, Error>,
    },
    /// A session was deleted from its agent, or could not be.
    Deleted {
        key: u64,
        result: Result<(), String>,
    },
    Ended {
        key: u64,
        result: Result<StopReason, Error>,
    },
    /// What a session's options are after one was changed.
    Options {
        key: u64,
        options: Vec<SessionConfigOption>,
    },
    /// A change asked of a session that the agent refused.
    Refused {
        key: u64,
        why: String,
    },
    /// Signing in to `agent` is done, or failed: why.
    Signed {
        agent: String,
        result: Result<(), String>,
    },
    /// An agent signed out, or could not.
    LoggedOut {
        agent: String,
        result: Result<(), String>,
    },
    /// How much memory each running agent holds: its id, process id and
    /// kilobytes.
    Weighed(Vec<(String, u32, u64)>),
    /// What of each agent the registry lists is here, by its id.
    Here(HashMap<String, Here>),
    /// What a CLI of yours says its version is.
    CliVersion {
        cli: PathBuf,
        version: String,
    },
}

/// What a session loaded or resumed starts with.
#[derive(Debug, Serialize, Deserialize)]
pub struct Opened {
    pub modes: Option<SessionModeState>,
    pub options: Option<Vec<SessionConfigOption>>,
}

/// What of an agent is here: whether it can be started, the version
/// installed, and the CLI of yours it drives.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Here {
    pub launches: bool,
    pub installed: Option<String>,
    pub cli: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

    use super::*;

    /// `value` written as JSON reads back as what writes the same JSON; the
    /// name of the variant it is.
    fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> String {
        let written = serde_json::to_value(value).unwrap();
        let read: T = serde_json::from_value(written.clone()).unwrap();
        assert_eq!(serde_json::to_value(read).unwrap(), written);
        match written {
            Value::String(unit) => unit,
            Value::Object(tagged) => tagged.keys().next().unwrap().clone(),
            _ => panic!("not an enum: {written}"),
        }
    }

    /// The name of each variant of `T`, as serde lists them when it meets
    /// one it does not know: so that a variant added is not left out here.
    fn variants<T: DeserializeOwned + std::fmt::Debug>() -> BTreeSet<String> {
        let why = serde_json::from_value::<T>(json!({"?": null}))
            .unwrap_err()
            .to_string();
        // unknown variant `?`, expected one of `A`, `B`, ...
        why.split('`')
            .skip(3)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    fn of<T: DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn every_call_and_event_reads_back_as_it_was_written() {
        let entry: Entry = of(json!({
            "id": "a", "name": "A", "version": "1",
            "distribution": {"npx": {"package": "@a/acp@1", "args": ["--acp"]}},
        }));
        let session = SessionId::new("s1");
        // Recorded from @agentclientprotocol/codex-acp 2.10.0.
        let opened: NewSessionResponse =
            serde_json::from_str(include_str!("fixtures/codex-session-new.json")).unwrap();
        let (dir, cli) = (PathBuf::from("/p"), PathBuf::from("/bin/a"));
        let calls = vec![
            Call::Fetch,
            Call::Install {
                entry: entry.clone(),
            },
            Call::CancelInstall { agent: "a".into() },
            Call::Start {
                entry: entry.clone(),
                command: Some("a --acp".into()),
            },
            Call::Stop { agent: "a".into() },
            Call::Kill { agent: "a".into() },
            Call::Open {
                agent: "a".into(),
                key: 1,
                folder: dir.clone(),
                also: vec![dir.join("b")],
            },
            Call::List {
                agent: "a".into(),
                places: vec![(dir.clone(), dir.clone())],
            },
            Call::Reopen {
                agent: "a".into(),
                key: 1,
                session: session.clone(),
                folder: dir.clone(),
                also: Vec::new(),
                resumed: true,
            },
            Call::Probe {
                agent: "a".into(),
                dir: dir.clone(),
                delete: true,
                close: false,
            },
            Call::Fork {
                agent: "a".into(),
                from: 1,
                session: session.clone(),
                folder: dir.clone(),
                upto: Some((3, MessageId::new("m2"))),
            },
            Call::Delete {
                agent: "a".into(),
                key: 1,
                session: session.clone(),
            },
            Call::Prompt {
                agent: "a".into(),
                key: 1,
                session: session.clone(),
                content: vec![of(json!({"type": "text", "text": "go"}))],
            },
            Call::StopTask {
                agent: "a".into(),
                session: session.clone(),
                task: "t1".into(),
            },
            Call::Cancel {
                agent: "a".into(),
                session: session.clone(),
            },
            Call::Answer {
                agent: "a".into(),
                request: 7,
                outcome: of(json!({"outcome": "selected", "optionId": "allow"})),
            },
            Call::SetMode {
                agent: "a".into(),
                key: 1,
                session: session.clone(),
                mode: "plan".into(),
            },
            Call::SetOption {
                agent: "a".into(),
                key: 1,
                session,
                config: "model".into(),
                value: "fast".into(),
            },
            Call::SignIn {
                agent: "a".into(),
                method: "browser".into(),
            },
            Call::SignOut { agent: "a".into() },
            Call::Weigh,
            Call::Here {
                agents: vec![entry.clone()],
            },
            Call::CliVersion { cli: cli.clone() },
        ];
        let events = vec![
            Event::Registry(Ok(registry::Registry {
                agents: vec![entry],
                logos: ["a".to_owned()].into(),
            })),
            Event::Installing {
                agent: "a".into(),
                what: "Downloading".into(),
                share: Some(0.5),
            },
            Event::Installed {
                agent: "a".into(),
                result: Err("cancelled".into()),
            },
            Event::Started {
                agent: "a".into(),
                result: Ok((Some(42), Box::new(of(json!({"protocolVersion": 1}))))),
            },
            Event::Said {
                agent: "a".into(),
                event: Box::new(acp::Event::Update(of(json!({
                    "sessionId": "s1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "hi"},
                    },
                })))),
            },
            Event::Opened {
                key: 1,
                result: Ok(Box::new(opened.clone())),
            },
            Event::Listed {
                agent: "a".into(),
                result: Ok(vec![(
                    dir.clone(),
                    of(json!({"sessionId": "s1", "cwd": "/p"})),
                )]),
            },
            Event::Elsewhere {
                agent: "a".into(),
                folders: vec![(dir.join("x"), 2)],
            },
            Event::Loaded {
                key: 1,
                resumed: false,
                result: Ok(Opened {
                    modes: opened.modes.clone(),
                    options: opened.config_options.clone(),
                }),
            },
            Event::Probed {
                agent: "a".into(),
                result: Err(Error::internal_error().data("no")),
            },
            Event::Forked {
                from: 1,
                at: Some(3),
                result: Ok(Box::new(of(json!({"sessionId": "s2"})))),
            },
            Event::Deleted {
                key: 1,
                result: Ok(()),
            },
            Event::Ended {
                key: 1,
                result: Ok(StopReason::EndTurn),
            },
            Event::Options {
                key: 1,
                options: opened.config_options.unwrap(),
            },
            Event::Refused {
                key: 1,
                why: "no".into(),
            },
            Event::Signed {
                agent: "a".into(),
                result: Err("no".into()),
            },
            Event::LoggedOut {
                agent: "a".into(),
                result: Ok(()),
            },
            Event::Weighed(vec![("a".into(), 42, 1024)]),
            Event::Here(HashMap::from([(
                "a".to_owned(),
                Here {
                    launches: true,
                    installed: Some("1".into()),
                    cli: Some(cli.clone()),
                },
            )])),
            Event::CliVersion {
                cli,
                version: "a 1.0".into(),
            },
        ];
        let called: BTreeSet<String> = calls.iter().map(round_trip).collect();
        assert_eq!(called, variants::<Call>());
        let said: BTreeSet<String> = events.iter().map(round_trip).collect();
        assert_eq!(said, variants::<Event>());
    }

    #[test]
    fn all_an_agent_says_on_its_own_reads_back_as_it_was_written() {
        let said = [
            acp::Event::Update(of(json!({
                "sessionId": "s1",
                "update": {"sessionUpdate": "plan", "entries": []},
            }))),
            acp::Event::Permission {
                id: 7,
                request: of(json!({
                    "sessionId": "s1",
                    "toolCall": {"toolCallId": "c1", "title": "rm -rf build"},
                    "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
                })),
            },
            acp::Event::Exited {
                pid: Some(42),
                why: "killed".into(),
            },
            acp::Event::Task {
                session: "s1".into(),
                task: acp::Task::Ended {
                    id: "t1".into(),
                    state: "completed".into(),
                },
            },
        ];
        let names: BTreeSet<String> = said.iter().map(round_trip).collect();
        assert_eq!(names, variants::<acp::Event>());
    }
}
