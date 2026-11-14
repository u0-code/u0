//! The places she has been in, and how far she has read in each.
//!
//! A place is somewhere a conversation happens on one platform: a private
//! conversation, a room, a public thread. It is known from the first message
//! heard or said there, and it keeps the newest message each way and the
//! newest she has read, so that what is unread is whatever came in after.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Line, Memory, as_i64, channel_of, opt_time_at};
use crate::{
    Result,
    journal::{Entry, Event, stamp},
    social::{self, PlaceKind},
};

/// A place and how far she has read in it.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceState {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub kind: PlaceKind,
    pub name: Option<String>,
    pub read_upto: i64,
    pub last_in_seq: i64,
    pub last_in_at: Option<DateTime<Utc>>,
    pub last_out_seq: i64,
    pub last_out_at: Option<DateTime<Utc>>,
    pub unread: i64,
    pub unread_addressed: i64,
    /// Unread messages that are directives from an operator of hers.
    pub unread_directed: i64,
}

/// A place other than the one she is looking at, and what went on there.
#[derive(Debug, Clone, Serialize)]
pub struct Elsewhere {
    pub place: PlaceState,
    /// Messages that reached her there in the window.
    pub heard: i64,
    /// Messages she sent there in the window.
    pub said: i64,
    /// The newest line there.
    pub last: Line,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS places (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    kind TEXT NOT NULL, name TEXT,
    read_upto INTEGER NOT NULL DEFAULT 0,
    last_in_seq INTEGER NOT NULL DEFAULT 0, last_in_at TEXT,
    last_out_seq INTEGER NOT NULL DEFAULT 0, last_out_at TEXT
);
CREATE INDEX IF NOT EXISTS places_channel ON places (channel);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let seq = entry.seq;
    match &entry.event {
        Event::Heard { channel, place, .. } => {
            tx.execute(
                "INSERT INTO places (key, channel, id, kind, name, last_in_seq, last_in_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(key) DO UPDATE SET kind = ?4, name = COALESCE(?5, name),
                     last_in_seq = ?6, last_in_at = ?7",
                params![
                    social::key(channel, &place.id),
                    channel,
                    place.id,
                    place.kind.as_str(),
                    place.name,
                    seq,
                    at
                ],
            )?;
        }
        Event::Said { channel, place, .. } => {
            tx.execute(
                "INSERT INTO places (key, channel, id, kind, last_out_seq, last_out_at)
                 VALUES (?1, ?2, ?3, 'public', ?4, ?5)
                 ON CONFLICT(key) DO UPDATE SET last_out_seq = ?4, last_out_at = ?5",
                params![social::key(channel, place), channel, place, seq, at],
            )?;
        }
        Event::Read {
            channel,
            place,
            upto,
        } => {
            tx.execute(
                "UPDATE places SET read_upto = MAX(read_upto, ?2) WHERE key = ?1",
                params![social::key(channel, place), upto],
            )?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    pub fn place(&self, key: &str) -> Result<Option<PlaceState>> {
        Ok(self
            .conn
            .query_row(
                &format!("{PLACE_SELECT} WHERE p.key = ?1"),
                [key],
                row_to_place,
            )
            .optional()?)
    }

    /// Every place on every platform, most recently active first.
    pub fn places(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self.conn.prepare(&format!(
            "{PLACE_SELECT} ORDER BY MAX(p.last_in_seq, p.last_out_seq) DESC"
        ))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every place on every platform with something she has not looked at,
    /// a platform at a time, each in the order she came to them.
    pub fn places_unread(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self.conn.prepare(&format!(
            "{PLACE_SELECT} WHERE p.last_in_seq > p.read_upto ORDER BY p.channel, p.rowid"
        ))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|p| p.unread > 0)
            .collect())
    }

    /// Places on the same platform as `except`, other than it, where
    /// something was said since `since`, most recent first.
    pub fn elsewhere(
        &self,
        since: DateTime<Utc>,
        except: &str,
        limit: usize,
    ) -> Result<Vec<Elsewhere>> {
        let Some(channel) = channel_of(except) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT place, SUM(dir = 'in'), SUM(dir = 'out'), MAX(seq) FROM messages
             WHERE channel = ?1 AND at >= ?2 AND place != ?3
             GROUP BY place ORDER BY MAX(seq) DESC LIMIT ?4",
        )?;
        let rows = stmt
            .query_map(
                params![channel, stamp(&since), except, as_i64(limit)],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (key, heard, said, last) in rows {
            let (Some(place), Some((_, last))) = (self.place(&key)?, self.line(last)?) else {
                continue;
            };
            out.push(Elsewhere {
                place,
                heard,
                said,
                last,
            });
        }
        Ok(out)
    }
}

const PLACE_SELECT: &str = "
SELECT p.key, p.channel, p.id, p.kind, p.name, p.read_upto,
       p.last_in_seq, p.last_in_at, p.last_out_seq, p.last_out_at,
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto),
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto AND m.addressed = 1),
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto
           AND m.directive IS NOT NULL)
