//! Which entries of her journal make up the conversation in each place.
//!
//! A console shows a place as a thread: what was said there, and what she
//! read, decided, felt, noted and intended about it. Deciding which entry
//! belongs where takes more than the entry itself (an image she looked at
//! belongs where the message was heard; a note about someone belongs in
//! her private conversation with them), so it is decided once, as each
//! entry is folded in, and kept here as `(place, seq)`. Reading a thread is
//! then a walk along one index, however long she has lived.
//!
//! What a line taken back carried with it leaves here as it leaves her
//! record of what was said: the journal keeps the words.

use rusqlite::{OptionalExtension, Transaction, params};

use super::{Memory, as_i64};
use crate::{
    Result,
    journal::{Entry, Event},
    social,
};

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS thread_of (
    place TEXT NOT NULL, seq INTEGER NOT NULL, PRIMARY KEY (place, seq)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS thread_of_seq ON thread_of (seq);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, _at: &str) -> Result<()> {
    let places: Vec<String> = match &entry.event {
        Event::Heard { channel, place, .. } => vec![social::key(channel, &place.id)],
        Event::Read { channel, place, .. }
        | Event::Said { channel, place, .. }
        | Event::Reacted { channel, place, .. } => vec![social::key(channel, place)],
        Event::Decided { about, .. } => vec![about.clone()],
        // Where she meant to do it, and the private conversation with
        // whoever it is about.
        Event::Intended { place, about, .. } => {
            let mut places: Vec<String> = place.iter().cloned().collect();
            if let Some(direct) = about.as_deref().map(|p| direct_of(tx, p)).transpose()? {
                places.extend(direct);
            }
            places
        }
        Event::Viewed { message, .. } => places_of(tx, *message)?,
        Event::Settled { intention, .. } => places_of(tx, *intention)?,
        Event::Felt { toward: person, .. }
        | Event::Noted { about: person, .. }
        | Event::Left { person, .. } => direct_of(tx, person)?.into_iter().collect(),
        Event::Canon { source, .. } => source
            .strip_prefix("said in ")
            .map(str::to_owned)
            .into_iter()
            .collect(),
        Event::Stopped { place, .. } => vec![place.clone()],
        Event::Rewound { place, to, .. } => {
            tx.execute(
                "DELETE FROM thread_of WHERE place = ?1 AND seq >= ?2 AND seq < ?3",
                params![place, to, entry.seq],
            )?;
            vec![place.clone()]
        }
        _ => Vec::new(),
    };
    for place in places {
        tx.execute(
            "INSERT OR IGNORE INTO thread_of (place, seq) VALUES (?1, ?2)",
            params![place, entry.seq],
        )?;
    }
    Ok(())
}

/// The places the entry `seq` belongs to, if it was folded in before.
fn places_of(tx: &Transaction<'_>, seq: i64) -> Result<Vec<String>> {
    let mut stmt = tx.prepare("SELECT place FROM thread_of WHERE seq = ?1")?;
    let places = stmt
        .query_map([seq], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(places)
}

/// Her private conversation with the person `key`, if she has had one.
fn direct_of(tx: &Transaction<'_>, key: &str) -> Result<Option<String>> {
    Ok(tx
        .query_row("SELECT direct FROM people WHERE key = ?1", [key], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten())
}

impl Memory {
    /// The entries that make up the conversation in `place`, by seq: the
    /// newest `limit` of them, oldest first.
    pub fn thread_of(&self, place: &str, limit: usize) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq FROM thread_of WHERE place = ?1 ORDER BY seq DESC LIMIT ?2")?;
        let mut seqs = stmt
            .query_map(params![place, as_i64(limit)], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        seqs.reverse();
        Ok(seqs)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::{
        journal::Event,
        memory::testing::{fold, heard, said},
        social::PlaceKind,
    };

    #[test]
    fn a_thread_is_what_was_said_in_a_place_and_what_she_made_of_it_there() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "look at this [image]",
                true,
            ),
            heard("g", PlaceKind::Group, "b", "2", "hello all", false),
            Event::Viewed {
                message: 1,
                image: 0,
                what: "a cat".into(),
                file: None,
            },
            Event::Decided {
                about: "t:a".into(),
                decision: "said something".into(),
                reason: "asked".into(),
            },
            said("a", "a fine cat"),
            // A note about them is part of the private conversation, from
            // wherever she learned it.
            Event::Noted {
                about: "t:a".into(),
                text: "keeps cats".into(),
                place: Some("t:g".into()),
                sources: None,
            },
            Event::Intended {
                due: Utc::now(),
                what: "ask after the cat".into(),
                about: Some("t:a".into()),
                place: Some("t:g".into()),
                owed: false,
                directed: false,
                report: false,
            },
            Event::Settled {
                intention: 7,
                outcome: "done".into(),
            },
            Event::Canon {
                fact: "she likes cats".into(),
                source: "said in t:a".into(),
            },
        ]);
        assert_eq!(m.thread_of("t:a", 20).unwrap(), [1, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(m.thread_of("t:g", 20).unwrap(), [2, 7, 8]);
        assert_eq!(
            m.thread_of("t:a", 2).unwrap(),
            [8, 9],
            "the newest, in order"
        );
        assert!(m.thread_of("t:nobody", 20).unwrap().is_empty());
    }

    #[test]
    fn a_line_taken_back_leaves_the_thread_with_everything_after_it() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "one", true),
            said("a", "two"),
            heard("a", PlaceKind::Direct, "a", "2", "three", true),
            said("a", "four"),
            Event::Rewound {
                place: "t:a".into(),
                by: "t:a".into(),
                to: 3,
                looking: false,
                unsaid: Vec::new(),
            },
            heard("a", PlaceKind::Direct, "a", "3", "five", true),
        ]);
        assert_eq!(m.thread_of("t:a", 20).unwrap(), [1, 2, 5, 6]);
    }
}
