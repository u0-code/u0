//! What she means to do, and has not done yet.
//!
//! An intention is something she said to herself she would do by a certain
//! time: ask after someone, come back with an answer she owes. It stays open
//! until she settles it, however that turned out.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event, stamp},
};

#[derive(Debug, Clone, Serialize)]
pub struct Intention {
    pub seq: i64,
    pub due: DateTime<Utc>,
    pub what: String,
    pub about: Option<String>,
    pub place: Option<String>,
    /// She said she would come back with this.
    pub owed: bool,
    /// It comes of work an operator directed, and does not wait on her day.
    pub directed: bool,
    /// It is her daily report to an operator, `about`.
    pub report: bool,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS intentions (
    seq INTEGER PRIMARY KEY, due TEXT NOT NULL, what TEXT NOT NULL,
    about TEXT, place TEXT, settled INTEGER NOT NULL DEFAULT 0,
    owed INTEGER NOT NULL DEFAULT 0, directed INTEGER NOT NULL DEFAULT 0,
    report INTEGER NOT NULL DEFAULT 0
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, _at: &str) -> Result<()> {
    match &entry.event {
        Event::Intended {
            due,
            what,
            about,
            place,
            owed,
            directed,
            report,
        } => {
            tx.execute(
                "INSERT INTO intentions (seq, due, what, about, place, owed, directed, report)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entry.seq,
                    stamp(due),
                    what,
                    about,
                    place,
                    owed,
                    directed,
                    report
                ],
            )?;
        }
        Event::Settled { intention, .. } => {
            tx.execute(
                "UPDATE intentions SET settled = 1 WHERE seq = ?1",
                [intention],
            )?;
        }
        _ => {}
    }
    Ok(())
}

const COLUMNS: &str = "seq, due, what, about, place, owed, directed, report";

fn row_to_intention(r: &rusqlite::Row<'_>) -> rusqlite::Result<Intention> {
    Ok(Intention {
        seq: r.get(0)?,
        due: time_at(r, 1)?,
        what: r.get(2)?,
        about: r.get(3)?,
        place: r.get(4)?,
        owed: r.get(5)?,
        directed: r.get(6)?,
        report: r.get(7)?,
    })
}

impl Memory {
    /// What she means to do and has not done, soonest first.
    pub fn open_intentions(&self) -> Result<Vec<Intention>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM intentions WHERE settled = 0 ORDER BY due ASC"
        ))?;
        let rows = stmt.query_map([], row_to_intention)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Her reports to `about` set before entry `before`, carried out or
    /// not, newest first, and no more than `limit` of them.
    pub fn reports_to(&self, about: &str, before: i64, limit: usize) -> Result<Vec<Intention>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM intentions WHERE report = 1 AND about = ?1 AND seq < ?2
             ORDER BY seq DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![about, before, as_i64(limit)], row_to_intention)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::testing::fold;
    use crate::journal::Event;

    #[test]
    fn intentions_open_and_settle() {
        let due = Utc::now();
        let (_, m) = fold(vec![
            Event::Intended {
                due,
                what: "ask about the interview".into(),
                about: Some("t:a".into()),
                place: None,
                owed: false,
                directed: false,
                report: false,
            },
            Event::Intended {
                due,
                what: "second".into(),
                about: None,
                place: None,
                owed: false,
                directed: false,
                report: false,
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
    fn reports_are_found_by_whom_they_are_to_carried_out_or_not() {
        let due = Utc::now();
        let report = |about: &str| Event::Intended {
            due,
            what: "tell them what I have been up to".into(),
            about: Some(about.into()),
            place: Some(about.into()),
            owed: true,
            directed: false,
            report: true,
        };
        let (_, m) = fold(vec![
            report("t:boss"),
            Event::Settled {
                intention: 1,
                outcome: "said 2 things".into(),
            },
            report("t:other"),
            Event::Intended {
                due,
                what: "send the photos".into(),
                about: Some("t:boss".into()),
                place: Some("t:boss".into()),
                owed: true,
                directed: false,
                report: false,
            },
            report("t:boss"),
        ]);
        let seqs = |before| {
            m.reports_to("t:boss", before, 5)
                .unwrap()
                .iter()
                .map(|r| r.seq)
                .collect::<Vec<_>>()
        };
        assert_eq!(seqs(i64::MAX), [5, 1]);
        assert_eq!(seqs(5), [1]);
        let open = m.open_intentions().unwrap();
        assert_eq!(open.iter().filter(|i| i.report).count(), 2);
    }
}
