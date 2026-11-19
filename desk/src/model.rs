//! What the desk knows about her, as read from the daemon.
//!
//! Everything here is shaped from the protocol's JSON. The desk does not
//! depend on `ghost-core`: it is a client like any presence, and knows her
//! journal only as the daemon serves it.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::{
    Abilities, Attention, CoreInfo, GhostCard, Listed, Look, Work, WorkContext, WorkLog,
};
use serde_json::Value;

/// One line of her journal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    pub think: String,
    /// The context window of the brain she looks with, in tokens, where
    /// her daemon knows it.
    pub think_window: Option<u64>,
    /// Whether her day has her awake now, when her daemon says.
    pub awake: Option<bool>,
    /// What her day has her doing now, in her plan's words, when it says.
    pub doing: Option<String>,
    /// What she can do besides talk, as her daemon runs her; `None` when
    /// what it said could not be read.
    pub abilities: Option<Abilities>,
    /// Where her daemon runs, as it says; `None` when what it said could
    /// not be read.
    pub core: Option<CoreInfo>,
    /// Whether her daemon runs on this computer, so that the paths she
    /// gives can be opened here: see [`on_this_computer`]. Worked out off
    /// the main thread, as the status is read.
    pub here: bool,
}

/// Whether the daemon that says `core` runs on this computer: the file it
/// says it keeps its id in is here, and holds that id. Finding it is
/// finding the daemon's own file system, which is what opening the paths
/// it gives needs. Two daemons on this computer each find their own; one
/// elsewhere, even under the same host name or with a home at the same
/// path, finds nothing, or another id. A host name is never asked: two
/// computers can share one.
pub fn on_this_computer(core: &CoreInfo) -> bool {
    !core.id.is_empty()
        && std::path::Path::new(&core.id_file).is_absolute()
        && std::fs::read_to_string(&core.id_file).is_ok_and(|kept| kept.trim() == core.id)
}

impl Status {
    /// Her status as her daemon gave it, with whether it runs here looked
    /// up: this reads one small file, so it is not for the main thread.
    pub fn read(v: &Value) -> Self {
        let mut status = Self::from_value(v);
        status.here = status.core.as_ref().is_some_and(on_this_computer);
        status
    }

    /// The directory her machine works in, on her daemon's computer, when
    /// it says.
    pub fn machine_dir(&self) -> Option<PathBuf> {
        let machine = self.abilities.as_ref()?.machine.as_ref()?;
        machine.dir.as_deref().map(PathBuf::from)
    }

    /// Where a file she named is on her daemon's computer: as named, if
    /// that is a full path; else under `within` (the folder of the work
    /// that named it), else under her machine's directory. `None` when
    /// there is nothing to put it under.
    pub fn locate(&self, file: &str, within: Option<&str>) -> Option<PathBuf> {
        let named = Path::new(file);
        if named.is_absolute() {
            return Some(named.to_path_buf());
        }
        within
            .map(PathBuf::from)
            .or_else(|| self.machine_dir())
            .map(|dir| dir.join(named))
    }

