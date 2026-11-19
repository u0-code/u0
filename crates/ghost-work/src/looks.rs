//! What she called while she looked at a conversation, and what came back.
//!
//! A look at a place can take several rounds: she runs a command on her
//! machine, reads a file, searches, calls a tool of a server she has, and
//! answers once what she needed is in. Her journal keeps what she said and
//! decided, and what she found on the web; the rounds themselves are not a
//! part of her life worth keeping forever, and are kept here instead, in the
//! work database, for whoever is watching her think. Each look is numbered
//! when it keeps its first round, and given the seq of the journal entry it
//! ends with once it does: what she `decided`, or that she was `stopped`.
//! A turn that called nothing keeps a round of no calls, for what it cost;
//! a look that never reached a brain leaves nothing here.
//!
//! This is working state: neither forever, like the journal, nor rebuilt
//! from it, like her memory. A look is kept for `KEPT_DAYS` days, and
//! older ones are let go each time a new look begins. Only her mind writes
//! here; anyone may read.

use std::{io, path::Path};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde_json::Value;

use crate::undertaking::{
    Records,
    db::{self, other},
};

/// How long a look's rounds are kept.
const KEPT_DAYS: i64 = 14;
/// The most looks one read returns: the newest of those asked for.
const READ_MOST: usize = 50;

/// The record of her looks, open for writing.
pub struct Looks {
    conn: Connection,
}

/// One look as it was kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    /// Its number, never another look's, kept or let go.
    pub id: i64,
    /// The seq of the journal entry it ended with, once it has.
    pub seq: Option<i64>,
    /// Its rounds, oldest first, each as its writer gave it.
    pub rounds: Vec<Value>,
}

impl Looks {
    /// The record in the work database at `db`, made if it is not there.
    pub fn open(db: &Path) -> io::Result<Self> {
        Ok(Self {
            conn: db::open(db)?,
        })
    }

    /// Number a new look at `place`, begun `at`, and let go of every look
    /// that began more than `KEPT_DAYS` days before it.
    pub fn begin(&self, place: &str, at: DateTime<Utc>) -> io::Result<i64> {
        let gone = (at - Duration::days(KEPT_DAYS)).timestamp_millis();
        self.conn
            .execute(
                "DELETE FROM look_rounds WHERE look IN (SELECT id FROM looks WHERE at < ?1)",
                [gone],
            )
            .map_err(other)?;
        self.conn
            .execute("DELETE FROM looks WHERE at < ?1", [gone])
            .map_err(other)?;
        self.conn
            .execute(
                "INSERT INTO looks (place, at) VALUES (?1, ?2)",
                params![place, at.timestamp_millis()],
            )
            .map_err(other)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Add one round to the look `look`, after those it has.
    pub fn round(&self, look: i64, round: &Value) -> io::Result<()> {
        self.conn
            .execute(
                "INSERT INTO look_rounds (look, n, round)
                 SELECT ?1, COALESCE(MAX(n), 0) + 1, ?2 FROM look_rounds WHERE look = ?1",
                params![look, round.to_string()],
            )
            .map_err(other)?;
        Ok(())
    }

    /// The look `look` ended with the journal entry `seq`.
    pub fn end(&self, look: i64, seq: i64) -> io::Result<()> {
        self.conn
            .execute(
                "UPDATE looks SET seq = ?2 WHERE id = ?1",
                params![look, seq],
            )
            .map_err(other)?;
        Ok(())
    }
}

/// The looks at `place` in the work database at `db`, as
/// [`Records::looks`] reads them. A database that is not there has none.
pub fn read(db: &Path, place: &str, after: i64, open: Option<i64>) -> io::Result<Vec<Kept>> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    Records::open(db)?.looks(place, after, open)
}

