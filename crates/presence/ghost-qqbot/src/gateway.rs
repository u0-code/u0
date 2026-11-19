//! The QQ Bot gateway: one WebSocket that carries every event, kept alive.
//!
//! [`Session`] is the protocol with no socket in it: what to say once the
//! gateway says hello, what each payload means, what a close code asks for.
//! [`run`] holds the socket and does what the session says. Keeping them
//! apart is what lets the protocol be tested without QQ.

use std::{future::Future, time::Duration};

use futures_util::{SinkExt, StreamExt};
use ghost_link::presence::Backoff;
use serde_json::{Value, json};
use tokio::{
    sync::mpsc,
    time::{Instant, Interval, MissedTickBehavior},
};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

/// Private chats and group @-mentions, and nothing else. Asking for an
/// intent the bot has not been granted closes the socket.
pub const INTENTS: u64 = 1 << 25;

/// The first and the longest wait between reconnections in a row that fail.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const SLOWEST_RETRY: Duration = Duration::from_secs(60);
const RATE_LIMITED: Duration = Duration::from_secs(60);
const INVALID_SESSION: Duration = Duration::from_secs(3);
/// What the gateway's heartbeat interval is when it does not say.
const HEARTBEAT: u64 = 41_250;

mod op {
    pub const DISPATCH: u64 = 0;
    pub const HEARTBEAT: u64 = 1;
    pub const IDENTIFY: u64 = 2;
    pub const RESUME: u64 = 6;
    pub const RECONNECT: u64 = 7;
    pub const INVALID_SESSION: u64 = 9;
    pub const HELLO: u64 = 10;
}

/// What the gateway tells the adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The gateway accepted a new session. Carries `READY`'s `user`: who
    /// the bot is.
    Ready(Value),
    /// Anything else it dispatched, by type: messages among them.
    Dispatch { kind: String, data: Value },
}

/// What the socket has to do next.
#[derive(Debug, PartialEq)]
pub enum Step {
    Send(Value),
    /// Send a heartbeat at this interval from now on.
    Beat(Duration),
    Emit(Event),
    /// Drop this socket and connect again after a while.
    Reconnect(Duration),
}

/// What a closed socket means.
#[derive(Debug, PartialEq)]
pub enum Closed {
    Retry(Duration),
    /// Connecting again cannot help.
    Fatal(String),
}

/// One gateway session, across however many sockets it takes.
#[derive(Debug)]
pub struct Session {
    id: Option<String>,
    seq: Option<u64>,
    /// The token was refused or the session is gone: fetch a new token
    /// before the next connection.
    pub stale_token: bool,
    retry: Backoff,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            id: None,
            seq: None,
            stale_token: false,
            retry: Backoff::new(FIRST_RETRY, SLOWEST_RETRY),
        }
    }
}

impl Session {
    /// Take in one payload from the gateway.
    pub fn on_payload(&mut self, payload: &Value, token: &str) -> Vec<Step> {
        if let Some(seq) = payload["s"].as_u64() {
            self.seq = Some(seq);
        }
        match payload["op"].as_u64() {
            Some(op::HELLO) => {
                let every = payload["d"]["heartbeat_interval"]
                    .as_u64()
                    .unwrap_or(HEARTBEAT);
                vec![
                    Step::Send(self.introduce(token)),
                    Step::Beat(Duration::from_millis(every)),
                ]
            }
            Some(op::DISPATCH) => {
                let kind = payload["t"].as_str().unwrap_or_default();
                let data = &payload["d"];
                match kind {
                    "READY" => {
                        self.id = data["session_id"].as_str().map(str::to_owned);
                        self.retry.reset();
                        vec![Step::Emit(Event::Ready(data["user"].clone()))]
                    }
                    "RESUMED" => {
                        self.retry.reset();
                        Vec::new()
                    }
                    _ => vec![Step::Emit(Event::Dispatch {
                        kind: kind.to_owned(),
                        data: data.clone(),
                    })],
                }
            }
            // The gateway is moving: the session goes with us.
            Some(op::RECONNECT) => vec![Step::Reconnect(self.backoff())],
            Some(op::INVALID_SESSION) => {
                // `true` means the session can still be resumed.
                if payload["d"].as_bool() != Some(true) {
                    self.forget();
                }
                vec![Step::Reconnect(INVALID_SESSION)]
            }
            _ => Vec::new(),
        }
    }

