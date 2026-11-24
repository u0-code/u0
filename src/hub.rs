//! The agents the desk runs, and the work done with them off the window's
//! thread.
//!
//! Each agent switched on has a [`Slot`]: what the registry says of it,
//! whether it can be started, and once started its connection and what it
//! said it can do. Starting an agent, opening a session, a turn, an install:
//! each runs on the ACP runtime and comes back to the window as a [`Msg`]
//! over one channel, which the desk reads in order. What an agent says on
//! its own comes back the same way.

use std::{path::PathBuf, sync::Arc};

use agent_client_protocol_schema::{
    ProtocolVersion,
    v1::{
        AuthenticateRequest, CancelNotification, ClientCapabilities, ContentBlock, Implementation,
        InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PermissionOptionId, PromptRequest, PromptResponse, RequestPermissionOutcome,
        RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigId, SessionConfigOption,
        SessionConfigOptionValue, SessionConfigValueId, SessionId, SessionModeId,
        SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
        StopReason, TextContent,
    },
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    acp::{self, Agent, Failure},
    git,
    registry::{self, Entry, Missing},
};

/// What comes back to the window.
pub enum Msg {
    Registry(Result<registry::Registry, String>),
    Installing {
        agent: String,
        stage: String,
    },
    Installed {
        agent: String,
        result: Result<(), String>,
    },
    Started {
        agent: String,
        result: Result<(Arc<Agent>, Box<InitializeResponse>), Failure>,
    },
    Opened {
        key: u64,
        result: Result<Box<NewSessionResponse>, Failure>,
    },
    Ended {
        key: u64,
        result: Result<StopReason, String>,
    },
    /// Something an agent said on its own.
    Said {
        agent: String,
        event: Box<acp::Event>,
    },
    /// Signing in to `agent` is done, or failed: why.
    Signed {
        agent: String,
        result: Result<(), String>,
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
    /// A worktree was made for the session, or could not be.
    Worktree {
        key: u64,
        result: Result<git::Worktree, String>,
    },
    /// A turn's checkpoint was taken; after it, what changed. `then` is
    /// what to say once the checkpoint before a turn is taken.
    Checkpoint {
        key: u64,
        turn: usize,
        after: bool,
        result: Result<Snapshot, String>,
        then: Option<String>,
    },
    /// A turn was undone (or put back): the checkpoint that reverses it,
    /// and what changed in all now.
    Restored {
        key: u64,
        turn: usize,
        undo: bool,
        result: Result<(String, Option<Vec<git::FileDiff>>), String>,
    },
    /// A commit, a push, a pull request or a worktree removed: what it said.
    Git {
        key: u64,
        action: GitAction,
        result: Result<String, String>,
    },
}

/// Where an agent stands.
#[derive(Clone)]
pub enum Stage {
    /// It cannot be started yet.
    Missing(Missing),
    Installing(String),
    /// It can be started, and is not running.
    Ready,
    Starting,
    Running {
        agent: Arc<Agent>,
        init: Box<InitializeResponse>,
    },
    /// It stopped, or could not be started: why.
    Failed(String),
}

pub struct Slot {
    pub entry: Entry,
    pub stage: Stage,
}

impl Slot {
    /// A slot for `entry`, ready or missing what it needs.
    pub fn new(entry: Entry, home: &std::path::Path) -> Self {
        let stage = match entry.launch(home) {
            Ok(_) => Stage::Ready,
            Err(m) => Stage::Missing(m),
        };
        Self { entry, stage }
    }

    pub fn running(&self) -> Option<&Arc<Agent>> {
        match &self.stage {
            Stage::Running { agent, .. } => Some(agent),
            _ => None,
        }
    }
}

pub type Sender = mpsc::UnboundedSender<Msg>;

/// A checkpoint, and what changed up to it.
pub struct Snapshot {
    pub commit: String,
    /// From the turn's checkpoint before to this one.
    pub turn: Option<Vec<git::FileDiff>>,
    /// From the session's first checkpoint to this one.
    pub total: Option<Vec<git::FileDiff>>,
}

/// What you ask of git from the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAction {
    Commit(String),
    CommitPush(String),
    /// Commit what is left with this message, push, and open a pull request.
    PullRequest(String),
    RemoveWorktree(git::Worktree),
}

/// Read the newest registry.
pub fn fetch(home: PathBuf, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let _ = tx.send(Msg::Registry(registry::fetch(&home)));
    });
}

