//! What the desk knows about her, as read from the daemon.
//!
//! Everything here is shaped from the protocol's JSON. The desk does not
//! depend on `ghost-core`: it is a client like any presence, and knows her
//! journal only as the daemon serves it.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::GhostCard;
use serde_json::Value;

/// One line of her journal.
#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub data: Value,
}

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl Entry {
    pub fn from_value(v: &Value) -> Option<Self> {
        let event = v.get("event")?;
        Some(Self {
            seq: v.get("seq")?.as_i64()?,
            at: v.get("at")?.as_str()?.parse().ok()?,
            kind: s(event, "kind"),
            data: event.get("data").cloned().unwrap_or(Value::Null),
        })
    }
}

/// One block of her day's plan.
#[derive(Debug, Clone)]
pub struct Block {
    pub what: String,
}

/// A channel she is on, as the daemon reports it.
#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
}

/// `ghost.status`, shaped.
#[derive(Debug, Clone)]
pub struct Status {
    pub name: String,
    pub city: String,
    pub tz: Tz,
    pub local_time: String,
    /// `awake` or `asleep`.
    pub phase: String,
    /// What she is doing now, if her plan has a block for it.
    pub doing: Option<Block>,
    pub reach: String,
    pub until: Option<DateTime<Utc>>,
    pub journal: i64,
    pub channels: Vec<Channel>,
    pub tempo: f64,
    pub think: String,
}

fn block(v: &Value) -> Block {
    Block { what: s(v, "what") }
}

impl Status {
    pub fn from_value(v: &Value) -> Self {
        let tz: Tz = s(v, "timezone").parse().unwrap_or(chrono_tz::UTC);
        let moment = v.get("moment").cloned().unwrap_or(Value::Null);
        let doing = moment.get("doing").filter(|d| !d.is_null()).map(|d| {
            // A `Doing` carries the block it resolves, under `block`, or is
            // itself block-shaped.
            d.get("block").map_or_else(|| block(d), block)
        });
        Self {
            name: s(v, "name"),
            city: s(v, "city"),
            tz,
            local_time: s(v, "local_time"),
            phase: s(&moment, "phase").to_lowercase(),
            doing,
            reach: s(&moment, "reach").to_lowercase(),
            until: moment
                .get("until")
                .and_then(Value::as_str)
                .and_then(|t| t.parse().ok()),
            journal: v.get("journal").and_then(Value::as_i64).unwrap_or_default(),
            channels: v
                .get("channels")
                .and_then(Value::as_array)
                .map(|cs| {
                    cs.iter()
                        .map(|c| Channel {
                            name: s(c, "channel"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            tempo: v.get("tempo").and_then(Value::as_f64).unwrap_or(1.),
            think: v.get("brains").map(|b| s(b, "think")).unwrap_or_default(),
        }
    }

    pub fn awake(&self) -> bool {
        self.phase == "awake"
    }

    /// The hour and minute of her clock.
    pub fn hhmm(&self) -> String {
        self.local_time
            .split(' ')
            .nth(1)
            .unwrap_or(&self.local_time)
            .to_owned()
    }
}

/// A place she is in, from `memory.places`: only what the desk shows of it.
#[derive(Debug, Clone)]
pub struct PlaceView {
    pub unread: i64,
}

impl PlaceView {
    pub fn from_value(v: &Value) -> Option<Self> {
        let place = v.get("place")?;
        Some(Self {
            unread: place
                .get("unread")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        })
    }
}

/// Someone she knows, from `memory.people`: what the desk shows of you.
#[derive(Debug, Clone)]
pub struct PersonView {
    pub key: String,
    pub heard: i64,
}

impl PersonView {
    pub fn from_value(v: &Value) -> Option<Self> {
        let person = v.get("person")?;
        Some(Self {
            key: s(person, "key"),
            heard: person
                .get("heard")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        })
    }
}

/// The connection to the daemon, as the top bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    Lost(String),
}

/// Something the link learned, handed to the desk.
#[derive(Debug, Clone)]
pub enum Update {
    Link(LinkState),
    Welcome(GhostCard),
    Status(Box<Status>),
    Agenda(Value),
    Places(Vec<PlaceView>),
    People(Vec<PersonView>),
    /// The journal's recent tail, oldest first. Replaces what was held.
    Journal(Vec<Entry>),
    /// One new entry, as it is written.
    Entry(Entry),
    /// She said something to you on the desk.
    HerSay(String),
    HerTyping,
    /// The desk's own channel is registered with the daemon.
    DeskOnline(bool),
}

/// A message in the conversation between you and her on the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub hers: bool,
    pub text: String,
    pub at: DateTime<Utc>,
}
