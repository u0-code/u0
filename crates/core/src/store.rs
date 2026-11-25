//! What the desk keeps of sessions between runs, in its home: an index of
//! the sessions it has seen (`sessions.json`) and each one's record
//! (`transcripts/<agent>/<id>.jsonl`), as t3code keeps its threads.
//!
//! The agents keep their sessions too; the desk keeps its own so that the
//! list is there the moment the window opens and a session reads at once,
//! before its agent has started. What only the desk knows lives here and
//! nowhere else: a name you gave a session, whether it is pinned,
//! archived, settled, snoozed or unread, and the worktree it works in.
//!
//! A record is the session as it came: what you said, each update the
//! agent sent, where each turn ended. Read back in order it builds the
//! same thread. With a record, the agent's session is resumed, not loaded,
//! so nothing comes twice.
//!
//! The server (`server`) is what writes both, through [`queue`].

use std::{
    io::{Read as _, Seek as _, Write as _},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agent_client_protocol::v1::schema::{SessionId, SessionUpdate, StopReason};
use serde::{Deserialize, Serialize};

use crate::{git, whole};

/// The sessions the desk has seen, by agent and the agent's id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub sessions: Vec<Meta>,
    /// Sessions you deleted whose agent cannot delete them: by agent and
    /// id, so that its list does not bring them back.
    #[serde(default)]
    pub gone: Vec<(String, String)>,
    /// What you put aside from the composer (⌘S), the latest last.
    #[serde(default)]
    pub stashed: Vec<String>,
}

/// What the desk keeps of one session: the part of a session that
/// outlives the window, which the session holds as it goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    /// The registry id of its agent.
    pub agent: String,
    /// The agent's id for it, once opened; a session is kept only then.
    #[serde(deserialize_with = "present")]
    pub id: Option<SessionId>,
    /// The project it is listed under: the folder it was begun in, which
    /// stays when it moves into a worktree.
    pub project: PathBuf,
    /// The folder it works in: its project, or its worktree once it has one.
    pub folder: PathBuf,
    /// The worktree it works in, when it has one.
    #[serde(default)]
    pub worktree: Option<git::Worktree>,
    pub title: String,
    /// The title is one you gave it: the agent's does not replace it.
    #[serde(default)]
    pub renamed: bool,
    /// When it began, and when anything last happened in it; kept as
    /// seconds since the epoch.
    #[serde(with = "epoch")]
    pub begun: SystemTime,
    #[serde(with = "epoch")]
    pub updated: SystemTime,
    #[serde(default)]
    pub flags: Flags,
    /// What you said while a turn went on, not said yet.
    #[serde(default)]
    pub queued: Vec<String>,
    /// What you were writing in it and had not sent.
    #[serde(default)]
    pub unsent: String,
    /// The id of the session it was branched off, if it was.
    #[serde(default)]
    pub from: Option<String>,
    /// Folders it works in beside its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<PathBuf>,
    /// The subagents it set to work, by id and title: their records are
    /// kept with it, and they open from its thread after a restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subagents: Vec<(String, String)>,
}

/// An id the index always has, though a session has none until opened.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<SessionId>, D::Error> {
    SessionId::deserialize(d).map(Some)
}

/// A time as the index keeps it: whole seconds since the epoch.
mod epoch {
    use std::time::SystemTime;

    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(super::secs(*t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        u64::deserialize(d).map(super::time)
    }
}

/// Where a session stands in your lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flags {
    /// Pinned to the top, in this order among the pinned.
    #[serde(default)]
    pub pinned: Option<i64>,
    /// Its place among the rest: the lower, the higher up. A session is
    /// put on top as it begins; what goes on in it does not move it.
    #[serde(default)]
    pub order: i64,
    /// Hidden, and read only, until restored.
    #[serde(default)]
    pub archived: bool,
    /// Done for now, since then (seconds since the epoch).
    #[serde(default)]
    pub settled: Option<u64>,
    /// Out of the way until then.
    #[serde(default)]
    pub snoozed: Option<u64>,
    /// Something came in it that you have not looked at.
    #[serde(default)]
    pub unread: bool,
    /// Never done on its own, however long it is left.
    #[serde(default)]
    pub kept: bool,
}

