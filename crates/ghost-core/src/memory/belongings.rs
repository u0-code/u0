//! What she has kept, and still holds.
//!
//! Something she saw and wanted to have again, with her note on it: what it
//! is and when she would want it. Keeping the same thing again replaces the
//! note, and letting go of it ends her holding it. What she let go of, or
//! wrote a newer note over, stays in the table as no longer held, so that
//! whether she ever kept a thing from a place can still be asked.
//!
//! It is her own memory, whole: found from any platform, by her note, through
//! the recall index (see `recall.rs`).

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
};

/// Something she has kept and still holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Belonging {
    /// The seq of the `Kept` entry: the number she knows it by.
    pub seq: i64,
    pub at: DateTime<Utc>,
    /// The name it is kept under in `media/`.
    pub file: String,
    pub note: String,
    /// Where she got it, as the `Kept` entry says.
    pub from: Option<String>,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS belongings (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, file TEXT NOT NULL, note TEXT NOT NULL,
    origin TEXT, held INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS belongings_file ON belongings (file, held);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        Event::Kept { file, note, from } => {
            // The latest note on a thing is the one that holds.
            tx.execute(
                "UPDATE belongings SET held = 0 WHERE file = ?1 AND held = 1",
                [file],
            )?;
            tx.execute(
                "INSERT INTO belongings (seq, at, file, note, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.seq, at, file, note, from],
            )?;
        }
        Event::LetGo { kept } => {
            tx.execute("UPDATE belongings SET held = 0 WHERE seq = ?1", [kept])?;
        }
        _ => {}
    }
    Ok(())
}

const COLUMNS: &str = "seq, at, file, note, origin";

fn row_to_belonging(r: &rusqlite::Row<'_>) -> rusqlite::Result<Belonging> {
    Ok(Belonging {
        seq: r.get(0)?,
        at: time_at(r, 1)?,
        file: r.get(2)?,
        note: r.get(3)?,
        from: r.get(4)?,
    })
}