    /// Her status as her daemon gave it, read as it is: nothing looked up.
    pub fn from_value(v: &Value) -> Self {
        let tz: Tz = s(v, "timezone").parse().unwrap_or(chrono_tz::UTC);
        let moment = v.get("moment");
        Self {
            awake: moment
                .and_then(|m| m.get("phase"))
                .and_then(Value::as_str)
                .map(|phase| phase == "awake"),
            doing: moment
                .and_then(|m| m.get("doing"))
                .and_then(|d| d.get("what"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_owned),
            name: s(v, "name"),
            city: s(v, "city"),
            tz,
            local_time: s(v, "local_time"),
            think: v.get("brains").map(|b| s(b, "think")).unwrap_or_default(),
            think_window: v
                .get("windows")
                .and_then(|w| w.get("think"))
                .and_then(Value::as_u64),
            abilities: v
                .get("abilities")
                .and_then(|a| serde_json::from_value(a.clone()).ok()),
            core: v
                .get("core")
                .and_then(|c| serde_json::from_value(c.clone()).ok()),
            here: false,
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

/// Why a daemon cannot be reached, in a sentence a person reads, from the
/// error the link gave. It does not name the daemon: it is read under its
/// name and address. The error itself stays a click away; this is what
/// most people need to know first.
pub fn reason(why: &str) -> String {
    let w = why.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|x| w.contains(x));
    if has(&["connection refused", "os error 61", "os error 111"]) {
        "Nothing is listening there.".to_owned()
    } else if has(&["unauthorized", "wrong or missing token", "401", "403"]) {
        "It did not accept the token.".to_owned()
    } else if has(&["is not set in the environment"]) {
        // The settings' own sentence names the variable, and says it well.
        sentence(why)
    } else if has(&[
        "failed to lookup",
        "nodename nor servname",
        "name or service not known",
        "no such host",
    ]) {
        "Its address could not be found.".to_owned()
    } else if has(&["timed out", "timeout", "deadline"]) {
        "It did not answer in time.".to_owned()
    } else if has(&["network is unreachable", "no route to host", "host is down"]) {
        "It cannot be reached from this network.".to_owned()
    } else if has(&["certificate", "tls", "handshake"]) {
        "A secure connection to it could not be set up.".to_owned()
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
    /// A stretch of the journal that ends at its newest entry, oldest
    /// first: the tail at a first connection, or what was written since the
    /// newest the desk held. Added to what is held.
    Journal(Vec<Entry>),
    /// One new entry, as it is written.
    Entry(Entry),
    /// The entries of one place, as `journal.place` says, however old.
    /// Added to what is held.
    Place(Vec<Entry>),
    /// Something you did to her could not be done, and why, in a line.
    Refused {
        order: crate::link::Order,
        why: String,
    },
    /// Who the daemon runs, as `ghost.list` says.
    Roster(Vec<Listed>),
    /// The work she has in hand and what ended lately, as `work.list` says.
    Work(Vec<Work>),
    /// Her sessions at a console, newest first, as `sessions.list` says.
    Sessions(Vec<ghost_link::protocol::Session>),
    /// What one of the daemon's characters, by her id, is waiting on you
    /// for, as `attention.list` says.
    Attention {
        ghost: String,
        items: Vec<Attention>,
    },
    /// What she was doing about a place, by its id on the console
    /// channel, was asked to stop; and whether anything was.
    Stopped {
        place: String,
        stopped: bool,
    },
    /// Something done to a character from Attention is done; or why not.
    Ordered {
        why: Option<String>,
    },
    /// What she called while she looked at a place, by key, as
    /// `look.steps` says; or why it could not be read.
    Looks {
        place: String,
        looks: Result<Vec<Look>, String>,
    },
    /// The steps of a work or a part, by id, as `work.steps` says; or why
    /// they could not be read.
    Steps {
        work: String,
        log: Result<WorkLog, String>,
    },
    /// What a work's newest step put in front of its model, by id, as
    /// `work.steps` says; or why it could not be read.
    Context {
        work: String,
        context: Result<Option<WorkContext>, String>,
    },
}

/// A message in the conversation between you and her from the desk, in
/// one place: each session is a place of its own on the console channel.
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

/// A stop asked for from the desk in one place, by its id on the console
/// channel: when the daemon answered, and whether anything was stopped.
/// The journal has no entry for it, so the desk keeps it while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub place: String,
    pub at: DateTime<Utc>,
    pub stopped: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_brains_window_is_what_her_daemon_says_and_none_from_one_that_does_not() {
        let said = serde_json::json!({
            "brains": {"think": "main"},
            "windows": {"think": 1_048_576, "tick": null},
        });
        assert_eq!(Status::from_value(&said).think_window, Some(1_048_576));
        let older = serde_json::json!({"brains": {"think": "main"}});
        assert_eq!(Status::from_value(&older).think_window, None);
    }

    #[test]
    fn what_she_can_do_is_what_her_daemon_says_and_unknown_from_one_that_does_not() {
        let said = serde_json::json!({
            "abilities": {"work": true, "machine": {"kind": "local", "spec": "local:"}, "web": true},
        });
        let abilities = Status::from_value(&said).abilities.unwrap();
        assert!(abilities.local() && abilities.web);
        assert!(abilities.agents.is_empty());
        let older = serde_json::json!({"brains": {"think": "main"}});
        assert_eq!(Status::from_value(&older).abilities, None);
    }

    /// A status from a daemon that keeps its id in `id_file`, as `id`.
    fn from_core(id: &str, id_file: &std::path::Path) -> Status {
        Status::read(&serde_json::json!({
            "abilities": {"work": true, "machine": {"kind": "local", "spec": "local:",
                "dir": "/srv/ghost/ghosts/solver/home"}},
            "core": {"id": id, "id_file": id_file, "host": "studio"},
        }))
    }

    #[test]
    fn a_core_is_on_this_computer_only_when_its_own_id_is_found_where_it_says() {
        let dir = std::env::temp_dir().join(format!("ghost-desk-here-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("other")).unwrap();
        let file = dir.join("core-id");
        std::fs::write(&file, "aaaa\n").unwrap();
        std::fs::write(dir.join("other/core-id"), "bbbb\n").unwrap();

        assert!(from_core("aaaa", &file).here, "the core that wrote it");
        assert!(
            from_core("bbbb", &dir.join("other/core-id")).here,
            "a second core on this computer is here too, by its own"
        );
        assert!(
            !from_core("cccc", &file).here,
            "a core elsewhere whose home is at the same path is not"
        );
        assert!(
            !from_core("aaaa", &dir.join("gone/core-id")).here,
            "nor one whose file is not on this computer at all"
        );
        assert!(
            !from_core("aaaa", std::path::Path::new("core-id")).here,
            "a path that is not a full one is nobody's"
        );
        let older = Status::read(&serde_json::json!({"name": "Ghost"}));
        assert!(older.core.is_none() && !older.here, "it never said");
    }

    #[test]
    fn a_file_she_names_is_found_under_her_works_folder_else_her_machine() {
        let status = from_core("aaaa", std::path::Path::new("/nowhere/core-id"));
        assert_eq!(
            status.locate("/tmp/report.md", Some("/w")),
            Some(PathBuf::from("/tmp/report.md"))
        );
        assert_eq!(
            status.locate("report.md", Some("/Users/me/Documents/Ghost/solver/w1")),
            Some(PathBuf::from(
                "/Users/me/Documents/Ghost/solver/w1/report.md"
            ))
        );
        assert_eq!(
            status.locate("out/report.md", None),
            Some(PathBuf::from("/srv/ghost/ghosts/solver/home/out/report.md"))
        );
        let older = Status::from_value(&serde_json::json!({}));
        assert_eq!(older.locate("report.md", None), None, "nowhere to put it");
    }

    #[test]
    fn her_moment_says_whether_she_is_awake_and_what_her_day_has_her_doing() {
        let said = serde_json::json!({
            "moment": {"phase": "awake", "doing": {"what": "reading the QQ group"}},
        });
        let status = Status::from_value(&said);
        assert_eq!(status.awake, Some(true));
        assert_eq!(status.doing.as_deref(), Some("reading the QQ group"));
        let asleep = Status::from_value(&serde_json::json!({"moment": {"phase": "asleep"}}));
        assert_eq!((asleep.awake, asleep.doing), (Some(false), None));
        let older = Status::from_value(&serde_json::json!({}));
        assert_eq!((older.awake, older.doing), (None, None));
    }

    #[test]
    fn a_refused_connection_says_nothing_is_listening() {
        let why = "connect to ws://srv-hk:7438: IO error: Connection refused (os error 61)";
        assert_eq!(reason(why), "Nothing is listening there.");
    }

    #[test]
    fn a_refused_token_names_the_token() {
        assert_eq!(
            reason("wrong or missing token"),
            "It did not accept the token."
        );
    }

    #[test]
    fn an_unknown_name_says_no_address() {
        let why = "connect to ws://srv-hk:7438: IO error: failed to lookup address information: nodename nor servname provided, or not known";
        assert_eq!(reason(why), "Its address could not be found.");
    }

    #[test]
    fn a_missing_variable_keeps_its_own_words() {
        assert_eq!(
            reason("HK_TOKEN is not set in the environment or .env"),
            "HK_TOKEN is not set in the environment or .env."
        );
    }

    #[test]
    fn anything_else_is_its_last_cause_as_a_sentence() {
        assert_eq!(
            reason("connect to ws://x: protocol error: bad frame"),
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
