//! What she knows of people: the notes she keeps and the accounts she puts
//! in order from them.
//!
//! A note is one thing worth keeping about someone, with the place she
//! learned it, so that what was said in private is not repeated elsewhere.
//! Whether that place was a private conversation is decided once, as the
//! note is folded in, so that nothing reading notes has to look it up.
//! A profile is her standing account of a person, written now and then from
//! the notes; each one replaces the last, and notes written after it are
//! what it does not yet know. Both are her memory, found from any platform.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, columns, sources_at, time_at};
use crate::{
    Result,
    journal::{Entry, Event, Sources},
    social::PlaceKind,
};

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub text: String,
    /// Where she learned it.
    pub place: Option<String>,
    /// Whether `place` was a private conversation when she learned it.
    pub private: bool,
    /// What she had in front of her when she wrote it.
    pub sources: Option<Sources>,
}

/// Her standing account of someone, as last put in order.
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    /// The seq of the entry that wrote it. Notes after this are newer.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub shared: String,
    pub private: String,
    /// What it was put in order from, when that was written down.
    pub sources: Option<Sources>,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 2;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS notes (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, person TEXT NOT NULL, text TEXT NOT NULL,
    place TEXT, private INTEGER NOT NULL, src_from INTEGER, src_upto INTEGER,
    since TEXT, until TEXT
);
CREATE INDEX IF NOT EXISTS notes_person ON notes (person, seq);
CREATE INDEX IF NOT EXISTS notes_private ON notes (private, seq);
CREATE TABLE IF NOT EXISTS profiles (
    person TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL,
    shared TEXT NOT NULL, private TEXT NOT NULL,
    src_from INTEGER, src_upto INTEGER, since TEXT, until TEXT
);
";

/// The columns a note is read from, in the order `note_at` reads them.
const NOTE: &str = "seq, at, text, place, private, src_from, src_upto, since, until";