    /// Resume the session if there is one to resume, or start a new one.
    fn introduce(&self, token: &str) -> Value {
        let token = format!("QQBot {token}");
        match (&self.id, self.seq) {
            (Some(id), Some(seq)) => json!({
                "op": op::RESUME,
                "d": {"token": token, "session_id": id, "seq": seq},
            }),
            _ => json!({
                "op": op::IDENTIFY,
                "d": {"token": token, "intents": INTENTS, "shard": [0, 1]},
            }),
        }
    }

    pub fn heartbeat(&self) -> Value {
        json!({"op": op::HEARTBEAT, "d": self.seq})
    }

    /// The socket closed, with this code if it gave one.
    pub fn on_close(&mut self, code: Option<u16>) -> Closed {
        match code {
            Some(4914) => Closed::Fatal(
                "qq closed the gateway with 4914: the bot is offline or limited to its sandbox"
                    .into(),
            ),
            Some(4915) => {
                Closed::Fatal("qq closed the gateway with 4915: the bot is banned".into())
            }
            Some(4004) => {
                self.stale_token = true;
                Closed::Retry(self.backoff())
            }
            Some(4008) => Closed::Retry(RATE_LIMITED),
            // The session cannot be resumed: start a new one.
            Some(4006 | 4007 | 4009 | 4900..=4913) => {
                self.forget();
                Closed::Retry(self.backoff())
            }
            _ => Closed::Retry(self.backoff()),
        }
    }

    fn forget(&mut self) {
        self.id = None;
        self.seq = None;
        self.stale_token = true;
    }

    /// How long to wait before the next try, longer each time in a row.
    pub fn backoff(&mut self) -> Duration {
        self.retry.wait()
    }
}

/// Keep a gateway session alive and send what it dispatches to `events`.
///
/// `connect` gives the token and gateway address to use, fetching a new
/// token when it is passed `true`. Returns when `events` is closed, or with
/// an error when QQ says connecting again cannot help.
pub async fn run<F, Fut>(mut connect: F, events: mpsc::Sender<Event>) -> anyhow::Result<()>
where
    F: FnMut(bool) -> Fut,
    Fut: Future<Output = anyhow::Result<(String, String)>>,
{
    let mut session = Session::default();
    loop {
        let fresh = std::mem::take(&mut session.stale_token);
        let wait = match connect(fresh).await {
            Ok((token, url)) => sitting(&mut session, &token, &url, &events).await?,
            Err(e) => {
                warn!("cannot reach the qq gateway: {e:#}");
                session.backoff()
            }
        };
        if events.is_closed() {
            return Ok(());
        }
        tokio::time::sleep(wait).await;
    }
}

/// One socket, until it closes. Returns how long to wait before the next.
async fn sitting(
    session: &mut Session,
    token: &str,
    url: &str,
    events: &mpsc::Sender<Event>,
) -> anyhow::Result<Duration> {
    let socket = match ghost_net::websocket(url, None).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!("cannot open the qq gateway: {e:#}");
            return Ok(session.backoff());
        }
    };
    info!("connected to the qq gateway");
    let (mut sink, mut stream) = socket.split();
    let mut beat: Option<Interval> = None;
    loop {
        tokio::select! {
            () = tick(&mut beat) => {
                let heartbeat = Message::text(session.heartbeat().to_string());
                if sink.send(heartbeat).await.is_err() {
                    return Ok(session.backoff());
                }
            }
            frame = stream.next() => {
                let payload: Option<Value> = match frame {
                    Some(Ok(Message::Text(text))) => serde_json::from_str(text.as_str()).ok(),
                    Some(Ok(Message::Binary(bytes))) => serde_json::from_slice(&bytes).ok(),
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code));
                        warn!("the qq gateway closed the socket ({code:?})");
                        return closed(session, code);
                    }
                    Some(Ok(_)) => None,
                    Some(Err(e)) => {
                        warn!("lost the qq gateway: {e}");
                        return closed(session, None);
                    }
                    None => return closed(session, None),
                };
                let Some(payload) = payload else { continue };
                for step in session.on_payload(&payload, token) {
                    match step {
                        Step::Send(value) => {
                            if sink.send(Message::text(value.to_string())).await.is_err() {
                                return Ok(session.backoff());
                            }
                        }
                        Step::Beat(every) => {
                            let mut interval = tokio::time::interval_at(Instant::now() + every, every);
                            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                            beat = Some(interval);
                        }
                        Step::Emit(event) => {
                            if events.send(event).await.is_err() {
                                // Nobody is listening any more.
                                return Ok(Duration::ZERO);
                            }
                        }
                        Step::Reconnect(after) => return Ok(after),
                    }
                }
            }
        }
    }
}

