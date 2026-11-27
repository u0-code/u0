//! One session with an agent, as the desk holds it (and the server, for
//! its record: `records`): the thread that the agent's updates build, what
//! it is doing now, and what it offers to be changed (its modes and its
//! options, such as the model).
//!
//! Everything here comes from the agent over ACP. The desk adds only what
//! you said, the permission questions as they were answered, when things
//! started and ended, and what went wrong between the two.

use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use agent_client_protocol::v1::schema::{
    CompactionStatus, ContentBlock, CreateElicitationRequest, ElicitationMode, ElicitationSchema,
    EmbeddedResourceResource, MessageId, PermissionOption, PermissionOptionKind, Plan,
    RequestPermissionRequest, SessionConfigOption, SessionId, SessionInfo, SessionModeState,
    SessionUpdate, StopReason, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolKind, UsageUpdate,
};
use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use serde_json::Value;

use crate::{git, store};

/// What a session is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing said yet: the agent is asked for a session with the first
    /// thing you say, so that a session begun and left costs nothing.
    Draft,
    /// The agent is starting, or opening the session.
    Opening,
    /// The agent asks to be signed in before it opens the session.
    SignIn,
    /// Waiting for you.
    Idle,
    /// Working on what you said.
    Working,
    /// Kept by its agent, not open here: opening it loads what was said
    /// in it from the agent.
    Closed,
    /// It could not be opened: why.
    Broken(String),
}

/// What a session works towards, and whether its agent has it paused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub objective: String,
    pub paused: bool,
}

impl Goal {
    /// The goal once you said `text`, where it was `now`: `/goal <what
    /// done means>` sets one, `/goal clear` takes it away, `/goal pause` and
    /// `/goal resume` pause it and take it up; anything else leaves it.
    pub fn asked(text: &str, now: Option<Self>) -> Option<Self> {
        let Some(rest) = text.trim().strip_prefix("/goal") else {
            return now;
        };
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return now;
        }
        match rest.trim() {
            "" => now,
            "clear" => None,
            "pause" => now.map(|g| Self { paused: true, ..g }),
            "resume" => now.map(|g| Self { paused: false, ..g }),
            objective => Some(Self {
                objective: objective.to_owned(),
                paused: false,
            }),
        }
    }

    /// What the agent's message `said` says of the goal, as Claude Code
    /// says it ("Goal set: …", "Goal active: … (3 turns)", "Goal cleared:
    /// …", "No goal set"): the goal now, or none; nothing where it says
    /// nothing of one.
    pub fn said(said: &str) -> Option<Option<Self>> {
        let said = said.trim_start();
        let set = |objective: &str| {
            Some(Some(Self {
                objective: objective.trim().to_owned(),
                paused: false,
            }))
        };
        if let Some(objective) = said.strip_prefix("Goal set: ") {
            return set(objective.lines().next().unwrap_or_default());
        }
        if let Some(rest) = said.strip_prefix("Goal active: ") {
            let line = rest.lines().next().unwrap_or_default();
            return set(line.rsplit_once(" (").map_or(line, |(o, _)| o));
        }
        (said.starts_with("Goal cleared") || said.starts_with("No goal set")).then_some(None)
    }
}

/// A question the agent asked, and what was chosen.
#[derive(Debug, Clone)]
pub struct Asked {
    /// The number the answer goes back under ([`crate::acp::Agent::respond`]).
    pub id: u64,
    pub title: String,
    pub options: Vec<PermissionOption>,
    /// What kind of call it asks for (a command, an edit), when known.
    pub kind: Option<ToolKind>,
    /// The option chosen, by its name and kind, or `None` while it waits.
    /// A turn stopped first answers it as rejected, named "Cancelled".
    pub chosen: Option<(String, PermissionOptionKind)>,
}

/// What the agent asked a person for, beyond a permission (ACP's
/// elicitation): a form to fill in, or a page to open (`url`). Once
/// answered, what was said back, in words.
#[derive(Debug, Clone)]
pub struct Elicit {
    /// The number the answer goes back under ([`crate::acp::Agent::respond`]).
    pub id: u64,
    pub message: String,
    pub form: Option<ElicitationSchema>,
    pub url: Option<String>,
    pub answered: Option<String>,
}

/// A tool call, with when it started and how long it took, as its record
/// says: an `At` goes before the line that begins a call and the one that
/// ends it, so a record read back keeps its times, to the second. A record
/// from before that has none, and says no time rather than a wrong one.
#[derive(Debug, Clone)]
pub struct Tool {
    pub call: ToolCall,
    pub started: Option<SystemTime>,
    pub took: Option<Duration>,
    /// For each diff in the call's content, the line of the file its new
    /// text starts on, found once the call is done; `None` where the file
    /// could not be read or no longer holds it.
    pub starts: Vec<Option<usize>>,
}

impl Tool {
    /// How long it has been going, where its start is known.
    pub fn running(&self) -> Option<Duration> {
        self.started.and_then(|t| t.elapsed().ok())
    }

    /// How long it took, where that is worth saying: a second or more.
    pub fn took_said(&self) -> Option<Duration> {
        self.took.filter(|t| t.as_secs() > 0)
    }

    /// The command a call that runs one ran, as the agent gave it: its
    /// input's `command` (words, or the script a shell was given), else
    /// its title without the backticks.
    pub fn command_line(&self) -> String {
        let words = |w: &[Value]| -> Option<Vec<String>> {
            w.iter().map(|w| w.as_str().map(str::to_owned)).collect()
        };
        match self.call.raw_input.as_ref().and_then(|v| v.get("command")) {
            Some(Value::String(line)) => line.clone(),
            Some(Value::Array(w)) => match words(w).as_deref() {
                Some([_, flag, script]) if flag == "-c" || flag == "-lc" => script.clone(),
                Some(w) => w.join(" "),
                None => self.call.title.trim().trim_matches('`').to_owned(),
            },
            _ => self.call.title.trim().trim_matches('`').to_owned(),
        }
    }
}

/// One thing in the thread.
#[derive(Debug, Clone)]
pub enum Item {
    You(String),
    /// What the agent said, as it streamed in.
    Agent(String),
    /// What it thought aloud, and for how long once it moved on.
    Thought {
        text: String,
        secs: Option<u64>,
    },
    Tool(Box<Tool>),
    Asked(Asked),
    Elicited(Elicit),
    /// The agent's plan as it stood at this point.
    Plan(Plan),
    /// Something the desk or the agent wants seen: a notice, a refusal.
    Note {
        text: String,
        trouble: bool,
    },
    /// An image the agent showed.
    Image {
        image: std::sync::Arc<Picture>,
        /// It went with what you said, not came from the agent.
        yours: bool,
    },
    /// A file that went with what you said, by its name and address; or a
    /// long paste, named so.
    File {
        name: String,
        uri: String,
    },
    /// A subagent the agent set to work: its session's id, and what the
    /// agent calls it.
    Child {
        id: SessionId,
        title: String,
    },
    /// A turn ended, why, and after how long.
    Ended {
        reason: StopReason,
        secs: u64,
    },
}

/// How a file was changed in the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    New,
    Edited,
    Deleted,
}

/// A file the session changed, with its lines added and taken away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub path: PathBuf,
    pub change: Change,
    pub added: usize,
    pub gone: usize,
}

/// The checkpoints of one turn, and what changed in it, once known.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    pub before: Option<String>,
    pub after: Option<String>,
    /// What changed from before to after.
    pub diff: Option<Vec<git::FileDiff>>,
}

/// A task the agent left going in the background (a command), and how
/// it stands: going since when, or over, how, and after how long.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub since: SystemTime,
    /// completed, failed, stopped; none while it goes on.
    pub ended: Option<(String, std::time::Duration)>,
    pub stoppable: bool,
}

impl Session {
    /// Take in what the agent says of a task in the background.
    pub fn task(&mut self, update: crate::acp::Task) {
        use crate::acp::Task as T;
        match update {
            T::Spawned {
                id,
                name,
                description,
                can_stop,
            } => {
                if !self.tasks.iter().any(|t| t.id == id) {
                    self.tasks.push(Task {
                        name: description.or(name).unwrap_or_else(|| "A command".into()),
                        id,
                        since: SystemTime::now(),
                        ended: None,
                        stoppable: can_stop,
                    });
                }
            }
            T::Progress { id, description } => {
                if let (Some(t), Some(d)) =
                    (self.tasks.iter_mut().find(|t| t.id == id), description)
                {
                    t.name = d;
                }
            }
            T::Ended { id, state } => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id)
                    && matches!(state.as_str(), "completed" | "failed" | "stopped")
                {
                    t.ended = Some((state, t.since.elapsed().unwrap_or_default()));
                }
            }
        }
    }
}

