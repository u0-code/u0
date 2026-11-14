//! What was said in each place, by her and to her.
//!
//! Every message she heard or sent, word for word, in the order the journal
//! has them. This is a platform's record: each line carries the platform it
//! was said on, and a conversation is shown its own lines and no others.

use chrono::{DateTime, Duration, Utc};
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
    /// When it was a directive from an operator of hers: the directive,
    /// without its prefix. `text` is still the message as it was written.
    pub directive: Option<String>,
}

/// How much of what was said lately in a place was hers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Share {
    /// Messages she sent, among them.
    pub hers: i64,
    /// Messages looked at: the last few, or every one if there are fewer.
    pub of: i64,
}

/// The newest message in a place, when it was written to someone other
/// than her: an answer to another person's message, or one that names
/// another person with an `@`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Aside {
    /// Who wrote it.
    pub from: String,
    /// Who it was to, when that is known.
    pub to: Option<String>,
}

/// The last time she spoke up in a place without anyone having turned to
/// her, and whether anyone turned to her soon after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unprompted {
    pub at: DateTime<Utc>,
    /// What she said, a bubble at a time.
    pub text: String,
    /// Someone answered her or addressed her within the time asked about.
    pub answered: bool,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, channel TEXT NOT NULL,
    place TEXT NOT NULL, dir TEXT NOT NULL,
    person TEXT, name TEXT NOT NULL, platform_id TEXT, reply_to TEXT,
    addressed INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL,
    directive TEXT
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
            ..
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
        // A directive is marked on the message it came in.
        Event::Directed { message, text, .. } => {
            tx.execute(
                "UPDATE messages SET directive = ?1 WHERE seq = ?2 AND dir = 'in'",
                params![text, message],
            )?;
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

    /// How many of the last `last` messages in a place are hers.
    pub fn share(&self, place: &str, last: usize) -> Result<Share> {
        let (hers, of) = self.conn.query_row(
            "SELECT COALESCE(SUM(dir = 'out'), 0), COUNT(*) FROM
             (SELECT dir FROM messages WHERE place = ?1 ORDER BY seq DESC LIMIT ?2)",
            params![place, as_i64(last)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(Share { hers, of })
    }

    /// The newest message in a place, if it is someone else's and was
    /// written to a third person rather than to her or to everyone.
    pub fn aside(&self, place: &str) -> Result<Option<Aside>> {
        let Some(newest) = self.thread(place, 1)?.pop() else {
            return Ok(None);
        };
        if !newest.incoming || newest.addressed {
            return Ok(None);
        }
        let answered: Option<String> = match &newest.reply_to {
            Some(id) => self
                .conn
                .query_row(
                    "SELECT name FROM messages WHERE place = ?1 AND platform_id = ?2",
                    params![place, id],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let to = answered.or_else(|| mentioned(&newest.text).map(str::to_owned));
        if to.is_none() && newest.reply_to.is_none() {
            return Ok(None);
        }
        Ok(Some(Aside {
            from: newest.name,
            to,
        }))
    }

    /// The last time she spoke up in a place unprompted, among its last
    /// `window` messages, and whether anyone answered her or addressed her
    /// within `within` of it. Unprompted is when nothing addressed to her
    /// came in between what she said before and this.
    pub fn last_unprompted(
        &self,
        place: &str,
        window: usize,
        within: Duration,
    ) -> Result<Option<Unprompted>> {
        Ok(last_unprompted(&self.thread(place, window)?, within))
    }

    /// The directives in a place after entry `after`, oldest first.
    pub fn directives(&self, place: &str, after: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT directive FROM messages
             WHERE place = ?1 AND seq > ?2 AND dir = 'in' AND directive IS NOT NULL ORDER BY seq",
        )?;
        Ok(stmt
            .query_map(params![place, after], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
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

/// The person a message names with an `@`, if it names one: the word that
/// follows it. Channels write a mention that way, whatever the platform
/// calls it; an address like `a@b` is not one.
fn mentioned(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find_map(|word| word.strip_prefix('@'))
        .map(|name| name.trim_end_matches(|c: char| c.is_ascii_punctuation()))
        .filter(|name| !name.is_empty())
}

/// Her last turn in `lines` that nothing addressed to her had led to, and
/// whether anything addressed to her came within `within` of its end. A
/// turn is her lines one after another, with nothing from anyone between:
/// the bubbles of one thing said.
fn last_unprompted(lines: &[Line], within: Duration) -> Option<Unprompted> {
    let mut found: Option<(usize, usize)> = None;
    let mut turned_to = false;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].incoming {
            turned_to |= lines[i].addressed;
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !lines[i].incoming {
            i += 1;
        }
        if !turned_to {
            found = Some((start, i));
        }
        turned_to = false;
    }
    let (start, end) = found?;
    let said = &lines[start..end];
    let last = said.last()?.at;
    Some(Unprompted {
        at: said[0].at,
        text: said
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" / "),
        answered: lines[end..]
            .iter()
            .any(|l| l.incoming && l.addressed && l.at - last <= within),
    })
}

const LINE_COLUMNS: &str =
    "seq, at, dir, person, name, platform_id, reply_to, addressed, text, directive";
const LINE_SELECT: &str = "SELECT seq, at, dir, person, name, platform_id, reply_to, addressed, text, directive FROM messages";

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
        directive: r.get(o + 9)?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::super::testing::{fold, heard, on, said};
    use super::{Line, described, last_unprompted, mentioned};
    use crate::{journal::Event, social::PlaceKind};

    fn chatter(id: &str, text: &str) -> Event {
        heard("g", PlaceKind::Group, "mei", id, text, false)
    }

    /// `chatter`, as an answer to message `to`.
    fn answer(id: &str, to: &str, text: &str) -> Event {
        let mut event = chatter(id, text);
        if let Event::Heard { reply_to, .. } = &mut event {
            *reply_to = Some(to.into());
        }
        event
    }

    #[test]
    fn her_share_is_counted_over_the_last_messages_only() {
        let (_, m) = fold(vec![
            said("g", "an old line of hers"),
            chatter("1", "one"),
            chatter("2", "two"),
            said("g", "a new line of hers"),
            chatter("3", "three"),
        ]);
        let share = m.share("t:g", 4).unwrap();
        assert_eq!((share.hers, share.of), (1, 4));
        let share = m.share("t:g", 50).unwrap();
        assert_eq!((share.hers, share.of), (2, 5));
        let none = m.share("t:nowhere", 50).unwrap();
        assert_eq!((none.hers, none.of), (0, 0));
    }

    #[test]
    fn a_message_to_someone_else_is_told_from_one_to_everyone() {
        let ladder = || heard("g", PlaceKind::Group, "lao", "1", "who has a ladder", false);
        let (_, m) = fold(vec![ladder()]);
        assert_eq!(m.aside("t:g").unwrap(), None);
        let (_, m) = fold(vec![ladder(), answer("2", "1", "I do")]);
        let aside = m.aside("t:g").unwrap().unwrap();
        assert_eq!(
            (aside.from.as_str(), aside.to.as_deref()),
            ("MEI", Some("LAO"))
        );
        // An answer to something she never heard is still someone else's.
        let (_, m) = fold(vec![answer("2", "gone", "I do")]);
        assert_eq!(m.aside("t:g").unwrap().unwrap().to, None);
        let (_, m) = fold(vec![chatter("1", "@老王 吃了吗")]);
        assert_eq!(m.aside("t:g").unwrap().unwrap().to.as_deref(), Some("老王"));
        // Her own line, or one meant for her, is nobody else's.
        let (_, m) = fold(vec![chatter("1", "@lao hi"), said("g", "hi all")]);
        assert_eq!(m.aside("t:g").unwrap(), None);
        let (_, m) = fold(vec![heard(
            "g",
            PlaceKind::Group,
            "mei",
            "1",
            "@weiwei hi",
            true,
        )]);
        assert_eq!(m.aside("t:g").unwrap(), None);
    }

    #[test]
    fn a_mention_is_an_at_and_the_word_after_it() {
        assert_eq!(mentioned("@Mei, look"), Some("Mei"));
        assert_eq!(mentioned("look @老王"), Some("老王"));
        assert_eq!(mentioned("write to a@b.com"), None);
        assert_eq!(mentioned("@ nobody"), None);
    }

    fn line(minute: i64, incoming: bool, addressed: bool, text: &str) -> Line {
        let start = Utc::now() - Duration::hours(1);
        Line {
            seq: minute,
            at: start + Duration::minutes(minute),
            incoming,
            person: incoming.then(|| "t:mei".to_owned()),
            name: if incoming { "Mei" } else { "you" }.to_owned(),
            platform_id: None,
            reply_to: None,
            addressed,
            text: text.to_owned(),
            directive: None,
        }
    }

    #[test]
    fn her_last_unprompted_turn_is_the_last_one_nothing_to_her_led_to() {
        let lines = [
            line(0, true, false, "chatter"),
            line(1, false, false, "unasked"),
            line(2, false, false, "and more"),
            line(4, true, true, "@weiwei what do you think"),
            line(5, false, false, "an answer, which was asked for"),
            line(6, true, false, "chatter"),
        ];
        let found = last_unprompted(&lines, Duration::minutes(10)).unwrap();
        assert_eq!(found.text, "unasked / and more");
        // Someone turned to her two minutes after she finished.
        assert!(found.answered);
        assert!(
            !last_unprompted(&lines, Duration::minutes(1))
                .unwrap()
                .answered
        );
        // Every turn she took after that was asked for.
        assert_eq!(last_unprompted(&lines[3..], Duration::minutes(10)), None);
    }

    #[test]
    fn an_unprompted_line_nobody_turned_to_is_unanswered() {
        let lines = [
            line(0, true, false, "chatter"),
            line(1, false, false, "unasked"),
            line(2, true, false, "more chatter"),
            line(30, true, true, "@weiwei much later"),
        ];
        let found = last_unprompted(&lines, Duration::minutes(10)).unwrap();
        assert_eq!(found.text, "unasked");
        assert!(!found.answered);
    }

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
