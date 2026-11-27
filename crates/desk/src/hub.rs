//! The work done off the window's thread, and what comes back of it.
//!
//! What touches a machine is its server's (`u0_core::server`): the
//! agents, the store, git and the files of a project. The window holds
//! several at once, each a [`Machine`]: this Mac's own, and each one
//! paired with while this Mac controls others. The desk asks one with a
//! `proto::Call` through its [`Transport`], and what it says comes back
//! as [`Msg::Event`], with the [`Host`] it came from. Each agent switched
//! on has a [`Slot`] here (`hub/agent.rs`): what the registry says of it,
//! whether it can be started, and once started what it said it can do.
//! What is the window's own (pictures, your editor, notifications, key
//! bindings) runs on the ACP runtime too, and all of it comes back to the
//! window as a [`Msg`] over one channel, which the desk reads in order.

use std::{collections::HashMap, path::PathBuf};

use tokio::sync::mpsc;

mod agent;

pub use agent::*;

pub use crate::servers::Host;
use crate::{
    access::Access,
    acp, link,
    proto::{Call, Event, Here},
    server::Server,
    servers::Servers,
    settings::Settings,
};

/// What comes back to the window.
pub enum Msg {
    /// What a server says.
    Event(Host, Event),
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
    /// Where an update of u0's own stands.
    Update(crate::desk::updates::Update),
    /// How the link to a server stands.
    Link(Host, link::State),
    /// What came of pairing with a server, or why it did not pair.
    Paired(Result<link::Paired, String>),
    /// A port of the server `host` brought here, to open `address` on;
    /// or why not.
    Forwarded {
        host: Host,
        address: String,
        result: Result<link::Forward, String>,
    },
}

pub type Sender = mpsc::UnboundedSender<Msg>;

/// The way to a server: each [`Call`] goes to it in order, and what it
/// says comes back as [`Msg::Event`].
pub enum Transport {
    /// The server in this process: a call reaches it as it is, not
    /// written out, and is handed on at once (what it waits for, it waits
    /// for on the ACP runtime).
    Local(std::sync::Arc<Server>),
    /// A server reached over a link (`link`): on another machine, or this
    /// Mac's service.
    Remote(link::Link),
}

/// How a server the window holds stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// Being reached, the first time.
    Reaching,
    On,
    /// Out of reach, why: it is reached again on its own.
    Lost(String),
    /// It will not let this Mac in, or this Mac has no key for it, why.
    Refused(String),
}

/// A server the window holds: this Mac's own, or one paired with.
pub struct Machine {
    pub host: Host,
    /// As `servers.json` keeps it, for one paired with.
    pub paired: Option<crate::servers::Server>,
    /// What it is called: "This Mac", or the name it gave itself.
    pub name: String,
    pub transport: Transport,
    pub up: Up,
    /// u0's home there: where its worktrees and scratch folders are.
    pub home: PathBuf,
    /// Its settings, as its home has them, and why they do not read: this
    /// Mac's are the desk's own (`Desk::settings`).
    pub settings: Settings,
    pub refused: Option<String>,
    /// Why the sessions it keeps could not be read, while that is so:
    /// nothing is saved over them.
    pub store_refused: Option<String>,
    /// What of each agent the registry lists is there, by its id.
    pub here: HashMap<String, Here>,
    /// Sessions deleted there whose agent cannot delete them (see
    /// `store::Index`), by agent and id.
    pub gone: Vec<(String, String)>,
    /// The folders each agent there has sessions in that are not projects.
    pub elsewhere: std::collections::BTreeMap<String, Vec<(PathBuf, usize)>>,
    /// What each CLI of yours an agent there runs says its version is.
    pub cli_versions: HashMap<PathBuf, String>,
    /// What this Mac may do there, as its welcome said: all, on its own.
    pub scope: crate::access::Scope,
    /// Its ports brought here (a dev server's), each while it is held.
    pub forwards: Vec<link::Forward>,
    /// The build of u0 it runs, as its welcome said.
    pub version: Option<String>,
}

