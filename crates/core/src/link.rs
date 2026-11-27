//! u0 reached from elsewhere: a desk on another machine asks this
//! server what the desk in this process would (each [`Call`]), and is told
//! what it says (each [`Event`]), as JSON over a WebSocket.
//!
//! The server listens on 127.0.0.1, where a tunnel that brings its own
//! TLS (cloudflared, or an `ssh -L`) reaches it; and, where you choose,
//! on a network (`reach`), speaking TLS itself with a certificate the
//! pairing link pins. Plain `ws://` never leaves the machine.
//!
//! The server first says a [`Challenge`], a nonce made for this link; the
//! desk answers with its [`Hello`]: the protocol it speaks, its public key
//! and its signature of the nonce (`key`, `access`), and what it last
//! heard. The server numbers what it says and keeps the last of it
//! ([`Outbox`]), so a desk that drops off (a laptop shut, a network
//! changed) and comes back is told what it missed, in order, and nothing
//! is lost; one that comes back to a server restarted, or after more than
//! is kept, begins again.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc, watch},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Bytes, Message, Utf8Bytes,
        protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
    },
};

use crate::{
    access::{Access, Scope, Turned},
    key::Key,
    proto::{Call, Challenge, Event, Hello, PROTOCOL, Pair, Then, Welcome},
    reach::Way,
    server::Server,
};

/// How much of what the server said is kept for a desk that comes back.
const KEPT: usize = 16 << 20;
/// How often each side shows the other it is there, and how long a desk
/// waits on a silent server before it reaches it again.
const PING: Duration = Duration::from_secs(20);
const QUIET: Duration = Duration::from_secs(60);
/// How long a desk waits to reach a server, and a server for a hello.
const WAIT: Duration = Duration::from_secs(15);

/// How long a connection let up to a WebSocket has to say its hello: a
/// desk says it at once, and one waiting holds a place others need.
const HELLO: Duration = Duration::from_secs(5);

/// The most a connection not yet let in may send in one message: a hello
/// is a few hundred bytes.
const BEFORE: usize = 64 << 10;
/// The most a device let in may send in one message: a prompt with its
/// pictures, a settings file.
const AFTER: usize = 64 << 20;
/// How many connections may wait at once to be let in (or answered, a
/// webhook's): more are closed as they come.
const PENDING: usize = 64;

/// What a server reads from a connection, `before` it is let in, or
/// after: so that no one it does not know can make it hold much.
fn server_config(before: bool) -> WebSocketConfig {
    let most = if before { BEFORE } else { AFTER };
    WebSocketConfig::default()
        .max_message_size(Some(most))
        .max_frame_size(Some(most))
}

/// A record or a file read can be many megabytes: sent as one message,
/// by a server the desk knows.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None)
}

/// What the server said, numbered, the last `KEPT` bytes of it kept.
pub struct Outbox {
    /// What this run of the server is known by.
    pub boot: u64,
    frames: Mutex<Frames>,
    said: watch::Sender<u64>,
    /// Whether a link listens: until then nothing is written out.
    open: AtomicBool,
}

/// A frame as every link is sent it, and as a device that may only read
/// is, where that differs (`Event::for_reader`).
type Frame = (u64, Utf8Bytes, Option<Utf8Bytes>);

#[derive(Default)]
struct Frames {
    kept: VecDeque<Frame>,
    bytes: usize,
    last: u64,
}

impl Default for Outbox {
    fn default() -> Self {
        Self {
            boot: crate::access::number(),
            frames: Mutex::default(),
            said: watch::Sender::new(0),
            open: AtomicBool::new(false),
        }
    }
}

impl Outbox {
    /// Number `event` and keep it, for each desk linked to send on.
    pub fn push(&self, event: &Event) {
        if !self.open.load(Ordering::Relaxed) {
            return;
        }
        let json = serde_json::to_string(event).expect("an event serializes");
        let read =
            (event.for_reader()).map(|e| serde_json::to_string(&e).expect("an event serializes"));
        let mut f = self.frames.lock().expect("outbox");
        f.last += 1;
        let n = f.last;
        let frame = Utf8Bytes::from(format!("[{n},{json}]"));
        let read = read.map(|json| Utf8Bytes::from(format!("[{n},{json}]")));
        f.bytes += frame.len() + read.as_ref().map_or(0, |r| r.len());
        f.kept.push_back((n, frame, read));
        while f.bytes > KEPT
            && let Some((_, old, read)) = f.kept.pop_front()
        {
            f.bytes -= old.len() + read.as_ref().map_or(0, |r| r.len());
        }
        self.said.send_replace(f.last);
    }

    /// What was said after the number `since`, as a device of `scope` is
    /// told it; none where some of it is no longer kept.
    fn after(&self, since: u64, scope: Scope) -> Option<Vec<(u64, Utf8Bytes)>> {
        let f = self.frames.lock().expect("outbox");
        let first = f.kept.front().map_or(f.last + 1, |(n, _, _)| *n);
        (since + 1 >= first && since <= f.last).then(|| {
            (f.kept.iter())
                .filter(|(n, _, _)| *n > since)
                .map(|(n, all, read)| match (scope, read) {
                    (Scope::Read, Some(read)) => (*n, read.clone()),
                    _ => (*n, all.clone()),
                })
                .collect()
        })
    }

    /// Where a desk that last heard `since` begins: after what it heard,
    /// where all after that is kept (resumed); else after all said so far.
    fn resume(&self, since: Option<(u64, u64)>) -> (u64, bool) {
        match since {
            Some((boot, n)) if boot == self.boot && self.after(n, Scope::Read).is_some() => {
                (n, true)
            }
            _ => (self.frames.lock().expect("outbox").last, false),
        }
    }
}

/// Let desks link to `server` through `listener`, for as long as it runs,
/// speaking TLS first where `tls` is given (on a network): each link it
/// let in ends with it.
pub async fn listen(
    server: Arc<Server>,
    listener: TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
) {
    server.outbox().open.store(true, Ordering::Relaxed);
    let pending = Arc::new(tokio::sync::Semaphore::new(PENDING));
    let mut links = tokio::task::JoinSet::new();
    loop {
        let stream = tokio::select! {
            stream = next(&listener) => stream,
            Some(_) = links.join_next() => continue,
        };
        // Held until it is let in, or answered: past `PENDING` waiting,
        // a new one is closed at once.
        let Ok(waits) = pending.clone().try_acquire_owned() else {
            continue;
        };
        let (server, tls) = (server.clone(), tls.clone());
        links.spawn(async move {
            let ended = match tls {
                None => Box::pin(http(server, stream, waits)).await,
                Some(tls) => match tokio::time::timeout(WAIT, tls.accept(stream)).await {
                    Ok(Ok(stream)) => Box::pin(http(server, stream, waits)).await,
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("no TLS in time".into()),
                },
            };
            if let Err(why) = ended {
                tracing::info!("a link ended: {why}");
            }
        });
    }
}