/// One line of a session's record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// When the message that follows began, in seconds since the epoch.
    At(u64),
    You(String),
    Update(Box<SessionUpdate>),
    Ended {
        reason: StopReason,
        secs: u64,
    },
    Note {
        text: String,
        trouble: bool,
    },
}

pub fn index_path(home: &Path) -> PathBuf {
    home.join("sessions.json")
}

pub fn record_path(home: &Path, agent: &str, id: &str) -> PathBuf {
    home.join("transcripts")
        .join(agent)
        .join(format!("{}.jsonl", safe(id)))
}

/// An id as a file name: what is not a letter, a digit, `-` or `_` is `_`.
fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The index in `home`; empty when there is none. One that does not read
/// is refused with why, and nothing is saved over it.
pub fn load(home: &Path) -> Result<Index, String> {
    whole::read_json(&index_path(home)).map_err(|e| {
        format!("{e}. Move it away to start afresh; until then sessions are not remembered.")
    })
}

/// The index as bytes, to be written whole.
pub fn bytes(index: &Index) -> Vec<u8> {
    serde_json::to_vec_pretty(index).expect("the index serializes")
}

/// Add `events` to the end of the record at `path`.
pub fn append(path: &Path, events: &[Event]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for e in events {
        text.push_str(&serde_json::to_string(e).expect("an event serializes"));
        text.push('\n');
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    // A line a crash cut short is ended first, so that it does not take
    // the first of these with it.
    if file.seek(std::io::SeekFrom::End(-1)).is_ok() {
        let mut last = [0u8];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            text.insert(0, '\n');
        }
    }
    file.write_all(text.as_bytes())
}

/// The record at `path`, its lines that read; `None` when there is none.
/// A line cut short by a crash is left out.
pub fn read(path: &Path) -> Option<Vec<Event>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
    )
}

/// What an agent said a session had cost, when it said it: in seconds
/// since the epoch, the amount so far (agents say the session's total,
/// not the turn's) and its currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spend {
    pub at: u64,
    pub amount: f64,
    pub currency: String,
}