/// Install `entry` from its archive.
pub fn install(entry: Entry, home: PathBuf, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let agent = entry.id.clone();
        let say = |stage: &str| {
            let _ = tx.send(Msg::Installing {
                agent: agent.clone(),
                stage: stage.to_owned(),
            });
        };
        let result = registry::install(&entry, &home, &say);
        let _ = tx.send(Msg::Installed { agent, result });
    });
}

/// Start `entry`'s program and say hello to it.
pub fn start(entry: &Entry, home: &std::path::Path, tx: Sender) {
    let agent = entry.id.clone();
    let launch = match entry.launch(home) {
        Ok(l) => l,
        Err(m) => {
            let _ = tx.send(Msg::Started {
                agent,
                result: Err(Failure {
                    code: None,
                    message: m.say(&entry.name),
                }),
            });
            return;
        }
    };
    let (events, mut said) = mpsc::unbounded_channel();
    let forward = tx.clone();
    let id = agent.clone();
    acp::runtime().spawn(async move {
        while let Some(event) = said.recv().await {
            if forward
                .send(Msg::Said {
                    agent: id.clone(),
                    event: Box::new(event),
                })
                .is_err()
            {
                break;
            }
        }
    });
    let running = match Agent::start(&launch, events) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            let _ = tx.send(Msg::Started {
                agent,
                result: Err(Failure {
                    code: None,
                    message: format!("{} could not be started: {e}", launch.program.display()),
                }),
            });
            return;
        }
    };
    acp::runtime().spawn(async move {
        let hello = InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(ClientCapabilities::new())
            .client_info(Implementation::new("ghost", env!("CARGO_PKG_VERSION")));
        let result = running
            .request::<_, InitializeResponse>("initialize", hello)
            .await
            .map(|init| (running, Box::new(init)));
        let _ = tx.send(Msg::Started { agent, result });
    });
}

/// Open a new session in `folder`.
pub fn open(agent: Arc<Agent>, key: u64, folder: PathBuf, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, NewSessionResponse>("session/new", NewSessionRequest::new(folder))
            .await
            .map(Box::new);
        let _ = tx.send(Msg::Opened { key, result });
    });
}

/// Say `text` in the session `id`, and wait for the turn to end.
pub fn prompt(agent: Arc<Agent>, key: u64, id: SessionId, text: String, tx: Sender) {
    acp::runtime().spawn(async move {
        let request = PromptRequest::new(id, vec![ContentBlock::Text(TextContent::new(text))]);
        let result = agent
            .request::<_, PromptResponse>("session/prompt", request)
            .await
            .map(|r| r.stop_reason)
            .map_err(|f| match f.code {
                Some(acp::AUTH_REQUIRED) => format!(
                    "{} The agent needs you to sign in: run its own CLI once in a terminal and sign in there, then try again.",
                    f.message
                ),
                _ => f.message,
            });
        let _ = tx.send(Msg::Ended { key, result });
    });
}

/// Stop the turn going on in `id`.
pub fn cancel(agent: &Agent, id: SessionId) {
    agent.notify("session/cancel", CancelNotification::new(id));
}

/// Answer the question asked under `request` with `option`, or say it was
/// cancelled.
pub fn answer(agent: &Agent, request: &Value, option: Option<String>) {
    let outcome = match option {
        Some(o) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            PermissionOptionId::new(o),
        )),
        None => RequestPermissionOutcome::Cancelled,
    };
    agent.respond(request, RequestPermissionResponse::new(outcome));
}

/// Put the session in another mode.
pub fn set_mode(agent: Arc<Agent>, key: u64, id: SessionId, mode: String, tx: Sender) {
    acp::runtime().spawn(async move {
        let request = SetSessionModeRequest::new(id, SessionModeId::new(mode));
        if let Err(f) = agent.request::<_, Value>("session/set_mode", request).await {
            let _ = tx.send(Msg::Refused {
                key,
                why: f.message,
            });
        }
    });
}

