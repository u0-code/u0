//! Ghost reached from elsewhere: a desk on another machine asks this
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

/// A record or a file read can be many megabytes: sent as one message.
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

#[derive(Default)]
struct Frames {
    kept: VecDeque<(u64, Utf8Bytes)>,
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
        let mut f = self.frames.lock().expect("outbox");
        f.last += 1;
        let n = f.last;
        let frame = Utf8Bytes::from(format!("[{n},{json}]"));
        f.bytes += frame.len();
        f.kept.push_back((n, frame));
        while f.bytes > KEPT
            && let Some((_, old)) = f.kept.pop_front()
        {
            f.bytes -= old.len();
        }
        self.said.send_replace(f.last);
    }

    /// What was said after the number `since`; none where some of it is
    /// no longer kept.
    fn after(&self, since: u64) -> Option<Vec<(u64, Utf8Bytes)>> {
        let f = self.frames.lock().expect("outbox");
        let first = f.kept.front().map_or(f.last + 1, |(n, _)| *n);
        (since + 1 >= first && since <= f.last).then(|| {
            (f.kept.iter())
                .filter(|(n, _)| *n > since)
                .cloned()
                .collect()
        })
    }

    /// Where a desk that last heard `since` begins: after what it heard,
    /// where all after that is kept (resumed); else after all said so far.
    fn resume(&self, since: Option<(u64, u64)>) -> (u64, bool) {
        match since {
            Some((boot, n)) if boot == self.boot && self.after(n).is_some() => (n, true),
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
    let mut links = tokio::task::JoinSet::new();
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
            Some(_) = links.join_next() => continue,
        };
        let (server, tls) = (server.clone(), tls.clone());
        links.spawn(async move {
            let ended = match tls {
                None => http(server, stream).await,
                Some(tls) => match tokio::time::timeout(WAIT, tls.accept(stream)).await {
                    Ok(Ok(stream)) => Box::pin(http(server, stream)).await,
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

/// The most a request's head may take, its line and headers.
const HEAD: usize = 16 << 10;

/// One connection, as HTTP/1.1 (hyper's): a webhook's call answered, or
/// a desk's WebSocket upgraded from it and served on this same task, so
/// that it ends when listening does.
async fn http<S>(server: Arc<Server>, stream: S) -> Result<(), String>
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
    serve(
        &server,
        WebSocketStream::from_raw_socket(io, role, Some(config())).await,
    )
    .await
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
    use hyper::{Method, StatusCode, header};
    if req.method() == Method::POST
        && let Some(token) = req.uri().path().strip_prefix("/hook/")
    {
        let token = token.to_owned();
        return hook(server, &token, req).await;
    }
    let asks = |name, value: &str| {
        (req.headers().get_all(name).iter())
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|v| v.trim().eq_ignore_ascii_case(value))
    };
    let key = req.headers().get(header::SEC_WEBSOCKET_KEY).cloned();
    let (Some(key), true) = (key, asks(header::UPGRADE, "websocket")) else {
        return status(StatusCode::NOT_FOUND);
    };
    *upgrade.lock().expect("upgrade") = Some(hyper::upgrade::on(&mut req));
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
    let mut answer = status(StatusCode::SWITCHING_PROTOCOLS);
    let headers = answer.headers_mut();
    headers.insert(
        header::UPGRADE,
        header::HeaderValue::from_static("websocket"),
    );
    headers.insert(
        header::CONNECTION,
        header::HeaderValue::from_static("Upgrade"),
    );
    headers.insert(
        header::SEC_WEBSOCKET_ACCEPT,
        header::HeaderValue::from_str(&accept).expect("base64 is a header's"),
    );
    answer
}

/// Serve the Ghost in this machine's home to desks elsewhere, with no
/// window, until stopped: what `ghost-server` and `ghost-desk --serve` do.
/// A pairing link for the first desk is printed as it starts.
pub fn serve_here() -> Result<(), String> {
    let server = Server::new(crate::settings::home(), |_| {});
    server.serve()?;
    server.share_as_kept(true);
    let link = server
        .offer(crate::access::Grant::default(), None)
        .expect("it listens");
    let (way, _) = crate::reach::pairing(&link).expect("a pairing link reads");
    // Light on dark, as a terminal mostly is: a camera reads it as dark
    // on light.
    let qr = qr(&link)
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .build();
    println!("Ghost serves on {}", way.url());
    println!("Pair a desk with it, once, in the next ten minutes: {link}\n{qr}");
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

/// What `server` says to a device let in with `scope`, as it is: to be
/// filled in with what the link resumes.
fn welcome(server: &Server, scope: Scope) -> Welcome {
    Welcome {
        boot: server.outbox().boot,
        name: server_name(server.home()),
        version: crate::update::this_build(),
        home: server.home().to_path_buf(),
        resumed: false,
        said: 0,
        settings: server.settings(),
        back: None,
        scope,
    }
}

/// Carry bytes between `ws` (as binary messages) and `near`, until either
/// ends.
async fn carry<S, N>(mut ws: Socket<S>, mut near: N) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    N: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut buf = vec![0u8; 64 << 10];
    loop {
        tokio::select! {
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

/// Bring the port `there` of the server `way` here, coming in with the key
/// kept as `kept`: at the same port where it is free, else at another.
pub fn forward(way: Way, kept: String, there: u16) -> Result<Forward, String> {
    let rt = crate::acp::runtime();
    let bound = std::net::TcpListener::bind(("127.0.0.1", there))
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
                accepted = listener.accept() => match accepted {
                    Ok((near, _)) => near,
                    Err(_) => continue,
                },
                Some(_) = carried.join_next() => continue,
            };
            let (way, key) = (way.clone(), Key::read(&kept));
            carried.spawn(async move {
                let Ok(key) = key else { return };
                match dial(&way, hello(&key, None, None, Then::Forward(there))).await {
                    Ok((ws, _)) => {
                        if let Err(why) = carry(ws, near).await {
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

/// Reach the tools of the server `way` over a link of their own, coming
/// in with `key`, and carry MCP between them and `here` until either ends.
pub async fn tools<H>(way: &Way, key: &Key, here: H) -> Result<(), String>
where
    H: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match dial(way, hello(key, None, None, Then::Tools)).await {
        Ok((ws, _)) => carry(ws, here).await,
        Err(Dial::Failed(why) | Dial::Refused(why)) => Err(why),
        Err(Dial::Waiting(name)) => Err(waiting(&name)),
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

/// A WebSocket upgraded from a connection to where Ghost listens.
type Upgraded = Socket<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>;

/// One desk's link: its hello, then what it asks and what it is told.
async fn serve(server: &Arc<Server>, mut ws: Upgraded) -> Result<(), String> {
    let nonce = crate::access::random::<32>();
    let challenge = Challenge {
        nonce: nonce.clone(),
    };
    let text = serde_json::to_string(&challenge).expect("a challenge serializes");
    ws.send(Message::text(text))
        .await
        .map_err(|e| e.to_string())?;
    let hello = match tokio::time::timeout(WAIT, ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<Hello>(&t).ok(),
        _ => None,
    };
    let Some(hello) = hello else {
        return refuse(ws, "Say hello first").await;
    };
    if hello.protocol != PROTOCOL {
        let why = format!(
            "This Ghost speaks protocol {PROTOCOL} and yours {}: update the older",
            hello.protocol
        );
        return refuse(ws, &why).await;
    }
    let admitted = server.access().admit(&hello, &nonce).await;
    // Its device is now paired, or last here now, or waits: Settings ›
    // Devices shows it so, wherever it is open.
    server.share(crate::share::Share::Look);
    let (device, mut scope) = match admitted {
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
    if hello.then == Then::Tools {
        let welcome = welcome(server, scope);
        let text = serde_json::to_string(&welcome).expect("a welcome serializes");
        ws.send(Message::text(text))
            .await
            .map_err(|e| e.to_string())?;
        let (near, far) = tokio::io::duplex(64 << 10);
        let tooled = server.clone();
        tokio::spawn(async move { tooled.tools_for(scope, far).await });
        return carry(ws, near).await;
    }
    if let Then::Forward(port) = hello.then {
        if scope != Scope::Full {
            return refuse(ws, "This device may only watch here: no port is forwarded").await;
        }
        let near = match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(near) => near,
            Err(e) => return refuse(ws, &format!("Nothing answers on port {port}: {e}")).await,
        };
        let welcome = welcome(server, scope);
        let text = serde_json::to_string(&welcome).expect("a welcome serializes");
        ws.send(Message::text(text))
            .await
            .map_err(|e| e.to_string())?;
        return carry(ws, near).await;
    }
    // Asked as it paired, and so by its code: pair back with it.
    let back = match hello.pair.and_then(|p| p.back) {
        Some(back) => Some(server.pair_back(&back).await),
        None => None,
    };
    let outbox = server.outbox();
    let mut said = outbox.said.subscribe();
    // What was missed goes first.
    said.mark_changed();
    let mut access = server.access().changes();
    let (mut at, resumed) = outbox.resume(hello.since);
    let welcome = Welcome {
        resumed,
        said: at,
        back,
        ..welcome(server, scope)
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
                    Ok(call) if Access::allows(scope, &call, server.home()) => server.handle(call),
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
                let Some(frames) = outbox.after(at) else {
                    return refuse(ws, "Too much was missed; come back afresh").await;
                };
                for (n, frame) in frames {
                    ws.feed(Message::Text(frame)).await.map_err(|e| e.to_string())?;
                    at = n;
                }
                ws.flush().await.map_err(|e| e.to_string())?;
            }
            // Unpaired, its access ended or changed: so it is from now on.
            _ = access.changed() => match server.access().lets_in(&device) {
                Ok(now) => scope = now,
                Err(why) => return refuse(ws, &cut(why)).await,
            },
            _ = ping.tick() => match server.access().lets_in(&device) {
                Ok(_) => next = Some(Message::Ping(Bytes::new())),
                Err(why) => return refuse(ws, &cut(why)).await,
            },
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

/// What the Ghost in `home` calls itself to a desk: its machine's name,
/// and for a home other than `~/.ghost` that home's folder too, so that a
/// desk holding two Ghosts of one machine tells them apart.
fn server_name(home: &std::path::Path) -> String {
    let default = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".ghost"));
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

    /// Wait, a few seconds at most, until all asked has gone: as Ghost
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
                "The server answered in another form ({e}): update the older Ghost"
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
        let body = "{\"ref\":\"main\"}";
        let sign = |secret: &str| {
            let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
            let tag = ring::hmac::sign(&key, body.as_bytes());
            let hex = tag.as_ref().iter().fold(String::new(), |mut hex, b| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{b:02x}");
                hex
            });
            format!("sha256={hex}")
        };
        let call = |token: &str, signature: &str, event: &str, delivery: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                s,
                "POST /hook/{token} HTTP/1.1\r\nHost: x\r\nX-Hub-Signature-256: {signature}\r\nX-GitHub-Event: {event}\r\nX-GitHub-Delivery: {delivery}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
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
        assert_eq!(call(&token, &sign("s3cret"), "push", "d4"), "HTTP/1.1 202");
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
        let forwarded = forward(here(port), mine, there).unwrap();
        assert_ne!(forwarded.port, there, "taken here, so another");
        assert_eq!(ask(&forwarded), "hello from there");
        let watcher = kept();
        let read = crate::access::Grant {
            scope: crate::access::Scope::Read,
            ..crate::access::Grant::default()
        };
        pair_as(&server, port, &watcher, read);
        let refused = forward(here(port), watcher, there).unwrap();
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
        let Paired { name, back, .. } = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                here(port_a),
                a.access().code(crate::access::Grant::default()),
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
            "two Ghosts of one machine, told apart"
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
    fn a_way_back_that_does_not_reach_leaves_the_pairing_one_way() {
        let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, port_a) = server(home_a.path());
        let nowhere = crate::reach::pairing_link(&here(1), "c0de");
        let Paired { name, back, .. } = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                here(port_a),
                a.access().code(crate::access::Grant::default()),
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
        let on = told(&shared);
        let (Some(Ok(port)), Some((offered, _, _))) = (on.serving, on.link) else {
            panic!("listening, with a link")
        };
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
        assert_eq!((off.serving, off.link), (None, None));
        assert!(matches!(told(&stood), State::Lost(_)));
        assert!(desk(port, Key::read(&kept).unwrap(), None).is_err());
        assert!(
            server.access().live(&spare).is_none(),
            "no code made before lets a key in"
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
}
