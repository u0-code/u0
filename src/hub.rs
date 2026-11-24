//! The agents the desk runs, and the work done with them off the window's
//! thread.
//!
//! Each agent switched on has a [`Slot`]: what the registry says of it,
//! whether it can be started, and once started its connection and what it
//! said it can do. Starting an agent, opening a session, a turn, an install:
//! each runs on the ACP runtime and comes back to the window as a [`Msg`]
//! over one channel, which the desk reads in order. What an agent says on
//! its own comes back the same way.

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use agent_client_protocol_schema::{
    ProtocolVersion,
    v1::{
        AuthenticateRequest, CancelNotification, ClientCapabilities, CloseSessionRequest,
        ContentBlock, DeleteSessionRequest, ForkSessionRequest, ForkSessionResponse,
        Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
        ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, LogoutRequest,
        NewSessionRequest, NewSessionResponse, PermissionOptionId, PromptRequest, PromptResponse,
        RequestPermissionOutcome, RequestPermissionResponse, ResumeSessionRequest,
        ResumeSessionResponse, SelectedPermissionOutcome, SessionConfigId, SessionConfigOption,
        SessionConfigOptionValue, SessionConfigValueId, SessionId, SessionInfo, SessionModeId,
        SessionModeState, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
        SetSessionModeRequest, StopReason, TextContent,
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
        what: String,
        share: Option<f32>,
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
    /// The sessions `agent` keeps in the projects, each with its project.
    Listed {
        agent: String,
        result: Result<Vec<(PathBuf, SessionInfo)>, String>,
    },
    /// A session the agent keeps was loaded: what was said in it came
    /// before this, as updates.
    Loaded {
        key: u64,
        /// Resumed from the desk's record, not loaded with what was said.
        resumed: bool,
        result: Result<Opened, Failure>,
    },
    /// What is not committed where a session works, as git reads it.
    Uncommitted {
        key: u64,
        result: Result<Vec<git::FileDiff>, String>,
    },
    /// What a session of `agent` offers before it is opened, or why the
    /// agent would not open one.
    Probed {
        agent: String,
        result: Result<Box<NewSessionResponse>, Failure>,
    },
    /// A session branched off the session `from`, or why it was not.
    Forked {
        from: u64,
        result: Result<Box<ForkSessionResponse>, Failure>,
    },
    /// The files of a project, for `@` to name.
    Files {
        folder: PathBuf,
        files: Vec<String>,
    },
    /// The open pull requests of a project, for `@` to name.
    Prs {
        folder: PathBuf,
        prs: Vec<(u64, String)>,
    },
    /// What `gh` says of the pull request `number`, named with `@`.
    PrText {
        number: u64,
        text: Option<String>,
    },
    /// The sessions whose records hold `query`.
    Found {
        query: String,
        keys: Vec<u64>,
    },
    /// A session was deleted from its agent, or could not be.
    Deleted {
        key: u64,
        result: Result<(), String>,
    },
    Ended {
        key: u64,
        result: Result<StopReason, Failure>,
    },
    /// An agent signed out, or could not.
    LoggedOut {
        agent: String,
        result: Result<(), String>,
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
    Installing(Install),
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

/// An install going on: what it is doing, how far it is (0 to 1) when
/// known, and the switch that stops it.
#[derive(Clone)]
pub struct Install {
    pub what: String,
    pub share: Option<f32>,
    pub stop: Arc<AtomicBool>,
}

pub struct Slot {
    pub entry: Entry,
    pub stage: Stage,
    /// What a session of it offers before one is opened: read from a
    /// session opened and deleted again as it starts.
    pub probe: Option<Probe>,
    pub auth: Auth,
    /// When it last did anything: one idle long enough is let go, and
    /// started again when it is wanted.
    pub used: std::time::Instant,
    /// The commands its sessions take, as the probe's session listed them.
    pub commands: Vec<agent_client_protocol_schema::v1::AvailableCommand>,
}

/// The modes and options a session of an agent starts with.
#[derive(Clone)]
pub struct Probe {
    pub modes: Option<SessionModeState>,
    pub options: Vec<SessionConfigOption>,
}

/// Whether an agent is signed in, as far as the desk can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auth {
    /// Not known yet.
    Unknown,
    /// It opened a session without asking.
    Ready,
    /// It asked to be signed in, or said its sign-in failed: what it said.
    Needed(String),
}

impl Slot {
    /// A slot for `entry`, ready or missing what it needs.
    pub fn new(entry: Entry, home: &std::path::Path) -> Self {
        let stage = match entry.launch(home) {
            Ok(_) => Stage::Ready,
            Err(m) => Stage::Missing(m),
        };
        Self {
            entry,
            stage,
            probe: None,
            auth: Auth::Unknown,
            used: std::time::Instant::now(),
            commands: Vec::new(),
        }
    }

    /// Give the draft `s` the modes and options a session of this agent
    /// starts with, so they can be chosen before it is opened.
    pub fn seed(&self, s: &mut crate::session::Session) {
        if let Some(p) = &self.probe
            && s.id.is_none()
        {
            s.modes.clone_from(&p.modes);
            s.options.clone_from(&p.options);
        }
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

/// Install `entry` from its archive, until `stop` is set.
pub fn install(entry: Entry, home: PathBuf, stop: Arc<AtomicBool>, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let agent = entry.id.clone();
        let say = |what: &str, share: Option<f32>| {
            let _ = tx.send(Msg::Installing {
                agent: agent.clone(),
                what: what.to_owned(),
                share,
            });
        };
        let result = registry::install(&entry, &home, &say, &stop);
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

/// How many pages of a project's sessions are read from an agent at most:
/// some page through all they keep, the project's few among them.
const PAGES: usize = 50;

/// The sessions `agent` keeps in each of `projects` and in Ghost's
/// worktrees of them (under `home`), each with the project it belongs to.
pub fn list(agent: Arc<Agent>, name: String, projects: Vec<PathBuf>, home: PathBuf, tx: Sender) {
    acp::runtime().spawn(async move {
        let under = home.join("worktrees");
        let mut places: Vec<(PathBuf, PathBuf)> = Vec::new();
        for project in projects {
            let p = project.clone();
            let u = under.clone();
            let worktrees = tokio::task::spawn_blocking(move || git::worktrees(&p, &u))
                .await
                .unwrap_or_default();
            places.push((project.clone(), project.clone()));
            places.extend(worktrees.into_iter().map(|w| (w.path, project.clone())));
        }
        let mut found = Vec::new();
        for (cwd, project) in places {
            let mut cursor = None;
            for _ in 0..PAGES {
                let request = ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor);
                match agent
                    .request::<_, ListSessionsResponse>("session/list", request)
                    .await
                {
                    Ok(page) => {
                        found.extend(page.sessions.into_iter().map(|s| (project.clone(), s)));
                        cursor = page.next_cursor;
                        if cursor.is_none() {
                            break;
                        }
                    }
                    Err(f) => {
                        let _ = tx.send(Msg::Listed {
                            agent: name,
                            result: Err(f.message),
                        });
                        return;
                    }
                }
            }
        }
        let _ = tx.send(Msg::Listed {
            agent: name,
            result: Ok(found),
        });
    });
}

/// What a session loaded or resumed starts with.
pub struct Opened {
    pub modes: Option<SessionModeState>,
    pub options: Option<Vec<SessionConfigOption>>,
}

/// Load the session `id` the agent keeps, in `folder`: it says again all
/// that was said in it, as updates.
pub fn load(agent: Arc<Agent>, key: u64, id: SessionId, folder: PathBuf, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, LoadSessionResponse>("session/load", LoadSessionRequest::new(id, folder))
            .await
            .map(|r| Opened {
                modes: r.modes,
                options: r.config_options,
            });
        let _ = tx.send(Msg::Loaded {
            key,
            resumed: false,
            result,
        });
    });
}

/// Resume the session `id` the agent keeps, in `folder`, without what was
/// said in it: the desk has that already.
pub fn resume(agent: Arc<Agent>, key: u64, id: SessionId, folder: PathBuf, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, ResumeSessionResponse>(
                "session/resume",
                ResumeSessionRequest::new(id, folder),
            )
            .await
            .map(|r| Opened {
                modes: r.modes,
                options: r.config_options,
            });
        let _ = tx.send(Msg::Loaded {
            key,
            resumed: true,
            result,
        });
    });
}

