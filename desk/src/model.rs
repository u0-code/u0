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

/// How the link to a daemon has gone, beyond how it stands this moment:
/// since when it has been up or down, whether it was ever up, and the last
/// reason it could not be reached. A link that is down tries again on its
/// own and says it is connecting while it does; that is still down, and the
/// reason from the last try still holds, so the card does not flicker
/// between an error and nothing every few seconds.
#[derive(Debug, Clone)]
pub struct Reach {
    /// When it last came up, or went down.
    pub since: std::time::Instant,
    /// Whether it has been up since the desk began following it.
    pub ever: bool,
    /// Why the last try failed, while it is down.
    pub why: Option<String>,
    /// How many tries have failed since it was last up.
    pub tries: u32,
}

impl Default for Reach {
    fn default() -> Self {
        Self {
            since: std::time::Instant::now(),
            ever: false,
            why: None,
            tries: 0,
        }
    }
}

impl Reach {
    /// Take in that the link has gone from `was` to `now`.
    pub fn note(&mut self, was: &LinkState, now: &LinkState) {
        let up = *now == LinkState::Connected;
        if up != (*was == LinkState::Connected) {
            self.since = std::time::Instant::now();
        }
        match now {
            LinkState::Connected => {
                self.ever = true;
                self.why = None;
                self.tries = 0;
            }
            LinkState::Lost(why) => {
                self.why = Some(why.clone());
                self.tries += 1;
            }
            LinkState::Connecting => {}
        }
    }
}

/// Why a daemon at `host` cannot be reached, in a sentence a person reads,
/// from the error the link gave. The error itself stays a click away; this
/// is what most people need to know first.
pub fn reason(why: &str, host: &str) -> String {
    let w = why.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|x| w.contains(x));
    if has(&["connection refused", "os error 61", "os error 111"]) {
        format!("Nothing is listening at {host}.")
    } else if has(&["unauthorized", "wrong or missing token", "401", "403"]) {
        format!("{host} did not accept the token.")
    } else if has(&["is not set in the environment"]) {
        // The settings' own sentence names the variable, and says it well.
        sentence(why)
    } else if has(&[
        "failed to lookup",
        "nodename nor servname",
        "name or service not known",
        "no such host",
    ]) {
        format!("No address could be found for {host}.")
    } else if has(&["timed out", "timeout", "deadline"]) {
        format!("{host} did not answer in time.")
    } else if has(&["network is unreachable", "no route to host", "host is down"]) {
        format!("There is no way to {host} from this network.")
    } else if has(&["certificate", "tls", "handshake"]) {
        format!("A secure connection to {host} could not be set up.")
    } else if has(&["closed the connection", "connection reset", "broken pipe"]) {
        "The daemon closed the connection.".to_owned()
    } else {
        // The last cause in the chain is the one that says what happened.
        let last = why.rsplit(": ").next().unwrap_or(why);
        let mut s = sentence(last);
        if s.chars().count() > 120 {
            s = s.chars().take(118).collect::<String>() + "…";
        }
        s
    }
}

/// `text` as a sentence: a capital at the start, a full stop at the end.
fn sentence(text: &str) -> String {
    let text = text.lines().next().unwrap_or_default().trim();
    let mut chars = text.chars();
    let mut s = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default();
    if !s.ends_with(['.', '!', '?']) {
        s.push('.');
    }
    s
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

impl Message {
    /// Whether `written`, a line her journal holds, is this one, shown
    /// before the journal had it. The time cannot be compared: this one has
    /// the desk's clock at sending, the journal the daemon's at writing. So
    /// the same words, by the same side, in the same place, written no
    /// earlier than a little before this was sent (clocks differ), so that
    /// the same words said yesterday do not take its place.
    pub fn written_as(&self, written: &Self) -> bool {
        const SKEW: chrono::Duration = chrono::Duration::minutes(2);
        self.place == written.place
            && self.hers == written.hers
            && self.text == written.text
            && written.at + SKEW >= self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "srv-hk:7438";

    #[test]
    fn a_refused_connection_says_nothing_is_listening() {
        let why = "connect to ws://srv-hk:7438: IO error: Connection refused (os error 61)";
        assert_eq!(reason(why, HOST), "Nothing is listening at srv-hk:7438.");
    }

    #[test]
    fn a_refused_token_names_the_token() {
        assert_eq!(
            reason("wrong or missing token", HOST),
            "srv-hk:7438 did not accept the token."
        );
    }

    #[test]
    fn an_unknown_name_says_no_address() {
        let why = "connect to ws://srv-hk:7438: IO error: failed to lookup address information: nodename nor servname provided, or not known";
        assert_eq!(
            reason(why, HOST),
            "No address could be found for srv-hk:7438."
        );
    }

    #[test]
    fn a_missing_variable_keeps_its_own_words() {
        assert_eq!(
            reason("HK_TOKEN is not set in the environment or .env", HOST),
            "HK_TOKEN is not set in the environment or .env."
        );
    }

    #[test]
    fn anything_else_is_its_last_cause_as_a_sentence() {
        assert_eq!(
            reason("connect to ws://x: protocol error: bad frame", HOST),
            "Bad frame."
        );
    }

    #[test]
    fn trying_again_keeps_the_reason_and_the_time_it_went_down() {
        let mut r = Reach::default();
        r.note(&LinkState::Connecting, &LinkState::Connected);
        let up = r.since;
        assert!(r.ever);
        r.note(&LinkState::Connected, &LinkState::Lost("refused".into()));
        let down = r.since;
        assert!(down >= up);
        r.note(&LinkState::Lost("refused".into()), &LinkState::Connecting);
        r.note(&LinkState::Connecting, &LinkState::Lost("refused".into()));
        assert_eq!(r.since, down, "still the moment it went down");
        assert_eq!(r.why.as_deref(), Some("refused"));
        assert_eq!(r.tries, 2);
        r.note(&LinkState::Lost("refused".into()), &LinkState::Connected);
        assert_eq!((r.why, r.tries), (None, 0));
    }

    fn line(text: &str, at: DateTime<Utc>) -> Message {
        Message {
            place: "desk-1".into(),
            hers: false,
            text: text.into(),
            at,
        }
    }

    #[test]
    fn a_line_is_known_in_the_journal_whatever_clock_wrote_it() {
        let now = Utc::now();
        let shown = line("你是?", now);
        assert!(shown.written_as(&line("你是?", now + chrono::Duration::milliseconds(800))));
        assert!(shown.written_as(&line("你是?", now - chrono::Duration::seconds(30))));
    }

    #[test]
    fn the_same_words_said_long_before_are_another_line() {
        let now = Utc::now();
        let shown = line("你是?", now);
        assert!(!shown.written_as(&line("你是?", now - chrono::Duration::hours(20))));
        assert!(!shown.written_as(&line("你是谁?", now)));
    }
}