impl Machine {
    fn new(host: Host, name: String, transport: Transport, up: Up) -> Self {
        Self {
            host,
            paired: None,
            name,
            transport,
            up,
            home: PathBuf::new(),
            settings: Settings::default(),
            refused: None,
            store_refused: None,
            here: HashMap::new(),
            gone: Vec::new(),
            elsewhere: std::collections::BTreeMap::new(),
            cli_versions: HashMap::new(),
            version: None,
            scope: crate::access::Scope::Full,
            forwards: Vec::new(),
        }
    }

    /// The servers the window holds as it opens, each saying what it has
    /// to say to `tx`: this Mac's own first (in this process, or its
    /// service where it runs one), then each paired with while this Mac
    /// controls others.
    pub fn open(home: &std::path::Path, tx: &Sender) -> Vec<Self> {
        let servers = Servers::load(home).unwrap_or_else(|why| {
            let _ = tx.send(Msg::Notice(Err((
                crate::tr("Couldn't read your machines").into(),
                why,
            ))));
            Servers::default()
        });
        let port = Access::new(home).kept().map_or(0, |k| k.port);
        let mut here = match servers.service(port) {
            Some(service) => Self::linked(Host::HERE, service, tx),
            None => Self::in_process(Host::HERE, home, tx),
        };
        here.name = crate::tr("This Mac").into();
        here.home = home.to_path_buf();
        let others = (servers.held(port).zip(1..)).map(|(s, n)| Self::linked(Host(n), s, tx));
        std::iter::once(here).chain(others).collect()
    }

    /// Hold as `host` a server in this process, for the u0 home `home`.
    fn in_process(host: Host, home: &std::path::Path, tx: &Sender) -> Self {
        let tx = tx.clone();
        let server = Server::new(home.to_path_buf(), move |event| {
            let _ = tx.send(Msg::Event(host, event));
        });
        Self::new(host, String::new(), Transport::Local(server), Up::On)
    }

    /// The server the window holds as `host`, of `held`.
    pub fn of(held: &[Self], host: Host) -> Option<&Self> {
        held.iter().find(|m| m.host == host)
    }

    /// Hold the server paired with `server` as `host`, reaching it.
    pub fn linked(host: Host, server: &crate::servers::Server, tx: &Sender) -> Self {
        let (said, stood, paired) = (tx.clone(), tx.clone(), server.clone());
        let link = link::connect(
            move || paired.reach(),
            move |event| {
                let _ = said.send(Msg::Event(host, event));
            },
            move |state| {
                let _ = stood.send(Msg::Link(host, state));
            },
        );
        let transport = Transport::Remote(link);
        let mut m = Self::new(host, server.name.clone(), transport, Up::Reaching);
        m.paired = Some(server.clone());
        m
    }

    pub fn send(&self, call: Call) {
        match &self.transport {
            Transport::Local(server) => server.handle(call),
            Transport::Remote(link) => link.send(call),
        }
    }

    /// The server in this process, where this is it.
    pub fn local(&self) -> Option<&std::sync::Arc<Server>> {
        match &self.transport {
            Transport::Local(server) => Some(server),
            Transport::Remote(_) => None,
        }
    }

    /// Reach it again now, rather than when its wait is over.
    pub fn again(&self) {
        if let Transport::Remote(link) = &self.transport {
            link.again();
        }
    }

    /// Wait until what the server was asked to write is written, or what
    /// was asked of one elsewhere has gone to it: as u0 quits.
    pub fn settle(&self) {
        match &self.transport {
            Transport::Local(server) => {
                server.unshare();
                Server::settle();
            }
            Transport::Remote(link) => link.settle(),
        }
    }
}

