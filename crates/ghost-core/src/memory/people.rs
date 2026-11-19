//! The people she has heard from, as each platform knows them.
//!
//! An account is someone on one platform: what they are called there, when
//! she first and last heard from them, how warm she is toward them, and
//! whether she has left them. The same person on two platforms is two
//! accounts until she is shown otherwise (see `links`).

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event, parse_stamp, stamp},
    social::{self, PlaceKind},
};

const WARMTH_MIN: i64 = -20;
const WARMTH_MAX: i64 = 20;

/// One shift in how she feels about someone, and what did it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Feeling {
    pub at: DateTime<Utc>,
    /// Which way, and how far: above zero toward them, below away.
    pub delta: i64,
    pub why: String,
}

/// Someone she has heard from.
#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub automated: bool,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    /// Messages she has heard from them, anywhere.
    pub heard: i64,
    /// Sum of every `Felt` toward them, clamped.
    pub warmth: i64,
    /// She has left them.
    pub gone: bool,
    /// Key of the private place she shares with them, once one exists.
    pub direct: Option<String>,
    /// Distinct days on which she heard from them.
    pub active_days: i64,
    /// Distinct days on which she wrote to them privately without an answer since.
    pub unanswered_days: i64,
    /// When she last wrote to them privately.
    pub last_out_at: Option<DateTime<Utc>>,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS people (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    name TEXT NOT NULL, handle TEXT, automated INTEGER NOT NULL DEFAULT 0,
    first_at TEXT NOT NULL, last_at TEXT NOT NULL,
    heard INTEGER NOT NULL DEFAULT 0, warmth INTEGER NOT NULL DEFAULT 0,
    gone INTEGER NOT NULL DEFAULT 0, direct TEXT
);
CREATE INDEX IF NOT EXISTS people_channel ON people (channel);
CREATE TABLE IF NOT EXISTS feelings (
    seq INTEGER PRIMARY KEY, toward TEXT NOT NULL, at TEXT NOT NULL,
    delta INTEGER NOT NULL, why TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS feelings_toward ON feelings (toward, seq);
CREATE TABLE IF NOT EXISTS days_heard (
    person TEXT NOT NULL, day TEXT NOT NULL, PRIMARY KEY (person, day)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS heard_in (
    place TEXT NOT NULL, person TEXT NOT NULL, last_seq INTEGER NOT NULL,
    PRIMARY KEY (place, person)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS heard_in_recent ON heard_in (place, last_seq);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        Event::Heard {
            channel,
            place,
            from,
            ..
        } => {
            let place_key = social::key(channel, &place.id);
            let person = social::key(channel, &from.id);
            let direct = (place.kind == PlaceKind::Direct).then_some(&place_key);
            tx.execute(
                "INSERT INTO people (key, channel, id, name, handle, automated, first_at, last_at, heard, direct)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, ?8)
                 ON CONFLICT(key) DO UPDATE SET name = ?4, handle = COALESCE(?5, handle),
                     automated = ?6, last_at = ?7, heard = heard + 1,
                     direct = COALESCE(?8, direct)",
                params![
                    person,
                    channel,
                    from.id,
                    from.name,
                    from.handle,
                    from.automated,
                    at,
                    direct
                ],
            )?;
            // The days she heard from someone, and the last time she heard
            // from them in each place, are kept as they happen, so that
            // reading either does not go through everything they ever said.
            tx.execute(
                "INSERT OR IGNORE INTO days_heard (person, day) VALUES (?1, substr(?2, 1, 10))",
                params![person, at],
            )?;
            tx.execute(
                "INSERT INTO heard_in (place, person, last_seq) VALUES (?1, ?2, ?3)
                 ON CONFLICT(place, person) DO UPDATE SET last_seq = MAX(last_seq, ?3)",
                params![place_key, person, entry.seq],
            )?;
        }
        // How she stands with an account, and whether she has left it, are
        // kept with the account, on its platform. Each shift is kept as well,
        // with what did it, so that what she last felt can be said.
        Event::Felt { toward, delta, why } => {
            tx.execute(
                "UPDATE people SET warmth = MAX(?3, MIN(?4, warmth + ?2)) WHERE key = ?1",
                params![toward, i64::from(*delta), WARMTH_MIN, WARMTH_MAX],
            )?;
            tx.execute(
                "INSERT INTO feelings (seq, toward, at, delta, why) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.seq, toward, at, i64::from(*delta), why],
            )?;
        }
        Event::Left { person, .. } => {
            tx.execute("UPDATE people SET gone = 1 WHERE key = ?1", [person])?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    pub fn person(&self, key: &str) -> Result<Option<Person>> {
        let base = self
            .conn
            .query_row(
                "SELECT key, channel, id, name, handle, automated, first_at, last_at, heard, warmth, gone, direct
                 FROM people WHERE key = ?1",
                [key],
                |r| {
                    Ok(Person {
                        key: r.get(0)?,
                        channel: r.get(1)?,
                        id: r.get(2)?,
                        name: r.get(3)?,
                        handle: r.get(4)?,
                        automated: r.get(5)?,
                        first_at: time_at(r, 6)?,
                        last_at: time_at(r, 7)?,
                        heard: r.get(8)?,
                        warmth: r.get(9)?,
                        gone: r.get(10)?,
                        direct: r.get(11)?,
                        active_days: 0,
                        unanswered_days: 0,
                        last_out_at: None,
                    })
                },
            )
            .optional()?;
        let Some(mut person) = base else {
            return Ok(None);
        };
        person.active_days = self.conn.query_row(
            "SELECT COUNT(*) FROM days_heard WHERE person = ?1",
            [key],
            |r| r.get(0),
        )?;
        if let Some(direct) = &person.direct {
            let (days, last): (i64, Option<String>) = self.conn.query_row(
                "SELECT COUNT(DISTINCT substr(m.at, 1, 10)), MAX(m.at) FROM messages m
                 JOIN places p ON p.key = m.place
                 WHERE m.place = ?1 AND m.dir = 'out' AND m.seq > p.last_in_seq",
                [direct],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            person.unanswered_days = days;
            person.last_out_at = match last {
                Some(at) => parse_stamp(&at).ok(),
                None => self
                    .conn
                    .query_row(
                        "SELECT last_out_at FROM places WHERE key = ?1",
                        [direct],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
                    .and_then(|at| parse_stamp(&at).ok()),
            };
        }
        Ok(Some(person))
    }

    /// The latest shifts in how she feels about the account `key` since
    /// `since`, at most `limit` of them, oldest first.
    pub fn feelings_toward(
        &self,
        key: &str,
        since: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Feeling>> {
        let mut stmt = self.conn.prepare(
            "SELECT at, delta, why FROM feelings WHERE toward = ?1 AND at >= ?2
             ORDER BY seq DESC LIMIT ?3",
        )?;
        let mut feelings = stmt
            .query_map(params![key, stamp(&since), as_i64(limit)], |r| {
                Ok(Feeling {
                    at: time_at(r, 0)?,
                    delta: r.get(1)?,
                    why: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        feelings.reverse();
        Ok(feelings)
    }

    /// The person on the other side of a private place.
    pub fn person_of_direct(&self, place: &str) -> Result<Option<Person>> {
        let key: Option<String> = self
            .conn
            .query_row(
                "SELECT key FROM people WHERE direct = ?1 LIMIT 1",
                [place],
                |r| r.get(0),
            )
            .optional()?;
        match key {
            Some(key) => self.person(&key),
            None => Ok(None),
        }
    }

    /// The people most recently heard in a place, most recent first.
    pub fn people_in(&self, place: &str, limit: usize) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(
            "SELECT person FROM heard_in WHERE place = ?1 ORDER BY last_seq DESC LIMIT ?2",
        )?;
        let keys = stmt
            .query_map(params![place, as_i64(limit)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    /// Everyone heard from since `since`, on every platform, most recent first.
    pub fn people_since(&self, since: DateTime<Utc>) -> Result<Vec<Person>> {
        // Only the messages since, found by when they came in. A platform
        // at a time, then by when each was last heard: those heard at the
        // same moment stay in that order.
        let mut stmt = self.conn.prepare(
            "SELECT person FROM messages WHERE dir = 'in' AND person IS NOT NULL AND at >= ?1
             GROUP BY person ORDER BY MIN(channel), MAX(seq) DESC",
        )?;
        let keys = stmt
            .query_map([stamp(&since)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut all = self.load_people(&keys)?;
        all.sort_by_key(|p| std::cmp::Reverse(p.last_at));
        Ok(all)
    }

    /// Everyone she knows, on every platform, most recent first.
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM people ORDER BY channel, last_at DESC")?;
        let keys = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut all = self.load_people(&keys)?;
        all.sort_by_key(|p| std::cmp::Reverse(p.last_at));
        Ok(all)
    }

    fn load_people(&self, keys: &[String]) -> Result<Vec<Person>> {
        let mut people = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(p) = self.person(key)? {
                people.push(p);
            }
        }
        Ok(people)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::super::{
        Memory,
        testing::{fold, heard, on, said, steps},
    };
    use crate::{
        journal::{Event, Journal},
        social::PlaceKind,
    };

    #[test]
    fn people_accumulate() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("g", PlaceKind::Group, "a", "2", "hello group", false),
            heard("g", PlaceKind::Group, "b", "3", "hey", false),
            Event::Felt {
                toward: "t:a".into(),
                delta: 2,
                why: "kind".into(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "designs buildings".into(),
                place: Some("t:a".into()),
            },
            said("a", "hello"),
        ]);
        let a = m.person("t:a").unwrap().unwrap();
        assert_eq!((a.heard, a.warmth, a.active_days), (2, 2, 1));
        assert_eq!(a.direct.as_deref(), Some("t:a"));
        assert_eq!(a.unanswered_days, 1);
        assert_eq!(m.person_of_direct("t:a").unwrap().unwrap().key, "t:a");
        assert_eq!(m.people_in("t:g", 5).unwrap().len(), 2);
        assert_eq!(m.notes("t:a", 5).unwrap()[0].text, "designs buildings");
        let p = m.place("t:g").unwrap().unwrap();
        assert_eq!((p.unread, p.unread_addressed), (2, 0));
    }

    #[test]
    fn what_she_feels_toward_an_account_stays_with_that_account() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            on("qq", heard("a", PlaceKind::Direct, "a", "2", "hi", true)),
            Event::Felt {
                toward: "qq:a".into(),
                delta: 3,
                why: "kind".into(),
            },
            Event::Left {
                person: "qq:a".into(),
                why: "unkind after all".into(),
            },
        ]);
        let (here, there) = (
            m.person("t:a").unwrap().unwrap(),
            m.person("qq:a").unwrap().unwrap(),
        );
        assert_eq!((here.warmth, here.gone), (0, false));
        assert_eq!((there.warmth, there.gone), (3, true));
        assert_eq!(there.channel, "qq");
        let since = Utc::now() - chrono::Duration::hours(1);
        assert_eq!(m.people_since(since).unwrap().len(), 2);
        // What she felt is kept with the account it was toward, and why.
        let felt = m.feelings_toward("qq:a", since, 5).unwrap();
        assert_eq!(
            felt.iter()
                .map(|f| (f.delta, f.why.as_str()))
                .collect::<Vec<_>>(),
            [(3, "kind")]
        );
        assert!(m.feelings_toward("t:a", since, 5).unwrap().is_empty());
        assert_eq!(m.people().unwrap().len(), 2);
    }

    #[test]
    fn who_she_heard_lately_costs_the_same_however_long_she_has_lived() {
        let cost = |n: i64| {
            let mut j = Journal::in_memory();
            let mut m = Memory::in_memory().unwrap();
            let now = Utc::now();
            let mut hear = |at, who: &str, id: i64| {
                let event = heard("g", PlaceKind::Group, who, &id.to_string(), "hi", false);
                m.apply(&j.append_at(at, event).unwrap()).unwrap();
            };
            // A month of others, then two people this last hour.
            for i in 0..n {
                hear(
                    now - Duration::days(30) + Duration::minutes(i),
                    &format!("old{i}"),
                    i,
                );
            }
            hear(now - Duration::minutes(10), "a", n);
            hear(now - Duration::minutes(5), "b", n + 1);
            let (people, steps) = steps(&mut m, |m| {
                m.people_since(now - Duration::hours(1)).unwrap()
            });
            let keys: Vec<String> = people.into_iter().map(|p| p.key).collect();
            assert_eq!(keys, ["t:b", "t:a"]);
            steps
        };
        let (short, long) = (cost(20), cost(1000));
        assert!(
            long < short * 2,
            "{short} steps after 20 others, {long} after 1000"
        );
    }

    #[test]
    fn reading_someone_costs_the_same_however_much_they_have_said() {
        let cost = |n: usize| {
            let mut events = Vec::new();
            for i in 0..n {
                let who = if i % 2 == 0 { "a" } else { "b" };
                let id = i.to_string();
                events.push(heard("g", PlaceKind::Group, who, &id, "hello", false));
                events.push(heard("a", PlaceKind::Direct, "a", &id, "you there", true));
                events.push(said("a", "here"));
            }
            let (_, mut m) = fold(events);
            let (people, steps) = steps(&mut m, |m| {
                let a = m.person("t:a").unwrap().unwrap();
                (a.active_days, m.people_in("t:g", 6).unwrap().len())
            });
            assert_eq!(people, (1, 2));
            steps
        };
        let (short, long) = (cost(20), cost(400));
        assert!(
            long < short * 2,
            "{short} steps after 20 messages each, {long} after 400"
        );
    }
}