/// What a subagent is doing, as its parent's agent says (ACP's
/// subagent state): at work since when, how long its last work took,
/// whether it waits on you, and whether its work can be stopped.
#[derive(Debug, Clone, Default)]
pub struct Sub {
    pub since: Option<SystemTime>,
    pub took: Option<std::time::Duration>,
    pub needs_you: bool,
    pub stoppable: bool,
}

impl Sub {
    /// Take in what the parent's agent says of it now.
    pub fn take(&mut self, u: &agent_client_protocol::v1::schema::SubagentUpdate) {
        use agent_client_protocol::{schema::MaybeUndefined, v1::schema::StateUpdate};
        match &u.state {
            MaybeUndefined::Value(StateUpdate::Running(_)) => {
                self.since.get_or_insert_with(SystemTime::now);
                self.needs_you = false;
            }
            MaybeUndefined::Value(StateUpdate::RequiresAction(_)) => {
                self.since.get_or_insert_with(SystemTime::now);
                self.needs_you = true;
            }
            MaybeUndefined::Value(StateUpdate::Idle(_)) | MaybeUndefined::Null => {
                if let Some(t) = self.since.take() {
                    self.took = t.elapsed().ok();
                }
                self.needs_you = false;
            }
            _ => {}
        }
        match &u.capabilities {
            MaybeUndefined::Value(c) => self.stoppable = c.cancel.is_some(),
            MaybeUndefined::Null => self.stoppable = false,
            MaybeUndefined::Undefined => {}
        }
    }

    /// At work now.
    pub fn working(&self) -> bool {
        self.since.is_some()
    }

    /// How long it has worked, or worked last: "1m 2s".
    pub fn time(&self) -> Option<String> {
        self.since
            .and_then(|t| t.elapsed().ok())
            .or(self.took)
            .map(crate::when::took)
    }
}

/// What a draft's worktree begins from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Begin {
    /// A local branch: a new branch from it.
    Branch(String),
    /// A pull request, by number and title: its own branch, checked out.
    Pr(u64, String),
}

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about the thread, not a state among several"
)]
pub struct Session {
    /// The desk's own key for it, stable from before the agent names it.
    pub key: u64,
    /// The server it is on, of those the desk holds: this Mac's, unless
    /// it is given another.
    pub host: crate::servers::Host,
    /// What of it the desk keeps between runs (`sessions.json`).
    pub meta: store::Meta,
    /// The git branch the folder is on, as last read.
    pub branch: Option<String>,
    /// The branch it was on when you last said something in it, while
    /// u0 runs: a turn sent after it changed goes on on another.
    pub ran_on: Option<String>,
    /// Whether the folder is in a git repository: then each turn is
    /// checkpointed, and can be undone.
    pub repo: bool,
    /// For a draft: whether it is to work in a worktree of its own, where
    /// it is in a repository, and what it is to begin from (where the
    /// folder is, if nothing).
    pub wants_worktree: bool,
    pub begin: Option<Begin>,
    /// Options and a mode chosen before the agent opened it, set on it
    /// once it does.
    pub chosen: Vec<(String, String)>,
    pub chosen_mode: Option<String>,
    /// Each turn's checkpoints, by turn, the first turn first.
    pub turns: Vec<Turn>,
    /// What changed from before the first turn to after the latest.
    pub total: Option<Vec<git::FileDiff>>,
    /// A turn undone, and the checkpoint that puts it back.
    pub redo: Option<(usize, String)>,
    pub state: State,
    pub items: Vec<Item>,
    /// The ids the agent gave its messages and yours, each by the item of
    /// the thread it is in: where a fork can end.
    pub marks: Vec<(usize, MessageId)>,
    /// How much of its thread there was when you last left it.
    pub seen: Option<usize>,
    /// The scripts run in its new worktree as it was made (u0.json's),
    /// each in a terminal: said at its head until put away.
    pub setup: Vec<String>,
    /// The terminals of setup scripts its first message waits for.
    pub holding: Vec<u64>,
    /// The session whose agent set this one to work as its subagent: shown,
    /// not listed, not kept, not written in.
    pub parent: Option<u64>,
    /// A subagent's work, as its parent's agent tells it.
    pub sub: Sub,
    /// The tasks its agent left going in the background: commands.
    pub tasks: Vec<Task>,
    /// What it works towards across turns, set with `/goal`: the
    /// agent's own command (Claude Code's, Codex's), which it keeps
    /// working on until it judges it met.
    pub goal: Option<Goal>,
    /// The session this one asks a side question of: branched off it
    /// whole, not listed, its answer taken back to it if you want.
    pub beside: Option<u64>,
    /// When each message (yours, the agent's) began, by its item; and the
    /// time read from the record for the next one.
    pub times: Vec<(usize, SystemTime)>,
    stamp: Option<SystemTime>,
    pub plan: Option<Plan>,
    pub modes: Option<SessionModeState>,
    pub options: Vec<SessionConfigOption>,
    pub usage: Option<UsageUpdate>,
    /// When the turn going on started, for the time beside it.
    pub since: Option<Instant>,
    /// When the thought being streamed began.
    thinking: Option<Instant>,
    /// What you said before the session was open, sent once it is.
    pub pending: Option<String>,
    /// What happened in it since its record was last written.
    pub log: Vec<store::Event>,
    /// Its record is being read back: nothing read is written again.
    replaying: bool,
    /// It has a record the desk wrote: its agent's session is resumed, not
    /// loaded, so that nothing comes twice.
    pub recorded: bool,
    /// The commands the agent takes in it, typed after `/`.
    pub commands: Vec<agent_client_protocol::v1::schema::AvailableCommand>,
    /// What goes with the next thing said: images, files and sessions
    /// mentioned.
    pub attached: Vec<ContentBlock>,
    /// What git says where it works, and what you chose of it.
    pub git: git::View,
    /// The scripts the `u0.json` of its repository names.
    pub scripts: Vec<crate::scripts::Script>,
    /// The calls just ended whose diffs are yet to be placed in their
    /// files, each diff's file and new text (see `hub::place`).
    pub placing: Vec<(ToolCallId, Vec<(PathBuf, String)>)>,
}

