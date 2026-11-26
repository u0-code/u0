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
    sync::{mpsc, watch},
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

/// The port a server listens on, on 127.0.0.1.
pub const PORT: u16 = 4317;

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

/// Let desks link to `server` through `listener`, for as long as it runs.
pub async fn listen(server: Arc<Server>, listener: TcpListener) {
    server.outbox().open.store(true, Ordering::Relaxed);
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let server = server.clone();
        tokio::spawn(async move {
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
    let host = format!("127.0.0.1:{PORT}");
    let link = pairing_link(&host, &server.access().code());
    println!("Ghost serves on ws://{host}");
    println!("Pair a desk with it, once, in the next ten minutes: {link}");
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
    // Its device is now paired, or last here now: Sharing & access shows
    // it so, wherever it is open.
    server.share(crate::share::Share::Look);
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
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is as long as said, and gethostname writes no
    // more than that.
    let named = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]);
    let name = name.trim_end_matches(".local");
    if named && !name.is_empty() {
        name.to_owned()
    } else {
        "this machine".into()
    }
}

/// How a link to a server stands, as the desk is told.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// It dropped, why: it is being reached again.
    Lost(String),
    /// It is back, and all said meanwhile has been told.
    Back,
    /// It is back, but not all said meanwhile is there to be told: the
    /// server restarted, or too much was said.
    Afresh,
    /// The server will not let this desk in, why: it is not reached again.
    Refused(String),
}

/// A desk's link to a server: what it asks goes in order, kept while the
/// server is out of reach and sent once it is back.
pub struct Link(mpsc::UnboundedSender<Out>);

/// What goes out on a link: a call, or a word back once all before it
/// has gone.
enum Out {
    Call(Box<Call>),
    Settle(std::sync::mpsc::Sender<()>),
}

impl Link {
    pub fn send(&self, call: Call) {
        let _ = self.0.send(Out::Call(Box::new(call)));
    }

    /// Wait, a few seconds at most, until all asked has gone: as Ghost
    /// quits.
    pub fn settle(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.0.send(Out::Settle(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
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
/// tunnel), signing with `key`, and the first time with `pair`: its
/// welcome, and the link, which reaches it again each time it drops. What
/// the server says goes to `said`, how the link stands to `state`.
pub async fn connect(
    url: String,
    key: Key,
    pair: Option<Pair>,
    said: impl Fn(Event) + Send + Sync + 'static,
    state: impl Fn(State) + Send + Sync + 'static,
) -> Result<(Link, Welcome), String> {
    let (mut ws, welcome) = dial(&url, hello(&key, pair, None))
        .await
        .map_err(|(Dial::Failed(why) | Dial::Refused(why))| why)?;
    let mut since = Some((welcome.boot, welcome.said));
    let (tx, mut calls) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Some(why) = pump(&mut ws, &mut calls, &said, &mut since).await else {
                return;
            };
            state(State::Lost(why));
            let mut wait = Duration::from_secs(1);
            ws = loop {
                tokio::time::sleep(wait).await;
                match dial(&url, hello(&key, None, since)).await {
                    Ok((ws, welcome)) => {
                        state(if welcome.resumed {
                            State::Back
                        } else {
                            State::Afresh
                        });
                        since = Some((welcome.boot, welcome.said));
                        break ws;
                    }
                    Err(Dial::Refused(why)) => return state(State::Refused(why)),
                    Err(Dial::Failed(_)) => wait = (wait * 2).min(Duration::from_secs(30)),
                }
            };
        }
    });
    Ok((Link(tx), welcome))
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
    let rest = link.trim().strip_prefix("ghost://pair?")?;
    let (query, code) = rest.split_once('#')?;
    let host = query.split('&').find_map(|kv| kv.strip_prefix("host="))?;
    (!host.is_empty() && !code.is_empty()).then(|| (host.to_owned(), code.to_owned()))
}

/// The pairing link for `code` at `host`.
pub fn pairing_link(host: &str, code: &str) -> String {
    format!("ghost://pair?host={host}#{code}")
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

    type Desk = (
        Link,
        Welcome,
        std_mpsc::Receiver<Event>,
        std_mpsc::Receiver<State>,
    );

    /// A desk linked to the server at `port` with `key`, pairing with
    /// `pair`: its link and welcome, what it is told and how its link
    /// stands.
    fn desk(port: u16, key: Key, pair: Option<Pair>) -> Result<Desk, String> {
        let (events, heard) = std_mpsc::channel();
        let (states, stood) = std_mpsc::channel();
        let (events, states) = (Mutex::new(events), Mutex::new(states));
        let (link, welcome) = crate::acp::runtime().block_on(connect(
            format!("ws://127.0.0.1:{port}"),
            key,
            pair,
            move |e| events.lock().unwrap().send(e).unwrap(),
            move |s| states.lock().unwrap().send(s).unwrap(),
        ))?;
        Ok((link, welcome, heard, stood))
    }

    fn pair(server: &Server) -> Pair {
        Pair {
            code: server.access().code(),
            device: "laptop".into(),
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
        let (link, welcome, heard, _) = desk(port, key(&mine), Some(pair(&server))).unwrap();
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
        drop(desk(port, key(&mine), Some(pair(&server))).unwrap());
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
            ..hello(&Key::generate(), Some(pair(&server)), None)(c)
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
        let (_link, _, heard, stood) = desk(through, Key::generate(), Some(pair(&server))).unwrap();
        cut.send_modify(|n| *n += 1);
        assert!(matches!(told(&stood), State::Lost(_)));
        // Said while the desk is away: for another desk, say.
        server.handle(Call::Stash(vec!["later".into()]));
        server.handle(Call::Watch);
        assert_eq!(told(&stood), State::Back);
        let Event::Index(Ok(index)) = told(&heard) else {
            panic!("the index, missed")
        };
        assert_eq!(index.stashed, ["later"]);
    }

    #[test]
    fn a_device_unpaired_is_cut_off_and_not_let_back_in() {
        let home = tempfile::tempdir().unwrap();
        let (server, port) = server(home.path());
        let (_kept, ..) = desk(port, Key::generate(), Some(pair(&server))).unwrap();
        let (_link, _, _, stood) = desk(port, Key::generate(), Some(pair(&server))).unwrap();
        let devices = server.access().kept().unwrap().devices;
        assert_eq!(devices.len(), 2);
        server.access().revoke(&devices[1].key);
        assert_eq!(told(&stood), State::Lost("This device was unpaired".into()));
        assert!(matches!(told(&stood), State::Refused(why) if why.contains("not paired")));
        assert_eq!(server.access().kept().unwrap().devices.len(), 1);
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
    }
}
