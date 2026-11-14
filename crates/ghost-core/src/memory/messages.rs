//! What was said in each place, by her and to her.
//!
//! Every message she heard or sent, word for word, in the order the journal
//! has them. This is a platform's record: each line carries the platform it
//! was said on, and a conversation is shown its own lines and no others.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event, stamp},
    social,
};

/// One message in a place, hers or someone else's.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub incoming: bool,
    pub person: Option<String>,
    pub name: String,
    pub platform_id: Option<String>,
    pub reply_to: Option<String>,
    pub addressed: bool,
    pub text: String,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, channel TEXT NOT NULL,
    place TEXT NOT NULL, dir TEXT NOT NULL,
    person TEXT, name TEXT NOT NULL, platform_id TEXT, reply_to TEXT,
    addressed INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_channel ON messages (channel, seq);
CREATE INDEX IF NOT EXISTS messages_place ON messages (place, seq);
CREATE INDEX IF NOT EXISTS messages_person ON messages (person, seq);
CREATE INDEX IF NOT EXISTS messages_platform ON messages (place, platform_id);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let seq = entry.seq;
    match &entry.event {
        Event::Heard {
            channel,
            place,
            from,
            id,
            text,
            reply_to,
            addressed,
            ..
        } => {
            tx.execute(
                "INSERT INTO messages (seq, at, channel, place, dir, person, name, platform_id, reply_to, addressed, text)
                 VALUES (?1, ?2, ?3, ?4, 'in', ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    seq,
                    at,
                    channel,
                    social::key(channel, &place.id),
                    social::key(channel, &from.id),
                    from.name,
                    id,
                    reply_to,
                    addressed,
                    text
                ],
            )?;
        }
        Event::Said {
            channel,
            place,
            id,
            text,
            reply_to,
        } => {
            tx.execute(
                "INSERT INTO messages (seq, at, channel, place, dir, name, platform_id, reply_to, text)
                 VALUES (?1, ?2, ?3, ?4, 'out', 'you', ?5, ?6, ?7)",
                params![
                    seq,
                    at,
                    channel,
                    social::key(channel, place),
                    id,
                    reply_to,
                    text
                ],
            )?;
        }
        // What she made of an image goes where the image was, so that the
        // line reads as she saw it from then on.
        Event::Viewed {
            message,
            image,
            what,
            ..
        } => {
            let text: Option<String> = tx
                .query_row("SELECT text FROM messages WHERE seq = ?1", [message], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(seen) = text.and_then(|t| described(&t, *image, what)) {
                tx.execute(
                    "UPDATE messages SET text = ?1 WHERE seq = ?2",
                    params![seen, message],
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// `text` with its `index`th image placeholder replaced by `[image: what]`.
/// A placeholder is `[image]`, or one already described; the description is
/// kept on one line and free of brackets, so it can be found again.
fn described(text: &str, index: u32, what: &str) -> Option<String> {
    let what = what
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('[', "(")
        .replace(']', ")");
    if what.is_empty() {
        return None;
    }
    let mut seen = 0;
    let mut from = 0;
    while let Some(at) = text[from..].find("[image") {
        let start = from + at;
        let rest = &text[start + "[image".len()..];
        let len = if rest.starts_with(']') {
            Some("[image]".len())
        } else if rest.starts_with(": ") {
            rest.find(']').map(|end| "[image".len() + end + 1)
        } else {
            None
        };
        let Some(len) = len else {
            from = start + "[image".len();
            continue;
        };
        if seen == index {
            return Some(format!(
                "{}[image: {what}]{}",
                &text[..start],
                &text[start + len..]
            ));
        }
        seen += 1;
        from = start + len;
    }
    None
}

impl Memory {
    /// The last `limit` messages in a place, oldest first.
    pub fn thread(&self, place: &str, limit: usize) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 ORDER BY seq DESC LIMIT ?2"
        ))?;
        let mut lines = stmt
            .query_map(params![place, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// The messages in a place after entry `after` and up to `upto`, oldest
    /// first, and no more than `limit` of them.
    pub fn thread_between(
        &self,
        place: &str,
        after: i64,
        upto: i64,
        limit: usize,
    ) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 AND seq > ?2 AND seq <= ?3 ORDER BY seq LIMIT ?4"
        ))?;
        Ok(stmt
            .query_map(params![place, after, upto, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The messages on a platform from entry `from` to entry `upto`, each
    /// with the place it was in, oldest first. When there are more than
    /// `limit`, the newest are kept.
    pub fn lines_on(
        &self,
        channel: &str,
        from: i64,
        upto: i64,
        limit: usize,
    ) -> Result<Vec<(String, Line)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT place, {LINE_COLUMNS} FROM messages
             WHERE channel = ?1 AND seq >= ?2 AND seq <= ?3 ORDER BY seq DESC LIMIT ?4"
        ))?;
        let mut lines = stmt
            .query_map(params![channel, from, upto, as_i64(limit)], |r| {
                Ok((r.get(0)?, line_at(r, 1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// The messages in a place around entry `seq`: up to `n` before it, the
    /// one at it, and up to `n` after, oldest first.
    pub fn around(&self, place: &str, seq: i64, n: usize) -> Result<Vec<Line>> {
        let mut lines = self.thread_before(place, seq, n + 1)?;
        lines.extend(self.thread_between(place, seq, i64::MAX, n)?);
        Ok(lines)
    }

    /// The last `limit` messages in a place up to and including entry
    /// `upto`, oldest first.
    pub fn thread_before(&self, place: &str, upto: i64, limit: usize) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 AND seq <= ?2 ORDER BY seq DESC LIMIT ?3"
        ))?;
        let mut lines = stmt
            .query_map(params![place, upto, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// How many messages a place holds after entry `after`.
    pub fn lines_after(&self, place: &str, after: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE place = ?1 AND seq > ?2",
            params![place, after],
            |r| r.get(0),
        )?)
    }

    /// The message with this journal seq, wherever it was.
    pub fn line(&self, seq: i64) -> Result<Option<(String, Line)>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT place, {LINE_COLUMNS} FROM messages WHERE seq = ?1"),
                [seq],
                |r| Ok((r.get(0)?, line_at(r, 1)?)),
            )
            .optional()?)
    }

    /// Whether a message with this channel id has already reached her here.
    pub fn knows_message(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 LIMIT 1",
            place,
            platform_id,
        )
    }

    /// Whether the message with this channel id in this place is one she sent.
    pub fn is_hers(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 AND dir = 'out' LIMIT 1",
            place,
            platform_id,
        )
    }

    /// Whether she has already answered this message.
    pub fn replied_to(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM messages WHERE place = ?1 AND reply_to = ?2 AND dir = 'out' LIMIT 1",
            place,
            platform_id,
        )
    }

    /// How many messages she has sent in a place since `since`.
    pub fn said_since(&self, place: &str, since: DateTime<Utc>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE place = ?1 AND dir = 'out' AND at >= ?2",
            params![place, stamp(&since)],
            |r| r.get(0),
        )?)
    }

    /// Whether a query about a place and a message's id finds a row.
    fn exists(&self, sql: &str, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(sql, params![place, platform_id], |_| Ok(()))
            .optional()?
            .is_some())
    }
}

