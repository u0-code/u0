//! The Ghost Protocol, server side.
//!
//! Each connection says hello and is then either a presence or a console.
//! Presences are also her [`Body`]: when the mind acts on a channel, the
//! request goes to whichever presence registered that channel and the mind
//! waits for the platform's answer.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use ghost_link::socket::{recv, send};
use ghost_mind::{Act, Acted, Body, MindHandle, Question};
use ghost_protocol::{
    Action, Capability, Frame, GhostCard, Hello, Inbound, JournalRead, PROTOCOL, PlaceRead,
    PresenceHello, Role, Typing, Welcome, code, method,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, broadcast::error::RecvError, mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    http::StatusCode,
};
use tracing::{debug, info, warn};

use crate::wire;

/// How long a client has to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// How long a presence has to carry out an action.
const ACT_WITHIN: Duration = Duration::from_secs(45);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Frame>>>>;

struct Presence {
    /// Which connection this is, so a replaced one cannot unregister its successor.
    id: u64,
    /// Signalled when a newer presence takes this channel.
    displaced: Arc<Notify>,
    out: mpsc::Sender<Frame>,
    pending: Pending,
    can: Vec<Capability>,
}

/// The presences currently connected, keyed by channel. This is her body.
#[derive(Default)]
pub struct Presences {
    channels: Mutex<HashMap<String, Presence>>,
    next_id: AtomicU64,
}

#[async_trait]
impl Body for Presences {
    async fn act(&self, channel: &str, act: Act) -> Result<Acted, String> {
        let action: Action = wire::action(act);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (reply_tx, reply_rx) = oneshot::channel();
        let (out, pending) = {
            let channels = self.channels.lock().map_err(|e| e.to_string())?;
            let presence = channels
                .get(channel)
                .ok_or_else(|| format!("{channel} is not connected"))?;
            if !presence.can.contains(&action.needs()) {
                return Err(format!("{channel} cannot {:?}", action.needs()));
            }
            presence
                .pending
                .lock()
                .map_err(|e| e.to_string())?
                .insert(id, reply_tx);
            (presence.out.clone(), presence.pending.clone())
        };
        let forget = || {
            if let Ok(mut pending) = pending.lock() {
                pending.remove(&id);
            }
        };
        if out
            .send(Frame::request(id, method::ACT, &action))
            .await
            .is_err()
        {
            forget();
            return Err(format!("{channel} went away"));
        }
        let frame = match tokio::time::timeout(ACT_WITHIN, reply_rx).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(_)) => return Err(format!("{channel} went away")),
            Err(_) => {
                forget();
                return Err(format!("{channel} did not answer in time"));
            }
        };
        frame
            .into_result::<ghost_protocol::Acted>()
            .map(wire::acted)
            .map_err(|e| e.message)
    }
}

/// What every connection needs.
pub struct Shared {
    pub mind: MindHandle,
    pub presences: Arc<Presences>,
    pub token: Option<String>,
    pub card: GhostCard,
    /// Web origins allowed to connect, beyond this machine's own.
    pub origins: Vec<String>,
}

