//! Her life, as an append-only, hash-chained log.
//!
//! Every state in the core is derived from this log. There is no `update` and
//! no `delete`; the `SQLite` schema carries triggers that abort both. The chain
//! is not a security boundary (anyone with the file can rewrite it), it is an
//! honesty boundary: tampering has to be deliberate, and it leaves a mark the
//! World can later detect.
//!
//! The [`Event`] enum is a forever format. Add variants and optional fields;
//! never rename or remove.

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    identity::Seal,
    life::Block,
    social::{Actor, Place},
};

/// File name of the journal inside a ghost directory.
pub const JOURNAL_FILE: &str = "journal.db";

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const COLUMNS: &str = "seq, at, kind, payload, prev_hash, hash";

/// Something that happened to her, or something she did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The first entry of every journal. Carries the seal of her persona
    /// as it was at birth.
    Born {
        name: String,
        seal: Seal,
    },
    /// Her body was not running between `since` and this entry.
    Dormant {
        since: DateTime<Utc>,
    },
    Woke,
    Slept,
    /// What she intends to do with a day. `day` is the local date she woke on.
    Planned {
        day: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weather: Option<String>,
        blocks: Vec<Block>,
    },
    /// Something she experienced. `block` indexes the day's plan when the
    /// experience is how a planned block actually went.
    Lived {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<u32>,
    },
    /// A message reached her. She has not necessarily looked at it yet.
    Heard {
        channel: String,
        place: Place,
        from: Actor,
        /// The channel's own id for the message.
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        /// It was meant for her: a direct message, a mention, or a reply.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        addressed: bool,
        /// Where the channel kept the images in it, in the order of their
        /// `[image]` placeholders in `text`. The links expire; what she made
        /// of each image is a `Viewed` entry.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
    },
    /// She looked at a place and saw everything up to journal entry `upto`.
    Read {
        channel: String,
        place: String,
        upto: i64,
    },
    /// She sent a message.
    Said {
        channel: String,
        place: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
    /// She reacted to a message.
    Reacted {
        channel: String,
        place: String,
        to: String,
        emoji: String,
    },
    /// She posted something publicly, to no one in particular.
    Posted {
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        text: String,
    },
    /// What she took away from scrolling a feed.
    Saw {
        channel: String,
        text: String,
    },
    /// A decision she made, and why.
    Decided {
        about: String,
        decision: String,
        reason: String,
    },
    /// A shift in how she feels about someone. `toward` is a person key.
    Felt {
        toward: String,
        delta: i8,
        why: String,
    },
    /// Something about a person worth keeping. `about` is a person key.
    Noted {
        about: String,
        text: String,
        /// The place she was in when she learned it, so that what was said
        /// in private is not later repeated somewhere else.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// What she had in front of her when she wrote it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// Something she means to do later.
    Intended {
        due: DateTime<Utc>,
        what: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        about: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// She said she would come back with this. Following through on it
        /// is not chasing.
        #[serde(default, skip_serializing_if = "is_false")]
        owed: bool,
    },
    /// An intention was carried out or dropped. `intention` is its seq.
    Settled {
        intention: i64,
        outcome: String,
    },
    /// She ended a relationship. There is no event that undoes this.
    Left {
        person: String,
        why: String,
    },
    /// A reflection from her sleep cycle: on her own day, or, with
    /// `channel`, on what went on there.
    Reflected {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// A hard fact about her life, registered in the canon.
    Canon {
        fact: String,
        source: String,
    },
    /// She took something on: a piece of work that will take more than one
    /// thought. `id` names its directory.
    Undertook {
        id: String,
        brief: String,
        /// Who it is for, if anyone asked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asked_by: Option<String>,
        /// Where they asked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
    },
    /// One sitting of work on an undertaking.
    Worked {
        id: String,
        /// Where she got to, in her own words.
        summary: String,
        steps: u32,
    },
    /// In the middle of an undertaking she needed something from whoever
    /// it is for, and stopped to ask.
    Asked {
        id: String,
        question: String,
    },
    /// Something was added to an undertaking after it began: more of what
    /// was wanted, or the answer to what she asked.
    Added {
        id: String,
        text: String,
    },
    /// An undertaking ended: `finished`, `blocked`, `gave_up` or `dropped`.
    Concluded {
        id: String,
        outcome: String,
        summary: String,
    },
    /// What a channel reported about how her public posts are doing.
    Measured {
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        followers: Option<u64>,
        #[serde(default)]
        posts: Vec<PostStats>,
    },
    /// Someone said they are a person she knows on another platform, and
    /// she asked for proof: a word she made up, to be sent to her from the
    /// account she already knows.
    Challenged {
        /// Who says so.
        claimant: String,
        /// Who they say they are.
        claimed: String,
        word: String,
    },
    /// Two accounts were shown to be one person: the word came back from
    /// the right place.
    Linked {
        one: String,
        other: String,
    },
    /// She put what she knows of someone in order: a short account that
    /// replaces the one before it. What she could have learned where others
    /// were present is kept apart from what they told her alone.
    Profiled {
        person: String,
        /// What she knows of them from shared and public places.
        shared: String,
        /// What they told her in private. Never for use anywhere else.
        #[serde(default)]
        private: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// Something she is after over months, in her own words: a direction,
    /// not a task. It replaces an earlier one when she has changed her
    /// mind; an empty `what` with `replaces` lets that one go.
    Pursued {
        what: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// Something doing a piece of work taught her about how to do things:
    /// what she would do differently, or a way that worked and was not
    /// obvious. Replaces an earlier one when she has learned better.
    Practised {
        lesson: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// She put what she wants to keep of the earlier part of a conversation
    /// into a few lines, replacing whatever she kept of it before. The
    /// messages up to entry `upto` stand behind it; later ones are still
    /// read as they were written.
    Digested {
        place: String,
        upto: i64,
        text: String,
        /// Everything it stands for, the earlier notes it replaces included.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// She looked something up on the web, and this is what came back: kept
    /// so that what she found out is something she knows, and can find
    /// again, and not only something that was in front of her once.
    LookedUp {
        query: String,
        found: String,
        /// The place she was in when she looked it up.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
    },
    /// Her persona as it stands from here on: written at birth and again
    /// whenever it is revised. The whole text is kept, so the journal alone
    /// says who she was at any point in her life.
    Described {
        /// Her persona. The field keeps the name it was first written
        /// under: the journal is a forever format.
        certificate: String,
        seal: Seal,
    },
    /// Something she worked out about her own craft: what she does well in
    /// public, what falls flat, what to try. Replaces an earlier lesson when
    /// she has changed her mind.
    Learned {
        lesson: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// She looked at an image in a message she heard. `message` is the seq
    /// of that `Heard` entry and `image` the image's place among its images.
    Viewed {
        message: i64,
        image: u32,
        /// What she saw: what is in it, what it says, what it means.
        what: String,
        /// The name it is kept under in `media/`, if it was kept.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
}

/// The stretch of her journal a piece of memory was made from: entries
/// `from` to `upto`, which happened between `since` and `until`. The
/// entries are still there to be read; this says which ones they were, and
/// when, so the memory can be dated however far back it is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sources {
    pub from: i64,
    pub upto: i64,
    pub since: DateTime<Utc>,
    pub until: DateTime<Utc>,
}

/// How one public post is doing, as the platform counts it.
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Every kind of event there is, by its variant and by the name it is
/// written under. One list gives both [`Event::kind`] and [`Event::KINDS`],
/// and the match it makes is exhaustive: a new variant does not compile
/// until it is named here, and once it is, the compatibility test asks for
/// an entry of it as it is written (`tests/fixtures/events.jsonl`).
macro_rules! kinds {
    ($($variant:ident => $name:literal),* $(,)?) => {
        impl Event {
            /// The name this event is written under.
            pub fn kind(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }

            /// The name of every kind of event, in the order they were added.
            pub const KINDS: &'static [&'static str] = &[$($name),*];
        }
    };
}

kinds! {
    Born => "born",
    Dormant => "dormant",
    Woke => "woke",
    Slept => "slept",
    Planned => "planned",
    Lived => "lived",
    Heard => "heard",
    Read => "read",
    Said => "said",
    Reacted => "reacted",
    Posted => "posted",
    Saw => "saw",
    Decided => "decided",
    Felt => "felt",
    Noted => "noted",
    Intended => "intended",
    Settled => "settled",
    Left => "left",
    Reflected => "reflected",
    Canon => "canon",
    Undertook => "undertook",
    Worked => "worked",
    Asked => "asked",
    Added => "added",
    Concluded => "concluded",
    Measured => "measured",
    Learned => "learned",
    Profiled => "profiled",
    Challenged => "challenged",
    Linked => "linked",
    Digested => "digested",
    Pursued => "pursued",
    Practised => "practised",
    LookedUp => "looked_up",
    Described => "described",
    Viewed => "viewed",
}

/// One line of her journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub event: Event,
    pub prev_hash: String,
    pub hash: String,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    /// Create a new, empty journal. Fails if one already exists.
    pub fn create(path: &Path) -> Result<Self> {
        if path.exists() {
            return Err(Error::AlreadyBorn(path.to_path_buf()));
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an existing journal. Fails if there is none.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::NoJournal(path.to_path_buf()));
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    /// Append an event, stamped now.
    pub fn append(&mut self, event: Event) -> Result<Entry> {
        self.append_at(Utc::now(), event)
    }

    /// Append an event with an explicit timestamp.
    pub fn append_at(&mut self, at: DateTime<Utc>, event: Event) -> Result<Entry> {
        let tx = self.conn.transaction()?;
        let (seq, prev_hash) = tx
            .query_row(
                "SELECT seq, hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, i64>(0)? + 1, r.get::<_, String>(1)?)),
            )
            .optional()?
            .unwrap_or((1, GENESIS.to_owned()));
        // An entry holds the time to the microsecond. The entry handed back
        // carries the time as it was written, not the finer one the clock
        // gave, so that it is the same entry a reader gets later.
        let at_text = stamp(&at);
        let at = parse_stamp(&at_text).unwrap_or(at);
        let payload = serde_json::to_string(&event)?;
        let hash = hash_entry(&prev_hash, seq, &at_text, event.kind(), &payload);
        tx.execute(
            "INSERT INTO events (seq, at, kind, payload, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![seq, at_text, event.kind(), payload, prev_hash, hash],
        )?;
        tx.commit()?;
        Ok(Entry {
            seq,
            at,
            event,
            prev_hash,
            hash,
        })
    }

    pub fn len(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?)
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// The seq of the newest entry, or 0.
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?)
    }

    pub fn first(&self) -> Result<Option<Entry>> {
        self.one(&format!(
            "SELECT {COLUMNS} FROM events ORDER BY seq ASC LIMIT 1"
        ))
    }

    pub fn last(&self) -> Result<Option<Entry>> {
        self.one(&format!(
            "SELECT {COLUMNS} FROM events ORDER BY seq DESC LIMIT 1"
        ))
    }

    pub fn entry(&self, seq: i64) -> Result<Option<Entry>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM events WHERE seq = ?1"),
                [seq],
                row_to_entry,
            )
            .optional()?)
    }

    /// The most recent entry whose kind is one of `kinds`.
    pub fn last_of(&self, kinds: &[&str]) -> Result<Option<Entry>> {
        Ok(self.last_n_of(kinds, 1)?.pop())
    }

    /// The last `n` entries whose kind is one of `kinds`, oldest first.
    pub fn last_n_of(&self, kinds: &[&str], n: usize) -> Result<Vec<Entry>> {
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE kind IN ({placeholders}) ORDER BY seq DESC LIMIT {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(kinds.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Entries of the given kinds with `seq > after`, oldest first.
    pub fn of_kinds_after(&self, kinds: &[&str], after: i64) -> Result<Vec<Entry>> {
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE seq > {after} AND kind IN ({placeholders}) ORDER BY seq ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        stmt.query_map(rusqlite::params_from_iter(kinds.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect()
    }

    /// Read up to `limit` entries starting at `from_seq` (inclusive).
    pub fn read(&self, from_seq: i64, limit: usize) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM events WHERE seq >= ?1 ORDER BY seq ASC LIMIT ?2"
        ))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![from_seq, limit], row_to_entry)?;
        rows.map(|r| r.map_err(Error::from)).collect()
    }

    /// The last `n` entries, oldest first.
    pub fn tail(&self, n: usize) -> Result<Vec<Entry>> {
        let total = self.last_seq()?;
        let n = i64::try_from(n).unwrap_or(i64::MAX);
        let from = (total - n + 1).max(1);
        self.read(from, usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// Walk the whole chain and recompute every hash. Returns the length.
    pub fn verify(&self) -> Result<i64> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM events ORDER BY seq ASC"))?;
        let mut rows = stmt.query([])?;
        let mut expected_prev = GENESIS.to_owned();
        let mut expected_seq = 1;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            let seq: i64 = row.get(0)?;
            let at: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let payload: String = row.get(3)?;
            let prev_hash: String = row.get(4)?;
            let hash: String = row.get(5)?;
            if seq != expected_seq {
                return Err(Error::BrokenChain {
                    seq,
                    reason: format!("expected seq {expected_seq}"),
                });
            }
            if prev_hash != expected_prev {
                return Err(Error::BrokenChain {
                    seq,
                    reason: "prev_hash does not match previous entry".into(),
                });
            }
            let recomputed = hash_entry(&prev_hash, seq, &at, &kind, &payload);
            if recomputed != hash {
                return Err(Error::BrokenChain {
                    seq,
                    reason: "hash does not match contents".into(),
                });
            }
            expected_prev = hash;
            expected_seq += 1;
            count += 1;
        }
        Ok(count)
    }

    fn one(&self, sql: &str) -> Result<Option<Entry>> {
        Ok(self.conn.query_row(sql, [], row_to_entry).optional()?)
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Self { conn }
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let at: String = row.get(1)?;
    let payload: String = row.get(3)?;
    let at = parse_stamp(&at).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let event: Event = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Entry {
        seq: row.get(0)?,
        at,
        event,
        prev_hash: row.get(4)?,
        hash: row.get(5)?,
    })
}

/// The canonical text form of a timestamp. Sorts lexicographically.
pub(crate) fn stamp(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(crate) fn parse_stamp(s: &str) -> std::result::Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc))
}

