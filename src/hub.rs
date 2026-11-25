//! The agents the desk runs, and the work done with them off the window's
//! thread.
//!
//! Each agent switched on has a [`Slot`]: what the registry says of it,
//! whether it can be started, and once started its connection and what it
//! said it can do. Starting an agent, opening a session, a turn, an install:
//! each runs on the ACP runtime and comes back to the window as a [`Msg`]
//! over one channel, which the desk reads in order. What an agent says on
//! its own comes back the same way.
//!
//! What is asked of the agents is in `hub/agent.rs`, what of git in
//! `hub/git.rs`; here, the messages and the rest of the work off the
//! window's thread (files, windows, terminals, searching, tidying).

use std::{path::PathBuf, sync::Arc};

use agent_client_protocol::{
    JsonRpcRequest,
    v1::schema::{
        ForkSessionResponse, InitializeResponse, NewSessionResponse, SessionConfigOption,
        SessionInfo, StopReason, ToolCallId,
    },
};
use tokio::sync::mpsc;

mod agent;
mod git;

pub use agent::*;
pub use git::*;

use crate::{
    acp::{self, Agent, Error},
    desk::runs::Script,
    registry,
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
    /// Your key bindings, as keybindings.json says now, or why it was not
    /// written.
    Keys(Result<Vec<crate::keys::Binding>, String>),
    /// What of each agent the registry lists is here, by its id.
    Here(std::collections::HashMap<String, Here>),
    /// The programs the desk offers that are on the PATH.
    OnPath(Vec<&'static str>),
    /// The hosts your SSH config names.
    Hosts(Vec<String>),
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
        result: Result<Vec<crate::git::FileDiff>, String>,
    },
    /// A terminal was opened, or why not; its screen changed; its shell
    /// ended.
    TerminalOpened {
        id: u64,
        result: Result<Box<crate::terminal::Terminal>, String>,
    },
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
    /// A picture to go with what you write: a window you chose, captured,
    /// or a file dropped in, read; nothing if you let it be.
    Picture(Option<crate::desk::mention::Picture>),
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
        scope: crate::git::Scope,
        result: Result<Vec<crate::git::FileDiff>, String>,
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
        pr: Option<crate::git::Pr>,
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
        result: Result<(crate::git::Worktree, Vec<Script>), String>,
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
        result: Result<(String, Option<Vec<crate::git::FileDiff>>), String>,
    },
    /// A commit, a push, a pull request or a worktree removed: what it said.
    Git {
        key: u64,
        action: GitAction,
        result: Result<String, String>,
    },
}

pub type Sender = mpsc::UnboundedSender<Msg>;

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
        Msg::Picture(bytes.map(|bytes| crate::desk::mention::Picture {
            mime: "image/png".into(),
            bytes,
        }))
    });
}

/// Read the picture at `path`, of the type `mime`, for what you write.
pub fn read_picture(path: PathBuf, mime: &'static str, tx: Sender) {
    off(tx, move || {
        Msg::Picture(
            std::fs::read(&path)
                .ok()
                .map(|bytes| crate::desk::mention::Picture {
                    mime: mime.into(),
                    bytes,
                }),
        )
    });
}

/// Open `path` in your editor: the one you chose (`chosen`, its command),
/// else the first of `desk::EDITORS` on the PATH, else the app macOS opens
/// text in. A file not there is first written as `start`, or with none is
/// said to be missing.
pub fn edit(path: PathBuf, chosen: Option<String>, start: Option<&'static [u8]>, tx: Sender) {
    acp::runtime().spawn_blocking(move || {
        let shown = crate::desk::Desk::shown(&path);
        let there = path.exists() || start.is_some_and(|s| crate::whole::write(&path, s).is_ok());
        if !there {
            let why = path.display().to_string();
            let _ = tx.send(Msg::Notice(Err((format!("There is no {shown} here"), why))));
            return;
        }
        let editor = (chosen.as_deref().and_then(registry::which))
            .or_else(|| (crate::desk::EDITORS.into_iter()).find_map(|(c, _)| registry::which(c)));
        let opened = match editor {
            Some(cmd) => std::process::Command::new(cmd).arg(&path).status(),
            None => std::process::Command::new("open")
                .arg("-t")
                .arg(&path)
                .status(),
        };
        if let Err(e) = opened {
            let _ = tx.send(Msg::Notice(Err((
                format!("Couldn't open {shown}"),
                e.to_string(),
            ))));
        }
    });
}

/// Show a notification from macOS (`macos::notify`).
pub fn notify(title: &str, body: &str, sound: bool) {
    let (title, body) = (title.to_owned(), body.to_owned());
    acp::runtime().spawn_blocking(move || crate::macos::notify(&title, &body, sound));
}

/// Bind the action `name` to `key` in your keybindings.json (none: give
/// it Ghost's keys back).
pub fn set_keys(home: PathBuf, name: &'static str, key: Option<String>, tx: Sender) {
    off(tx, move || {
        Msg::Keys(crate::keys::set_yours(&home, name, key))
    });
}

/// Open the terminal `id` beside the session `session` in `dir` (on
/// `host`, if it is on another machine), `first` typed into it, and read
/// what it prints on the same thread, after it is the window's.
pub fn open_terminal(
    id: u64,
    session: u64,
    dir: PathBuf,
    host: Option<String>,
    first: Option<String>,
    size: (u16, u16),
    tx: Sender,
) {
    std::thread::spawn(move || {
        let opened = crate::terminal::Terminal::open(
            id,
            session,
            &dir,
            host.as_deref(),
            first.as_deref(),
            size,
        );
        match opened {
            Ok((t, from)) => {
                let read = t.reader(from);
                let result = Ok(Box::new(t));
                let _ = tx.send(Msg::TerminalOpened { id, result });
                read(&tx);
            }
            Err(why) => {
                let _ = tx.send(Msg::TerminalOpened {
                    id,
                    result: Err(why),
                });
            }
        }
    });
}

/// Read the hosts your SSH config names, for a project on another machine.
pub fn hosts(tx: Sender) {
    off(tx, || Msg::Hosts(crate::remote::hosts()));
}

/// Write `text` to `path` whole, and say that it is there, or why not.
pub fn write(path: PathBuf, text: String, tx: Sender) {
    off(tx, move || {
        let shown = crate::desk::Desk::shown(&path);
        Msg::Notice(match crate::whole::write(&path, text.as_bytes()) {
            Ok(()) => Ok(format!("It is in {shown}")),
            Err(e) => Err((format!("Couldn't write {shown}"), e.to_string())),
        })
    });
}

/// Which of the programs the desk offers (gh, the editors it knows) are
/// on the PATH.
pub fn on_path(tx: Sender) {
    off(tx, || {
        let offered = std::iter::once("gh").chain(crate::desk::EDITORS.map(|(c, _)| c));
        Msg::OnPath(offered.filter(|c| registry::which(c).is_some()).collect())
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
