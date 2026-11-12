//! # ghost-link
//!
//! The client side of the Ghost Protocol.
//!
//! - [`presence::run`] is everything a channel adapter needs: implement
//!   [`presence::Platform`] for your platform, describe yourself in a
//!   [`presence::Link`], and it keeps a connection to the daemon alive,
//!   forwards what arrives, and routes her actions to you.
//! - [`Console`] is a small client for tools that only watch.
//!
//! Adapters depend on this crate and nothing else from Ghost. They never see
//! her journal, her memory, or a brain.

pub mod presence;
pub mod socket;

use anyhow::{Context, anyhow};
pub use ghost_protocol as protocol;
use ghost_protocol::{Frame, Hello, JournalRead, PROTOCOL, Role, Welcome, method};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The `ws://` URL of a daemon on this machine at the default port.
pub fn default_url() -> String {
    format!("ws://{}", ghost_protocol::DEFAULT_ADDR)
}

/// One connection to a daemon, after a successful hello.
pub struct Conn {
    ws: Socket,
    next_id: u64,
}

impl Conn {
    /// Connect and introduce ourselves.
    pub async fn open(url: &str, hello: &Hello) -> anyhow::Result<(Self, Welcome)> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .with_context(|| format!("connect to {url}"))?;
        let mut conn = Self { ws, next_id: 1 };
        let welcome: Welcome = conn.call(method::HELLO, hello).await?;
        Ok((conn, welcome))
    }

    pub async fn send(&mut self, frame: &Frame) -> anyhow::Result<()> {
        socket::send(&mut self.ws, frame).await
    }

    /// The next frame, or `None` once the daemon has closed the connection.
    pub async fn recv(&mut self) -> anyhow::Result<Option<Frame>> {
        socket::recv(&mut self.ws).await
    }

    /// Send a request and wait for its response. Anything else that arrives
    /// in between is dropped, so use this before subscribing to a stream.
    pub async fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &impl Serialize,
    ) -> anyhow::Result<T> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Frame::request(id, method, params)).await?;
        loop {
            let frame = self
                .recv()
                .await?
                .ok_or_else(|| anyhow!("the daemon closed the connection"))?;
            if frame.is_response() && frame.id == Some(id) {
                return Ok(frame.into_result()?);
            }
        }
    }
}

/// A client that watches a ghost. It can ask; it cannot change anything.
pub struct Console {
    conn: Conn,
}

impl Console {
    pub async fn connect(url: &str, token: Option<String>) -> anyhow::Result<(Self, Welcome)> {
        let hello = Hello {
            protocol: PROTOCOL,
            role: Role::Console,
            token,
            presence: None,
        };
        let (conn, welcome) = Conn::open(url, &hello).await?;
        Ok((Self { conn }, welcome))
    }

    /// Who she is and what she is in the middle of.
    pub async fn status(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::STATUS, &()).await
    }

    /// What she is going to do without being asked, and when.
    pub async fn agenda(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::AGENDA, &()).await
    }

    /// Everyone she knows and where she stands with them.
    pub async fn people(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::PEOPLE, &()).await
    }

    /// Journal entries starting at `from`. Negative counts back from the end.
    pub async fn journal(&mut self, from: i64, limit: u32) -> anyhow::Result<Vec<Value>> {
        self.conn
            .call(method::JOURNAL_READ, &JournalRead { from, limit })
            .await
    }

    /// Ask for every journal entry from now on. Read them with [`Self::next_entry`].
    pub async fn subscribe(&mut self) -> anyhow::Result<()> {
        self.conn
            .call::<Value>(method::JOURNAL_SUBSCRIBE, &())
            .await?;
        Ok(())
    }

    /// The next journal entry, once subscribed. `None` when the daemon is gone.
    pub async fn next_entry(&mut self) -> anyhow::Result<Option<Value>> {
        loop {
            let Some(frame) = self.conn.recv().await? else {
                return Ok(None);
            };
            if frame.method.as_deref() == Some(method::JOURNAL_ENTRY) {
                return Ok(Some(frame.params.unwrap_or(Value::Null)));
            }
        }
    }
}