/// Open a session of `agent` in `dir` to read what a session offers, then
/// delete it (or close it) again, so that nothing of it is kept.
pub fn probe(agent: Arc<Agent>, name: String, dir: PathBuf, delete: bool, close: bool, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, NewSessionResponse>("session/new", NewSessionRequest::new(dir))
            .await;
        if let Ok(r) = &result {
            let id = r.session_id.clone();
            if delete {
                let _ = agent
                    .request::<_, Value>("session/delete", DeleteSessionRequest::new(id))
                    .await;
            } else if close {
                let _ = agent
                    .request::<_, Value>("session/close", CloseSessionRequest::new(id))
                    .await;
            }
        }
        let _ = tx.send(Msg::Probed {
            agent: name,
            result: result.map(Box::new),
        });
    });
}

/// Branch the session `id` off into a new one, in `folder`, with all that
/// was said in it.
pub fn fork(agent: Arc<Agent>, from: u64, id: SessionId, folder: PathBuf, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, ForkSessionResponse>("session/fork", ForkSessionRequest::new(id, folder))
            .await
            .map(Box::new);
        let _ = tx.send(Msg::Forked { from, result });
    });
}

/// Read the files of `folder` that git sees (tracked, and new ones not
/// ignored), for `@` to name.
pub fn list_files(folder: PathBuf, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let files = git::files(&folder);
        let _ = tx.send(Msg::Files { folder, files });
    });
}

