//! The desk's connections to daemons, on a runtime of their own.
//!
//! Two kinds. A [`Link`] follows one ghost on one daemon, in the two roles
//! the protocol has: a console that asks about her and streams her
//! journal, and, once there is something to say, a presence named `desk`
//! that carries the conversation between you and her. Each session with
//! the solver is a place of its own on that channel, so her journal keeps
//! the threads apart. A [`Listing`] only asks a daemon who it runs, now and
//! then, so that the desk can show a daemon's cast without opening any of
//! them.
//!
//! All of it runs on one small tokio runtime and hands what it learns to
//! the UI as [`Update`]s over a plain channel; the UI drains it on its own
//! clock. Dropping either stops it. Nothing here touches a ghost's files.

use std::{
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

use async_trait::async_trait;
use ghost_link::{
    Console,
    presence::{self, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, Inbound, Place, PlaceKind},
};
use serde_json::Value;
use tokio::{runtime::Runtime, task::JoinHandle};
use tracing::{info, warn};

use crate::model::{Entry, LinkState, PersonView, Status, Update};

/// How many journal entries the desk holds when it connects.
pub const TAIL: u32 = 400;
/// How often it asks after her, and after who a daemon runs, again.
const REFRESH: Duration = Duration::from_secs(20);
const RETRY: Duration = Duration::from_secs(3);

/// The channel name the desk registers as.
pub const CHANNEL: &str = "desk";

/// Where every link runs: two threads are plenty for a handful of sockets.
pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ghost-link")
            .enable_all()
            .build()
            .expect("a tokio runtime")
    })
}

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub token: Option<String>,
    /// Which of the daemon's characters, by her id: `None` when it runs
    /// only one.
    pub ghost: Option<String>,
    /// Who you are to her.
    pub who: String,
}

/// A line you typed, on its way to her, in one place on the desk channel.
pub struct Outgoing {
    pub place: String,
    pub text: String,
}

