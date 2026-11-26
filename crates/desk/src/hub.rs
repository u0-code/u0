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
    acp, link,
    proto::{Call, Event},
    server::Server,
    settings::Settings,
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
    Attached(Option<crate::desk::mention::Attached>),
    /// What speech recognition heard, as you dictate.
    Heard(crate::voice::Heard),
    /// Where an update of Ghost's own stands.
    Update(crate::desk::updates::Update),
    /// How the link to the server elsewhere stands.
    Link(link::State),
    /// A server paired with, and the private half of the key this Mac
    /// paired with, as it is kept; or why not.
    Paired(Result<(crate::servers::Server, String), String>),
}

pub type Sender = mpsc::UnboundedSender<Msg>;

/// The way to the server: each [`Call`] goes to it in order, and what it
/// says comes back as [`Msg::Event`].
pub enum Transport {
    /// The server in this process: a call reaches it as it is, not
    /// written out, and is handed on at once (what it waits for, it waits
    /// for on the ACP runtime).
    Local(std::sync::Arc<Server>),
    /// A server on another machine, by its name, over a link (`link`),
    /// and Ghost's home there.
    Remote(link::Link, String, std::path::PathBuf),
}

impl Transport {
    /// The server the window opens on (`servers.json`), saying what it
    /// has to say to `tx`, with its settings as its home has them (or why
    /// they do not read). One that cannot be reached is said to be so, and
    /// the window opens on this Mac's own.
    pub fn open(home: &std::path::Path, tx: &Sender) -> (Self, (Settings, Option<String>)) {
        let servers = crate::servers::Servers::load(home);
        if let Ok(Some(server)) = servers.as_ref().map(crate::servers::Servers::opened) {
            let name = server.name.clone();
            match Self::reach(server, tx) {
                Ok((link, welcome)) => {
                    let settings = match welcome.settings {
                        Ok(s) => (*s, None),
                        Err(why) => (Settings::default(), Some(why)),
                    };
                    return (Self::Remote(link, name, welcome.home), settings);
                }
                Err(why) => {
                    let _ = tx.send(Msg::Notice(Err((
                        format!("Couldn't reach {name}: Ghost opened on this Mac"),
                        why,
                    ))));
                }
            }
        }
        if let Err(why) = servers {
            let _ = tx.send(Msg::Notice(Err(("Couldn't read your servers".into(), why))));
        }
        let local = Self::Local(Server::new(home.to_path_buf(), {
            let tx = tx.clone();
            move |event| {
                let _ = tx.send(Msg::Event(event));
            }
        }));
        (local, Settings::load(home))
    }

    /// Link to `server`, waiting for its welcome.
    fn reach(
        server: &crate::servers::Server,
        tx: &Sender,
    ) -> Result<(link::Link, crate::proto::Welcome), String> {
        let (url, key) = server.reach()?;
        let (said, stood) = (tx.clone(), tx.clone());
        acp::runtime().block_on(link::connect(
            url,
            key,
            None,
            move |event| {
                let _ = said.send(Msg::Event(event));
            },
            move |state| {
                let _ = stood.send(Msg::Link(state));
            },
        ))
    }

    pub fn send(&self, call: Call) {
        match self {
            Self::Local(server) => server.handle(call),
            Self::Remote(link, ..) => link.send(call),
        }
    }

    /// Ghost's home where the server runs: `home`, this Mac's, for the
    /// server in this process.
    pub fn home_there<'a>(&'a self, home: &'a std::path::Path) -> &'a std::path::Path {
        match self {
            Self::Local(_) => home,
            Self::Remote(.., there) => there,
        }
    }

    /// The name of the server elsewhere the window is on; none, this Mac.
    pub fn elsewhere(&self) -> Option<&str> {
        match self {
            Self::Local(_) => None,
            Self::Remote(_, name, _) => Some(name),
        }
    }

    /// Wait until what the server was asked to write is written, or what
    /// was asked of one elsewhere has gone to it: as Ghost quits.
    pub fn settle(&self) {
        match self {
            Self::Local(server) => {
                server.unshare();
                Server::settle();
            }
            Self::Remote(link, ..) => link.settle(),
        }
    }
}

/// Pair with the server at `host` by the one-time `code`: link to it as
/// this Mac, with `key`, and let go once it has let the key in.
pub fn pair(host: String, code: String, key: crate::key::Key, tx: Sender) {
    acp::runtime().spawn(async move {
        let paired = async {
            let pair = crate::proto::Pair {
                code,
                device: link::host_name(),
            };
            let (public, kept) = (key.public(), key.kept().to_owned());
            let url = link::url_of(&host);
            let (_, welcome) = link::connect(url, key, Some(pair), |_| {}, |_| {}).await?;
            let server = crate::servers::Server {
                name: welcome.name,
                host,
                key: public,
            };
            Ok((server, kept))
        };
        let _ = tx.send(Msg::Paired(paired.await));
    });
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
        Msg::Attached(bytes.map(|bytes| crate::desk::mention::Attached::Image {
            mime: "image/png".into(),
            bytes,
        }))
    });
}

