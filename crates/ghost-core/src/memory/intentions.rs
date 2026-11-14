//! What she means to do, and has not done yet.
//!
//! An intention is something she said to herself she would do by a certain
//! time: ask after someone, come back with an answer she owes. It stays open
//! until she settles it, however that turned out.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, time_at};
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
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS intentions (
    seq INTEGER PRIMARY KEY, due TEXT NOT NULL, what TEXT NOT NULL,
    about TEXT, place TEXT, settled INTEGER NOT NULL DEFAULT 0,
    owed INTEGER NOT NULL DEFAULT 0, directed INTEGER NOT NULL DEFAULT 0
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
        } => {
            tx.execute(
                "INSERT INTO intentions (seq, due, what, about, place, owed, directed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![entry.seq, stamp(due), what, about, place, owed, directed],
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

impl Memory {
    /// What she means to do and has not done, soonest first.
    pub fn open_intentions(&self) -> Result<Vec<Intention>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, due, what, about, place, owed, directed FROM intentions WHERE settled = 0 ORDER BY due ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Intention {
                seq: r.get(0)?,
                due: time_at(r, 1)?,
                what: r.get(2)?,
                about: r.get(3)?,
                place: r.get(4)?,
                owed: r.get(5)?,
                directed: r.get(6)?,
            })
        })?;
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
            },
            Event::Intended {
                due,
                what: "second".into(),
                about: None,
                place: None,
                owed: false,
                directed: false,
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
}