/// Accept connections until the task is dropped.
pub async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let shared = shared.clone();
                tokio::spawn(async move {
                    if let Err(e) = connection(stream, peer, shared).await {
                        debug!("{peer}: {e:#}");
                    }
                });
            }
            Err(e) => {
                warn!("accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn connection(
    stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
) -> anyhow::Result<()> {
    // A browser says which page opened the socket. Without this check any
    // web page the operator visits could read her journal on localhost.
    let origins = shared.origins.clone();
    // The callback's signature, large error and all, is tungstenite's.
    #[allow(clippy::result_large_err)]
    let ws = tokio_tungstenite::accept_hdr_async(
        stream,
        move |request: &Request, response: Response| {
            let origin = request
                .headers()
                .get("origin")
                .and_then(|v| v.to_str().ok());
            if origin.is_none_or(|origin| origin_allowed(origin, &origins)) {
                Ok(response)
            } else {
                warn!(
                    "{peer}: refused a connection from {}",
                    origin.unwrap_or_default()
                );
                let mut refusal = ErrorResponse::new(Some("origin not allowed".to_owned()));
                *refusal.status_mut() = StatusCode::FORBIDDEN;
                Err(refusal)
            }
        },
    )
    .await?;
    let (mut sink, mut stream) = ws.split();

    let Some(first) = tokio::time::timeout(HELLO_WITHIN, recv(&mut stream)).await?? else {
        return Ok(());
    };
    let id = first.id.unwrap_or(0);
    let refuse = |code: i64, message: &str| Frame::err(id, code, message);
    let (Some(method::HELLO), Ok(hello)) = (first.method.as_deref(), first.params::<Hello>())
    else {
        send(&mut sink, &refuse(code::INVALID_REQUEST, "say hello first")).await?;
        return Ok(());
    };
    if hello.protocol != PROTOCOL {
        let message = format!("this daemon speaks protocol {PROTOCOL}");
        send(&mut sink, &refuse(code::VERSION, &message)).await?;
        return Ok(());
    }
    if shared.token.is_some() && hello.token != shared.token {
        send(
            &mut sink,
            &refuse(code::UNAUTHORIZED, "wrong or missing token"),
        )
        .await?;
        return Ok(());
    }

    // Everything outgoing funnels through one writer.
    let (out, mut outgoing) = mpsc::channel::<Frame>(256);
    let writer = tokio::spawn(async move {
        while let Some(frame) = outgoing.recv().await {
            if send(&mut sink, &frame).await.is_err() {
                break;
            }
        }
    });
    let welcome = Frame::ok(
        id,
        &Welcome {
            protocol: PROTOCOL,
            ghost: shared.card.clone(),
        },
    );
    match (hello.role, hello.presence) {
        (Role::Presence, Some(presence)) => {
            carry(presence, welcome, id, out, &mut stream, &shared, peer).await;
        }
        (Role::Presence, None) => {
            let _ = out
                .send(refuse(
                    code::INVALID_PARAMS,
                    "a presence must describe itself",
                ))
                .await;
        }
        (Role::Console, _) => {
            let _ = out.send(welcome).await;
            watch(out, &mut stream, &shared).await;
        }
    }
    // Let the writer flush whatever is queued, then end with the connection.
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    Ok(())
}

/// Whether a web page at `origin` may connect. Native clients send no origin
/// and are not asked. Pages served from this machine are allowed; anything
/// else has to be listed.
fn origin_allowed(origin: &str, extra: &[String]) -> bool {
    if extra.iter().any(|allowed| allowed == origin) {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.split_once(']'))
        .map_or_else(
            || host.rsplit_once(':').map_or(host, |(h, _)| h),
            |(h, _)| h,
        );
    matches!(scheme, "http" | "https") && matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// A presence: register its channel, feed what it hears to the mind, and
/// route its answers back to whoever asked.
async fn carry<S>(
    hello: PresenceHello,
    welcome: Frame,
    hello_id: u64,
    out: mpsc::Sender<Frame>,
    stream: &mut S,
    shared: &Shared,
    peer: SocketAddr,
) where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let channel = hello.channel;
    let pending: Pending = Pending::default();
    // The newest presence for a channel carries it. Whoever held it before is
    // told and let go: a window left open, or an adapter that was restarted,
    // must not be able to hold a channel against its replacement.
    let id = shared.presences.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let displaced = Arc::new(Notify::new());
    let Ok(replaced) = shared.presences.channels.lock().map(|mut channels| {
        channels.insert(
            channel.clone(),
            Presence {
                id,
                displaced: displaced.clone(),
                out: out.clone(),
                pending: pending.clone(),
                can: hello.can.clone(),
            },
        )
    }) else {
        let _ = out
            .send(Frame::err(
                hello_id,
                code::INTERNAL,
                "the daemon is in a bad way",
            ))
            .await;
        return;
    };
    if let Some(old) = replaced {
        let _ = old.out.send(Frame::notify(method::DISPLACED, &())).await;
        old.displaced.notify_one();
        info!("{channel} taken over from {peer}");
    } else {
        info!("{channel} connected from {peer}");
    }
    let _ = out.send(welcome).await;
    let can = hello.can.into_iter().map(wire::can).collect();
    let _ = shared
        .mind
        .online(&channel, hello.me.map(wire::actor), can)
        .await;

    loop {
        let frame = tokio::select! {
            frame = recv(stream) => frame,
            // Replaced: say nothing more and let the connection close.
            () = displaced.notified() => break,
        };
        let frame = match frame {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) => {
                debug!("{channel}: {e:#}");
                break;
            }
        };
        if frame.is_response() {
            let waiter = frame
                .id
                .and_then(|id| pending.lock().ok().and_then(|mut p| p.remove(&id)));
            if let Some(waiter) = waiter {
                let _ = waiter.send(frame);
            }
            continue;
        }
        match frame.method.as_deref() {
            Some(method::INBOUND) => match frame.params::<Inbound>() {
                Ok(message) => {
                    let _ = shared.mind.heard(&channel, wire::incoming(message)).await;
                }
                Err(e) => warn!("{channel}: malformed inbound: {e}"),
            },
            Some(method::TYPING) => {
                if let Ok(typing) = frame.params::<Typing>() {
                    let _ = shared.mind.typing(&channel, &typing.place).await;
                }
            }
            other => {
                if let Some(id) = frame.id {
                    let message = format!("unknown method {other:?}");
                    let _ = out
                        .send(Frame::err(id, code::METHOD_NOT_FOUND, message))
                        .await;
                }
            }
        }
    }

    // Only the connection that still holds the channel may give it up.
    let held = shared.presences.channels.lock().is_ok_and(|mut channels| {
        let mine = channels.get(&channel).is_some_and(|p| p.id == id);
        if mine {
            channels.remove(&channel);
        }
        mine
    });
    if held {
        info!("{channel} disconnected");
        let _ = shared.mind.offline(&channel).await;
    }
}

