//! What she can recall, derived from the journal.
//!
//! Everything in here is a view. The file can be deleted at any time and the
//! journal will rebuild it: places and what is unread in them, the people she
//! knows, notes, intentions, the canon of her own life, and a full-text index
//! for remembering things that are no longer in front of her.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::Serialize;

use crate::{
    Result,
    journal::{Entry, Event, Journal, parse_stamp, stamp},
    social::{self, PlaceKind},
};

/// Where the derived memory lives inside a ghost directory.
pub const MEMORY_FILE: &str = "derived/memory.db";

/// Bump when the schema changes. A mismatch rebuilds from the journal.
const SCHEMA_VERSION: i64 = 1;

const WARMTH_MIN: i64 = -20;
const WARMTH_MAX: i64 = 20;

/// A place and how far she has read in it.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceState {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub kind: PlaceKind,
    pub name: Option<String>,
    pub read_upto: i64,
    pub last_in_seq: i64,
    pub last_in_at: Option<DateTime<Utc>>,
    pub last_out_seq: i64,
    pub last_out_at: Option<DateTime<Utc>>,
    pub unread: i64,
    pub unread_addressed: i64,
}

/// One message in a place, hers or someone else's.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub incoming: bool,
    pub person: Option<String>,
    pub name: String,
    pub platform_id: Option<String>,
    pub reply_to: Option<String>,
    pub addressed: bool,
    pub text: String,
}

/// Someone she has heard from.
#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub automated: bool,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    /// Messages she has heard from them, anywhere.
    pub heard: i64,
    /// Sum of every `Felt` toward them, clamped.
    pub warmth: i64,
    /// She has left them.
    pub gone: bool,
    /// Key of the private place she shares with them, once one exists.
    pub direct: Option<String>,
    /// Distinct days on which she heard from them.
    pub active_days: i64,
    /// Distinct days on which she wrote to them privately without an answer since.
    pub unanswered_days: i64,
    /// When she last wrote to them privately.
    pub last_out_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub fact: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Intention {
    pub seq: i64,
    pub due: DateTime<Utc>,
    pub what: String,
    pub about: Option<String>,
    pub place: Option<String>,
}

/// Something found by searching her memory.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub seq: i64,
    pub at: Option<DateTime<Utc>>,
    pub kind: String,
    pub text: String,
}

pub struct Memory {
    conn: Connection,
}

impl Memory {
    /// Open the derived store under `dir`, creating or rebuilding its schema.
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join(MEMORY_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let conn = if version == SCHEMA_VERSION || version == 0 {
            conn
        } else {
            // An older shape. It is only a view: throw it away.
            drop(conn);
            std::fs::remove_file(&path)?;
            Connection::open(&path)?
        };
        Self::prepare(conn)
    }

