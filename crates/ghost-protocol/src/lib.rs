//! # ghost-protocol
//!
//! The Ghost Protocol: JSON-RPC 2.0 over a WebSocket, one JSON object per
//! text frame. The daemon is the server. Two kinds of client connect to it:
//!
//! - a **presence** carries her onto one channel (Telegram, Discord, a
//!   terminal). It reports what arrives with [`method::INBOUND`] and carries
//!   out what she does when the daemon sends it [`method::ACT`].
//! - a **console** watches, and is how whoever runs her speaks to her. It
//!   can read her status, her journal, her agenda and the work she has in
//!   hand, and say something to her with [`method::SAY`]. The protocol has
//!   no method that edits her identity or removes a journal entry; that is
//!   not a permission a console lacks, it is a method that does not exist.
//!
//! A console the daemon admits is its owner's: it holds the daemon's token,
//! or the daemon has none, which it allows only while it listens on this
//! machine alone. So whoever is at a console is an operator of every
//! character the daemon runs, and what they say with [`method::SAY`] is
//! said by one of the people who run her.
//!
//! A connection begins with one [`method::HELLO`] request. Everything after
//! depends on the role declared in it. The one thing that may come before
//! it is [`method::LIST`], which asks who the daemon runs: a client has to
//! know that to say which of them its hello is for.
//!
//! One daemon can run several characters. A client is for one of them: its
//! hello names her in [`Hello::ghost`], and everything on that connection
//! goes to her alone. A daemon running one character takes a hello that
//! names nobody, so clients from before there could be several still work.
//!
//! These types are deliberately separate from the journal's own. The journal
//! format is forever; this one is versioned by [`PROTOCOL`] and will change.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Version of the protocol spoken by this crate.
pub const PROTOCOL: u32 = 1;

/// Where a daemon listens unless told otherwise.
pub const DEFAULT_ADDR: &str = "127.0.0.1:7437";

/// Method names.
pub mod method {
    /// Client → daemon request. Params [`Hello`](super::Hello), result
    /// [`Welcome`](super::Welcome). Must be the first frame, but for any
    /// [`LIST`] before it.
    pub const HELLO: &str = "hello";
    /// Client → daemon request. Params [`ListGhosts`](super::ListGhosts),
    /// result [`Roster`](super::Roster): every character the daemon runs.
    /// It may be sent before the hello, as often as a client likes, and on
    /// a console after it. Before the hello it carries the token itself.
    pub const LIST: &str = "ghost.list";

    /// Presence → daemon notification. Params [`Inbound`](super::Inbound).
    pub const INBOUND: &str = "presence.inbound";
    /// Presence → daemon notification. Params [`Typing`](super::Typing).
    pub const TYPING: &str = "presence.typing";
    /// Daemon → presence request. Params [`Action`](super::Action), result
    /// [`Acted`](super::Acted).
    pub const ACT: &str = "presence.act";
    /// Daemon → presence notification. No params. A newer presence has
    /// registered this channel; this one has been replaced and should stop.
    pub const DISPLACED: &str = "presence.displaced";

    /// Console → daemon request. No params. Result: who she is and what
    /// she is in the middle of, as a JSON object; among it, what she can do
    /// besides talk, under `abilities`, as [`Abilities`](super::Abilities),
    /// and where her daemon runs, under `core`, as
    /// [`CoreInfo`](super::CoreInfo).
    pub const STATUS: &str = "ghost.status";
    /// Console → daemon request. No params.
    pub const AGENDA: &str = "mind.agenda";
    /// Console → daemon request. No params.
    pub const PEOPLE: &str = "memory.people";
    /// Console → daemon request. No params. Her persona as it stands.
    pub const IDENTITY: &str = "ghost.identity";
    /// Client → daemon request, from a console or a presence. No params.
    /// Result: [`Avatar`](super::Avatar), or `null` when she has none. Her
    /// picture is asked for apart from the hello because it can be large,
    /// and only a client that shows it or sets a platform's profile picture
    /// needs it.
    pub const AVATAR: &str = "ghost.avatar";
    /// Console → daemon request. Params [`MediaRead`](super::MediaRead),
    /// result [`MediaFile`](super::MediaFile): a file she keeps in her
    /// `media/`, by the name her journal and her work give it, which is
    /// the hash of its bytes. It is how a console shows her images
    /// wherever her daemon runs: what she saw, sent, opened and kept. A
    /// name that is not one she keeps a file by, or not a bare name at
    /// all, is refused with [`NO_SUCH_MEDIA`](super::code::NO_SUCH_MEDIA);
    /// nothing outside `media/` is ever handed out. Any console of hers
    /// may ask, as any may read the journal that names them.
    pub const MEDIA: &str = "media.get";
    /// Console → daemon request. No params. What is true about her life.
    pub const CANON: &str = "memory.canon";
    /// Console → daemon request. No params. Every place she is in.
    pub const PLACES: &str = "memory.places";
    /// Console → daemon request. Params [`PlaceRead`](super::PlaceRead). The
    /// journal entries that belong to one place: what was said there and
    /// what she made of it.
    pub const JOURNAL_PLACE: &str = "journal.place";
    /// Console → daemon request. No params. Result: a list of
    /// [`Work`](super::Work), what she has in hand and what ended lately,
    /// most recently active first, each with its parts.
    pub const WORK: &str = "work.list";
    /// Console → daemon request. Params [`StepsRead`](super::StepsRead),
    /// result [`WorkLog`](super::WorkLog): the steps one piece of work, or
    /// one part of it, has taken, each with what it called and the head of
    /// what came back, and the parts it handed out. An id she has no work
    /// by is refused with [`NO_SUCH_WORK`](super::code::NO_SUCH_WORK).
    pub const WORK_STEPS: &str = "work.steps";
    /// Console → daemon request. Params [`LookRead`](super::LookRead),
    /// result: a list of [`Look`](super::Look), oldest first. What she
    /// called while she looked at a place, and what came back, round by
    /// round: the looks that ended after a journal entry, and the one she
    /// is in the middle of. Kept for a fortnight.
    pub const LOOK_STEPS: &str = "look.steps";
    /// Console → daemon request. Params [`JournalRead`](super::JournalRead).
    pub const JOURNAL_READ: &str = "journal.read";
    /// Console → daemon request. No params. Afterwards the daemon sends a
    /// [`JOURNAL_ENTRY`] notification for every entry as it is written.
    pub const JOURNAL_SUBSCRIBE: &str = "journal.subscribe";
    /// Daemon → console notification. Params: one journal entry.
    pub const JOURNAL_ENTRY: &str = "journal.entry";
    /// Console → daemon request. Params [`ConsoleSay`](super::ConsoleSay),
    /// result [`ConsoleSaid`](super::ConsoleSaid). Whoever is at the
    /// console, as its hello names them in [`Hello::who`](super::Hello::who),
    /// says something to her, as an operator of hers, in a place on the
    /// channel [`CONSOLE`](super::CONSOLE), where every line is a
    /// directive (see [`SayMode`](super::SayMode)). What she says back
    /// there is written in her journal and nowhere else: a console reads
    /// it as a `said` entry, from [`JOURNAL_SUBSCRIBE`] or [`JOURNAL_PLACE`].
    pub const SAY: &str = "console.say";
    /// Console → daemon request. Params [`ConsoleStop`](super::ConsoleStop),
    /// result [`ConsoleStopped`](super::ConsoleStopped). An operator at the
    /// console stops her in a place on the channel
    /// [`CONSOLE`](super::CONSOLE), as one stops a person mid-sentence:
    /// what she is thinking there is abandoned, nothing she has not yet
    /// said there is said, what is unread there is left read, and the work
    /// she took on there is stopped as [`WORK_STOP`] stops it. Allowed as
    /// [`SAY`] is.
    pub const STOP: &str = "console.stop";
    /// Console → daemon request. Params [`ConsoleRewind`](super::ConsoleRewind),
    /// result [`ConsoleRewound`](super::ConsoleRewound). An operator at the
    /// console takes back a line of theirs in a place on the channel
    /// [`CONSOLE`](super::CONSOLE), to put it another way: as one rewinds
    /// a coding agent to before a turn. She is stopped there as [`STOP`]
    /// stops her, and from that line on, what was said there is out of
    /// the conversation in front of her; her journal keeps every word,
    /// and says so in a `rewound` entry. The line must be one of theirs
    /// in that place, or the request is refused with
    /// [`NO_SUCH_LINE`](super::code::NO_SUCH_LINE). Allowed as [`SAY`] is.
    pub const REWIND: &str = "console.rewind";
    /// Console → daemon request. Params [`WorkAnswer`](super::WorkAnswer),
    /// result `{}`. An operator at the console answers what a piece of work
    /// of hers asked, or adds to what it was asked, as she would pass it
    /// on: it reaches the work before its next step, and her journal says
    /// it was theirs. Her open work only, by its id; anything else is
    /// refused with [`NO_SUCH_WORK`](super::code::NO_SUCH_WORK).
    pub const WORK_ANSWER: &str = "work.answer";
    /// Console → daemon request. Params [`WorkStop`](super::WorkStop),
    /// result `{}`. An operator at the console stops a piece of work of
    /// hers, as she would: a sitting at it ends after the step it is on.
    /// Refused as [`WORK_ANSWER`] is.
    pub const WORK_STOP: &str = "work.stop";
    /// Console → daemon request. No params. Result: a list of
    /// [`Attention`](super::Attention), newest first: what is waiting on
    /// whoever runs her. Read-only; which of it a client has shown is the
    /// client's to remember, by each item's id.
    pub const ATTENTION: &str = "attention.list";