/// The text of a content block, or a word for what it is.
pub fn text_of(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(t) => t.text.clone(),
        ContentBlock::Image(_) => "[image]".into(),
        ContentBlock::Audio(_) => "[audio]".into(),
        ContentBlock::ResourceLink(r) => format!("[{}]({})", r.name, r.uri),
        ContentBlock::Resource(r) => match &r.resource {
            EmbeddedResourceResource::TextResourceContents(t) => t.text.clone(),
            EmbeddedResourceResource::BlobResourceContents(b) => format!("[{}]", b.uri),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The name and address of a file that went with what you said: one
/// linked, or a paste too long for the line, sent as its text.
fn file_of(block: &ContentBlock) -> Option<(String, String)> {
    match block {
        ContentBlock::ResourceLink(r) => Some((r.name.clone(), r.uri.clone())),
        ContentBlock::Resource(r) => match &r.resource {
            EmbeddedResourceResource::TextResourceContents(t) => {
                Some((Context::of(&t.uri)?.name().into(), t.uri.clone()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Text that goes with what you say, as a chip beside the line rather
/// than in it, named by where it came from: sent as a resource whose
/// address says that (`u0://<kind>/<n>`), so the thread names it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// A paste too long for the line.
    Paste,
    /// What you quoted from the thread.
    Quote,
    /// What you chose in a terminal.
    Terminal,
    /// Your comments on the changes.
    Review,
}

impl Context {
    const ALL: [Self; 4] = [Self::Paste, Self::Quote, Self::Terminal, Self::Review];

    fn word(self) -> &'static str {
        match self {
            Self::Paste => "paste",
            Self::Quote => "quote",
            Self::Terminal => "terminal",
            Self::Review => "review",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Paste => "Pasted text",
            Self::Quote => "Quote",
            Self::Terminal => "Terminal output",
            Self::Review => "Review comments",
        }
    }

    /// The address of the `n`th that goes with what you say.
    pub fn uri(self, n: usize) -> String {
        format!("u0://{}/{n}", self.word())
    }

    /// The kind an address of [`Context::uri`] names.
    pub fn of(uri: &str) -> Option<Self> {
        let word = uri.strip_prefix("u0://")?.split('/').next()?;
        Self::ALL.into_iter().find(|c| c.word() == word)
    }
}

/// A picture in a thread, in a format the desk draws.
#[derive(Debug)]
pub struct Picture {
    pub format: Format,
    pub bytes: Vec<u8>,
    /// A hash of the bytes, taken once: the desk knows a picture it has
    /// decoded by it, without reading the bytes again each time it draws.
    id: u64,
}

impl std::hash::Hash for Picture {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.id);
    }
}

/// The formats a picture can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
    Bmp,
    Tiff,
    Ico,
}

impl Format {
    /// The format of the MIME type `mime`; none, not one the desk draws.
    pub fn of(mime: &str) -> Option<Self> {
        Some(match mime {
            "image/png" => Self::Png,
            "image/jpeg" | "image/jpg" => Self::Jpeg,
            "image/gif" => Self::Gif,
            "image/webp" => Self::Webp,
            "image/svg+xml" => Self::Svg,
            "image/bmp" => Self::Bmp,
            "image/tiff" => Self::Tiff,
            "image/x-icon" | "image/vnd.microsoft.icon" => Self::Ico,
            _ => return None,
        })
    }
}

/// The picture in `block`, if it is one the desk draws.
fn image_of(block: &ContentBlock) -> Option<std::sync::Arc<Picture>> {
    let ContentBlock::Image(image) = block else {
        return None;
    };
    let format = Format::of(&image.mime_type)?;
    // Space is passed over and padding may be left off; an agent that
    // writes the URL-safe alphabet is read too.
    let data: String = image.data.split_whitespace().collect();
    let loose = GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true);
    let bytes = GeneralPurpose::new(&alphabet::STANDARD, loose)
        .decode(&data)
        .or_else(|_| GeneralPurpose::new(&alphabet::URL_SAFE, loose).decode(&data))
        .ok()?;
    let mut id = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(&bytes, &mut id);
    let id = std::hash::Hasher::finish(&id);
    Some(std::sync::Arc::new(Picture { format, bytes, id }))
}

impl Session {
    /// A session its agent listed, in `project`: closed until opened.
    pub fn listed(key: u64, agent: &str, info: SessionInfo, project: PathBuf) -> Self {
        let mut s = Self::new(key, agent, info.cwd);
        s.meta.project = project;
        s.meta.id = Some(info.session_id);
        if let Some(title) = info.title.filter(|t| !t.trim().is_empty()) {
            s.meta.title = title;
        }
        if let Some(t) = info.updated_at.as_deref().and_then(crate::when::parse) {
            s.meta.begun = t;
            s.meta.updated = t;
            s.meta.flags.order = -i64::try_from(store::secs(t)).unwrap_or(0);
        }
        s.state = State::Closed;
        s
    }

    /// Choose `value` for the option `config` here, before the agent has
    /// the session: it is set on it once it is opened.
    pub fn choose(&mut self, config: &str, value: &str) {
        for o in &mut self.options {
            if &*o.id.0 == config
                && let agent_client_protocol::v1::schema::SessionConfigKind::Select(select) =
                    &mut o.kind
            {
                select.current_value =
                    agent_client_protocol::v1::schema::SessionConfigValueId::new(value.to_owned());
            }
        }
        self.chosen.retain(|(c, _)| c != config);
        self.chosen.push((config.to_owned(), value.to_owned()));
    }

    /// Choose the mode `mode` here, before the agent has the session.
    pub fn choose_mode(&mut self, mode: &str) {
        if let Some(modes) = &mut self.modes {
            modes.current_mode_id =
                agent_client_protocol::v1::schema::SessionModeId::new(mode.to_owned());
        }
        self.chosen_mode = Some(mode.to_owned());
    }

    /// Whether a worktree is to be made for it before it is opened.
    pub fn needs_worktree(&self) -> bool {
        self.git_dir().is_some() && self.wants_worktree && self.meta.worktree.is_none()
    }

    /// Work in the worktree `wt` made for it before its first turn, with
    /// the scripts its repository names: a repository of its own now, or
    /// a folder of worktrees of those its folder held, so nothing read of
    /// that folder holds; and its first turn runs on the worktree's
    /// branch, not on the one its folder was on.
    pub fn into_worktree(&mut self, wt: git::Worktree, scripts: Vec<crate::scripts::Script>) {
        self.meta.folder.clone_from(&wt.path);
        self.branch = Some(wt.branch.clone());
        self.ran_on.clone_from(&self.branch);
        self.scripts = scripts;
        self.repo = wt.repos.is_empty();
        self.git = git::View::default();
        self.git.repos = wt.repos.iter().map(|w| w.path.clone()).collect();
        self.git.repo = self.git.repos.first().cloned();
        self.total = None;
        self.meta.worktree = Some(wt);
    }

    /// Take what its folder holds, as git said it: whether it is in a
    /// repository, else the repositories in it, the one chosen of them
    /// kept where it is still there.
    pub fn holds(&mut self, repo: bool, repos: Vec<std::path::PathBuf>) {
        self.repo = repo;
        if !self.git.repo.as_ref().is_some_and(|r| repos.contains(r)) {
            self.git.repo = repos.first().cloned();
        }
        self.git.repos = repos;
    }

    /// Where git is asked about it: its folder, in a repository; else the
    /// repository chosen of those its folder holds.
    pub fn git_dir(&self) -> Option<std::path::PathBuf> {
        if self.repo {
            Some(self.meta.folder.clone())
        } else {
            self.git.repo.clone()
        }
    }

    /// Whether its turns are checkpointed: its folder is in a repository,
    /// or holds some (each checkpointed, `git::checkpoint`).
    pub fn checkpoints(&self) -> bool {
        self.repo || !self.git.repos.is_empty()
    }

    /// Whether it is a session of `agent` on `host`.
    pub fn of(&self, host: crate::servers::Host, agent: &str) -> bool {
        self.host == host && self.meta.agent == agent
    }

    /// A session the server `host` kept from an earlier run: closed until
    /// opened.
    pub fn stored(key: u64, host: crate::servers::Host, meta: store::Meta) -> Self {
        let mut s = Self::new(key, &meta.agent, meta.folder.clone());
        s.host = host;
        s.meta = meta;
        s.state = State::Closed;
        s
    }

    /// What the desk keeps of it; `None` until its agent has it, and for
    /// a subagent, which is kept with its parent.
    pub fn kept(&self) -> Option<&store::Meta> {
        (self.parent.is_none() && self.meta.id.is_some()).then_some(&self.meta)
    }

    /// Build the thread again from its record.
    pub fn replay(&mut self, events: Vec<store::Event>) {
        self.replaying = true;
        for e in events {
            self.take(e);
        }
        self.replaying = false;
    }

    fn take(&mut self, e: store::Event) {
        match e {
            store::Event::At(secs) => self.stamp = Some(store::time(secs)),
            store::Event::You(text) => {
                if let Some(t) = self.stamp.take() {
                    self.times.push((self.items.len(), t));
                }
                self.goal = Goal::asked(&text, self.goal.take());
                self.items.push(Item::You(text));
            }
            store::Event::Update(u) => self.apply(*u),
            store::Event::Ended { reason, secs, .. } => {
                self.thought_over();
                self.items.push(Item::Ended { reason, secs });
            }
            store::Event::Note { text, trouble } => {
                self.items.push(Item::Note { text, trouble });
            }
        }
    }

    /// The last message a fork made before your message at `item` of the
    /// thread keeps, by the id its agent gave it; none for your first
    /// message, or where the agent gave no ids.
    pub fn fork_point(&self, item: usize) -> Option<MessageId> {
        if !matches!(self.items.get(item), Some(Item::You(_)))
            || !self.items[..item].iter().any(|i| matches!(i, Item::You(_)))
        {
            return None;
        }
        self.marks
            .iter()
            .rev()
            .find(|(at, _)| *at < item)
            .map(|(_, id)| id.clone())
    }

    /// The last message a fork kept through the item `item` of the
    /// thread, by the id its agent gave it.
    pub fn through(&self, item: usize) -> Option<MessageId> {
        self.marks
            .iter()
            .rev()
            .find(|(at, _)| *at <= item)
            .map(|(_, id)| id.clone())
    }

    /// How its last turn came out, where that is worth a word in a list:
    /// a plan made in a mode that only plans, waiting to be carried out;
    /// or a turn that failed (the agent refused, ran out, or erred).
    pub fn outcome(&self) -> Option<&'static str> {
        if !matches!(self.state, State::Idle | State::Closed) {
            return None;
        }
        let start = self.items.iter().rposition(|i| matches!(i, Item::You(_)))?;
        let turn = &self.items[start..];
        let failed = turn.iter().any(|i| {
            matches!(i, Item::Note { trouble: true, .. })
                || matches!(
                    i,
                    Item::Ended {
                        reason: StopReason::Refusal
                            | StopReason::MaxTokens
                            | StopReason::MaxTurnRequests,
                        ..
                    }
                )
        });
        if failed {
            Some("failed")
        } else if self.planning() && turn.iter().any(|i| matches!(i, Item::Plan(_))) {
            Some("plan ready")
        } else {
            None
        }
    }

    /// When the message at `item` began, if that is known.
    pub fn time_of(&self, item: usize) -> Option<SystemTime> {
        self.times
            .iter()
            .find(|(at, _)| *at == item)
            .map(|(_, t)| *t)
    }

    /// Empty the thread, to build it again.
    pub fn forget(&mut self) {
        self.items.clear();
        self.placing.clear();
        self.marks.clear();
        self.times.clear();
    }

    /// The events of a record up to the one that begins your message
    /// after the first `yours`: the record of a fork made before it.
    pub fn record_before(events: Vec<store::Event>, yours: usize) -> Vec<store::Event> {
        let mut thread = Self::new(0, "", PathBuf::new());
        thread.replaying = true;
        let mut kept = Vec::new();
        for e in events {
            thread.take(e.clone());
            if thread
                .items
                .iter()
                .filter(|i| matches!(i, Item::You(_)))
                .count()
                > yours
            {
                break;
            }
            kept.push(e);
        }
        kept
    }

    /// Whether it is in a mode that only plans (its mode, or an option of
    /// the kind, is one named for planning).
    pub fn planning(&self) -> bool {
        let plan = |s: &str| s.to_lowercase().contains("plan");
        self.modes
            .as_ref()
            .is_some_and(|m| plan(&m.current_mode_id.0))
            || self.options.iter().any(|o| match &o.kind {
                agent_client_protocol::v1::schema::SessionConfigKind::Select(sel) => {
                    plan(&sel.current_value.0)
                }
                _ => false,
            })
    }

    /// A note in the thread, kept in its record.
    pub fn note(&mut self, text: &str) {
        self.record(store::Event::Note {
            text: text.to_owned(),
            trouble: true,
        });
        self.items.push(Item::Note {
            text: text.to_owned(),
            trouble: true,
        });
    }

    /// Note `event` for its record, unless the record is being read.
    fn record(&mut self, event: store::Event) {
        if !self.replaying {
            self.log.push(event);
        }
    }

    /// A new session in `folder`; what git says of it comes later (see
    /// `hub::look`).
    pub fn new(key: u64, agent: &str, folder: PathBuf) -> Self {
        let title = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let now = SystemTime::now();
        Self {
            key,
            host: crate::servers::Host::HERE,
            meta: store::Meta {
                agent: agent.to_owned(),
                id: None,
                project: folder.clone(),
                folder,
                worktree: None,
                title,
                renamed: false,
                begun: now,
                updated: now,
                flags: store::Flags {
                    order: -i64::try_from(store::secs(now)).unwrap_or(0),
                    ..store::Flags::default()
                },
                queued: Vec::new(),
                unsent: String::new(),
                from: None,
                watch: None,
                also: Vec::new(),
                subagents: Vec::new(),
                links: Vec::new(),
                tags: Vec::new(),
            },
            branch: None,
            ran_on: None,
            repo: false,
            wants_worktree: false,
            begin: None,
            chosen: Vec::new(),
            chosen_mode: None,
            turns: Vec::new(),
            total: None,
            redo: None,
            state: State::Draft,
            items: Vec::new(),
            marks: Vec::new(),
            parent: None,
            sub: Sub::default(),
            tasks: Vec::new(),
            goal: None,
            beside: None,
            setup: Vec::new(),
            holding: Vec::new(),
            seen: None,
            times: Vec::new(),
            stamp: None,
            plan: None,
            modes: None,
            options: Vec::new(),
            usage: None,
            since: None,
            thinking: None,
            pending: None,
            log: Vec::new(),
            replaying: false,
            recorded: false,
            commands: Vec::new(),
            attached: Vec::new(),
            git: git::View::default(),
            scripts: Vec::new(),
            placing: Vec::new(),
        }
    }

    /// What you said, put in the thread as the turn begins, with what
    /// went with it.
    pub fn said(&mut self, text: &str) {
        let now = SystemTime::now();
        self.record(store::Event::At(store::secs(now)));
        self.record(store::Event::You(text.to_owned()));
        self.times.push((self.items.len(), now));
        if self.items.is_empty() && !self.meta.renamed {
            self.meta.title = text
                .lines()
                .next()
                .unwrap_or(text)
                .chars()
                .take(60)
                .collect();
            self.meta.begun = SystemTime::now();
        }
        self.goal = Goal::asked(text, self.goal.take());
        self.items.push(Item::You(text.to_owned()));
        // What goes with it shows under it, and is kept with its record:
        // images, files and long pastes; not what a session or pull
        // request named said.
        let shown: Vec<ContentBlock> = (self.attached.iter())
            .filter(|b| image_of(b).is_some() || file_of(b).is_some())
            .cloned()
            .collect();
        for b in shown {
            self.apply(SessionUpdate::UserMessageChunk(
                agent_client_protocol::v1::schema::ContentChunk::new(b),
            ));
        }
        self.state = State::Working;
        self.since = Some(Instant::now());
        self.meta.updated = SystemTime::now();
    }

    /// The turn is over, having taken `tokens`.
    pub fn ended(&mut self, reason: Result<StopReason, String>, tokens: Vec<crate::usage::Tokens>) {
        self.thought_over();
        let secs = self.since.map_or(0, |t| t.elapsed().as_secs());
        match reason {
            Ok(reason) => {
                self.record(store::Event::Ended {
                    reason,
                    secs,
                    tokens,
                });
                self.items.push(Item::Ended { reason, secs });
            }
            Err(why) => {
                self.record(store::Event::Note {
                    text: why.clone(),
                    trouble: true,
                });
                self.items.push(Item::Note {
                    text: why,
                    trouble: true,
                });
            }
        }
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && a.chosen.is_none()
            {
                a.chosen = Some(("Cancelled".into(), PermissionOptionKind::RejectOnce));
            }
        }
        self.state = State::Idle;
        self.since = None;
        self.meta.updated = SystemTime::now();
    }

    /// A thought being streamed is over: it is timed.
    fn thought_over(&mut self) {
        if let Some(from) = self.thinking.take()
            && let Some(Item::Thought { secs, .. }) = self
                .items
                .iter_mut()
                .rev()
                .find(|i| matches!(i, Item::Thought { .. }))
        {
            *secs = Some(from.elapsed().as_secs().max(1));
        }
    }

    /// Take in one update from the agent.
    pub fn apply(&mut self, update: SessionUpdate) {
        // What Claude's adapter says of its own (a hook's warning) comes as
        // the agent's words marked informational: they are not what the
        // agent said. An error is the session's, a row of its own; the rest
        // stays out of the thread.
        if let SessionUpdate::AgentMessageChunk(c) = &update
            && let Some(level) = informational(c)
        {
            let ContentBlock::Text(t) = &c.content else {
                return;
            };
            let text = t.text.trim().trim_start_matches('>').replace("**", "");
            if level == "error" {
                self.note(text.trim());
            } else {
                tracing::info!("{} said: {}", self.meta.agent, text.trim());
            }
            return;
        }
        // A new message from the agent: when it began goes before it.
        let begins = matches!(update, SessionUpdate::AgentMessageChunk(_))
            && !matches!(self.items.last(), Some(Item::Agent(_)));
        // A call begun or ended: when, likewise, for how long it took.
        let timed = match &update {
            SessionUpdate::ToolCall(_) => true,
            SessionUpdate::ToolCallUpdate(u) => matches!(
                u.fields.status,
                Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
            ),
            _ => false,
        };
        let at = if !(begins || timed) {
            None
        } else if self.replaying {
            self.stamp.take()
        } else {
            let now = SystemTime::now();
            self.record(store::Event::At(store::secs(now)));
            Some(now)
        };
        if begins && let Some(t) = at {
            self.times.push((self.items.len(), t));
        }
        if !self.replaying {
            self.meta.updated = SystemTime::now();
            self.record(store::Event::Update(Box::new(update.clone())));
        }
        if !matches!(update, SessionUpdate::AgentThoughtChunk(_)) {
            self.thought_over();
        }
        if let SessionUpdate::UserMessageChunk(c)
        | SessionUpdate::AgentMessageChunk(c)
        | SessionUpdate::AgentThoughtChunk(c) = &update
            && let Some(id) = &c.message_id
        {
            // Where the message goes: the last item, or one about to be
            // added; a message of yours with another id is one of its own.
            let last = self
                .marks
                .last()
                .filter(|(at, _)| *at + 1 == self.items.len())
                .map(|(_, id)| id);
            let at = match (&update, self.items.last()) {
                (SessionUpdate::UserMessageChunk(_), Some(Item::You(_)))
                    if last.is_none_or(|last| last == id) =>
                {
                    self.items.len() - 1
                }
                (SessionUpdate::AgentMessageChunk(_), Some(Item::Agent(_)))
                | (SessionUpdate::AgentThoughtChunk(_), Some(Item::Thought { .. })) => {
                    self.items.len() - 1
                }
                _ => self.items.len(),
            };
            if self.marks.last() != Some(&(at, id.clone())) {
                self.marks.push((at, id.clone()));
            }
        }
        match update {
            SessionUpdate::UserMessageChunk(c) => {
                // An image that went with it shows as itself.
                if let Some(image) = image_of(&c.content) {
                    self.items.push(Item::Image { image, yours: true });
                    return;
                }
                // And a file, or a long paste, as a file of its own.
                if let Some((name, uri)) = file_of(&c.content) {
                    self.items.push(Item::File { name, uri });
                    return;
                }
                // A session being reopened replays what you said.
                let text = text_of(&c.content);
                let own = self
                    .marks
                    .last()
                    .is_some_and(|(at, _)| *at == self.items.len());
                match self.items.last_mut() {
                    Some(Item::You(t)) if !own => t.push_str(&text),
                    _ => self.items.push(Item::You(text)),
                }
            }
            SessionUpdate::AgentMessageChunk(c) => {
                // An image shown is an item of its own, drawn as itself.
                if let Some(image) = image_of(&c.content) {
                    self.items.push(Item::Image {
                        image,
                        yours: false,
                    });
                    return;
                }
                let text = text_of(&c.content);
                match self.items.last_mut() {
                    Some(Item::Agent(t)) => t.push_str(&text),
                    _ => self.items.push(Item::Agent(text)),
                }
                if let Some(Item::Agent(said)) = self.items.last()
                    && let Some(goal) = Goal::said(said)
                {
                    self.goal = goal;
                }
            }
            SessionUpdate::AgentThoughtChunk(c) => {
                let text = text_of(&c.content);
                if let Some(Item::Thought { text: t, .. }) = self.items.last_mut() {
                    t.push_str(&text);
                } else {
                    self.thinking = Some(Instant::now());
                    self.items.push(Item::Thought { text, secs: None });
                }
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.clone();
                if let Some(old) = self.tool_mut(&id) {
                    old.call = call;
                } else {
                    self.items.push(Item::Tool(Box::new(Tool {
                        call,
                        started: at,
                        took: None,
                        starts: Vec::new(),
                    })));
                }
                self.settle(&id, at);
            }
            SessionUpdate::ToolCallUpdate(update) => self.update_tool(update, at),
            SessionUpdate::Plan(plan) => {
                // One plan a turn, brought up to date where it first showed.
                let turn = self
                    .items
                    .iter()
                    .rposition(|i| matches!(i, Item::You(_)))
                    .map_or(0, |k| k + 1);
                match self.items[turn..]
                    .iter_mut()
                    .rfind(|i| matches!(i, Item::Plan(_)))
                {
                    Some(Item::Plan(p)) => *p = plan.clone(),
                    _ => self.items.push(Item::Plan(plan.clone())),
                }
                self.plan = Some(plan);
            }
            SessionUpdate::CurrentModeUpdate(m) => {
                if let Some(modes) = &mut self.modes {
                    modes.current_mode_id = m.current_mode_id;
                }
            }
            SessionUpdate::ConfigOptionUpdate(o) => self.options = o.config_options,
            SessionUpdate::SessionInfoUpdate(info) => {
                if let Some(title) = info
                    .title
                    .take()
                    .filter(|t| !t.is_empty() && !self.meta.renamed)
                {
                    self.meta.title = title;
                }
            }
            SessionUpdate::UsageUpdate(u) => self.usage = Some(u),
            SessionUpdate::SubagentUpdate(u) => {
                let id = u.session_id.clone();
                let named = match u.title {
                    agent_client_protocol::schema::MaybeUndefined::Value(t) => Some(t),
                    _ => None,
                };
                if let Some(Item::Child { title, .. }) = self
                    .items
                    .iter_mut()
                    .find(|i| matches!(i, Item::Child { id: c, .. } if *c == id))
                {
                    if let Some(t) = named {
                        *title = t;
                    }
                } else {
                    self.items.push(Item::Child {
                        id: id.clone(),
                        title: named.unwrap_or_else(|| "A subagent".into()),
                    });
                }
                // The index keeps it too, to bring it back after a restart.
                let title = self.items.iter().rev().find_map(|i| match i {
                    Item::Child { id: c, title } if *c == id => Some(title.clone()),
                    _ => None,
                });
                let id = id.0.to_string();
                match self.meta.subagents.iter_mut().find(|(c, _)| *c == id) {
                    Some((_, t)) => *t = title.unwrap_or_default(),
                    None => self.meta.subagents.push((id, title.unwrap_or_default())),
                }
            }
            SessionUpdate::AvailableCommandsUpdate(c) => self.commands = c.available_commands,
            SessionUpdate::Notice(n) => self.items.push(Item::Note {
                text: match n.description {
                    Some(d) => format!("{} — {d}", n.title),
                    None => n.title,
                },
                trouble: false,
            }),
            // The context compacted: said where it was, as a divider is.
            SessionUpdate::CompactionUpdate(c) => match c.status {
                CompactionStatus::Completed => self.items.push(Item::Note {
                    text: "The context was compacted: what came before is summarized for the agent"
                        .into(),
                    trouble: false,
                }),
                CompactionStatus::Failed => self.items.push(Item::Note {
                    text: match c.error {
                        agent_client_protocol::schema::MaybeUndefined::Value(why) => {
                            format!("The context could not be compacted: {why}")
                        }
                        _ => "The context could not be compacted".into(),
                    },
                    trouble: true,
                }),
                _ => {}
            },
            _ => {}
        }
    }

    fn tool_mut(&mut self, id: &ToolCallId) -> Option<&mut Tool> {
        self.items.iter_mut().rev().find_map(|i| match i {
            Item::Tool(t) if &t.call.tool_call_id == id => Some(&mut **t),
            _ => None,
        })
    }

    fn update_tool(&mut self, update: ToolCallUpdate, at: Option<SystemTime>) {
        let id = update.tool_call_id.clone();
        if let Some(tool) = self.tool_mut(&id) {
            tool.call.update(update.fields);
        } else if let Ok(call) = ToolCall::try_from(update) {
            self.items.push(Item::Tool(Box::new(Tool {
                call,
                started: at,
                took: None,
                starts: Vec::new(),
            })));
        }
        self.settle(&id, at);
    }

    /// A call that has just ended, at `at`, is timed, and its diffs are to
    /// be placed in their files.
    fn settle(&mut self, id: &ToolCallId, at: Option<SystemTime>) {
        let Some(tool) = self.tool_mut(id) else {
            return;
        };
        let done = matches!(
            tool.call.status,
            ToolCallStatus::Completed | ToolCallStatus::Failed
        );
        if !done || tool.took.is_some() {
            return;
        }
        // Unknown, it is none at all: nothing is said of it, as of a call
        // over in under a second.
        tool.took = Some(
            at.zip(tool.started)
                .and_then(|(end, start)| end.duration_since(start).ok())
                .unwrap_or_default(),
        );
        let diffs: Vec<(PathBuf, String)> = tool
            .call
            .content
            .iter()
            .filter_map(|c| match c {
                ToolCallContent::Diff(d) => Some((d.path.clone(), d.new_text.clone())),
                _ => None,
            })
            .collect();
        if !diffs.is_empty() {
            self.placing.push((id.clone(), diffs));
        }
    }

    /// Where the diffs of the call `id` start in their files, as read.
    pub fn placed(&mut self, id: &ToolCallId, starts: Vec<Option<usize>>) {
        if let Some(tool) = self.tool_mut(id) {
            tool.starts = starts;
        }
    }

    /// A question the agent asks before it goes on.
    pub fn asked(&mut self, id: u64, request: RequestPermissionRequest) {
        let title = request
            .tool_call
            .fields
            .title
            .clone()
            .or_else(|| {
                self.tool_mut(&request.tool_call.tool_call_id)
                    .map(|t| t.call.title.clone())
            })
            .unwrap_or_else(|| "The agent asks to go on".into());
        let call = request.tool_call.tool_call_id.clone();
        let kind = request.tool_call.fields.kind.or_else(|| {
            self.tools()
                .filter(|t| t.call.tool_call_id == call)
                .last()
                .map(|t| t.call.kind)
        });
        self.update_tool(request.tool_call, None);
        self.items.push(Item::Asked(Asked {
            id,
            title,
            options: request.options,
            kind,
            chosen: None,
        }));
    }

    /// The agent asks for a form or a page under `id`.
    pub fn elicited(&mut self, id: u64, request: CreateElicitationRequest) {
        let (form, url) = match request.mode {
            ElicitationMode::Form(f) => (Some(f.requested_schema), None),
            ElicitationMode::Url(u) => (None, Some(u.url)),
            _ => (None, None),
        };
        self.items.push(Item::Elicited(Elicit {
            id,
            message: request.message,
            form,
            url,
            answered: None,
        }));
    }

    /// What the agent asked for under `id`, while it waits for you.
    pub fn eliciting(&self) -> Option<&Elicit> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Elicited(e) if e.answered.is_none() => Some(e),
            _ => None,
        })
    }

    /// Note that what was asked for under `id` was answered: `said`, in
    /// words.
    pub fn elicit_answered(&mut self, id: u64, said: &str) {
        for item in &mut self.items {
            if let Item::Elicited(e) = item
                && e.id == id
            {
                e.answered = Some(said.to_owned());
            }
        }
    }

    /// Its title as it is shown and said: "New session" for one not begun,
    /// whose title is only its folder's name till you write in it.
    pub fn shown_title(&self) -> &str {
        if self.state == State::Draft && self.items.is_empty() {
            "New thread"
        } else {
            &self.meta.title
        }
    }

    /// Whether something the agent asked waits on you: a permission, a
    /// form, a page.
    pub fn asks(&self) -> bool {
        self.waiting().is_some() || self.eliciting().is_some()
    }

    /// The question waiting on you, if one is.
    pub fn waiting(&self) -> Option<&Asked> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Asked(a) if a.chosen.is_none() => Some(a),
            _ => None,
        })
    }

    /// Note that the question asked under `id` was answered with `option`.
    pub fn answered(&mut self, id: u64, option: &PermissionOption) {
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && a.id == id
            {
                a.chosen = Some((option.name.clone(), option.kind));
            }
        }
    }

    /// The tool calls, in order.
    pub fn tools(&self) -> impl Iterator<Item = &Tool> {
        self.items.iter().filter_map(|i| match i {
            Item::Tool(t) => Some(&**t),
            _ => None,
        })
    }

    /// The commands the agent ran, in order.
    pub fn commands(&self) -> impl Iterator<Item = &Tool> {
        self.tools().filter(|t| t.call.kind == ToolKind::Execute)
    }

    /// The files the agent changed in this session, each once, in the order
    /// first changed, with the lines added and taken away.
    pub fn changed(&self) -> Vec<Changed> {
        let mut files: Vec<Changed> = Vec::new();
        for t in self.tools() {
            if t.call.kind == ToolKind::Delete {
                for l in &t.call.locations {
                    files.retain(|f| f.path != l.path);
                    files.push(Changed {
                        path: l.path.clone(),
                        change: Change::Deleted,
                        added: 0,
                        gone: 0,
                    });
                }
            }
            for c in &t.call.content {
                let ToolCallContent::Diff(d) = c else {
                    continue;
                };
                let crate::git::FileDiff { added, gone, .. } = crate::git::edit_diff(d, None);
                match files.iter_mut().find(|f| f.path == d.path) {
                    Some(f) => {
                        f.added += added;
                        f.gone += gone;
                    }
                    None => files.push(Changed {
                        path: d.path.clone(),
                        change: if d.old_text.is_none() {
                            Change::New
                        } else {
                            Change::Edited
                        },
                        added,
                        gone,
                    }),
                }
            }
        }
        files
    }

    /// Every file the session touched, read or changed, each once.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for t in self.tools() {
            let diffs = t.call.content.iter().filter_map(|c| match c {
                ToolCallContent::Diff(d) => Some(&d.path),
                _ => None,
            });
            for p in t.call.locations.iter().map(|l| &l.path).chain(diffs) {
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// How many turns there have been.
    pub fn turn_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, Item::You(_)))
            .count()
    }

    /// The name its checkpoints are kept under: unique across runs.
    pub fn uid(&self) -> String {
        let secs = self
            .meta
            .begun
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("{secs}-{}", self.key)
    }

    /// The turn the item at `i` belongs to, counting from zero.
    pub fn turn_of(&self, i: usize) -> Option<usize> {
        self.items
            .iter()
            .take(i + 1)
            .filter(|x| matches!(x, Item::You(_)))
            .count()
            .checked_sub(1)
    }

    /// The checkpoint before the first turn.
    pub fn first(&self) -> Option<&str> {
        self.turns.first().and_then(|t| t.before.as_deref())
    }
}

