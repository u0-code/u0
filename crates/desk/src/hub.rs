//! The work done off the window's thread, and what comes back of it.
//!
//! The agents are the server's (`ghost_core::server`): the desk asks it by
//! an agent's id, and what it says comes back as [`Msg::Event`]. Each agent
//! switched on has a [`Slot`] here: what the registry says of it, whether it
//! can be started, and once started what it said it can do. The rest of the
//! work (git, files, windows, terminals, searching, tidying) runs on the ACP
//! runtime too, and all of it comes back to the window as a [`Msg`] over one
//! channel, which the desk reads in order.
//!
//! The slots are in `hub/agent.rs`, what is asked of git in `hub/git.rs`;
//! here, the messages and the rest of the work.

use std::path::PathBuf;

use agent_client_protocol::v1::schema::ToolCallId;
use tokio::sync::mpsc;

mod agent;
mod git;

pub use agent::*;
pub use git::*;

use crate::{acp, proto::Event, registry, scripts::Script};

/// What comes back to the window.
pub enum Msg {
    /// What the server says.
    Event(Event),
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
    /// How much each part of Ghost's home takes, and what a tidying did.
    Sizes(Vec<(&'static str, u64)>),
    Tidied(String),
    /// The sessions whose records hold `query`.
    Found {
        query: String,
        keys: Vec<u64>,
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
/// it Ghost's keys back), on the writer, so that two binds made quickly
/// are kept in the order they were made.
pub fn set_keys(home: PathBuf, name: &'static str, key: Option<String>, tx: Sender) {
    crate::store::queue(crate::store::Job::Then(Box::new(move || {
        let _ = tx.send(Msg::Keys(crate::keys::set_yours(&home, name, key)));
    })));
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
    crate::store::queue(crate::store::Job::Then(Box::new(move || {
        let events = crate::store::read(&path);
        let _ = tx.send(Msg::Record { key, events });
    })));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn a_diffs_place_is_found_in_its_file_off_the_window_once_the_call_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
        let mut s = Session::new(1, "x", dir.path().to_path_buf());
        let json = format!(
            r#"{{"sessionUpdate":"tool_call","toolCallId":"e","title":"Edit","kind":"edit","status":"completed","content":[{{"type":"diff","path":"{}","oldText":"3\n","newText":"three\nfour\n"}}]}}"#,
            file.display()
        );
        s.apply(serde_json::from_str(&json).unwrap());
        assert!(s.tools().next().unwrap().starts.is_empty());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        place(1, std::mem::take(&mut s.placing), tx);
        let Some(Msg::Placed { starts, .. }) = rx.blocking_recv() else {
            panic!("the diffs were not placed");
        };
        for (id, starts) in starts {
            s.placed(&id, starts);
        }
        assert_eq!(s.tools().next().unwrap().starts, vec![Some(3)]);
    }
}