fn note_at(r: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        seq: r.get(0)?,
        at: time_at(r, 1)?,
        text: r.get(2)?,
        place: r.get(3)?,
        private: r.get(4)?,
        sources: sources_at(r, 5)?,
    })
}

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let seq = entry.seq;
    match &entry.event {
        Event::Noted {
            about,
            text,
            place,
            sources,
        } => {
            let (from, upto, since, until) = columns(sources.as_ref());
            // As the place was at this entry: places are folded before
            // notes.
            let private = match place {
                Some(key) => tx
                    .query_row("SELECT kind FROM places WHERE key = ?1", [key], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()?
                    .is_some_and(|kind| kind == PlaceKind::Direct.as_str()),
                None => false,
            };
            tx.execute(
                "INSERT INTO notes (seq, at, person, text, place, private, src_from, src_upto, since, until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![seq, at, about, text, place, private, from, upto, since, until],
            )?;
        }
        Event::Profiled {
            person,
            shared,
            private,
            sources,
        } => {
            let (from, upto, since, until) = columns(sources.as_ref());
            tx.execute(
                "INSERT INTO profiles (person, seq, at, shared, private, src_from, src_upto, since, until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(person) DO UPDATE SET seq = ?2, at = ?3, shared = ?4, private = ?5,
                   src_from = ?6, src_upto = ?7, since = ?8, until = ?9",
                params![person, seq, at, shared, private, from, upto, since, until],
            )?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// Notes about someone, the newest `limit` of them, oldest first.
    pub fn notes(&self, person: &str, limit: usize) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE} FROM notes WHERE person = ?1 ORDER BY seq DESC LIMIT ?2"
        ))?;
        let mut notes = stmt
            .query_map(params![person, as_i64(limit)], note_at)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        notes.reverse();
        Ok(notes)
    }

    /// The newest `limit` notes she learned in private, about anyone, each
    /// with the name of whoever it is about; oldest first.
    pub fn private_notes(&self, limit: usize) -> Result<Vec<(String, Note)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE}, COALESCE(people.name, notes.person) FROM notes
             LEFT JOIN people ON people.key = notes.person
             WHERE private = 1 ORDER BY seq DESC LIMIT ?1"
        ))?;
        let mut notes = stmt
            .query_map([as_i64(limit)], |r| Ok((r.get(9)?, note_at(r)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        notes.reverse();
        Ok(notes)
    }

    /// Notes about someone written after entry `after`, oldest first.
    pub fn notes_after(&self, person: &str, after: i64, limit: usize) -> Result<Vec<Note>> {
        let mut notes = self.notes(person, limit)?;
        notes.retain(|n| n.seq > after);
        Ok(notes)
    }

    /// Her standing account of someone, if she has written one.
    pub fn profile(&self, person: &str) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT seq, at, shared, private, src_from, src_upto, since, until
                 FROM profiles WHERE person = ?1",
                [person],
                |r| {
                    Ok(Profile {
                        seq: r.get(0)?,
                        at: time_at(r, 1)?,
                        shared: r.get(2)?,
                        private: r.get(3)?,
                        sources: sources_at(r, 4)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::testing::{fold, heard};
    use crate::{
        journal::{Event, Sources},
        social::PlaceKind,
    };

    #[test]
    fn a_note_is_private_when_it_was_learned_in_a_private_conversation() {
        let note = |text: &str, place: Option<&str>| Event::Noted {
            about: "t:a".into(),
            text: text.into(),
            place: place.map(Into::into),
            sources: None,
        };
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "between us", true),
            heard("g", PlaceKind::Group, "a", "2", "hello all", false),
            note("told alone", Some("t:a")),
            note("told in the group", Some("t:g")),
            note("from nowhere in particular", None),
            note("from a place she never heard from", Some("t:gone")),
        ]);
        let private: Vec<(String, bool)> = m
            .notes("t:a", 10)
            .unwrap()
            .into_iter()
            .map(|n| (n.text, n.private))
            .collect();
        assert_eq!(
            private,
            [
                ("told alone".to_owned(), true),
                ("told in the group".to_owned(), false),
                ("from nowhere in particular".to_owned(), false),
                ("from a place she never heard from".to_owned(), false),
            ]
        );
        let told: Vec<(String, String)> = m
            .private_notes(10)
            .unwrap()
            .into_iter()
            .map(|(name, n)| (name, n.text))
            .collect();
        assert_eq!(told, [("A".to_owned(), "told alone".to_owned())]);
    }

    #[test]
    fn what_was_learned_keeps_where_it_was_learned() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "my sister is moving to Kunming",
                true,
            ),
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "sister is moving to Kunming".into(),
                place: Some("t:a".into()),
            },
        ]);
        assert_eq!(m.notes("t:a", 5).unwrap()[0].place.as_deref(), Some("t:a"));
        let hits = m.search("t", "Kunming", 5).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.place.as_deref() == Some("t:a")));
    }

    #[test]
    fn a_profile_replaces_the_one_before_and_dates_the_notes() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "designs buildings".into(),
                place: None,
            },
            Event::Profiled {
                sources: None,
                person: "t:a".into(),
                shared: "An architect.".into(),
                private: String::new(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "is leaving the institute".into(),
                place: Some("t:a".into()),
            },
            Event::Profiled {
                sources: None,
                person: "t:a".into(),
                shared: "An architect, careful with words.".into(),
                private: "Leaving the institute; has told nobody.".into(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "likes the plane trees".into(),
                place: None,
            },
        ]);
        let profile = m.profile("t:a").unwrap().unwrap();
        assert_eq!(profile.shared, "An architect, careful with words.");
        assert!(profile.private.starts_with("Leaving"));
        let since: Vec<String> = m
            .notes_after("t:a", profile.seq, 10)
            .unwrap()
            .into_iter()
            .map(|n| n.text)
            .collect();
        assert_eq!(since, ["likes the plane trees"]);
        assert!(m.profile("t:nobody").unwrap().is_none());
    }

    #[test]
    fn what_a_memory_was_made_from_is_kept_with_it() {
        let then = Utc::now() - chrono::Duration::days(3);
        let sources = Sources {
            from: 2,
            upto: 9,
            since: then,
            until: then + chrono::Duration::hours(2),
        };
        let (_, m) = fold(vec![
            Event::Noted {
                about: "t:a".into(),
                text: "is leaving the institute".into(),
                place: Some("t:a".into()),
                sources: Some(sources),
            },
            Event::Profiled {
                person: "t:a".into(),
                shared: "An architect.".into(),
                private: String::new(),
                sources: Some(sources),
            },
            Event::Digested {
                place: "t:a".into(),
                upto: 9,
                text: "Talk of the institute.".into(),
                sources: Some(sources),
            },
            Event::Noted {
                about: "t:a".into(),
                text: "likes the plane trees".into(),
                place: None,
                sources: None,
            },
        ]);
        let kept = |s: Option<Sources>| s.map(|s| (s.from, s.upto, s.since.timestamp()));
        let want = Some((2, 9, then.timestamp()));
        let notes = m.notes("t:a", 10).unwrap();
        assert_eq!(kept(notes[0].sources), want);
        assert_eq!(
            notes[1].sources, None,
            "a note from before sources were kept"
        );
        assert_eq!(kept(m.profile("t:a").unwrap().unwrap().sources), want);
        assert_eq!(kept(m.digest("t:a").unwrap().unwrap().sources), want);
    }
}
