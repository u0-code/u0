//! Everything a channel adapter needs besides the channel itself.
//!
//! An adapter implements [`Platform`]: one method that pumps what arrives on
//! the platform upward, one that carries out an action. [`run`] does the
//! rest. It keeps the connection to the daemon alive with backoff, holds
//! incoming messages while the daemon is away so a restart loses nothing,
//! and answers each action request with what the platform said.
//!
//! A presence carries one character, named in [`Link::ghost`]: one daemon
//! can run several, and a platform account belongs to one of them. Once it
//! knows who she is, [`Platform::appear`] makes the account look like her
//! where the platform lets a program do that, and [`set_by_hand`] says what
//! is left for a person to set.

use std::{sync::Arc, time::Duration};

use anyhow::anyhow;
use async_trait::async_trait;
use futures_util::StreamExt;
use ghost_protocol::{
    Acted, Action, Actor, Allows, Capability, Frame, GhostCard, Hello, Inbound, PROTOCOL,
    PresenceHello, Role, RpcError, Typing, code, method,
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::{
    Conn,
    socket::{recv, send},
};

/// How many incoming messages are held while the daemon is unreachable.
const HELD: usize = 2048;
const FIRST_RETRY: Duration = Duration::from_secs(1);
const SLOWEST_RETRY: Duration = Duration::from_secs(30);

/// One messaging platform, as seen from Ghost.
#[async_trait]
pub trait Platform: Send + Sync + 'static {
    /// Pump what arrives on the platform into `up`. Return only when the
    /// platform connection is gone for good; reconnecting to the platform is
    /// this method's job.
    async fn listen(&self, up: Uplink) -> anyhow::Result<()>;

    /// Carry out one thing she does. Only actions matching the declared
    /// capabilities are sent.
    async fn act(&self, action: Action) -> anyhow::Result<Acted>;

    /// Make the account look like her: her name and bio, from `card`. Called
    /// when the daemon first says who she is, and again only if that
    /// changes, so it runs once at start and not at every reconnection. It
    /// runs beside everything else and nothing waits for it.
    ///
    /// Only what the platform's API lets a program change, and simply,
    /// belongs here; for the rest, call [`set_by_hand`]. The default does
    /// nothing, for a presence with no profile to speak of.
    async fn appear(&self, card: &GhostCard) {
        let _ = card;
    }
}

/// A part of her profile on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Bio,
    Avatar,
}

/// Say, in one line, which parts of her profile whoever runs this presence
/// has to set by hand, and where: `place` is the platform's own console,
/// such as "the QQ open platform's bot settings". A bio she does not have
/// is not asked for.
pub fn set_by_hand(card: &GhostCard, fields: &[Field], place: &str) {
    if let Some(line) = by_hand(card, fields, place) {
        info!("{line}");
    }
}

fn by_hand(card: &GhostCard, fields: &[Field], place: &str) -> Option<String> {
    let parts: Vec<String> = fields
        .iter()
        .filter_map(|field| match field {
            Field::Name => Some(format!("name ({})", card.name)),
            Field::Bio => card.bio.as_ref().map(|bio| format!("bio ({bio:?})")),
            Field::Avatar => {
                Some("avatar (assets/avatar.* in her ghost directory, if she has one)".to_owned())
            }
        })
        .collect();
    let (final_one, rest) = parts.split_last()?;
    let list = if rest.is_empty() {
        final_one.clone()
    } else {
        format!("{} and {final_one}", rest.join(", "))
    };
    Some(format!(
        "set her {list} by hand in {place}: this presence cannot change them there"
    ))
}

/// How an adapter is known to the daemon.
#[derive(Debug, Clone)]
pub struct Link {
    /// `ws://host:port` of the daemon.
    pub url: String,
    pub token: Option<String>,
    /// The character this presence carries: the name of her directory.
    /// `None` is whoever the daemon runs, when it runs only one.
    pub ghost: Option<String>,
    /// The channel name this adapter carries. One adapter per name.
    pub channel: String,
    /// Who she is on this platform, so her own messages can be recognized.
    pub me: Option<Actor>,
    pub can: Vec<Capability>,
    /// What the platform allows her.
    pub allows: Allows,
}

/// The adapter's way of telling the daemon what happened on the platform.
#[derive(Clone)]
pub struct Uplink {
    tx: mpsc::Sender<Frame>,
}

impl Uplink {
    /// A message arrived.
    pub async fn inbound(&self, message: Inbound) {
        self.push(Frame::notify(method::INBOUND, &message)).await;
    }

    /// Someone is typing in a place.
    pub fn typing(&self, place: impl Into<String>) {
        // Typing is only useful fresh: drop it rather than queue it.
        let frame = Frame::notify(
            method::TYPING,
            &Typing {
                place: place.into(),
            },
        );
        let _ = self.tx.try_send(frame);
    }

    async fn push(&self, frame: Frame) {
        if self.tx.send(frame).await.is_err() {
            debug!("the link is gone; dropping a message");
        }
    }
}

