//! The work she has taken on, as she remembers it.
//!
//! An undertaking is a piece of work that takes more than one thought: begun,
//! sat down to again and again, paused to ask whoever it is for, told about
//! as it goes, and ended one way or another. What is kept here is her account of it, not the work
//! itself, which lives in its own directory. It is her memory, found from any
//! platform, and it says where it was asked for.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, opt_time_at, time_at};
use crate::{
    Result,
    journal::{Entry, Event, stamp},
};

/// Something she has taken on.
#[derive(Debug, Clone, Serialize)]
pub struct Undertaking {
    pub id: String,
    /// The seq of the entry that began it.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub brief: String,
    pub asked_by: Option<String>,
    pub place: Option<String>,
    /// `None` while it is still open.
    pub outcome: Option<String>,
    /// The latest account of it: the last sitting's, or how it ended.
    pub summary: Option<String>,
    pub sittings: i64,
    pub last_at: DateTime<Utc>,
    /// What she asked whoever it is for, and is waiting to hear.
    pub waiting: Option<String>,
    /// When she asked.
    pub asked_at: Option<DateTime<Utc>>,
    /// An operator directed it.
    pub directed: bool,
}

/// Something a piece of work told her while it was under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkReport {
    /// The seq of the `Reported` entry.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub text: String,
    pub files: Vec<String>,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS undertakings (
    id TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL, brief TEXT NOT NULL,
    asked_by TEXT, place TEXT, outcome TEXT, summary TEXT,
    sittings INTEGER NOT NULL DEFAULT 0, last_at TEXT NOT NULL,
    waiting TEXT, asked_at TEXT, directed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS reports (
    seq INTEGER PRIMARY KEY, id TEXT NOT NULL, at TEXT NOT NULL, text TEXT NOT NULL,
    files TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reports_id ON reports (id, seq);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        Event::Undertook {
            id,
            brief,
            asked_by,
            place,
            directed,
        } => {
            tx.execute(
                "INSERT OR IGNORE INTO undertakings (id, seq, at, brief, asked_by, place, last_at, directed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3, ?7)",
                params![id, entry.seq, at, brief, asked_by, place, directed],
            )?;
        }
        Event::Worked { id, summary, .. } => {
            tx.execute(
                "UPDATE undertakings SET sittings = sittings + 1, summary = ?2, last_at = ?3 WHERE id = ?1",
                params![id, summary, at],
            )?;
        }
        Event::Asked { id, question } => {
            tx.execute(
                "UPDATE undertakings SET waiting = ?2, asked_at = ?3, last_at = ?3 WHERE id = ?1",
                params![id, question, at],
            )?;
        }
        Event::Added { id, .. } => {
            // Whatever she was waiting to hear, this is it.
            tx.execute(
                "UPDATE undertakings SET waiting = NULL, asked_at = NULL, last_at = ?2 WHERE id = ?1",
                params![id, at],
            )?;
        }
        Event::Reported { id, text, files } => {
            let files = serde_json::to_string(files)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            tx.execute(
                "INSERT INTO reports (seq, id, at, text, files) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.seq, id, at, text, files],
            )?;
        }
        Event::Concluded {
            id,
            outcome,
            summary,
        } => {
            tx.execute(
                "UPDATE undertakings SET outcome = ?2, summary = ?3, last_at = ?4,
                 waiting = NULL, asked_at = NULL WHERE id = ?1",
                params![id, outcome, summary, at],
            )?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// What she has taken on, most recently touched first. `open` leaves out
    /// what has ended.
    pub fn undertakings(&self, open: bool, limit: usize) -> Result<Vec<Undertaking>> {
        let only = if open { "WHERE outcome IS NULL" } else { "" };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {UNDERTAKING_COLUMNS} FROM undertakings {only} ORDER BY last_at DESC LIMIT ?1"
        ))?;
        Ok(stmt
            .query_map([as_i64(limit)], row_to_undertaking)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn undertaking(&self, id: &str) -> Result<Option<Undertaking>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {UNDERTAKING_COLUMNS} FROM undertakings WHERE id = ?1"),
                [id],
                row_to_undertaking,
            )
            .optional()?)
    }

    /// The undertaking whose `Undertook` is entry `seq`: the number she
    /// knows it by.
    pub fn undertaking_numbered(&self, seq: i64) -> Result<Option<Undertaking>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {UNDERTAKING_COLUMNS} FROM undertakings WHERE seq = ?1"),
                [seq],
                row_to_undertaking,
            )
            .optional()?)
    }

    /// The newest `limit` things undertaking `id` told her while it was
    /// under way, oldest first.
    pub fn reports(&self, id: &str, limit: usize) -> Result<Vec<WorkReport>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, text, files FROM reports WHERE id = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut told = stmt
            .query_map(params![id, as_i64(limit)], |r| {
                let files: String = r.get(3)?;
                Ok(WorkReport {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    text: r.get(2)?,
                    files: serde_json::from_str(&files).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        told.reverse();
        Ok(told)
    }

    /// How many things undertaking `id` has told her, all told.
    pub fn reported(&self, id: &str) -> Result<usize> {
        let n: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM reports WHERE id = ?1", [id], |r| {
                    r.get(0)
                })?;
        Ok(usize::try_from(n).unwrap_or_default())
    }

    /// How many undertakings were begun at or after `since`.
    pub fn undertaken_since(&self, since: DateTime<Utc>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM undertakings WHERE at >= ?1",
            [stamp(&since)],
            |r| r.get(0),
        )?)
    }
}

