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

use agent_client_protocol::{
    JsonRpcRequest,
    schema::ProtocolVersion,
    v1::schema::{
        AgentCapabilities, AuthMethod, AuthenticateRequest, CancelNotification, ClientCapabilities,
        CloseSessionRequest, ContentBlock, DeleteSessionRequest, ForkSessionRequest,
        ForkSessionResponse, Implementation, InitializeRequest, InitializeResponse,
        ListSessionsRequest, LoadSessionRequest, LogoutRequest, MessageId, NewSessionRequest,
        NewSessionResponse, PermissionOptionId, PromptRequest, RequestPermissionOutcome,
        RequestPermissionResponse, ResumeSessionRequest, SelectedPermissionOutcome,
        SessionConfigId, SessionConfigOption, SessionConfigOptionValue, SessionConfigValueId,
        SessionId, SessionInfo, SessionModeId, SessionModeState, SetSessionConfigOptionRequest,
        SetSessionModeRequest, StopReason, SubagentCapabilities, TextContent, ToolCallId,
    },
};
use tokio::sync::mpsc;

use crate::{
    acp::{self, Agent, Error},
    desk::runs::{self, Script},
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
        result: Result<(Arc<Agent>, Box<InitializeResponse>), Error>,
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
    /// A session the agent keeps was loaded: what was said in it came
    /// before this, as updates.
    Loaded {
        key: u64,
        /// Resumed from the desk's record, not loaded with what was said.
        resumed: bool,
        result: Result<Opened, Error>,
    },
    /// What git says of `folder`, where the session `key` works: its
    /// branch, whether it is in a repository, and the scripts the
    /// repository's `ghost.json` names.
    Looked {
        key: u64,
        folder: PathBuf,
        branch: Option<String>,
        repo: bool,
        scripts: Vec<Script>,
    },
    /// What the folder of a project holds.
    Held {
        folder: PathBuf,
        held: Held,
    },
    /// Something done off the window's thread, to be said: what was done,
    /// or what was not and why.
    Notice(Result<String, (String, String)>),
    /// Where the diffs of calls of the session `key` start in their files.
    Placed {
        key: u64,
        starts: Vec<(ToolCallId, Vec<Option<usize>>)>,
    },
    /// The record of the session `key`, read back: none where there is none.
    Record {
        key: u64,
        events: Option<Vec<crate::store::Event>>,
    },
    /// What is not committed where a session works, as git reads it.
    Uncommitted {
        key: u64,
        result: Result<Vec<git::FileDiff>, String>,
    },
    /// A terminal's screen changed; a terminal's shell ended.
    Terminal(u64),
    TerminalEnded(u64),
    /// The folders `agent` has sessions in that are not projects, with how
    /// many in each.
    Elsewhere {
        agent: String,
        folders: Vec<(PathBuf, usize)>,
    },
    /// What a CLI of yours says its version is.
    CliVersion {
        cli: PathBuf,
        version: String,
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
    /// A window you chose was captured, to go with what you write: the
    /// picture's bytes, or nothing if you let it be.
    Captured(Option<Vec<u8>>),
    /// What git grep found for `words`.
    Grepped {
        words: String,
        found: Vec<(String, String)>,
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
        issues: Vec<(u64, String)>,
    },
    /// What changed in a scope git is asked for: the branch against its
    /// base, or one commit.
    Diffed {
        key: u64,
        scope: git::Scope,
        result: Result<Vec<git::FileDiff>, String>,
    },
    /// The recent commits and the local branches where a session works.
    Commits {
        key: u64,
        commits: Vec<(String, String, String)>,
        branches: Vec<String>,
        /// How far behind its upstream, and whether it has an origin.
        upstream: (usize, bool),
    },
    /// The pull request of the branch a session works on, as `gh` says.
    Pr {
        key: u64,
        pr: Option<git::Pr>,
    },
    /// A step of a git action on its way.
    GitStep {
        key: u64,
        text: String,
    },
    /// What `gh` says of the pull request `number`, named with `@`.
    PrText {
        number: u64,
        text: Option<String>,
    },
    /// How a clone goes, and how it (or a new project made) ended.
    Cloning {
        what: String,
        share: Option<f32>,
    },
    Cloned {
        dir: PathBuf,
        result: Result<(), String>,
    },
    /// How much memory each running agent holds: its id, process id and
    /// kilobytes.
    Weighed(Vec<(String, u32, u64)>),
    /// How much each part of Ghost's home takes, and what a tidying did.
    Sizes(Vec<(&'static str, u64)>),
    Tidied(String),
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
        result: Result<StopReason, Error>,
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
    /// A worktree was made for the session, with the scripts its
    /// `ghost.json` names, or could not be.
    Worktree {
        key: u64,
        result: Result<(git::Worktree, Vec<Script>), String>,
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
    pub commands: Vec<agent_client_protocol::v1::schema::AvailableCommand>,
    /// A command line of yours that starts it instead of the registry's.
    pub command: Option<String>,
    /// What it said of itself when it last started (what it can do, how
    /// it signs in): kept while it is let go; none before it first starts.
    pub known: Option<Box<InitializeResponse>>,
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
    pub fn new(entry: Entry, home: &std::path::Path, command: Option<String>) -> Self {
        let stage = match registry::launch_with(&entry, home, command.as_deref()) {
            Ok(_) => Stage::Ready,
            Err(m) => Stage::Missing(m),
        };
        Self {
            command,
            entry,
            stage,
            probe: None,
            auth: Auth::Unknown,
            used: std::time::Instant::now(),
            commands: Vec::new(),
            known: None,
        }
    }

    /// Whether a session can be begun with it now: running, starting, or
    /// ready to start.
    pub fn usable(&self) -> bool {
        matches!(
            self.stage,
            Stage::Ready | Stage::Starting | Stage::Running { .. }
        )
    }

    pub fn launch(&self, home: &std::path::Path) -> Result<acp::Launch, Missing> {
        registry::launch_with(&self.entry, home, self.command.as_deref())
    }

    /// Where it stands now that it can (or cannot) be started; one running
    /// or on its way is left as it is.
    pub fn restage(&mut self, home: &std::path::Path) {
        if matches!(
            self.stage,
            Stage::Missing(_) | Stage::Ready | Stage::Failed(_)
        ) {
            self.stage = match self.launch(home) {
                Ok(_) => Stage::Ready,
                Err(m) => Stage::Missing(m),
            };
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

    /// What it can do, as it said when it last started.
    pub fn caps(&self) -> Option<&AgentCapabilities> {
        self.known.as_ref().map(|k| &k.agent_capabilities)
    }

    /// The ways it offers, while it runs, to sign in that Ghost can take:
    /// done by the agent itself, or typed in a terminal.
    pub fn sign_ins(&self) -> impl Iterator<Item = &AuthMethod> {
        self.running()
            .and(self.known.as_ref())
            .into_iter()
            .flat_map(|k| &k.auth_methods)
            .filter(|m| matches!(m, AuthMethod::Agent(_) | AuthMethod::Terminal(_)))
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
    /// Check out this branch.
    Switch(String),
    /// Rename the worktree's branch, unless it is pushed.
    RenameBranch(String),
    /// Merge the branch's pull request (squashed), as gh does.
    Merge,
    /// Make this branch, from where it is, and check it out.
    NewBranch(String),
    /// Bring in what the upstream has.
    Pull,
    /// Make the folder a repository.
    Init,
    /// Put the repository on GitHub, private.
    Publish,
}

impl GitAction {
    /// What it does, as "Couldn't …" ends.
    pub fn doing(&self) -> &'static str {
        match self {
            Self::Commit(_) => "commit",
            Self::CommitPush(_) => "commit and push",
            Self::PullRequest(_) => "open the pull request",
            Self::RemoveWorktree(_) => "remove the worktree",
            Self::Switch(_) => "switch the branch",
            Self::RenameBranch(_) => "rename the branch",
            Self::Merge => "merge the pull request",
            Self::Pull => "pull",
            Self::NewBranch(_) => "make the branch",
            Self::Init => "make it a repository",
            Self::Publish => "put it on GitHub",
        }
    }
}

/// Do `work` off the window's thread, and send what it comes to back.
fn off(tx: Sender, work: impl FnOnce() -> Msg + Send + 'static) {
    acp::runtime().spawn_blocking(move || {
        let _ = tx.send(work());
    });
}

/// Ask `agent` `request`, and send what `then` makes of the answer back.
fn ask<R>(
    agent: Arc<Agent>,
    request: R,
    tx: Sender,
    then: impl FnOnce(Result<R::Response, Error>) -> Msg + Send + 'static,
) where
    R: JsonRpcRequest + Send + 'static,
    R::Response: Send,
{
    acp::runtime().spawn(async move {
        let _ = tx.send(then(agent.request(request).await));
    });
}

/// Read the newest registry.
pub fn fetch(home: PathBuf, tx: Sender) {
    off(tx, move || Msg::Registry(registry::fetch(&home)));
}

/// Install `entry` from its archive, until `stop` is set.
pub fn install(entry: Entry, home: PathBuf, stop: Arc<AtomicBool>, tx: Sender) {
    off(tx.clone(), move || {
        let agent = entry.id.clone();
        let say = |what: &str, share: Option<f32>| {
            let _ = tx.send(Msg::Installing {
                agent: agent.clone(),
                what: what.to_owned(),
                share,
            });
        };
        let result = registry::install(&entry, &home, &say, &stop);
        Msg::Installed { agent, result }
    });
}

/// Start `slot`'s program and say hello to it. What it says on its own
/// comes back as [`Msg::Said`], in order with the answers to the desk.
pub fn start(slot: &Slot, home: &std::path::Path, tx: Sender) {
    let entry = &slot.entry;
    let agent = entry.id.clone();
    let launch = match slot.launch(home) {
        Ok(l) => l,
        Err(m) => {
            let _ = tx.send(Msg::Started {
                agent,
                result: Err(Error::internal_error().data(m.say(&entry.name))),
            });
            return;
        }
    };
    acp::runtime().spawn(async move {
        let (said, id) = (tx.clone(), agent.clone());
        let result = match Agent::start(&launch, move |event| {
            let _ = said.send(Msg::Said {
                agent: id.clone(),
                event: Box::new(event),
            });
        })
        .await
        {
            Ok(running) => {
                let hello = InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(
                        ClientCapabilities::new()
                            .subagents(SubagentCapabilities::new())
                            // Tasks left going in the background (a
                            // command), as the JetBrains extension says
                            // them, are shown.
                            .meta(serde_json::Map::from_iter([(
                                "jetbrains".into(),
                                serde_json::json!({"air": {"version": 1, "capabilities": ["asyncTasks"]}}),
                            )])),
                    )
                    .client_info(Implementation::new("ghost", env!("CARGO_PKG_VERSION")));
                running
                    .request(hello)
                    .await
                    .map(|init| (Arc::new(running), Box::new(init)))
            }
            Err(e) => Err(e),
        };
        let _ = tx.send(Msg::Started { agent, result });
    });
}

/// Open a new session in `folder`, with the folders `also` beside it.
pub fn open(agent: Arc<Agent>, key: u64, (folder, also): (PathBuf, Vec<PathBuf>), tx: Sender) {
    let request = NewSessionRequest::new(folder).additional_directories(also);
    ask(agent, request, tx, move |r| Msg::Opened {
        key,
        result: r.map(Box::new),
    });
}

/// How many pages of a project's sessions are read from an agent at most:
/// some page through all they keep, the project's few among them.
const PAGES: usize = 50;

/// The sessions `agent` keeps in each of `projects` and in Ghost's
/// worktrees of them (under `home`), each with the project it belongs to.
pub fn list(
    agent: Arc<Agent>,
    name: String,
    projects: Vec<(PathBuf, PathBuf)>,
    home: PathBuf,
    tx: Sender,
) {
    acp::runtime().spawn(async move {
        let under = home.join("worktrees");
        let mut places: Vec<(PathBuf, PathBuf)> = Vec::new();
        // Each project's folder (on another machine, its path there), and
        // here, Ghost's worktrees of it.
        for (cwd, project) in projects {
            places.push((cwd.clone(), project.clone()));
            if cwd != project {
                continue;
            }
            let p = project.clone();
            let u = under.clone();
            let worktrees = tokio::task::spawn_blocking(move || git::worktrees(&p, &u))
                .await
                .unwrap_or_default();
            places.extend(worktrees.into_iter().map(|w| (w.path, project.clone())));
        }
        let mut found = Vec::new();
        for (cwd, project) in places {
            let mut cursor = None;
            for _ in 0..PAGES {
                let request = ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor);
                match agent.request(request).await {
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
        // The folders it has sessions in that are not projects (nor Ghost's
        // own), each with how many: what could be added.
        let known: Vec<PathBuf> = found.iter().map(|(_, s)| s.cwd.clone()).collect();
        let mut elsewhere = std::collections::BTreeMap::<PathBuf, usize>::default();
        let mut cursor = None;
        for _ in 0..PAGES / 5 {
            let request = ListSessionsRequest::new().cursor(cursor);
            let Ok(page) = agent.request(request).await else {
                break;
            };
            for s in page.sessions {
                if !known.contains(&s.cwd) && !s.cwd.starts_with(&home) {
                    *elsewhere.entry(s.cwd).or_default() += 1;
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let _ = tx.send(Msg::Listed {
            agent: name.clone(),
            result: Ok(found),
        });
        let _ = tx.send(Msg::Elsewhere {
            agent: name,
            folders: elsewhere.into_iter().collect(),
        });
    });
}

/// What a session loaded or resumed starts with.
pub struct Opened {
    pub modes: Option<SessionModeState>,
    pub options: Option<Vec<SessionConfigOption>>,
}

/// Open again the session `id` the agent keeps, in `folder` (and `also`):
/// resumed, without what was said in it, which the desk has already; or
/// loaded, and the agent says again all that was said in it, as updates.
pub fn reopen(
    agent: Arc<Agent>,
    key: u64,
    id: SessionId,
    (folder, also): (PathBuf, Vec<PathBuf>),
    resumed: bool,
    tx: Sender,
) {
    let loaded = move |result| Msg::Loaded {
        key,
        resumed,
        result,
    };
    if resumed {
        let request = ResumeSessionRequest::new(id, folder).additional_directories(also);
        ask(agent, request, tx, move |r| {
            loaded(r.map(|r| Opened {
                modes: r.modes,
                options: r.config_options,
            }))
        });
    } else {
        let request = LoadSessionRequest::new(id, folder).additional_directories(also);
        ask(agent, request, tx, move |r| {
            loaded(r.map(|r| Opened {
                modes: r.modes,
                options: r.config_options,
            }))
        });
    }
}

/// Open a session of `agent` in `dir` to read what a session offers, then
/// delete it (or close it) again, so that nothing of it is kept.
pub fn probe(agent: Arc<Agent>, name: String, dir: PathBuf, delete: bool, close: bool, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = agent.request(NewSessionRequest::new(dir)).await;
        if let Ok(r) = &result {
            let id = r.session_id.clone();
            if delete {
                let _ = agent.request(DeleteSessionRequest::new(id)).await;
            } else if close {
                let _ = agent.request(CloseSessionRequest::new(id)).await;
            }
        }
        let _ = tx.send(Msg::Probed {
            agent: name,
            result: result.map(Box::new),
        });
    });
}

/// Branch the session `id` off into a new one, in `folder`: with all that
/// was said in it, or, given `upto` (an item of its thread and the last
/// message the fork keeps), with what was said before that item.
///
/// ACP's own fork point is a cursor, which agents announce only on v2. The
/// adapters of Claude Code and Codex read one from the request's `_meta` on
/// v1 as well: `jetbrains.air.fork`, the id of the last message to keep.
pub fn fork(
    agent: Arc<Agent>,
    from: u64,
    id: SessionId,
    folder: PathBuf,
    upto: Option<(usize, MessageId)>,
    tx: Sender,
) {
    let mut request = ForkSessionRequest::new(id, folder);
    if let Some((_, last)) = &upto {
        let point = serde_json::json!({"air": {"fork": {"version": 1, "messageId": last}}});
        request = request.meta(serde_json::Map::from_iter([("jetbrains".into(), point)]));
    }
    let at = upto.map(|(at, _)| at);
    ask(agent, request, tx, move |r| Msg::Forked {
        from,
        at,
        result: r.map(Box::new),
    });
}

/// Let you choose a window and capture it (macOS's screencapture), for
/// what you write.
pub fn capture_window(tx: Sender) {
    off(tx, || {
        let path = std::env::temp_dir().join(format!("ghost-capture-{}.png", std::process::id()));
        let shot = std::process::Command::new("screencapture")
            .args(["-i", "-W", "-x"])
            .arg(&path)
            .status();
        let bytes = shot
            .ok()
            .filter(std::process::ExitStatus::success)
            .and_then(|_| std::fs::read(&path).ok());
        let _ = std::fs::remove_file(&path);
        Msg::Captured(bytes)
    });
}

/// Look for `words` in the files of `folder` git sees (git grep), for the
/// palette: each place as "path:line", and the line.
pub fn grep(folder: PathBuf, words: String, tx: Sender) {
    off(tx, move || Msg::Grepped {
        found: git::grep(&folder, &words),
        words,
    });
}

/// Read the files of `folder` that git sees (tracked, and new ones not
/// ignored), for `@` to name.
pub fn list_files(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Files {
        files: git::files(&folder),
        folder,
    });
}

/// Ask `cli` its version (`--version`, its first line); none, if it does
/// not say.
pub fn cli_version(cli: PathBuf, tx: Sender) {
    off(tx, move || {
        let version = std::process::Command::new(&cli)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| {
                let said = String::from_utf8_lossy(&out.stdout);
                said.lines().next().unwrap_or_default().trim().to_owned()
            })
            .unwrap_or_default();
        Msg::CliVersion { cli, version }
    });
}

/// Read the open pull requests and issues of `folder` with `gh`, for `@`
/// to name and the palette to begin from.
pub fn list_prs(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Prs {
        prs: git::open_on_github(&folder, "pr"),
        issues: git::open_on_github(&folder, "issue"),
        folder,
    });
}

/// Make the new project `name` in `dir`: a folder, a repository.
pub fn create(dir: PathBuf, name: String, tx: Sender) {
    off(tx, move || Msg::Cloned {
        result: git::create(&dir, &name),
        dir,
    });
}

/// Clone `url` into `dir`, until `stop` is set.
pub fn clone(url: String, dir: PathBuf, stop: Arc<AtomicBool>, tx: Sender) {
    off(tx.clone(), move || {
        let say = |what: &str, share: Option<f32>| {
            let _ = tx.send(Msg::Cloning {
                what: what.to_owned(),
                share,
            });
        };
        let result = git::clone(&url, &dir, &say, &stop);
        Msg::Cloned { dir, result }
    });
}

/// What changed in `scope` (the branch against its base, or a commit)
/// where the session `key` works.
pub fn scope_diff(dir: PathBuf, key: u64, scope: git::Scope, base: Option<String>, tx: Sender) {
    off(tx, move || {
        let result = match &scope {
            git::Scope::Commit(id) => git::commit_diff(&dir, id),
            _ => git::against_base(&dir, base.as_deref()),
        };
        Msg::Diffed { key, scope, result }
    });
}

/// The recent commits and the local branches where the session `key` works.
pub fn commits(dir: PathBuf, key: u64, tx: Sender) {
    off(tx, move || Msg::Commits {
        key,
        commits: git::log(&dir, 12),
        branches: git::branches(&dir),
        upstream: git::upstream(&dir),
    });
}

/// Read the pull request of the branch where the session `key` works.
pub fn pr(dir: PathBuf, key: u64, tx: Sender) {
    off(tx, move || Msg::Pr {
        key,
        pr: git::read_pr(&dir),
    });
}

/// How much memory each of the `running` agents holds with what it
/// started (ps), by its id and process id: in kilobytes.
pub fn weigh(running: Vec<(String, u32)>, tx: Sender) {
    off(tx, move || {
        Msg::Weighed(
            running
                .into_iter()
                .filter_map(|(id, pid)| {
                    let out = std::process::Command::new("ps")
                        .args(["-o", "rss=", "-g", &pid.to_string()])
                        .output()
                        .ok()?;
                    let kb: u64 = String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .filter_map(|l| l.trim().parse::<u64>().ok())
                        .sum();
                    Some((id, pid, kb))
                })
                .collect(),
        )
    });
}

/// Measure Ghost's home.
pub fn sizes(home: PathBuf, tx: Sender) {
    off(tx, move || Msg::Sizes(crate::tidy::sizes(&home)));
}

/// Tidy `what` around what is kept, then measure again.
pub fn tidy(home: PathBuf, what: crate::tidy::Tidy, kept: crate::tidy::Kept, tx: Sender) {
    off(tx.clone(), move || {
        let _ = tx.send(Msg::Tidied(crate::tidy::tidy(&home, what, &kept)));
        Msg::Sizes(crate::tidy::sizes(&home))
    });
}

/// Look for `query` (lower case) in the records at `paths`.
pub fn search(query: String, paths: Vec<(u64, PathBuf)>, tx: Sender) {
    off(tx, move || {
        let keys = paths
            .into_iter()
            .filter(|(_, p)| {
                std::fs::read_to_string(p).is_ok_and(|t| t.to_lowercase().contains(&query))
            })
            .map(|(k, _)| k)
            .collect();
        Msg::Found { query, keys }
    });
}

/// Read the branch of `folder`, where the session `key` works, whether it
/// is in a repository, and the scripts its `ghost.json` names.
pub fn look(folder: PathBuf, key: u64, tx: Sender) {
    off(tx, move || looked(key, folder));
}

fn looked(key: u64, folder: PathBuf) -> Msg {
    let root = git::root(&folder);
    Msg::Looked {
        key,
        branch: git::branch(&folder),
        repo: root.is_some(),
        scripts: root.map(|r| runs::config(&r).scripts).unwrap_or_default(),
        folder,
    }
}

/// Put `script` at `at` in the `ghost.json` of the repository at `dir`, or
/// take the one at `at` away; then read the session `key`'s folder again.
pub fn save_script(dir: PathBuf, at: Option<usize>, script: Option<Script>, key: u64, tx: Sender) {
    off(tx, move || match runs::write_script(&dir, at, script) {
        Ok(()) => looked(key, dir),
        Err(why) => Msg::Notice(Err(("Couldn't write ghost.json".into(), why))),
    });
}

/// What a project's folder holds: whether it is in a repository, and an
/// icon of its own.
#[derive(Debug, Clone, Default)]
pub struct Held {
    pub repo: bool,
    pub icon: Option<PathBuf>,
}

/// Read what the project `folder` holds.
pub fn read_project(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Held {
        held: Held {
            repo: git::root(&folder).is_some(),
            icon: icon_of(&folder),
        },
        folder,
    });
}

/// The icon a project has of its own, if it has one where icons are kept.
fn icon_of(folder: &std::path::Path) -> Option<PathBuf> {
    [
        "favicon.svg",
        "favicon.png",
        "public/favicon.svg",
        "public/favicon.png",
        "static/favicon.png",
        "assets/icon.svg",
        "assets/icon.png",
        "icon.svg",
        "icon.png",
        "logo.svg",
        "logo.png",
    ]
    .iter()
    .map(|name| folder.join(name))
    .find(|p| p.is_file())
}

/// Find the line each diff of the calls `calls` of the session `key` starts
/// on in its file.
pub fn place(key: u64, calls: Vec<(ToolCallId, Vec<(PathBuf, String)>)>, tx: Sender) {
    off(tx, move || Msg::Placed {
        key,
        starts: calls
            .into_iter()
            .map(|(id, diffs)| (id, diffs.iter().map(|(f, t)| line_of(f, t)).collect()))
            .collect(),
    });
}

/// The line of `file` that `text` starts on, counting from one.
fn line_of(file: &std::path::Path, text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let whole = std::fs::read_to_string(file).ok()?;
    let at = whole.find(text)?;
    Some(whole[..at].matches('\n').count() + 1)
}

/// Read back the record of the session `key` at `path`, after what is being
/// written to it.
pub fn read_record(path: PathBuf, key: u64, tx: Sender) {
    let then = move |events| {
        let _ = tx.send(Msg::Record { key, events });
    };
    crate::store::queue(crate::store::Job::Read(path, Box::new(then)));
}

/// Read what is not committed in `dir`.
pub fn uncommitted(dir: PathBuf, key: u64, tx: Sender) {
    off(tx, move || Msg::Uncommitted {
        key,
        result: git::uncommitted(&dir),
    });
}

/// Delete the session `id` from its agent.
pub fn delete(agent: Arc<Agent>, key: u64, id: SessionId, tx: Sender) {
    ask(agent, DeleteSessionRequest::new(id), tx, move |r| {
        Msg::Deleted {
            key,
            result: r.map(|_| ()).map_err(|f| f.message),
        }
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
    let mut content = vec![ContentBlock::Text(TextContent::new(text))];
    content.extend(attached);
    ask(agent, PromptRequest::new(id, content), tx, move |r| {
        Msg::Ended {
            key,
            result: r.map(|r| r.stop_reason),
        }
    });
}

/// Stop the task `task` going in the background in the session `id`, as
/// the `JetBrains` extension of ACP asks it.
pub fn stop_task(agent: Arc<Agent>, id: SessionId, task: String) {
    acp::runtime().spawn(async move {
        let _ = agent
            .request(
                agent_client_protocol::UntypedMessage::new(
                    "_session/async_task/stop",
                    serde_json::json!({"sessionId": id.0, "asyncTaskId": task}),
                )
                .expect("a JSON object"),
            )
            .await;
    });
}

/// Stop the turn going on in `id`.
pub fn cancel(agent: &Agent, id: SessionId) {
    agent.cancel(CancelNotification::new(id));
}

/// Answer the question asked under `request` with `option`, or say it was
/// cancelled.
pub fn answer(agent: &Agent, request: u64, option: Option<String>) {
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
        if let Err(f) = agent.request(request).await {
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
    let request = SetSessionConfigOptionRequest::new(
        id,
        SessionConfigId::new(config),
        SessionConfigOptionValue::value_id(SessionConfigValueId::new(value)),
    );
    ask(agent, request, tx, move |r| match r {
        Ok(r) => Msg::Options {
            key,
            options: r.config_options,
        },
        Err(f) => Msg::Refused {
            key,
            why: f.message,
        },
    });
}

/// Sign in to `agent` (the registry id `id`) with its method `method`: the
/// agent itself does it, often in your browser.
pub fn authenticate(agent: Arc<Agent>, id: String, method: String, tx: Sender) {
    ask(agent, AuthenticateRequest::new(method), tx, move |r| {
        Msg::Signed {
            agent: id,
            result: r.map(|_| ()).map_err(|f| f.message),
        }
    });
}

/// Sign `agent` out.
pub fn logout(agent: Arc<Agent>, name: String, tx: Sender) {
    ask(agent, LogoutRequest::new(), tx, move |r| Msg::LoggedOut {
        agent: name,
        result: r.map(|_| ()).map_err(|f| f.message),
    });
}

/// Make a worktree for the session `key`, about `about`, on a branch
/// beginning with `prefix`.
pub fn make_worktree(
    dir: PathBuf,
    about: String,
    home: PathBuf,
    prefix: String,
    from: Option<crate::session::Begin>,
    key: u64,
    tx: Sender,
) {
    use crate::session::Begin;
    off(tx, move || {
        let result = match from {
            Some(Begin::Pr(number, _)) => git::pr_worktree(&dir, number, &home),
            Some(Begin::Branch(b)) => git::worktree(&dir, &about, &home, &prefix, &b),
            None => git::worktree(&dir, &about, &home, &prefix, "HEAD"),
        };
        let result = result.map(|wt| {
            let scripts = runs::config(&wt.path).scripts;
            (wt, scripts)
        });
        Msg::Worktree { key, result }
    });
}

/// Take a checkpoint; after a turn, read what changed in it and in all.
///
/// `after` says it is taken after the turn `turn`, and `before` and `first`
/// are the turn's checkpoint before it and the session's first; `then` is
/// what is said once it is taken.
pub fn checkpoint(
    dir: PathBuf,
    name: String,
    key: u64,
    turn: usize,
    after: bool,
    before: Option<String>,
    first: Option<String>,
    then: Option<String>,
    tx: Sender,
) {
    off(tx, move || {
        let result = git::checkpoint(&dir, &name).map(|commit| {
            let diff = |from: &Option<String>| {
                from.as_deref()
                    .filter(|_| after)
                    .and_then(|f| git::diff(&dir, f, &commit).ok())
            };
            Snapshot {
                turn: diff(&before),
                total: diff(&first),
                commit,
            }
        });
        Msg::Checkpoint {
            key,
            turn,
            after,
            result,
            then,
        }
    });
}

/// Put the session's files back as `target` had them.
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
    off(tx, move || {
        let result = git::restore(&dir, &target, &name).map(|back| {
            let total = first.and_then(|f| {
                let now = git::checkpoint(&dir, &format!("{name}-now")).ok()?;
                git::diff(&dir, &f, &now).ok()
            });
            (back, total)
        });
        Msg::Restored {
            key,
            turn,
            undo,
            result,
        }
    });
}

/// Do `action` in `dir`.
pub fn git_action(
    dir: PathBuf,
    action: GitAction,
    prefix: String,
    left_out: Vec<String>,
    key: u64,
    tx: Sender,
) {
    off(tx.clone(), move || {
        let step = |text: &str| {
            let _ = tx.send(Msg::GitStep {
                key,
                text: text.to_owned(),
            });
        };
        // Work on the main branch goes on a branch of its own before a pull
        // request is opened for it: named for what the commit says. A push
        // alone goes where you are.
        let own_branch = |m: &str| -> Result<(), String> {
            let main = git::default_branch(&dir);
            let now = git::branch(&dir);
            if now.is_some()
                && main.as_deref().map(|b| b.trim_start_matches("origin/")) == now.as_deref()
            {
                step("Making a branch");
                git::branch_off(&dir, &format!("{prefix}{}", git::slug(m)))?;
            }
            Ok(())
        };
        let result = match &action {
            GitAction::Commit(m) => {
                git::commit(&dir, m, &left_out).map(|id| format!("Committed {id}"))
            }
            GitAction::CommitPush(m) => {
                step("Committing");
                git::commit(&dir, m, &left_out).and_then(|id| {
                    step("Pushing");
                    git::push(&dir).map(|_| format!("Committed {id} and pushed"))
                })
            }
            GitAction::PullRequest(m) => own_branch(m).and_then(|()| {
                step("Committing");
                // Nothing left to commit is no reason to stop.
                let _ = git::commit(&dir, m, &left_out);
                step("Pushing");
                git::push(&dir).and_then(|_| {
                    step("Opening the pull request");
                    git::pull_request(&dir)
                })
            }),
            GitAction::Switch(b) => git::switch(&dir, b).map(|()| format!("On {b}")),
            GitAction::Merge => git::merge_pr(&dir).map(|_| "The pull request is merged".into()),
            GitAction::Pull => git::pull(&dir).map(|_| "Pulled".into()),
            GitAction::NewBranch(b) => git::branch_off(&dir, b).map(|()| format!("On {b}, new")),
            GitAction::Init => {
                git::init_here(&dir).map(|_| "It is a repository now, all of it committed".into())
            }
            GitAction::Publish => git::publish(&dir).map(|_| "It is on GitHub now, private".into()),
            GitAction::RenameBranch(b) => git::rename_branch(&dir, b).map(|()| b.clone()),
            GitAction::RemoveWorktree(wt) => git::remove_worktree(wt)
                .map(|()| format!("Worktree removed; the branch {} is kept", wt.branch)),
        };
        Msg::Git {
            key,
            action,
            result,
        }
    });
}