/// Pair with the server `way` by the one-time `code` with `key`, and
/// keep it in `home`; `back`, a pairing link for it to pair back with.
pub fn pair(
    home: PathBuf,
    way: crate::reach::Way,
    code: String,
    key: crate::key::Key,
    back: Option<String>,
    tx: Sender,
) {
    acp::runtime().spawn(async move {
        let paired = crate::servers::pair(&home, way, code, key, back).await;
        let _ = tx.send(Msg::Paired(paired));
    });
}

/// Bring the port `there` of `server`, held as `host`, here, to open
/// `address` on: over a link of its own, signed with this Mac's key for
/// it (read off the window's thread: the Keychain may ask you first).
pub fn forward(
    host: Host,
    server: crate::servers::Server,
    there: u16,
    address: String,
    tx: Sender,
) {
    off(tx, move || Msg::Forwarded {
        host,
        address,
        result: server
            .reach()
            .and_then(|(way, key)| link::forward(way, key.kept().to_owned(), there)),
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
        let path = std::env::temp_dir().join(format!("u0-capture-{}.png", std::process::id()));
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
            mime: gpui::ImageFormat::Png.mime_type().into(),
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

/// The MIME type a picture in `format` is sent to an agent as; none, a
/// kind agents do not read.
pub fn agents_read(format: gpui::ImageFormat) -> Option<&'static str> {
    use gpui::ImageFormat::{Gif, Jpeg, Png, Webp};
    matches!(format, Png | Jpeg | Gif | Webp).then(|| format.mime_type())
}

/// What a photo in HEIC (or HEIF) is taken as, to be made a JPEG.
const HEIC: &str = "image/heic";

/// The kind of picture `path` holds, by its name; none, not one.
fn picture_kind(path: &std::path::Path) -> Option<&'static str> {
    let mime = mime_guess::from_path(path).first()?;
    match mime.essence_str() {
        "image/heic" | "image/heif" => Some(HEIC),
        mime => gpui::ImageFormat::from_mime_type(mime).and_then(agents_read),
    }
}

/// Read the picture at `path`, for what you write: a photo in HEIC, which
/// agents do not read, or one over `PICTURE`, made a JPEG (by macOS's
/// `sips`), no wider or taller than 2560 points.
pub fn read_picture(path: PathBuf, tx: Sender) {
    off(tx, move || {
        let mime = picture_kind(&path).unwrap_or(gpui::ImageFormat::Png.mime_type());
        let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > PICTURE);
        let read = if mime == HEIC || big {
            let out = std::env::temp_dir().join(format!("u0-picture-{}.jpg", std::process::id()));
            let made = std::process::Command::new("sips")
                .args(["-s", "format", "jpeg", "-Z", "2560"])
                .arg(&path)
                .arg("--out")
                .arg(&out)
                .output()
                .is_ok_and(|o| o.status.success());
            let bytes = made.then(|| std::fs::read(&out).ok()).flatten();
            let _ = std::fs::remove_file(&out);
            bytes.map(|b| (gpui::ImageFormat::Jpeg.mime_type(), b))
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
/// With `line` (`12`, or `12:5` with its column), at that line, where
/// the editor takes one.
pub fn edit(
    path: PathBuf,
    line: Option<String>,
    chosen: Option<String>,
    start: Option<&'static [u8]>,
    tx: Sender,
) {
    acp::runtime().spawn_blocking(move || {
        let shown = crate::desk::Desk::shown(&path);
        let there = path.exists() || start.is_some_and(|s| crate::whole::write(&path, s).is_ok());
        if !there {
            let why = path.display().to_string();
            let _ = tx.send(Msg::Notice(Err((
                crate::tr_with("There is no %{shown} here", &[("shown", &shown)]).into(),
                why,
            ))));
            return;
        }
        let editor = (chosen.as_deref().and_then(|c| which::which(c).ok()))
            .or_else(|| (crate::desk::EDITORS.into_iter()).find_map(|(c, _)| which::which(c).ok()));
        let opened = match editor {
            Some(cmd) => {
                let mut command = std::process::Command::new(&cmd);
                match &line {
                    // VS Code and Cursor go to a line with -g; Zed and
                    // Sublime Text read it after the name.
                    Some(line) => {
                        let at = format!("{}:{line}", path.display());
                        let goes = cmd
                            .file_name()
                            .is_some_and(|n| n == "code" || n == "cursor");
                        command.args(goes.then_some("-g")).arg(at)
                    }
                    None => command.arg(&path),
                }
                .status()
            }
            None => std::process::Command::new("open")
                .arg("-t")
                .arg(&path)
                .status(),
        };
        if let Err(e) = opened {
            let _ = tx.send(Msg::Notice(Err((
                crate::tr_with("Couldn't open %{shown}", &[("shown", &shown)]).into(),
                e.to_string(),
            ))));
        }
    });
}

/// Bind the action `name` to `key` in your keybindings.json (none: give
/// it u0's keys back), on the writer, so that two binds made quickly
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
            Ok(()) => Ok(crate::tr_with("It is in %{shown}", &[("shown", &shown)]).into()),
            Err(e) => Err((
                crate::tr_with("Couldn't write %{shown}", &[("shown", &shown)]).into(),
                e.to_string(),
            )),
        })
    });
}

