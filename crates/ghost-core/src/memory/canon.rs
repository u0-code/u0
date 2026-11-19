//! The canon of her own life.
//!
//! Hard facts about her life, so that what she says of herself holds
//! together: where she lives, what her cat is called. A fact registered here
//! stays; nothing in the journal takes one back.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
};

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub fact: String,
    pub source: String,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS canon (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, fact TEXT NOT NULL, source TEXT NOT NULL
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    if let Event::Canon { fact, source } = &entry.event {
        tx.execute(
            "INSERT INTO canon (seq, at, fact, source) VALUES (?1, ?2, ?3, ?4)",
            params![entry.seq, at, fact, source],
        )?;
    }
    Ok(())
}

impl Memory {
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
}

#[cfg(test)]
mod tests {
    use super::super::testing::fold;
    use crate::journal::Event;

    #[test]
    fn the_canon_keeps_the_newest_facts_oldest_first() {
        let (_, m) = fold(vec![
            Event::Canon {
                fact: "lives near the old gate".into(),
                source: "life".into(),
            },
            Event::Canon {
                fact: "has a cat called Doubao".into(),
                source: "said".into(),
            },
            Event::Canon {
                fact: "draws in the mornings".into(),
                source: "life".into(),
            },
        ]);
        let facts: Vec<String> = m.canon(2).unwrap().into_iter().map(|f| f.fact).collect();
        assert_eq!(facts, ["has a cat called Doubao", "draws in the mornings"]);
    }
}