fn closed(session: &mut Session, code: Option<u16>) -> anyhow::Result<Duration> {
    match session.on_close(code) {
        Closed::Retry(after) => Ok(after),
        Closed::Fatal(why) => anyhow::bail!(why),
    }
}

/// The next heartbeat, or never before the gateway has said hello.
async fn tick(beat: &mut Option<Interval>) {
    match beat {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

    use super::*;

    fn hello() -> Value {
        json!({"op": 10, "d": {"heartbeat_interval": 45000}})
    }

    fn ready(seq: u64) -> Value {
        json!({"op": 0, "s": seq, "t": "READY", "d": {
            "session_id": "s-1", "version": 1,
            "user": {"id": "9001", "username": "薇薇", "bot": true}
        }})
    }

    #[test]
    fn a_new_session_identifies_with_only_the_intents_it_needs() {
        let mut session = Session::default();
        let steps = session.on_payload(&hello(), "t0");
        assert_eq!(
            steps,
            [
                Step::Send(json!({"op": 2, "d": {
                    "token": "QQBot t0", "intents": 1 << 25, "shard": [0, 1]
                }})),
                Step::Beat(Duration::from_secs(45)),
            ]
        );
    }

    #[test]
    fn after_ready_a_new_socket_resumes_from_the_last_event_seen() {
        let mut session = Session::default();
        session.on_payload(&hello(), "t0");
        assert_eq!(
            session.on_payload(&ready(1), "t0"),
            [Step::Emit(Event::Ready(
                json!({"id": "9001", "username": "薇薇", "bot": true})
            ))]
        );
        let message = json!({"op": 0, "s": 7, "t": "C2C_MESSAGE_CREATE", "d": {"id": "m"}});
        assert!(matches!(&session.on_payload(&message, "t0")[..],
            [Step::Emit(Event::Dispatch { kind, .. })] if kind == "C2C_MESSAGE_CREATE"));
        assert_eq!(session.heartbeat(), json!({"op": 1, "d": 7}));

        // The socket drops with no code: the session is kept.
        assert_eq!(
            session.on_close(None),
            Closed::Retry(Duration::from_secs(1))
        );
        assert!(!session.stale_token);
        assert_eq!(
            session.on_payload(&hello(), "t1")[0],
            Step::Send(json!({"op": 6, "d": {"token": "QQBot t1", "session_id": "s-1", "seq": 7}}))
        );
    }

    #[test]
    fn a_session_qq_will_not_resume_is_started_again_with_a_new_token() {
        for code in [4006, 4007, 4009, 4900, 4913] {
            let mut session = Session::default();
            session.on_payload(&ready(3), "t0");
            assert!(matches!(session.on_close(Some(code)), Closed::Retry(_)));
            assert!(session.stale_token, "{code} asks for a new token");
            assert_eq!(
                session.on_payload(&hello(), "t1")[0],
                Step::Send(json!({"op": 2, "d": {
                    "token": "QQBot t1", "intents": 1 << 25, "shard": [0, 1]
                }})),
                "{code} starts a new session"
            );
        }
        // Told the session is invalid and cannot be resumed: the same.
        let mut session = Session::default();
        session.on_payload(&ready(3), "t0");
        assert_eq!(
            session.on_payload(&json!({"op": 9, "d": false}), "t0"),
            [Step::Reconnect(Duration::from_secs(3))]
        );
        assert!(session.stale_token);
        // Told it can be: it is resumed.
        let mut session = Session::default();
        session.on_payload(&ready(3), "t0");
        session.on_payload(&json!({"op": 9, "d": true}), "t0");
        assert!(!session.stale_token);
        assert!(matches!(&session.on_payload(&hello(), "t0")[0], Step::Send(v) if v["op"] == 6));
    }

    #[test]
    fn a_refused_token_is_fetched_again_and_a_ban_ends_the_adapter() {
        let mut session = Session::default();
        assert!(matches!(session.on_close(Some(4004)), Closed::Retry(_)));
        assert!(session.stale_token);
        assert_eq!(
            session.on_close(Some(4008)),
            Closed::Retry(Duration::from_secs(60))
        );
        assert!(
            matches!(session.on_close(Some(4914)), Closed::Fatal(why) if why.contains("sandbox"))
        );
        assert!(
            matches!(session.on_close(Some(4915)), Closed::Fatal(why) if why.contains("banned"))
        );
    }

    #[test]
    fn retries_wait_longer_each_time_until_a_session_is_ready() {
        let mut session = Session::default();
        let waits: Vec<u64> = (0..8).map(|_| session.backoff().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
        session.on_payload(&ready(1), "t0");
        assert_eq!(session.backoff(), Duration::from_secs(1));
    }

    /// A stand-in for QQ's gateway on a local socket: the first connection
    /// is identified and dropped after two events, the second resumed.
    #[tokio::test]
    async fn the_socket_identifies_beats_resumes_and_hands_on_every_event() {
        async fn next(ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Value {
            let Some(Ok(Message::Text(text))) = ws.next().await else {
                panic!("the adapter went away");
            };
            serde_json::from_str(text.as_str()).unwrap()
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let server = tokio::spawn({
            let seen = seen.clone();
            async move {
                let quick = json!({"op": 10, "d": {"heartbeat_interval": 30}});
                // First connection: identified, two events, a heartbeat, then dropped.
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(Message::text(quick.to_string())).await.unwrap();
                let said = next(&mut ws).await;
                seen.lock().unwrap().push(said);
                ws.send(Message::text(ready(1).to_string())).await.unwrap();
                let message = json!({"op": 0, "s": 2, "t": "C2C_MESSAGE_CREATE",
                    "d": {"id": "m-1", "content": "在吗", "author": {"user_openid": "U1"}}});
                ws.send(Message::text(message.to_string())).await.unwrap();
                loop {
                    let beat = next(&mut ws).await;
                    if beat == json!({"op": 1, "d": 2}) {
                        seen.lock().unwrap().push(beat);
                        break;
                    }
                }
                ws.close(Some(CloseFrame {
                    code: CloseCode::Away,
                    reason: "".into(),
                }))
                .await
                .unwrap();
                // Second connection: resumed, one more event.
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(Message::text(quick.to_string())).await.unwrap();
                let said = next(&mut ws).await;
                seen.lock().unwrap().push(said);
                let resumed = json!({"op": 0, "s": 3, "t": "RESUMED", "d": ""});
                ws.send(Message::text(resumed.to_string())).await.unwrap();
                let group = json!({"op": 0, "s": 4, "t": "GROUP_AT_MESSAGE_CREATE",
                    "d": {"id": "m-2", "content": " 你好", "group_openid": "G1",
                          "author": {"member_openid": "U2"}}});
                ws.send(Message::text(group.to_string())).await.unwrap();
                // Hold the socket open until the test is done with it.
                while ws.next().await.is_some() {}
            }
        });

        let fresh = Arc::new(Mutex::new(Vec::new()));
        let (tx, mut rx) = mpsc::channel(8);
        let driver = tokio::spawn({
            let fresh = fresh.clone();
            run(
                move |stale| {
                    fresh.lock().unwrap().push(stale);
                    let url = url.clone();
                    async move { Ok(("tok".to_owned(), url)) }
                },
                tx,
            )
        });
        let mut events = Vec::new();
        for _ in 0..3 {
            events.push(
                tokio::time::timeout(Duration::from_secs(10), rx.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        driver.abort();
        server.abort();

        assert!(matches!(&events[0], Event::Ready(user) if user["username"] == "薇薇"));
        assert!(matches!(&events[1], Event::Dispatch { kind, data }
            if kind == "C2C_MESSAGE_CREATE" && data["id"] == "m-1"));
        assert!(matches!(&events[2], Event::Dispatch { kind, data }
            if kind == "GROUP_AT_MESSAGE_CREATE" && data["id"] == "m-2"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0]["op"], 2);
        assert_eq!(seen[0]["d"]["token"], "QQBot tok");
        assert_eq!(
            seen[1],
            json!({"op": 1, "d": 2}),
            "the heartbeat carries the last seq"
        );
        assert_eq!(
            seen[2],
            json!({"op": 6, "d": {"token": "QQBot tok", "session_id": "s-1", "seq": 2}})
        );
        // A dropped socket kept the token.
        assert_eq!(*fresh.lock().unwrap(), [false, false]);
    }
}