/// Choose `value` for the option `config` (a model, a level of thought).
pub fn set_option(
    agent: Arc<Agent>,
    key: u64,
    id: SessionId,
    config: String,
    value: String,
    tx: Sender,
) {
    acp::runtime().spawn(async move {
        let request = SetSessionConfigOptionRequest::new(
            id,
            SessionConfigId::new(config),
            SessionConfigOptionValue::value_id(SessionConfigValueId::new(value)),
        );
        let msg = match agent
            .request::<_, SetSessionConfigOptionResponse>("session/set_config_option", request)
            .await
        {
            Ok(r) => Msg::Options {
                key,
                options: r.config_options,
            },
            Err(f) => Msg::Refused {
                key,
                why: f.message,
            },
        };
        let _ = tx.send(msg);
    });
}

/// Sign in to `agent` (the registry id `id`) with its method `method`: the
/// agent itself does it, often in your browser.
pub fn authenticate(agent: Arc<Agent>, id: String, method: String, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, Value>("authenticate", AuthenticateRequest::new(method))
            .await
            .map(|_| ())
            .map_err(|f| f.message);
        let _ = tx.send(Msg::Signed { agent: id, result });
    });
}

/// Run `program` with `args` and `env` in a Terminal window, for a sign-in
/// that is done by typing: the agent's own login, which it says how to run.
pub fn in_terminal(
    program: &std::path::Path,
    args: &[String],
    env: &[(String, String)],
) -> std::io::Result<()> {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut parts: Vec<String> = env
        .iter()
        .map(|(k, v)| format!("{k}={}", quote(v)))
        .collect();
    parts.push(quote(&program.to_string_lossy()));
    parts.extend(args.iter().map(|a| quote(a)));
    let line = parts.join(" ");
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate",
        line.replace('\\', "\\\\").replace('"', "\\\"")
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .map(|_| ())
}

/// Make a worktree for the session `key`, about `about`.
pub fn make_worktree(dir: PathBuf, about: String, home: PathBuf, key: u64, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let result = git::worktree(&dir, &about, &home);
        let _ = tx.send(Msg::Worktree { key, result });
    });
}

/// What a checkpoint is to take, and what to compare it with.
pub struct Take {
    pub dir: PathBuf,
    pub name: String,
    pub key: u64,
    pub turn: usize,
    pub after: bool,
    /// The turn's checkpoint before, and the session's first.
    pub before: Option<String>,
    pub first: Option<String>,
    pub then: Option<String>,
}

/// Take a checkpoint; after a turn, read what changed in it and in all.
pub fn checkpoint(take: Take, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let result = git::checkpoint(&take.dir, &take.name).map(|commit| {
            let diff = |from: &Option<String>| {
                from.as_deref()
                    .filter(|_| take.after)
                    .and_then(|f| git::diff(&take.dir, f, &commit).ok())
            };
            Snapshot {
                turn: diff(&take.before),
                total: diff(&take.first),
                commit,
            }
        });
        let _ = tx.send(Msg::Checkpoint {
            key: take.key,
            turn: take.turn,
            after: take.after,
            result,
            then: take.then,
        });
    });
}

/// Put the session's files back as `target` had them.
#[allow(clippy::too_many_arguments, reason = "each is one fact of the restore")]
pub fn restore(
    dir: PathBuf,
    target: String,
    name: String,
    first: Option<String>,
    key: u64,
    turn: usize,
    undo: bool,
    tx: Sender,
) {
    acp::runtime().spawn_blocking(move || {
        let result = git::restore(&dir, &target, &name).map(|back| {
            let total = first.and_then(|f| {
                let now = git::checkpoint(&dir, &format!("{name}-now")).ok()?;
                git::diff(&dir, &f, &now).ok()
            });
            (back, total)
        });
        let _ = tx.send(Msg::Restored {
            key,
            turn,
            undo,
            result,
        });
    });
}

/// Do `action` in `dir`.
pub fn git_action(dir: PathBuf, action: GitAction, key: u64, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let result = match &action {
            GitAction::Commit(m) => git::commit(&dir, m).map(|id| format!("Committed {id}")),
            GitAction::CommitPush(m) => git::commit(&dir, m)
                .and_then(|id| git::push(&dir).map(|_| format!("Committed {id} and pushed"))),
            GitAction::PullRequest(m) => {
                // Nothing left to commit is no reason to stop.
                let _ = git::commit(&dir, m);
                git::push(&dir).and_then(|_| git::pull_request(&dir))
            }
            GitAction::RemoveWorktree(wt) => git::remove_worktree(wt)
                .map(|()| format!("Worktree removed; the branch {} is kept", wt.branch)),
        };
        let _ = tx.send(Msg::Git {
            key,
            action,
            result,
        });
    });
}
