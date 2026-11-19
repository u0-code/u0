//! The Ghost Protocol, server side.
//!
//! Each connection says hello and is then either a presence or a console,
//! for one character: the one its hello names, or the only one there is.
//! Everything that arrives on it goes to her mind and no other, and a
//! console sees her alone. Before the hello, the one question a client may
//! ask is who the daemon runs, so that it can name one of them.
//!
//! Each character's presences are her [`Body`]: when her mind acts on a
//! channel, the request goes to whichever of her presences registered that
//! channel, and the mind waits for the platform's answer. Two characters
//! may each have a channel of the same name; they are different accounts.
//! The one channel no presence carries is the console's, which the daemon
//! answers itself: see [`crate::operator`].

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::Engine as _;
use futures_util::StreamExt;
use ghost_link::socket::{recv, send};
use ghost_mind::{Act, Acted, Body, ChannelFacts, MindHandle, Question};
use ghost_protocol::{
    Action, Actor, Allows, Avatar, CONSOLE, Capability, Frame, Hello, Inbound, JournalRead,
    ListGhosts, LookRead, MediaFile, MediaRead, PROTOCOL, PlaceRead, PresenceHello, Role,
    StepsRead, Typing, Welcome, code, method,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, broadcast::error::RecvError, mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    http::StatusCode,
};
use tracing::{Instrument as _, debug, info, info_span, warn};

use crate::{
    brains::served::{self, Held},
    cast::{Cast, Character},
    operator, sessions, wire,
};

/// How long a client has to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// How long a presence has to carry out an action.
const ACT_WITHIN: Duration = Duration::from_secs(45);
/// How many steps of a piece of work a console is sent when it does not
/// say.
const STEPS_ASKED: usize = 100;
/// The largest picture handed out as her avatar. Platforms take far
/// smaller ones; anything this big is a mistake in her directory.
const AVATAR_MOST: u64 = 8 << 20;
/// The largest file of hers handed out from `media/`: as large as any
/// image she takes in from a platform, and in base64 still well inside
/// the 16 MB a WebSocket frame may carry.
const MEDIA_MOST: u64 = 8 << 20;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Frame>>>>;

struct Presence {
    /// Which connection this is, so a replaced one cannot unregister its successor.
    id: u64,
    /// Signalled when a newer presence takes this channel.
    displaced: Arc<Notify>,
    out: mpsc::Sender<Frame>,
    pending: Pending,
    me: Option<Actor>,
    can: Vec<Capability>,
    allows: Allows,
}

/// The presences currently connected for one character, keyed by channel.
/// This is her body.
#[derive(Default)]
pub struct Presences {
    channels: Mutex<HashMap<String, Presence>>,
    next_id: AtomicU64,
}

impl Presences {
    /// Tell `mind`, which has just started, about every channel already
    /// connected: after her mind is started again, her presences are still
    /// there and have nothing new to say. The console's channel is always
    /// there, since what she says on it is kept in her journal.
    pub async fn announce(&self, mind: &MindHandle) {
        let _ = mind
            .online(
                CONSOLE,
                None,
                operator::CAN.to_vec(),
                ChannelFacts::console(),
            )
            .await;
        let present: Vec<(String, Option<Actor>, Vec<Capability>, Allows)> =
            self.channels.lock().map_or_else(
                |_| Vec::new(),
                |channels| {
                    channels
                        .iter()
                        .map(|(channel, p)| {
                            (channel.clone(), p.me.clone(), p.can.clone(), p.allows)
                        })
                        .collect()
                },
            );
        for (channel, me, can, allows) in present {
            let can = can.into_iter().map(wire::can).collect();
            let _ = mind
                .online(&channel, me.map(wire::actor), can, wire::facts(allows))
                .await;
        }
    }
}