impl Memory {
    /// The thing she kept as entry `seq`, if she still holds it under that
    /// note.
    pub fn belonging(&self, seq: i64) -> Result<Option<Belonging>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM belongings WHERE seq = ?1 AND held = 1"),
                [seq],
                row_to_belonging,
            )
            .optional()?)
    }

    /// What she holds of the file `file`, if anything.
    pub fn holding(&self, file: &str) -> Result<Option<Belonging>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM belongings WHERE file = ?1 AND held = 1"),
                [file],
                row_to_belonging,
            )
            .optional()?)
    }

    /// The newest `limit` things she holds, oldest first.
    pub fn belongings(&self, limit: usize) -> Result<Vec<Belonging>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM belongings WHERE held = 1 ORDER BY seq DESC LIMIT ?1"
        ))?;
        let mut held = stmt
            .query_map([as_i64(limit)], row_to_belonging)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        held.reverse();
        Ok(held)
    }

    /// What she got from `from` and still holds, the newest `limit`,
    /// oldest first: from one place, or from one piece of work.
    pub fn kept_from(&self, from: &str, limit: usize) -> Result<Vec<Belonging>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM belongings WHERE origin = ?1 AND held = 1
             ORDER BY seq DESC LIMIT ?2"
        ))?;
        let mut held = stmt
            .query_map(params![from, as_i64(limit)], row_to_belonging)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        held.reverse();
        Ok(held)
    }

    /// Whether she has ever kept `file` from `from` with exactly this note,
    /// whether or not she still holds it.
    pub fn ever_kept(&self, file: &str, note: &str, from: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM belongings WHERE file = ?1 AND note = ?2 AND origin = ?3 LIMIT 1",
                params![file, note, from],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{fold, heard, on};
    use crate::{journal::Event, social::PlaceKind};

    fn kept(file: &str, note: &str) -> Event {
        Event::Kept {
            file: file.into(),
            note: note.into(),
            from: Some("qq:b".into()),
        }
    }

    #[test]
    fn what_she_keeps_is_held_and_found_by_her_note_from_any_platform() {
        let (_, m) = fold(vec![
            on(
                "qq",
                heard("b", PlaceKind::Direct, "b", "1", "[image]", true),
            ),
            kept("a.png", "A heron standing in the rain. For grey mornings."),
        ]);
        let held = m.belongings(5).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].seq, 2);
        assert_eq!(held[0].file, "a.png");
        assert_eq!(held[0].from.as_deref(), Some("qq:b"));
        assert_eq!(m.belonging(2).unwrap().unwrap(), held[0]);
        assert_eq!(m.holding("a.png").unwrap().unwrap().seq, 2);
        // Her memory, whole: found from a platform it was not sent on.
        for channel in ["qq", "telegram"] {
            let hits = m.search(channel, "heron", 5).unwrap();
            assert_eq!(hits.len(), 1, "{channel}");
            assert_eq!((hits[0].seq, hits[0].kind.as_str()), (2, "kept"));
            assert_eq!(hits[0].place, None);
        }
    }

    #[test]
    fn keeping_the_same_thing_again_replaces_the_note() {
        let (_, m) = fold(vec![
            kept("a.png", "A heron in the rain."),
            kept("b.png", "The east gate."),
            kept(
                "a.png",
                "The heron again. For when someone is waiting on something.",
            ),
        ]);
        let held: Vec<(i64, String)> = m
            .belongings(5)
            .unwrap()
            .into_iter()
            .map(|b| (b.seq, b.file))
            .collect();
        assert_eq!(held, [(2, "b.png".into()), (3, "a.png".into())]);
        assert!(m.belonging(1).unwrap().is_none());
        // Only the note that holds is found.
        let found: Vec<i64> = m
            .search("t", "heron", 5)
            .unwrap()
            .into_iter()
            .map(|h| h.seq)
            .collect();
        assert_eq!(found, [3]);
        assert!(m.search("t", "rain", 5).unwrap().is_empty());
        assert!(
            m.ever_kept("a.png", "A heron in the rain.", "qq:b")
                .unwrap()
        );
    }

    #[test]
    fn what_she_holds_from_one_piece_of_work_is_listed_apart() {
        let from = |file: &str, origin: &str| Event::Kept {
            file: file.into(),
            note: format!("{file}, kept"),
            from: Some(origin.into()),
        };
        let (_, m) = fold(vec![
            from("a.png", "u1"),
            from("b.png", "qq:b"),
            from("c.png", "u1"),
            Event::LetGo { kept: 1 },
        ]);
        let files: Vec<String> = m
            .kept_from("u1", 5)
            .unwrap()
            .into_iter()
            .map(|b| b.file)
            .collect();
        assert_eq!(files, ["c.png"]);
    }

    #[test]
    fn something_let_go_is_no_longer_held_or_found() {
        let (_, m) = fold(vec![
            kept("a.png", "A heron in the rain."),
            kept("a.png", "The heron again."),
            // Letting go of a note already replaced changes nothing.
            Event::LetGo { kept: 1 },
        ]);
        assert_eq!(m.belongings(5).unwrap().len(), 1);
        assert_eq!(m.search("t", "heron", 5).unwrap().len(), 1);

        let (_, m) = fold(vec![
            kept("a.png", "A heron in the rain."),
            Event::LetGo { kept: 1 },
        ]);
        assert!(m.belongings(5).unwrap().is_empty());
        assert!(m.holding("a.png").unwrap().is_none());
        assert!(m.search("t", "heron", 5).unwrap().is_empty());
        // That she once kept it is still known.
        assert!(
            m.ever_kept("a.png", "A heron in the rain.", "qq:b")
                .unwrap()
        );
        assert!(!m.ever_kept("a.png", "Something else.", "qq:b").unwrap());
    }
}
