//! # ghost-protocol
//!
//! The Ghost Protocol: JSON-RPC 2.0 over a WebSocket, one JSON object per
//! text frame. The daemon is the server. Two kinds of client connect to it:
//!
//! - a **presence** carries her onto one channel (Telegram, Discord, a
//!   terminal). It reports what arrives with [`method::INBOUND`] and carries
//!   out what she does when the daemon sends it [`method::ACT`].
//! - a **console** watches. It can read her status, her journal and her
//!   agenda. The protocol has no method that edits her identity or removes a
//!   journal entry; that is not a permission a console lacks, it is a method
//!   that does not exist.
//!
//! A connection begins with one [`method::HELLO`] request. Everything after
//! depends on the role declared in it.
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
    /// [`Welcome`](super::Welcome). Must be the first frame.
    pub const HELLO: &str = "hello";

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

    /// Console → daemon request. No params.
    pub const STATUS: &str = "ghost.status";
    /// Console → daemon request. No params.
    pub const AGENDA: &str = "mind.agenda";
    /// Console → daemon request. No params.
    pub const PEOPLE: &str = "memory.people";
    /// Console → daemon request. No params. Her sealed birth certificate.
    pub const IDENTITY: &str = "ghost.identity";
    /// Console → daemon request. No params. What is true about her life.
    pub const CANON: &str = "memory.canon";
    /// Console → daemon request. No params. Every place she is in.
    pub const PLACES: &str = "memory.places";
    /// Console → daemon request. Params [`PlaceRead`](super::PlaceRead). The
    /// journal entries that belong to one place: what was said there and
    /// what she made of it.
    pub const JOURNAL_PLACE: &str = "journal.place";
    /// Console → daemon request. Params [`JournalRead`](super::JournalRead).
    pub const JOURNAL_READ: &str = "journal.read";
    /// Console → daemon request. No params. Afterwards the daemon sends a
    /// [`JOURNAL_ENTRY`] notification for every entry as it is written.
    pub const JOURNAL_SUBSCRIBE: &str = "journal.subscribe";
    /// Daemon → console notification. Params: one journal entry.
    pub const JOURNAL_ENTRY: &str = "journal.entry";
}

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
    /// The presence could not carry out an action.
    pub const ACT_FAILED: i64 = -32010;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
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
    /// Required when `role` is `presence`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceHello>,
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
    /// The seal of her birth certificate.
    pub seal: String,
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

#[cfg(test)]
mod tests {
    use super::*;

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
                items: vec![],
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
    fn inbound_defaults_are_lenient() {
        let m: Inbound = serde_json::from_str(
            r#"{"place":{"id":"1","kind":"direct"},"from":{"id":"u","name":"U"},"id":"9","text":"hi"}"#,
        )
        .unwrap();
        assert!(!m.addressed && !m.from.automated && m.reply_to.is_none());
    }
}