/// The `u0` command, as t3code's `t3`: all it is given said to this
/// program, marked as a terminal's (`cli`), which reads the words and
/// opens the folders named.
fn command(exe: &std::path::Path) -> String {
    format!(
        "#!/bin/sh\n# u0 from a terminal: u0 --help.\nexec '{}' --shell \"$@\"\n",
        exe.display().to_string().replace('\'', "'\\''")
    )
}

/// Put the `u0` command in `~/.local/bin`, and say where it is, or
/// why it is not.
pub fn install_command(tx: Sender) {
    off(tx, || {
        let Some(home) = crate::desk::Desk::home_path() else {
            return Msg::Notice(Err((
                crate::tr("Couldn't add the u0 command").into(),
                crate::tr("HOME is not set").into(),
            )));
        };
        let path = home.join(".local/bin/u0");
        let shown = crate::desk::Desk::shown(&path);
        let exe = std::env::current_exe().unwrap_or_default();
        let put = crate::whole::write(&path, command(&exe).as_bytes()).and_then(|()| {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        });
        Msg::Notice(match put {
            Ok(()) if which::which("u0").is_ok_and(|p| p == path) => Ok(crate::tr_with(
                "u0 <folder> opens it in u0: %{shown}",
                &[("shown", &shown)],
            )
            .into()),
            Ok(()) => Ok(crate::tr_with(
                "It is in %{shown}; add ~/.local/bin to your PATH",
                &[("shown", &shown)],
            )
            .into()),
            Err(e) => Err((
                crate::tr("Couldn't add the u0 command").into(),
                e.to_string(),
            )),
        })
    });
}

/// Look for a build of u0 newer than this one on `channel`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_goes_to_the_server_named_and_what_it_says_comes_back_named_by_it() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // Something kept on B alone, to tell its index from A's.
        std::fs::write(
            b.path().join("sessions.json"),
            r#"{"sessions":[],"stashed":["on b"]}"#,
        )
        .unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let held = [
            Machine::in_process(Host::HERE, a.path(), &tx),
            Machine::in_process(Host(7), b.path(), &tx),
        ];
        Machine::of(&held, Host(7)).unwrap().send(Call::Watch);
        let said = acp::runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await
        });
        let Ok(Some(Msg::Event(host, Event::Index(Ok(index))))) = said else {
            panic!("B's index, named by B")
        };
        assert_eq!((host, index.stashed), (Host(7), vec!["on b".to_owned()]));
        assert!(Machine::of(&held, Host(3)).is_none());
    }
}