/// One ghost the desk follows.
pub struct Link {
    /// Where what the link learns arrives.
    pub rx: mpsc::Receiver<Update>,
    up: mpsc::Sender<Update>,
    cfg: Config,
    /// Where what you say goes, once the desk speaks to her.
    lines: Option<tokio::sync::mpsc::UnboundedSender<Outgoing>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Link {
    /// Watch the ghost `cfg` names: ask about her, and follow her journal.
    pub fn watch(cfg: Config) -> Self {
        let (up, rx) = mpsc::channel();
        let console = runtime().spawn(console(cfg.clone(), up.clone()));
        Self {
            rx,
            up,
            cfg,
            lines: None,
            tasks: vec![console],
        }
    }

    /// Carry what you say to her from now on, by registering the desk's
    /// channel for her. Until then the desk only watches, and takes no
    /// channel of hers.
    pub fn speak(&mut self) {
        if self.lines.is_some() {
            return;
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let desk = Arc::new(Desk {
            who: self.cfg.who.clone(),
            lines: Mutex::new(Some(rx)),
            up: self.up.clone(),
        });
        let link = presence::Link {
            url: self.cfg.url.clone(),
            token: self.cfg.token.clone(),
            ghost: self.cfg.ghost.clone(),
            channel: CHANNEL.to_owned(),
            me: None,
            can: vec![Capability::Send, Capability::Typing, Capability::Read],
        };
        self.tasks.push(runtime().spawn(async move {
            if let Err(e) = presence::run(desk, link).await {
                warn!("the desk's presence stopped: {e:#}");
            }
        }));
        self.lines = Some(tx);
    }

    /// Say `text` to her in the place `place` on the desk channel.
    pub fn say(&mut self, place: String, text: String) {
        self.speak();
        if let Some(lines) = &self.lines {
            let _ = lines.send(Outgoing { place, text });
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Who one daemon runs, asked now and then.
pub struct Listing {
    pub rx: mpsc::Receiver<Update>,
    task: JoinHandle<()>,
}

impl Listing {
    /// Ask the daemon at `url` who it runs, and again every little while,
    /// connecting again when it goes away.
    pub fn follow(url: String, token: Option<String>) -> Self {
        let (up, rx) = mpsc::channel();
        let task = runtime().spawn(async move {
            loop {
                match ghost_link::list(&url, token.clone()).await {
                    Ok(roster) => {
                        let _ = up.send(Update::Link(LinkState::Connected));
                        let _ = up.send(Update::Roster(roster.ghosts));
                        tokio::time::sleep(REFRESH).await;
                    }
                    Err(e) => {
                        let _ = up.send(Update::Link(LinkState::Lost(format!("{e:#}"))));
                        tokio::time::sleep(RETRY).await;
                    }
                }
            }
        });
        Self { rx, task }
    }
}

impl Drop for Listing {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The console role: connect, ask everything once, then follow her journal
/// as it is written and ask again now and then. Reconnects when the daemon
/// goes away.
async fn console(cfg: Config, up: mpsc::Sender<Update>) {
    loop {
        let _ = up.send(Update::Link(LinkState::Connecting));
        match session(&cfg, &up).await {
            Ok(()) => {
                let _ = up.send(Update::Link(LinkState::Lost(
                    "the daemon closed the connection".into(),
                )));
            }
            Err(e) => {
                let _ = up.send(Update::Link(LinkState::Lost(format!("{e:#}"))));
            }
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn session(cfg: &Config, up: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    // One connection streams her journal; a second asks questions, since a
    // call on a subscribed connection would drop entries that arrive while
    // it waits for its answer.
    let (mut asker, welcome) =
        Console::connect(&cfg.url, cfg.token.clone(), cfg.ghost.clone()).await?;
    let (mut stream, _) = Console::connect(&cfg.url, cfg.token.clone(), cfg.ghost.clone()).await?;
    info!(
        "connected to {} as a console for {}",
        cfg.url, welcome.ghost.name
    );
    let _ = up.send(Update::Welcome(welcome.ghost));
    let _ = up.send(Update::Link(LinkState::Connected));

    ask_all(&mut asker, up).await?;
    let tail = asker.journal(-i64::from(TAIL), TAIL).await?;
    let mut entries: Vec<Entry> = tail.iter().filter_map(Entry::from_value).collect();
    entries.sort_by_key(|e| e.seq);
    let _ = up.send(Update::Journal(entries));

    stream.subscribe().await?;
    let mut refresh = tokio::time::interval(REFRESH);
    refresh.tick().await;
    loop {
        tokio::select! {
            entry = stream.next_entry() => {
                match entry? {
                    Some(v) => {
                        if let Some(entry) = Entry::from_value(&v) {
                            let _ = up.send(Update::Entry(entry));
                        }
                    }
                    None => return Ok(()),
                }
            }
            _ = refresh.tick() => {
                ask_all(&mut asker, up).await?;
            }
        }
    }
}

async fn ask_all(asker: &mut Console, up: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let status = asker.status().await?;
    let _ = up.send(Update::Status(Box::new(Status::from_value(&status))));
    if let Ok(Value::Array(people)) = asker.people().await {
        let _ = up.send(Update::People(
            people.iter().filter_map(PersonView::from_value).collect(),
        ));
    }
    // A daemon from before work could be listed has no answer; the desk
    // then shows none.
    if let Ok(work) = asker.work().await {
        let _ = up.send(Update::Work(work));
    }
    Ok(())
}

/// The key her memory gives you, as a person, on the desk.
pub fn person_key(who: &str) -> String {
    format!("{CHANNEL}:{}", who.to_lowercase())
}

/// The desk as a platform: you type, she answers. Each session is a place.
struct Desk {
    who: String,
    lines: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Outgoing>>>,
    up: mpsc::Sender<Update>,
}

impl Desk {
    fn id() -> String {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        format!("desk-{stamp}")
    }
}

#[async_trait]
impl Platform for Desk {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let Some(mut lines) = self.lines.lock().expect("the lines lock").take() else {
            // A second listen: the first took the receiver and is still the
            // one pumping. Nothing to do but wait out this connection.
            std::future::pending::<()>().await;
            return Ok(());
        };
        let _ = self.up.send(Update::DeskOnline(true));
        while let Some(Outgoing { place, text }) = lines.recv().await {
            up.inbound(Inbound {
                place: Place {
                    id: place,
                    kind: PlaceKind::Direct,
                    name: None,
                },
                from: Actor {
                    id: self.who.to_lowercase(),
                    name: self.who.clone(),
                    handle: None,
                    automated: false,
                },
                id: Self::id(),
                text,
                reply_to: None,
                addressed: true,
                images: Vec::new(),
            })
            .await;
        }
        Ok(())
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        match action {
            Action::Send { place, text, .. } => {
                let _ = self.up.send(Update::HerSay { place, text });
                Ok(Acted {
                    id: Some(Self::id()),
                    items: Vec::new(),
                    measure: None,
                })
            }
            Action::Typing { place } => {
                let _ = self.up.send(Update::HerTyping { place });
                Ok(Acted::default())
            }
            _ => Ok(Acted::default()),
        }
    }
}