/// Read the open pull requests of `folder` with `gh`, for `@` to name.
pub fn list_prs(folder: PathBuf, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let prs = git::pull_requests(&folder);
        let _ = tx.send(Msg::Prs { folder, prs });
    });
}

/// Look for `query` (lower case) in the records at `paths`.
pub fn search(query: String, paths: Vec<(u64, PathBuf)>, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let keys = paths
            .into_iter()
            .filter(|(_, p)| {
                std::fs::read_to_string(p).is_ok_and(|t| t.to_lowercase().contains(&query))
            })
            .map(|(k, _)| k)
            .collect();
        let _ = tx.send(Msg::Found { query, keys });
    });
}

/// Read what is not committed in `dir`.
pub fn uncommitted(dir: PathBuf, key: u64, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let _ = tx.send(Msg::Uncommitted {
            key,
            result: git::uncommitted(&dir),
        });
    });
}

/// Delete the session `id` from its agent.
pub fn delete(agent: Arc<Agent>, key: u64, id: SessionId, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, Value>("session/delete", DeleteSessionRequest::new(id))
            .await
            .map(|_| ())
            .map_err(|f| f.message);
        let _ = tx.send(Msg::Deleted { key, result });
    });
}

/// Say `text` in the session `id`, and wait for the turn to end.
pub fn prompt(
    agent: Arc<Agent>,
    key: u64,
    id: SessionId,
    text: String,
    attached: Vec<ContentBlock>,
    tx: Sender,
) {
    acp::runtime().spawn(async move {
        let mut content = vec![ContentBlock::Text(TextContent::new(text))];
        content.extend(attached);
        let request = PromptRequest::new(id, content);
        let result = agent
            .request::<_, PromptResponse>("session/prompt", request)
            .await
            .map(|r| r.stop_reason);
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

/// Sign `agent` out.
pub fn logout(agent: Arc<Agent>, name: String, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent
            .request::<_, Value>("logout", LogoutRequest::new())
            .await
            .map(|_| ())
            .map_err(|f| f.message);
        let _ = tx.send(Msg::LoggedOut {
            agent: name,
            result,
        });
    });
}

/// Show a notification from macOS: a turn ended, or a question is asked,
/// while Ghost is not in front.
pub fn notify(title: &str, body: &str) {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"Ghost\" subtitle \"{}\"",
        quote(body),
        quote(title)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
