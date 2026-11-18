//! Remembering what is no longer in front of her.
//!
//! A full-text index of what she heard, said, posted and saw, what she noted
//! and lived and reflected on, what she found out and took on. Searching it
//! from a conversation finds her memory, whole, and what was said on that
//! conversation's platform, and nothing said on any other: a line heard on
//! one platform carries that platform; everything else carries none.
//!
//! An entry makes at most one row, kept under the entry's seq as its rowid,
//! so that a row can be taken out again when what it stands for is no
//! longer so: a note on something she kept, once she has replaced it or let
//! the thing go.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params, types::Value};
use serde::Serialize;

use super::{Memory, as_i64};
use crate::{
    Result,
    journal::{Entry, Event, parse_stamp},
    social,
};

/// Something found by searching her memory.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub seq: i64,
    pub at: Option<DateTime<Utc>>,
    pub kind: String,
    pub text: String,
    /// The place it was said or learned in, when it belongs to one.
    pub place: Option<String>,
}

pub(super) const SCHEMA: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS recall USING fts5(
    text, kind UNINDEXED, seq UNINDEXED, at UNINDEXED, place UNINDEXED, channel UNINDEXED,
    tokenize = 'trigram'
);
";

/// What of an entry is kept to be found again.
struct Kept<'a> {
    kind: &'static str,
    text: String,
    /// The place it was said or learned in.
    place: Option<String>,
    /// The platform whose record it is; `None` for her memory, whole.
    channel: Option<&'a str>,
}