    /// Console → daemon request. No params. Result
    /// [`BrainList`](super::BrainList): the brains the daemon is configured
    /// with, which of them she thinks and ticks with, and whether it takes
    /// changes to them. The brains are the daemon's, shared by every
    /// character it runs; a key is never in the answer, only whether one
    /// is set.
    pub const BRAINS: &str = "brains.list";
    /// Console → daemon request. Params [`BrainSave`](super::BrainSave),
    /// result [`BrainList`](super::BrainList). An operator adds a brain or
    /// changes one, and may give the key it reads, which the daemon keeps
    /// in its home's `.env` and never in its configuration. A change to a
    /// brain in use takes effect at once. Refused with
    /// [`UNCHANGEABLE`](super::code::UNCHANGEABLE) by a daemon that runs
    /// no home.
    pub const BRAIN_SAVE: &str = "brains.save";
    /// Console → daemon request. Params [`BrainRemove`](super::BrainRemove),
    /// result [`BrainList`](super::BrainList). An operator takes a brain
    /// out; the one in use stays. Refused as [`BRAIN_SAVE`] is.
    pub const BRAIN_REMOVE: &str = "brains.remove";
    /// Console → daemon request. Params [`BrainUse`](super::BrainUse),
    /// result [`BrainList`](super::BrainList). An operator has her think
    /// and tick with a brain, on a model of its own: the daemon first asks
    /// it to think once, refuses with
    /// [`BRAIN_FAILED`](super::code::BRAIN_FAILED) and why if it cannot,
    /// and otherwise starts every mind it runs again on it. Refused as
    /// [`BRAIN_SAVE`] is.
    pub const BRAIN_USE: &str = "brains.use";
    /// Console → daemon request. Params [`BrainAsk`](super::BrainAsk),
    /// result [`BrainModels`](super::BrainModels): the models an API's own
    /// listing offers, asked by the daemon, with the key it holds. Only an
    /// operator may ask, since it spends the key.
    pub const BRAIN_MODELS: &str = "brains.models";
    /// Console → daemon request. Params [`BrainAsk`](super::BrainAsk),
    /// result `{}`: the brain was asked to think once, as little as there
    /// is, the way her mind would ask it, and answered. If it did not, the
    /// request is refused with [`BRAIN_FAILED`](super::code::BRAIN_FAILED)
    /// and what it said. Only an operator may ask.
    pub const BRAIN_CHECK: &str = "brains.check";
}

/// The channel a console speaks to her on. It is the daemon's own: no
/// presence can register it, so everything said on it came from a console
/// the daemon admitted, and what she says on it goes into her journal for
/// any console to read.
pub const CONSOLE: &str = "console";

/// JSON-RPC error codes used by the daemon.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// The token was missing or wrong.
    pub const UNAUTHORIZED: i64 = -32001;
    /// The client speaks a protocol version the daemon does not.
    pub const VERSION: i64 = -32002;
    /// Unused since the newest presence for a channel replaces the older one.
    pub const CHANNEL_TAKEN: i64 = -32003;
    /// The hello named a character this daemon does not run, or named
    /// nobody when it runs several. Retrying cannot fix it.
    pub const NO_SUCH_GHOST: i64 = -32004;
    /// The work named is not one she has open: there is none by that id,
    /// it is a part, or it has ended. Asked for the steps of work, there
    /// is none by that id.
    pub const NO_SUCH_WORK: i64 = -32005;
    /// The line named is not one the operator said in that place: there
    /// is no such entry, it is hers, or it was said elsewhere.
    pub const NO_SUCH_LINE: i64 = -32006;
    /// The daemon does not change its brains when asked: it runs no home,
    /// so its configuration is a file it was given rather than its own.
    pub const UNCHANGEABLE: i64 = -32007;
    /// There is no brain by that name.
    pub const NO_SUCH_BRAIN: i64 = -32008;
    /// The brain's key is not set: neither in the daemon's environment nor
    /// in its home's `.env`.
    pub const NO_KEY: i64 = -32009;
    /// The brain was asked, and could not answer: the message says what it
    /// said.
    pub const BRAIN_FAILED: i64 = -32011;
    /// The presence could not carry out an action.
    pub const ACT_FAILED: i64 = -32010;
    /// She keeps no file by that name in her `media/`: there is none, or
    /// the name is not a bare name of one.
    pub const NO_SUCH_MEDIA: i64 = -32012;
}

/// One JSON-RPC message: a request, a notification, or a response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Present on every successful response, even when what it carries is
    /// `null`: a response that answers "nothing" is still an answer.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A field that is there, `null` included. Serde's default reads `null` as
/// absent, which would make a response whose result is `null` look like no
/// response at all.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

