//! Everything a channel adapter needs besides the channel itself.
//!
//! An adapter implements [`Platform`]: one method that pumps what arrives on
//! the platform upward, one that carries out an action. [`run`] does the
//! rest. It keeps the connection to the daemon alive with backoff, holds
//! incoming messages while the daemon is away so a restart loses nothing,
//! and answers each action request with what the platform said.

use std::{sync::Arc, time::Duration};

use anyhow::anyhow;
use async_trait::async_trait;
use futures_util::StreamExt;
use ghost_protocol::{
    Acted, Action, Actor, Capability, Frame, Hello, Inbound, PROTOCOL, PresenceHello, Role,
    RpcError, Typing, code, method,
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
}

/// How an adapter is known to the daemon.
#[derive(Debug, Clone)]
pub struct Link {
    /// `ws://host:port` of the daemon.
    pub url: String,
    pub token: Option<String>,
    /// The channel name this adapter carries. One adapter per name.
    pub channel: String,
    /// Who she is on this platform, so her own messages can be recognized.
    pub me: Option<Actor>,
    pub can: Vec<Capability>,
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
        presence: Some(PresenceHello {
            channel: link.channel.clone(),
            me: link.me.clone(),
            can: link.can.clone(),
        }),
    };
    // A frame taken off the queue that the daemon never received.
    let mut undelivered: Option<Frame> = None;
    let mut retry = FIRST_RETRY;
    loop {
        match Conn::open(&link.url, &hello).await {
            Ok((conn, welcome)) => {
                info!("carrying {} on {}", welcome.ghost.name, link.channel);
                retry = FIRST_RETRY;
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
                    && matches!(rpc.code, code::UNAUTHORIZED | code::VERSION)
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
