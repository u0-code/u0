//! What the desk knows about her, as read from the daemon.
//!
//! Everything here is shaped from the protocol's JSON. The desk does not
//! depend on `ghost-core`: it is a client like any presence, and knows her
//! journal only as the daemon serves it.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::{GhostCard, Listed, Work};
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

/// `ghost.status`, shaped.
#[derive(Debug, Clone)]
pub struct Status {
    pub name: String,
    pub city: String,
    pub tz: Tz,
    pub local_time: String,
    pub journal: i64,
    pub tempo: f64,
    pub think: String,
}

impl Status {
    pub fn from_value(v: &Value) -> Self {
        let tz: Tz = s(v, "timezone").parse().unwrap_or(chrono_tz::UTC);
        Self {
            name: s(v, "name"),
            city: s(v, "city"),
            tz,
            local_time: s(v, "local_time"),
            journal: v.get("journal").and_then(Value::as_i64).unwrap_or_default(),
            tempo: v.get("tempo").and_then(Value::as_f64).unwrap_or(1.),
            think: v.get("brains").map(|b| s(b, "think")).unwrap_or_default(),
        }
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
    People(Vec<PersonView>),
    /// The journal's recent tail, oldest first. Replaces what was held.
    Journal(Vec<Entry>),
    /// One new entry, as it is written.
    Entry(Entry),
    /// She said something to you on the desk, in a session (a place).
    HerSay {
        place: String,
        text: String,
    },
    HerTyping {
        place: String,
    },
    /// The desk's own channel is registered with the daemon.
    DeskOnline(bool),
    /// Who the daemon runs, as `ghost.list` says.
    Roster(Vec<Listed>),
    /// The work she has in hand and what ended lately, as `work.list` says.
    Work(Vec<Work>),
}

/// A message in the conversation between you and her on the desk, in one
/// session: each session is a place of its own on the `desk` channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub place: String,
    pub hers: bool,
    pub text: String,
    pub at: DateTime<Utc>,
}
