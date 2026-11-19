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
//! - [`home`] is where the core this machine runs for itself keeps
//!   everything, for whatever starts it.
//!
//! Adapters depend on this crate and nothing else from Ghost. They never see
//! her journal, her memory, or a brain.

pub mod presence;
pub mod socket;
pub mod text;

use anyhow::{Context, anyhow};
pub use ghost_protocol as protocol;
use ghost_protocol::{
    BrainAsk, BrainList, BrainModel, BrainModels, BrainRemove, BrainSave, BrainUse, ConsoleRewind,
    ConsoleRewound, ConsoleSaid, ConsoleSay, ConsoleStop, ConsoleStopped, Frame, Hello,
    JournalRead, ListGhosts, Look, LookRead, PROTOCOL, Role, Roster, Session, SessionDelete,
    SessionNew, SessionRename, StepsRead, Welcome, WorkAnswer, WorkLog, WorkStop, method,
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

/// Where the core this machine runs for itself keeps everything: its
/// configuration, its keys, its ghosts. `GHOST_HOME` if it is set; else
/// `~/Library/Application Support/Ghost` on macOS, and elsewhere
/// `$XDG_DATA_HOME/ghost`, or `~/.local/share/ghost`.
pub fn home() -> std::path::PathBuf {
    home_from(&|var| std::env::var_os(var), cfg!(target_os = "macos"))
}

/// [`home`], with the environment read through `var`.
fn home_from(var: &dyn Fn(&str) -> Option<std::ffi::OsString>, macos: bool) -> std::path::PathBuf {
    use std::path::PathBuf;
    let set = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(home) = set("GHOST_HOME") {
        return home;
    }
    let user = set("HOME").unwrap_or_else(|| PathBuf::from("."));
    if macos {
        return user.join("Library/Application Support/Ghost");
    }
    set("XDG_DATA_HOME")
        .unwrap_or_else(|| user.join(".local/share"))
        .join("ghost")
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

    /// A file she keeps in her `media/`, by the name her journal gives it:
    /// its type, and its bytes.
    pub async fn media(&mut self, name: &str) -> anyhow::Result<(String, Vec<u8>)> {
        use base64::Engine as _;
        let file: ghost_protocol::MediaFile = self
            .conn
            .call(
                method::MEDIA,
                &ghost_protocol::MediaRead {
                    name: name.to_owned(),
                },
            )
            .await?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.data)
            .with_context(|| format!("{name} did not come as base64"))?;
        Ok((file.mime, bytes))
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

    /// The steps the work `work`, or a part of it by its own id, has
    /// taken after the `after`th, at most `limit` of them, each with what
    /// it called and the head of what came back, and the parts it handed
    /// out. Ask again after the last step while
    /// [`more`](ghost_protocol::WorkLog::more) says there are more.
    pub async fn work_steps(
        &mut self,
        work: &str,
        after: Option<u64>,
        limit: Option<u32>,
    ) -> anyhow::Result<WorkLog> {
        let params = StepsRead {
            work: work.to_owned(),
            after,
            limit,
        };
        self.conn.call(method::WORK_STEPS, &params).await
    }

    /// What she called while she looked at `place`, round by round: in the
    /// looks that ended after the journal entry `after`, and in the one she
    /// is in the middle of there. Polled about once a second while she is
    /// thinking in a place, it follows her as she goes.
    pub async fn look_steps(
        &mut self,
        place: &str,
        after: Option<u64>,
    ) -> anyhow::Result<Vec<Look>> {
        let params = LookRead {
            place: place.to_owned(),
            after,
        };
        self.conn.call(method::LOOK_STEPS, &params).await
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
    pub async fn say(&mut self, place: Option<String>, text: &str) -> anyhow::Result<ConsoleSaid> {
        let params = ConsoleSay {
            place,
            text: text.to_owned(),
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

    /// Take back the line `to` (the seq of its `heard` entry) in the place
    /// `place` on the console channel, or in the one named after whoever
    /// is at the console, as an operator of hers: she is stopped there,
    /// and that line and everything said there after it leave the
    /// conversation in front of her. Returns how many lines did.
    pub async fn rewind_place(&mut self, place: Option<String>, to: i64) -> anyhow::Result<i64> {
        let rewound: ConsoleRewound = self
            .conn
            .call(method::REWIND, &ConsoleRewind { place, to })
            .await?;
        Ok(rewound.lines)
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

    /// Her sessions at a console, newest first: her conversations on the
    /// console channel, as every console lists them.
    pub async fn sessions(&mut self) -> anyhow::Result<Vec<Session>> {
        self.conn.call(method::SESSIONS, &()).await
    }

    /// Begin a session, as an operator: by the id `id` if it is free,
    /// called `title` if one is given. Returns it as the daemon made it.
    pub async fn new_session(
        &mut self,
        id: Option<String>,
        title: Option<String>,
    ) -> anyhow::Result<Session> {
        self.conn
            .call(method::SESSION_NEW, &SessionNew { id, title })
            .await
    }

    /// Call the session `id` `title`, as an operator: every console lists
    /// it so from now on.
    pub async fn rename_session(&mut self, id: &str, title: &str) -> anyhow::Result<()> {
        let params = SessionRename {
            id: id.to_owned(),
            title: title.to_owned(),
        };
        self.conn
            .call::<Value>(method::SESSION_RENAME, &params)
            .await?;
        Ok(())
    }

    /// Set the session `id` aside, as an operator: it leaves the list,
    /// and what was said in it stays in her journal.
    pub async fn delete_session(&mut self, id: &str) -> anyhow::Result<()> {
        let params = SessionDelete { id: id.to_owned() };
        self.conn
            .call::<Value>(method::SESSION_DELETE, &params)
            .await?;
        Ok(())
    }

    /// The brains the daemon is configured with, and which she thinks with.
    pub async fn brains(&mut self) -> anyhow::Result<BrainList> {
        self.conn.call(method::BRAINS, &()).await
    }

    /// Add a brain or change one, as an operator, giving its key if `save`
    /// carries one; what the brains are afterwards.
    pub async fn save_brain(&mut self, save: &BrainSave) -> anyhow::Result<BrainList> {
        self.conn.call(method::BRAIN_SAVE, save).await
    }

    /// Take the brain `name` out, as an operator.
    pub async fn remove_brain(&mut self, name: &str) -> anyhow::Result<BrainList> {
        let params = BrainRemove {
            name: name.to_owned(),
        };
        self.conn.call(method::BRAIN_REMOVE, &params).await
    }

    /// Have her think with the brain `name`, on `model` if given, as an
    /// operator, once it has shown it can.
    pub async fn use_brain(
        &mut self,
        name: &str,
        model: Option<String>,
    ) -> anyhow::Result<BrainList> {
        let params = BrainUse {
            name: name.to_owned(),
            model,
        };
        self.conn.call(method::BRAIN_USE, &params).await
    }

    /// The models the brain `ask` names offers, as the daemon finds them.
    pub async fn brain_models(&mut self, ask: &BrainAsk) -> anyhow::Result<Vec<BrainModel>> {
        let listed: BrainModels = self.conn.call(method::BRAIN_MODELS, ask).await?;
        Ok(listed.models)
    }

    /// Ask the brain `ask` names to think once; an error says why it could not.
    pub async fn check_brain(&mut self, ask: &BrainAsk) -> anyhow::Result<()> {
        self.conn.call::<Value>(method::BRAIN_CHECK, ask).await?;
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

#[cfg(test)]
mod home_tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::home_from;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |var| {
            pairs
                .iter()
                .find(|(k, _)| k == var)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn the_home_is_ghost_home_when_it_is_set_and_the_platforms_place_for_an_apps_data_otherwise() {
        let set = env(&[("GHOST_HOME", "/tmp/g"), ("HOME", "/Users/a")]);
        assert_eq!(home_from(&set, true), PathBuf::from("/tmp/g"));
        let unset = env(&[("GHOST_HOME", ""), ("HOME", "/Users/a")]);
        assert_eq!(
            home_from(&unset, true),
            PathBuf::from("/Users/a/Library/Application Support/Ghost")
        );
        assert_eq!(
            home_from(&unset, false),
            PathBuf::from("/Users/a/.local/share/ghost")
        );
        let xdg = env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(home_from(&xdg, false), PathBuf::from("/data/ghost"));
    }
}
