//! The sessions a console lists: each conversation, by its place, with
//! what it is called, when it began and when anything was last said in it.
//!
//! A conversation is known from the first line said in it, or from the
//! first name an operator gives it, whichever comes first. It goes by that
//! name, or else by its first line, so that every console calls it the
//! same thing. Set aside, it is left out of the list; a line heard there
//! afterwards, or a new name, brings it back. Every conversation on every
//! platform is kept here, since what makes one a session is only which
//! channel a console asks about.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

use super::{Memory, channel_of, opt_time_at, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
    social,
};

/// How much of its first line a session without a name goes by.
const TITLE_CHARS: usize = 48;

/// One conversation, as a console lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Its place key, `channel:id`.
    pub key: String,
    /// Its id on its channel.
    pub id: String,
    /// What it is called: the name it was given, or its first line, or
    /// nothing when it has neither.
    pub title: String,
    /// It was given its name, rather than going by its first line.
    pub named: bool,
    /// When it began: the first line said in it, or its first name.
    pub started: DateTime<Utc>,
    /// When anything was last said in it, either way.
    pub last: Option<DateTime<Utc>>,
    /// It was set aside, and nothing since has brought it back.
    pub set_aside: bool,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    place TEXT PRIMARY KEY, channel TEXT NOT NULL, started TEXT NOT NULL,
    first TEXT, title TEXT, set_aside INTEGER
);
CREATE INDEX IF NOT EXISTS sessions_channel ON sessions (channel, started);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        // A line heard there begins it, or brings it back if it was set
        // aside: someone is speaking in it again.
        Event::Heard {
            channel,
            place,
            text,
            ..
        } => {
            tx.execute(
                "INSERT INTO sessions (place, channel, started, first) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(place) DO UPDATE SET first = COALESCE(first, ?4), set_aside = NULL",
                params![social::key(channel, &place.id), channel, at, text],
            )?;
        }
        // She may speak first somewhere. What she says there brings
        // nothing back: she may be finishing an answer as it is set aside.
        Event::Said { channel, place, .. } => {
            tx.execute(
                "INSERT OR IGNORE INTO sessions (place, channel, started) VALUES (?1, ?2, ?3)",
                params![social::key(channel, place), channel, at],
            )?;
        }
        Event::SessionNamed { place, title, .. } => {
            tx.execute(
                "INSERT INTO sessions (place, channel, started, title) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(place) DO UPDATE SET title = ?4, set_aside = NULL",
                params![place, channel_of(place).unwrap_or_default(), at, title],
            )?;
        }
        Event::SessionSetAside { place, .. } => {
            tx.execute(
                "UPDATE sessions SET set_aside = ?2 WHERE place = ?1",
                params![place, entry.seq],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// What a session without a name goes by: the first line of what was
/// first said in it that has anything on it, cut short.
fn title_of(first: &str) -> String {
    first
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .chars()
        .take(TITLE_CHARS)
        .collect()
}

impl Memory {
    /// Every conversation on `channel`, newest first, those set aside
    /// among them, marked so.
    pub fn sessions(&self, channel: &str) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.place, s.title, s.first, s.started, s.set_aside IS NOT NULL,
                    MAX(COALESCE(p.last_in_at, ''), COALESCE(p.last_out_at, ''))
             FROM sessions s LEFT JOIN places p ON p.key = s.place
             WHERE s.channel = ?1
             ORDER BY s.started DESC, s.place DESC",
        )?;
        let rows = stmt.query_map([channel], |r| {
            let key: String = r.get(0)?;
            let title: Option<String> = r.get(1)?;
            let first: Option<String> = r.get(2)?;
            Ok(Session {
                id: social::split_key(&key).map_or_else(|| key.clone(), |(_, id)| id.to_owned()),
                key,
                named: title.is_some(),
                title: title.unwrap_or_else(|| first.as_deref().map(title_of).unwrap_or_default()),
                started: time_at(r, 3)?,
                set_aside: r.get(4)?,
                last: opt_time_at(r, 5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::super::testing::{heard, said};
    use super::*;
    use crate::{
        journal::{Event, Journal},
        social::PlaceKind,
    };

    /// Fold `events` in, a minute apart, as the journal hands them over.
    fn lived(events: Vec<Event>) -> (Journal, Memory) {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        let start = Utc.with_ymd_and_hms(2026, 11, 19, 9, 0, 0).unwrap();
        for (n, e) in events.into_iter().enumerate() {
            let at = start + Duration::minutes(i64::try_from(n).unwrap());
            let entry = j.append_at(at, e).unwrap();
            m.apply(&entry).unwrap();
        }
        (j, m)
    }

    fn named(place: &str, title: &str) -> Event {
        Event::SessionNamed {
            place: place.into(),
            by: "t:fox".into(),
            title: title.into(),
        }
    }

    fn set_aside(place: &str) -> Event {
        Event::SessionSetAside {
            place: place.into(),
            by: "t:fox".into(),
        }
    }

    fn titles(m: &Memory) -> Vec<(String, String, bool)> {
        m.sessions("t")
            .unwrap()
            .into_iter()
            .map(|s| (s.id, s.title, s.set_aside))
            .collect()
    }

    #[test]
    fn a_session_goes_by_its_first_line_until_it_is_named() {
        let (_, m) = lived(vec![
            heard(
                "s1",
                PlaceKind::Direct,
                "fox",
                "1",
                "\n  Free some disk space on this machine, the big caches first\nthen tell me",
                true,
            ),
            said("s1", "On it."),
            heard("s1", PlaceKind::Direct, "fox", "2", "and the logs", true),
        ]);
        let listed = m.sessions("t").unwrap();
        assert_eq!(listed.len(), 1);
        let s = &listed[0];
        assert_eq!(s.title, "Free some disk space on this machine, the big ca");
        assert_eq!(s.title.chars().count(), TITLE_CHARS);
        assert!(!s.named);
        assert_eq!(s.started.to_rfc3339(), "2026-11-19T09:00:00+00:00");
        assert_eq!(
            s.last.map(|l| l.to_rfc3339()).as_deref(),
            Some("2026-11-19T09:02:00+00:00"),
            "the last line either way"
        );

        let (_, m) = lived(vec![
            heard("s1", PlaceKind::Direct, "fox", "1", "free some disk", true),
            named("t:s1", "Disk"),
            named("t:s1", "Disk space"),
        ]);
        assert_eq!(titles(&m), [("s1".into(), "Disk space".into(), false)]);
        assert!(m.sessions("t").unwrap()[0].named);
    }

    #[test]
    fn a_session_named_before_anything_is_said_is_listed_newest_first() {
        let (_, m) = lived(vec![
            heard("s1", PlaceKind::Direct, "fox", "1", "older", true),
            named("t:s2", "Imported"),
            heard("s3", PlaceKind::Direct, "fox", "2", "newest", true),
        ]);
        assert_eq!(
            titles(&m),
            [
                ("s3".into(), "newest".into(), false),
                ("s2".into(), "Imported".into(), false),
                ("s1".into(), "older".into(), false),
            ]
        );
        assert_eq!(m.sessions("t").unwrap()[1].last, None);
        assert!(
            m.sessions("qq").unwrap().is_empty(),
            "one channel at a time"
        );
    }

    #[test]
    fn a_session_set_aside_stays_aside_until_someone_speaks_in_it_or_names_it() {
        let (_, m) = lived(vec![
            heard("s1", PlaceKind::Direct, "fox", "1", "one", true),
            heard("s2", PlaceKind::Direct, "fox", "2", "two", true),
            set_aside("t:s1"),
            set_aside("t:s2"),
            // Her answer, finished as it was set aside, brings nothing back.
            said("s1", "done"),
        ]);
        assert_eq!(
            titles(&m),
            [
                ("s2".into(), "two".into(), true),
                ("s1".into(), "one".into(), true)
            ]
        );

        let (_, m) = lived(vec![
            heard("s1", PlaceKind::Direct, "fox", "1", "one", true),
            heard("s2", PlaceKind::Direct, "fox", "2", "two", true),
            set_aside("t:s1"),
            set_aside("t:s2"),
            heard("s1", PlaceKind::Direct, "fox", "3", "one again", true),
            named("t:s2", "Two"),
        ]);
        assert_eq!(
            titles(&m),
            [
                ("s2".into(), "Two".into(), false),
                ("s1".into(), "one".into(), false)
            ]
        );
    }

    #[test]
    fn the_sessions_are_rebuilt_from_the_journal_alone() {
        let (j, m) = lived(vec![
            heard("s1", PlaceKind::Direct, "fox", "1", "free some disk", true),
            said("s1", "On it."),
            named("t:s1", "Disk"),
            heard(
                "s2",
                PlaceKind::Direct,
                "fox",
                "2",
                "summarise the commits",
                true,
            ),
            set_aside("t:s2"),
            named("t:s3", "Imported"),
        ]);
        let mut rebuilt = Memory::in_memory().unwrap();
        rebuilt.catch_up(&j).unwrap();
        assert_eq!(rebuilt.sessions("t").unwrap(), m.sessions("t").unwrap());
        assert_eq!(rebuilt.sessions("t").unwrap().len(), 3);
    }
}
