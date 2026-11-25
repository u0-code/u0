//! The work done off the window's thread, and what comes back of it.
//!
//! What touches the machine is the server's (`ghost_core::server`): the
//! agents, the store, git and the files of a project. The desk asks it with
//! a `proto::Call` through the [`Transport`], and what it says comes back
//! as [`Msg::Event`]. Each agent switched on has a [`Slot`] here
//! (`hub/agent.rs`): what the registry says of it, whether it can be
//! started, and once started what it said it can do. What is the window's
//! own (pictures, your editor, notifications, key bindings)
//! runs on the ACP runtime too, and all of it comes back to the window as
//! a [`Msg`] over one channel, which the desk reads in order.

use std::path::PathBuf;

use tokio::sync::mpsc;

mod agent;

pub use agent::*;

use crate::{
    acp,
    proto::{Call, Event},
    registry,
    server::Server,
};

/// What comes back to the window.
pub enum Msg {
    /// What the server says.
    Event(Event),
    /// Your key bindings, as keybindings.json says now, or why it was not
    /// written.
    Keys(Result<Vec<crate::keys::Binding>, String>),
    /// The programs the desk offers that are on the PATH.
    OnPath(Vec<&'static str>),
    /// Something done off the window's thread, to be said: what was done,
    /// or what was not and why.
    Notice(Result<String, (String, String)>),
    /// A picture to go with what you write: a window you chose, captured,
    /// or a file dropped in, read; nothing if you let it be.
    Picture(Option<crate::desk::mention::Picture>),
}

pub type Sender = mpsc::UnboundedSender<Msg>;

/// The way to the server: each [`Call`] goes to it in order, and what it
/// says comes back as [`Msg::Event`]. The server runs in this process: a
/// call reaches it as it is, not written out, and is handed on at once
/// (what it waits for, it waits for on the ACP runtime).
pub struct Transport(Server);

impl Transport {
    /// A server for Ghost's home `home` in this process, saying what it
    /// has to say to `tx`.
    pub fn local(home: PathBuf, tx: Sender) -> Self {
        Self(Server::new(home, move |event| {
            let _ = tx.send(Msg::Event(event));
        }))
    }

    pub fn send(&self, call: Call) {
        self.0.handle(call);
    }

    /// Wait until what the server in this process was asked to write is
    /// written: as Ghost quits.
    pub fn settle() {
        Server::settle();
    }
}

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

/// Bind the action `name` to `key` in your keybindings.json (none: give
/// it Ghost's keys back), on the writer, so that two binds made quickly
/// are kept in the order they were made.
pub fn set_keys(home: PathBuf, name: &'static str, key: Option<String>, tx: Sender) {
    crate::store::queue(crate::store::Job::Then(Box::new(move || {
        let _ = tx.send(Msg::Keys(crate::keys::set_yours(&home, name, key)));
    })));
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