/// What the record `events` says the session cost, each time its agent
/// said, at the time of the message before it.
pub fn spent(events: &[Event]) -> Vec<Spend> {
    let mut at = 0;
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::At(t) => at = *t,
            Event::Update(u) => {
                if let SessionUpdate::UsageUpdate(u) = &**u
                    && let Some(c) = &u.cost
                {
                    out.push(Spend {
                        at,
                        amount: c.amount,
                        currency: c.currency.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a record ends in a turn it never saw end: Ghost stopped before
/// the rest was written, and resuming would leave that rest out for good.
pub fn cut_short(events: &[Event]) -> bool {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::You(_) => Some(true),
            Event::Ended { .. } | Event::Note { .. } => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

/// What the writer does, in the order it was asked.
pub enum Job {
    Append(PathBuf, Vec<Event>),
    /// A file the desk keeps, written whole (`whole::write`).
    Write(PathBuf, Vec<u8>),
    Remove(PathBuf),
    /// Run once everything asked before is done: a record read back, a
    /// file read, changed and written again.
    Then(Box<dyn FnOnce() + Send>),
}

/// Hand `job` to the one thread that writes the desk's files, so that the
/// window never waits on the disk, two writes of one file never cross, and
/// a file is read back with all that was written to it.
pub fn queue(job: Job) {
    use std::sync::{Mutex, OnceLock, mpsc};
    static WRITER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let tx = WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("ghost-store".into())
            .spawn(move || {
                for job in rx {
                    let done = match job {
                        Job::Append(path, events) => append(&path, &events),
                        Job::Write(path, bytes) => whole::write(&path, &bytes),
                        Job::Remove(path) => match std::fs::remove_file(&path) {
                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                            _ => Ok(()),
                        },
                        Job::Then(then) => {
                            then();
                            Ok(())
                        }
                    };
                    if let Err(e) = done {
                        tracing::warn!("a file of the desk's could not be written: {e}");
                    }
                }
            })
            .expect("the writer thread starts");
        Mutex::new(tx)
    });
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(job);
    }
}

/// Wait until what was handed to the writer is written.
pub fn settle() {
    let (tx, rx) = std::sync::mpsc::channel();
    queue(Job::Then(Box::new(move || {
        let _ = tx.send(());
    })));
    let _ = rx.recv_timeout(Duration::from_secs(5));
}

/// Seconds since the epoch.
pub fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn time(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_session_cost_is_read_from_its_record_at_the_time_said() {
        let usage = |amount: f64| {
            Event::Update(Box::new(
                serde_json::from_value(serde_json::json!({
                    "sessionUpdate": "usage_update", "used": 1, "size": 2,
                    "cost": {"amount": amount, "currency": "USD"},
                }))
                .unwrap(),
            ))
        };
        let events = [Event::At(100), usage(0.5), Event::At(200), usage(1.25)];
        let spent = spent(&events);
        assert_eq!(
            spent.iter().map(|s| (s.at, s.amount)).collect::<Vec<_>>(),
            [(100, 0.5), (200, 1.25)]
        );
    }

    #[test]
    fn a_record_reads_back_in_order_and_a_line_cut_short_loses_only_itself() {
        let dir = std::env::temp_dir().join(format!("ghost-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = record_path(&dir, "claude-acp", "a/b");
        assert!(path.ends_with("claude-acp/a_b.jsonl"));
        let update: SessionUpdate = serde_json::from_str(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#,
        )
        .unwrap();
        append(
            &path,
            &[Event::You("go".into()), Event::Update(Box::new(update))],
        )
        .unwrap();
        append(
            &path,
            &[Event::Ended {
                reason: StopReason::EndTurn,
                secs: 3,
            }],
        )
        .unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"{\"you\":\"cut sh").unwrap();
        append(&path, &[Event::You("next".into())]).unwrap();
        let events = read(&path).unwrap();
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], Event::You(t) if t == "go"));
        assert!(matches!(events[2], Event::Ended { secs: 3, .. }));
        assert!(matches!(&events[3], Event::You(t) if t == "next"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_read_back_holds_what_was_queued_to_it_before() {
        let dir = tempfile::tempdir().unwrap();
        let path = record_path(dir.path(), "x", "read-after");
        queue(Job::Append(path.clone(), vec![Event::You("go".into())]));
        let (tx, rx) = std::sync::mpsc::channel();
        queue(Job::Then(Box::new(move || {
            let _ = tx.send(read(&path));
        })));
        let events = rx.recv().unwrap().expect("a record");
        assert!(matches!(&events[..], [Event::You(t)] if t == "go"));
    }

    #[test]
    fn a_record_that_stops_inside_a_turn_is_cut_short() {
        let you = || Event::You("go".into());
        let ended = || Event::Ended {
            reason: StopReason::EndTurn,
            secs: 1,
        };
        let update = || {
            Event::Update(Box::new(
                serde_json::from_str(
                    r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Do"}}"#,
                )
                .unwrap(),
            ))
        };
        assert!(cut_short(&[you(), update()]));
        assert!(!cut_short(&[you(), update(), ended(), update()]));
        assert!(!cut_short(&[update()]));
    }

    #[test]
    fn an_index_keeps_ids_as_strings_and_times_as_seconds_and_needs_an_id() {
        let text = r#"{"sessions":[{"agent":"a","id":"s1","project":"/p","folder":"/p","title":"t","begun":5,"updated":7}]}"#;
        let index: Index = serde_json::from_str(text).unwrap();
        let meta = &index.sessions[0];
        assert_eq!(meta.id, Some(SessionId::new("s1")));
        assert_eq!(meta.updated, time(7));
        let back: serde_json::Value = serde_json::from_slice(&bytes(&index)).unwrap();
        assert_eq!(back["sessions"][0]["id"], "s1");
        assert_eq!(back["sessions"][0]["begun"], 5);
        let no_id = text.replace(r#""id":"s1","#, "");
        assert!(serde_json::from_str::<Index>(&no_id).is_err());
    }

    #[test]
    fn an_index_in_another_form_is_refused_not_replaced() {
        let dir = std::env::temp_dir().join(format!("ghost-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(index_path(&dir), r#"{"threads":[]}"#).unwrap();
        assert!(load(&dir).unwrap_err().contains("Move it away"));
        std::fs::write(index_path(&dir), b"{\"sessions\":[\xff]}").unwrap();
        assert!(load(&dir).unwrap_err().contains("Move it away"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