/// What of an event is kept to be found again, if anything. Every event is
/// named here, so a new one cannot be added without deciding.
fn kept(event: &Event) -> Option<Kept<'_>> {
    let memory = |kind, text: &str, place: Option<&String>| Kept {
        kind,
        text: text.to_owned(),
        place: place.cloned(),
        channel: None,
    };
    Some(match event {
        Event::Heard {
            channel,
            place,
            from,
            text,
            ..
        } => Kept {
            kind: "heard",
            text: format!("{}: {text}", from.name),
            place: Some(social::key(channel, &place.id)),
            channel: Some(channel),
        },
        Event::Said {
            channel,
            place,
            text,
            ..
        } => Kept {
            kind: "said",
            text: format!("you: {text}"),
            place: Some(social::key(channel, place)),
            channel: Some(channel),
        },
        Event::Posted { channel, text, .. } => Kept {
            kind: "posted",
            text: text.clone(),
            place: None,
            channel: Some(channel),
        },
        Event::Saw { channel, text, .. } => Kept {
            kind: "saw",
            text: text.clone(),
            place: None,
            channel: Some(channel),
        },
        Event::Noted { text, place, .. } => memory("noted", text, place.as_ref()),
        Event::Canon { fact, .. } => memory("canon", fact, None),
        Event::Lived { text, .. } => memory("lived", text, None),
        Event::Reflected { text, .. } => memory("reflected", text, None),
        Event::Undertook { brief, place, .. } => memory("undertook", brief, place.as_ref()),
        Event::Added { text, .. } => memory("added", text, None),
        Event::Concluded { summary, .. } => memory("concluded", summary, None),
        Event::Reported { text, .. } => memory("reported", text, None),
        Event::Pursued { what, .. } if !what.trim().is_empty() => memory("pursued", what, None),
        Event::Practised { lesson, .. } => memory("practised", lesson, None),
        Event::Learned { lesson, .. } => memory("learned", lesson, None),
        // Her own: found from anywhere, by her note, while she holds it.
        Event::Kept { note, .. } => memory("kept", note, None),
        // Kept without a place: it came from the web, not from anyone she
        // was talking with, so it is nobody's secret.
        Event::LookedUp { query, found, .. } => Kept {
            kind: "looked_up",
            text: format!("you looked up \"{query}\": {found}"),
            place: None,
            channel: None,
        },
        // Like what she looked up: from the web, and nobody's secret.
        Event::Opened { url, text, .. } => Kept {
            kind: "opened",
            text: format!("you opened {url}: {text}"),
            place: None,
            channel: None,
        },
        // What she can do is hers, and worth finding when she wonders
        // whether she has something for a job.
        Event::Installed { name, tools, .. } => Kept {
            kind: "installed",
            text: format!("you installed {name}, with the tools {}", tools.join(", ")),
            place: None,
            channel: None,
        },
        Event::Uninstalled { name, why } => Kept {
            kind: "uninstalled",
            text: format!("you no longer have {name}: {why}"),
            place: None,
            channel: None,
        },
        // Like a server of tools: what she knows how to do is hers.
        Event::SkillInstalled {
            name, description, ..
        } => Kept {
            kind: "skill_installed",
            text: format!("you installed the skill {name}: {description}"),
            place: None,
            channel: None,
        },
        Event::SkillUninstalled { name, why } => Kept {
            kind: "skill_uninstalled",
            text: format!("you no longer have the skill {name}: {why}"),
            place: None,
            channel: None,
        },
        // Found through the message it belongs to; see `fold`.
        Event::Viewed { .. }
        // Kept with who stopped her, and the work, by name; see `fold`.
        | Event::Stopped { .. }
        // Takes what was said out; see `fold`.
        | Event::Rewound { .. }
        // The message it came in is already kept, word for word.
        | Event::Directed { .. }
        // Being asked to be quiet is in what was said; agreeing to it is
        // how she holds herself, not something to remember.
        | Event::Quieted { .. }
        // Takes a note out; see `fold`.
        | Event::LetGo { .. }
        | Event::Pursued { .. }
        | Event::Born { .. }
        | Event::Described { .. }
        | Event::Dormant { .. }
        | Event::Woke
        | Event::Slept
        | Event::Planned { .. }
        | Event::Read { .. }
        | Event::Reacted { .. }
        | Event::Decided { .. }
        | Event::Felt { .. }
        | Event::Left { .. }
        | Event::Intended { .. }
        | Event::Settled { .. }
        | Event::Worked { .. }
        | Event::Asked { .. }
        | Event::Measured { .. }
        | Event::Challenged { .. }
        | Event::Linked { .. }
        | Event::Profiled { .. }
        | Event::Digested { .. }
        // Which brain she thinks with is her body's, not something she
        // lived.
        | Event::BrainChanged { .. }
        // What a session is called, and whether a console lists it, is
        // how its operator finds it again; what was said in it is kept
        // as it was said.
        | Event::SessionNamed { .. }
        | Event::SessionSetAside { .. } => return None,
    })
}

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        // A note on something she kept is found for as long as it holds: a
        // later note on the same thing, or letting it go, takes it out.
        Event::Kept { file, .. } => {
            tx.execute(
                "DELETE FROM recall WHERE kind = 'kept' AND rowid IN
                 (SELECT seq FROM belongings WHERE file = ?1 AND seq < ?2)",
                params![file, entry.seq],
            )?;
        }
        Event::LetGo { kept } => {
            tx.execute(
                "DELETE FROM recall WHERE rowid = ?1 AND kind = 'kept'",
                [kept],
            )?;
            return Ok(());
        }
        _ => {}
    }
    // What she saw in an image is kept as part of the conversation it was
    // sent in: found from that platform, and from nowhere else.
    if let Event::Viewed { message, what, .. } = &entry.event {
        let sent: Option<(String, String, String)> = tx
            .query_row(
                "SELECT channel, place, name FROM messages WHERE seq = ?1",
                [message],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((channel, place, name)) = sent {
            tx.execute(
                "INSERT INTO recall (rowid, text, kind, seq, at, place, channel) VALUES (?2, ?1, 'viewed', ?2, ?3, ?4, ?5)",
                params![format!("{name} sent an image: {what}"), entry.seq, at, place, channel],
            )?;
        }
        return Ok(());
    }
    // Being stopped is part of the conversation it happened in, as what
    // was said there is: found from that platform.
    if let Event::Stopped {
        place,
        by,
        looking,
        unsaid,
        works,
    } = &entry.event
    {
        let text = stopped(tx, by, *looking, unsaid, works)?;
        let channel = social::split_key(place).map(|(channel, _)| channel);
        tx.execute(
            "INSERT INTO recall (rowid, text, kind, seq, at, place, channel) VALUES (?2, ?1, 'stopped', ?2, ?3, ?4, ?5)",
            params![text, entry.seq, at, place, channel],
        )?;
        return Ok(());
    }
    // What was taken back is no longer there to be found, in the
    // conversation or in anything she made of it there.
    if let Event::Rewound { place, to, .. } = &entry.event {
        tx.execute(
            "DELETE FROM recall WHERE place = ?1 AND seq >= ?2 AND seq < ?3",
            params![place, to, entry.seq],
        )?;
        return Ok(());
    }
    if let Some(k) = kept(&entry.event) {
        tx.execute(
            "INSERT INTO recall (rowid, text, kind, seq, at, place, channel) VALUES (?3, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![k.text, k.kind, entry.seq, at, k.place, k.channel],
        )?;
    }
    Ok(())
}

/// How much of a work's brief she remembers being stopped at.
const BRIEF_CHARS: usize = 160;