impl Frame {
    fn base() -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            ..Self::default()
        }
    }

    /// A request: expects a response carrying the same `id`.
    pub fn request(id: u64, method: &str, params: &impl Serialize) -> Self {
        Self {
            id: Some(id),
            method: Some(method.to_owned()),
            params: serde_json::to_value(params).ok().filter(|v| !v.is_null()),
            ..Self::base()
        }
    }

    /// A notification: expects nothing back.
    pub fn notify(method: &str, params: &impl Serialize) -> Self {
        Self {
            method: Some(method.to_owned()),
            params: serde_json::to_value(params).ok().filter(|v| !v.is_null()),
            ..Self::base()
        }
    }

    /// A successful response to request `id`.
    pub fn ok(id: u64, result: &impl Serialize) -> Self {
        Self {
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            ..Self::base()
        }
    }

    /// A failed response to request `id`.
    pub fn err(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            id: Some(id),
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
            ..Self::base()
        }
    }

    pub fn is_response(&self) -> bool {
        self.method.is_none() && (self.result.is_some() || self.error.is_some())
    }

    /// Decode this frame's params.
    pub fn params<T: DeserializeOwned>(&self) -> Result<T, RpcError> {
        serde_json::from_value(self.params.clone().unwrap_or(Value::Null)).map_err(|e| RpcError {
            code: code::INVALID_PARAMS,
            message: e.to_string(),
        })
    }

    /// Decode this response's result, or surface its error.
    pub fn into_result<T: DeserializeOwned>(self) -> Result<T, RpcError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        serde_json::from_value(self.result.unwrap_or(Value::Null)).map_err(|e| RpcError {
            code: code::INTERNAL,
            message: e.to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Presence,
    Console,
}

/// The first thing a client says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Which character this client is for: the name of her directory, such
    /// as `weiwei`. It may be left out when the daemon runs only one; when
    /// it runs several, a hello without it is refused. It is here rather
    /// than in [`PresenceHello`] because a console is for one character as
    /// much as a presence is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghost: Option<String>,
    /// Required when `role` is `presence`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceHello>,
    /// For a console that speaks to her: who is at it, by name, as she is
    /// to know them. She knows them as `console:` and the name in lower
    /// case. A console that only watches, and a presence, leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
}

/// What a presence declares about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceHello {
    /// The name of the channel this presence carries, e.g. `telegram`. One
    /// presence per channel name.
    pub channel: String,
    /// Who she is on this channel, once the adapter has logged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub me: Option<Actor>,
    /// What this channel can do.
    pub can: Vec<Capability>,
    /// What the platform allows her, beyond what it can do. Left out, no
    /// limits.
    #[serde(default)]
    pub allows: Allows,
}

/// What a platform allows her: how long she has to answer what is written
/// to her, and whether she may speak first. Some platforms limit a bot in
/// both (QQ's bot platform does), and what she does has to fit: an answer
/// put off past the limit, or a message to someone who has not just
/// written, is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allows {
    /// Seconds after a message reaches her in a private chat within which
    /// she can still answer it, where the platform limits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_direct_secs: Option<u64>,
    /// The same in a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_group_secs: Option<u64>,
    /// Whether she may write to someone who has not just written to her.
    #[serde(default = "yes")]
    pub writes_first: bool,
}

impl Default for Allows {
    fn default() -> Self {
        Self {
            answer_direct_secs: None,
            answer_group_secs: None,
            writes_first: true,
        }
    }
}

fn yes() -> bool {
    true
}

/// The daemon's answer to [`Hello`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol: u32,
    pub ghost: GhostCard,
}

/// Who a daemon is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GhostCard {
    pub name: String,
    pub pronouns: String,
    pub city: String,
    pub country: String,
    /// The seal of her persona.
    pub seal: String,
    /// A line or two about her for the profile of an account she has, in
    /// her own words, if her persona has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
}

/// Params of [`method::LIST`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListGhosts {
    /// The daemon's token, when it requires one. Asked before a hello, the
    /// question has no hello to carry it; after one, it is not looked at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The result of [`method::LIST`]: who a daemon runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roster {
    /// The protocol the daemon speaks, so that a client knows before its
    /// hello whether the two of them can talk.
    pub protocol: u32,
    /// In the order the daemon runs them.
    pub ghosts: Vec<Listed>,
}

/// One character in a [`Roster`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    /// The name of her directory: what a hello names her by.
    pub id: String,
    /// Who she is, as a hello naming her is told.
    #[serde(flatten)]
    pub card: GhostCard,
    /// Whether her day has her awake now. `None` when her mind did not
    /// answer, as while it is being started again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awake: Option<bool>,
    /// What her day has her doing now, in her plan's words, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
    /// Whether she has a picture to ask for with [`method::AVATAR`].
    #[serde(default)]
    pub avatar: bool,
}

/// Her picture, the result of [`method::AVATAR`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Avatar {
    /// Its type, such as `image/png`.
    pub mime: String,
    /// The file's bytes, base64.
    pub data: String,
}

/// The params of [`method::MEDIA`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRead {
    /// The file's name in her `media/`, as her journal gives it: the
    /// BLAKE3 hash of its bytes, and an extension for its type.
    pub name: String,
}

/// A file she keeps, the result of [`method::MEDIA`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaFile {
    /// Its type, such as `image/png`, as its name says it.
    pub mime: String,
    /// The file's bytes, base64.
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Send a message to a place.
    Send,
    /// Show that she is typing.
    Typing,
    /// React to a message.
    React,
    /// Mark a place as read.
    Read,
    /// Post publicly, to no one in particular.
    Post,
    /// Fetch a timeline.
    Feed,
    /// Report how her posts are doing and how many follow her.
    Metrics,
    /// Take a file and deliver it to a place.
    Files,
}

/// Someone on a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// Stable identifier on the channel.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The channel marks this account as a bot.
    #[serde(default)]
    pub automated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// A private conversation with one person.
    Direct,
    /// A room with several people.
    Group,
    /// A public thread anyone can read.
    Public,
}

/// Somewhere a conversation happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub kind: PlaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A message that arrived on the channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inbound {
    pub place: Place,
    pub from: Actor,
    /// The channel's own id for the message. Used to drop redeliveries.
    pub id: String,
    /// Plain text. Media is described in brackets, e.g. `[photo]`.
    pub text: String,
    /// The id of the message this one replies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// It was meant for her: a direct message, a mention, a reply to her.
    #[serde(default)]
    pub addressed: bool,
    /// The images in the message, in the order their `[image]` placeholders
    /// appear in `text`. The text keeps the placeholders, so a daemon that
    /// cannot look at an image still knows one was sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
}

/// An image in a message, where the channel keeps it. A platform's links
/// expire, so a daemon that wants the image fetches it soon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub url: String,
    /// The type the channel says it is, e.g. `image/png`, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

/// Someone is typing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typing {
    /// Place id.
    pub place: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feed {
    Timeline,
    Mentions,
}

/// Something she does on the channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    Send {
        place: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
    Typing {
        place: String,
    },
    React {
        place: String,
        to: String,
        emoji: String,
    },
    Read {
        place: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upto: Option<String>,
    },
    Post {
        text: String,
    },
    Fetch {
        feed: Feed,
        limit: u32,
    },
    /// How are her recent posts doing? Answered in [`Acted::measure`].
    Measure,
    /// Hand over a file, with a line to go with it if she has one.
    File {
        place: String,
        name: String,
        /// The file's bytes, base64.
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
}

