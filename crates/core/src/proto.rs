//! The u0 protocol: what the desk asks of the server (a [`Call`]) and
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
    SessionConfigOption, SessionId, SessionInfo, SessionModeState, StopReason, ToolCallId,
};
use serde::{Deserialize, Serialize};

use crate::{
    acp::{self, Error},
    git,
    registry::{self, Entry},
    repo::{GitAction, Held, Snapshot},
    scripts::Script,
    session::Begin,
    store, tasks, tidy, workflows, writer,
};

/// Bytes, as base64 on the wire: a third more than they are, where
/// JSON's numbers take three or four times (a picture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine as _;
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use base64::Engine as _;
        let text = String::deserialize(d)?;
        (base64::engine::general_purpose::STANDARD.decode(text))
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

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
    /// where you gave one) with the variables `env` (as in the settings'
    /// `env`) and say hello to it.
    Start {
        entry: Entry,
        command: Option<String>,
        env: std::collections::BTreeMap<String, Option<String>>,
    },
    /// Keep `value` secret as `account` (`secrets`), or nothing, with none;
    /// answered as a `Notice` where it could not be.
    Secret {
        account: String,
        value: Option<String>,
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
    /// The sessions `agent` keeps in each of the projects `places` and in
    /// u0's worktrees of them, those in `under`.
    List {
        agent: String,
        places: Vec<PathBuf>,
        under: Vec<PathBuf>,
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
    /// Have a writer write something for a session.
    Write(writer::Order),
    /// Say `content` in `session`; the turn's end comes back.
    Prompt {
        agent: String,
        key: u64,
        session: SessionId,
        content: Vec<ContentBlock>,
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
        answer: acp::Answer,
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
    /// u0's processes, and which are the agents running.
    Weigh,
    /// Something of telemetry: a count from the window, or what is sent.
    Telemetry(crate::telemetry::Ask),
    /// Send the process `pid`, one of u0's, the signal `signal` (TERM,
    /// KILL); answered as a `Notice`.
    Signal {
        pid: u32,
        signal: String,
    },
    /// The warnings and errors in u0's log, newest first.
    Troubles,
    /// What of each of `agents` is here.
    Here {
        agents: Vec<Entry>,
        /// Your commands for them, by id.
        commands: std::collections::BTreeMap<String, String>,
    },
    /// Remove the download of `entry`; answered with what of it is here.
    Uninstall {
        entry: Entry,
    },
    /// What `cli` says its version is.
    CliVersion {
        cli: PathBuf,
    },
    /// The index of the sessions kept: answered with [`Event::Index`].
    Watch,
    /// Keep these settings in the server's home.
    SaveSettings(Box<crate::settings::Settings>),
    /// Something of sharing this u0 with other machines.
    Share(crate::share::Share),
    /// These sessions' metas are now as given (each by its agent and id).
    Keep(Vec<store::Meta>),
    /// Take the session `id` of `agent` out of the index, its record with
    /// it; `keep_out`: its agent cannot delete it, so that its list does not
    /// bring it back.
    Forget {
        agent: String,
        id: SessionId,
        keep_out: bool,
    },
    /// What you put aside from the composer, the latest last.
    Stash(Vec<String>),
    /// Add `events`, what the desk made (what you said, a turn's end, a
    /// note), to the end of the record of the session `id`.
    Append {
        agent: String,
        id: SessionId,
        events: Vec<store::Event>,
    },
    /// Drop the record of the session `id`: its agent loads it again.
    DropRecord {
        agent: String,
        id: SessionId,
    },
    /// Read back the record of the session `id`, after all that is being
    /// written to it, for the session `key`.
    Read {
        key: u64,
        agent: String,
        id: SessionId,
    },
    /// Begin the record of the session `to`, branched off `from`, with
    /// `from`'s up to its message after the first `yours`.
    CopyRecord {
        agent: String,
        from: SessionId,
        to: SessionId,
        yours: usize,
    },
    /// The sessions among `records` whose records hold `query` (lower case).
    Search {
        query: String,
        records: Vec<(u64, String, SessionId)>,
    },
    /// What each of the sessions `records` (by key, agent and id) used,
    /// as their records say.
    Used {
        records: Vec<(u64, String, SessionId)>,
    },
    /// What git says of `folder`, where the session `key` works.
    Look {
        key: u64,
        folder: PathBuf,
    },
    /// Put `script` at `at` in the `u0.json` of the repository at `dir`
    /// (none: take the one at `at` away), then look again.
    SaveScript {
        dir: PathBuf,
        at: Option<usize>,
        script: Option<Script>,
        key: u64,
    },
    /// What the project `folder` holds.
    ReadProject {
        folder: PathBuf,
    },
    /// What is not committed in `dir`, or what changed there since the
    /// session's first checkpoint, `since`.
    Uncommitted {
        dir: PathBuf,
        key: u64,
        since: Option<String>,
        no_spacing: bool,
    },
    /// What changed in `scope` where the session `key` works.
    ScopeDiff {
        dir: PathBuf,
        key: u64,
        scope: git::Scope,
        base: Option<String>,
        no_spacing: bool,
    },
    /// The recent commits and local branches where the session `key` works.
    Commits {
        dir: PathBuf,
        key: u64,
    },
    /// The pull request of the branch where the session `key` works.
    ReadPr {
        dir: PathBuf,
        key: u64,
    },
    /// What `gh` says of the pull request `number` of the project `dir`.
    PrText {
        dir: PathBuf,
        number: u64,
    },
    /// The open pull requests and issues of `folder`.
    ListPrs {
        folder: PathBuf,
    },
    /// The files of `folder` git sees.
    ListFiles {
        folder: PathBuf,
    },
    /// The folders in `dir`, to add one as a project.
    Folders {
        dir: PathBuf,
    },
    /// Make the folder `dir`, where it is not yet: a project's, a scratch
    /// session's. Said only where it could not be made.
    MakeFolder {
        dir: PathBuf,
    },
    /// Ask the pull requests or issues of the project in `folder`, from
    /// the host of its remote; its issues from the Linear team keyed
    /// `linear`, where it is bound to one.
    Forge {
        folder: PathBuf,
        kind: crate::github::Kind,
        ask: crate::github::Ask,
        linear: Option<String>,
    },
    /// The pictures at `urls` that the pull request or issue `number` of
    /// the project in `folder` shows: only those the server found there
    /// as it read it (`pictures::of`) are fetched, each answered as an
    /// `Event::Picture` to the desk that asked alone (`Server::pictures`).
    Pictures {
        folder: PathBuf,
        kind: crate::github::Kind,
        number: u64,
        urls: Vec<String>,
    },
    /// What the file at `path` holds, for the column's viewer (`full` loads
    /// past the minified-file preview cap).
    ReadFile {
        path: PathBuf,
        #[serde(default)]
        full: bool,
    },
    /// Unpack the `.zip` archive `archive` into `dest`.
    ExtractArchive {
        archive: PathBuf,
        dest: PathBuf,
    },
    /// Copy an outside-workspace file `from` into the project at `to`.
    CopyFile {
        from: PathBuf,
        to: PathBuf,
    },
    /// Where `words` are in the files of `folder` git sees, looked for
    /// `how`.
    Grep {
        folder: PathBuf,
        words: String,
        how: git::Match,
    },
    /// Make the new project `name` in `dir`: a folder, a repository; and
    /// (`publish`) a private repository of yours on GitHub, pushed to.
    Create {
        dir: PathBuf,
        name: String,
        publish: bool,
    },
    /// Clone `url` into `dir`; the clone going on stops at `CancelClone`.
    Clone {
        url: String,
        dir: PathBuf,
    },
    CancelClone,
    /// Fetch from `origin` for each project folder; those marked, on their
    /// default branch with nothing changed, moved on to it. Nothing is
    /// said back: what fails is only logged.
    Refresh {
        projects: Vec<(PathBuf, bool)>,
    },
    /// Make a worktree of `dir` in `under` for the session `key`, about
    /// `about`, on a branch beginning with `prefix`, from `from` (where
    /// `dir` is, if nothing).
    MakeWorktree {
        dir: PathBuf,
        about: String,
        under: PathBuf,
        prefix: String,
        from: Option<Begin>,
        submodules: crate::settings::Submodules,
        key: u64,
    },
    /// Take the checkpoint `name` of the turn `turn`; after it (`after`),
    /// what changed since the turn's checkpoint before and the session's
    /// first; `then`, what is said once it is taken.
    Checkpoint {
        dir: PathBuf,
        name: String,
        key: u64,
        turn: usize,
        after: bool,
        before: Option<String>,
        first: Option<String>,
        then: Option<String>,
        no_spacing: bool,
    },
    /// Put the files back as the checkpoint `target` had them.
    Restore {
        dir: PathBuf,
        target: String,
        name: String,
        first: Option<String>,
        key: u64,
        turn: usize,
        undo: bool,
        no_spacing: bool,
    },
    /// Do `action` in `dir`, leaving `left_out` out of a commit.
    Git {
        dir: PathBuf,
        action: GitAction,
        prefix: String,
        left_out: Vec<String>,
        key: u64,
    },
    /// Drop the checkpoints of the session `uid` in the repository at `dir`.
    DropCheckpoints {
        dir: PathBuf,
        uid: String,
    },
    /// Where each diff of the calls of the session `key` starts in its file.
    Place {
        key: u64,
        calls: Vec<(ToolCallId, Vec<(PathBuf, String)>)>,
    },
    /// The simulators and emulators here.
    Devices,
    /// Do `act` to `device`; then the devices, listed again.
    Device {
        device: crate::devices::Device,
        act: crate::devices::Act,
    },
    /// What `device` shows now.
    Screen {
        device: crate::devices::Device,
    },
    /// Install `AXe` with Homebrew, as you asked in the Devices tab; then
    /// the devices, listed again.
    InstallAxe,
    /// How much each part of u0's home takes, and what can be tidied.
    Sizes {
        kept: tidy::Kept,
    },
    /// Tidy `what` around what is `kept`, then measure again.
    Tidy {
        what: tidy::Tidy,
        kept: tidy::Kept,
    },
    /// What is known of each forge here (`forge::seen`).
    Forges,
    /// Check who is signed in to this forge, and list the forges again.
    CheckForge(crate::forge::Host),
    /// Secret providers and named secret references on this machine.
    Secrets,
    /// Save or update a named secret reference, storing `value` in the
    /// Keychain when applicable.
    SetSecretRef {
        entry: crate::secrets::SecretRef,
        value: Option<String>,
    },
    /// Remove a named secret reference and any Keychain item for it.
    DropSecretRef(String),
    /// The repositories `gh` can clone (`github::repos`).
    Repos,
    /// Open the terminal `id` in `dir`, running
    /// `shell` (your login shell when none), `size` rows by columns, `first`
    /// typed into it; one open already is said again.
    TermOpen {
        id: u64,
        dir: PathBuf,
        shell: Option<String>,
        first: Option<String>,
        size: (u16, u16),
    },
    /// What was typed or pasted in the terminal `id`.
    TermInput {
        id: u64,
        bytes: Vec<u8>,
    },
    TermResize {
        id: u64,
        size: (u16, u16),
    },
    /// Close the terminal `id`; one with a command running in it only when
    /// `force`d.
    TermClose {
        id: u64,
        force: bool,
    },
    /// Keep `task` as it is now: a new one (its id 0) is given an id.
    /// Answered, as each change to them is, with [`Event::Tasks`].
    SetTask(tasks::Task),
    /// Take the task `id` away.
    DropTask(u64),
    /// Run the task `id` now, whatever its schedule says.
    RunTask(u64),
    /// Say `prompt` to `agent` in its session `session` (its next turn,
    /// the one going on let end first), or in a new one in `project`:
    /// answered, under `tag`, as the session is known and as its turn
    /// ends, with [`Event::Turned`]. What `u0 thread` asks.
    Say {
        tag: u64,
        agent: String,
        session: Option<SessionId>,
        project: PathBuf,
        prompt: String,
    },
    /// Give the webhook task `id` a new address, its secret moved to it:
    /// the old one runs it no more.
    RotateHook(u64),
    /// The workflows and their runs: answered, as each change to a run
    /// is, with [`Event::Workflows`].
    Workflows,
    /// Save or update a workflow definition (`workflows/<name>.json`).
    SetWorkflow(workflows::Workflow),
    /// Remove a workflow definition (`workflows/<name>.json`).
    DropWorkflow(String),
    /// Begin a run of the workflow `workflow` with `input`.
    StartFlow {
        workflow: String,
        input: String,
    },
    /// Re-run `run` starting from step index `step`.
    RerunFlowFrom {
        run: u64,
        step: usize,
    },
    /// Answer the question the run `run` waits on.
    AnswerFlow {
        run: u64,
        choice: String,
    },
    /// Take the paused run up again, at the step it stopped at.
    ResumeFlow(u64),
    CancelFlow(u64),
    /// Give agents outside u0 a new key to its tools (`mcp.json`): the
    /// old one lets no one in. Answered as a `Notice`.
    NewToolsKey,
}

impl Call {
    /// Where it only reads, what of the disk it reads (nothing but
    /// u0's own index, records and registry: none); `None` where it
    /// changes something, runs a program, or acts as you anywhere. Each
    /// call is named here, so that a new one is placed as it is added: a
    /// device that may only read is let make those that read
    /// (`access::Scope::Read`).
    pub fn reads(&self) -> Option<Vec<&std::path::Path>> {
        use crate::{github::Ask, share::Share, telemetry::Ask as Count};
        Some(match self {
            Self::Fetch
            | Self::Weigh
            | Self::Troubles
            | Self::Here { .. }
            | Self::Watch
            | Self::Share(Share::Look)
            | Self::Read { .. }
            | Self::Search { .. }
            | Self::Used { .. }
            | Self::List { .. }
            | Self::Forges
            | Self::Workflows
            | Self::Devices
            | Self::Screen { .. }
            | Self::Telemetry(Count::Opened(_) | Count::Window(_) | Count::Show(_)) => Vec::new(),
            Self::Look { folder, .. }
            | Self::ReadProject { folder }
            | Self::ListPrs { folder }
            | Self::ListFiles { folder }
            | Self::Grep { folder, .. }
            | Self::Pictures { folder, .. }
            | Self::Forge {
                folder,
                ask:
                    Ask::List { .. }
                    | Ask::View(_)
                    | Ask::Diff(_)
                    | Ask::Candidates
                    | Ask::Log(_)
                    | Ask::Templates,
                ..
            } => vec![folder.as_path()],
            Self::Uncommitted { dir, .. }
            | Self::ScopeDiff { dir, .. }
            | Self::Commits { dir, .. }
            | Self::ReadPr { dir, .. }
            | Self::PrText { dir, .. }
            | Self::Folders { dir } => vec![dir.as_path()],
            Self::ReadFile { path, .. } => vec![path.as_path()],
            Self::Sizes { kept } => kept
                .under
                .iter()
                .chain(&kept.projects)
                .map(PathBuf::as_path)
                .collect(),
            Self::Place { calls, .. } => (calls.iter())
                .flat_map(|(_, files)| files.iter().map(|(path, _)| path.as_path()))
                .collect(),
            Self::Install { .. }
            | Self::CancelInstall { .. }
            | Self::InstallAxe
            | Self::Start { .. }
            | Self::Secret { .. }
            | Self::Stop { .. }
            | Self::Kill { .. }
            | Self::Open { .. }
            | Self::Reopen { .. }
            | Self::Probe { .. }
            | Self::Fork { .. }
            | Self::Delete { .. }
            | Self::Write(_)
            | Self::Prompt { .. }
            | Self::Cancel { .. }
            | Self::Answer { .. }
            | Self::SetMode { .. }
            | Self::SetOption { .. }
            | Self::SignIn { .. }
            | Self::SignOut { .. }
            | Self::Telemetry(Count::Forget)
            | Self::Signal { .. }
            | Self::Uninstall { .. }
            | Self::CliVersion { .. }
            | Self::SaveSettings(_)
            | Self::Share(_)
            | Self::Keep(_)
            | Self::Forget { .. }
            | Self::Stash(_)
            | Self::Append { .. }
            | Self::DropRecord { .. }
            | Self::CopyRecord { .. }
            | Self::SaveScript { .. }
            | Self::MakeFolder { .. }
            | Self::ExtractArchive { .. }
            | Self::CopyFile { .. }
            | Self::Forge { .. }
            | Self::Repos
            | Self::Create { .. }
            | Self::Clone { .. }
            | Self::CancelClone
            | Self::Refresh { .. }
            | Self::MakeWorktree { .. }
            | Self::Checkpoint { .. }
            | Self::Restore { .. }
            | Self::Git { .. }
            | Self::DropCheckpoints { .. }
            | Self::Tidy { .. }
            | Self::TermOpen { .. }
            | Self::TermInput { .. }
            | Self::TermResize { .. }
            | Self::TermClose { .. }
            | Self::SetTask(_)
            | Self::DropTask(_)
            | Self::RunTask(_)
            | Self::Device { .. }
            | Self::Say { .. }
            | Self::RotateHook(_)
            | Self::SetWorkflow(_)
            | Self::DropWorkflow(_)
            | Self::StartFlow { .. }
            | Self::RerunFlowFrom { .. }
            | Self::AnswerFlow { .. }
            | Self::ResumeFlow(_)
            | Self::CancelFlow(_)
            | Self::CheckForge(_)
            | Self::Secrets
            | Self::SetSecretRef { .. }
            | Self::DropSecretRef(_)
            | Self::NewToolsKey => return None,
        })
    }
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
    /// Lines of the record of the session `id` of `agent`, made of what
    /// the agent said in it, as they were written.
    Appended {
        agent: String,
        id: SessionId,
        events: Vec<store::Event>,
    },
    /// Something an agent said on its own, outside the records: a question,
    /// a task, its end, and what it said in a session not open with it.
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
    /// The sessions of `agent` its CLI runs now in a process of its own
    /// outside u0 (a terminal's), as it says: each of the others is not.
    Running {
        agent: String,
        sessions: Vec<SessionId>,
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
    /// What a writer wrote for the session `key`, or why it did not.
    Written {
        key: u64,
        ask: writer::Ask,
        result: Result<String, String>,
    },
    Ended {
        key: u64,
        result: Result<StopReason, Error>,
        /// What the turn took, as its agent said.
        tokens: Vec<crate::usage::Tokens>,
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
    /// u0's processes (`diagnostics`), and the agents running among
    /// them: each one's id and process id.
    Weighed {
        agents: Vec<(String, u32)>,
        processes: crate::diagnostics::Processes,
    },
    /// The warnings and errors in u0's log, newest first.
    Troubles(Vec<crate::diagnostics::LogTrouble>),
    /// The batch of telemetry that goes next at `level`, as JSON, and this
    /// install's id.
    Telemetry {
        level: crate::telemetry::Level,
        batch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        install_id: Option<String>,
    },
    /// What of each agent the registry lists is here, by its id.
    Here(HashMap<String, Here>),
    /// What a CLI of yours says its version is.
    CliVersion {
        cli: PathBuf,
        version: String,
    },
    /// The index of the sessions kept, or why it does not read: then
    /// nothing is saved over it.
    Index(Result<store::Index, String>),
    /// How sharing this u0 stands.
    Shared(Box<crate::share::Shared>),
    /// The settings as the server keeps them now, changed there by one of
    /// u0's tools (a project added), not by the desk.
    Settings(Box<crate::settings::Settings>),
    /// The scheduled tasks as they are now, or why `tasks.json` does not
    /// read.
    Tasks(Result<Vec<tasks::Task>, String>),
    /// Each workflow, or why its file does not read (by name); the runs,
    /// or why `runs.json` does not read.
    Workflows {
        flows: Vec<Result<workflows::Workflow, (String, String)>>,
        runs: Result<Vec<workflows::Run>, String>,
    },
    /// What an agent asked in the session `session` of `agent` under
    /// `request` was answered with `option` by one of u0's tools, as
    /// you allowed: the desk marks its card so.
    Answered {
        agent: String,
        session: SessionId,
        request: u64,
        option: agent_client_protocol::v1::schema::PermissionOption,
    },
    /// A session organised by one of u0's tools (pinned, archived,
    /// renamed, tagged), as the index keeps it now: the desk takes those
    /// of it as if you changed them there.
    Organized(Box<store::Meta>),
    /// A task's turn in the session `meta` begins (`ended` none), with the
    /// session's record so far where `record`; or it ended, how.
    /// What came of the `Call::Say` of `tag`: its session, or why it was
    /// not said; and once its turn ends, what the agent answered.
    /// The pairing link this desk asked for (`Share::Link`), when it was
    /// made and what it lets in: said to that desk alone, never to every
    /// link.
    Offered {
        link: String,
        made: u64,
        grant: crate::access::Grant,
    },
    Turned {
        tag: u64,
        session: Result<SessionId, String>,
        answer: Option<String>,
    },
    TaskTurn {
        meta: Box<store::Meta>,
        record: Option<Vec<store::Event>>,
        ended: Option<Result<StopReason, String>>,
    },
    /// The record of the session `key`, read back: none where there is none.
    Record {
        key: u64,
        events: Option<Vec<store::Event>>,
    },
    /// The sessions whose records hold `query`: each by its key, the item
    /// of its thread where it was said last, and that line.
    Found {
        query: String,
        said: Vec<(u64, usize, String)>,
    },
    /// What sessions used, by key, as their agents said (`usage::used`).
    Used(Vec<(u64, Vec<crate::usage::Used>)>),
    /// What git says of `folder`, where the session `key` works: its
    /// branch, whether it is in a repository, and the scripts the
    /// repository's `u0.json` names.
    Looked {
        key: u64,
        folder: PathBuf,
        branch: Option<String>,
        repo: bool,
        repos: Vec<PathBuf>,
        #[serde(default)]
        summaries: Vec<git::RepoSummary>,
        scripts: Vec<Script>,
    },
    /// What the folder of a project holds.
    Held {
        folder: PathBuf,
        held: Held,
    },
    /// The simulators and emulators here, and what is missing to have
    /// any.
    Devices {
        found: Vec<crate::devices::Device>,
        missing: Vec<String>,
        #[serde(default)]
        tools: Vec<crate::devices::ToolStatus>,
    },
    /// What the device `id` shows, as a PNG or a JPEG; or why it cannot be
    /// read.
    Screen {
        id: String,
        picture: Result<Vec<u8>, String>,
    },
    /// How `Call::InstallAxe` ended.
    AxeInstalled(Result<(), String>),
    /// Something done, to be said: what was done, or what was not and why.
    Notice(Result<String, (String, String)>),
    /// What is not committed where a session works, as git reads it.
    Uncommitted {
        key: u64,
        result: Result<Vec<git::FileDiff>, String>,
    },
    /// What changed in a scope git is asked for: the branch against its
    /// base, or one commit.
    Diffed {
        key: u64,
        scope: git::Scope,
        result: Result<Vec<git::FileDiff>, String>,
    },
    /// The recent commits, stashes and local branches where a session works.
    Commits {
        key: u64,
        commits: Vec<git::Commit>,
        #[serde(default)]
        stashes: Vec<git::Stash>,
        branches: Vec<String>,
        /// How far ahead of and behind its upstream, and whether it has an origin.
        upstream: (usize, usize, bool),
    },
    /// The pull request of the branch a session works on, as `gh` says.
    Pr {
        key: u64,
        pr: Option<Box<git::Pr>>,
    },
    /// What `gh` says of the pull request `number`, named with `@`.
    PrText {
        number: u64,
        text: Option<String>,
    },
    /// The open pull requests and issues of a project.
    Prs {
        folder: PathBuf,
        prs: Vec<(u64, String)>,
        issues: Vec<(u64, String)>,
    },
    /// The files of a project, for `@` to name.
    Files {
        folder: PathBuf,
        files: Vec<String>,
    },
    /// The folders in `dir`, by name; none where it cannot be read.
    Folders {
        dir: PathBuf,
        names: Vec<String>,
    },
    /// What came back of a project's pull requests or issues.
    Forge {
        folder: PathBuf,
        kind: crate::github::Kind,
        answer: crate::github::Answer,
    },
    /// The picture at `url`, as it came; or why it did not.
    Picture {
        url: String,
        picture: Result<Bytes, String>,
    },
    /// What the file at `path` holds.
    FileRead {
        path: PathBuf,
        body: Result<Box<crate::repo::Body>, String>,
    },
    /// What git grep found for `words`: each place as "path:line", and the
    /// line.
    Grepped {
        words: String,
        found: Vec<(String, String)>,
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
    /// A worktree was made for the session, with the scripts its
    /// `u0.json` names, or could not be.
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
    /// A step of a git action on its way.
    GitStep {
        key: u64,
        text: String,
    },
    /// A commit, a push, a pull request or a worktree removed: what it said.
    Git {
        key: u64,
        action: GitAction,
        result: Result<String, String>,
    },
    /// Where the diffs of calls of the session `key` start in their files.
    Placed {
        key: u64,
        starts: Vec<(ToolCallId, Vec<Option<usize>>)>,
    },
    /// How much each part of u0's home takes, and what a tidying did.
    Sizes(tidy::StorageReport),
    Tidied(String),
    /// What is known of each forge here.
    Forges(Vec<crate::forge::Seen>),
    /// Secret providers and named secret references here.
    Secrets {
        providers: Vec<crate::secrets::SecretProvider>,
        refs: Result<Vec<crate::secrets::SecretRef>, String>,
    },
    /// The repositories `gh` listed for cloning.
    Repos(Result<Vec<crate::github::Repo>, String>),
    /// The terminal `id` opened, called what it is called in its tab, or
    /// why it did not.
    TermOpened {
        id: u64,
        result: Result<String, String>,
    },
    /// What the terminal `id` printed: on opening one again, the last of
    /// all it printed.
    TermOutput {
        id: u64,
        bytes: Vec<u8>,
    },
    /// The shell of the terminal `id` ended, how.
    TermEnded {
        id: u64,
        how: String,
    },
    /// The terminal `id` closed, or was left (`busy`): a command runs in
    /// it.
    TermClosed {
        id: u64,
        busy: bool,
    },
}

impl Event {
    /// This event as a device that may only read is told it, where it
    /// carries what would let one act: a webhook task's address (its
    /// token) taken out; the settings as it reads them in its welcome
    /// (`Settings::for_reader`); the workflows without their steps'
    /// commands, their runs without what each step gave back or why one
    /// paused (a command's output may hold a key). None where it carries
    /// nothing such.
    pub fn for_reader(&self) -> Option<Self> {
        match self {
            Self::Settings(settings) => Some(Self::Settings(Box::new(settings.for_reader()))),
            Self::Secrets { .. } => Some(Self::Secrets {
                providers: Vec::new(),
                refs: Ok(Vec::new()),
            }),
            Self::Tasks(Ok(tasks)) => Some(Self::Tasks(Ok((tasks.iter().cloned())
                .map(|mut t| {
                    if let tasks::Schedule::Hook(hook) = &mut t.schedule {
                        hook.token.clear();
                    }
                    t
                })
                .collect()))),
            Self::Workflows { flows, runs } => Some(Self::Workflows {
                flows: (flows.iter().cloned())
                    .map(|flow| {
                        flow.map(|mut f| {
                            for step in &mut f.steps {
                                if let workflows::Step::Run { command, .. } = step {
                                    command.clear();
                                }
                            }
                            f
                        })
                    })
                    .collect(),
                runs: runs.clone().map(|runs| {
                    (runs.into_iter())
                        .map(|mut run| {
                            run.done.iter_mut().for_each(|d| d.result.clear());
                            if let workflows::State::Paused(why) = &mut run.state {
                                why.clear();
                            }
                            run
                        })
                        .collect()
                }),
            }),
            _ => None,
        }
    }
}

/// The version of what is said over a link (`link`): a desk and a server
/// that speak another are refused, each told to update.
pub const PROTOCOL: u32 = 5;

/// The first thing a server says over a link: a nonce made for it, for
/// the desk to sign (`key`).
#[derive(Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub nonce: String,
}

/// What a desk says to the [`Challenge`], and nothing before it: who it
/// is, and what it last heard from this server, to be told the rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// The public half of its key for this server (`ssh-ed25519 AAAA…`),
    /// and its signature of the challenge.
    pub key: String,
    pub signature: String,
    /// As it pairs: the code made on the server, which lets its key in.
    pub pair: Option<Pair>,
    /// The server's boot and the number of the last thing it said that
    /// this desk heard, when it comes back.
    pub since: Option<(u64, u64)>,
    /// What the link is for.
    pub then: Then,
    /// Build version of the desk connecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// What a link is for, once its device is let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Then {
    /// A desk: calls one way, events the other.
    Desk,
    /// One connection to this port on the server's machine (a dev
    /// server's), its bytes carried both ways: for a device of full
    /// scope.
    Forward(u16),
    /// u0's tools, MCP carried both ways (`u0 --mcp --on`): an
    /// agent on the device reads, and acts as its scope lets it.
    Tools,
}

/// A pairing code made on the server, and the name of the device pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    pub code: String,
    pub device: String,
    /// A pairing link of the device's own, for the server to pair back
    /// with: so that each can control the other.
    pub back: Option<String>,
}

/// The server's answer to [`Hello`], after which each thing it says is
/// sent numbered, as `[number, event]`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Welcome {
    /// What this server's run is known by: another after it restarts.
    pub boot: u64,
    /// The server's name for itself: its machine's.
    pub name: String,
    /// The build it runs, as `update::this_build` says it.
    pub version: String,
    /// u0's home there: where its worktrees and scratch folders are.
    pub home: PathBuf,
    /// All said since the desk's `since` follows; else what it had of
    /// this server is gone, and it begins again.
    pub resumed: bool,
    /// The number of the last thing said before this welcome: what is
    /// sent next follows it.
    pub said: u64,
    /// The server's settings, as its home has them, or why they do not
    /// read.
    pub settings: Result<Box<crate::settings::Settings>, String>,
    /// As a desk pairs with a link to pair back with: the public half of
    /// the key the server paired back with, or why it could not.
    pub back: Option<Result<String, String>>,
    /// What this device may do there.
    pub scope: crate::access::Scope,
    /// What this device called itself when pairing with this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// When this device's access ends (seconds since 1970); none, until revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
}

/// What a session loaded or resumed starts with.
#[derive(Debug, Serialize, Deserialize)]
pub struct Opened {
    pub modes: Option<SessionModeState>,
    pub options: Option<Vec<SessionConfigOption>>,
}

/// What of an agent is here: what it needs before it can be started
/// (none: it can be), the version installed, and the CLI of yours it
/// drives.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Here {
    pub missing: Option<registry::Missing>,
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

    /// A call and an answer for each thing asked of the machine: its git,
    /// its files, its home.
    fn machine() -> (Vec<Call>, Vec<Event>) {
        let dir = PathBuf::from("/p");
        let diff: git::FileDiff = of(json!({
            "path": "a.rs", "added": 1, "gone": 1, "patch": "--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n",
        }));
        let script: Script = of(json!({"name": "dev", "command": "make dev", "worktree": true}));
        let worktree: git::Worktree = of(json!({"repo": "/p", "path": "/w", "branch": "u0/a"}));
        let call: ToolCallId = of(json!("c1"));
        let calls = vec![
            Call::Look {
                key: 1,
                folder: dir.clone(),
            },
            Call::SaveScript {
                dir: dir.clone(),
                at: Some(0),
                script: Some(script.clone()),
                key: 1,
            },
            Call::ReadProject {
                folder: dir.clone(),
            },
            Call::Uncommitted {
                dir: dir.clone(),
                key: 1,
                since: Some("abc".into()),
                no_spacing: true,
            },
            Call::ScopeDiff {
                dir: dir.clone(),
                key: 1,
                scope: git::Scope::Commit("abc".into()),
                base: Some("main".into()),
                no_spacing: false,
            },
            Call::Commits {
                dir: dir.clone(),
                key: 1,
            },
            Call::ReadPr {
                dir: dir.clone(),
                key: 1,
            },
            Call::PrText {
                dir: dir.clone(),
                number: 7,
            },
            Call::ListPrs {
                folder: dir.clone(),
            },
            Call::ListFiles {
                folder: dir.clone(),
            },
            Call::Folders { dir: dir.clone() },
            Call::MakeFolder { dir: dir.clone() },
            Call::Grep {
                folder: dir.clone(),
                words: "fn main".into(),
                how: git::Match {
                    case: true,
                    word: true,
                    regex: false,
                },
            },
            Call::Pictures {
                folder: dir.clone(),
                kind: crate::github::Kind::Issue,
                number: 213,
                urls: vec!["https://github.com/user-attachments/assets/a".into()],
            },
            Call::ReadFile {
                path: dir.join("a.rs"),
                full: false,
            },
            Call::ExtractArchive {
                archive: dir.join("release.zip"),
                dest: dir.join("release"),
            },
            Call::CopyFile {
                from: PathBuf::from("/tmp/spec.md"),
                to: dir.join("spec.md"),
            },
            Call::Forge {
                folder: dir.clone(),
                kind: crate::github::Kind::Pr,
                ask: crate::github::Ask::Act(
                    7,
                    crate::github::Act::Review {
                        verdict: "approve".into(),
                        body: "fine".into(),
                        lines: Vec::new(),
                    },
                ),
                linear: Some("ENG".into()),
            },
            Call::Create {
                dir: dir.clone(),
                name: "new".into(),
                publish: true,
            },
            Call::Clone {
                url: "https://example.com/a.git".into(),
                dir: dir.clone(),
            },
            Call::CancelClone,
            Call::Refresh {
                projects: vec![(dir.clone(), true)],
            },
            Call::MakeWorktree {
                dir: dir.clone(),
                about: "fix it".into(),
                under: dir.join("w"),
                prefix: "u0/".into(),
                from: Some(Begin::Pr(7, "Fix".into())),
                submodules: crate::settings::Submodules::Top,
                key: 1,
            },
            Call::Checkpoint {
                dir: dir.clone(),
                name: "u/0-after".into(),
                key: 1,
                turn: 0,
                after: true,
                before: Some("a1".into()),
                first: Some("a0".into()),
                then: None,
                no_spacing: false,
            },
            Call::Restore {
                dir: dir.clone(),
                target: "a1".into(),
                name: "u/0-undo".into(),
                first: None,
                key: 1,
                turn: 0,
                undo: true,
                no_spacing: false,
            },
            Call::Git {
                dir: dir.clone(),
                action: GitAction::RemoveWorktree(worktree.clone()),
                prefix: "u0/".into(),
                left_out: vec!["b.rs".into()],
                key: 1,
            },
            Call::DropCheckpoints {
                dir: dir.clone(),
                uid: "u".into(),
            },
            Call::Place {
                key: 1,
                calls: vec![(call.clone(), vec![(dir.join("a.rs"), "b\n".into())])],
            },
            Call::Sizes {
                kept: of(json!({
                    "records": [["a", "s1"]], "worktrees": ["/w"], "under": ["/u"], "projects": ["/p"], "uids": ["u"],
                })),
            },
            Call::Tidy {
                what: tidy::Tidy::Records,
                kept: of(json!({
                    "records": [["a", "s1"]], "worktrees": ["/w"], "under": ["/u"], "projects": ["/p"], "uids": ["u"],
                })),
            },
            Call::Forges,
            Call::CheckForge(crate::forge::Host::GitHub),
            Call::Secrets,
            Call::SetSecretRef {
                entry: crate::secrets::SecretRef {
                    name: "STRIPE_KEY".into(),
                    source: crate::secrets::SecretSource::Keychain,
                    reference: None,
                    projects: vec![dir.clone()],
                },
                value: Some("sk_test".into()),
            },
            Call::DropSecretRef("STRIPE_KEY".into()),
            Call::Devices,
            Call::Device {
                device: crate::devices::Device {
                    id: "Pixel_9".into(),
                    name: "Pixel 9".into(),
                    os: "Android 15".into(),
                    android: true,
                    serial: Some("emulator-5554".into()),
                    running: true,
                },
                act: crate::devices::Act::Tap { x: 10, y: 20 },
            },
            Call::Screen {
                device: crate::devices::Device {
                    id: "A".into(),
                    name: "iPhone".into(),
                    os: "iOS 18.2".into(),
                    android: false,
                    serial: None,
                    running: true,
                },
            },
            Call::TermOpen {
                id: 2,
                dir: dir.clone(),
                shell: Some("fish".into()),
                first: Some("make dev".into()),
                size: (24, 80),
            },
            Call::TermInput {
                id: 2,
                bytes: b"ls\r".to_vec(),
            },
            Call::TermResize {
                id: 2,
                size: (30, 100),
            },
            Call::TermClose {
                id: 2,
                force: false,
            },
        ];
        let events = vec![
            Event::Looked {
                key: 1,
                folder: dir.clone(),
                branch: Some("main".into()),
                repo: true,
                repos: Vec::new(),
                summaries: Vec::new(),
                scripts: vec![script.clone()],
            },
            Event::Held {
                folder: dir.clone(),
                held: Held {
                    repo: true,
                    repos: Vec::new(),
                    icon: Some(dir.join("icon.png")),
                },
            },
            Event::Notice(Err(("Couldn't write u0.json".into(), "no".into()))),
            Event::Uncommitted {
                key: 1,
                result: Ok(vec![diff.clone()]),
            },
            Event::Diffed {
                key: 1,
                scope: git::Scope::Branch,
                result: Err("no base".into()),
            },
            Event::Commits {
                key: 1,
                commits: vec![git::Commit {
                    id: "abc".into(),
                    subject: "Fix".into(),
                    author: "Ann".into(),
                    ago: "1h".into(),
                    refs: vec!["HEAD".into()],
                    added: 3,
                    gone: 1,
                    unpushed: true,
                }],
                stashes: vec![git::Stash {
                    index: 0,
                    branch: "main".into(),
                    message: "WIP".into(),
                    files: 1,
                    ago: "5m".into(),
                }],
                branches: vec!["main".into()],
                upstream: (1, 2, true),
            },
            Event::Pr {
                key: 1,
                pr: Some(Box::new(of(json!({
                    "number": 7, "title": "Fix", "url": "https://example.com/7", "state": "open",
                    "draft": false, "conflicts": false, "behind": false, "passed": 3, "failed": 0, "going": 1,
                    "review": "", "head": "fix", "base": "main", "checks": [],
                })))),
            },
            Event::PrText {
                number: 7,
                text: Some("Fix it".into()),
            },
            Event::Prs {
                folder: dir.clone(),
                prs: vec![(7, "Fix".into())],
                issues: Vec::new(),
            },
            Event::Files {
                folder: dir.clone(),
                files: vec!["a.rs".into()],
            },
            Event::Folders {
                dir: dir.clone(),
                names: vec!["src".into()],
            },
            Event::Forge {
                folder: dir.clone(),
                kind: crate::github::Kind::Issue,
                answer: crate::github::Answer::Acted(None, Err("no gh".into())),
            },
            Event::Picture {
                url: "https://github.com/user-attachments/assets/a".into(),
                picture: Ok(Bytes(vec![0x89, b'P', 0xff])),
            },
            Event::FileRead {
                path: dir.join("a.pdf"),
                body: Ok(Box::new(crate::repo::Body::Pdf {
                    pages: vec![vec![0x89, b'P']],
                    of: 3,
                })),
            },
            Event::Grepped {
                words: "fn main".into(),
                found: vec![("a.rs:1".into(), "fn main() {}".into())],
            },
            Event::Cloning {
                what: "Receiving objects".into(),
                share: Some(0.5),
            },
            Event::Cloned {
                dir: dir.clone(),
                result: Ok(()),
            },
            Event::Worktree {
                key: 1,
                result: Ok((worktree, vec![script])),
            },
            Event::Checkpoint {
                key: 1,
                turn: 0,
                after: true,
                result: Ok(Snapshot {
                    commit: "a2".into(),
                    turn: Some(vec![diff.clone()]),
                    total: None,
                }),
                then: Some("go".into()),
            },
            Event::Restored {
                key: 1,
                turn: 0,
                undo: true,
                result: Ok(("a3".into(), Some(vec![diff]))),
            },
            Event::GitStep {
                key: 1,
                text: "Pushing".into(),
            },
            Event::Git {
                key: 1,
                action: GitAction::Commit("Fix".into()),
                result: Ok("Committed abc".into()),
            },
            Event::Placed {
                key: 1,
                starts: vec![(call, vec![Some(3), None])],
            },
            Event::Sizes(tidy::StorageReport {
                buckets: vec![tidy::StorageBucket {
                    label: "Thread database".into(),
                    detail: "1 thread".into(),
                    bytes: 3,
                    cleanable: Some(2),
                }],
                free_bytes: 1024,
            }),
            Event::Tidied("1 record removed".into()),
            Event::Forges(vec![crate::forge::Seen {
                host: crate::forge::Host::GitLab,
                cli: Some("glab 1.50".into()),
                token: true,
                login: Some("octocat".into()),
            }]),
            Event::Secrets {
                providers: vec![crate::secrets::SecretProvider {
                    id: "keychain".into(),
                    name: "macOS Keychain".into(),
                    detail: "Built in".into(),
                    connected: true,
                }],
                refs: Ok(vec![crate::secrets::SecretRef {
                    name: "STRIPE_KEY".into(),
                    source: crate::secrets::SecretSource::OnePassword,
                    reference: Some("op://Dev/Stripe/key".into()),
                    projects: vec![dir.clone()],
                }]),
            },
            Event::Repos(Ok(vec![crate::github::Repo {
                name: "acme/u0".into(),
                description: "app".into(),
                private: true,
                updated: "2026-01-01T00:00:00Z".into(),
                url: "https://github.com/acme/u0".into(),
                ssh_url: "git@github.com:acme/u0.git".into(),
            }])),
            Event::TermOpened {
                id: 2,
                result: Ok("zsh".into()),
            },
            Event::TermOutput {
                id: 2,
                bytes: b"\x1b[31mred\x1b[0m".to_vec(),
            },
            Event::TermEnded {
                id: 2,
                how: "exit 1".into(),
            },
            Event::TermClosed { id: 2, busy: true },
        ];
        (calls, events)
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
        let index: store::Index = of(json!({
            "sessions": [{"agent": "a", "id": "s1", "project": "/p", "folder": "/p",
                          "title": "t", "begun": 5, "updated": 7}],
            "gone": [["a", "s0"]],
            "stashed": ["later"],
        }));
        let meta = index.sessions[0].clone();
        let said = vec![
            store::Event::At(5),
            store::Event::You("go".into()),
            store::Event::Ended {
                reason: StopReason::EndTurn,
                secs: 3,
                tokens: Vec::new(),
            },
        ];
        let calls = vec![
            Call::Fetch,
            Call::SaveSettings(Box::default()),
            Call::Share(crate::share::Share::Revoke("h".into())),
            Call::Install {
                entry: entry.clone(),
            },
            Call::CancelInstall { agent: "a".into() },
            Call::Start {
                entry: entry.clone(),
                command: Some("a --acp".into()),
                env: [("K".to_owned(), None)].into(),
            },
            Call::Secret {
                account: "env/a/K".into(),
                value: Some("v".into()),
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
                places: vec![dir.clone()],
                under: vec![dir.join("w")],
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
            Call::Write(writer::Order {
                agent: "a".into(),
                model: Some("m".into()),
                key: 1,
                ask: writer::Ask::Commit { dir: dir.clone() },
                yours: "short".into(),
                dir: dir.clone(),
                delete: true,
                close: false,
            }),
            Call::Cancel {
                agent: "a".into(),
                session: session.clone(),
            },
            Call::Answer {
                agent: "a".into(),
                request: 7,
                answer: acp::Answer::Permission(of(
                    json!({"outcome": "selected", "optionId": "allow"}),
                )),
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
                session: session.clone(),
                config: "model".into(),
                value: "fast".into(),
            },
            Call::SignIn {
                agent: "a".into(),
                method: "browser".into(),
            },
            Call::SignOut { agent: "a".into() },
            Call::Weigh,
            Call::Telemetry(crate::telemetry::Ask::Show(crate::telemetry::Level::Usage)),
            Call::Signal {
                pid: 7,
                signal: "TERM".into(),
            },
            Call::Troubles,
            Call::Here {
                agents: vec![entry.clone()],
                commands: [("a".into(), "/bin/a --acp".into())].into(),
            },
            Call::Uninstall {
                entry: entry.clone(),
            },
            Call::CliVersion { cli: cli.clone() },
            Call::Watch,
            Call::InstallAxe,
            Call::SetTask(crate::tasks::Task {
                id: 1,
                title: "t".into(),
                prompt: "go".into(),
                agent: "a".into(),
                model: None,
                mode: None,
                project: "/p".into(),
                schedule: crate::tasks::Schedule::Every(5),
                paused: false,
                same: true,
                worktree: false,
                if_running: crate::tasks::Overlap::Skip,
                timeout_mins: Some(30),
                notify: true,
                workflow: None,
                session: None,
                next: Some(1),
                last: None,
                runs: 0,
            }),
            Call::DropTask(1),
            Call::Workflows,
            Call::SetWorkflow(workflows::Workflow {
                name: "w".into(),
                about: "ship".into(),
                agent: Some("a".into()),
                project: Some("/p".into()),
                worktree: true,
                steps: Vec::new(),
            }),
            Call::DropWorkflow("w".into()),
            Call::StartFlow {
                workflow: "w".into(),
                input: "i".into(),
            },
            Call::RerunFlowFrom { run: 1, step: 2 },
            Call::AnswerFlow {
                run: 1,
                choice: "Ship".into(),
            },
            Call::ResumeFlow(1),
            Call::CancelFlow(1),
            Call::NewToolsKey,
            Call::RunTask(1),
            Call::Say {
                tag: 1,
                agent: "a".into(),
                session: None,
                project: "/p".into(),
                prompt: "Go".into(),
            },
            Call::RotateHook(1),
            Call::Keep(vec![meta]),
            Call::Forget {
                agent: "a".into(),
                id: session.clone(),
                keep_out: true,
            },
            Call::Stash(vec!["later".into()]),
            Call::Append {
                agent: "a".into(),
                id: session.clone(),
                events: said.clone(),
            },
            Call::DropRecord {
                agent: "a".into(),
                id: session.clone(),
            },
            Call::Read {
                key: 1,
                agent: "a".into(),
                id: session.clone(),
            },
            Call::CopyRecord {
                agent: "a".into(),
                from: session.clone(),
                to: SessionId::new("s2"),
                yours: 1,
            },
            Call::Search {
                query: "go".into(),
                records: vec![(1, "a".into(), session.clone())],
            },
            Call::Used {
                records: vec![(1, "a".into(), session)],
            },
            Call::Repos,
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
            Event::Appended {
                agent: "a".into(),
                id: SessionId::new("s1"),
                events: said.clone(),
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
            Event::Running {
                agent: "a".into(),
                sessions: vec![SessionId::new("s1")],
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
            Event::Written {
                key: 1,
                ask: writer::Ask::Commit { dir: dir.clone() },
                result: Ok("Add it".into()),
            },
            Event::Ended {
                key: 1,
                result: Ok(StopReason::EndTurn),
                tokens: Vec::new(),
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
            Event::Weighed {
                agents: vec![("a".into(), 42)],
                processes: crate::diagnostics::Processes {
                    procs: vec![crate::diagnostics::Proc {
                        pid: 42,
                        depth: 1,
                        cpu: 1.5,
                        kb: 1024,
                        name: "a".into(),
                    }],
                    load: "1.0 1.0 1.0".into(),
                    cores: 8,
                    memory: 1 << 34,
                    uptime_secs: 60,
                },
            },
            Event::Troubles(vec![crate::diagnostics::LogTrouble {
                level: "error".into(),
                source: "acp".into(),
                message: "x".into(),
                count: 1,
                time: "10:12:04".into(),
            }]),
            Event::Telemetry {
                level: crate::telemetry::Level::Basic,
                batch: "{}".into(),
                install_id: Some("id-1".into()),
            },
            Event::Here(HashMap::from([(
                "a".to_owned(),
                Here {
                    missing: Some(registry::Missing::Runner("npx".into())),
                    installed: Some("1".into()),
                    cli: Some(cli.clone()),
                },
            )])),
            Event::CliVersion {
                cli,
                version: "a 1.0".into(),
            },
            Event::Tasks(Ok(vec![crate::tasks::Task {
                id: 1,
                title: "t".into(),
                prompt: "go".into(),
                agent: "a".into(),
                model: None,
                mode: None,
                project: "/p".into(),
                schedule: crate::tasks::Schedule::Every(5),
                paused: false,
                same: true,
                worktree: false,
                if_running: crate::tasks::Overlap::Skip,
                timeout_mins: None,
                notify: true,
                workflow: None,
                session: None,
                next: Some(1),
                last: None,
                runs: 0,
            }])),
            Event::Workflows {
                flows: vec![Err(("w".into(), "bad".into()))],
                runs: Ok(vec![workflows::Run {
                    id: 1,
                    workflow: "w".into(),
                    input: "i".into(),
                    trigger: "By hand".into(),
                    began: 1,
                    ended: Some(2),
                    state: workflows::State::Paused("x".into()),
                    done: Vec::new(),
                    turn: None,
                    worktree: None,
                }]),
            },
            Event::TaskTurn {
                meta: Box::new(index.sessions[0].clone()),
                record: None,
                ended: Some(Ok(StopReason::EndTurn)),
            },
            Event::Organized(Box::new(index.sessions[0].clone())),
            Event::Answered {
                agent: "a".into(),
                session: SessionId::new("s1"),
                request: 3,
                option: agent_client_protocol::v1::schema::PermissionOption::new(
                    "allow",
                    "Allow",
                    agent_client_protocol::v1::schema::PermissionOptionKind::AllowOnce,
                ),
            },
            Event::Index(Ok(index)),
            Event::Offered {
                link: "u0://pair?host=a#c".into(),
                made: 1,
                grant: crate::access::Grant::default(),
            },
            Event::Turned {
                tag: 1,
                session: Ok(SessionId::new("s1")),
                answer: Some("Done.".into()),
            },
            Event::Settings(Box::default()),
            Event::Shared(Box::new(crate::share::Shared {
                serving: Some(Ok(4317)),
                tunnel: crate::share::Tunnel::Up("a.trycloudflare.com".into()),
                tunnel_since: Some(1),
                named: None,
                awake: true,
                service: false,
                icon: crate::access::MachineIcon::Server,
                devices: Ok(vec![crate::access::Device {
                    label: "laptop".into(),
                    key: "ssh-ed25519 AAAA".into(),
                    scope: crate::access::Scope::Read,
                    until: Some(9),
                    grant_days: Some(7),
                    version: Some("0.4.0".into()),
                    waiting: true,
                    paired: 1,
                    seen: 2,
                }]),
                linked: vec!["ssh-ed25519 AAAA".into()],
                offer: Some((1, crate::access::Grant::default())),
                bind: crate::reach::Bind::Network,
                routes: Ok(vec![(crate::reach::Route::Lan, "192.168.1.5:4318".into())]),
            })),
            Event::Record {
                key: 1,
                events: Some(said),
            },
            Event::Devices {
                found: Vec::new(),
                missing: vec!["Xcode, for iOS simulators".into()],
                tools: vec![crate::devices::ToolStatus {
                    name: "AXe (Homebrew)".into(),
                    installed: Some("1.6.0".into()),
                }],
            },
            Event::Screen {
                id: "A".into(),
                picture: Err("it is not running".into()),
            },
            Event::AxeInstalled(Err("brew failed".into())),
            Event::Found {
                query: "go".into(),
                said: vec![(1, 0, "go on".into())],
            },
            Event::Used(vec![(
                1,
                vec![crate::usage::Used {
                    at: 1,
                    what: crate::usage::Use::Cost {
                        amount: 0.5,
                        currency: "USD".into(),
                    },
                }],
            )]),
        ];
        let (machine_calls, machine_events) = machine();
        let calls: Vec<Call> = calls.into_iter().chain(machine_calls).collect();
        let events: Vec<Event> = events.into_iter().chain(machine_events).collect();
        let called: BTreeSet<String> = calls.iter().map(round_trip).collect();
        assert_eq!(called, variants::<Call>());
        let said: BTreeSet<String> = events.iter().map(round_trip).collect();
        assert_eq!(said, variants::<Event>());
    }

    #[test]
    fn settings_a_tool_changed_are_told_a_reader_without_an_agents_key() {
        let settings: crate::settings::Settings = serde_json::from_value(json!({
            "enabled": [], "folders": [],
            "env": {"a": {"API_KEY": "sk-1"}}, "commands": {"a": "a --key sk-2"},
        }))
        .unwrap();
        let Some(Event::Settings(told)) = Event::Settings(Box::new(settings)).for_reader() else {
            panic!("told otherwise")
        };
        let told = serde_json::to_string(&told).unwrap();
        assert!(!told.contains("sk-1") && !told.contains("sk-2"), "{told}");
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
                asker: acp::Asker::Tool("merge_pr".into()),
            },
            acp::Event::Elicit {
                id: 8,
                request: of(json!({
                    "mode": "url",
                    "sessionId": "s1",
                    "elicitationId": "e1",
                    "url": "https://example.com/sign-in",
                    "message": "Sign in",
                })),
            },
            acp::Event::Exited {
                pid: Some(42),
                why: "killed".into(),
            },
        ];
        let names: BTreeSet<String> = said.iter().map(round_trip).collect();
        assert_eq!(names, variants::<acp::Event>());
    }
}
