//! What the desk knows about her, as read from the daemon.
//!
//! Everything here is shaped from the protocol's JSON. The desk does not
//! depend on `ghost-core`: it is a client like any presence, and knows her
//! journal only as the daemon serves it. Where it describes an event in a
//! line it does so the way `ghostd watch` does, so the two agree.

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

fn flat(text: &str) -> String {
    text.replace('\n', " ⏎ ")
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

    /// The event in one line, in the words `ghostd watch` uses.
    pub fn describe(&self, tz: Tz) -> String {
        let d = &self.data;
        let when = |key: &str| {
            d.get(key)
                .and_then(Value::as_str)
                .and_then(|t| t.parse::<DateTime<Utc>>().ok())
                .map(|t| t.with_timezone(&tz).format("%m-%d %H:%M").to_string())
                .unwrap_or_default()
        };
        match self.kind.as_str() {
            "born" => format!("{} was born (seal {})", s(d, "name"), short(&s(d, "seal"))),
            "described" => format!(
                "her persona as it stands now (seal {})",
                short(&s(d, "seal"))
            ),
            "dormant" => format!("nothing since {}", when("since")),
            "woke" => "woke up".into(),
            "slept" => "went to sleep".into(),
            "planned" => {
                let plan = d
                    .get("blocks")
                    .and_then(Value::as_array)
                    .map(|blocks| {
                        blocks
                            .iter()
                            .map(|b| {
                                format!(
                                    "{}–{} {} [{}]",
                                    s(b, "from"),
                                    s(b, "to"),
                                    s(b, "what"),
                                    s(b, "reach")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_default();
                match d.get("weather").and_then(Value::as_str) {
                    Some(w) => format!("{} ({w}): {plan}", s(d, "day")),
                    None => format!("{}: {plan}", s(d, "day")),
                }
            }
            "lived" | "learned" | "practised" => flat(&s(d, "text").max_len(&s(d, "lesson"))),
            "reflected" => match d.get("channel").and_then(Value::as_str) {
                Some(c) => format!("[{c}] {}", flat(&s(d, "text"))),
                None => flat(&s(d, "text")),
            },
            "heard" => {
                let place = d.get("place").cloned().unwrap_or(Value::Null);
                let name = place
                    .get("name")
                    .and_then(Value::as_str)
                    .map_or_else(|| s(&place, "id"), str::to_owned);
                let from = d.get("from").cloned().unwrap_or(Value::Null);
                let addressed = d.get("addressed").and_then(Value::as_bool).unwrap_or(false);
                let images = d
                    .get("images")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                format!(
                    "[{}:{name}] {}{}: {}{}",
                    s(d, "channel"),
                    s(&from, "name"),
                    if addressed { "" } else { " (not to her)" },
                    flat(&s(d, "text")),
                    match images {
                        0 => String::new(),
                        1 => " (1 image)".to_owned(),
                        n => format!(" ({n} images)"),
                    }
                )
            }
            "read" => format!(
                "[{}:{}] up to #{}",
                s(d, "channel"),
                s(d, "place"),
                d.get("upto").and_then(Value::as_i64).unwrap_or_default()
            ),
            "said" => format!(
                "[{}:{}] {}",
                s(d, "channel"),
                s(d, "place"),
                flat(&s(d, "text"))
            ),
            "reacted" => format!("[{}:{}] {}", s(d, "channel"), s(d, "place"), s(d, "emoji")),
            "posted" | "saw" => format!("[{}] {}", s(d, "channel"), flat(&s(d, "text"))),
            "decided" => {
                let reason = s(d, "reason");
                if reason.is_empty() {
                    format!("[{}] {}", s(d, "about"), s(d, "decision"))
                } else {
                    format!(
                        "[{}] {} — {}",
                        s(d, "about"),
                        s(d, "decision"),
                        flat(&reason)
                    )
                }
            }
            "felt" => format!(
                "{:+} toward {}: {}",
                d.get("delta").and_then(Value::as_i64).unwrap_or_default(),
                s(d, "toward"),
                flat(&s(d, "why"))
            ),
            "noted" | "profiled" => format!(
                "{}: {}",
                s(d, "about").max_len(&s(d, "person")),
                flat(&s(d, "text").max_len(&s(d, "shared")))
            ),
            "intended" => format!(
                "by {}{}: {}",
                when("due"),
                if d.get("report").and_then(Value::as_bool).unwrap_or(false) {
                    " (report)"
                } else {
                    ""
                },
                flat(&s(d, "what"))
            ),
            "settled" => format!(
                "#{}: {}",
                d.get("intention")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                flat(&s(d, "outcome"))
            ),
            "left" => format!("{}: {}", s(d, "person"), flat(&s(d, "why"))),
            "canon" => format!("{} [{}]", flat(&s(d, "fact")), s(d, "source")),
            "undertook" => format!("[{}] {}", s(d, "id"), flat(&s(d, "brief"))),
            "asked" => format!("[{}] {}", s(d, "id"), flat(&s(d, "question"))),
            "added" => format!("[{}] {}", s(d, "id"), flat(&s(d, "text"))),
            "worked" => format!(
                "[{}] {} steps: {}",
                s(d, "id"),
                d.get("steps").and_then(Value::as_i64).unwrap_or_default(),
                flat(&s(d, "summary"))
            ),
            "concluded" => format!(
                "[{}] {}: {}",
                s(d, "id"),
                s(d, "outcome"),
                flat(&s(d, "summary"))
            ),
            "reported" => format!("[{}] {}", s(d, "id"), flat(&s(d, "text"))),
            "kept" => format!(
                "media/{}{}: {}",
                s(d, "file"),
                d.get("from")
                    .and_then(Value::as_str)
                    .map_or_else(String::new, |f| format!(" (from {f})")),
                flat(&s(d, "note"))
            ),
            "let_go" => format!(
                "let #{} go",
                d.get("kept").and_then(Value::as_i64).unwrap_or_default()
            ),
            "viewed" => format!(
                "#{} image {}: {}",
                d.get("message").and_then(Value::as_i64).unwrap_or_default(),
                d.get("image").and_then(Value::as_i64).unwrap_or_default(),
                flat(&s(d, "what"))
            ),
            "opened" => format!(
                "{}: {}",
                s(d, "url"),
                flat(&s(d, "text").chars().take(200).collect::<String>())
            ),
            "looked_up" => format!(
                "\"{}\": {}",
                s(d, "query"),
                flat(&s(d, "found").chars().take(200).collect::<String>())
            ),
            "directed" => format!(
                "#{} from {}: {}",
                d.get("message").and_then(Value::as_i64).unwrap_or_default(),
                s(d, "from"),
                flat(&s(d, "text"))
            ),
            "quieted" => format!("quiet in {} until {}", s(d, "place"), when("until")),
            "installed" | "uninstalled" | "skill_installed" | "skill_uninstalled" => s(d, "name"),
            "measured" => format!(
                "[{}] {} posts measured",
                s(d, "channel"),
                d.get("posts").and_then(Value::as_array).map_or(0, Vec::len)
            ),
            "pursued" => {
                let what = s(d, "what");
                if what.trim().is_empty() {
                    "let it go".to_owned()
                } else {
                    flat(&what)
                }
            }
            _ => match d {
                Value::Null => String::new(),
                other => flat(&other.to_string()),
            },
        }
    }
}

/// The longer of two strings: for events whose text sits under one of two
/// keys depending on the variant.
trait MaxLen {
    fn max_len(&self, other: &str) -> String;
}

impl MaxLen for String {
    fn max_len(&self, other: &str) -> String {
        if self.is_empty() {
            other.to_owned()
        } else {
            self.clone()
        }
    }
}

/// A hash as the journal shows it: four characters, an ellipsis, two.
pub fn short(hash: &str) -> String {
    if hash.chars().count() <= 8 {
        return hash.to_owned();
    }
    let head: String = hash.chars().take(4).collect();
    let tail: String = hash
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
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
    pub name: String,
    pub heard: i64,
    pub first_at: Option<DateTime<Utc>>,
    pub standing: String,
    pub notes: Vec<String>,
}

impl PersonView {
    pub fn from_value(v: &Value) -> Option<Self> {
        let person = v.get("person")?;
        Some(Self {
            key: s(person, "key"),
            name: s(person, "name"),
            heard: person
                .get("heard")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            first_at: person
                .get("first_at")
                .and_then(Value::as_str)
                .and_then(|t| t.parse().ok()),
            standing: match v.get("standing") {
                Some(Value::String(st)) => st.to_lowercase(),
                Some(other) if !other.is_null() => other.to_string(),
                _ => String::new(),
            },
            notes: v
                .get("notes")
                .and_then(Value::as_array)
                .map(|ns| {
                    ns.iter()
                        .map(|n| match n {
                            Value::String(t) => t.clone(),
                            other => s(other, "text"),
                        })
                        .filter(|t| !t.is_empty())
                        .collect()
                })
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