impl Action {
    /// The capability a presence needs in order to carry this out.
    pub fn needs(&self) -> Capability {
        match self {
            Self::Send { .. } => Capability::Send,
            Self::Typing { .. } => Capability::Typing,
            Self::React { .. } => Capability::React,
            Self::Read { .. } => Capability::Read,
            Self::Post { .. } => Capability::Post,
            Self::Fetch { .. } => Capability::Feed,
            Self::Measure => Capability::Metrics,
            Self::File { .. } => Capability::Files,
        }
    }
}

/// What came of an action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acted {
    /// The channel's id for whatever was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What a `fetch` brought back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Inbound>,
    /// What a `measure` brought back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<Measure>,
}

/// Her public standing on a channel, as the platform counts it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followers: Option<u64>,
    /// Her recent posts, newest first.
    #[serde(default)]
    pub posts: Vec<PostStats>,
}

/// How one post is doing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostStats {
    /// The platform's id for the post.
    pub id: String,
    #[serde(default)]
    pub likes: u64,
    #[serde(default)]
    pub replies: u64,
    #[serde(default)]
    pub reposts: u64,
    #[serde(default)]
    pub views: u64,
}

/// Params of [`method::JOURNAL_READ`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalRead {
    /// First seq to return. Zero or less counts back from the newest entry.
    pub from: i64,
    pub limit: u32,
}

/// Params of [`method::JOURNAL_PLACE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceRead {
    /// A place key, `channel:id`.
    pub place: String,
    pub limit: u32,
}

/// Params of [`method::SAY`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleSay {
    /// The place on the [`CONSOLE`] channel to say it in: its id, without
    /// the channel. Each place is a conversation of its own, private to
    /// whoever is at the console. Left out, it is the one named after them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    pub text: String,
    #[serde(default)]
    pub mode: SayMode,
}

/// How something said from a console asked to reach her, once. It no
/// longer changes anything: the [`CONSOLE`] channel is a terminal, where
/// whoever is at it runs her, and every line there is looked at and
/// answered at once, whatever her day. The field stays because the wire
/// format does, and clients send it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SayMode {
    /// The default.
    #[default]
    Say,
    /// Taken as `say`.
    Directive,
}

/// The result of [`method::SAY`]: where it was said, and the id it was
/// given, as her journal's `heard` entry has them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleSaid {
    /// The place key, `console:<id>`, to read the conversation with
    /// [`method::JOURNAL_PLACE`].
    pub place: String,
    /// The message's id.
    pub id: String,
}

/// Params of [`method::STOP`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleStop {
    /// The place on the [`CONSOLE`] channel to stop her in, as for
    /// [`ConsoleSay::place`]. Left out, it is the one named after whoever
    /// is at the console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
}

/// Params of [`method::REWIND`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRewind {
    /// The place on the [`CONSOLE`] channel the line was said in, as for
    /// [`ConsoleSay::place`]. Left out, it is the one named after whoever
    /// is at the console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// The seq of the journal's `heard` entry for the line taken back.
    pub to: i64,
}

/// The result of [`method::REWIND`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRewound {
    /// How many lines left the conversation: the one taken back and all
    /// said there after it, hers among them.
    pub lines: i64,
}

/// The result of [`method::STOP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleStopped {
    /// Whether anything was stopped: a look she was in the middle of or
    /// about to take, something she had not yet said, or work. False when
    /// she was doing nothing there, which is not an error.
    pub stopped: bool,
}

/// A piece of work she took on, or a part of one, as it stands: the result
/// of [`method::WORK`].
///
/// Her journal says she took it on, what it told her and how it ended. Its
/// steps, its plan and its parts are kept only where the work keeps its
/// own record, and are read from there for whoever asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    /// Its id. A part's is its parent's, a slash, and its number.
    pub id: String,
    /// The number she knows it by: the seq of the journal entry that began
    /// it. A part has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    /// What was asked, as it was asked.
    pub brief: String,
    pub state: WorkState,
    /// Steps taken so far, over all its sittings.
    #[serde(default)]
    pub steps: u64,
    /// When it began, RFC 3339. A part waiting its turn has not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    /// When anything last happened in it, RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// What it asked whoever it is for, while it waits to hear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// What answering the question takes, when the work said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AskKind>,
    /// For a question of kind `choose`, what to choose from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// How it ended, in its own words, once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Its plan, if it made one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<WorkStep>,
    /// The parts it handed out, by number. A part hands out none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Work>,
    /// What steps it: the installed agent it was handed to, by the name it
    /// is configured as (`claude-code`, `codex`), or the label of the model
    /// that stepped it last (`openai/deepseek-chat`). None before it has
    /// begun, or from a daemon from before it was told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<String>,
}

/// What answering a piece of work's question takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    /// Saying something: a fact, a preference, a yes or no.
    Answer,
    /// Giving it a file only they have.
    File,
    /// Doing something only they can, such as signing in, and saying when
    /// it is done.
    Act,
    /// Picking one of [`Work::options`].
    Choose,
    /// A kind this client does not know, from a newer daemon.
    #[serde(other)]
    Other,
}

/// Params of [`method::WORK_ANSWER`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAnswer {
    /// The work's id, as [`Work::id`] has it. A part is answered by the
    /// work it is part of, not from a console.
    pub work: String,
    /// The answer, or what is to be added to what it was asked.
    pub text: String,
}

/// Params of [`method::WORK_STOP`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkStop {
    /// The work's id, as [`Work::id`] has it.
    pub work: String,
}

/// Params of [`method::WORK_STEPS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepsRead {
    /// The work's id, as [`Work::id`] has it. A part's steps are asked for
    /// by the part's own id.
    pub work: String,
    /// Only the steps after this one, by [`LoggedStep::n`]. Left out, from
    /// the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
    /// At most this many steps. Left out, 100; never more than 500.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// The result of [`method::WORK_STEPS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkLog {
    /// Oldest first.
    pub steps: Vec<LoggedStep>,
    /// Every part the work handed out, by number, whatever steps were
    /// asked for: each one's steps are read by its own id.
    #[serde(default)]
    pub parts: Vec<WorkPart>,
    /// There are steps after the last of these: ask again after it.
    #[serde(default)]
    pub more: bool,
    /// What the newest step of the work put in front of its model,
    /// whatever steps were asked for; none before its first step, from a
    /// daemon from before it was kept, or for steps it did not take itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<WorkContext>,
}

/// What one step of work put in front of its model, and the room it had:
/// what a console shows as the worker's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkContext {
    /// The step, by [`LoggedStep::n`].
    pub n: u64,
    /// When it was taken, RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// What the model read and wrote for it, in tokens, as the provider
    /// counted them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LookUsage>,
    /// What it was given, in characters, by part: `system` (how to work,
    /// and for whom), `tools` (what it may call), `task` (what its sitting
    /// opened with: the task, and after a fold the plan, the handoff and
    /// the notes), `messages` (what it said and called, and what it was
    /// told along the way), `results` (what its calls brought back). The
    /// tokens `usage` measures can be shared out among them by this.
    #[serde(default)]
    pub parts: std::collections::BTreeMap<String, u64>,
    /// The model's context window, in tokens, where the daemon knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// The size, in tokens, past which the work folds its transcript into
    /// a handoff and goes on from that: the room above it is left free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folds_at: Option<u64>,
}