impl Records {
    /// The looks at `place` that ended after the journal entry `after`,
    /// with the look `open` if it is one of them and has not ended: oldest
    /// first, at most the newest `READ_MOST`. Read without writing, on
    /// the connection these records hold: a console asks this once a
    /// second while she thinks.
    pub fn looks(&self, place: &str, after: i64, open: Option<i64>) -> io::Result<Vec<Kept>> {
        let conn = &self.conn;
        let most = i64::try_from(READ_MOST).unwrap_or(i64::MAX);
        let mut looks: Vec<(i64, Option<i64>)> = conn
            .prepare_cached(
                "SELECT id, seq FROM looks
                 WHERE place = ?1 AND (seq > ?2 OR (seq IS NULL AND id = ?3))
                 ORDER BY id DESC LIMIT ?4",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![place, after, open, most], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect()
            })
            .map_err(other)?;
        looks.reverse();
        let mut rounds = conn
            .prepare_cached("SELECT round FROM look_rounds WHERE look = ?1 ORDER BY n")
            .map_err(other)?;
        looks
            .into_iter()
            .map(|(id, seq)| {
                let rounds = rounds
                    .query_map([id], |row| row.get::<_, String>(0))
                    .and_then(Iterator::collect::<rusqlite::Result<Vec<String>>>)
                    .map_err(other)?
                    .iter()
                    .filter_map(|text| serde_json::from_str(text).ok())
                    .collect();
                Ok(Kept { id, seq, rounds })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 11, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn a_look_reads_back_with_its_rounds_in_order_and_the_entry_it_ended_with() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(crate::WORK_DB);
        let looks = Looks::open(&db).unwrap();
        let first = looks.begin("t:wen", at(10, 9)).unwrap();
        looks.round(first, &json!({"n": "one"})).unwrap();
        looks.round(first, &json!({"n": "two"})).unwrap();
        looks.end(first, 40).unwrap();
        let elsewhere = looks.begin("t:mei", at(10, 9)).unwrap();
        looks.round(elsewhere, &json!({"n": "mei"})).unwrap();
        let running = looks.begin("t:wen", at(10, 10)).unwrap();
        looks.round(running, &json!({"n": "now"})).unwrap();

        let read = read(&db, "t:wen", 0, Some(running)).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].seq, Some(40));
        assert_eq!(read[0].rounds, [json!({"n": "one"}), json!({"n": "two"})]);
        assert_eq!((read[1].id, read[1].seq), (running, None));
        // After the entry it ended with, only the one still going is left.
        let later = super::read(&db, "t:wen", 40, Some(running)).unwrap();
        assert_eq!(later.iter().map(|k| k.id).collect::<Vec<_>>(), [running]);
        // One that did not end, and is not the one going on, is not shown.
        assert!(super::read(&db, "t:wen", 40, None).unwrap().is_empty());
    }

    #[test]
    fn looks_older_than_a_fortnight_are_let_go_when_a_new_one_begins_and_numbers_are_not_reused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(crate::WORK_DB);
        let looks = Looks::open(&db).unwrap();
        let old = looks.begin("t:wen", at(1, 9)).unwrap();
        looks.round(old, &json!({})).unwrap();
        looks.end(old, 3).unwrap();
        let recent = looks.begin("t:wen", at(10, 9)).unwrap();
        looks.end(recent, 9).unwrap();
        assert_eq!(read(&db, "t:wen", 0, None).unwrap().len(), 2);

        let new = looks.begin("t:wen", at(20, 9)).unwrap();
        looks.end(new, 12).unwrap();
        let ids: Vec<i64> = read(&db, "t:wen", 0, None)
            .unwrap()
            .iter()
            .map(|k| k.id)
            .collect();
        assert_eq!(ids, [recent, new]);
        assert!(new > recent && recent > old);
        let rounds: i64 = looks
            .conn
            .query_row("SELECT COUNT(*) FROM look_rounds", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rounds, 0, "the rounds went with their look");
    }

    #[test]
    fn a_database_that_never_kept_a_look_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(crate::WORK_DB);
        assert!(read(&db, "t:wen", 0, None).unwrap().is_empty());
        crate::Undertaking::open(&db, "w1", Some("brief")).unwrap();
        assert!(read(&db, "t:wen", 0, None).unwrap().is_empty());
    }
}
