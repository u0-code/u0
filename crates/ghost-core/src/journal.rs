//! Her life, as an append-only, hash-chained log.
//!
//! Every state in the core is derived from this log. There is no `update` and
//! no `delete`; the `SQLite` schema carries triggers that abort both. The chain
//! is not a security boundary (anyone with the file can rewrite it), it is an
//! honesty boundary: tampering has to be deliberate, and it leaves a mark the
//! World can later detect.

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, identity::Seal};

/// File name of the journal inside a ghost directory.
pub const JOURNAL_FILE: &str = "journal.db";

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Something that happened to her.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The first entry of every journal. Carries the seal of her certificate.
    Born {
        name: String,
        seal: Seal,
    },
    Woke,
    Slept,
    /// What she intends to do with a day.
    Planned {
        plan: String,
    },
    /// Something she experienced, written by the life engine.
    Lived {
        text: String,
    },
    /// Someone said something to her.
    Heard {
        channel: String,
        from: String,
        text: String,
    },
    /// She said something to someone.
    Said {
        channel: String,
        to: String,
        text: String,
    },
    /// A decision her will made, and why.
    Decided {
        about: String,
        decision: String,
        reason: String,
    },
    /// A reflection from her sleep cycle.
    Reflected {
        text: String,
    },
    /// A hard fact about her life, registered in the canon.
    Canon {
        fact: String,
        source: String,
    },
}

impl Event {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Born { .. } => "born",
            Self::Woke => "woke",
            Self::Slept => "slept",
            Self::Planned { .. } => "planned",
            Self::Lived { .. } => "lived",
            Self::Heard { .. } => "heard",
            Self::Said { .. } => "said",
            Self::Decided { .. } => "decided",
            Self::Reflected { .. } => "reflected",
            Self::Canon { .. } => "canon",
        }
    }
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
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
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
        let at_text = stamp(&at);
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

    pub fn first(&self) -> Result<Option<Entry>> {
        self.one(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events ORDER BY seq ASC LIMIT 1",
        )
    }

    pub fn last(&self) -> Result<Option<Entry>> {
        self.one(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events ORDER BY seq DESC LIMIT 1",
        )
    }

    /// The most recent entry whose kind is one of `kinds`.
    pub fn last_of(&self, kinds: &[&str]) -> Result<Option<Entry>> {
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events WHERE kind IN ({placeholders}) ORDER BY seq DESC LIMIT 1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let entry = stmt
            .query_row(rusqlite::params_from_iter(kinds.iter()), row_to_entry)
            .optional()?;
        Ok(entry)
    }

    /// The last `n` entries whose kind is one of `kinds`, oldest first.
    pub fn last_n_of(&self, kinds: &[&str], n: usize) -> Result<Vec<Entry>> {
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events WHERE kind IN ({placeholders}) ORDER BY seq DESC LIMIT {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(kinds.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Read up to `limit` entries starting at `from_seq` (inclusive).
    pub fn read(&self, from_seq: i64, limit: usize) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events WHERE seq >= ?1 ORDER BY seq ASC LIMIT ?2",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![from_seq, limit], row_to_entry)?;
        rows.map(|r| r.map_err(Error::from)).collect()
    }

    /// The last `n` entries, oldest first.
    pub fn tail(&self, n: usize) -> Result<Vec<Entry>> {
        let total = self.len()?;
        let n = i64::try_from(n).unwrap_or(i64::MAX);
        let from = (total - n + 1).max(1);
        self.read(from, usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// Walk the whole chain and recompute every hash. Returns the length.
    pub fn verify(&self) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, kind, payload, prev_hash, hash FROM events ORDER BY seq ASC",
        )?;
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
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let at: String = row.get(1)?;
    let payload: String = row.get(3)?;
    let at = DateTime::parse_from_rfc3339(&at)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);
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

fn stamp(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
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

    fn fresh() -> Journal {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Journal { conn }
    }

    #[test]
    fn chain_links_and_verifies() {
        let mut j = fresh();
        let a = j.append(Event::Woke).unwrap();
        let b = j
            .append(Event::Lived {
                text: "rain".into(),
            })
            .unwrap();
        assert_eq!(a.seq, 1);
        assert_eq!(a.prev_hash, GENESIS);
        assert_eq!(b.prev_hash, a.hash);
        assert_eq!(j.verify().unwrap(), 2);
        assert_eq!(j.last().unwrap().unwrap(), b);
    }

    #[test]
    fn storage_refuses_rewrites() {
        let mut j = fresh();
        j.append(Event::Woke).unwrap();
        let upd = j.conn.execute("UPDATE events SET kind = 'slept'", []);
        assert!(upd.is_err());
        let del = j.conn.execute("DELETE FROM events", []);
        assert!(del.is_err());
        assert_eq!(j.len().unwrap(), 1);
    }

    #[test]
    fn last_of_filters_by_kind() {
        let mut j = fresh();
        j.append(Event::Woke).unwrap();
        j.append(Event::Lived { text: "x".into() }).unwrap();
        j.append(Event::Slept).unwrap();
        j.append(Event::Lived { text: "y".into() }).unwrap();
        let e = j.last_of(&["woke", "slept"]).unwrap().unwrap();
        assert_eq!(e.event, Event::Slept);
        assert!(j.last_of(&["canon"]).unwrap().is_none());
    }

    #[test]
    fn tail_returns_newest_in_order() {
        let mut j = fresh();
        for i in 0..5 {
            j.append(Event::Lived {
                text: i.to_string(),
            })
            .unwrap();
        }
        let t = j.tail(2).unwrap();
        assert_eq!(t.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(j.tail(100).unwrap().len(), 5);
    }
}