/// The level ("warning", "error") of a chunk Claude's adapter marks as
/// its own information, not the agent's words.
fn informational(c: &agent_client_protocol::v1::schema::ContentChunk) -> Option<&str> {
    let own = c.meta.as_ref()?.get("claudeCode")?;
    (own.get("kind")?.as_str()? == "informational")
        .then(|| own.get("level").and_then(|l| l.as_str()).unwrap_or("info"))
}

/// What was said in a session, as its record `events` has it: what the
/// agent said in its last turn (`last`), or all of it, with what you said
/// led by "You:".
pub fn said(events: Vec<store::Event>, last: bool) -> String {
    let mut thread = Session::new(0, "", std::path::PathBuf::new());
    thread.replay(events);
    let from = if last {
        (thread.items.iter())
            .rposition(|i| matches!(i, Item::You(_)))
            .map_or(0, |at| at + 1)
    } else {
        0
    };
    (thread.items[from..].iter())
        .filter_map(|i| match i {
            Item::You(text) => Some(format!("You: {text}")),
            Item::Agent(text) => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_sent_beside_the_line_is_named_again_in_the_thread_by_where_it_came_from() {
        use super::{Context, file_of};
        use agent_client_protocol::v1::schema::{
            EmbeddedResource, EmbeddedResourceResource, TextResourceContents,
        };
        let sent = |uri: String| {
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(TextResourceContents::new("x", uri)),
            ))
        };
        let quote = file_of(&sent(Context::Quote.uri(2))).unwrap();
        assert_eq!(quote, ("Quote".into(), "u0://quote/2".into()));
        let terminal = file_of(&sent(Context::Terminal.uri(1))).unwrap();
        assert_eq!(terminal.0, "Terminal output");
        assert!(file_of(&sent("file:///a.txt".into())).is_none());
    }

    #[test]
    fn a_goal_is_set_paused_and_cleared_by_what_you_say_and_what_the_agent_answers() {
        use super::Goal;
        let goal = Goal::asked("/goal all tests pass", None);
        assert_eq!(
            goal.as_ref().map(|g| g.objective.as_str()),
            Some("all tests pass")
        );
        let paused = Goal::asked("/goal pause", goal.clone()).unwrap();
        assert!(paused.paused);
        assert!(!Goal::asked("/goal resume", Some(paused)).unwrap().paused);
        assert_eq!(Goal::asked("/goal", goal.clone()), goal);
        assert_eq!(Goal::asked("/goals are nice", goal.clone()), goal);
        assert_eq!(Goal::asked("/goal clear", goal), None);
        let set = Goal::said("Goal set: the build is green").unwrap().unwrap();
        assert_eq!(set.objective, "the build is green");
        let active = Goal::said("Goal active: lint is clean (3 turns)")
            .unwrap()
            .unwrap();
        assert_eq!(active.objective, "lint is clean");
        assert_eq!(Goal::said("Goal cleared: done"), Some(None));
        assert_eq!(Goal::said("Working on it"), None);
    }

    use super::*;

    fn update(json: &str) -> SessionUpdate {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_form_asked_for_waits_on_you_until_answered() {
        let mut s = Session::new(1, "claude-acp", PathBuf::from("/tmp"));
        let request: CreateElicitationRequest = serde_json::from_str(
            r#"{"mode":"form","sessionId":"s","message":"Which?","requestedSchema":{"type":"object","properties":{"db":{"type":"string","enum":["pg"]}}}}"#,
        )
        .unwrap();
        s.elicited(3, request);
        assert!(s.asks() && s.eliciting().is_some_and(|e| e.form.is_some()));
        s.elicit_answered(3, "db: pg");
        assert!(!s.asks());
    }

    #[test]
    fn a_compaction_is_said_once_done_and_as_trouble_when_it_fails() {
        let mut s = Session::new(1, "claude-acp", PathBuf::from("/tmp"));
        s.apply(update(
            r#"{"sessionUpdate":"compaction_update","compactionId":"c","status":"in_progress"}"#,
        ));
        assert!(s.items.is_empty());
        s.apply(update(
            r#"{"sessionUpdate":"compaction_update","compactionId":"c","status":"completed"}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"compaction_update","compactionId":"d","status":"failed","error":"too big"}"#,
        ));
        assert!(matches!(
            &s.items[..],
            [Item::Note { trouble: false, .. }, Item::Note { text, trouble: true }]
                if text.ends_with("too big")
        ));
    }

    #[test]
    fn a_session_names_its_subagents_for_its_index() {
        let mut s = Session::new(1, "claude-acp", PathBuf::from("/tmp"));
        s.apply(update(
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1"}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1","title":"Read the papers"}"#,
        ));
        assert_eq!(
            s.meta.subagents,
            [("c1".to_owned(), "Read the papers".to_owned())]
        );
    }

    #[test]
    fn a_subagent_is_at_work_waits_on_you_and_is_done_as_its_parent_says() {
        let sub = |json: &str| match update(json) {
            SessionUpdate::SubagentUpdate(u) => u,
            other => panic!("{other:?}"),
        };
        let mut s = Sub::default();
        s.take(&sub(
            r#"{"sessionUpdate":"subagent_update","sessionId":"c","state":{"state":"running"},"capabilities":{"cancel":{}}}"#,
        ));
        assert!(s.working() && s.stoppable && !s.needs_you);
        s.take(&sub(r#"{"sessionUpdate":"subagent_update","sessionId":"c","state":{"state":"requires_action"}}"#));
        assert!(s.working() && s.needs_you);
        s.take(&sub(
            r#"{"sessionUpdate":"subagent_update","sessionId":"c","state":{"state":"idle"}}"#,
        ));
        assert!(!s.working() && !s.needs_you && s.took.is_some());
    }

    #[test]
    fn what_the_adapter_says_of_its_own_stays_out_of_the_thread() {
        let mut s = Session::new(1, "claude-acp", PathBuf::from("/tmp"));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"\n\n> **Warning:** A hook gave no usable answer, so it didn't check this action.\n\n"},"_meta":{"claudeCode":{"kind":"informational","level":"warning"}}}"#,
        ));
        assert!(s.items.is_empty());
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"> **Error:** the hook failed"},"_meta":{"claudeCode":{"kind":"informational","level":"error"}}}"#,
        ));
        assert!(
            matches!(&s.items[..], [Item::Note { text, trouble: true }] if text == "Error: the hook failed")
        );
    }

    #[test]
    fn a_command_is_read_from_its_input_before_its_title() {
        let tool = |input: Value| Tool {
            call: serde_json::from_value(serde_json::json!({
                "toolCallId": "t", "title": "`cargo test`", "kind": "execute", "rawInput": input,
            }))
            .unwrap(),
            started: None,
            took: None,
            starts: Vec::new(),
        };
        let line = |v: Value| tool(v).command_line();
        assert_eq!(line(serde_json::json!({"command": "ls -la"})), "ls -la");
        assert_eq!(
            line(serde_json::json!({"command": ["bash", "-lc", "rg -n x src"]})),
            "rg -n x src"
        );
        assert_eq!(
            line(serde_json::json!({"command": ["git", "status"]})),
            "git status"
        );
        assert_eq!(line(serde_json::json!({})), "cargo test");
    }

    #[test]
    fn a_record_read_back_builds_the_same_thread_without_recording_it_again() {
        let mut s = Session::new(1, "codex-acp", "/tmp/repo".into());
        s.said("fix it");
        s.apply(update(
            r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#,
        ));
        s.ended(Ok(StopReason::EndTurn), Vec::new());
        let log = std::mem::take(&mut s.log);
        // Each message's time goes before it, and a call's: seven lines
        // for four.
        assert_eq!(log.len(), 7);

        let mut again = Session::new(2, "codex-acp", "/tmp/repo".into());
        again.replay(log);
        assert!(again.log.is_empty());
        assert_eq!(again.items.len(), s.items.len());
        assert!(matches!(&again.items[0], Item::You(t) if t == "fix it"));
        assert!(matches!(again.items[2], Item::Agent(_)));
        assert!(matches!(again.items[3], Item::Ended { .. }));
    }

    #[test]
    fn a_call_read_back_takes_as_long_as_its_record_says_and_one_untimed_says_nothing() {
        let call = |status: &str| {
            store::Event::Update(Box::new(update(&format!(
                r#"{{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"{status}"}}"#
            ))))
        };
        let begun = store::Event::Update(Box::new(update(
            r#"{"sessionUpdate":"tool_call","toolCallId":"t","title":"ls","kind":"execute","status":"in_progress"}"#,
        )));
        let took = |events: Vec<store::Event>| {
            let mut s = Session::new(1, "codex-acp", "/tmp/repo".into());
            s.replay(events);
            s.tools().next().and_then(Tool::took_said)
        };
        let timed = vec![
            store::Event::At(100),
            begun.clone(),
            store::Event::At(107),
            call("completed"),
        ];
        assert_eq!(took(timed), Some(Duration::from_secs(7)));
        assert_eq!(took(vec![begun, call("completed")]), None);
    }

    #[test]
    fn chunks_of_one_message_join_and_a_tool_call_between_starts_a_new_one() {
        let mut s = Session::new(1, "codex-acp", "/tmp/repo".into());
        s.said("fix it");
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Look"}}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ing"}}"#,
        ));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read lib.rs","kind":"read","status":"in_progress"}"#));
        s.apply(update(
            r#"{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done"}}"#,
        ));
        assert_eq!(s.meta.title, "fix it");
        assert!(matches!(&s.items[1], Item::Agent(t) if t == "Looking"));
        assert!(
            matches!(&s.items[2], Item::Tool(t) if t.call.status == ToolCallStatus::Completed && t.took.is_some())
        );
        assert!(matches!(&s.items[3], Item::Agent(t) if t == "Done"));
    }

    #[test]
    fn a_question_waits_until_answered_and_a_stopped_turn_cancels_it() {
        let mut s = Session::new(1, "claude-acp", "/tmp".into());
        s.said("go");
        let request: RequestPermissionRequest = serde_json::from_str(
            r#"{"sessionId":"s","toolCall":{"toolCallId":"t","title":"rm -rf build"},"options":[{"optionId":"a","name":"Allow","kind":"allow_once"}]}"#,
        )
        .unwrap();
        s.asked(5, request);
        assert_eq!(s.waiting().unwrap().title, "rm -rf build");
        s.ended(Ok(StopReason::Cancelled), Vec::new());
        assert!(s.waiting().is_none());
        assert!(matches!(
            s.items.last(),
            Some(Item::Ended {
                reason: StopReason::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn a_worktree_is_made_only_once_git_has_said_the_folder_is_a_repository() {
        let mut s = Session::new(1, "x", "/r".into());
        s.wants_worktree = true;
        assert!(!s.needs_worktree());
        s.repo = true;
        assert!(s.needs_worktree());
    }

    #[test]
    fn a_session_moved_into_its_worktree_ran_on_the_worktrees_branch() {
        let mut s = Session::new(1, "x", "/r".into());
        s.branch = Some("main".into());
        // Sent from the draft, on the folder's branch, before the worktree.
        s.ran_on.clone_from(&s.branch);
        let wt = git::Worktree {
            repo: "/r".into(),
            path: "/w/r-1".into(),
            branch: "u0/flow".into(),
            repos: Vec::new(),
        };
        s.into_worktree(wt, Vec::new());
        assert_eq!(s.meta.folder, PathBuf::from("/w/r-1"));
        assert_eq!(s.ran_on, s.branch, "no branch changed under it");
        assert_eq!(s.ran_on.as_deref(), Some("u0/flow"));
    }

    #[test]
    fn a_draft_in_a_folder_of_repositories_wants_a_worktree_of_the_one_chosen() {
        let mut s = Session::new(1, "x", "/w".into());
        s.wants_worktree = true;
        s.holds(false, vec!["/w/a".into(), "/w/b".into()]);
        assert_eq!(s.git_dir(), Some(PathBuf::from("/w/a")));
        assert!(s.needs_worktree());
        s.git.repo = Some("/w/b".into());
        s.holds(false, vec!["/w/a".into(), "/w/b".into()]);
        assert_eq!(s.git_dir(), Some(PathBuf::from("/w/b")), "the choice stays");
        s.holds(false, Vec::new());
        assert!(!s.needs_worktree());
    }

    #[test]
    fn the_files_changed_are_counted_once_each() {
        let mut s = Session::new(1, "x", "/r".into());
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"x\n","newText":"y\nz\n"}]}"#));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"b","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"y\n","newText":"w\n"}]}"#));
        let files = s.changed();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/r/a.rs"));
        assert_eq!(files[0].change, Change::Edited);
        assert_eq!((files[0].added, files[0].gone), (3, 2));
    }

    #[test]
    fn a_thought_is_timed_when_the_agent_moves_on_and_a_plan_update_replaces_the_last() {
        let mut s = Session::new(1, "x", "/r".into());
        s.said("go");
        s.apply(update(
            r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}"#,
        ));
        s.apply(update(r#"{"sessionUpdate":"plan","entries":[{"content":"a","priority":"high","status":"pending"}]}"#));
        s.apply(update(r#"{"sessionUpdate":"plan","entries":[{"content":"a","priority":"high","status":"completed"}]}"#));
        assert!(matches!(&s.items[1], Item::Thought { secs: Some(_), .. }));
        assert_eq!(s.items.len(), 3);
    }

    #[test]
    fn a_turn_keeps_one_plan_brought_up_to_date_where_it_first_showed() {
        use agent_client_protocol::v1::schema::PlanEntryStatus;
        let plan = |status: &str| {
            update(&format!(
                r#"{{"sessionUpdate":"plan","entries":[{{"content":"a","priority":"high","status":"{status}"}}]}}"#
            ))
        };
        let plans = |s: &Session| -> Vec<usize> {
            (0..s.items.len())
                .filter(|&k| matches!(s.items[k], Item::Plan(_)))
                .collect()
        };
        let mut s = Session::new(1, "x", "/r".into());
        s.said("go");
        s.apply(plan("pending"));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#));
        s.apply(plan("completed"));
        assert_eq!(plans(&s), [1]);
        assert!(
            matches!(&s.items[1], Item::Plan(p) if p.entries[0].status == PlanEntryStatus::Completed)
        );
        // The next turn's plan is a card of its own.
        s.said("more");
        s.apply(plan("pending"));
        assert_eq!(plans(&s), [1, 4]);
    }

    #[test]
    fn codexs_own_answer_to_a_new_session_is_read_with_its_modes_and_options() {
        // Recorded from @agentclientprotocol/codex-acp 2.10.0.
        let r: agent_client_protocol::v1::schema::NewSessionResponse =
            serde_json::from_str(include_str!("fixtures/codex-session-new.json")).unwrap();
        assert!(r.modes.is_some());
        let options = r.config_options.unwrap();
        assert!(options.iter().any(|o| &*o.id.0 == "model"));
    }

    #[test]
    fn a_fork_before_a_message_keeps_up_to_what_the_agent_last_said_and_its_record() {
        let chunk = |kind: &str, id: &str, text: &str| {
            update(&format!(
                r#"{{"sessionUpdate":"{kind}","messageId":"{id}","content":{{"type":"text","text":"{text}"}}}}"#
            ))
        };
        let events = vec![
            store::Event::Update(Box::new(chunk("user_message_chunk", "u1", "first"))),
            store::Event::Update(Box::new(chunk("agent_thought_chunk", "t1", "hmm"))),
            store::Event::Update(Box::new(chunk("agent_message_chunk", "a1", "one"))),
            store::Event::Update(Box::new(chunk("agent_message_chunk", "a1", " two"))),
            store::Event::Update(Box::new(chunk("user_message_chunk", "u2", "[stopped]"))),
            store::Event::Update(Box::new(chunk("user_message_chunk", "u3", "go on"))),
            store::Event::You("second".into()),
            store::Event::Update(Box::new(chunk("agent_message_chunk", "a2", "three"))),
        ];
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.replay(events.clone());
        let second = s
            .items
            .iter()
            .rposition(|i| matches!(i, Item::You(_)))
            .unwrap();
        assert_eq!(s.fork_point(second), Some(MessageId::new("u3")));
        let stopped = s
            .items
            .iter()
            .position(|i| matches!(i, Item::You(t) if t == "[stopped]"))
            .unwrap();
        assert_eq!(s.fork_point(stopped), Some(MessageId::new("a1")));
        // Your first message has nothing before it to keep.
        assert_eq!(s.fork_point(0), None);
        // Two messages of yours in a row stay two.
        assert_eq!(
            s.items.iter().filter(|i| matches!(i, Item::You(_))).count(),
            4
        );
        let kept = Session::record_before(events, 1);
        assert_eq!(kept.len(), 4);
        assert!(!kept.iter().any(|e| matches!(e, store::Event::You(_))));
    }

    #[test]
    fn a_file_or_a_long_paste_that_went_with_your_message_shows_as_a_file() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.said("read these");
        s.apply(update(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"resource_link","name":"a.pdf","uri":"file:///tmp/a.pdf"}}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"resource","resource":{"uri":"u0://paste/2","text":"long"}}}"#,
        ));
        assert!(
            matches!(&s.items[1], Item::File { name, uri } if name == "a.pdf" && uri == "file:///tmp/a.pdf")
        );
        assert!(matches!(&s.items[2], Item::File { name, .. } if name == "Pasted text"));
        assert!(matches!(&s.items[0], Item::You(t) if t == "read these"));
    }

    #[test]
    fn an_image_that_went_with_your_message_shows_as_yours() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.said("look");
        s.apply(update(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"image","mimeType":"image/png","data":"iVBORw0KGgo="}}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"image","mimeType":"image/png","data":"iVBORw0KGgo="}}"#,
        ));
        assert!(matches!(s.items[1], Item::Image { yours: true, .. }));
        assert!(matches!(s.items[2], Item::Image { yours: false, .. }));
    }

    #[test]
    fn when_each_message_began_is_kept_and_read_back() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.said("go");
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}"#,
        ));
        assert!(s.time_of(0).is_some() && s.time_of(1).is_some());
        let mut again = Session::new(2, "x", std::env::temp_dir());
        again.replay(std::mem::take(&mut s.log));
        assert_eq!(
            again.time_of(0).map(store::secs),
            s.time_of(0).map(store::secs)
        );
        assert_eq!(
            again.time_of(1).map(store::secs),
            s.time_of(1).map(store::secs)
        );
    }

    #[test]
    fn a_subagent_set_to_work_is_one_row_named_as_the_agent_names_it() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.apply(update(
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1"}"#,
        ));
        s.apply(update(r#"{"sessionUpdate":"subagent_update","sessionId":"c1","title":"Explore the uploader"}"#));
        assert_eq!(s.items.len(), 1);
        assert!(
            matches!(&s.items[0], Item::Child { title, .. } if title == "Explore the uploader")
        );
    }

    #[test]
    fn a_turn_that_failed_says_so_in_a_list() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.said("go");
        s.ended(Err("the agent stopped".into()), Vec::new());
        assert_eq!(s.outcome(), Some("failed"));
        s.said("again");
        s.ended(Ok(StopReason::EndTurn), Vec::new());
        assert_eq!(s.outcome(), None);
    }
}