/// A console: answer its questions and, if asked, stream her journal.
async fn watch<S>(out: mpsc::Sender<Frame>, stream: &mut S, shared: &Shared)
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let mut subscribed = false;
    while let Ok(Some(frame)) = recv(stream).await {
        let Some(id) = frame.id else { continue };
        let question = match frame.method.as_deref() {
            Some(method::STATUS) => Question::Status,
            Some(method::AGENDA) => Question::Agenda,
            Some(method::PEOPLE) => Question::People,
            Some(method::IDENTITY) => Question::Identity,
            Some(method::CANON) => Question::Canon,
            Some(method::PLACES) => Question::Places,
            Some(method::JOURNAL_PLACE) => match frame.params::<PlaceRead>() {
                Ok(read) => Question::Thread {
                    place: read.place,
                    limit: read.limit as usize,
                },
                Err(e) => {
                    let _ = out.send(Frame::err(id, e.code, e.message)).await;
                    continue;
                }
            },
            Some(method::JOURNAL_READ) => match frame.params::<JournalRead>() {
                Ok(read) => Question::Journal {
                    from: read.from,
                    limit: read.limit as usize,
                },
                Err(e) => {
                    let _ = out.send(Frame::err(id, e.code, e.message)).await;
                    continue;
                }
            },
            Some(method::JOURNAL_SUBSCRIBE) => {
                if !subscribed {
                    subscribed = true;
                    let mut entries = shared.mind.watch();
                    let out = out.clone();
                    tokio::spawn(async move {
                        loop {
                            match entries.recv().await {
                                Ok(entry) => {
                                    let frame = Frame::notify(method::JOURNAL_ENTRY, &entry);
                                    if out.send(frame).await.is_err() {
                                        break;
                                    }
                                }
                                Err(RecvError::Lagged(missed)) => {
                                    debug!("a console fell {missed} entries behind");
                                }
                                Err(RecvError::Closed) => break,
                            }
                        }
                    });
                }
                let _ = out.send(Frame::ok(id, &serde_json::json!({}))).await;
                continue;
            }
            other => {
                // Nothing here writes. There is no method that could.
                let message = format!("unknown method {other:?}");
                let _ = out
                    .send(Frame::err(id, code::METHOD_NOT_FOUND, message))
                    .await;
                continue;
            }
        };
        let reply = match shared.mind.ask(question).await {
            Ok(value) => Frame::ok(id, &value),
            Err(e) => Frame::err(id, code::INTERNAL, e.to_string()),
        };
        if out.send(reply).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::origin_allowed;

    #[test]
    fn only_local_pages_and_listed_origins_may_connect() {
        let none: [String; 0] = [];
        for ok in [
            "http://localhost:5173",
            "http://127.0.0.1:1420",
            "http://[::1]:8080",
        ] {
            assert!(origin_allowed(ok, &none), "{ok}");
        }
        for bad in [
            "tauri://localhost",
            "https://tauri.localhost",
            "https://evil.example",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example:80",
            "file://",
            "null",
        ] {
            assert!(!origin_allowed(bad, &none), "{bad}");
        }
        assert!(origin_allowed(
            "https://console.example",
            &["https://console.example".to_owned()]
        ));
    }
}