/// `blake3(prev_hash ‖ seq ‖ at ‖ kind ‖ payload)`, hex.
pub fn hash_entry(prev_hash: &str, seq: i64, at: &str, kind: &str, payload: &str) -> String {
    let mut h = blake3::Hasher::new();
    for part in [prev_hash, &seq.to_string(), at, kind, payload] {
        h.update(part.as_bytes());
        h.update(b"\n");
    }
    h.finalize().to_hex().to_string()
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS events (
    seq       INTEGER PRIMARY KEY,
    at        TEXT    NOT NULL,
    kind      TEXT    NOT NULL,
    payload   TEXT    NOT NULL,
    prev_hash TEXT    NOT NULL,
    hash      TEXT    NOT NULL UNIQUE
);
CREATE INDEX IF NOT EXISTS events_kind ON events (kind, seq);
CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'journal is append-only'); END;
CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'journal is append-only'); END;
";

#[cfg(test)]
mod tests {
    use super::*;

    fn lived(text: &str) -> Event {
        Event::Lived {
            text: text.into(),
            block: None,
        }
    }

    #[test]
    fn chain_links_and_verifies() {
        let mut j = Journal::in_memory();
        let a = j.append(Event::Woke).unwrap();
        let b = j.append(lived("rain")).unwrap();
        assert_eq!(a.seq, 1);
        assert_eq!(a.prev_hash, GENESIS);
        assert_eq!(b.prev_hash, a.hash);
        assert_eq!(j.verify().unwrap(), 2);
        assert_eq!(j.last().unwrap().unwrap(), b);
        assert_eq!(j.entry(1).unwrap().unwrap(), a);
    }