const UNDERTAKING_COLUMNS: &str = "id, seq, at, brief, asked_by, place, outcome, summary, sittings, last_at, waiting, asked_at, directed";

fn row_to_undertaking(r: &rusqlite::Row<'_>) -> rusqlite::Result<Undertaking> {
    Ok(Undertaking {
        id: r.get(0)?,
        seq: r.get(1)?,
        at: time_at(r, 2)?,
        brief: r.get(3)?,
        asked_by: r.get(4)?,
        place: r.get(5)?,
        outcome: r.get(6)?,
        summary: r.get(7)?,
        sittings: r.get(8)?,
        last_at: time_at(r, 9)?,
        waiting: r.get(10)?,
        asked_at: opt_time_at(r, 11)?,
        directed: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::fold;
    use crate::journal::Event;

    #[test]
    fn an_undertaking_is_followed_from_start_to_end() {
        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "find out when the old tea house closes".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
                directed: false,
            },
            Event::Worked {
                id: "u1".into(),
                summary: "two sources so far".into(),
                steps: 9,
            },
        ]);
        let open = m.undertakings(true, 5).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(
            (open[0].sittings, open[0].summary.as_deref()),
            (1, Some("two sources so far"))
        );

        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "b".into(),
                asked_by: None,
                place: None,
                directed: false,
            },
            Event::Concluded {
                id: "u1".into(),
                outcome: "finished".into(),
                summary: "it closes at nine".into(),
            },
        ]);
        assert!(m.undertakings(true, 5).unwrap().is_empty());
        let done = m.undertaking("u1").unwrap().unwrap();
        assert_eq!(done.outcome.as_deref(), Some("finished"));
        assert_eq!(m.search("t", "closes at nine", 5).unwrap().len(), 1);
    }

    #[test]
    fn what_work_told_her_is_remembered_newest_last_and_found_again() {
        let report = |text: &str, files: &[&str]| Event::Reported {
            id: "u1".into(),
            text: text.into(),
            files: files.iter().map(|f| (*f).to_owned()).collect(),
        };
        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "find the hours".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
                directed: false,
            },
            report("the park page says nine", &[]),
            report("the city listing agrees", &[]),
            report("written up", &["/w/hours.md"]),
        ]);
        let told = m.reports("u1", 2).unwrap();
        assert_eq!(
            told.iter()
                .map(|r| (r.text.as_str(), r.files.clone()))
                .collect::<Vec<_>>(),
            [
                ("the city listing agrees", vec![]),
                ("written up", vec!["/w/hours.md".to_owned()]),
            ]
        );
        assert_eq!(m.reported("u1").unwrap(), 3);
        assert_eq!(m.reported("u2").unwrap(), 0);
        assert_eq!(m.undertaking_numbered(1).unwrap().unwrap().id, "u1");
        assert!(m.undertaking_numbered(2).unwrap().is_none());
        assert_eq!(m.search("t", "park page", 5).unwrap().len(), 1);
    }

    #[test]
    fn work_is_remembered_whole_and_says_where_it_was_asked_for() {
        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "find the hours".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
                directed: false,
            },
            Event::Undertook {
                id: "u2".into(),
                brief: "draw the gate".into(),
                asked_by: None,
                place: None,
                directed: false,
            },
            Event::Concluded {
                id: "u1".into(),
                outcome: "finished".into(),
                summary: "nine on weekdays".into(),
            },
        ]);
        assert_eq!(
            m.undertaking("u1").unwrap().unwrap().outcome.as_deref(),
            Some("finished")
        );
        assert!(m.undertaking("u2").unwrap().unwrap().outcome.is_none());
        assert_eq!(m.undertakings(false, 10).unwrap().len(), 2);
        assert_eq!(
            m.undertaking("u1").unwrap().unwrap().place.as_deref(),
            Some("t:a")
        );
        // Found from another platform too: what she remembers is not split.
        assert_eq!(m.search("qq", "nine on weekdays", 5).unwrap().len(), 1);
    }
}
