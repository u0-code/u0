//! Turning who she is and what has happened into words a brain can read.
//!
//! Pure text formatting over domain values. The templates themselves live
//! with whoever calls a brain; this module only knows how to describe her,
//! her day, the people around her, and a conversation.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::{
    identity::SealedIdentity,
    journal::{Entry, Event},
    life::{Clock, Day, Moment, Phase, Reach},
    memory::{Fact, Line, Note, Person},
    when::{day, when},
    will::Standing,
};

/// Replace `{{key}}` placeholders in one pass.
///
/// Values are never rescanned, so text that came from other people cannot
/// smuggle a placeholder in. Unknown keys are left as written.
pub fn fill(template: &str, pairs: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = after[..close].trim();
        match pairs.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[open..open + 2 + close + 2]),
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Everything about her and this moment that every prompt needs.
pub struct Scene<'a> {
    pub identity: &'a SealedIdentity,
    pub clock: &'a Clock,
    pub now: DateTime<Utc>,
    pub moment: &'a Moment,
    pub day: Option<&'a Day>,
    /// Recent `lived` / `reflected` entries, oldest first.
    pub recent: &'a [Entry],
    pub canon: &'a [Fact],
}

impl Scene<'_> {
    /// The placeholder values for a self template.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        let seed = self.identity.seed();
        let tz = self.clock.tz();
        vec![
            ("name", seed.name.clone()),
            ("pronouns", seed.pronouns.clone()),
            ("city", seed.home.city.clone()),
            ("country", seed.home.country.clone()),
            ("temperament", seed.temperament.trim().to_owned()),
            ("voice", seed.voice.trim().to_owned()),
            ("origin", seed.origin.trim().to_owned()),
            ("values", list(&seed.values)),
            ("cares_about", list(&seed.cares_about)),
            ("cannot_stand", list(&seed.cannot_stand)),
            (
                "now",
                self.now
                    .with_timezone(&tz)
                    .format("%A %Y-%m-%d %H:%M")
                    .to_string(),
            ),
            ("state", self.state(tz)),
            ("day", self.plan(tz)),
            (
                "weather",
                self.day
                    .and_then(|d| d.weather.clone())
                    .map(|w| format!("Weather: {w}"))
                    .unwrap_or_default(),
            ),
            ("recent", self.lately()),
            ("canon", self.facts()),
        ]
    }

    fn state(&self, tz: Tz) -> String {
        if self.moment.phase == Phase::Asleep {
            return "You are asleep. It is the middle of your night; if you are reading this, something woke you.".into();
        }
        match &self.moment.doing {
            Some(doing) => format!(
                "You are in the middle of: {} (until {}). {}",
                doing.what,
                doing.to.with_timezone(&tz).format("%H:%M"),
                match doing.reach {
                    Reach::Free => "Your phone is in your hand.",
                    Reach::Glance => "You look at your phone now and then.",
                    Reach::Busy => "You are occupied and only glanced at your phone.",
                    Reach::Away => "You were not looking at your phone until just now.",
                }
            ),
            None => "You are awake and nothing in particular is going on.".into(),
        }
    }

    fn plan(&self, tz: Tz) -> String {
        match self.day {
            Some(day) if !day.blocks.is_empty() => day
                .blocks
                .iter()
                .map(|b| {
                    // A plan is not a memory: say what is behind her, what
                    // is now, and leave the rest as still to come.
                    let when = if b.to <= self.now {
                        " (earlier)"
                    } else if b.from <= self.now {
                        " (now)"
                    } else {
                        " (still to come)"
                    };
                    format!(
                        "- {}–{} {}{when}",
                        b.from.with_timezone(&tz).format("%H:%M"),
                        b.to.with_timezone(&tz).format("%H:%M"),
                        b.what
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => "(nothing planned)".into(),
        }
    }

    fn lately(&self) -> String {
        if self.recent.is_empty() {
            return "(nothing worth noting yet)".into();
        }
        let rows = self.recent.iter().filter_map(|e| {
            let text = match &e.event {
                Event::Lived { text, .. } => text.clone(),
                Event::Reflected { text } => format!("(looking back) {text}"),
                Event::Posted { text, .. } => format!("you posted: {text}"),
                Event::Dormant { since } => format!(
                    "(a blank: nothing from {} until then)",
                    when(*since, self.now, self.clock)
                ),
                _ => return None,
            };
            Some((when(e.at, self.now, self.clock), text))
        });
        grouped(rows)
    }

    fn facts(&self) -> String {
        if self.canon.is_empty() {
            return "(nothing registered yet beyond who you are)".into();
        }
        self.canon
            .iter()
            .map(|f| format!("- {}", f.fact))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A conversation as she sees it on her phone.
///
/// Every line carries `[mN]`, its journal seq, so a brain can point at a
/// message. A marker separates what she has already seen from what is new.
/// Lines carry the time of day; when the conversation reaches back past
/// today, each day begins with a line that names it, the way a phone does.
pub fn transcript(lines: &[Line], read_upto: i64, clock: &Clock, now: DateTime<Utc>) -> String {
    if lines.is_empty() {
        return "(no messages yet)".into();
    }
    let by_platform_id: HashMap<&str, i64> = lines
        .iter()
        .filter_map(|l| Some((l.platform_id.as_deref()?, l.seq)))
        .collect();
    let tz = clock.tz();
    let all_today = lines
        .iter()
        .all(|l| clock.day_of(l.at) == clock.day_of(now));
    let mut out = String::new();
    let mut marked = false;
    let mut on = None;
    for line in lines {
        let this = clock.day_of(line.at);
        if !all_today && on != Some(this) {
            out.push_str(&format!("--- {} ---\n", day(line.at, now, clock)));
            on = Some(this);
        }
        if !marked && line.incoming && line.seq > read_upto {
            if !out.is_empty() {
                out.push_str("--- new since you last looked ---\n");
            }
            marked = true;
        }
        let who = if line.incoming {
            line.name.as_str()
        } else {
            "you"
        };
        let quoted = line
            .reply_to
            .as_deref()
            .and_then(|id| by_platform_id.get(id))
            .map(|seq| format!(" (replying to m{seq})"))
            .unwrap_or_default();
        let text = line.text.replace('\n', "\n      ");
        out.push_str(&format!(
            "[m{}] {} {who}{quoted}: {text}\n",
            line.seq,
            line.at.with_timezone(&tz).format("%H:%M"),
        ));
    }
    out.trim_end().to_owned()
}

/// A few lines about someone, for when they are in front of her.
pub fn person_card(
    person: &Person,
    standing: &Standing,
    notes: &[Note],
    now: DateTime<Utc>,
    clock: &Clock,
) -> String {
    let handle = person
        .handle
        .as_deref()
        .map(|h| format!(" (@{h})"))
        .unwrap_or_default();
    let mut out = format!(
        "- {}{handle}: {}. You first heard from them {}, and have talked on {} day{}.",
        person.name,
        standing.stage.word(),
        when(person.first_at, now, clock),
        standing.active_days,
        if standing.active_days == 1 { "" } else { "s" },
    );
    match standing.warmth {
        w if w >= 8 => out.push_str(" You are fond of them."),
        w if w >= 3 => out.push_str(" You like them."),
        w if w <= -6 => out.push_str(" You want little to do with them."),
        w if w <= -3 => out.push_str(" They have been wearing on you."),
        _ => {}
    }
    if person.automated {
        out.push_str(" This account is automated: a bot, or another ghost.");
    }
    for note in notes {
        out.push_str(&format!("\n  · {}", note.text));
    }
    out
}

/// Rows of `(when, what)` in order, with a run of rows that happened at
/// the same remembered time put under it once.
pub fn grouped(rows: impl IntoIterator<Item = (String, String)>) -> String {
    let mut runs: Vec<(String, Vec<String>)> = Vec::new();
    for (at, text) in rows {
        match runs.last_mut() {
            Some((last, texts)) if *last == at => texts.push(text),
            _ => runs.push((at, vec![text])),
        }
    }
    runs.iter()
        .map(|(at, texts)| match texts.as_slice() {
            [one] => format!("- {}: {one}", capital(at)),
            many => format!(
                "- {}:\n{}",
                capital(at),
                many.iter()
                    .map(|t| format!("  - {t}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text with its first letter capitalised.
pub fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "(nothing listed)".to_owned()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveTime};

    use super::*;

    fn clock() -> Clock {
        Clock::new(
            chrono_tz::UTC,
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
        )
    }

    #[test]
    fn fill_is_single_pass() {
        let out = fill(
            "a {{x}} b {{ y }} c {{z}} {{",
            &[("x", "{{y}}".into()), ("y", "Y".into())],
        );
        assert_eq!(out, "a {{y}} b Y c {{z}} {{");
    }

    fn line(seq: i64, incoming: bool, text: &str, pid: &str, reply_to: Option<&str>) -> Line {
        Line {
            seq,
            at: Utc::now(),
            incoming,
            person: None,
            name: "Haoran".into(),
            platform_id: Some(pid.into()),
            reply_to: reply_to.map(Into::into),
            addressed: true,
            text: text.into(),
        }
    }

    #[test]
    fn transcript_marks_what_is_new() {
        let lines = vec![
            line(3, true, "hi", "a", None),
            line(5, false, "hey", "b", None),
            line(8, true, "two\nlines", "c", Some("b")),
        ];
        let t = transcript(&lines, 5, &clock(), Utc::now());
        let rows: Vec<&str> = t.lines().collect();
        assert!(rows[0].starts_with("[m3]") && rows[0].ends_with("Haoran: hi"));
        assert!(rows[1].contains("you: hey"));
        assert_eq!(rows[2], "--- new since you last looked ---");
        assert!(rows[3].contains("Haoran (replying to m5): two"));
        assert_eq!(rows[4], "      lines");
    }

    #[test]
    fn a_conversation_that_reaches_back_names_its_days() {
        let now = Utc::now();
        let mut lines = vec![
            line(3, true, "hi", "a", None),
            line(5, false, "hey", "b", None),
            line(8, true, "again", "c", None),
        ];
        lines[0].at = now - Duration::days(1);
        lines[1].at = now - Duration::days(1);
        let t = transcript(&lines, 8, &clock(), now);
        let rows: Vec<&str> = t.lines().collect();
        assert_eq!(rows[0], "--- yesterday ---");
        assert!(rows[1].starts_with("[m3]"));
        assert!(rows[2].starts_with("[m5]"));
        assert_eq!(rows[3], "--- today ---");
        assert!(rows[4].starts_with("[m8]"));
    }

    #[test]
    fn rows_remembered_at_the_same_time_are_put_under_it_once() {
        let out = grouped([
            ("this morning".to_owned(), "a".to_owned()),
            ("this morning".to_owned(), "b".to_owned()),
            ("about two hours ago".to_owned(), "c".to_owned()),
        ]);
        assert_eq!(
            out,
            "- This morning:\n  - a\n  - b\n- About two hours ago: c"
        );
    }
}
