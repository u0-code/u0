//! Ghost reached from elsewhere: a desk on another machine asks this
//! server what the desk in this process would (each [`Call`]), and is told
//! what it says (each [`Event`]), as JSON over a WebSocket.
//!
//! The server listens on 127.0.0.1 only. What reaches it from elsewhere
//! comes through a tunnel that brings its own TLS (cloudflared, or an
//! `ssh -L`), so Ghost holds no certificate and plain `ws://` never leaves
//! the machine.
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
    key::Key,
    proto::{Call, Challenge, Event, Hello, PROTOCOL, Pair, Welcome},
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

/// Let desks link to `server` through `listener`, for as long as it runs:
/// each link it let in ends with it.
pub async fn listen(server: Arc<Server>, listener: TcpListener) {
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
        let server = server.clone();
        links.spawn(async move {
            if let Err(why) = serve(&server, stream).await {
                tracing::info!("a link ended: {why}");
            }
        });
    }
}

/// Serve the Ghost in this machine's home to desks elsewhere, with no
/// window, until stopped: what `ghost-server` and `ghost-desk --serve` do.
/// A pairing link for the first desk is printed as it starts.
pub fn serve_here() -> Result<(), String> {
    let server = Server::new(crate::settings::home(), |_| {});
    server.serve()?;
    server.share_as_kept(true);
    let link = server.offer().expect("it listens");
    let (host, _) = pairing(&link).expect("a pairing link reads");
    // Light on dark, as a terminal mostly is: a camera reads it as dark
    // on light.
    let qr = qr(&link)
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .build();
    println!("Ghost serves on ws://{host}");
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

/// How a call to a webhook task begins, where desks link (`Schedule::Hook`).
const HOOK: &[u8; 11] = b"POST /hook/";

/// The most of a webhook call's body read.
const HOOK_BODY: usize = 1 << 20;

/// A call to a webhook task: its token from the path, its body by its
/// `Content-Length`; answered 202 when the task runs, else 404.
async fn hook(server: &Arc<Server>, stream: TcpStream) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
    let mut read = tokio::io::BufReader::new(stream);
    let mut line = String::new();
    read.read_line(&mut line).await.map_err(|e| e.to_string())?;
    let token = (line.strip_prefix("POST /hook/"))
        .and_then(|r| r.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    let mut length = 0;
    loop {
        line.clear();
        if read.read_line(&mut line).await.map_err(|e| e.to_string())? == 0
            || line.trim().is_empty()
        {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; length.min(HOOK_BODY)];
    read.read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    let ran = server.hook(&token, &String::from_utf8_lossy(&body));
    let status = if ran { "202 Accepted" } else { "404 Not Found" };
    let answer = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    read.get_mut()
        .write_all(answer.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

type Socket<S> = WebSocketStream<S>;

/// Close `ws`, saying `why` to a desk that is not let in.
async fn refuse<S>(mut ws: Socket<S>, why: &str) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // A close frame's reason is at most 123 bytes.
    let mut end = why.len().min(123);
    while !why.is_char_boundary(end) {
        end -= 1;
    }
    let frame = CloseFrame {
        code: CloseCode::Policy,
        reason: why[..end].to_owned().into(),
    };
    let _ = ws.close(Some(frame)).await;
    Err(why.to_owned())
}

/// One desk's link: its hello, then what it asks and what it is told.
async fn serve(server: &Arc<Server>, stream: TcpStream) -> Result<(), String> {
    let mut start = [0u8; HOOK.len()];
    if stream.peek(&mut start).await.is_ok_and(|n| n == HOOK.len()) && start == *HOOK {
        return hook(server, stream).await;
    }
    let accepted = tokio_tungstenite::accept_async_with_config(stream, Some(config()));
    let mut ws = accepted.await.map_err(|e| e.to_string())?;
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
    let device = match server.access().admit(&hello, &nonce).await {
        Ok(admitted) => admitted,
        Err(why) => return refuse(ws, &why).await,
    };
    // Its device is now paired, or last here now: Settings › Devices shows
    // it so, wherever it is open.
    server.share(crate::share::Share::Look);
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
        boot: outbox.boot,
        name: host_name(),
        home: server.home().to_path_buf(),
        resumed,
        said: at,
        settings: server.settings(),
        back,
    };
    let text = serde_json::to_string(&welcome).expect("a welcome serializes");
    let mut ping = tokio::time::interval(PING);
    let mut next = Some(Message::text(text));
    loop {
        if let Some(message) = next.take() {
            ws.send(message).await.map_err(|e| e.to_string())?;
        }
        tokio::select! {
            heard = ws.next() => match heard {
                Some(Ok(Message::Text(t))) => match serde_json::from_str::<Call>(&t) {
                    Ok(call) => server.handle(call),
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
            _ = access.changed() => {
                if !server.access().paired(&device) {
                    return refuse(ws, "This device was unpaired").await;
                }
            }
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
}

type Client = Socket<MaybeTlsStream<TcpStream>>;

/// Reach the server at `url` (`ws://127.0.0.1:…`, or `wss://` through a
/// tunnel), signing with `key`, from now until the link is let go: each
/// time it is reached its welcome goes to `state`, and each time it
/// cannot be, or drops, why; it is reached again after a second, then
/// twice as long each time, up to half a minute. What the server says
/// goes to `said`.
pub fn connect(
    url: String,
    key: Key,
    said: impl Fn(Event) + Send + Sync + 'static,
    state: impl Fn(State) + Send + Sync + 'static,
) -> Link {
    const FIRST: Duration = Duration::from_secs(1);
    let (tx, mut calls) = mpsc::unbounded_channel();
    let again = Arc::new(Notify::new());
    let woken = again.clone();
    crate::acp::runtime().spawn(async move {
        let (mut since, mut wait, mut lost) = (None, FIRST, false);
        while !calls.is_closed() {
            match dial(&url, hello(&key, None, since)).await {
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
            }
            tokio::select! {
                () = tokio::time::sleep(wait) => wait = (wait * 2).min(Duration::from_secs(30)),
                () = woken.notified() => wait = FIRST,
            }
        }
    });
    Link { calls: tx, again }
}

/// Pair with the server at `host` by its one-time `code`, coming in with
/// `key` as this machine; `back`, a pairing link for it to pair back
/// with. Its welcome, once it has let the key in; the link is let go.
pub async fn pair(
    host: &str,
    code: String,
    key: &Key,
    back: Option<String>,
) -> Result<Welcome, String> {
    let pair = Pair {
        code,
        device: host_name(),
        back,
    };
    let (mut ws, welcome) = dial(&url_of(host), hello(key, Some(pair), None))
        .await
        .map_err(|(Dial::Failed(why) | Dial::Refused(why))| why)?;
    let _ = ws.close(None).await;
    Ok(welcome)
}

/// What a desk says to a challenge: it is `key`, pairing with `pair`, and
/// last heard `since`.
fn hello(
    key: &Key,
    pair: Option<Pair>,
    since: Option<(u64, u64)>,
) -> impl FnOnce(&Challenge) -> Hello {
    move |c| Hello {
        protocol: PROTOCOL,
        key: key.public(),
        signature: key.sign(&c.nonce),
        pair,
        since,
    }
}

/// Reach the server at `url` and say `hello` to its challenge: the
/// socket, and its welcome.
async fn dial(
    url: &str,
    hello: impl FnOnce(&Challenge) -> Hello,
) -> Result<(Client, Welcome), Dial> {
    let reached = tokio_tungstenite::connect_async_with_config(url, Some(config()), false);
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

/// The address of a server at `host`: on this machine (an `ssh -L`) plain,
/// anywhere else through a tunnel's TLS.
pub fn url_of(host: &str) -> String {
    let here = ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|h| host == *h || host.starts_with(&format!("{h}:")));
    format!("{}://{host}", if here { "ws" } else { "wss" })
}

/// The host and code of a pairing link, `ghost://pair?host=…#code`.
pub fn pairing(link: &str) -> Option<(String, String)> {
    let url = url::Url::parse(link).ok()?;
    if url.scheme() != "ghost" || url.host_str() != Some("pair") {
        return None;
    }
    let host = url.query_pairs().find(|(k, _)| k == "host")?.1;
    let code = url.fragment()?;
    (!host.is_empty() && !code.is_empty()).then(|| (host.into_owned(), code.to_owned()))
}

/// The pairing link for `code` at `host`.
pub fn pairing_link(host: &str, code: &str) -> String {
    let mut url = url::Url::parse("ghost://pair").expect("a link");
    url.query_pairs_mut().append_pair("host", host);
    url.set_fragment(Some(code));
    url.into()
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
        rt.spawn(listen(server.clone(), listener));
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
            schedule: crate::tasks::Schedule::Hook("tok".into()),
            paused: false,
            same: false,
            worktree: false,
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
        // The run is kept as the writer gets to it.
        let ran = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            crate::tasks::load(home.path()).is_ok_and(|t| t.tasks[0].runs == 1)
        });
        assert!(ran, "the task ran once");
        assert_eq!(
            crate::tasks::Schedule::filled("Look at {{body}}", "x"),
            "Look at <webhook-data untrusted>\nx\n</webhook-data>"
        );
    }

    type Desk = (
        Link,
        Welcome,
        std_mpsc::Receiver<Event>,
        std_mpsc::Receiver<State>,
    );

    /// A desk linked to the server at `port` with `key`, paired first
    /// with `server` where there is one: its link and welcome, what it is
    /// told and how its link stands.
    fn desk(port: u16, key: Key, server: Option<&Server>) -> Result<Desk, String> {
        if let Some(server) = server {
            let host = format!("127.0.0.1:{port}");
            let code = server.access().code();
            crate::acp::runtime().block_on(super::pair(&host, code, &key, None))?;
        }
        let (events, heard) = std_mpsc::channel();
        let (states, stood) = std_mpsc::channel();
        let (events, states) = (Mutex::new(events), Mutex::new(states));
        let link = connect(
            format!("ws://127.0.0.1:{port}"),
            key,
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
            code: server.access().code(),
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
        let url = format!("ws://127.0.0.1:{port}");
        match crate::acp::runtime().block_on(dial(&url, hello)) {
            Ok((_, welcome)) => Ok(welcome),
            Err(Dial::Refused(why) | Dial::Failed(why)) => Err(why),
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
            let said = hello(&key(&mine), None, None)(c);
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
            ..hello(&Key::generate(), Some(code(&server)), None)(c)
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
        let listening = rt.spawn(listen(server.clone(), listener));
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let code = server.access().code();
        rt.block_on(pair(&format!("127.0.0.1:{port}"), code, &key, None))
            .unwrap();
        // It stops listening, and the port is free again.
        listening.abort();
        let bind = || rt.block_on(TcpListener::bind(("127.0.0.1", port)));
        while bind().is_err() {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (states, stood) = std_mpsc::channel();
        let states = Mutex::new(states);
        let link = connect(
            format!("ws://127.0.0.1:{port}"),
            Key::read(&kept).unwrap(),
            |_| {},
            move |s| {
                let _ = states.lock().unwrap().send(s);
            },
        );
        assert!(matches!(told(&stood), State::Lost(_)));
        rt.spawn(listen(server.clone(), bind().unwrap()));
        link.again();
        assert!(matches!(told(&stood), State::Up(w) if !w.resumed));
    }

    /// Link with the key kept for the server `name` in `home`'s servers.
    fn open(home: &std::path::Path, name: &str) -> Result<Welcome, String> {
        let servers = crate::servers::Servers::load(home).unwrap();
        let server = servers.servers.iter().find(|s| s.name == name).unwrap();
        let (url, key) = server.reach()?;
        let (states, stood) = std_mpsc::channel();
        let states = Mutex::new(states);
        let _link = connect(
            url,
            key,
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
        let back = pairing_link(&format!("127.0.0.1:{port_b}"), &b.access().code());
        let (name, back) = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                format!("127.0.0.1:{port_a}"),
                a.access().code(),
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
        assert!(open(home_b.path(), &name).is_ok(), "B opens A");
        assert!(open(home_a.path(), &name).is_ok(), "A opens B");
        // A lets B go; B still lets A in.
        a.access().revoke(&on_a.devices[0].key);
        let cut = open(home_b.path(), &name).unwrap_err();
        assert!(cut.contains("not paired"), "{cut}");
        assert!(open(home_a.path(), &name).is_ok(), "A still opens B");
    }

    #[test]
    fn a_way_back_that_does_not_reach_leaves_the_pairing_one_way() {
        let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, port_a) = server(home_a.path());
        let nowhere = pairing_link("127.0.0.1:1", "c0de");
        let (name, back) = crate::acp::runtime()
            .block_on(crate::servers::pair(
                home_b.path(),
                format!("127.0.0.1:{port_a}"),
                a.access().code(),
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
        let (Some(Ok(port)), Some((offered, _))) = (on.serving, on.link) else {
            panic!("listening, with a link")
        };
        let (host, code) = pairing(&offered).unwrap();
        assert_eq!(host, format!("127.0.0.1:{port}"));
        let key = Key::generate();
        let kept = key.kept().to_owned();
        crate::acp::runtime()
            .block_on(pair(&host, code, &key, None))
            .unwrap();
        let (_link, _, _heard, stood) = desk(port, Key::read(&kept).unwrap(), None).unwrap();
        let spare = server.access().code();
        server.share(Share::Serve(false));
        let off = std::iter::from_fn(|| shared.try_recv().ok())
            .last()
            .unwrap();
        assert_eq!((off.serving, off.link), (None, None));
        assert!(matches!(told(&stood), State::Lost(_)));
        assert!(desk(port, Key::read(&kept).unwrap(), None).is_err());
        assert!(
            !server.access().live(&spare),
            "no code made before lets a key in"
        );
    }

    #[test]
    fn the_way_back_is_this_macs_port_for_a_server_here_and_its_tunnel_for_one_elsewhere() {
        let home = tempfile::tempdir().unwrap();
        let (server, shared) = sharing(home.path());
        let here = server.back_link("127.0.0.1:4317").unwrap();
        let Some(Ok(port)) = told(&shared).serving else {
            panic!("listening")
        };
        let (host, code) = pairing(&here).unwrap();
        assert_eq!(host, format!("127.0.0.1:{port}"));
        assert!(server.access().live(&code));
        let why = server
            .back_link("calm-river-1.trycloudflare.com")
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
    fn a_pairing_link_names_the_server_and_its_code() {
        let link = pairing_link("abc.trycloudflare.com", "c0de");
        let (host, code) = pairing(&link).unwrap();
        assert_eq!(url_of(&host), "wss://abc.trycloudflare.com");
        assert_eq!(code, "c0de");
        assert_eq!(url_of("127.0.0.1:4317"), "ws://127.0.0.1:4317");
        assert_eq!(pairing("ghost://pair?host=#x"), None);
        assert_eq!(pairing("ghost://new?host=h#x"), None);
    }

    #[test]
    fn a_host_with_a_port_and_a_code_with_plus_or_percent_come_back_as_given() {
        let link = pairing_link("127.0.0.1:4317", "a+b%2Fc");
        assert_eq!(
            pairing(&link),
            Some(("127.0.0.1:4317".into(), "a+b%2Fc".into()))
        );
    }
}
