//! One WebSocket to a OneBot v11 implementation, kept alive.
//!
//! OneBot multiplexes two things over the socket: events it pushes, and
//! answers to actions we send, matched by an `echo` string. A background task
//! owns the socket, reconnects with backoff, and routes each.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

/// How long an action may take before we stop waiting for its answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(15);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>;

pub struct Wire {
    out: mpsc::Sender<String>,
    pending: Pending,
    next: AtomicU64,
}

impl Wire {
    /// Start talking to `endpoint`. Events arrive on the returned receiver for
    /// as long as the `Wire` lives.
    pub fn open(endpoint: String, access_token: Option<String>) -> (Self, mpsc::Receiver<Value>) {
        let (out, outgoing) = mpsc::channel(64);
        let (events_tx, events) = mpsc::channel(1024);
        let pending = Pending::default();
        tokio::spawn(keep_alive(
            endpoint,
            access_token,
            outgoing,
            events_tx,
            pending.clone(),
        ));
        (
            Self {
                out,
                pending,
                next: AtomicU64::new(1),
            },
            events,
        )
    }

    /// Call a OneBot action and return its `data`.
    pub async fn call(&self, action: &str, params: Value) -> anyhow::Result<Value> {
        let echo = format!("ghost-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|e| anyhow!("{e}"))?
            .insert(echo.clone(), tx);
        let forget = || {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&echo);
            }
        };
        let request = json!({"action": action, "params": params, "echo": echo});
        if self.out.send(request.to_string()).await.is_err() {
            forget();
            bail!("the onebot connection is gone");
        }
        let answer = match tokio::time::timeout(ANSWER_WITHIN, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => bail!("onebot disconnected before answering {action}"),
            Err(_) => {
                forget();
                bail!("onebot did not answer {action} in time");
            }
        };
        if answer["status"] == "ok" || answer["retcode"] == 0 {
            return Ok(answer["data"].clone());
        }
        let reason = answer["wording"]
            .as_str()
            .or_else(|| answer["message"].as_str())
            .or_else(|| answer["msg"].as_str())
            .unwrap_or("no reason given");
        bail!(
            "onebot refused {action}: {reason} (retcode {})",
            answer["retcode"]
        )
    }
}

async fn keep_alive(
    endpoint: String,
    access_token: Option<String>,
    mut outgoing: mpsc::Receiver<String>,
    events: mpsc::Sender<Value>,
    pending: Pending,
) {
    let mut retry = Duration::from_secs(1);
    loop {
        match connect(&endpoint, access_token.as_deref()).await {
            Ok(socket) => {
                info!("connected to onebot at {endpoint}");
                retry = Duration::from_secs(1);
                let (mut sink, mut stream) = socket.split();
                loop {
                    tokio::select! {
                        request = outgoing.recv() => match request {
                            Some(text) => {
                                if sink.send(Message::text(text)).await.is_err() {
                                    break;
                                }
                            }
                            // The adapter is shutting down.
                            None => return,
                        },
                        frame = stream.next() => match frame {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
                                    route(value, &events, &pending).await;
                                }
                            }
                            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                            Some(Ok(_)) => {}
                        },
                    }
                }
                warn!("lost onebot; reconnecting");
                // Whoever was waiting on an answer will not get one.
                if let Ok(mut pending) = pending.lock() {
                    pending.clear();
                }
            }
            Err(e) => warn!("cannot reach onebot: {e:#}"),
        }
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(30));
    }
}

async fn connect(
    endpoint: &str,
    access_token: Option<&str>,
) -> anyhow::Result<ghost_net::WebSocket> {
    let authorization = access_token.map(|token| format!("Bearer {token}"));
    ghost_net::websocket(endpoint, authorization.as_deref()).await
}

/// An answer goes to whoever asked; anything else with a `post_type` is an event.
async fn route(value: Value, events: &mpsc::Sender<Value>, pending: &Pending) {
    if let Some(echo) = value["echo"].as_str() {
        let waiter = pending.lock().ok().and_then(|mut p| p.remove(echo));
        if let Some(waiter) = waiter {
            let _ = waiter.send(value);
        }
        return;
    }
    if value.get("post_type").is_some() {
        let _ = events.send(value).await;
    }
}
