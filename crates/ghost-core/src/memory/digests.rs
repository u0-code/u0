//! What she kept of the earlier part of each conversation.
//!
//! A conversation runs longer than she can hold in front of her. Now and then
//! she writes down what the older part of it came to; each digest of a place
//! replaces the one before, and what is newer than it is read as it was said.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, columns, sources_at, time_at};
use crate::{
    Result,
    journal::{Entry, Event, Sources},
};

/// What she kept of the earlier part of a conversation.
#[derive(Debug, Clone, Serialize)]
pub struct Digest {
    /// The newest message it stands for.
    pub upto: i64,
    pub at: DateTime<Utc>,
    pub text: String,
    /// Everything it stands for, when that was written down.
    pub sources: Option<Sources>,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS digests (
    place TEXT PRIMARY KEY, upto INTEGER NOT NULL, at TEXT NOT NULL, text TEXT NOT NULL,
    src_from INTEGER, src_upto INTEGER, since TEXT, until TEXT
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    if let Event::Digested {
        place,
        upto,
        text,
        sources,
    } = &entry.event
    {
        let (from, src_upto, since, until) = columns(sources.as_ref());
        tx.execute(
            "INSERT INTO digests (place, upto, at, text, src_from, src_upto, since, until)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(place) DO UPDATE SET upto = ?2, at = ?3, text = ?4,
               src_from = ?5, src_upto = ?6, since = ?7, until = ?8",
            params![place, upto, at, text, from, src_upto, since, until],
        )?;
    }
    Ok(())
}

impl Memory {
    /// What she kept of the earlier part of a conversation, if she has.
    pub fn digest(&self, place: &str) -> Result<Option<Digest>> {
        Ok(self
            .conn
            .query_row(
                "SELECT upto, at, text, src_from, src_upto, since, until FROM digests WHERE place = ?1",
                [place],
                |r| {
                    Ok(Digest {
                        upto: r.get(0)?,
                        at: time_at(r, 1)?,
                        text: r.get(2)?,
                        sources: sources_at(r, 3)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{fold, heard};
    use crate::{journal::Event, social::PlaceKind};

    #[test]
    fn a_digest_replaces_the_one_before() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "one", true),
            heard("a", PlaceKind::Direct, "a", "2", "two", true),
            heard("a", PlaceKind::Direct, "a", "3", "three", true),
            Event::Digested {
                sources: None,
                place: "t:a".into(),
                upto: 1,
                text: "said one".into(),
            },
            Event::Digested {
                sources: None,
                place: "t:a".into(),
                upto: 2,
                text: "said one, then two".into(),
            },
        ]);
        let digest = m.digest("t:a").unwrap().unwrap();
        assert_eq!(
            (digest.upto, digest.text.as_str()),
            (2, "said one, then two")
        );
        assert!(m.digest("t:b").unwrap().is_none());
        assert_eq!(m.lines_after("t:a", digest.upto).unwrap(), 1);
        let between = m.thread_between("t:a", 0, 2, 10).unwrap();
        assert_eq!(between.len(), 2);
        assert_eq!(between[1].text, "two");
    }
}
