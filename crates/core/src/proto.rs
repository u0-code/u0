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
    store, tidy, writer,
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
    /// The sessions `agent` keeps in each of `places` (where it works, and
    /// the project that is) and in Ghost's worktrees of them, those in
    /// `under`.
    List {
        agent: String,
        places: Vec<(PathBuf, PathBuf)>,
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
    /// Ghost's processes, and which are the agents running.
    Weigh,
    /// Send the process `pid`, one of Ghost's, the signal `signal` (TERM,
    /// KILL); answered as a `Notice`.
    Signal {
        pid: u32,
        signal: String,
    },
    /// The warnings and errors in Ghost's log, newest first.
    Troubles,
    /// What of each of `agents` is here.
    Here {
        agents: Vec<Entry>,
    },
    /// What `cli` says its version is.
    CliVersion {
        cli: PathBuf,
    },
    /// The index of the sessions kept: answered with [`Event::Index`].
    Watch,
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
    /// What each of the sessions `records` (by key, agent and id) cost,
    /// as their records say.
    Spent {
        records: Vec<(u64, String, SessionId)>,
    },
    /// What git says of `folder`, where the session `key` works.
    Look {
        key: u64,
        folder: PathBuf,
    },
    /// Put `script` at `at` in the `ghost.json` of the repository at `dir`
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
    /// What is not committed in `dir`.
    Uncommitted {
        dir: PathBuf,
        key: u64,
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
    /// Ask the pull requests or issues of the project in `folder`, from
    /// the host of its remote; its issues from the Linear team keyed
    /// `linear`, where it is bound to one.
    Forge {
        folder: PathBuf,
        kind: crate::github::Kind,
        ask: crate::github::Ask,
        linear: Option<String>,
    },
    /// What the file at `path` holds, for the column's viewer.
    ReadFile {
        path: PathBuf,
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
    /// How much each part of Ghost's home takes, its worktrees those in
    /// `worktrees`.
    Sizes {
        worktrees: Vec<PathBuf>,
    },
    /// Tidy `what` around what is `kept`, then measure again.
    Tidy {
        what: tidy::Tidy,
        kept: tidy::Kept,
    },
    /// The hosts your SSH config names.
    Hosts,
    /// What is known of each forge here (`forge::seen`).
    Forges,
    /// Open the terminal `id` in `dir` (on `host`, over SSH), running
    /// `shell` (your login shell when none), `size` rows by columns, `first`
    /// typed into it; one open already is said again.
    TermOpen {
        id: u64,
        dir: PathBuf,
        host: Option<String>,
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
    /// Ghost's processes (`diagnostics`), and the agents running among
    /// them: each one's id and process id.
    Weighed {
        agents: Vec<(String, u32)>,
        processes: crate::diagnostics::Processes,
    },
    /// The warnings and errors in Ghost's log, newest first.
    Troubles(Vec<String>),
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
    /// The record of the session `key`, read back: none where there is none.
    Record {
        key: u64,
        events: Option<Vec<store::Event>>,
    },
    /// The sessions whose records hold `query`.
    Found {
        query: String,
        keys: Vec<u64>,
    },
    /// What sessions cost, by key, as their agents said (`store::spent`).
    Spent(Vec<(u64, Vec<store::Spend>)>),
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
    /// What the file at `path` holds.
    FileRead {
        path: PathBuf,
        body: Result<crate::repo::Body, String>,
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
    /// How much each part of Ghost's home takes, and what a tidying did.
    Sizes(Vec<(String, u64)>),
    Tidied(String),
    /// The hosts your SSH config names.
    Hosts(Vec<String>),
    /// What is known of each forge here.
    Forges(Vec<crate::forge::Seen>),
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

    /// A call and an answer for each thing asked of the machine: its git,
    /// its files, its home.
    fn machine() -> (Vec<Call>, Vec<Event>) {
        let dir = PathBuf::from("/p");
        let diff: git::FileDiff = of(json!({
            "path": "a.rs", "from": null, "status": "Modified", "added": 1, "gone": 1,
            "binary": false, "patch": "--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n",
        }));
        let script: Script = of(json!({"name": "dev", "command": "make dev", "worktree": true}));
        let worktree: git::Worktree = of(json!({"repo": "/p", "path": "/w", "branch": "ghost/a"}));
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
            Call::Grep {
                folder: dir.clone(),
                words: "fn main".into(),
                how: git::Match {
                    case: true,
                    word: true,
                    regex: false,
                },
            },
            Call::ReadFile {
                path: dir.join("a.rs"),
            },
            Call::Forge {
                folder: dir.clone(),
                kind: crate::github::Kind::Pr,
                ask: crate::github::Ask::Act(
                    7,
                    crate::github::Act::Review {
                        verdict: "approve".into(),
                        body: "fine".into(),
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
                prefix: "ghost/".into(),
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
                prefix: "ghost/".into(),
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
                worktrees: vec![dir.join("w")],
            },
            Call::Tidy {
                what: tidy::Tidy::Records,
                kept: of(json!({
                    "records": [["a", "s1"]], "worktrees": ["/w"], "under": ["/u"], "projects": ["/p"], "uids": ["u"],
                })),
            },
            Call::Hosts,
            Call::Forges,
            Call::TermOpen {
                id: 2,
                dir: dir.clone(),
                host: Some("box".into()),
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
                scripts: vec![script.clone()],
            },
            Event::Held {
                folder: dir.clone(),
                held: Held {
                    repo: true,
                    icon: Some(dir.join("icon.png")),
                },
            },
            Event::Notice(Err(("Couldn't write ghost.json".into(), "no".into()))),
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
                commits: vec![("abc".into(), "Fix".into(), "1 hour ago".into())],
                branches: vec!["main".into()],
                upstream: (2, true),
            },
            Event::Pr {
                key: 1,
                pr: Some(of(json!({
                    "number": 7, "title": "Fix", "url": "https://example.com/7", "state": "open",
                    "draft": false, "conflicts": false, "passed": 3, "failed": 0, "going": 1,
                    "review": "",
                }))),
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
            Event::FileRead {
                path: dir.join("a.pdf"),
                body: Ok(crate::repo::Body::Pdf {
                    pages: vec![vec![0x89, b'P']],
                    of: 3,
                }),
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
            Event::Sizes(vec![("Session records".into(), 3)]),
            Event::Tidied("1 record removed".into()),
            Event::Hosts(vec!["box".into()]),
            Event::Forges(vec![crate::forge::Seen {
                host: crate::forge::Host::GitLab,
                cli: Some("glab 1.50".into()),
                token: true,
            }]),
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
            },
        ];
        let calls = vec![
            Call::Fetch,
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
                places: vec![(dir.clone(), dir.clone())],
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
            Call::Signal {
                pid: 7,
                signal: "TERM".into(),
            },
            Call::Troubles,
            Call::Here {
                agents: vec![entry.clone()],
            },
            Call::CliVersion { cli: cli.clone() },
            Call::Watch,
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
            Call::Spent {
                records: vec![(1, "a".into(), session)],
            },
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
                },
            },
            Event::Troubles(vec!["t ERROR x".into()]),
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
            Event::Index(Ok(index)),
            Event::Record {
                key: 1,
                events: Some(said),
            },
            Event::Found {
                query: "go".into(),
                keys: vec![1],
            },
            Event::Spent(vec![(
                1,
                vec![store::Spend {
                    at: 1,
                    amount: 0.5,
                    currency: "USD".into(),
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