FROM places p";

fn row_to_place(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlaceState> {
    let kind: String = r.get(3)?;
    Ok(PlaceState {
        key: r.get(0)?,
        channel: r.get(1)?,
        id: r.get(2)?,
        kind: PlaceKind::parse(&kind),
        name: r.get(4)?,
        read_upto: r.get(5)?,
        last_in_seq: r.get(6)?,
        last_in_at: opt_time_at(r, 7)?,
        last_out_seq: r.get(8)?,
        last_out_at: opt_time_at(r, 9)?,
        unread: r.get(10)?,
        unread_addressed: r.get(11)?,
        unread_directed: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::testing::{fold, heard, on, said};
    use crate::{journal::Event, social::PlaceKind};

    #[test]
    fn unread_follows_read_markers() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("a", PlaceKind::Direct, "a", "2", "there?", true),
            Event::Read {
                channel: "t".into(),
                place: "a".into(),
                upto: 2,
            },
            said("a", "yes"),
            heard("a", PlaceKind::Direct, "a", "3", "good", true),
        ]);
        let p = m.place("t:a").unwrap().unwrap();
        assert_eq!((p.unread, p.read_upto, p.last_out_seq), (1, 2, 4));
        assert_eq!(m.places_unread().unwrap().len(), 1);
        assert!(m.knows_message("t:a", "2").unwrap());
        assert!(!m.knows_message("t:a", "9").unwrap());
        let thread = m.thread("t:a", 10).unwrap();
        assert_eq!(thread.len(), 4);
        assert!(!thread[2].incoming);
    }

    #[test]
    fn a_directive_is_marked_on_its_line_and_waits_until_she_reads_it() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("a", PlaceKind::Direct, "a", "2", "fox: say hi", true),
            Event::Directed {
                message: 2,
                from: "t:a".into(),
                text: "say hi".into(),
            },
        ]);
        let p = m.place("t:a").unwrap().unwrap();
        assert_eq!((p.unread, p.unread_directed), (2, 1));
        assert_eq!(m.directives("t:a", 0).unwrap(), ["say hi"]);
        let thread = m.thread("t:a", 5).unwrap();
        assert_eq!(thread[0].directive, None);
        assert_eq!(thread[1].text, "fox: say hi");
        assert_eq!(thread[1].directive.as_deref(), Some("say hi"));

        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "fox: say hi", true),
            Event::Directed {
                message: 1,
                from: "t:a".into(),
                text: "say hi".into(),
            },
            Event::Read {
                channel: "t".into(),
                place: "a".into(),
                upto: 1,
            },
        ]);
        assert_eq!(m.place("t:a").unwrap().unwrap().unread_directed, 0);
        assert!(m.directives("t:a", 1).unwrap().is_empty());
    }

    #[test]
    fn elsewhere_is_everywhere_but_here() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "are you around", true),
            heard(
                "g",
                PlaceKind::Group,
                "b",
                "2",
                "anyone seen the new line 30",
                false,
            ),
            said("g", "not yet"),
        ]);
        let since = Utc::now() - chrono::Duration::hours(1);
        let other = m.elsewhere(since, "t:a", 5).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].place.key, "t:g");
        assert_eq!((other[0].heard, other[0].said), (1, 1));
        assert_eq!(other[0].last.text, "not yet");
    }

    #[test]
    fn elsewhere_stays_on_the_platform_she_is_looking_from() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "are you around", true),
            on(
                "qq",
                heard("b", PlaceKind::Group, "b", "2", "busy on qq", false),
            ),
        ]);
        let since = Utc::now() - chrono::Duration::hours(1);
        let from_t = m.elsewhere(since, "t:x", 5).unwrap();
        assert_eq!(from_t.len(), 1);
        assert_eq!(from_t[0].place.key, "t:a");
        let from_qq = m.elsewhere(since, "qq:x", 5).unwrap();
        assert_eq!(from_qq.len(), 1);
        assert_eq!(from_qq[0].place.key, "qq:b");
        assert!(m.elsewhere(since, "nowhere", 5).unwrap().is_empty());
        // Every place is still listed together.
        assert_eq!(m.places().unwrap().len(), 2);
        assert_eq!(m.places_unread().unwrap().len(), 2);
    }
}
