//! What she holds until she changes her mind: her craft, her pursuits, her
//! practice.
//!
//! Three lists of the same shape. Craft is what she has worked out about
//! doing well in public; pursuits are what she is after over months; practice
//! is what doing things has taught her about doing things. Each entry is
//! held until a later one replaces it, and a pursuit can be let go with
//! nothing in its place.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
};

/// Something she has learned about her craft and not since replaced.
#[derive(Debug, Clone, Serialize)]
pub struct Lesson {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub lesson: String,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS craft (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS pursuits (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS practice (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let (table, lesson, replaces) = match &entry.event {
        Event::Learned { lesson, replaces } => ("craft", lesson, replaces),
        Event::Pursued { what, replaces } => ("pursuits", what, replaces),
        Event::Practised { lesson, replaces } => ("practice", lesson, replaces),
        _ => return Ok(()),
    };
    if let Some(old) = replaces {
        tx.execute(
            &format!("UPDATE {table} SET retired = 1 WHERE seq = ?1"),
            [old],
        )?;
    }
    // An empty pursuit lets the one it replaces go and puts nothing in its
    // place.
    if table == "pursuits" && lesson.trim().is_empty() {
        return Ok(());
    }
    tx.execute(
        &format!("INSERT INTO {table} (seq, at, lesson) VALUES (?1, ?2, ?3)"),
        params![entry.seq, at, lesson],
    )?;
    Ok(())
}

impl Memory {
    /// What she has learned about her craft and still holds, oldest first.
    pub fn craft(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("craft", limit)
    }

    /// What she is after over months and has not let go of, oldest first.
    pub fn pursuits(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("pursuits", limit)
    }

    /// What doing things has taught her about doing things, oldest first.
    pub fn practice(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("practice", limit)
    }

    /// The newest entries of one of the tables of things she holds until
    /// she replaces them, oldest first.
    fn held(&self, table: &'static str, limit: usize) -> Result<Vec<Lesson>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT seq, at, lesson FROM {table} WHERE retired = 0 ORDER BY seq DESC LIMIT ?1"
        ))?;
        let mut lessons = stmt
            .query_map([as_i64(limit)], |r| {
                Ok(Lesson {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    lesson: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lessons.reverse();
        Ok(lessons)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::fold;
    use crate::journal::Event;

    #[test]
    fn a_lesson_replaces_the_one_she_changed_her_mind_about() {
        let (_, m) = fold(vec![
            Event::Learned {
                lesson: "long threads do well".into(),
                replaces: None,
            },
            Event::Learned {
                lesson: "short posts about one street do better than threads".into(),
                replaces: Some(1),
            },
        ]);
        let held = m.craft(10).unwrap();
        assert_eq!(held.len(), 1);
        assert!(held[0].lesson.starts_with("short posts"));
    }

    #[test]
    fn what_she_is_after_and_what_work_taught_her_are_held_until_replaced() {
        let (journal, m) = fold(vec![
            Event::Pursued {
                what: "draw every tea house in the city".into(),
                replaces: None,
            },
            Event::Practised {
                lesson: "check the opening hours on the day, pages go stale".into(),
                replaces: None,
            },
        ]);
        let first = m.pursuits(5).unwrap()[0].seq;
        assert_eq!(m.practice(5).unwrap().len(), 1);
        drop((journal, m));
        let (_, m) = fold(vec![
            Event::Pursued {
                what: "draw every tea house in the city".into(),
                replaces: None,
            },
            Event::Pursued {
                what: "draw the ones that are closing first".into(),
                replaces: Some(first),
            },
            Event::Pursued {
                what: "learn to bind a book".into(),
                replaces: None,
            },
        ]);
        let held: Vec<String> = m
            .pursuits(5)
            .unwrap()
            .into_iter()
            .map(|p| p.lesson)
            .collect();
        assert_eq!(
            held,
            [
                "draw the ones that are closing first",
                "learn to bind a book"
            ]
        );
        // Letting one go: nothing in its place.
        let (_, m) = fold(vec![
            Event::Pursued {
                what: "a".into(),
                replaces: None,
            },
            Event::Pursued {
                what: String::new(),
                replaces: Some(1),
            },
        ]);
        assert!(m.pursuits(5).unwrap().is_empty());
    }
}