    #[test]
    fn an_entry_is_handed_back_as_it_was_written() {
        // A clock finer than the journal keeps: Linux gives nanoseconds,
        // and an entry is written to the microsecond.
        let fine = DateTime::parse_from_rfc3339("2026-11-13T11:06:48.502265504Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut j = Journal::in_memory();
        let written = j.append_at(fine, lived("rain")).unwrap();
        assert_eq!(written.at.timestamp_subsec_nanos(), 502_265_000);
        assert_eq!(j.last().unwrap().unwrap(), written);
        assert_eq!(j.verify().unwrap(), 1);
    }

    #[test]
    fn storage_refuses_rewrites() {
        let mut j = Journal::in_memory();
        j.append(Event::Woke).unwrap();
        assert!(
            j.conn
                .execute("UPDATE events SET kind = 'slept'", [])
                .is_err()
        );
        assert!(j.conn.execute("DELETE FROM events", []).is_err());
        assert_eq!(j.len().unwrap(), 1);
    }

    #[test]
    fn filters_by_kind() {
        let mut j = Journal::in_memory();
        j.append(Event::Woke).unwrap();
        j.append(lived("x")).unwrap();
        j.append(Event::Slept).unwrap();
        j.append(lived("y")).unwrap();
        let e = j.last_of(&["woke", "slept"]).unwrap().unwrap();
        assert_eq!(e.event, Event::Slept);
        assert!(j.last_of(&["canon"]).unwrap().is_none());
        let after = j.of_kinds_after(&["lived"], 2).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].seq, 4);
    }

    #[test]
    fn tail_returns_newest_in_order() {
        let mut j = Journal::in_memory();
        for i in 0..5 {
            j.append(lived(&i.to_string())).unwrap();
        }
        let t = j.tail(2).unwrap();
        assert_eq!(t.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(j.tail(100).unwrap().len(), 5);
    }

    #[test]
    fn optional_fields_stay_out_of_the_payload() {
        let e = lived("x");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"kind":"lived","data":{"text":"x"}}"#
        );
    }
}