    /// A store that lives only as long as the value. For tests and tools.
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// The seq of the last journal entry folded in.
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'last_seq'", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(0))
    }

    /// Fold in every journal entry not yet seen. Returns how many.
    pub fn catch_up(&mut self, journal: &Journal) -> Result<u64> {
        let mut applied = 0;
        loop {
            let batch = journal.read(self.last_seq()? + 1, 500)?;
            if batch.is_empty() {
                return Ok(applied);
            }
            for entry in &batch {
                self.apply(entry)?;
                applied += 1;
            }
        }
    }

    /// Fold one journal entry into the views.
    pub fn apply(&mut self, entry: &Entry) -> Result<()> {
        let tx = self.conn.transaction()?;
        let at = stamp(&entry.at);
        let seq = entry.seq;
        match &entry.event {
            Event::Heard {
                channel,
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
            } => {
                let place_key = social::key(channel, &place.id);
                let person_key = social::key(channel, &from.id);
                tx.execute(
                    "INSERT INTO places (key, channel, id, kind, name, last_in_seq, last_in_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(key) DO UPDATE SET kind = ?4, name = COALESCE(?5, name),
                         last_in_seq = ?6, last_in_at = ?7",
                    params![
                        place_key,
                        channel,
                        place.id,
                        place.kind.as_str(),
                        place.name,
                        seq,
                        at
                    ],
                )?;
                let direct = (place.kind == PlaceKind::Direct).then_some(&place_key);
                tx.execute(
                    "INSERT INTO people (key, channel, id, name, handle, automated, first_at, last_at, heard, direct)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, ?8)
                     ON CONFLICT(key) DO UPDATE SET name = ?4, handle = COALESCE(?5, handle),
                         automated = ?6, last_at = ?7, heard = heard + 1,
                         direct = COALESCE(?8, direct)",
                    params![person_key, channel, from.id, from.name, from.handle, from.automated, at, direct],
                )?;
                tx.execute(
                    "INSERT INTO messages (seq, at, place, dir, person, name, platform_id, reply_to, addressed, text)
                     VALUES (?1, ?2, ?3, 'in', ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![seq, at, place_key, person_key, from.name, id, reply_to, addressed, text],
                )?;
                remember(&tx, seq, &at, "heard", &format!("{}: {text}", from.name))?;
            }
            Event::Said {
                channel,
                place,
                id,
                text,
                reply_to,
            } => {
                let place_key = social::key(channel, place);
                tx.execute(
                    "INSERT INTO places (key, channel, id, kind, last_out_seq, last_out_at)
                     VALUES (?1, ?2, ?3, 'public', ?4, ?5)
                     ON CONFLICT(key) DO UPDATE SET last_out_seq = ?4, last_out_at = ?5",
                    params![place_key, channel, place, seq, at],
                )?;
                tx.execute(
                    "INSERT INTO messages (seq, at, place, dir, name, platform_id, reply_to, text)
                     VALUES (?1, ?2, ?3, 'out', 'you', ?4, ?5, ?6)",
                    params![seq, at, place_key, id, reply_to, text],
                )?;
                remember(&tx, seq, &at, "said", &format!("you: {text}"))?;
            }
            Event::Read {
                channel,
                place,
                upto,
            } => {
                tx.execute(
                    "UPDATE places SET read_upto = MAX(read_upto, ?2) WHERE key = ?1",
                    params![social::key(channel, place), upto],
                )?;
            }
            Event::Felt { toward, delta, .. } => {
                tx.execute(
                    "UPDATE people SET warmth = MAX(?3, MIN(?4, warmth + ?2)) WHERE key = ?1",
                    params![toward, i64::from(*delta), WARMTH_MIN, WARMTH_MAX],
                )?;
            }
            Event::Noted { about, text } => {
                tx.execute(
                    "INSERT INTO notes (seq, at, person, text) VALUES (?1, ?2, ?3, ?4)",
                    params![seq, at, about, text],
                )?;
                remember(&tx, seq, &at, "noted", text)?;
            }
            Event::Left { person, .. } => {
                tx.execute("UPDATE people SET gone = 1 WHERE key = ?1", [person])?;
            }
            Event::Intended {
                due,
                what,
                about,
                place,
            } => {
                tx.execute(
                    "INSERT INTO intentions (seq, due, what, about, place) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![seq, stamp(due), what, about, place],
                )?;
            }
            Event::Settled { intention, .. } => {
                tx.execute(
                    "UPDATE intentions SET settled = 1 WHERE seq = ?1",
                    [intention],
                )?;
            }
            Event::Canon { fact, source } => {
                tx.execute(
                    "INSERT INTO canon (seq, at, fact, source) VALUES (?1, ?2, ?3, ?4)",
                    params![seq, at, fact, source],
                )?;
                remember(&tx, seq, &at, "canon", fact)?;
            }
            Event::Lived { text, .. } => remember(&tx, seq, &at, "lived", text)?,
            Event::Reflected { text } => remember(&tx, seq, &at, "reflected", text)?,
            Event::Posted { text, .. } => remember(&tx, seq, &at, "posted", text)?,
            Event::Saw { text, .. } => remember(&tx, seq, &at, "saw", text)?,
            Event::Born { .. }
            | Event::Dormant { .. }
            | Event::Woke
            | Event::Slept
            | Event::Planned { .. }
            | Event::Reacted { .. }
            | Event::Decided { .. } => {}
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('last_seq', ?1)
             ON CONFLICT(key) DO UPDATE SET value = ?1",
            [seq],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn place(&self, key: &str) -> Result<Option<PlaceState>> {
        Ok(self
            .conn
            .query_row(
                &format!("{PLACE_SELECT} WHERE p.key = ?1"),
                [key],
                row_to_place,
            )
            .optional()?)
    }

    /// Every place she has been in, most recently active first.
    pub fn places(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self.conn.prepare(&format!(
            "{PLACE_SELECT} ORDER BY MAX(p.last_in_seq, p.last_out_seq) DESC"
        ))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every place with something she has not looked at.
    pub fn places_unread(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{PLACE_SELECT} WHERE p.last_in_seq > p.read_upto"))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|p| p.unread > 0)
            .collect())
    }

    /// The last `limit` messages in a place, oldest first.
    pub fn thread(&self, place: &str, limit: usize) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 ORDER BY seq DESC LIMIT ?2"
        ))?;
        let mut lines = stmt
            .query_map(params![place, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    pub fn line(&self, seq: i64) -> Result<Option<(String, Line)>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT place, {LINE_COLUMNS} FROM messages WHERE seq = ?1"),
                [seq],
                |r| Ok((r.get(0)?, line_at(r, 1)?)),
            )
            .optional()?)
    }

    /// Whether a message with this channel id has already reached her here.
    pub fn knows_message(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether the message with this channel id in this place is one she sent.
    pub fn is_hers(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 AND dir = 'out' LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether she has already replied to the message with this channel id.
    pub fn replied_to(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND reply_to = ?2 AND dir = 'out' LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// How many messages she has sent in a place since `since`.
    pub fn said_since(&self, place: &str, since: DateTime<Utc>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE place = ?1 AND dir = 'out' AND at >= ?2",
            params![place, stamp(&since)],
            |r| r.get(0),
        )?)
    }

    pub fn person(&self, key: &str) -> Result<Option<Person>> {
        let base = self
            .conn
            .query_row(
                "SELECT key, channel, id, name, handle, automated, first_at, last_at, heard, warmth, gone, direct
                 FROM people WHERE key = ?1",
                [key],
                |r| {
                    Ok(Person {
                        key: r.get(0)?,
                        channel: r.get(1)?,
                        id: r.get(2)?,
                        name: r.get(3)?,
                        handle: r.get(4)?,
                        automated: r.get(5)?,
                        first_at: time_at(r, 6)?,
                        last_at: time_at(r, 7)?,
                        heard: r.get(8)?,
                        warmth: r.get(9)?,
                        gone: r.get(10)?,
                        direct: r.get(11)?,
                        active_days: 0,
                        unanswered_days: 0,
                        last_out_at: None,
                    })
                },
            )
            .optional()?;
        let Some(mut person) = base else {
            return Ok(None);
        };
        person.active_days = self.conn.query_row(
            "SELECT COUNT(DISTINCT substr(at, 1, 10)) FROM messages WHERE person = ?1",
            [key],
            |r| r.get(0),
        )?;
        if let Some(direct) = &person.direct {
            let (days, last): (i64, Option<String>) = self.conn.query_row(
                "SELECT COUNT(DISTINCT substr(m.at, 1, 10)), MAX(m.at) FROM messages m
                 JOIN places p ON p.key = m.place
                 WHERE m.place = ?1 AND m.dir = 'out' AND m.seq > p.last_in_seq",
                [direct],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            person.unanswered_days = days;
            person.last_out_at = match last {
                Some(at) => parse_stamp(&at).ok(),
                None => self
                    .conn
                    .query_row(
                        "SELECT last_out_at FROM places WHERE key = ?1",
                        [direct],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
                    .and_then(|at| parse_stamp(&at).ok()),
            };
        }
        Ok(Some(person))
    }

    /// The person on the other side of a private place.
    pub fn person_of_direct(&self, place: &str) -> Result<Option<Person>> {
        let key: Option<String> = self
            .conn
            .query_row(
                "SELECT key FROM people WHERE direct = ?1 LIMIT 1",
                [place],
                |r| r.get(0),
            )
            .optional()?;
        match key {
            Some(key) => self.person(&key),
            None => Ok(None),
        }
    }

    /// The people most recently heard in a place, most recent first.
    pub fn people_in(&self, place: &str, limit: usize) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(
            "SELECT person FROM messages WHERE place = ?1 AND dir = 'in' AND person IS NOT NULL
             GROUP BY person ORDER BY MAX(seq) DESC LIMIT ?2",
        )?;
        let keys = stmt
            .query_map(params![place, as_i64(limit)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    /// Everyone she heard from since `since`, most recent first.
    pub fn people_since(&self, since: DateTime<Utc>) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(
            "SELECT person FROM messages WHERE dir = 'in' AND person IS NOT NULL AND at >= ?1
             GROUP BY person ORDER BY MAX(seq) DESC",
        )?;
        let keys = stmt
            .query_map([stamp(&since)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    /// Everyone she knows, most recently heard first.
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM people ORDER BY last_at DESC")?;
        let keys = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    fn load_people(&self, keys: &[String]) -> Result<Vec<Person>> {
        let mut people = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(p) = self.person(key)? {
                people.push(p);
            }
        }
        Ok(people)
    }

    /// The newest `limit` notes about a person, oldest first.
    pub fn notes(&self, person: &str, limit: usize) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, text FROM notes WHERE person = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut notes = stmt
            .query_map(params![person, as_i64(limit)], |r| {
                Ok(Note {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    text: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        notes.reverse();
        Ok(notes)
    }

    /// The newest `limit` facts about her own life, oldest first.
    pub fn canon(&self, limit: usize) -> Result<Vec<Fact>> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, at, fact, source FROM canon ORDER BY seq DESC LIMIT ?1")?;
        let mut facts = stmt
            .query_map([as_i64(limit)], |r| {
                Ok(Fact {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    fact: r.get(2)?,
                    source: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        facts.reverse();
        Ok(facts)
    }

    /// Intentions not yet settled, soonest first.
    pub fn open_intentions(&self) -> Result<Vec<Intention>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, due, what, about, place FROM intentions WHERE settled = 0 ORDER BY due ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Intention {
                seq: r.get(0)?,
                due: time_at(r, 1)?,
                what: r.get(2)?,
                about: r.get(3)?,
                place: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Search everything she has heard, said, lived and noted.
    ///
    /// Terms of three or more characters go through the trigram index; shorter
    /// ones (common in Chinese) fall back to a substring scan.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let limit = as_i64(limit);
        let (long, short): (Vec<&str>, Vec<&str>) =
            terms.iter().partition(|t| t.chars().count() >= 3);
        let mut hits: Vec<Hit> = Vec::new();
        if !long.is_empty() {
            let phrases = long
                .iter()
                .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" OR ");
            let mut stmt = self.conn.prepare(
                "SELECT seq, at, kind, text FROM recall WHERE recall MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            hits = stmt
                .query_map(params![phrases, limit], row_to_hit)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        if !short.is_empty() {
            let clause = short
                .iter()
                .map(|_| "text LIKE ? ESCAPE '\\'")
                .collect::<Vec<_>>()
                .join(" OR ");
            let patterns = short.iter().map(|t| {
                let escaped = t
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                format!("%{escaped}%")
            });
            let mut stmt = self.conn.prepare(&format!(
                "SELECT seq, at, kind, text FROM recall WHERE {clause} ORDER BY rowid DESC LIMIT {limit}"
            ))?;
            let scanned = stmt
                .query_map(rusqlite::params_from_iter(patterns), row_to_hit)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for hit in scanned {
                if !hits.iter().any(|h| h.seq == hit.seq) {
                    hits.push(hit);
                }
            }
        }
        hits.sort_by_key(|h| h.seq);
        Ok(hits)
    }
}

fn remember(
    tx: &rusqlite::Transaction<'_>,
    seq: i64,
    at: &str,
    kind: &str,
    text: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO recall (text, kind, seq, at) VALUES (?1, ?2, ?3, ?4)",
        params![text, kind, seq, at],
    )?;
    Ok(())
}

fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn time_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    let text: String = row.get(idx)?;
    parse_stamp(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn opt_time_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let text: Option<String> = row.get(idx)?;
    Ok(text.and_then(|t| parse_stamp(&t).ok()))
}

const PLACE_SELECT: &str = "
SELECT p.key, p.channel, p.id, p.kind, p.name, p.read_upto,
       p.last_in_seq, p.last_in_at, p.last_out_seq, p.last_out_at,
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto),
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto AND m.addressed = 1)
FROM places p";

fn row_to_place(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlaceState> {
    let kind: String = r.get(3)?;
    Ok(PlaceState {
        key: r.get(0)?,
        channel: r.get(1)?,
        id: r.get(2)?,
        kind: PlaceKind::parse(&kind),
        name: r.get(4)?,
        read_upto: r.get(5)?,
        last_in_seq: r.get(6)?,
        last_in_at: opt_time_at(r, 7)?,
        last_out_seq: r.get(8)?,
        last_out_at: opt_time_at(r, 9)?,
        unread: r.get(10)?,
        unread_addressed: r.get(11)?,
    })
}

const LINE_COLUMNS: &str = "seq, at, dir, person, name, platform_id, reply_to, addressed, text";
const LINE_SELECT: &str =
    "SELECT seq, at, dir, person, name, platform_id, reply_to, addressed, text FROM messages";

fn row_to_line(r: &rusqlite::Row<'_>) -> rusqlite::Result<Line> {
    line_at(r, 0)
}

fn line_at(r: &rusqlite::Row<'_>, o: usize) -> rusqlite::Result<Line> {
    let dir: String = r.get(o + 2)?;
    Ok(Line {
        seq: r.get(o)?,
        at: time_at(r, o + 1)?,
        incoming: dir == "in",
        person: r.get(o + 3)?,
        name: r.get(o + 4)?,
        platform_id: r.get(o + 5)?,
        reply_to: r.get(o + 6)?,
        addressed: r.get(o + 7)?,
        text: r.get(o + 8)?,
    })
}

fn row_to_hit(r: &rusqlite::Row<'_>) -> rusqlite::Result<Hit> {
    // FTS5 does not promise to keep column types, so accept either.
    let seq = match r.get::<_, Value>(0)? {
        Value::Integer(i) => i,
        Value::Text(t) => t.parse().unwrap_or(0),
        _ => 0,
    };
    let at: Option<String> = r.get(1)?;
    Ok(Hit {
        seq,
        at: at.and_then(|t| parse_stamp(&t).ok()),
        kind: r.get(2)?,
        text: r.get(3)?,
    })
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS places (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    kind TEXT NOT NULL, name TEXT,
    read_upto INTEGER NOT NULL DEFAULT 0,
    last_in_seq INTEGER NOT NULL DEFAULT 0, last_in_at TEXT,
    last_out_seq INTEGER NOT NULL DEFAULT 0, last_out_at TEXT
);
CREATE TABLE IF NOT EXISTS people (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    name TEXT NOT NULL, handle TEXT, automated INTEGER NOT NULL DEFAULT 0,
    first_at TEXT NOT NULL, last_at TEXT NOT NULL,
    heard INTEGER NOT NULL DEFAULT 0, warmth INTEGER NOT NULL DEFAULT 0,
    gone INTEGER NOT NULL DEFAULT 0, direct TEXT
);
CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, place TEXT NOT NULL, dir TEXT NOT NULL,
    person TEXT, name TEXT NOT NULL, platform_id TEXT, reply_to TEXT,
    addressed INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_place ON messages (place, seq);
CREATE INDEX IF NOT EXISTS messages_person ON messages (person, seq);
CREATE INDEX IF NOT EXISTS messages_platform ON messages (place, platform_id);
CREATE TABLE IF NOT EXISTS notes (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, person TEXT NOT NULL, text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS notes_person ON notes (person, seq);
CREATE TABLE IF NOT EXISTS intentions (
    seq INTEGER PRIMARY KEY, due TEXT NOT NULL, what TEXT NOT NULL,
    about TEXT, place TEXT, settled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS canon (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, fact TEXT NOT NULL, source TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS recall USING fts5(
    text, kind UNINDEXED, seq UNINDEXED, at UNINDEXED, tokenize = 'trigram'
);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::{Actor, Place};

    fn heard(
        place: &str,
        kind: PlaceKind,
        from: &str,
        id: &str,
        text: &str,
        addressed: bool,
    ) -> Event {
        Event::Heard {
            channel: "t".into(),
            place: Place {
                id: place.into(),
                kind,
                name: None,
            },
            from: Actor {
                id: from.into(),
                name: from.to_uppercase(),
                handle: None,
                automated: false,
            },
            id: id.into(),
            text: text.into(),
            reply_to: None,
            addressed,
        }
    }

    fn said(place: &str, text: &str) -> Event {
        Event::Said {
            channel: "t".into(),
            place: place.into(),
            id: None,
            text: text.into(),
            reply_to: None,
        }
    }

    fn fold(events: Vec<Event>) -> (Journal, Memory) {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        for e in events {
            let entry = j.append(e).unwrap();
            m.apply(&entry).unwrap();
        }
        (j, m)
    }

    #[test]
    fn unread_follows_read_markers() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("a", PlaceKind::Direct, "a", "2", "there?", true),
            Event::Read {
                channel: "t".into(),
                place: "a".into(),
                upto: 2,
            },
            said("a", "yes"),
            heard("a", PlaceKind::Direct, "a", "3", "good", true),
        ]);
        let p = m.place("t:a").unwrap().unwrap();
        assert_eq!((p.unread, p.read_upto, p.last_out_seq), (1, 2, 4));
        assert_eq!(m.places_unread().unwrap().len(), 1);
        assert!(m.knows_message("t:a", "2").unwrap());
        assert!(!m.knows_message("t:a", "9").unwrap());
        let thread = m.thread("t:a", 10).unwrap();
        assert_eq!(thread.len(), 4);
        assert!(!thread[2].incoming);
    }

    #[test]
    fn people_accumulate() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("g", PlaceKind::Group, "a", "2", "hello group", false),
            heard("g", PlaceKind::Group, "b", "3", "hey", false),
            Event::Felt {
                toward: "t:a".into(),
                delta: 2,
                why: "kind".into(),
            },
            Event::Noted {
                about: "t:a".into(),
                text: "designs buildings".into(),
            },
            said("a", "hello"),
        ]);
        let a = m.person("t:a").unwrap().unwrap();
        assert_eq!((a.heard, a.warmth, a.active_days), (2, 2, 1));
        assert_eq!(a.direct.as_deref(), Some("t:a"));
        assert_eq!(a.unanswered_days, 1);
        assert_eq!(m.person_of_direct("t:a").unwrap().unwrap().key, "t:a");
        assert_eq!(m.people_in("t:g", 5).unwrap().len(), 2);
        assert_eq!(m.notes("t:a", 5).unwrap()[0].text, "designs buildings");
        let p = m.place("t:g").unwrap().unwrap();
        assert_eq!((p.unread, p.unread_addressed), (2, 0));
    }

    #[test]
    fn intentions_open_and_settle() {
        let due = Utc::now();
        let (_, m) = fold(vec![
            Event::Intended {
                due,
                what: "ask about the interview".into(),
                about: Some("t:a".into()),
                place: None,
            },
            Event::Intended {
                due,
                what: "second".into(),
                about: None,
                place: None,
            },
            Event::Settled {
                intention: 1,
                outcome: "asked".into(),
            },
        ]);
        let open = m.open_intentions().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].what, "second");
    }

    #[test]
    fn search_finds_latin_and_short_chinese() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "my interview is on friday",
                true,
            ),
            said("a", "东边那家茶馆要拆了"),
            Event::Canon {
                fact: "她养了一只叫豆包的猫".into(),
                source: "life".into(),
            },
        ]);
        assert_eq!(m.search("interview", 5).unwrap().len(), 1);
        // Two characters: below the trigram minimum, found by scan.
        let tea = m.search("茶馆", 5).unwrap();
        assert_eq!(tea.len(), 1);
        assert_eq!(tea[0].kind, "said");
        assert_eq!(m.search("豆包 interview", 5).unwrap().len(), 2);
        assert!(m.search("nothing here", 5).unwrap().is_empty());
        assert!(m.search("100%_", 5).unwrap().is_empty());
    }

    #[test]
    fn catch_up_rebuilds_from_the_journal() {
        let (j, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            said("a", "hey"),
        ]);
        let mut fresh = Memory::in_memory().unwrap();
        assert_eq!(fresh.catch_up(&j).unwrap(), 2);
        assert_eq!(fresh.last_seq().unwrap(), m.last_seq().unwrap());
        assert_eq!(fresh.thread("t:a", 10).unwrap().len(), 2);
        assert_eq!(fresh.catch_up(&j).unwrap(), 0);
    }
}