/// How she remembers being stopped by `by`: who it was, by the name she
/// knows them by, and what she was in the middle of, the work by its brief.
fn stopped(
    tx: &Transaction<'_>,
    by: &str,
    looking: bool,
    unsaid: &[String],
    works: &[String],
) -> Result<String> {
    let name: Option<String> = tx
        .query_row("SELECT name FROM people WHERE key = ?1", [by], |r| r.get(0))
        .optional()?;
    let name = name.unwrap_or_else(|| {
        social::split_key(by).map_or_else(|| by.to_owned(), |(_, id)| id.to_owned())
    });
    let mut what = Vec::new();
    if !unsaid.is_empty() {
        let quoted: Vec<String> = unsaid.iter().map(|u| format!("\"{}\"", u.trim())).collect();
        what.push(format!(
            "answering (you were about to say {})",
            quoted.join(" ")
        ));
    } else if looking {
        what.push("answering".to_owned());
    }
    let mut briefs = Vec::new();
    for id in works {
        let brief: Option<String> = tx
            .query_row("SELECT brief FROM undertakings WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let brief = brief.unwrap_or_else(|| id.clone());
        // The first line of a brief says what it is; the rest is detail.
        let first = brief.lines().next().unwrap_or_default().trim();
        briefs.push(first.chars().take(BRIEF_CHARS).collect::<String>());
    }
    if !briefs.is_empty() {
        what.push(format!(
            "the work you had taken on there ({})",
            briefs.join("; ")
        ));
    }
    Ok(if what.is_empty() {
        format!("{name} stopped you.")
    } else {
        format!(
            "{name} stopped you in the middle of {}: they called it off.",
            what.join(" and ")
        )
    })
}

impl Memory {
    /// Search her memory, and the messages on platform `channel`, oldest
    /// first. Words of three letters or more are matched as words
    /// and the best of them kept; shorter ones (most Chinese) are matched
    /// as text and the newest kept.
    pub fn search(&self, channel: &str, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let n = as_i64(limit);
        let (long, short): (Vec<&str>, Vec<&str>) =
            terms.iter().partition(|t| t.chars().count() >= 3);
        // Terms of three or more characters go through the trigram index,
        // best first; shorter ones fall back to a scan, newest first.
        let mut ranked = Vec::new();
        if !long.is_empty() {
            let phrases = long
                .iter()
                .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" OR ");
            let mut stmt = self.conn.prepare(
                "SELECT seq, at, kind, text, place FROM recall
                 WHERE recall MATCH ?1 AND (channel IS NULL OR channel = ?2)
                 ORDER BY rank LIMIT ?3",
            )?;
            ranked = stmt
                .query_map(params![phrases, channel, n], row_to_hit)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        let mut scanned = Vec::new();
        if !short.is_empty() {
            let clause = short
                .iter()
                .map(|_| "text LIKE ? ESCAPE '\\'")
                .collect::<Vec<_>>()
                .join(" OR ");
            let patterns = short.iter().map(|t| {
                let escaped = t
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                format!("%{escaped}%")
            });
            let mut stmt = self.conn.prepare(&format!(
                "SELECT seq, at, kind, text, place FROM recall
                 WHERE ({clause}) AND (channel IS NULL OR channel = ?)
                 ORDER BY rowid DESC LIMIT {n}"
            ))?;
            scanned = stmt
                .query_map(
                    rusqlite::params_from_iter(patterns.chain([channel.to_owned()])),
                    row_to_hit,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        let mut hits = ranked;
        for hit in scanned {
            if !hits.iter().any(|h| h.seq == hit.seq) {
                hits.push(hit);
            }
        }
        hits.sort_by_key(|h| h.seq);
        Ok(hits)
    }
}

fn row_to_hit(r: &rusqlite::Row<'_>) -> rusqlite::Result<Hit> {
    // FTS5 does not promise to keep column types, so accept either.
    let seq = match r.get::<_, Value>(0)? {
        Value::Integer(i) => i,
        Value::Text(t) => t.parse().unwrap_or(0),
        _ => 0,
    };
    let at: Option<String> = r.get(1)?;
    Ok(Hit {
        seq,
        at: at.and_then(|t| parse_stamp(&t).ok()),
        kind: r.get(2)?,
        text: r.get(3)?,
        place: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{fold, heard, on, said};
    use super::Hit;
    use crate::{journal::Event, social::PlaceKind};

    #[test]
    fn search_finds_latin_and_short_chinese() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "my interview is on friday",
                true,
            ),
            said("a", "东边那家茶馆要拆了"),
            Event::Canon {
                fact: "她养了一只叫豆包的猫".into(),
                source: "life".into(),
            },
        ]);
        assert_eq!(m.search("t", "interview", 5).unwrap().len(), 1);
        // Two characters: below the trigram minimum, found by scan.
        let tea = m.search("t", "茶馆", 5).unwrap();
        assert_eq!(tea.len(), 1);
        assert_eq!(tea[0].kind, "said");
        assert_eq!(m.search("t", "豆包 interview", 5).unwrap().len(), 2);
        assert!(m.search("t", "nothing here", 5).unwrap().is_empty());
        assert!(m.search("t", "100%_", 5).unwrap().is_empty());
    }

    #[test]
    fn a_conversation_sees_its_own_platform_and_the_whole_memory() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "the lantern festival is friday",
                true,
            ),
            on(
                "qq",
                heard(
                    "b",
                    PlaceKind::Direct,
                    "b",
                    "2",
                    "the lantern shop closed",
                    true,
                ),
            ),
            Event::Noted {
                sources: None,
                about: "qq:b".into(),
                text: "runs the lantern stall".into(),
                place: Some("qq:b".into()),
            },
        ]);
        let texts = |hits: Vec<Hit>| hits.into_iter().map(|h| h.text).collect::<Vec<_>>();
        // From Telegram: Telegram's messages, and every memory.
        let from_t = texts(m.search("t", "lantern", 10).unwrap());
        assert!(from_t.iter().any(|t| t.contains("festival is friday")));
        assert!(from_t.iter().any(|t| t == "runs the lantern stall"));
        assert!(!from_t.iter().any(|t| t.contains("shop closed")));
        // From QQ: the other way round for the messages, the same memory.
        let from_qq = texts(m.search("qq", "lantern", 10).unwrap());
        assert!(from_qq.iter().any(|t| t.contains("shop closed")));
        assert!(from_qq.iter().any(|t| t == "runs the lantern stall"));
        assert!(!from_qq.iter().any(|t| t.contains("festival is friday")));
    }

    #[test]
    fn what_she_saw_in_an_image_is_found_from_the_platform_it_was_sent_on() {
        let (_, m) = fold(vec![
            on(
                "qq",
                heard("b", PlaceKind::Direct, "b", "1", "[image]", true),
            ),
            Event::Viewed {
                message: 1,
                image: 0,
                what: "A sticker of a panda shrugging, captioned 无所谓.".into(),
                file: None,
            },
        ]);
        let hits = m.search("qq", "panda", 5).unwrap();
        let viewed: Vec<&Hit> = hits.iter().filter(|h| h.kind == "viewed").collect();
        assert_eq!(viewed.len(), 1);
        assert_eq!(
            viewed[0].text,
            "B sent an image: A sticker of a panda shrugging, captioned 无所谓."
        );
        assert_eq!(viewed[0].place.as_deref(), Some("qq:b"));
        assert!(m.search("t", "panda", 5).unwrap().is_empty());
        assert_eq!(m.search("qq", "无所谓", 5).unwrap().len(), 1);
    }

    #[test]
    fn being_stopped_is_remembered_with_who_did_it_and_what_was_called_off_in_that_conversation() {
        let (_, m) = fold(vec![
            on(
                "console",
                heard(
                    "fox",
                    PlaceKind::Direct,
                    "fox",
                    "1",
                    "what time is it",
                    true,
                ),
            ),
            Event::Undertook {
                id: "w1".into(),
                brief: "Draw the east gate.\nThey said: in ink.".into(),
                asked_by: Some("console:fox".into()),
                place: Some("console:fox".into()),
                directed: true,
                dir: None,
            },
            Event::Stopped {
                place: "console:fox".into(),
                by: "console:fox".into(),
                looking: true,
                unsaid: vec!["half past nine".into()],
                works: vec!["w1".into()],
            },
        ]);
        let hits = m.search("console", "stopped", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "stopped");
        assert_eq!(
            hits[0].text,
            "FOX stopped you in the middle of answering (you were about to say \"half past nine\") \
             and the work you had taken on there (Draw the east gate.): they called it off."
        );
        assert_eq!(hits[0].place.as_deref(), Some("console:fox"));
        // It belongs to that conversation, as what was said there does.
        assert!(m.search("t", "stopped", 5).unwrap().is_empty());
    }

    #[test]
    fn a_short_word_said_on_one_platform_is_not_found_from_another() {
        let (_, m) = fold(vec![
            said("a", "东边那家茶馆要拆了"),
            on(
                "qq",
                heard("b", PlaceKind::Direct, "b", "2", "茶馆的猫", true),
            ),
            Event::Posted {
                channel: "x".into(),
                id: None,
                text: "茶馆 at dusk".into(),
            },
        ]);
        let kinds = |channel| {
            m.search(channel, "茶馆", 10)
                .unwrap()
                .into_iter()
                .map(|h| h.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds("t"), ["said"]);
        assert_eq!(kinds("qq"), ["heard"]);
        assert_eq!(kinds("x"), ["posted"]);
    }
}