#[async_trait]
impl Body for Presences {
    async fn act(&self, channel: &str, act: Act) -> Result<Acted, String> {
        if channel == CONSOLE {
            return operator::act(&act);
        }
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
    pub cast: Cast,
    /// The brains its minds are made with, which an operator may change.
    pub brains: Arc<Held>,
    pub token: Option<String>,
    /// Web origins allowed to connect, beyond this machine's own.
    pub origins: Vec<String>,
    /// Where it runs, as every character's status says: the daemon's, not
    /// anything she lived.
    pub core: ghost_protocol::CoreInfo,
}

impl Shared {
    /// Whether a client that gave `token` may go on: it is the daemon's, or
    /// the daemon requires none.
    fn admits(&self, token: Option<&str>) -> bool {
        self.token.is_none() || token == self.token.as_deref()
    }
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

    // Before its hello a client may ask who the daemon runs, since it has
    // to know that to say which of them it is for. It still has to say
    // hello in time, or go.
    let deadline = tokio::time::Instant::now() + HELLO_WITHIN;
    let first = loop {
        let Some(frame) = tokio::time::timeout_at(deadline, recv(&mut stream)).await?? else {
            return Ok(());
        };
        if frame.method.as_deref() != Some(method::LIST) {
            break frame;
        }
        let id = frame.id.unwrap_or(0);
        // Asked with no params at all, it is asked with no token.
        let asked = match frame.params {
            None => Ok(ListGhosts::default()),
            Some(_) => frame.params::<ListGhosts>(),
        };
        let answer = match asked {
            Ok(asked) if shared.admits(asked.token.as_deref()) => {
                Frame::ok(id, &shared.cast.roster().await)
            }
            Ok(_) => Frame::err(id, code::UNAUTHORIZED, "wrong or missing token"),
            Err(e) => Frame::err(id, e.code, e.message),
        };
        let refused = answer.error.is_some();
        send(&mut sink, &answer).await?;
        if refused {
            return Ok(());
        }
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
    if !shared.admits(hello.token.as_deref()) {
        send(
            &mut sink,
            &refuse(code::UNAUTHORIZED, "wrong or missing token"),
        )
        .await?;
        return Ok(());
    }
    let who = match shared.cast.pick(hello.ghost.as_deref()) {
        Ok(who) => who.clone(),
        Err(why) => {
            send(&mut sink, &refuse(code::NO_SUCH_GHOST, &why)).await?;
            return Ok(());
        }
    };

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
            ghost: who.card.clone(),
        },
    );
    let span = info_span!("ghost", id = %who.id);
    match (hello.role, hello.presence) {
        (Role::Presence, Some(presence)) => {
            carry(presence, welcome, id, out, &mut stream, &who, peer)
                .instrument(span)
                .await;
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
            let at = Console {
                owner: operator::owns(shared.token.as_deref(), peer),
                who: hello.who,
            };
            watch(out, &mut stream, &who, &shared, &at)
                .instrument(span)
                .await;
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
    who: &Character,
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
    if channel == CONSOLE {
        let _ = out
            .send(Frame::err(
                hello_id,
                code::INVALID_PARAMS,
                "the console channel is the daemon's own; a presence carries another",
            ))
            .await;
        return;
    }
    let pending: Pending = Pending::default();
    // The newest presence for a channel carries it. Whoever held it before is
    // told and let go: a window left open, or an adapter that was restarted,
    // must not be able to hold a channel against its replacement.
    let id = who.body.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let displaced = Arc::new(Notify::new());
    let Ok(replaced) = who.body.channels.lock().map(|mut channels| {
        channels.insert(
            channel.clone(),
            Presence {
                id,
                displaced: displaced.clone(),
                out: out.clone(),
                pending: pending.clone(),
                me: hello.me.clone(),
                can: hello.can.clone(),
                allows: hello.allows,
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
    let _ = who
        .mind()
        .online(
            &channel,
            hello.me.map(wire::actor),
            can,
            wire::facts(hello.allows),
        )
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
                    who.hear(&channel, wire::incoming(message)).await;
                }
                Err(e) => warn!("{channel}: malformed inbound: {e}"),
            },
            Some(method::TYPING) => {
                if let Ok(typing) = frame.params::<Typing>() {
                    let _ = who.mind().typing(&channel, &typing.place).await;
                }
            }
            Some(method::AVATAR) => {
                if let Some(id) = frame.id {
                    let _ = out.send(avatar(id, &who.dir).await).await;
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
    let held = who.body.channels.lock().is_ok_and(|mut channels| {
        let mine = channels.get(&channel).is_some_and(|p| p.id == id);
        if mine {
            channels.remove(&channel);
        }
        mine
    });
    if held {
        info!("{channel} disconnected");
        let _ = who.mind().offline(&channel).await;
    }
}

/// Who is at a console, as its hello and the daemon's rule say.
struct Console {
    /// It is the daemon's owner's, and whoever is at it an operator of hers.
    owner: bool,
    /// Their name, if the hello gave one.
    who: Option<String>,
}

/// A console: answer its questions, take what its operator says to her,
/// and, if asked, stream her journal. It is for her, but may still ask who
/// else the daemon runs, and about the brains, which are the daemon's.
async fn watch<S>(
    out: mpsc::Sender<Frame>,
    stream: &mut S,
    who: &Character,
    shared: &Shared,
    at: &Console,
) where
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
            Some(method::WORK) => Question::Work,
            Some(method::ATTENTION) => Question::Waiting,
            Some(method::LIST) => {
                if out
                    .send(Frame::ok(id, &shared.cast.roster().await))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            Some(
                method::BRAINS
                | method::BRAIN_SAVE
                | method::BRAIN_REMOVE
                | method::BRAIN_USE
                | method::BRAIN_MODELS
                | method::BRAIN_CHECK,
            ) => {
                let reply =
                    served::answer(id, &frame, at.owner, &shared.brains, &shared.cast).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            // Read from her directory, not asked of her mind: her picture
            // is not something she lived.
            Some(method::AVATAR) => {
                if out.send(avatar(id, &who.dir).await).await.is_err() {
                    break;
                }
                continue;
            }
            // So are the files she keeps: the journal names them, and a
            // console anywhere shows them by that name.
            Some(method::MEDIA) => {
                let reply = match frame.params::<MediaRead>() {
                    Ok(read) => media(id, &who.dir, &read.name).await,
                    Err(e) => Frame::err(id, e.code, e.message),
                };
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(method::SAY) => {
                let reply = operator::say(id, &frame, at.owner, at.who.as_deref(), who).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(method::STOP) => {
                let reply = operator::stop(id, &frame, at.owner, at.who.as_deref(), who).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(method::REWIND) => {
                let reply = operator::rewind(id, &frame, at.owner, at.who.as_deref(), who).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(
                method::SESSIONS
                | method::SESSION_NEW
                | method::SESSION_RENAME
                | method::SESSION_DELETE,
            ) => {
                let reply = sessions::answer(id, &frame, at.owner, at.who.as_deref(), who).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(method::WORK_ANSWER | method::WORK_STOP) => {
                let reply = operator::work(id, &frame, at.owner, at.who.as_deref(), who).await;
                if out.send(reply).await.is_err() {
                    break;
                }
                continue;
            }
            Some(method::WORK_STEPS) => match frame.params::<StepsRead>() {
                Ok(read) => Question::WorkSteps {
                    work: read.work,
                    after: read.after.unwrap_or(0),
                    limit: read.limit.map_or(STEPS_ASKED, |l| l as usize),
                },
                Err(e) => {
                    let _ = out.send(Frame::err(id, e.code, e.message)).await;
                    continue;
                }
            },
            Some(method::LOOK_STEPS) => match frame.params::<LookRead>() {
                Ok(read) => Question::LookSteps {
                    place: read.place,
                    after: read
                        .after
                        .map_or(0, |seq| i64::try_from(seq).unwrap_or(i64::MAX)),
                },
                Err(e) => {
                    let _ = out.send(Frame::err(id, e.code, e.message)).await;
                    continue;
                }
            },
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
                    let mut minds = who.minds();
                    let mut entries = minds.borrow_and_update().watch();
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
                                // Her mind stopped. If it is started again,
                                // her journal goes on, and so does this.
                                Err(RecvError::Closed) => {
                                    if minds.changed().await.is_err() {
                                        break;
                                    }
                                    entries = minds.borrow_and_update().watch();
                                }
                            }
                        }
                    });
                }
                let _ = out.send(Frame::ok(id, &serde_json::json!({}))).await;
                continue;
            }
            other => {
                // Nothing here edits who she is or what she lived. There
                // is no method that could.
                let message = format!("unknown method {other:?}");
                let _ = out
                    .send(Frame::err(id, code::METHOD_NOT_FOUND, message))
                    .await;
                continue;
            }
        };
        let steps = matches!(question, Question::WorkSteps { .. });
        let status = matches!(question, Question::Status);
        let reply = match who.mind().ask(question).await {
            // Her mind has no work by that id.
            Ok(serde_json::Value::Null) if steps => {
                Frame::err(id, code::NO_SUCH_WORK, "she has no work by that id")
            }
            // Where she runs is the daemon's to say, beside what she is.
            Ok(mut value) if status && value.is_object() => {
                value["core"] = serde_json::json!(shared.core);
                Frame::ok(id, &value)
            }
            Ok(value) => Frame::ok(id, &value),
            Err(e) => Frame::err(id, code::INTERNAL, e.to_string()),
        };
        if out.send(reply).await.is_err() {
            break;
        }
    }
}

/// The answer to request `id` for the avatar in the ghost directory `dir`:
/// her picture, or `null` if she has none.
async fn avatar(id: u64, dir: &Path) -> Frame {
    let Some(path) = ghost_core::ghost::avatar(dir) else {
        return Frame::ok(id, &None::<Avatar>);
    };
    let mime = ghost_core::media::mime(&path.to_string_lossy());
    match read_at_most(&path, AVATAR_MOST).await {
        Ok(data) => Frame::ok(
            id,
            &Avatar {
                mime: mime.to_owned(),
                data,
            },
        ),
        Err(e) => {
            warn!("her avatar {} cannot be handed out: {e}", path.display());
            Frame::err(
                id,
                code::INTERNAL,
                format!("her avatar cannot be read: {e}"),
            )
        }
    }
}

/// The answer to request `id` for the file `name` in the `media/` of the
/// ghost directory `dir`. Only a bare name of a plain file there is looked
/// for, as `ghost_core::media::path` finds one: a path, a hidden name or a
/// link out of `media/` is no file she keeps.
async fn media(id: u64, dir: &Path, name: &str) -> Frame {
    let Some(path) = ghost_core::media::path(dir, name) else {
        return Frame::err(
            id,
            code::NO_SUCH_MEDIA,
            format!("she keeps no file called {name:?}"),
        );
    };
    match read_at_most(&path, MEDIA_MOST).await {
        Ok(data) => Frame::ok(
            id,
            &MediaFile {
                mime: ghost_core::media::mime(name).to_owned(),
                data,
            },
        ),
        Err(e) => {
            warn!("{} cannot be handed out: {e}", path.display());
            Frame::err(id, code::INTERNAL, format!("{name} cannot be read: {e}"))
        }
    }
}

/// The file at `path` in base64, unless it is larger than `most` bytes.
async fn read_at_most(path: &Path, most: u64) -> std::io::Result<String> {
    let size = tokio::fs::metadata(path).await?.len();
    if size > most {
        return Err(std::io::Error::other(format!(
            "it is {size} bytes, more than the {most} handed out"
        )));
    }
    let bytes = tokio::fs::read(path).await?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
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