const LINE_COLUMNS: &str = "seq, at, dir, person, name, platform_id, reply_to, addressed, text";
const LINE_SELECT: &str =
    "SELECT seq, at, dir, person, name, platform_id, reply_to, addressed, text FROM messages";

fn row_to_line(r: &rusqlite::Row<'_>) -> rusqlite::Result<Line> {
    line_at(r, 0)
}

fn line_at(r: &rusqlite::Row<'_>, o: usize) -> rusqlite::Result<Line> {
    let dir: String = r.get(o + 2)?;
    Ok(Line {
        seq: r.get(o)?,
        at: time_at(r, o + 1)?,
        incoming: dir == "in",
        person: r.get(o + 3)?,
        name: r.get(o + 4)?,
        platform_id: r.get(o + 5)?,
        reply_to: r.get(o + 6)?,
        addressed: r.get(o + 7)?,
        text: r.get(o + 8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{fold, heard, on, said};
    use super::described;
    use crate::{journal::Event, social::PlaceKind};

    fn viewed(message: i64, image: u32, what: &str) -> Event {
        Event::Viewed {
            message,
            image,
            what: what.into(),
            file: None,
        }
    }

    #[test]
    fn what_she_saw_in_an_image_replaces_its_placeholder() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "look [image] and [image]",
                true,
            ),
            // The second image is described first, then the first.
            viewed(1, 1, "A cat asleep\non a [keyboard]."),
            viewed(1, 0, "A bowl of noodles."),
            // An image that is not there changes nothing.
            viewed(1, 5, "Nothing."),
        ]);
        assert_eq!(
            m.thread("t:a", 5).unwrap()[0].text,
            "look [image: A bowl of noodles.] and [image: A cat asleep on a (keyboard).]"
        );
    }

    #[test]
    fn placeholders_are_counted_whether_or_not_they_are_described() {
        assert_eq!(
            described("[image: one] [image]", 1, "two").as_deref(),
            Some("[image: one] [image: two]")
        );
        assert_eq!(
            described("[image: old]", 0, "new").as_deref(),
            Some("[image: new]")
        );
        assert_eq!(described("[images] [imagex]", 0, "x"), None);
        assert_eq!(described("[image]", 0, "   "), None);
    }

    #[test]
    fn what_was_said_on_a_platform_is_only_found_from_that_platform() {
        let (_, m) = fold(vec![
            on(
                "telegram",
                heard(
                    "a",
                    PlaceKind::Direct,
                    "a",
                    "1",
                    "a secret on telegram",
                    true,
                ),
            ),
            on(
                "qq",
                heard("a", PlaceKind::Group, "b", "2", "chatter on qq", false),
            ),
            said("a", "and one on t"),
        ]);
        // The same id on two platforms is two places.
        assert_eq!(
            m.thread("telegram:a", 5).unwrap()[0].text,
            "a secret on telegram"
        );
        assert_eq!(m.thread("qq:a", 5).unwrap()[0].text, "chatter on qq");
        assert!(m.thread("nowhere:a", 5).unwrap().is_empty());
        assert!(m.place("nowhere:a").unwrap().is_none());
        // A platform's lines are that platform's alone.
        let on_qq: Vec<String> = m
            .lines_on("qq", 0, i64::MAX, 10)
            .unwrap()
            .into_iter()
            .map(|(place, _)| place)
            .collect();
        assert_eq!(on_qq, ["qq:a"]);
        assert!(m.lines_on("x", 0, i64::MAX, 10).unwrap().is_empty());
        // A line can still be found by its seq, wherever it was said.
        assert_eq!(m.line(1).unwrap().unwrap().0, "telegram:a");
        assert_eq!(m.line(3).unwrap().unwrap().0, "t:a");
    }

    #[test]
    fn lines_on_a_platform_keep_the_newest_when_there_are_too_many() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "one", true),
            heard("g", PlaceKind::Group, "b", "2", "two", false),
            said("a", "three"),
        ]);
        let lines: Vec<(String, String)> = m
            .lines_on("t", 1, 3, 2)
            .unwrap()
            .into_iter()
            .map(|(place, l)| (place, l.text))
            .collect();
        assert_eq!(
            lines,
            [("t:g".into(), "two".into()), ("t:a".into(), "three".into())]
        );
        let near: Vec<String> = m
            .around("t:a", 1, 1)
            .unwrap()
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(near, ["one", "three"]);
    }
}