/// The next connection `listener` takes. Where it can take none (u0
/// out of file descriptors, say), it waits a moment before it tries
/// again, rather than trying again at once, for ever.
pub(crate) async fn next(listener: &TcpListener) -> TcpStream {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => return stream,
            Err(e) => {
                tracing::debug!("a connection was not taken: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// The most a request's head may take, its line and headers.
const HEAD: usize = 16 << 10;

/// One connection, as HTTP/1.1 (hyper's): a webhook's call answered, or
/// a desk's WebSocket upgraded from it and served on this same task, so
/// that it ends when listening does.
async fn http<S>(
    server: Arc<Server>,
    stream: S,
    waits: tokio::sync::OwnedSemaphorePermit,
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use hyper::{Request, body::Incoming, upgrade::OnUpgrade};
    let upgrading: Arc<Mutex<Option<OnUpgrade>>> = Arc::default();
    let (to, of) = (upgrading.clone(), server.clone());
    let answer = hyper::service::service_fn(move |req: Request<Incoming>| {
        let (to, server) = (to.clone(), of.clone());
        async move { Ok::<_, std::convert::Infallible>(respond(&server, req, &to).await) }
    });
    hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(WAIT)
        .keep_alive(false)
        .max_buf_size(HEAD)
        .serve_connection(hyper_util::rt::TokioIo::new(stream), answer)
        .with_upgrades()
        .await
        .map_err(|e| e.to_string())?;
    let Some(upgrade) = upgrading.lock().expect("upgrade").take() else {
        return Ok(());
    };
    let io = hyper_util::rt::TokioIo::new(upgrade.await.map_err(|e| e.to_string())?);
    let role = tokio_tungstenite::tungstenite::protocol::Role::Server;
    let ws = WebSocketStream::from_raw_socket(io, role, Some(server_config(true))).await;
    Box::pin(serve(&server, ws, waits)).await
}

type Answer = hyper::Response<http_body_util::Full<Bytes>>;

/// An answer of `status` alone.
fn status(status: hyper::StatusCode) -> Answer {
    let mut answer = Answer::default();
    *answer.status_mut() = status;
    answer
}

/// What `req` asks: a webhook's call; a desk's WebSocket, the upgrade to
/// it left in `upgrade` for the connection to take up; else nothing here.
async fn respond(
    server: &Arc<Server>,
    mut req: hyper::Request<hyper::body::Incoming>,
    upgrade: &Mutex<Option<hyper::upgrade::OnUpgrade>>,
) -> Answer {
    use hyper::{Method, StatusCode};
    use tokio_tungstenite::tungstenite::handshake::server::create_response_with_body;
    if req.method() == Method::POST
        && let Some(token) = req.uri().path().strip_prefix("/hook/")
    {
        let token = token.to_owned();
        return hook(server, &token, req).await;
    }
    // No page in a browser, which says where it comes from (Origin), is
    // let up: a desk, the command and the tools say none.
    if req.headers().contains_key(hyper::header::ORIGIN) {
        return status(StatusCode::FORBIDDEN);
    }
    // The upgrade as tungstenite checks it: a GET, HTTP/1.1, version 13.
    let Ok(answer) = create_response_with_body(&req, Default::default) else {
        return status(StatusCode::NOT_FOUND);
    };
    *upgrade.lock().expect("upgrade") = Some(hyper::upgrade::on(&mut req));
    answer
}

/// Serve the u0 in this machine's home to desks elsewhere, with no
/// window, until stopped: what `u0-server` and `u0-desk --serve` do.
/// It says where it serves as it starts, and at a terminal a pairing link
/// for the first desk.
pub fn serve_here() -> Result<(), String> {
    use std::io::IsTerminal as _;
    let server = Server::new(crate::settings::home(), |_| {});
    server.serve()?;
    server.share_as_kept(true);
    println!("{}", greeting(&server, std::io::stdout().is_terminal()));
    crate::acp::runtime().block_on(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| e.to_string())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
        Ok::<(), String>(())
    })?;
    // What was asked to be written is, before it goes; and the tunnel
    // goes with it.
    server.unshare();
    Server::settle();
    Ok(())
}

/// What `serve_here` says as it starts: where it serves; and, to a person
/// at a `terminal` alone, a pairing link for the first desk and its code.
/// What is not a terminal is a log (launchd's, for the service), which
/// would keep a link that lets anyone who reads it in.
fn greeting(server: &Arc<Server>, terminal: bool) -> String {
    let urls: Vec<String> = (server.ways().into_iter()).map(|(_, w)| w.url()).collect();
    let whereabouts = format!("u0 serves on {}", urls.join(", "));
    if !terminal {
        return format!(
            "{whereabouts}\nPair a desk with it from a window on this Mac, under Settings › Devices."
        );
    }
    let link = server
        .offer(crate::access::Grant::default(), None)
        .expect("it listens");
    // Light on dark, as a terminal mostly is: a camera reads it as dark
    // on light.
    let qr = qr(&link)
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .build();
    format!("{whereabouts}\nPair a desk with it, once, in the next ten minutes: {link}\n{qr}")
}

/// Why a device that asked was not paired back with.
const NOT_BOTH: &str =
    "Its pairing link was not made to pair both ways: it pairs with this Mac one way only";

/// The most of a webhook call's body taken: more is refused.
const HOOK_BODY: usize = 1 << 20;

/// A call to the webhook task of `token` (`Schedule::Hook`), its body read
/// whole within [`WAIT`] and [`HOOK_BODY`]: 202 when the task runs, 204
/// when the call is not one it runs for, 200 for a delivery heard before,
/// 401 for one not signed as it must be, 404 where there is no task, 413
/// for a body too long, 408 for one too slow.
async fn hook(
    server: &Arc<Server>,
    token: &str,
    req: hyper::Request<hyper::body::Incoming>,
) -> Answer {
    use http_body_util::BodyExt as _;
    use hyper::{StatusCode, body::Body as _};

    use crate::tasks::{Call, Hooked};
    let (head, body) = req.into_parts();
    // Said too long by its Content-Length, it is refused before any of it
    // is read.
    if body.size_hint().lower() > HOOK_BODY as u64 {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let read = http_body_util::Limited::new(body, HOOK_BODY).collect();
    let body = match tokio::time::timeout(WAIT, read).await {
        Ok(Ok(body)) => body.to_bytes(),
        Ok(Err(_)) => return status(StatusCode::PAYLOAD_TOO_LARGE),
        Err(_) => return status(StatusCode::REQUEST_TIMEOUT),
    };
    let headers = (head.headers.iter())
        .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect();
    let call = Call {
        headers,
        body: &body,
    };
    status(match server.hook(token, &call) {
        Hooked::Ran => StatusCode::ACCEPTED,
        Hooked::Skipped => StatusCode::NO_CONTENT,
        Hooked::Again => StatusCode::OK,
        Hooked::Unsigned => StatusCode::UNAUTHORIZED,
        Hooked::Nowhere => StatusCode::NOT_FOUND,
    })
}

type Socket<S> = WebSocketStream<S>;

/// Close `ws`, saying `why` to a desk that is not let in.
async fn refuse<S>(ws: Socket<S>, why: &str) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    refuse_as(ws, CloseCode::Policy, why).await
}

/// Close `ws` with `code`, saying `why`.
async fn refuse_as<S>(mut ws: Socket<S>, code: CloseCode, why: &str) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // A close frame's reason is at most 123 bytes.
    let mut end = why.len().min(123);
    while !why.is_char_boundary(end) {
        end -= 1;
    }
    let frame = CloseFrame {
        code,
        reason: why[..end].to_owned().into(),
    };
    let _ = ws.close(Some(frame)).await;
    Err(why.to_owned())
}

/// What `server` says to `device`, let in with `scope`, as it is: to be
/// filled in with what the link resumes.
fn welcome(server: &Server, device: &str, scope: Scope) -> Welcome {
    let kept = server.access().device(device);
    Welcome {
        boot: server.outbox().boot,
        name: server_name(server.home()),
        version: crate::update::this_build(),
        home: server.home().to_path_buf(),
        resumed: false,
        said: 0,
        settings: match scope {
            Scope::Full => server.settings(),
            Scope::Read => server.settings().map(|s| Box::new(s.for_reader())),
        },
        back: None,
        scope,
        label: kept.as_ref().map(|d| d.label.clone()),
        until: kept.and_then(|d| d.until),
    }
}

/// Carry bytes between `ws` (as binary messages) and `near`, until either
/// ends, or `lasts` does: then `ws` is closed, saying why.
async fn carry<S, N>(
    mut ws: Socket<S>,
    mut near: N,
    lasts: impl std::future::Future<Output = String>,
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    N: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut buf = vec![0u8; 64 << 10];
    tokio::pin!(lasts);
    loop {
        tokio::select! {
            why = &mut lasts => return refuse(ws, &why).await,
            heard = ws.next() => match heard {
                Some(Ok(Message::Binary(bytes))) => {
                    near.write_all(&bytes).await.map_err(|e| e.to_string())?;
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Err(e)) => return Err(e.to_string()),
                Some(Ok(_)) => {}
            },
            read = near.read(&mut buf) => match read {
                Ok(0) => {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
                Ok(n) => {
                    let bytes = Bytes::copy_from_slice(&buf[..n]);
                    ws.send(Message::Binary(bytes)).await.map_err(|e| e.to_string())?;
                }
                Err(e) => return Err(e.to_string()),
            },
        }
    }
}

/// A port of a server's machine brought here: what connects to `port` on
/// 127.0.0.1 here is carried to it over a link of its own, each signed in
/// as a desk's. It stops when let go.
pub struct Forward {
    /// The port there, and where it is here.
    pub there: u16,
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// How many connections a port brought here carries at once.
const CARRIED: usize = 64;

/// Bring the port `there` of the server `way` here, coming in with the key
/// kept as `kept`: at the same port where it is free, else from
/// `first_port` upward where given, else at a port of the system's choosing.
pub fn forward(way: Way, kept: String, there: u16, first_port: u16) -> Result<Forward, String> {
    let rt = crate::acp::runtime();
    let bind_range = || {
        (first_port > 0)
            .then(|| {
                (first_port..=first_port.saturating_add(63))
                    .find_map(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok())
            })
            .flatten()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrInUse))
    };
    let bound = std::net::TcpListener::bind(("127.0.0.1", there))
        .or_else(|_| bind_range())
        .or_else(|_| std::net::TcpListener::bind(("127.0.0.1", 0)))
        .and_then(|l| l.set_nonblocking(true).map(|()| l))
        .map_err(|e| format!("No port here to bring {there} to: {e}"))?;
    let port = bound.local_addr().map_err(|e| e.to_string())?.port();
    let listener = {
        let _in_runtime = rt.enter();
        TcpListener::from_std(bound).map_err(|e| e.to_string())?
    };
    let task = rt.spawn(async move {
        let mut carried = tokio::task::JoinSet::new();
        loop {
            let near = tokio::select! {
                near = next(&listener) => near,
                Some(_) = carried.join_next() => continue,
            };
            // Past `CARRIED` carried at once, one more is closed at once.
            if carried.len() >= CARRIED {
                continue;
            }
            let (way, key) = (way.clone(), Key::read(&kept));
            carried.spawn(async move {
                let Ok(key) = key else { return };
                match dial(&way, hello(&key, None, None, Then::Forward(there))).await {
                    Ok((ws, _)) => {
                        if let Err(why) = carry(ws, near, std::future::pending()).await {
                            tracing::info!("port {there} carried no more: {why}");
                        }
                    }
                    Err(Dial::Failed(why) | Dial::Refused(why) | Dial::Waiting(why)) => {
                        tracing::info!("port {there} was not reached: {why}");
                    }
                }
            });
        }
    });
    Ok(Forward { there, port, task })
}

/// Ask the server `way`, coming in with `key`, `call` as a desk would, and
/// hand what it says to `heard` until it has what it waits for.
pub async fn ask<T>(
    way: &Way,
    key: &Key,
    call: Call,
    mut heard: impl FnMut(Event) -> Option<Result<T, String>>,
) -> Result<T, String> {
    let (mut ws, _) = match dial(way, hello(key, None, None, Then::Desk)).await {
        Ok(reached) => reached,
        Err(Dial::Failed(why) | Dial::Refused(why)) => return Err(why),
        Err(Dial::Waiting(name)) => return Err(waiting(&name)),
    };
    let text = serde_json::to_string(&call).expect("a call serializes");
    ws.send(Message::text(text))
        .await
        .map_err(|e| e.to_string())?;
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => {
                if let Ok((_, event)) = serde_json::from_str::<(u64, Event)>(&t)
                    && let Some(done) = heard(event)
                {
                    let _ = ws.close(None).await;
                    return done;
                }
            }
            Some(Ok(Message::Close(f))) => {
                return Err(f.map_or_else(
                    || "The server closed the link".into(),
                    |f| f.reason.to_string(),
                ));
            }
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("The server closed the link".into()),
            Some(Ok(_)) => {}
        }
    }
}

/// Reach the tools of the server `way` over a link of their own, coming
/// in with `key`, and carry MCP between them and `here` until either ends.
pub async fn tools<H>(way: &Way, key: &Key, here: H) -> Result<(), String>
where
    H: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match dial(way, hello(key, None, None, Then::Tools)).await {
        Ok((ws, _)) => carry(ws, here, std::future::pending()).await,
        Err(Dial::Failed(why) | Dial::Refused(why)) => Err(why),
        Err(Dial::Waiting(name)) => Err(waiting(&name)),
    }
}

/// Until the device known by `device`, let in with `scope`, may no longer
/// come in as it did: unpaired, its access ended, or its scope changed
/// (it comes in again as it now may). Why it is cut off. Every kind of
/// link a device makes (a desk's, a port carried, the tools) lasts this
/// long, and no longer.
async fn lasts(server: &Server, device: &str, scope: Scope) -> String {
    let mut access = server.access().changes();
    let mut ping = tokio::time::interval(PING);
    loop {
        tokio::select! {
            _ = access.changed() => {}
            _ = ping.tick() => {}
        }
        match server.access().lets_in(device) {
            Ok(now) if now == scope => {}
            Ok(_) => return "What this device may do changed: it comes in again".into(),
            Err(why) => return cut(why),
        }
    }
}