/// Carry a ghost on one platform until the platform goes away.
pub async fn run<P: Platform>(platform: Arc<P>, link: Link) -> anyhow::Result<()> {
    let (up_tx, mut up_rx) = mpsc::channel::<Frame>(HELD);
    let mut listener = tokio::spawn({
        let platform = platform.clone();
        async move { platform.listen(Uplink { tx: up_tx }).await }
    });
    let hello = Hello {
        protocol: PROTOCOL,
        role: Role::Presence,
        token: link.token.clone(),
        ghost: link.ghost.clone(),
        presence: Some(PresenceHello {
            channel: link.channel.clone(),
            me: link.me.clone(),
            can: link.can.clone(),
            allows: link.allows,
        }),
        who: None,
    };
    // A frame taken off the queue that the daemon never received.
    let mut undelivered: Option<Frame> = None;
    let mut retry = FIRST_RETRY;
    // Who she was the last time the account was made to look like her.
    let mut appeared: Option<GhostCard> = None;
    loop {
        match Conn::open(&link.url, &hello).await {
            Ok((conn, welcome)) => {
                info!("carrying {} on {}", welcome.ghost.name, link.channel);
                retry = FIRST_RETRY;
                if appeared.as_ref() != Some(&welcome.ghost) {
                    appeared = Some(welcome.ghost.clone());
                    let (platform, card) = (platform.clone(), welcome.ghost.clone());
                    tokio::spawn(async move { platform.appear(&card).await });
                }
                let (mut sink, mut stream) = conn.ws.split();
                let (done_tx, mut done_rx) = mpsc::channel::<Frame>(64);
                let lost = 'connected: loop {
                    if let Some(frame) = undelivered.take()
                        && let Err(e) = send(&mut sink, &frame).await
                    {
                        undelivered = Some(frame);
                        break 'connected e;
                    }
                    tokio::select! {
                        Some(frame) = up_rx.recv() => undelivered = Some(frame),
                        Some(frame) = done_rx.recv() => {
                            if let Err(e) = send(&mut sink, &frame).await {
                                break 'connected e;
                            }
                        }
                        frame = recv(&mut stream) => match frame {
                            Ok(Some(frame)) if frame.method.as_deref() == Some(method::DISPLACED) => {
                                // A newer presence carries this channel now.
                                // Reconnecting would only take it back.
                                listener.abort();
                                anyhow::bail!("a newer presence took over the {} channel", link.channel);
                            }
                            Ok(Some(frame)) => answer(&frame, &platform, &done_tx),
                            Ok(None) => break 'connected anyhow!("it closed the connection"),
                            Err(e) => break 'connected e,
                        },
                        ended = &mut listener => return ended?,
                    }
                };
                warn!("lost the daemon: {lost:#}");
            }
            Err(e) => {
                if let Some(rpc) = e.downcast_ref::<RpcError>()
                    && matches!(
                        rpc.code,
                        code::UNAUTHORIZED | code::VERSION | code::NO_SUCH_GHOST
                    )
                {
                    // Retrying cannot fix these.
                    listener.abort();
                    return Err(e);
                }
                warn!("cannot reach the daemon: {e:#}");
            }
        }
        tokio::select! {
            () = tokio::time::sleep(retry) => {}
            ended = &mut listener => return ended?,
        }
        retry = (retry * 2).min(SLOWEST_RETRY);
    }
}

/// Deal with one frame from the daemon. Actions run concurrently; each
/// reports back through `done`.
fn answer<P: Platform>(frame: &Frame, platform: &Arc<P>, done: &mpsc::Sender<Frame>) {
    let Some(id) = frame.id else {
        return;
    };
    if frame.is_response() {
        return;
    }
    if frame.method.as_deref() != Some(method::ACT) {
        let _ = done.try_send(Frame::err(
            id,
            code::METHOD_NOT_FOUND,
            "not something a presence does",
        ));
        return;
    }
    let action = match frame.params::<Action>() {
        Ok(action) => action,
        Err(e) => {
            let _ = done.try_send(Frame::err(id, e.code, e.message));
            return;
        }
    };
    let (platform, done) = (platform.clone(), done.clone());
    tokio::spawn(async move {
        let reply = match platform.act(action).await {
            Ok(acted) => Frame::ok(id, &acted),
            Err(e) => Frame::err(id, code::ACT_FAILED, format!("{e:#}")),
        };
        let _ = done.send(reply).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(bio: Option<&str>) -> GhostCard {
        GhostCard {
            name: "薇薇".into(),
            pronouns: "she/her".into(),
            city: "大荒".into(),
            country: "CN".into(),
            seal: "s".into(),
            bio: bio.map(Into::into),
        }
    }

    #[test]
    fn what_is_left_to_set_by_hand_is_said_in_one_line_and_a_missing_bio_is_not_asked_for() {
        let all = [Field::Name, Field::Bio, Field::Avatar];
        let line = by_hand(&card(Some("看兽的医生")), &all, "the console").unwrap();
        assert!(line.starts_with("set her name (薇薇), bio ("), "{line}");
        assert!(
            line.contains("看兽的医生") && line.contains(" and avatar ("),
            "{line}"
        );
        assert!(line.contains("in the console"), "{line}");
        let line = by_hand(&card(None), &all, "the console").unwrap();
        assert!(line.starts_with("set her name (薇薇) and avatar"), "{line}");
        assert_eq!(by_hand(&card(None), &[Field::Bio], "the console"), None);
    }
}