/// One step of a piece of work: what the model said, and what it called.
///
/// Where the work is stepped by a command-line agent, each call it makes
/// is a step of its own, and the agent's words are not known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggedStep {
    /// Its place in the work's log, from 1, over every sitting.
    pub n: u64,
    /// When it was taken, RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// What the model said at that step, cut to a few hundred characters;
    /// empty if it said nothing.
    #[serde(default)]
    pub said: String,
    /// How long the model took to answer, in milliseconds, where that is
    /// known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// What it called, in order.
    #[serde(default)]
    pub calls: Vec<ToolCall>,
}

/// One call a step or a look made, and what came of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The tool, as the model named it: `shell`, `read`, `search`, or a
    /// tool of a server she has.
    pub name: String,
    /// What it was called with, as JSON, with any string in it longer than
    /// a few hundred characters cut to its head. A step logged before
    /// calls kept their output has a string here.
    #[serde(default)]
    pub args: Value,
    /// The head of what came back, as it came back: at most 1,500
    /// characters.
    #[serde(default)]
    pub out: String,
    /// What came back was longer than `out`.
    #[serde(default)]
    pub cut: bool,
    /// How many characters came back in all.
    #[serde(default)]
    pub chars: u64,
    /// How long it took, in milliseconds.
    #[serde(default)]
    pub ms: u64,
    #[serde(default)]
    pub failed: bool,
    /// How many images came back with it.
    #[serde(default)]
    pub pictures: u32,
}

/// A part a piece of work handed out, in [`WorkLog::parts`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPart {
    /// Its id: its parent's, a slash, and its number.
    pub id: String,
    pub brief: String,
    pub state: WorkState,
    /// What steps it, as [`Work::brain`] has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<String>,
}

/// Params of [`method::LOOK_STEPS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookRead {
    /// A place key, `channel:id`.
    pub place: String,
    /// Only the looks that ended after the journal entry with this seq,
    /// and the one going on. Left out, every look kept, up to the newest
    /// fifty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
}

/// One look she took at a place, in the result of [`method::LOOK_STEPS`]:
/// the rounds in which she called something before she answered. A look
/// that called nothing that brought something back is not kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    /// Its number, never another look's.
    pub look: u64,
    /// The seq of the `decided` entry it ended with in her journal, or of
    /// the `stopped` entry if an operator stopped her in the middle of it;
    /// none while it goes on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    /// She is in the middle of it: more rounds may come.
    #[serde(default)]
    pub running: bool,
    /// Oldest first.
    #[serde(default)]
    pub rounds: Vec<LookRound>,
}

/// One round of a look: what she called at once, and what came back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookRound {
    /// When she called it, RFC 3339.
    pub at: String,
    /// In the order she called them, what she decided to do among them
    /// (`say`, `react`) as well, which comes back as `ok`.
    #[serde(default)]
    pub calls: Vec<ToolCall>,
    /// What the brain read and wrote for this round, in tokens, where
    /// the provider says; none from a daemon from before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LookUsage>,
    /// What was put in front of the brain for this look, in characters,
    /// by part: `system` (who she is), `people` (what she knows of who is
    /// here), `transcript` (the conversation), `recalled` (what she asked
    /// her memory for), `past` (pieces of her past the talk touched),
    /// `tools` (what she may call), `frame` (the rest of the prompt). A
    /// console can show what fills her context from this, against the
    /// tokens `usage` measures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<std::collections::BTreeMap<String, u64>>,
}

/// Tokens one round of a look, or one step of work, cost, as the provider
/// counted them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LookUsage {
    /// Everything the brain read, cached or not.
    pub input: u64,
    /// The part of `input` served from the provider's prompt cache.
    #[serde(default)]
    pub cached: u64,
    /// Everything the brain wrote, thinking included.
    #[serde(default)]
    pub output: u64,
}

/// One thing waiting on whoever runs her, in the result of
/// [`method::ATTENTION`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    /// The same each time it is listed, and never another item's: its kind
    /// and the seq of the journal entry it comes from, such as `asked-812`.
    /// A client keeps what it has shown by this.
    pub id: String,
    pub kind: AttentionKind,
    /// When it happened, RFC 3339.
    pub at: String,
    /// The journal entry it comes from: the `asked`, `concluded` or
    /// `reported` entry, or, for a report, the `intended` entry that
    /// meant it.
    pub seq: i64,
    /// The work it is about, by [`Work::id`], when it is about work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    /// That work's brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    /// The question; why the work ended; what the work told; or what she
    /// said in her report.
    pub text: String,
    /// For a question: what answering it takes, when the work said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<AskKind>,
    /// For a question of kind `choose`: what to choose from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// For work that ended: `blocked` or `gave_up`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// For what work told: the files it made, as her machine knows them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// For a report: where she said it, a place key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
}

/// What kind of thing is waiting on whoever runs her.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    /// A piece of work is waiting on an answer: [`method::WORK_ANSWER`]
    /// gives it one. Listed for as long as it waits.
    Question,
    /// A piece of work ended blocked or given up, in the last few days.
    Ended,
    /// A piece of work done for an operator, or directed by one, told her
    /// something while it ran, in the last few days.
    Told,
    /// What she said when she told an operator about her day, in the last
    /// few days.
    Report,
    /// A kind this client does not know, from a newer daemon.
    #[serde(other)]
    Other,
}

/// Where a piece of work stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    /// Taken on and not over, with no sitting at it this moment: it waits
    /// its turn, or rests between sittings.
    Open,
    /// A sitting is at it now. For a part: the sitting it belongs to is
    /// running, so it is running or about to.
    Running,
    /// It asked something, and waits to hear.
    Waiting,
    /// Done, and checked.
    Finished,
    /// Something outside its control is in the way.
    Blocked,
    /// It ran out of ideas.
    GaveUp,
    /// Ended from outside before it was done.
    Dropped,
    /// A state this client does not know, from a newer daemon.
    #[serde(other)]
    Other,
}

impl WorkState {
    /// Whether the work is over.
    pub fn ended(self) -> bool {
        matches!(
            self,
            Self::Finished | Self::Blocked | Self::GaveUp | Self::Dropped
        )
    }
}

/// One step of a piece of work's plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkStep {
    pub text: String,
    /// `todo`, `doing`, `done`, `dropped` or `failed`.
    pub status: String,
    /// The number of the part handed this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u64>,
}

/// What she can do besides talk, as her daemon runs her: `abilities` in
/// the answer to [`method::STATUS`]. A console says so and offers only
/// that, rather than reading her daemon's configuration, which is on the
/// daemon's machine and is not its to read. A daemon from before it said
/// so leaves it out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abilities {
    /// She takes things on as work: work is on for her, and she has a
    /// brain that can run tools.
    #[serde(default)]
    pub work: bool,
    /// The machine her work runs on, if she has one. Without one she works
    /// only by looking things up and thinking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineInfo>,
    /// She may search and read the web while working.
    #[serde(default)]
    pub web: bool,
    /// The agents installed on her machine that her work can hand a part
    /// to, by name: `claude-code`, `codex`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
}