/// Why a device let in is cut off: its access ended, as it says; else it
/// was unpaired.
fn cut(why: Turned) -> String {
    match why {
        Turned::Ended(why) => why,
        _ => "This device was unpaired".into(),
    }
}

/// A WebSocket upgraded from a connection to where u0 listens.
type Upgraded = Socket<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>;

/// Marks a desk link as active on `server` until dropped.
struct Linked<'a>(&'a Server, String);

impl Drop for Linked<'_> {
    fn drop(&mut self) {
        self.0.set_desk_linked(&self.1, false);
    }
}

/// One desk's link: its hello, then what it asks and what it is told.
async fn serve(
    server: &Arc<Server>,
    mut ws: Upgraded,
    waits: tokio::sync::OwnedSemaphorePermit,
) -> Result<(), String> {
    let nonce = crate::access::random::<32>();
    let challenge = Challenge {
        nonce: nonce.clone(),
    };
    let text = serde_json::to_string(&challenge).expect("a challenge serializes");
    ws.send(Message::text(text))
        .await
        .map_err(|e| e.to_string())?;
    let hello = match tokio::time::timeout(HELLO, ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<Hello>(&t).ok(),
        _ => None,
    };
    let Some(hello) = hello else {
        return refuse(ws, "Say hello first").await;
    };
    if hello.protocol != PROTOCOL {
        let why = format!(
            "This u0 speaks protocol {PROTOCOL} and yours {}: update the older",
            hello.protocol
        );
        return refuse(ws, &why).await;
    }
    let admitted = server.access().admit(&hello, &nonce).await;
    let _linked = match (&admitted, hello.then) {
        (Ok(a), Then::Desk) => {
            server.set_desk_linked(&a.key, true);
            Some(Linked(server, a.key.clone()))
        }
        _ => None,
    };
    // Its device is now paired, or last here now, or begins to wait:
    // Settings › Devices shows it so, wherever it is open. One turned
    // away, or that waits still, changed nothing, and is told to no one.
    let newly_waits = hello.pair.is_some() && matches!(admitted, Err(Turned::Waiting));
    if admitted.is_ok() || newly_waits {
        server.share(crate::share::Share::Look);
    }
    let crate::access::Admitted {
        key: device,
        scope,
        back: may_back,
    } = match admitted {
        Ok(admitted) => admitted,
        Err(Turned::Away(why) | Turned::Ended(why)) => return refuse(ws, &why).await,
        // Told by its close's code, with the server's name, so that a
        // desk pairing keeps it and comes back until it is let in.
        Err(Turned::Waiting) => {
            if let Some(pair) = &hello.pair {
                server.say(Event::Notice(Ok(format!(
                    "{} asks to come in: let it in under Settings › Devices",
                    pair.device
                ))));
            }
            let name = server_name(server.home());
            return refuse_as(ws, CloseCode::Again, &name).await;
        }
    };
    // Let in: it no longer waits, and may send what a device sends. It
    // has sent nothing since its hello: it waits for its welcome.
    drop(waits);
    let role = tokio_tungstenite::tungstenite::protocol::Role::Server;
    let mut ws =
        WebSocketStream::from_raw_socket(ws.into_inner(), role, Some(server_config(false))).await;
    if hello.then == Then::Tools {
        let welcome = welcome(server, &device, scope);
        let text = serde_json::to_string(&welcome).expect("a welcome serializes");
        ws.send(Message::text(text))
            .await
            .map_err(|e| e.to_string())?;
        let (near, far) = tokio::io::duplex(64 << 10);
        let tooled = server.clone();
        tokio::spawn(async move { tooled.tools_for(scope, far).await });
        return carry(ws, near, lasts(server, &device, scope)).await;
    }
    if let Then::Forward(port) = hello.then {
        if scope != Scope::Full {
            return refuse(ws, "This device may only watch here: no port is forwarded").await;
        }
        let near = match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(near) => near,
            Err(e) => return refuse(ws, &format!("Nothing answers on port {port}: {e}")).await,
        };
        let welcome = welcome(server, &device, scope);
        let text = serde_json::to_string(&welcome).expect("a welcome serializes");
        ws.send(Message::text(text))
            .await
            .map_err(|e| e.to_string())?;
        return carry(ws, near, lasts(server, &device, scope)).await;
    }
    // Asked as it paired by a code made to pair both ways: pair back
    // with it. Any other is told no, and this Mac pairs with nothing.
    let back = match hello.pair.and_then(|p| p.back) {
        Some(back) if may_back => Some(server.pair_back(&back).await),
        Some(_) => Some(Err(NOT_BOTH.to_owned())),
        None => None,
    };
    let outbox = server.outbox();
    let mut said = outbox.said.subscribe();
    // What was missed goes first.
    said.mark_changed();
    let ends = lasts(server, &device, scope);
    tokio::pin!(ends);
    let (mut at, resumed) = outbox.resume(hello.since);
    let welcome = Welcome {
        resumed,
        said: at,
        back,
        ..welcome(server, &device, scope)
    };
    let text = serde_json::to_string(&welcome).expect("a welcome serializes");
    let mut ping = tokio::time::interval(PING);
    let mut next = Some(Message::text(text));
    // A device that may only read is told once, on its link alone, that
    // what it asked to change was not done.
    let mut told = false;
    loop {
        if let Some(message) = next.take() {
            ws.send(message).await.map_err(|e| e.to_string())?;
        }
        tokio::select! {
            heard = ws.next() => match heard {
                Some(Ok(Message::Text(t))) => match serde_json::from_str::<Call>(&t) {
                    Ok(call) if Access::allows(scope, &call, server.home()) => {
                        let offers = matches!(call, Call::Share(crate::share::Share::Link(..)));
                        server.handle(call);
                        // The link made is said on this link alone: every
                        // other device hears only that there is one.
                        if offers && let Some((link, made, grant)) = server.offered() {
                            let offered = Event::Offered { link, made, grant };
                            let frame = serde_json::to_string(&(at, offered)).expect("an event serializes");
                            next = Some(Message::text(frame));
                        }
                    }
                    Ok(_) => {
                        tracing::info!("a call refused: its device may only read");
                        if !std::mem::replace(&mut told, true) {
                            let refused = Event::Notice(Err((
                                "This Mac may only watch here".into(),
                                "What it asked to change was not done: its access reads only.".into(),
                            )));
                            let frame = serde_json::to_string(&(at, refused)).expect("an event serializes");
                            next = Some(Message::text(frame));
                        }
                    }
                    Err(e) => tracing::warn!("a call that does not read: {e}"),
                },
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Err(e)) => return Err(e.to_string()),
                Some(Ok(_)) => {}
            },
            _ = said.changed() => {
                let Some(frames) = outbox.after(at, scope) else {
                    return refuse(ws, "Too much was missed; come back afresh").await;
                };
                for (n, frame) in frames {
                    ws.feed(Message::Text(frame)).await.map_err(|e| e.to_string())?;
                    at = n;
                }
                ws.flush().await.map_err(|e| e.to_string())?;
            }
            why = &mut ends => return refuse(ws, &why).await,
            _ = ping.tick() => next = Some(Message::Ping(Bytes::new())),
        }
    }
}

/// This machine's name, for a desk to call its server by.
pub fn host_name() -> String {
    sysinfo::System::host_name()
        .ok()
        .map(|name| name.trim_end_matches(".local").to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this machine".into())
}

/// What the u0 in `home` calls itself to a desk: its machine's name,
/// and for a home other than `~/.u0` that home's folder too, so that a
/// desk holding two u0s of one machine tells them apart.
fn server_name(home: &std::path::Path) -> String {
    let default = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".u0"));
    match home.file_name() {
        Some(folder) if default.as_deref() != Some(home) => {
            format!("{} · {}", host_name(), folder.to_string_lossy())
        }
        _ => host_name(),
    }
}

/// How a link to a server stands, as the desk is told.
#[derive(Debug)]
pub enum State {
    /// It is reached, first or again: its welcome, which says whether all
    /// said meanwhile follows (`resumed`), or the server restarted, or
    /// too much was said, and what the desk had of it is to be read again.
    Up(Box<Welcome>),
    /// It could not be reached, or dropped, why: it is reached again,
    /// waiting longer each time it is not.
    Lost(String),
    /// The server will not let this desk in, why: it is reached again
    /// only when asked to (`Link::again`).
    Refused(String),
}

/// A desk's link to a server: what it asks goes in order, kept while the
/// server is out of reach and sent once it is back.
pub struct Link {
    calls: mpsc::UnboundedSender<Out>,
    /// Woken, it reaches the server now rather than when its wait is
    /// over.
    again: Arc<Notify>,
}

/// What goes out on a link: a call, or a word back once all before it
/// has gone.
enum Out {
    Call(Box<Call>),
    Settle(std::sync::mpsc::Sender<()>),
}

impl Link {
    pub fn send(&self, call: Call) {
        let _ = self.calls.send(Out::Call(Box::new(call)));
    }

    /// Reach the server again now: one out of reach, or one that refused.
    pub fn again(&self) {
        self.again.notify_one();
    }

