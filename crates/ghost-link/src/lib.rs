//! # ghost-link
//!
//! The client side of the Ghost Protocol.
//!
//! - [`presence::run`] is everything a channel adapter needs: implement
//!   [`presence::Platform`] for your platform, describe yourself in a
//!   [`presence::Link`], and it keeps a connection to the daemon alive,
//!   forwards what arrives, and routes her actions to you.
//! - [`Console`] is a small client for tools that watch her, and for whoever
//!   runs her to speak to her from.
//! - [`list`] asks a daemon who it runs, before saying hello to any of them.
//!
//! Adapters depend on this crate and nothing else from Ghost. They never see
//! her journal, her memory, or a brain.

pub mod presence;
pub mod socket;
pub mod text;

use anyhow::{Context, anyhow};
pub use ghost_protocol as protocol;
use ghost_protocol::{
    ConsoleSaid, ConsoleSay, ConsoleStop, ConsoleStopped, Frame, Hello, JournalRead, ListGhosts,
    PROTOCOL, Role, Roster, SayMode, Welcome, WorkAnswer, WorkStop, method,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The `ws://` URL of a daemon on this machine at the default port.
pub fn default_url() -> String {
    format!("ws://{}", ghost_protocol::DEFAULT_ADDR)
}

/// Who the daemon at `url` runs, asked on a connection of its own before
/// any hello: the way to learn which character a hello can name. `token` is
/// the daemon's, when it requires one.
pub async fn list(url: &str, token: Option<String>) -> anyhow::Result<Roster> {
    let mut conn = Conn::dial(url).await?;
    let roster = conn.call(method::LIST, &ListGhosts { token }).await;
    let _ = conn.ws.close(None).await;
    roster
}

/// One connection to a daemon, after a successful hello.
pub struct Conn {
    ws: Socket,
    next_id: u64,
}

impl Conn {
    /// Connect and introduce ourselves.
    pub async fn open(url: &str, hello: &Hello) -> anyhow::Result<(Self, Welcome)> {
        let mut conn = Self::dial(url).await?;
        let welcome: Welcome = conn.call(method::HELLO, hello).await?;
        Ok((conn, welcome))
    }

    /// Connect, and say nothing yet.
    async fn dial(url: &str) -> anyhow::Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .with_context(|| format!("connect to {url}"))?;
        Ok(Self { ws, next_id: 1 })
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

/// A client that watches a ghost, and through which whoever runs her can
/// speak to her. It cannot change who she is or what she has lived.
pub struct Console {
    conn: Conn,
}

impl Console {
    /// Connect to the character called `ghost`, the name of her directory.
    /// It may be `None` when the daemon runs only one.
    pub async fn connect(
        url: &str,
        token: Option<String>,
        ghost: Option<String>,
    ) -> anyhow::Result<(Self, Welcome)> {
        Self::connect_as(url, token, ghost, None).await
    }

    /// The same, saying who is at the console, which it has to before it
    /// can [`say`](Self::say) anything to her.
    pub async fn connect_as(
        url: &str,
        token: Option<String>,
        ghost: Option<String>,
        who: Option<String>,
    ) -> anyhow::Result<(Self, Welcome)> {
        let hello = Hello {
            protocol: PROTOCOL,
            role: Role::Console,
            token,
            ghost,
            presence: None,
            who,
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

    /// Her persona as it stands.
    pub async fn identity(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::IDENTITY, &()).await
    }

    /// Everyone the daemon runs, her among them.
    pub async fn list(&mut self) -> anyhow::Result<Roster> {
        self.conn.call(method::LIST, &ListGhosts::default()).await
    }

    /// Her picture, if she has one.
    pub async fn avatar(&mut self) -> anyhow::Result<Option<ghost_protocol::Avatar>> {
        self.conn.call(method::AVATAR, &()).await
    }

    /// What is true about her life, oldest first.
    pub async fn canon(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::CANON, &()).await
    }

    /// Every place she is in, most recently active first.
    pub async fn places(&mut self) -> anyhow::Result<Value> {
        self.conn.call(method::PLACES, &()).await
    }

    /// What she has in hand and what ended lately, most recently active
    /// first, each with the parts it handed out.
    pub async fn work(&mut self) -> anyhow::Result<Vec<ghost_protocol::Work>> {
        self.conn.call(method::WORK, &()).await
    }

    /// What is waiting on whoever runs her, newest first: work waiting on
    /// an answer, work that ended blocked or given up lately, and what was
    /// told and reported to them lately.
    pub async fn attention(&mut self) -> anyhow::Result<Vec<ghost_protocol::Attention>> {
        self.conn.call(method::ATTENTION, &()).await
    }

    /// The journal entries that belong to one place, oldest first.
    pub async fn thread(&mut self, place: &str, limit: u32) -> anyhow::Result<Vec<Value>> {
        let params = ghost_protocol::PlaceRead {
            place: place.to_owned(),
            limit,
        };
        self.conn.call(method::JOURNAL_PLACE, &params).await
    }

    /// Journal entries starting at `from`. Negative counts back from the end.
    pub async fn journal(&mut self, from: i64, limit: u32) -> anyhow::Result<Vec<Value>> {
        self.conn
            .call(method::JOURNAL_READ, &JournalRead { from, limit })
            .await
    }

    /// Say `text` to her, as an operator of hers, in the place `place` on
    /// the console channel, or in the one named after whoever is at the
    /// console. What she says back is in her journal: read it from
    /// [`Self::next_entry`] on another connection, or with [`Self::thread`].
    pub async fn say(
        &mut self,
        place: Option<String>,
        text: &str,
        mode: SayMode,
    ) -> anyhow::Result<ConsoleSaid> {
        let params = ConsoleSay {
            place,
            text: text.to_owned(),
            mode,
        };
        self.conn.call(method::SAY, &params).await
    }

    /// Stop her in the place `place` on the console channel, or in the one
    /// named after whoever is at the console, as an operator of hers: what
    /// she is thinking and about to say there is called off, and the work
    /// she took on there is stopped. Returns whether anything was.
    pub async fn stop_place(&mut self, place: Option<String>) -> anyhow::Result<bool> {
        let stopped: ConsoleStopped = self.conn.call(method::STOP, &ConsoleStop { place }).await?;
        Ok(stopped.stopped)
    }

    /// Answer what the work `work` asked, or add to what it was asked, as
    /// an operator of hers. Say who is at the console first, with
    /// [`Self::connect_as`].
    pub async fn answer(&mut self, work: &str, text: &str) -> anyhow::Result<()> {
        let params = WorkAnswer {
            work: work.to_owned(),
            text: text.to_owned(),
        };
        self.conn
            .call::<Value>(method::WORK_ANSWER, &params)
            .await?;
        Ok(())
    }

    /// Stop the work `work`, as an operator of hers.
    pub async fn stop(&mut self, work: &str) -> anyhow::Result<()> {
        let params = WorkStop {
            work: work.to_owned(),
        };
        self.conn.call::<Value>(method::WORK_STOP, &params).await?;
        Ok(())
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