impl Abilities {
    /// Whether her work has a shell on her daemon's own computer, as
    /// whoever runs the daemon.
    pub fn local(&self) -> bool {
        self.work && self.machine.as_ref().is_some_and(MachineInfo::local)
    }
}

/// The machine her work runs on, in [`Abilities`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineInfo {
    /// `local`, a directory on her daemon's own computer, worked in as
    /// whoever runs the daemon; or `docker`, a container.
    pub kind: String,
    /// As her configuration writes it: `local:`, `docker:box`.
    pub spec: String,
    /// The directory it works in, as a full path on her daemon's computer,
    /// for a `local` machine. A path she gives that is not absolute is
    /// under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

impl MachineInfo {
    /// Whether it is her daemon's own computer.
    pub fn local(&self) -> bool {
        self.kind == "local"
    }
}

/// Where her daemon runs: `core` in the answer to [`method::STATUS`]. The
/// paths she gives (her machine's directory, the folders of her work, the
/// files it hands over) are paths on that computer. A client can open them
/// only on the same one, and it knows it is there when it finds the same
/// [`id`](Self::id) in [`id_file`](Self::id_file): the file is the
/// daemon's, so finding it is finding the daemon's own file system. A host
/// name proves nothing: two computers can share one, and a laptop's
/// changes with the network it is on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInfo {
    /// Made at random the first time the daemon runs, and kept: no two
    /// daemons have the same, on one computer or on two.
    pub id: String,
    /// Where the daemon keeps [`id`](Self::id), as a full path on its own
    /// computer: `core-id` in its home.
    pub id_file: String,
    /// The name of the computer it runs on, as that computer gives it, for
    /// saying where it is. Never for telling whether it is here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// The result of [`method::BRAINS`], and of every request that changes
/// the brains: what they are once it has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainList {
    /// In the order the daemon's configuration has them.
    pub brains: Vec<BrainInfo>,
    /// The brain she thinks with, by name: in conversation and reflection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub think: Option<String>,
    /// The brain she ticks with: the routine of her day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<String>,
    /// Whether the daemon changes its brains when asked. A daemon that runs
    /// no home says no: its configuration is a file it was given.
    #[serde(default)]
    pub changeable: bool,
}

/// One brain a daemon is configured with, in a [`BrainList`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainInfo {
    /// What it is saved as: `[providers.<name>]`.
    pub name: String,
    /// `openai`, `anthropic`, `claude-code` or `codex`.
    pub kind: String,
    /// The model it is set to; empty for a command-line agent left to its
    /// own default.
    #[serde(default)]
    pub model: String,
    /// Where its API is, when its configuration says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The environment variable its key is read from, the kind's default
    /// where its configuration names none. None for a command-line agent,
    /// and for an API given a key in its configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// Whether it has a key to send: the variable is set, or its
    /// configuration gives one. False for a command-line agent.
    #[serde(default)]
    pub key_set: bool,
    /// The program, for a command-line agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Why the daemon's machine cannot run it, if it cannot: its key is not
    /// set, its program is not installed, nothing answers where its API is
    /// on that machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
}

/// Params of [`method::BRAIN_SAVE`].
///
/// Its `Debug` leaves the key out, so that logging a request cannot log one.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainSave {
    /// The brain changed, by the name it is saved as; left out, a new one.
    /// With another `name`, it is renamed, and [`BrainList::think`] and
    /// [`BrainList::tick`] follow it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// What it is saved as: letters, digits, `-` and `_`.
    pub name: String,
    /// Keys of `[providers.<name>]` to set, as `ghostd.toml` has them
    /// (`kind`, `model`, `base_url`, `api_key_env`, `json_mode`, …). A key
    /// is not one of them: it goes in `key`.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub set: serde_json::Map<String, Value>,
    /// Keys of `[providers.<name>]` to take out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clear: Vec<String>,
    /// The key itself, if one is given: kept in the home's `.env` under the
    /// variable the brain reads, never in the configuration, and never said
    /// back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl std::fmt::Debug for BrainSave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrainSave")
            .field("was", &self.was)
            .field("name", &self.name)
            .field("set", &self.set)
            .field("clear", &self.clear)
            .field("key", &self.key.as_ref().map(|_| "…"))
            .finish()
    }
}

/// Params of [`method::BRAIN_REMOVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainRemove {
    pub name: String,
}

/// Params of [`method::BRAIN_USE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainUse {
    /// The brain she is to think and tick with.
    pub name: String,
    /// The model it is to think with: left out, the one it is set to;
    /// empty, none named, for a command-line agent's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Params of [`method::BRAIN_MODELS`] and [`method::BRAIN_CHECK`]: a brain