    /// Wait, a few seconds at most, until all asked has gone: as u0
    /// quits.
    pub fn settle(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.calls.send(Out::Settle(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

impl Drop for Link {
    /// Let go of, the link ends, even while it waits to reach the server.
    fn drop(&mut self) {
        self.again.notify_one();
    }
}

enum Dial {
    /// Not reached; tried again.
    Failed(String),
    /// Reached, and refused; not tried again.
    Refused(String),
    /// Paired, and waiting to be let in by the server of this name; tried
    /// again.
    Waiting(String),
}

type Client = Socket<MaybeTlsStream<TcpStream>>;

/// Reach the server at the address `reach` gives (`ws://127.0.0.1:…`, or
/// `wss://` through a tunnel), signing with the key it gives, from now
/// until the link is let go; `reach` is asked off the caller's thread
/// (the Keychain may ask you first), and where it cannot say, the link is
/// refused, why. Each
/// time it is reached its welcome goes to `state`, and each time it
/// cannot be, or drops, why; it is reached again after a second, then
/// twice as long each time, up to half a minute. What the server says
/// goes to `said`.
pub fn connect(
    reach: impl FnOnce() -> Result<(Way, Key), String> + Send + 'static,
    said: impl Fn(Event) + Send + Sync + 'static,
    state: impl Fn(State) + Send + Sync + 'static,
) -> Link {
    const FIRST: Duration = Duration::from_secs(1);
    let (tx, mut calls) = mpsc::unbounded_channel();
    let again = Arc::new(Notify::new());
    let woken = again.clone();
    crate::acp::runtime().spawn(async move {
        let (way, key) = match tokio::task::spawn_blocking(reach).await {
            Ok(Ok(reached)) => reached,
            Ok(Err(why)) => return state(State::Refused(why)),
            Err(e) => return state(State::Refused(e.to_string())),
        };
        let (mut since, mut wait, mut lost) = (None, FIRST, false);
        while !calls.is_closed() {
            match dial(&way, hello(&key, None, since, Then::Desk)).await {
                Ok((mut ws, welcome)) => {
                    since = Some((welcome.boot, welcome.said));
                    wait = FIRST;
                    state(State::Up(Box::new(welcome)));
                    let Some(why) = pump(&mut ws, &mut calls, &said, &mut since).await else {
                        return;
                    };
                    state(State::Lost(why));
                    lost = true;
                }
                Err(Dial::Refused(why)) => {
                    state(State::Refused(why));
                    woken.notified().await;
                    continue;
                }
                Err(Dial::Failed(why)) => {
                    if !lost {
                        state(State::Lost(why));
                    }
                    lost = true;
                }
                Err(Dial::Waiting(name)) => {
                    if !lost {
                        state(State::Lost(waiting(&name)));
                    }
                    lost = true;
                }
            }
            tokio::select! {
                () = tokio::time::sleep(wait) => wait = (wait * 2).min(Duration::from_secs(30)),
                () = woken.notified() => wait = FIRST,
            }
        }
    });
    Link { calls: tx, again }
}

/// Pair with the server `way` by its one-time `code`, coming in with
/// `key` as this machine; `back`, a pairing link for it to pair back
/// with. Its welcome, once it has let the key in; the link is let go.
pub async fn pair(
    way: &Way,
    code: String,
    key: &Key,
    back: Option<String>,
) -> Result<Paired, String> {
    let pair = Pair {
        code,
        device: host_name(),
        back,
    };
    match dial(way, hello(key, Some(pair), None, Then::Desk)).await {
        Ok((mut ws, welcome)) => {
            let _ = ws.close(None).await;
            Ok(Paired {
                name: welcome.name,
                back: welcome.back,
                waiting: false,
            })
        }
        // Kept, to come in once it is let in; it pairs back no sooner.
        Err(Dial::Waiting(name)) => Ok(Paired {
            name,
            back: None,
            waiting: true,
        }),
        Err(Dial::Failed(why) | Dial::Refused(why)) => Err(why),
    }
}

/// What came of pairing: the server's name; how it paired back, where it
/// was asked to (the public half of its key for this Mac, or why not);
/// and whether it waits for you there to let this Mac in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub name: String,
    pub back: Option<Result<String, String>>,
    pub waiting: bool,
}

/// What a desk is told while the server `name` has not let it in.
fn waiting(name: &str) -> String {
    format!("Waiting for {name} to let this Mac in, under Settings › Devices there")
}

/// What a desk says to a challenge: it is `key`, pairing with `pair`, and
/// last heard `since`.
fn hello(
    key: &Key,
    pair: Option<Pair>,
    since: Option<(u64, u64)>,
    then: Then,
) -> impl FnOnce(&Challenge) -> Hello {
    move |c| Hello {
        protocol: PROTOCOL,
        key: key.public(),
        signature: key.sign(&c.nonce),
        pair,
        since,
        then,
        version: Some(crate::update::this_build()),
    }
}

/// Reach the server `way` and say `hello` to its challenge: the socket,
/// and its welcome. Where it pins a certificate, that one alone is
/// trusted.
async fn dial(
    way: &Way,
    hello: impl FnOnce(&Challenge) -> Hello,
) -> Result<(Client, Welcome), Dial> {
    let tls = (way.pin.as_deref())
        .map(|pin| tokio_tungstenite::Connector::Rustls(Arc::new(crate::reach::pinned(pin))));
    let reached =
        tokio_tungstenite::connect_async_tls_with_config(way.url(), Some(config()), false, tls);
    let (mut ws, _) = match tokio::time::timeout(WAIT, reached).await {
        Ok(Ok(reached)) => reached,
        Ok(Err(e)) => return Err(Dial::Failed(e.to_string())),
        Err(_) => return Err(Dial::Failed("It did not answer".into())),
    };
    let challenge = answer::<Challenge>(&mut ws).await?;
    let text = serde_json::to_string(&hello(&challenge)).expect("a hello serializes");
    if let Err(e) = ws.send(Message::text(text)).await {
        return Err(Dial::Failed(e.to_string()));
    }
    let welcome = answer(&mut ws).await?;
    Ok((ws, welcome))
}

/// What the server says next on `ws`, or why it says nothing more.
async fn answer<T: serde::de::DeserializeOwned>(ws: &mut Client) -> Result<T, Dial> {
    match tokio::time::timeout(WAIT, ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str(&t).map_err(|e| {
            Dial::Refused(format!(
                "The server answered in another form ({e}): update the older u0"
            ))
        }),
        Ok(Some(Ok(Message::Close(Some(f))))) if f.code == CloseCode::Policy => {
            Err(Dial::Refused(f.reason.to_string()))
        }
        Ok(Some(Ok(Message::Close(Some(f))))) if f.code == CloseCode::Again => {
            Err(Dial::Waiting(f.reason.to_string()))
        }
        Ok(Some(Err(e))) => Err(Dial::Failed(e.to_string())),
        _ => Err(Dial::Failed("It did not answer".into())),
    }
}

/// Send what is asked and hand on what is said, until the link drops
/// (why) or the desk lets go of it (none). `since` follows what was heard.
async fn pump(
    ws: &mut Client,
    calls: &mut mpsc::UnboundedReceiver<Out>,
    said: &impl Fn(Event),
    since: &mut Option<(u64, u64)>,
) -> Option<String> {
    loop {
        tokio::select! {
            call = calls.recv() => {
                let call = match call {
                    Some(Out::Call(call)) => *call,
                    Some(Out::Settle(done)) => {
                        let _ = done.send(());
                        continue;
                    }
                    None => {
                        let _ = ws.close(None).await;
                        return None;
                    }
                };
                let text = serde_json::to_string(&call).expect("a call serializes");
                if let Err(e) = ws.send(Message::text(text)).await {
                    return Some(e.to_string());
                }
            }
            heard = tokio::time::timeout(QUIET, ws.next()) => match heard {
                Err(_) => return Some("The server went quiet".into()),
                Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str::<(u64, Event)>(&t) {
                    Ok((n, event)) => {
                        if let Some((_, at)) = since {
                            *at = n;
                        }
                        said(event);
                    }
                    Err(e) => tracing::warn!("an event that does not read: {e}"),
                },
                Ok(Some(Ok(Message::Close(f)))) => {
                    return Some(f.map_or_else(|| "The server closed the link".into(), |f| f.reason.to_string()));
                }
                Ok(Some(Err(e))) => return Some(e.to_string()),
                Ok(None) => return Some("The server closed the link".into()),
                Ok(Some(Ok(_))) => {}
            },
        }
    }
}

/// The QR code of the pairing link `link`, for a camera to read off a
/// screen.
pub fn qr(link: &str) -> qrcode::QrCode {
    qrcode::QrCode::new(link).expect("a pairing link fits a QR code")
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc as std_mpsc;

    use super::*;

    /// A server in a home of its own, listening on a port of loopback's
    /// choosing; and that port.
    fn server(home: &std::path::Path) -> (Arc<Server>, u16) {
        let server = Server::new(home.to_path_buf(), |_| {});
        let rt = crate::acp::runtime();
        let listener = rt.block_on(TcpListener::bind(("127.0.0.1", 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        rt.spawn(listen(server.clone(), listener, None));
        (server, port)
    }

    #[test]
    fn only_a_whole_websocket_upgrade_is_taken_up() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let (_server, port) = server(home.path());
        let first = |head: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            write!(
                s,
                "{head}Host: x\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
            )
            .unwrap();
            let mut said = [0; 12];
            let _ = s.read_exact(&mut said);
            String::from_utf8_lossy(&said).into_owned()
        };
        let whole = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n";
        assert_eq!(first(&format!("GET / HTTP/1.1\r\n{whole}")), "HTTP/1.1 101");
        // A page in a browser, which says where it comes from, is not.
        let page = format!("GET / HTTP/1.1\r\n{whole}Origin: https://evil.example\r\n");
        assert_eq!(first(&page), "HTTP/1.1 403");
        for head in [
            format!("PUT / HTTP/1.1\r\n{whole}"),
            "GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n".into(),
            "GET / HTTP/1.1\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\n".into(),
            "GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 8\r\n".into(),
        ] {
            assert_eq!(first(&head), "HTTP/1.1 404", "{head}");
        }
    }

    #[test]
    fn a_webhook_call_runs_its_task_and_one_to_no_task_is_not_found() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let task = crate::tasks::Task {
            id: 1,
            title: "On a push".into(),
            prompt: "Look at {{body}}".into(),
            agent: "nowhere".into(),
            model: None,
            mode: None,
            project: home.path().to_path_buf(),
            schedule: crate::tasks::Schedule::Hook(crate::tasks::Hook {
                token: "tok".into(),
                signed: false,
                only: Vec::new(),
            }),
            paused: false,
            same: false,
            worktree: false,
            if_running: crate::tasks::Overlap::Skip,
            timeout_mins: None,
            notify: true,
            workflow: None,
            session: None,
            next: None,
            last: None,
            runs: 0,
        };
        let tasks = crate::tasks::Tasks { tasks: vec![task] };
        std::fs::write(
            home.path().join("tasks.json"),
            serde_json::to_vec(&tasks).unwrap(),
        )
        .unwrap();
        let (_server, port) = server(home.path());
        let call = |token: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = "{\"ref\":\"main\"}";
            write!(
                s,
                "POST /hook/{token} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut answer = String::new();
            s.read_to_string(&mut answer).unwrap();
            answer
        };
        assert!(call("tok").starts_with("HTTP/1.1 202"));
        assert!(call("other").starts_with("HTTP/1.1 404"));
        let said = |request: &[u8]| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = s.write_all(request);
            let mut answer = String::new();
            let _ = s.read_to_string(&mut answer);
            answer
        };
        // A body sent in chunks is read whole; one too long is refused.
        let chunked = said(
            b"POST /hook/other HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        );
        assert!(chunked.starts_with("HTTP/1.1 404"), "{chunked}");
        let long = format!(
            "POST /hook/tok HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
            HOOK_BODY + 1
        );
        assert!(said(long.as_bytes()).starts_with("HTTP/1.1 413"));
        // A head with no end is cut off, not read for ever.
        let mut endless = b"POST /hook/tok HTTP/1.1\r\nX: ".to_vec();
        endless.resize(HEAD * 2, b'y');
        assert!(!said(&endless).starts_with("HTTP/1.1 202"));
        // The run is kept as the writer gets to it.
        let ran = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            crate::tasks::load(home.path()).is_ok_and(|t| t.tasks[0].runs == 1)
        });
        assert!(ran, "the task ran once");
    }

    #[test]
    fn a_signed_webhook_runs_for_its_secret_its_filters_and_each_delivery_once() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let hook = crate::tasks::Hook {
            token: "tok".into(),
            signed: true,
            only: vec![crate::tasks::Only {
                at: "header.x-github-event".into(),
                is: vec!["push".into()],
            }],
        };
        crate::secrets::set(&crate::tasks::Hook::secret("tok"), Some("s3cret")).unwrap();
        let task = serde_json::json!({"id": 1, "title": "On a push", "prompt": "Look",
            "agent": "nowhere", "project": home.path(), "schedule": {"hook": hook}});
        std::fs::write(
            home.path().join("tasks.json"),
            serde_json::to_vec(&serde_json::json!({"tasks": [task]})).unwrap(),
        )
        .unwrap();
        let (server, port) = server(home.path());
        // What is sent, signed: changed once, below.
        let body = std::cell::Cell::new("{\"ref\":\"main\"}");
        let sign = |secret: &str| {
            let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
            let tag = ring::hmac::sign(&key, body.get().as_bytes());
            format!("sha256={}", data_encoding::HEXLOWER.encode(tag.as_ref()))
        };
        let call = |token: &str, signature: &str, event: &str, delivery: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                s,
                "POST /hook/{token} HTTP/1.1\r\nHost: x\r\nX-Hub-Signature-256: {signature}\r\nX-GitHub-Event: {event}\r\nX-GitHub-Delivery: {delivery}\r\nContent-Length: {}\r\n\r\n{}",
                body.get().len(),
                body.get()
            )
            .unwrap();
            let mut answer = String::new();
            s.read_to_string(&mut answer).unwrap();
            answer[..12].to_owned()
        };
        assert_eq!(call("tok", "sha256=00", "push", "d0"), "HTTP/1.1 401");
        assert_eq!(call("tok", &sign("other"), "push", "d0"), "HTTP/1.1 401");
        assert_eq!(call("tok", &sign("s3cret"), "issues", "d1"), "HTTP/1.1 204");
        assert_eq!(call("tok", &sign("s3cret"), "push", "d2"), "HTTP/1.1 202");
        assert_eq!(call("tok", &sign("s3cret"), "push", "d2"), "HTTP/1.1 200");
        // Rotated, the old address runs it no more; the secret goes along.
        server.handle(Call::RotateHook(1));
        // Read as the writer gets to it.
        let token = (0..100)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(20));
                match &crate::tasks::load(home.path()).ok()?.tasks[0].schedule {
                    crate::tasks::Schedule::Hook(h) if h.token != "tok" => Some(h.token.clone()),
                    _ => None,
                }
            })
            .expect("a new address");
        assert_eq!(call("tok", &sign("s3cret"), "push", "d3"), "HTTP/1.1 404");
        // The body sent before is a replay at any address of its task.
        assert_eq!(call(&token, &sign("s3cret"), "push", "d4"), "HTTP/1.1 200");
        body.set("{\"ref\":\"next\"}");
        assert_eq!(call(&token, &sign("s3cret"), "push", "d4b"), "HTTP/1.1 202");
        // Signed and sent again, with a delivery of another name, and its
        // signature written another way: a replay each.
        assert_eq!(call(&token, &sign("s3cret"), "push", "d5"), "HTTP/1.1 200");
        let upper = sign("s3cret").to_uppercase().replace("SHA256=", "sha256=");
        assert_eq!(call(&token, &upper, "push", "d6"), "HTTP/1.1 200");
        let twice = sign("s3cret").replace("sha256=", "sha256=sha256=");
        assert_eq!(call(&token, &twice, "push", "d7"), "HTTP/1.1 200");
        assert_eq!(
            crate::secrets::get(&crate::tasks::Hook::secret("tok")),
            Ok(None)
        );
    }

    type Desk = (
        Link,
        Welcome,
        std_mpsc::Receiver<Event>,
        std_mpsc::Receiver<State>,
    );

    /// The way to the server at `port` on this machine.
    fn here(port: u16) -> Way {
        Way {
            host: format!("127.0.0.1:{port}"),
            pin: None,
        }
    }

    /// A desk linked to the server at `port` with `key`, paired first
    /// with `server` where there is one: its link and welcome, what it is
    /// told and how its link stands.
    fn desk(port: u16, key: Key, server: Option<&Server>) -> Result<Desk, String> {
        if let Some(server) = server {
            let way = here(port);
            let code = server.access().code(crate::access::Grant::default());
            crate::acp::runtime().block_on(super::pair(&way, code, &key, None))?;
        }
        let (events, heard) = std_mpsc::channel();
        let (states, stood) = std_mpsc::channel();
        let (events, states) = (Mutex::new(events), Mutex::new(states));
        let way = here(port);
        let link = connect(
            move || Ok((way, key)),
            move |e| events.lock().unwrap().send(e).unwrap(),
            move |s| {
                let _ = states.lock().unwrap().send(s);
            },
        );
        let welcome = reached(&stood)?;
        Ok((link, welcome, heard, stood))
    }

    /// The welcome of a link first reached, or why it was not.
    fn reached(stood: &std_mpsc::Receiver<State>) -> Result<Welcome, String> {
        match told(stood) {
            State::Up(welcome) => Ok(*welcome),
            State::Lost(why) | State::Refused(why) => Err(why),
        }
    }

    fn code(server: &Server) -> Pair {
        Pair {
            code: server.access().code(crate::access::Grant::default()),
            device: "laptop".into(),
            back: None,
        }
    }

    /// A key kept, as a desk keeps it, to be read again for each link.
    fn kept() -> String {
        Key::generate().kept().to_owned()
    }

    fn key(kept: &str) -> Key {
        Key::read(kept).unwrap()
    }

    fn told<T>(rx: &std_mpsc::Receiver<T>) -> T {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("told in time")
    }

    #[test]
    fn a_stranger_turned_away_writes_nothing_and_is_told_to_no_desk() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let (_mine, _, heard, _) = desk(port, Key::generate(), Some(&server)).unwrap();
        let devices = home.path().join("devices.json");
        let written = || {
            std::fs::metadata(&devices)
                .and_then(|m| m.modified())
                .unwrap()
        };
        let before = written();
        // What its coming in said, said and gone.
        std::thread::sleep(Duration::from_millis(300));
        while heard.try_recv().is_ok() {}
        for _ in 0..3 {
            let why = desk(port, Key::generate(), None).map(|_| ()).unwrap_err();
            assert!(why.contains("not paired"), "{why}");
        }
        assert_eq!(written(), before, "devices.json is not written");
        let shared = std::iter::from_fn(|| heard.recv_timeout(Duration::from_millis(300)).ok())
            .filter(|e| matches!(e, Event::Shared(_)))
            .count();
        assert_eq!(shared, 0);
    }

    #[test]
    fn a_device_that_waits_or_whose_access_ended_coming_again_writes_and_tells_nothing() {
        use crate::access::Grant;
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let (_mine, _, heard, _) = desk(port, Key::generate(), Some(&server)).unwrap();
        let (waits, ended) = (kept(), kept());
        let ask = Grant {
            ask: true,
            ..Grant::default()
        };
        pair_as(&server, port, &waits, ask);
        let none_left = Grant {
            days: Some(0),
            ..Grant::default()
        };
        let code = server.access().code(none_left);
        let paired = crate::acp::runtime().block_on(pair(&here(port), code, &key(&ended), None));
        assert!(paired.unwrap_err().contains("ended"));
        let devices = home.path().join("devices.json");
        let written = || std::fs::read_to_string(&devices).unwrap();
        // What their pairing said, said and gone; and a second gone, so
        // that a `seen` written now would differ.
        std::thread::sleep(Duration::from_millis(1100));
        while heard.try_recv().is_ok() {}
        let before = written();
        for _ in 0..3 {
            let why = desk(port, key(&waits), None).map(|_| ()).unwrap_err();
            assert!(why.contains("Waiting for"), "{why}");
            let why = desk(port, key(&ended), None).map(|_| ()).unwrap_err();
            assert!(why.contains("ended"), "{why}");
        }
        assert_eq!(written(), before, "devices.json is not written");
        let shared = std::iter::from_fn(|| heard.recv_timeout(Duration::from_millis(300)).ok())
            .filter(|e| matches!(e, Event::Shared(_)))
            .count();
        assert_eq!(shared, 0);
    }

    #[test]
    fn a_desk_pairs_once_then_comes_in_by_its_signature_and_is_answered() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let mine = kept();
        let (link, welcome, heard, _) = desk(port, key(&mine), Some(&server)).unwrap();
        assert_eq!(welcome.boot, server.outbox().boot);
        link.send(Call::Watch);
        assert!(matches!(told(&heard), Event::Index(Ok(_))));
        assert!(desk(port, key(&mine), None).is_ok());
        let why = desk(port, Key::generate(), None).err().unwrap();
        assert!(why.contains("not paired"), "{why}");
    }

    /// Say to the server at `port` what `hello` makes of its challenge.
    fn say(port: u16, hello: impl FnOnce(&Challenge) -> Hello) -> Result<Welcome, String> {
        let way = here(port);
        match crate::acp::runtime().block_on(dial(&way, hello)) {
            Ok((_, welcome)) => Ok(welcome),
            Err(Dial::Refused(why) | Dial::Failed(why) | Dial::Waiting(why)) => Err(why),
        }
    }

    #[test]
    fn a_signature_heard_on_one_link_does_not_open_another() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let mine = kept();
        drop(desk(port, key(&mine), Some(&server)).unwrap());
        let mut heard = None;
        say(port, |c| {
            let said = hello(&key(&mine), None, None, Then::Desk)(c);
            heard = Some(said.clone());
            said
        })
        .unwrap();
        let replayed = say(port, |_| heard.unwrap()).unwrap_err();
        assert!(replayed.contains("does not verify"), "{replayed}");
    }

    #[test]
    fn a_desk_that_speaks_another_protocol_is_told_to_update() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let refused = say(port, |c| Hello {
            protocol: PROTOCOL + 1,
            ..hello(&Key::generate(), Some(code(&server)), None, Then::Desk)(c)
        });
        assert!(refused.unwrap_err().contains("update"));
    }

    /// A way to `port` that can be cut: each connection through it then
    /// drops.
    fn breakable(port: u16) -> (u16, watch::Sender<u32>) {
        let rt = crate::acp::runtime();
        let front = rt.block_on(TcpListener::bind(("127.0.0.1", 0))).unwrap();
        let through = front.local_addr().unwrap().port();
        let cut = watch::Sender::new(0);
        let cuts = cut.clone();
        rt.spawn(async move {
            while let Ok((mut a, _)) = front.accept().await {
                let mut cut = cuts.subscribe();
                tokio::spawn(async move {
                    let mut b = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut a, &mut b) => {}
                        _ = cut.changed() => {}
                    }
                });
            }
        });
        (through, cut)
    }

    #[test]
    fn a_desk_that_drops_off_is_told_on_coming_back_what_was_said_meanwhile() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let (through, cut) = breakable(port);
        let (_link, _, heard, stood) = desk(through, Key::generate(), Some(&server)).unwrap();
        cut.send_modify(|n| *n += 1);
        assert!(matches!(told(&stood), State::Lost(_)));
        // Said while the desk is away: for another desk, say.
        server.handle(Call::Stash(vec!["later".into()]));
        server.handle(Call::Watch);
        assert!(matches!(told(&stood), State::Up(w) if w.resumed));
        let Event::Index(Ok(index)) = told(&heard) else {
            panic!("the index, missed")
        };
        assert_eq!(index.stashed, ["later"]);
    }

    /// Pair `kept`'s key with the server at `port` by a code `server`
    /// makes for `grant`.
    fn pair_as(server: &Server, port: u16, kept: &str, grant: crate::access::Grant) -> Paired {
        let code = server.access().code(grant);
        let way = here(port);
        crate::acp::runtime()
            .block_on(pair(&way, code, &key(kept), None))
            .unwrap()
    }

    #[test]
    fn a_device_that_may_only_read_watches_and_what_it_asks_to_change_is_not_done() {
        use crate::access::{Grant, Scope};
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let mine = kept();
        let read = Grant {
            scope: Scope::Read,
            ..Grant::default()
        };
        pair_as(&server, port, &mine, read);
        let (link, welcome, heard, _) = desk(port, key(&mine), None).unwrap();
        assert_eq!(welcome.scope, Scope::Read);
        link.send(Call::Stash(vec!["mine".into()]));
        link.send(Call::Stash(vec!["again".into()]));
        link.send(Call::Watch);
        // Told once that it was not done; then the index, unchanged.
        let Event::Notice(Err((said, _))) = told(&heard) else {
            panic!("told it may only watch")
        };
        assert!(said.contains("only watch"), "{said}");
        let Event::Index(Ok(index)) = told(&heard) else {
            panic!("the index, read")
        };
        assert!(index.stashed.is_empty(), "nothing it asked was kept");
    }

    #[test]
    fn a_device_that_must_be_let_in_waits_until_it_is_and_one_turned_away_is_gone() {
        use crate::access::Grant;
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let ask = Grant {
            ask: true,
            ..Grant::default()
        };
        let mine = kept();
        let paired = pair_as(&server, port, &mine, ask);
        assert!(paired.waiting && paired.name == server_name(home.path()));
        let why = desk(port, key(&mine), None).err().unwrap();
        assert!(why.contains("Waiting for"), "{why}");
        server.access().approve(&key(&mine).public());
        assert!(desk(port, key(&mine), None).is_ok(), "let in");
        let theirs = kept();
        pair_as(&server, port, &theirs, ask);
        server.access().revoke(&key(&theirs).public());
        let why = desk(port, key(&theirs), None).err().unwrap();
        assert!(why.contains("not paired"), "{why}");
    }

    #[test]
    fn over_a_network_a_desk_trusts_the_certificate_its_link_pinned_and_no_other() {
        let home = tempfile::tempdir().unwrap();
        let (server, _) = server(home.path());
        let rt = crate::acp::runtime();
        // Where it listens on a network, as on 127.0.0.1 for the test.
        let listener = rt.block_on(TcpListener::bind(("127.0.0.1", 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tls, pin) = crate::reach::acceptor().unwrap();
        rt.spawn(listen(server.clone(), listener, Some(tls)));
        let way = |pin: Option<&str>| Way {
            host: format!("127.0.0.1:{port}"),
            pin: pin.map(str::to_owned),
        };
        let mine = kept();
        let code = server.access().code(crate::access::Grant::default());
        let paired = rt.block_on(pair(&way(Some(&pin)), code, &key(&mine), None));
        assert!(paired.is_ok(), "{paired:?}");
        let other = server.access().code(crate::access::Grant::default());
        let forged = rt.block_on(pair(
            &way(Some("not-its-pin")),
            other.clone(),
            &key(&mine),
            None,
        ));
        assert!(forged.unwrap_err().contains("certificate"));
        let plain = rt.block_on(pair(&way(None), other.clone(), &key(&mine), None));
        assert!(plain.is_err(), "no plain WebSocket where it speaks TLS");
        assert!(
            server.access().live(&other).is_some(),
            "the code was not spent"
        );
    }

    #[test]
    fn a_port_brought_here_carries_so_many_at_once_and_closes_one_more_at_once() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        // A dev server there: each connection answers what is sent, once.
        let dev = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let there = dev.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut c, _)) = dev.accept() {
                std::thread::spawn(move || {
                    let mut asked = [0u8; 5];
                    if c.read_exact(&mut asked).is_ok() {
                        let _ = c.write_all(b"hello from there");
                    }
                });
            }
        });
        let mine = kept();
        pair_as(&server, port, &mine, crate::access::Grant::default());
        let forwarded = forward(here(port), mine, there, 0).unwrap();
        let connect = || {
            let c = std::net::TcpStream::connect(("127.0.0.1", forwarded.port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c
        };
        let mut held: Vec<_> = (0..CARRIED).map(|_| connect()).collect();
        std::thread::sleep(Duration::from_millis(500));
        let began = std::time::Instant::now();
        let mut said = Vec::new();
        assert!(connect().read_to_end(&mut said).is_ok() && said.is_empty());
        assert!(began.elapsed() < Duration::from_secs(2), "closed at once");
        // One carried is answered still, and ends; then one more is carried.
        let mut first = held.remove(0);
        first.write_all(b"hello").unwrap();
        let mut answer = String::new();
        let _ = first.read_to_string(&mut answer);
        assert_eq!(answer, "hello from there");
        std::thread::sleep(Duration::from_millis(300));
        let mut next = connect();
        next.write_all(b"hello").unwrap();
        let mut answer = String::new();
        let _ = next.read_to_string(&mut answer);
        assert_eq!(answer, "hello from there");
    }

    #[test]
    fn a_dev_server_on_the_paired_machine_is_opened_here_and_a_watcher_gets_no_port() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        // A dev server there: it answers whatever is sent, once.
        let dev = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let there = dev.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut c, _)) = dev.accept() {
                let mut asked = [0u8; 5];
                if c.read_exact(&mut asked).is_ok() {
                    let _ = c.write_all(b"hello from there");
                }
            }
        });
        let ask = |forwarded: &Forward| {
            let mut c = std::net::TcpStream::connect(("127.0.0.1", forwarded.port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.write_all(b"hello").unwrap();
            let mut said = String::new();
            let _ = c.read_to_string(&mut said);
            said
        };
        let mine = kept();
        pair_as(&server, port, &mine, crate::access::Grant::default());
        let forwarded = forward(here(port), mine, there, 41873).unwrap();
        assert_ne!(forwarded.port, there, "taken here, so another");
        assert!(
            (41873..=41936).contains(&forwarded.port),
            "bound in configured range: {}",
            forwarded.port
        );
        assert_eq!(ask(&forwarded), "hello from there");
        let watcher = kept();
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        pair_as(&server, port, &watcher, read);
        let refused = forward(here(port), watcher, there, 0).unwrap();
        assert_eq!(ask(&refused), "", "nothing is carried for a watcher");
    }

    #[test]
    fn an_agent_on_a_paired_device_reaches_the_tools_and_acts_only_with_full_scope() {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let meta = serde_json::json!({"agent": "a", "id": "s1", "project": home.path(),
            "folder": home.path(), "title": "Build", "begun": 5, "updated": 7});
        server.handle(Call::Keep(vec![serde_json::from_value(meta).unwrap()]));
        // What an agent there is told, sending to s1, by a device of `grant`.
        let send = |grant: crate::access::Grant| {
            let kept = kept();
            pair_as(&server, port, &kept, grant);
            crate::acp::runtime().block_on(async move {
                let (mine, theirs) = tokio::io::duplex(1 << 16);
                let way = here(port);
                tokio::spawn(async move { tools(&way, &key(&kept), theirs).await });
                let (read, mut write) = tokio::io::split(mine);
                let mut lines = tokio::io::BufReader::new(read).lines();
                let say = [
                    serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                        "protocolVersion": "2025-06-18", "capabilities": {},
                        "clientInfo": {"name": "stand-in", "version": "1"}}}),
                    serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                    serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                        "name": "send_to_session",
                        "arguments": {"agent": "a", "session": "s1", "prompt": "Go"}}}),
                ];
                for line in say {
                    write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
                }
                loop {
                    let line = lines.next_line().await.unwrap().unwrap();
                    let said: serde_json::Value = serde_json::from_str(&line).unwrap();
                    if said["id"] == 2 {
                        break said["result"]["content"][0]["text"].as_str().unwrap().to_owned();
                    }
                }
            })
        };
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        assert!(send(read).contains("may only read"));
        // Of full scope, it is asked of you in the session (here no agent
        // runs it to ask, so it is not allowed).
        assert_eq!(
            send(crate::access::Grant::default()),
            "You did not allow it"
        );
    }

    #[test]
    fn a_terminal_says_a_prompt_over_a_link_and_is_told_the_session_and_its_answer() {
        let home = tempfile::tempdir().unwrap();
        // An agent that opens `t1` and answers each prompt in it.
        let agent = home.path().join("agent.sh");
        let script = r#"id() { printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }
while read line; do case "$line" in
*'"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$(id)" ;;
*'"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"t1"}}\n' "$(id)" ;;
*'"session/prompt"'*) printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"t1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Found it."}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)" ;;
esac; done"#;
        std::fs::write(&agent, script).unwrap();
        std::fs::write(
            home.path().join("registry.json"),
            r#"{"agents":[{"id":"a","name":"A","version":"1","distribution":{}}]}"#,
        )
        .unwrap();
        let settings = serde_json::json!({"enabled": ["a"], "folders": [home.path()],
            "commands": {"a": format!("/bin/sh {}", agent.display())}});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let (server, port) = server(home.path());
        // Each its own tag: a turn before goes on, and is told every desk.
        let say = |tag: u64, grant: crate::access::Grant, wait: bool| {
            let mine = kept();
            pair_as(&server, port, &mine, grant);
            let call = Call::Say {
                tag,
                agent: "a".into(),
                session: None,
                project: home.path().to_path_buf(),
                prompt: "Why are the tests slow?".into(),
            };
            crate::acp::runtime().block_on(ask(&here(port), &key(&mine), call, |e| match e {
                Event::Turned { tag: t, .. } if t != tag => None,
                Event::Turned {
                    session: Err(why), ..
                } => Some(Err(why)),
                Event::Turned {
                    session: Ok(id),
                    answer,
                    ..
                } => match answer {
                    Some(answer) if wait => Some(Ok(answer)),
                    _ if wait => None,
                    _ => Some(Ok(id.0.to_string())),
                },
                Event::Notice(Err((what, _))) => Some(Err(what)),
                _ => None,
            }))
        };
        assert_eq!(
            say(1, crate::access::Grant::default(), false),
            Ok("t1".into())
        );
        assert_eq!(
            say(2, crate::access::Grant::default(), true),
            Ok("Found it.".into())
        );
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        let refused = say(3, read, true).unwrap_err();
        assert!(refused.contains("only watch"), "{refused}");
    }

    #[test]
    fn a_port_carried_and_the_tools_are_cut_off_when_their_device_is_revoked_or_narrowed() {
        use std::io::{Read as _, Write as _};
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        // A dev server there that holds each connection open.
        let dev = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let there = dev.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok((c, _)) = dev.accept() {
                held.push(c);
            }
        });
        let mine = kept();
        pair_as(&server, port, &mine, crate::access::Grant::default());
        let public = key(&mine).public();
        let forwarded = forward(here(port), mine.clone(), there, 0).unwrap();
        let mut c = std::net::TcpStream::connect(("127.0.0.1", forwarded.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        c.write_all(b"hello").unwrap();
        // The tools, over a link of their own.
        let (mcp, theirs) = tokio::io::duplex(1 << 16);
        let way = here(port);
        let tools_key = key(&mine);
        let tools =
            crate::acp::runtime().spawn(async move { tools(&way, &tools_key, theirs).await });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!tools.is_finished(), "the tools link stands");
        // Narrowed to read only: the tools link it opened as full ends.
        server
            .access()
            .change(|k| k.devices[0].scope = crate::access::Scope::Read);
        let ended = crate::acp::runtime()
            .block_on(async { tokio::time::timeout(Duration::from_secs(10), tools).await });
        assert!(ended.is_ok(), "the tools link was cut");
        // Revoked: the connection carried is closed.
        server.access().revoke(&public);
        let mut rest = Vec::new();
        let read = c.read_to_end(&mut rest);
        assert!(
            read.is_ok(),
            "the port carried was closed, not left open: {read:?}"
        );
        drop(mcp);
    }

    #[test]
    fn one_not_let_in_can_send_little_and_only_so_many_wait_at_once() {
        use std::io::Read as _;
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let rt = crate::acp::runtime();
        // A hello far larger than any is refused before it is read whole.
        let huge = rt.block_on(async {
            let url = format!("ws://127.0.0.1:{port}");
            let (mut ws, _) =
                tokio_tungstenite::connect_async_with_config(&url, Some(config()), false)
                    .await
                    .unwrap();
            let _challenge = ws.next().await;
            let _ = ws.send(Message::text("x".repeat(BEFORE * 4))).await;
            match tokio::time::timeout(WAIT, ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => Some(t.to_string()),
                _ => None,
            }
        });
        assert_eq!(huge, None, "nothing but a close or an error answers it");
        // So many connections waiting to be let in; one more is closed.
        let held: Vec<std::net::TcpStream> = (0..PENDING)
            .map(|_| std::net::TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect();
        std::thread::sleep(Duration::from_millis(300));
        let mut more = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        more.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut said = Vec::new();
        assert!(
            matches!(more.read_to_end(&mut said), Ok(0)),
            "closed at once"
        );
        drop(held);
        std::thread::sleep(Duration::from_millis(300));
        // Once they go, a desk comes in as ever, and may send much.
        let (link, _, heard, _) = desk(port, Key::generate(), Some(&server)).unwrap();
        link.send(Call::Stash(vec!["y".repeat(BEFORE * 4)]));
        link.send(Call::Watch);
        assert!(matches!(told(&heard), Event::Index(Ok(i)) if i.stashed[0].len() == BEFORE * 4));
    }

    #[test]
    fn a_device_whose_access_ended_is_turned_away_and_cut_off() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let mine = kept();
        pair_as(&server, port, &mine, crate::access::Grant::default());
        let (_link, _, _, stood) = desk(port, key(&mine), None).unwrap();
        server.access().change(|k| k.devices[0].until = Some(1));
        assert!(matches!(told(&stood), State::Lost(_)));
        let why = desk(port, key(&mine), None).err().unwrap();
        assert!(why.contains("access ended"), "{why}");
    }

    #[test]
    fn a_device_unpaired_is_cut_off_and_not_let_back_in() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let (_kept, ..) = desk(port, Key::generate(), Some(&server)).unwrap();
        let (_link, _, _, stood) = desk(port, Key::generate(), Some(&server)).unwrap();

        let devices = server.access().kept().unwrap().devices;
        assert_eq!(devices.len(), 2);
        server.access().revoke(&devices[1].key);
        assert!(matches!(told(&stood), State::Lost(why) if why == "This device was unpaired"));
        assert!(matches!(told(&stood), State::Refused(why) if why.contains("not paired")));
        assert_eq!(server.access().kept().unwrap().devices.len(), 1);
    }

    #[test]
    fn a_server_out_of_reach_is_said_lost_once_and_reached_once_it_listens() {
        let home = tempfile::tempdir().unwrap();
        let server = Server::new(home.path().to_path_buf(), |_| {});
        let rt = crate::acp::runtime();
        let listener = rt.block_on(TcpListener::bind(("127.0.0.1", 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        let listening = rt.spawn(listen(server.clone(), listener, None));
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let code = server.access().code(crate::access::Grant::default());
        rt.block_on(pair(&here(port), code, &key, None)).unwrap();
        // It stops listening, and the port is free again.
        listening.abort();
        let bind = || rt.block_on(TcpListener::bind(("127.0.0.1", port)));
        while bind().is_err() {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (states, stood) = std_mpsc::channel();
        let states = Mutex::new(states);
        let (way, key) = (here(port), Key::read(&kept).unwrap());
        let link = connect(
            move || Ok((way, key)),
            |_| {},
            move |s| {
                let _ = states.lock().unwrap().send(s);
            },
        );
        assert!(matches!(told(&stood), State::Lost(_)));
        rt.spawn(listen(server.clone(), bind().unwrap(), None));
        link.again();
        assert!(matches!(told(&stood), State::Up(w) if !w.resumed));
    }

    /// Link with the key kept for the server `name` in `home`'s servers.
    fn open(home: &std::path::Path, name: &str) -> Result<Welcome, String> {
        let servers = crate::servers::Servers::load(home).unwrap();
        let server = servers.servers.iter().find(|s| s.name == name).unwrap();
        let server = server.clone();
        let (states, stood) = std_mpsc::channel();
        let states = Mutex::new(states);
        let _link = connect(
            move || server.reach(),
            |_| {},
            move |s| {
                let _ = states.lock().unwrap().send(s);
            },
        );
        reached(&stood)
    }

    #[test]
    fn paired_both_ways_each_opens_the_other_by_its_signature_until_revoked() {
        let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let ((a, port_a), (b, port_b)) = (server(home_a.path()), server(home_b.path()));
        let back = crate::reach::pairing_link(
            &here(port_b),
            &b.access().code(crate::access::Grant::default()),
        );
        let both = crate::access::Grant {
            both: true,
            ..crate::access::Grant::default()
        };
        let Paired { name, back, .. } = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                here(port_a),
                a.access().code(both),
                Key::generate(),
                Some(back),
            ))
            .unwrap();
        // Each knows the other's key, and keeps its own for the other.
        let a_for_b = back.unwrap().unwrap();
        let (on_a, on_b) = (a.access().kept().unwrap(), b.access().kept().unwrap());
        assert_eq!(on_b.devices[0].key, a_for_b);
        let mine = crate::servers::Servers::load(home_b.path()).unwrap();
        assert_eq!(on_a.devices[0].key, mine.servers[0].key);
        let theirs = crate::servers::Servers::load(home_a.path()).unwrap();
        assert_eq!(theirs.servers[0].key, a_for_b);
        assert!(mine.control && theirs.control);
        assert_ne!(
            name, theirs.servers[0].name,
            "two u0s of one machine, told apart"
        );
        assert!(open(home_b.path(), &name).is_ok(), "B opens A");
        assert!(
            open(home_a.path(), &theirs.servers[0].name).is_ok(),
            "A opens B"
        );
        // A lets B go; B still lets A in.
        a.access().revoke(&on_a.devices[0].key);
        let cut = open(home_b.path(), &name).unwrap_err();
        assert!(cut.contains("not paired"), "{cut}");
        assert!(
            open(home_a.path(), &theirs.servers[0].name).is_ok(),
            "A still opens B"
        );
    }

    #[test]
    fn a_server_whose_words_go_to_a_log_offers_no_pairing_link() {
        let home = tempfile::tempdir().unwrap();
        let (server, _) = sharing(home.path());
        server.serve().unwrap();
        let logged = greeting(&server, false);
        assert!(logged.contains("u0 serves on ws://127.0.0.1:"), "{logged}");
        assert!(!logged.contains("u0://"), "{logged}");
        assert!(server.offered().is_none());
        assert!(server.access().kept().is_ok_and(|k| k.devices.is_empty()));
        let shown = greeting(&server, true);
        let (link, ..) = server.offered().unwrap();
        assert!(shown.contains(&link), "{shown}");
    }

    #[test]
    fn only_a_link_made_to_pair_both_ways_and_for_everything_pairs_back() {
        let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let ((a, port_a), (b, port_b)) = (server(home_a.path()), server(home_b.path()));
        let read_both = crate::access::Grant {
            scope: crate::access::Scope::Read,
            both: true,
            ..crate::access::Grant::default()
        };
        for grant in [crate::access::Grant::default(), read_both] {
            let back = crate::reach::pairing_link(
                &here(port_b),
                &b.access().code(crate::access::Grant::default()),
            );
            let Paired { back, .. } = crate::acp::runtime()
                .block_on(crate::servers::pair(
                    home_b.path(),
                    here(port_a),
                    a.access().code(grant),
                    Key::generate(),
                    Some(back),
                ))
                .unwrap();
            assert_eq!(back, Some(Err(NOT_BOTH.to_owned())), "{grant:?}");
            // A pairs with nothing, and controls nothing more.
            let theirs = crate::servers::Servers::load(home_a.path()).unwrap();
            assert!(theirs.servers.is_empty() && !theirs.control, "{grant:?}");
            assert!(b.access().kept().unwrap().devices.is_empty());
        }
    }

    #[test]
    fn a_way_back_that_does_not_reach_leaves_the_pairing_one_way() {
        let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, port_a) = server(home_a.path());
        let nowhere = crate::reach::pairing_link(&here(1), "c0de");
        let Paired { name, back, .. } = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                here(port_a),
                a.access().code(crate::access::Grant {
                    both: true,
                    ..crate::access::Grant::default()
                }),
                Key::generate(),
                Some(nowhere),
            ))
            .unwrap();
        assert!(back.unwrap().is_err());
        assert!(open(home_b.path(), &name).is_ok());
        let theirs = crate::servers::Servers::load(home_a.path()).unwrap();
        assert!(theirs.servers.is_empty());
    }

    /// A server in a home of its own that shares as asked, on a port of
    /// loopback's choosing; and how it says sharing stands.
    fn sharing(home: &std::path::Path) -> (Arc<Server>, std_mpsc::Receiver<crate::share::Shared>) {
        let (tx, rx) = std_mpsc::channel();
        let tx = Mutex::new(tx);
        let server = Server::new(home.to_path_buf(), move |e| {
            if let Event::Shared(s) = e {
                let _ = tx.lock().unwrap().send(*s);
            }
        });
        server.access().change(|k| k.port = 0);
        (server, rx)
    }

    #[test]
    fn turned_off_a_server_cuts_its_links_listens_no_more_and_offers_no_link() {
        use crate::share::Share;
        let home = tempfile::tempdir().unwrap();
        let (server, shared) = sharing(home.path());
        server.share(Share::Serve(true));
        server.share(Share::Link(crate::access::Grant::default(), None));
        let on = std::iter::from_fn(|| shared.try_recv().ok())
            .last()
            .unwrap();
        let (Some(Ok(port)), Some(_)) = (on.serving, on.offer) else {
            panic!("listening, with a link")
        };
        let (offered, ..) = server.offered().unwrap();
        let (way, code) = crate::reach::pairing(&offered).unwrap();
        assert_eq!(way, here(port));
        let key = Key::generate();
        let kept = key.kept().to_owned();
        crate::acp::runtime()
            .block_on(pair(&way, code, &key, None))
            .unwrap();
        let (_link, _, _heard, stood) = desk(port, Key::read(&kept).unwrap(), None).unwrap();
        let spare = server.access().code(crate::access::Grant::default());
        server.share(Share::Serve(false));
        let off = std::iter::from_fn(|| shared.try_recv().ok())
            .last()
            .unwrap();
        assert_eq!((off.serving, off.offer), (None, None));
        assert!(matches!(told(&stood), State::Lost(_)));
        assert!(desk(port, Key::read(&kept).unwrap(), None).is_err());
        assert!(
            server.access().live(&spare).is_none(),
            "no code made before lets a key in"
        );
    }

    #[test]
    fn a_pairing_link_is_told_to_the_desk_that_made_it_and_to_no_other_device() {
        let home = tempfile::tempdir().unwrap();
        let (server, shared) = sharing(home.path());
        server.share(crate::share::Share::Serve(true));
        let Some(Ok(port)) = told(&shared).serving else {
            panic!("listening")
        };
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        let watcher = kept();
        pair_as(&server, port, &watcher, read);
        let (_watching, _, heard, _) = desk(port, key(&watcher), None).unwrap();
        let (link, _, mine, _) = desk(port, Key::generate(), Some(&server)).unwrap();
        link.send(Call::Share(crate::share::Share::Link(
            crate::access::Grant::default(),
            None,
        )));
        let offered = loop {
            if let Event::Offered { link, .. } = told(&mine) {
                break link;
            }
        };
        let (_, code) = crate::reach::pairing(&offered).unwrap();
        assert!(server.access().live(&code).is_some());
        // Whatever the watcher heard (that a link was made, sharing as it
        // stands) holds no code to pair with.
        let mut said = Vec::new();
        while let Ok(e) = heard.recv_timeout(Duration::from_millis(500)) {
            said.push(serde_json::to_string(&e).unwrap());
        }
        assert!(
            said.iter().any(|e| e.contains("offer")),
            "it heard of the link"
        );
        assert!(
            said.iter().all(|e| !e.contains(&code)),
            "no code reached it"
        );
    }

    #[test]
    fn a_device_that_may_only_read_is_never_told_a_webhooks_address() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        let watcher = kept();
        pair_as(&server, port, &watcher, read);
        let (_watching, _, theirs, _) = desk(port, key(&watcher), None).unwrap();
        let (_owning, _, mine, _) = desk(port, Key::generate(), Some(&server)).unwrap();
        let task: crate::tasks::Task = serde_json::from_value(serde_json::json!({
            "id": 0, "title": "On a push", "prompt": "Look", "agent": "a",
            "project": home.path(),
            "schedule": {"hook": {"token": "s3cr3t-t0ken", "signed": false}}}))
        .unwrap();
        server.handle(Call::SetTask(task));
        let tasks = |rx: &std_mpsc::Receiver<Event>| loop {
            if let Event::Tasks(Ok(tasks)) = told(rx) {
                break serde_json::to_string(&tasks).unwrap();
            }
        };
        assert!(tasks(&mine).contains("s3cr3t-t0ken"));
        let seen = tasks(&theirs);
        assert!(
            seen.contains("On a push") && !seen.contains("s3cr3t-t0ken"),
            "{seen}"
        );
        assert!(crate::tasks::Hook::new().signed, "a new webhook is signed");
    }

    #[test]
    fn a_device_that_may_only_read_is_told_no_agents_variable_nor_command_line() {
        let home = tempfile::tempdir().unwrap();
        let settings = serde_json::json!({"env": {"a": {"API_KEY": "sk-live-1"}},
            "commands": {"a": "/bin/agent --key sk-live-2"}});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let (server, port) = server(home.path());
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        let watcher = kept();
        pair_as(&server, port, &watcher, read);
        let (_watching, theirs, ..) = desk(port, key(&watcher), None).unwrap();
        let (_owning, mine, ..) = desk(port, Key::generate(), Some(&server)).unwrap();
        let said = |w: &Welcome| serde_json::to_string(&w.settings).unwrap();
        assert!(said(&mine).contains("sk-live-1") && said(&mine).contains("sk-live-2"));
        let told = said(&theirs);
        assert!(!told.contains("sk-live"), "{told}");
        assert_eq!(
            theirs.settings.as_ref().unwrap().env["a"]["API_KEY"],
            None,
            "named, as if secret"
        );
    }

    #[test]
    fn the_way_back_is_this_macs_port_for_a_server_here_and_its_tunnel_for_one_elsewhere() {
        let home = tempfile::tempdir().unwrap();
        let (server, shared) = sharing(home.path());
        let mine = server.back_link(&here(4317)).unwrap();
        let Some(Ok(port)) = told(&shared).serving else {
            panic!("listening")
        };
        let (way, code) = crate::reach::pairing(&mine).unwrap();
        assert_eq!(way, here(port));
        assert!(server.access().live(&code).is_some());
        let why = server
            .back_link(&Way {
                host: "calm-river-1.trycloudflare.com".into(),
                pin: None,
            })
            .unwrap_err();
        assert!(why.contains("tunnel"), "{why}");
    }

    #[test]
    fn a_desk_back_after_a_restart_or_more_than_is_kept_begins_afresh() {
        let outbox = Outbox::default();
        outbox.open.store(true, Ordering::Relaxed);
        let big = Event::Tidied("x".repeat(KEPT / 3));
        for _ in 0..3 {
            outbox.push(&big);
        }
        assert_eq!(outbox.resume(Some((outbox.boot, 2))), (2, true));
        assert_eq!(outbox.resume(Some((outbox.boot + 1, 2))), (3, false));
        outbox.push(&big);
        assert_eq!(outbox.resume(Some((outbox.boot, 0))), (4, false));
    }

    #[test]
    fn welcome_reports_the_devices_label_and_expiry_and_shared_tracks_linked_desks() {
        use crate::{
            access::{Grant, Scope},
            share::Share,
        };
        let home = tempfile::tempdir().unwrap();
        let (server, shared) = sharing(home.path());
        server.share(Share::Serve(true));
        let Some(Ok(port)) = told(&shared).serving else {
            panic!("listening")
        };
        server.share(Share::Link(
            Grant {
                scope: Scope::Read,
                days: Some(7),
                ..Grant::default()
            },
            None,
        ));
        let _ = told(&shared);
        assert!(server.offered().is_some());
        server.share(Share::Withdraw);
        let withdrawn = told(&shared);
        assert!(withdrawn.offer.is_none() && withdrawn.serving.is_some());

        let guest = kept();
        let pubkey = key(&guest).public();
        pair_as(
            &server,
            port,
            &guest,
            Grant {
                scope: Scope::Read,
                days: Some(7),
                ..Grant::default()
            },
        );
        while shared.try_recv().is_ok() {}
        let (link, welcome, ..) = desk(port, key(&guest), None).unwrap();
        assert_eq!(welcome.scope, Scope::Read);
        assert_eq!(welcome.label.as_deref(), Some(host_name().as_str()));
        assert!(welcome.until.is_some());
        let live = told(&shared);
        assert_eq!(live.linked, [pubkey]);
        drop(link);
        std::thread::sleep(Duration::from_millis(100));
        server.share(Share::Look);
        let after = std::iter::from_fn(|| shared.recv_timeout(Duration::from_millis(200)).ok())
            .last()
            .unwrap();
        assert!(after.linked.is_empty());
    }
}
