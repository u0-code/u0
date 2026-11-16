//! The desk's connection to `ghostd`, on its own thread.
//!
//! Two roles at once, as the protocol has them: a console that asks about
//! her and streams her journal, and a presence named `desk` that carries
//! the conversation between you and her. Both run on a tokio runtime on a
//! thread of their own and hand what they learn to the UI as [`Update`]s
//! over a plain channel; the UI drains it on its own clock. Nothing here
//! touches a ghost's files.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use async_trait::async_trait;
use ghost_link::{
    Console,
    presence::{self, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, Inbound, Place, PlaceKind},
};
use serde_json::Value;
use tracing::{info, warn};

use crate::model::{Entry, LinkState, PersonView, PlaceView, Status, Update};

/// How many journal entries the desk holds when it connects.
pub const TAIL: u32 = 300;
/// How often it asks after her again.
const REFRESH: Duration = Duration::from_secs(20);
const RETRY: Duration = Duration::from_secs(3);

/// The channel name the desk registers as.
pub const CHANNEL: &str = "desk";

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub token: Option<String>,
    pub ghost: Option<String>,
    /// Who you are to her.
    pub who: String,
}

/// A line you typed, on its way to her.
pub struct Outgoing(pub String);

/// Start the link. Returns where updates arrive and where to put what you say.
pub fn start(
    cfg: Config,
) -> (
    mpsc::Receiver<Update>,
    tokio::sync::mpsc::UnboundedSender<Outgoing>,
) {
    let (up_tx, up_rx) = mpsc::channel();
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("ghost-link".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("a tokio runtime");
            rt.block_on(async move {
                let console = tokio::spawn(console(cfg.clone(), up_tx.clone()));
                let desk = Arc::new(Desk {
                    who: cfg.who.clone(),
                    lines: Mutex::new(Some(out_rx)),
                    up: up_tx.clone(),
                });
                let link = Link {
                    url: cfg.url.clone(),
                    token: cfg.token.clone(),
                    ghost: cfg.ghost.clone(),
                    channel: CHANNEL.to_owned(),
                    me: None,
                    can: vec![Capability::Send, Capability::Typing, Capability::Read],
                };
                let presence = tokio::spawn(async move {
                    if let Err(e) = presence::run(desk, link).await {
                        warn!("the desk's presence stopped: {e:#}");
                    }
                });
                let _ = tokio::join!(console, presence);
            });
        })
        .expect("spawn the link thread");
    (up_rx, out_tx)
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
    let agenda = asker.agenda().await.unwrap_or(Value::Null);
    let _ = up.send(Update::Agenda(agenda));
    if let Ok(Value::Array(places)) = asker.places().await {
        let _ = up.send(Update::Places(
            places.iter().filter_map(PlaceView::from_value).collect(),
        ));
    }
    if let Ok(Value::Array(people)) = asker.people().await {
        let _ = up.send(Update::People(
            people.iter().filter_map(PersonView::from_value).collect(),
        ));
    }
    Ok(())
}

/// The key of the private place between you and her, as her memory names it.
pub fn place_key(who: &str) -> String {
    format!("{CHANNEL}:{}", who.to_lowercase())
}

/// The desk as a platform: you type, she answers.
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

    fn place(&self) -> Place {
        Place {
            id: self.who.to_lowercase(),
            kind: PlaceKind::Direct,
            name: None,
        }
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
        while let Some(Outgoing(text)) = lines.recv().await {
            up.inbound(Inbound {
                place: self.place(),
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
            Action::Send { text, .. } => {
                let _ = self.up.send(Update::HerSay(text));
                Ok(Acted {
                    id: Some(Self::id()),
                    items: Vec::new(),
                    measure: None,
                })
            }
            Action::Typing { .. } => {
                let _ = self.up.send(Update::HerTyping);
                Ok(Acted::default())
            }
            _ => Ok(Acted::default()),
        }
    }
}

/// What a daemon said it runs, when asked without naming anyone.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub url: String,
    /// Each character's directory name, or `None` for a daemon that runs
    /// one and needs no name.
    pub ghosts: Vec<Option<String>>,
    pub error: Option<String>,
}

/// Ask a daemon which characters it runs. A daemon that runs one answers
/// a hello without a name; one that runs several refuses it and names them
/// in its refusal, which is the only way the protocol lists them today.
pub fn discover(url: String, token: Option<String>, out: mpsc::Sender<Discovered>) {
    std::thread::Builder::new()
        .name("ghost-discover".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
            let found = rt.block_on(async {
                match Console::connect(&url, token, None).await {
                    Ok(_) => Discovered {
                        url: url.clone(),
                        ghosts: vec![None],
                        error: None,
                    },
                    Err(e) => {
                        let text = format!("{e:#}");
                        let ghosts = names_in_refusal(&text);
                        if ghosts.is_empty() {
                            Discovered {
                                url: url.clone(),
                                ghosts: Vec::new(),
                                error: Some(text),
                            }
                        } else {
                            Discovered {
                                url: url.clone(),
                                ghosts: ghosts.into_iter().map(Some).collect(),
                                error: None,
                            }
                        }
                    }
                }
            });
            let _ = out.send(found);
        })
        .expect("spawn the discovery thread");
}

/// The names in "this daemon runs a, b; say which of them this is for".
fn names_in_refusal(text: &str) -> Vec<String> {
    let Some(rest) = text.split("this daemon runs ").nth(1) else {
        return Vec::new();
    };
    let list = rest.split(';').next().unwrap_or_default();
    list.split(',')
        .map(|n| n.trim().trim_matches('"').to_owned())
        .filter(|n| !n.is_empty() && !n.contains(' '))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_characters_it_runs() {
        let text = "hello: this daemon runs lin, mei; say which of them this is for (--ghost or GHOST_NAME)";
        assert_eq!(names_in_refusal(text), vec!["lin", "mei"]);
        assert!(names_in_refusal("connect to ws://x: refused").is_empty());
    }
}