/// saved, by its name, or one being written, by what its
/// `[providers.<name>]` would say; with both, what is written is laid over
/// what is saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainAsk {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Keys of a `[providers.<name>]` table, as [`BrainSave::set`] has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Map<String, Value>>,
    /// For a check, the model to think with in place of its own; empty, a
    /// command-line agent's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The result of [`method::BRAIN_MODELS`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainModels {
    /// By id, as the API lists them.
    pub models: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brain_list_reads_with_only_what_a_daemon_sends_and_never_carries_a_key() {
        let list: BrainList = serde_json::from_str(
            r#"{"brains":[{"name":"main","kind":"openai","model":"m","key_env":"K","key_set":true},
                {"name":"cc","kind":"claude-code","program":"claude","blocker":"`claude` was not found"}],
                "think":"main","tick":"main","changeable":true}"#,
        )
        .unwrap();
        assert_eq!(list.think.as_deref(), Some("main"));
        assert!(list.brains[0].key_set && !list.brains[1].key_set);
        assert_eq!(list.brains[1].model, "");
        let back: BrainList = serde_json::from_str(&serde_json::to_string(&list).unwrap()).unwrap();
        assert_eq!(back, list);
        // A daemon that says nothing of changing them does not take changes.
        let old: BrainList = serde_json::from_str(r#"{"brains":[]}"#).unwrap();
        assert!(!old.changeable && old.think.is_none());
    }

    #[test]
    fn a_saved_brain_carries_its_key_on_the_wire_and_never_in_its_debug() {
        let mut fields = serde_json::Map::new();
        fields.insert("json_mode".into(), Value::from(true));
        fields.insert("kind".into(), Value::from("openai"));
        let save = BrainSave {
            was: None,
            name: "deepseek".into(),
            set: fields,
            clear: Vec::new(),
            key: Some("sk-secret".into()),
        };
        let sent = serde_json::to_string(&Frame::request(9, method::BRAIN_SAVE, &save)).unwrap();
        assert_eq!(
            sent,
            r#"{"jsonrpc":"2.0","id":9,"method":"brains.save","params":{"key":"sk-secret","name":"deepseek","set":{"json_mode":true,"kind":"openai"}}}"#
        );
        let read: Frame = serde_json::from_str(&sent).unwrap();
        assert_eq!(read.params::<BrainSave>().unwrap(), save);
        assert!(!format!("{save:?}").contains("sk-secret"), "{save:?}");
        // Asked by name alone, nothing else rides along.
        let ask = BrainAsk {
            name: Some("main".into()),
            ..BrainAsk::default()
        };
        assert_eq!(
            serde_json::to_value(&ask).unwrap(),
            serde_json::json!({"name": "main"})
        );
        let used: BrainUse = serde_json::from_str(r#"{"name":"main"}"#).unwrap();
        assert_eq!(used.model, None, "left out, the model it is set to");
    }

    #[test]
    fn abilities_round_trip_and_a_daemon_that_says_little_can_do_little() {
        let full = Abilities {
            work: true,
            machine: Some(MachineInfo {
                kind: "local".into(),
                spec: "local:".into(),
                dir: Some("/srv/ghost/ghosts/solver/home".into()),
            }),
            web: true,
            agents: vec!["claude-code".into()],
        };
        let sent = serde_json::to_value(&full).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"work": true, "machine": {"kind": "local", "spec": "local:",
                "dir": "/srv/ghost/ghosts/solver/home"}, "web": true, "agents": ["claude-code"]})
        );
        assert_eq!(serde_json::from_value::<Abilities>(sent).unwrap(), full);
        assert!(full.local());
        // A daemon from before machines had a directory says none.
        let older: MachineInfo =
            serde_json::from_str(r#"{"kind": "local", "spec": "local:"}"#).unwrap();
        assert_eq!(older.dir, None);

        // Without a machine or agents, neither is written at all.
        let bare = Abilities {
            work: true,
            ..Abilities::default()
        };
        assert_eq!(
            serde_json::to_value(&bare).unwrap(),
            serde_json::json!({"work": true, "web": false})
        );
        // Whatever a daemon leaves out, she cannot do.
        let said: Abilities = serde_json::from_str("{}").unwrap();
        assert_eq!(said, Abilities::default());
        assert!(!said.local());

        // A container is not her daemon's computer, and a machine is
        // nothing with work turned off.
        let docker = Abilities {
            machine: Some(MachineInfo {
                kind: "docker".into(),
                spec: "docker:box".into(),
                dir: None,
            }),
            ..full.clone()
        };
        assert!(!docker.local());
        assert!(
            !Abilities {
                work: false,
                ..full
            }
            .local()
        );
    }

    #[test]
    fn where_a_core_runs_round_trips_and_its_host_may_be_left_out() {
        let core = CoreInfo {
            id: "5f0c3a9e1b7d4e2a8c6f0b1d3e5a7c9f".into(),
            id_file: "/Users/me/Library/Application Support/Ghost/core-id".into(),
            host: Some("studio".into()),
        };
        let sent = serde_json::to_value(&core).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"id": "5f0c3a9e1b7d4e2a8c6f0b1d3e5a7c9f",
                "id_file": "/Users/me/Library/Application Support/Ghost/core-id", "host": "studio"})
        );
        assert_eq!(serde_json::from_value::<CoreInfo>(sent).unwrap(), core);
        let nameless: CoreInfo =
            serde_json::from_str(r#"{"id": "a", "id_file": "/srv/ghost/core-id"}"#).unwrap();
        assert_eq!(nameless.host, None);
        assert_eq!(
            serde_json::to_value(&nameless).unwrap(),
            serde_json::json!({"id": "a", "id_file": "/srv/ghost/core-id"}),
            "a host it does not know is not written"
        );
    }

    #[test]
    fn a_media_file_is_asked_for_by_its_name_and_comes_back_as_base64() {
        let ask = Frame::request(
            4,
            method::MEDIA,
            &MediaRead {
                name: "ab12.png".into(),
            },
        );
        let sent = serde_json::to_string(&ask).unwrap();
        assert_eq!(
            sent,
            r#"{"jsonrpc":"2.0","id":4,"method":"media.get","params":{"name":"ab12.png"}}"#
        );
        let read: Frame = serde_json::from_str(&sent).unwrap();
        assert_eq!(read.params::<MediaRead>().unwrap().name, "ab12.png");
        let file = MediaFile {
            mime: "image/png".into(),
            data: "aGVy".into(),
        };
        let sent = serde_json::to_string(&Frame::ok(4, &file)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&sent).unwrap(),
            serde_json::json!({"jsonrpc": "2.0", "id": 4,
                "result": {"mime": "image/png", "data": "aGVy"}})
        );
        let back: Frame = serde_json::from_str(&sent).unwrap();
        assert_eq!(back.into_result::<MediaFile>().unwrap(), file);
    }

    #[test]
    fn a_response_whose_result_is_null_is_still_a_response() {
        let sent = serde_json::to_string(&Frame::ok(3, &None::<Avatar>)).unwrap();
        assert_eq!(sent, r#"{"jsonrpc":"2.0","id":3,"result":null}"#);
        let read: Frame = serde_json::from_str(&sent).unwrap();
        assert_eq!(read.result, Some(Value::Null));
        // And a frame with no result at all still has none.
        let bare: Frame = serde_json::from_str(r#"{"jsonrpc":"2.0","id":3}"#).unwrap();
        assert_eq!(bare.result, None);
    }

    #[test]
    fn frames_have_the_json_rpc_shape() {
        let req = Frame::request(7, method::ACT, &Action::Typing { place: "42".into() });
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"jsonrpc":"2.0","id":7,"method":"presence.act","params":{"do":"typing","place":"42"}}"#
        );
        let note = Frame::notify(method::JOURNAL_SUBSCRIBE, &());
        assert_eq!(
            serde_json::to_string(&note).unwrap(),
            r#"{"jsonrpc":"2.0","method":"journal.subscribe"}"#
        );
        assert!(!req.is_response());
        assert!(Frame::ok(7, &Acted::default()).is_response());
    }

    #[test]
    fn results_and_errors_round_trip() {
        let ok = Frame::ok(
            1,
            &Acted {
                id: Some("m1".into()),
                ..Acted::default()
            },
        );
        assert_eq!(ok.into_result::<Acted>().unwrap().id.as_deref(), Some("m1"));
        let err = Frame::err(1, code::ACT_FAILED, "chat not found");
        let e = err.into_result::<Acted>().unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (code::ACT_FAILED, "chat not found")
        );
        let bad = Frame::request(1, method::ACT, &"not an action");
        assert_eq!(
            bad.params::<Action>().unwrap_err().code,
            code::INVALID_PARAMS
        );
    }

    #[test]
    fn a_hello_and_a_card_from_before_there_could_be_several_characters_still_read() {
        let hello: Hello = serde_json::from_str(
            r#"{"protocol":1,"role":"presence","presence":{"channel":"qq","can":["send"]}}"#,
        )
        .unwrap();
        assert_eq!(hello.ghost, None);
        // A client that names nobody says nothing new on the wire.
        assert!(!serde_json::to_string(&hello).unwrap().contains("ghost"));
        let card: GhostCard = serde_json::from_str(
            r#"{"name":"W","pronouns":"she/her","city":"C","country":"CN","seal":"s"}"#,
        )
        .unwrap();
        assert_eq!(card.bio, None);
    }

    #[test]
    fn a_listed_character_reads_as_her_card_with_her_id_beside_it() {
        let listed = Listed {
            id: "weiwei".into(),
            card: GhostCard {
                name: "W".into(),
                pronouns: "she/her".into(),
                city: "C".into(),
                country: "CN".into(),
                seal: "s".into(),
                bio: None,
            },
            awake: None,
            doing: None,
            avatar: true,
        };
        let sent = serde_json::to_string(&listed).unwrap();
        assert_eq!(
            sent,
            r#"{"id":"weiwei","name":"W","pronouns":"she/her","city":"C","country":"CN","seal":"s","avatar":true}"#
        );
        assert_eq!(serde_json::from_str::<Listed>(&sent).unwrap(), listed);
        // Asked with no token, the question says nothing of one.
        let ask = Frame::request(1, method::LIST, &ListGhosts::default());
        assert_eq!(ask.params, Some(serde_json::json!({})));
    }

    #[test]
    fn inbound_defaults_are_lenient() {
        let m: Inbound = serde_json::from_str(
            r#"{"place":{"id":"1","kind":"direct"},"from":{"id":"u","name":"U"},"id":"9","text":"hi"}"#,
        )
        .unwrap();
        assert!(!m.addressed && !m.from.automated && m.reply_to.is_none());
        assert!(m.images.is_empty());
    }

    #[test]
    fn images_ride_along_only_when_there_are_some() {
        let mut m: Inbound = serde_json::from_str(
            r#"{"place":{"id":"1","kind":"direct"},"from":{"id":"u","name":"U"},"id":"9","text":"[image]"}"#,
        )
        .unwrap();
        assert!(!serde_json::to_string(&m).unwrap().contains("images"));
        m.images.push(Image {
            url: "https://example.invalid/a.png".into(),
            mime: Some("image/png".into()),
        });
        let back: Inbound = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn a_step_and_a_look_read_with_only_what_they_carry() {
        let log: WorkLog = serde_json::from_str(
            r#"{"steps":[{"n":3,"calls":[{"name":"shell","args":"{\"command\":\"ls\"}"}]}]}"#,
        )
        .unwrap();
        let step = &log.steps[0];
        assert_eq!((step.n, step.said.as_str(), step.ms), (3, "", None));
        assert_eq!(step.calls[0].args, "{\"command\":\"ls\"}");
        assert!(!step.calls[0].cut && step.calls[0].out.is_empty());
        assert!(log.parts.is_empty() && !log.more);

        let look: Look = serde_json::from_str(r#"{"look":4,"running":true,"rounds":[]}"#).unwrap();
        assert_eq!((look.seq, look.running), (None, true));
        let asked = serde_json::to_value(LookRead {
            place: "t:wen".into(),
            after: None,
        })
        .unwrap();
        assert_eq!(asked, serde_json::json!({"place": "t:wen"}));
    }

    #[test]
    fn a_work_state_from_a_newer_daemon_still_reads() {
        let work: Work = serde_json::from_str(
            r#"{"id":"u1","brief":"b","state":"pondering","parts":[{"id":"u1/1","brief":"p","state":"running"}]}"#,
        )
        .unwrap();
        assert_eq!(work.state, WorkState::Other);
        assert_eq!(work.parts[0].state, WorkState::Running);
        assert!(!work.state.ended() && work.parts[0].steps == 0);
    }

    #[test]
    fn what_a_question_asks_for_rides_along_only_when_the_work_said_and_a_newer_kind_still_reads() {
        let plain: Work =
            serde_json::from_str(r#"{"id":"u1","brief":"b","state":"waiting","question":"q?"}"#)
                .unwrap();
        assert_eq!((plain.kind, plain.options.len()), (None, 0));
        let sent = serde_json::to_string(&plain).unwrap();
        assert!(
            !sent.contains("kind") && !sent.contains("options"),
            "{sent}"
        );
        let choice: Work = serde_json::from_str(
            r#"{"id":"u1","brief":"b","state":"waiting","question":"q?","kind":"choose","options":["a","b"]}"#,
        )
        .unwrap();
        assert_eq!(choice.kind, Some(AskKind::Choose));
        assert_eq!(choice.options, ["a", "b"]);
        // As her mind answers it, with nothing said.
        let nothing: Work = serde_json::from_str(
            r#"{"id":"u1","brief":"b","state":"open","kind":null,"options":[]}"#,
        )
        .unwrap();
        assert_eq!(nothing.kind, None);
        let newer: Work =
            serde_json::from_str(r#"{"id":"u1","brief":"b","state":"waiting","kind":"sign"}"#)
                .unwrap();
        assert_eq!(newer.kind, Some(AskKind::Other));
    }

    #[test]
    fn an_attention_item_reads_as_her_mind_answers_it_and_a_newer_kind_still_reads() {
        let question: Attention = serde_json::from_str(
            r#"{"id":"asked-9","kind":"question","at":"2026-11-16T06:00:00Z","seq":9,
                "work":"w1","brief":"b","text":"which?","ask":"choose","options":["a","b"]}"#,
        )
        .unwrap();
        assert_eq!(question.kind, AttentionKind::Question);
        assert_eq!(question.ask, Some(AskKind::Choose));
        let report: Attention = serde_json::from_str(
            r#"{"id":"report-4","kind":"report","at":"2026-11-16T13:00:00Z","seq":4,
                "place":"qq:boss","text":"a quiet day"}"#,
        )
        .unwrap();
        assert_eq!((report.work, report.ask), (None, None));
        let newer: Attention = serde_json::from_str(
            r#"{"id":"x-1","kind":"invoice","at":"2026-11-16T13:00:00Z","seq":1,"text":"t"}"#,
        )
        .unwrap();
        assert_eq!(newer.kind, AttentionKind::Other);
    }

    #[test]
    fn something_said_from_a_console_says_say_unless_it_names_a_directive_on_the_wire() {
        let said: ConsoleSay = serde_json::from_str(r#"{"text":"hi"}"#).unwrap();
        assert_eq!((said.place, said.mode), (None, SayMode::Say));
        let directed = ConsoleSay {
            place: Some("desk-1".into()),
            text: "send the rota".into(),
            mode: SayMode::Directive,
        };
        assert_eq!(
            serde_json::to_string(&Frame::request(4, method::SAY, &directed)).unwrap(),
            r#"{"jsonrpc":"2.0","id":4,"method":"console.say","params":{"mode":"directive","place":"desk-1","text":"send the rota"}}"#
        );
        // A hello from a console that only watches says nothing of who is at it.
        let hello: Hello = serde_json::from_str(r#"{"protocol":1,"role":"console"}"#).unwrap();
        assert_eq!(hello.who, None);
        assert!(!serde_json::to_string(&hello).unwrap().contains("who"));
    }

    #[test]
    fn a_presence_that_says_nothing_of_what_it_allows_has_no_limits() {
        let older: PresenceHello =
            serde_json::from_value(serde_json::json!({"channel": "qq", "can": ["send"]})).unwrap();
        assert_eq!(older.allows, Allows::default());
        assert!(older.allows.writes_first);
        let qq: PresenceHello = serde_json::from_value(serde_json::json!({
            "channel": "qq", "can": ["send"],
            "allows": {"answer_direct_secs": 1800, "answer_group_secs": 300, "writes_first": false}
        }))
        .unwrap();
        assert_eq!(qq.allows.answer_group_secs, Some(300));
        assert!(!qq.allows.writes_first);
    }
}