/// The largest image sent as it is; a larger one is made smaller first.
const PICTURE: u64 = 10 * 1024 * 1024;

/// Whether the file at `path` is a picture an agent is shown, as itself or
/// made a JPEG.
pub fn is_picture(path: &std::path::Path) -> bool {
    picture_kind(path).is_some()
}

/// The kind of picture `path` holds, by its name; none, not one.
fn picture_kind(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        _ => return None,
    })
}

/// Read the picture at `path`, for what you write: a photo in HEIC, which
/// agents do not read, or one over `PICTURE`, made a JPEG (by macOS's
/// `sips`), no wider or taller than 2560 points.
pub fn read_picture(path: PathBuf, tx: Sender) {
    off(tx, move || {
        let mime = picture_kind(&path).unwrap_or("image/png");
        let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > PICTURE);
        let read = if mime == "image/heic" || big {
            let out =
                std::env::temp_dir().join(format!("ghost-picture-{}.jpg", std::process::id()));
            let made = std::process::Command::new("sips")
                .args(["-s", "format", "jpeg", "-Z", "2560"])
                .arg(&path)
                .arg("--out")
                .arg(&out)
                .output()
                .is_ok_and(|o| o.status.success());
            let bytes = made.then(|| std::fs::read(&out).ok()).flatten();
            let _ = std::fs::remove_file(&out);
            bytes.map(|b| ("image/jpeg", b))
        } else {
            std::fs::read(&path).ok().map(|b| (mime, b))
        };
        Msg::Attached(
            read.map(|(mime, bytes)| crate::desk::mention::Attached::Image {
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
        let editor = (chosen.as_deref().and_then(|c| which::which(c).ok()))
            .or_else(|| (crate::desk::EDITORS.into_iter()).find_map(|(c, _)| which::which(c).ok()));
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
pub fn set_keys(home: PathBuf, name: &'static str, keys: crate::keys::Keys, tx: Sender) {
    crate::store::queue(crate::store::Job::Then(Box::new(move || {
        let _ = tx.send(Msg::Keys(crate::keys::set_yours(&home, name, keys)));
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

/// The `ghost` command, as t3code's `t3`: each folder named (none, the
/// one you are in) opened in Ghost as a project, through macOS, which
/// hands it to the Ghost running or starts one.
const COMMAND: &str = "#!/bin/sh\n# Open each folder named (none: this one) in Ghost, as a project.\n[ $# -eq 0 ] && set -- .\nfor f in \"$@\"; do open -b local.ghost.desk \"$f\" || exit; done\n";

/// Put the `ghost` command in `~/.local/bin`, and say where it is, or
/// why it is not.
pub fn install_command(tx: Sender) {
    off(tx, || {
        let Some(home) = crate::desk::Desk::home_path() else {
            return Msg::Notice(Err((
                "Couldn't add the ghost command".into(),
                "HOME is not set".into(),
            )));
        };
        let path = home.join(".local/bin/ghost");
        let shown = crate::desk::Desk::shown(&path);
        let put = crate::whole::write(&path, COMMAND.as_bytes()).and_then(|()| {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        });
        Msg::Notice(match put {
            Ok(()) if which::which("ghost").is_ok_and(|p| p == path) => {
                Ok(format!("ghost <folder> opens it in Ghost: {shown}"))
            }
            Ok(()) => Ok(format!("It is in {shown}; add ~/.local/bin to your PATH")),
            Err(e) => Err(("Couldn't add the ghost command".into(), e.to_string())),
        })
    });
}

/// Look for a build of Ghost newer than this one on `channel`.
pub fn check_update(channel: crate::update::Channel, tx: Sender) {
    off(tx, move || {
        Msg::Update(crate::desk::updates::Update::Checked(crate::update::check(
            channel,
        )))
    });
}

/// Download the build `found` and put it in place of `app`, saying how it
/// goes.
pub fn install_update(found: crate::update::Found, app: PathBuf, home: PathBuf, tx: Sender) {
    let saying = tx.clone();
    off(tx, move || {
        let say = |what: &str, _: Option<f32>| {
            let _ = saying.send(Msg::Update(crate::desk::updates::Update::Doing(
                what.to_owned(),
            )));
        };
        let stop = std::sync::atomic::AtomicBool::new(false);
        Msg::Update(crate::desk::updates::Update::Installed(
            crate::update::install(&found, &app, &home, &say, &stop),
        ))
    });
}

/// Which of the programs the desk offers (gh, the editors it knows) are
/// on the PATH.
pub fn on_path(tx: Sender) {
    off(tx, || {
        let offered = std::iter::once("gh").chain(crate::desk::EDITORS.map(|(c, _)| c));
        Msg::OnPath(offered.filter(|c| which::which(c).is_ok()).collect())
    });
}
