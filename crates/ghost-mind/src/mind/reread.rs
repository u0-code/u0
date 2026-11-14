//! Going back to what a memory came from.
//!
//! What she keeps in short form says which stretch of her journal it was
//! made from. When the words themselves matter, she can go back to them:
//! the messages a note or an account was made from, or the conversation
//! around a message she remembers. What she is shown follows the rule for
//! everything else in a conversation: this platform's record only, and
//! nobody else's private conversation.

use chrono::{DateTime, Utc};
use ghost_core::{
    journal::{Event, Sources},
    memory::{Line, PlaceState},
    narrate,
    social::{PlaceKind, split_key},
    when::when,
};

use super::Mind;
use crate::Result;

/// The most messages she is shown at once.
const SHOWN: usize = 40;
/// How many messages either side of one she remembers.
const AROUND: usize = 4;
/// How many messages are looked through to find the ones that matter.
const SCANNED: usize = 400;

/// What a memory was made from.
enum Behind {
    /// One conversation.
    Place(String),
    /// Whatever one person said, and their private conversation with her.
    Person(String),
    /// A whole platform.
    Channel(String),
}

impl Mind {
    /// What memory `n` came from, as she would be shown it in `here`.
    pub(super) fn reread(&self, n: i64, here: &PlaceState, now: DateTime<Utc>) -> Result<String> {
        let memory = self.ghost.memory();
        // A message she remembers: the conversation around it.
        if let Some((place, _)) = memory.line(n)? {
            if let Some(elsewhere) = not_here(&place, here) {
                return Ok(elsewhere);
            }
            let lines = memory
                .around(&place, n, AROUND)?
                .into_iter()
                .map(|l| (place.clone(), l))
                .collect();
            return self.as_shown(
                &format!("The conversation around message #{n}:"),
                lines,
                here,
                now,
            );
        }
        let Some(entry) = self.ghost.journal().entry(n)? else {
            return Ok(format!("You have no memory numbered {n}."));
        };
        let (sources, behind) = match entry.event {
            Event::Noted {
                place: Some(place),
                sources: Some(s),
                ..
            }
            | Event::Digested {
                place,
                sources: Some(s),
                ..
            } => (s, Behind::Place(place)),
            Event::Noted {
                about,
                sources: Some(s),
                ..
            }
            | Event::Profiled {
                person: about,
                sources: Some(s),
                ..
            } => (s, Behind::Person(about)),
            Event::Reflected {
                channel: Some(channel),
                sources: Some(s),
                ..
            } => (s, Behind::Channel(channel)),
            Event::Reflected {
                channel: None,
                sources: Some(s),
                ..
            } => return self.own_day(n, s, now),
            Event::LookedUp { query, found, .. } => {
                return Ok(format!(
                    "Memory #{n} came from the web, when you looked up \"{query}\". What came back:\n{found}"
                ));
            }
            _ => {
                return Ok(format!(
                    "Memory #{n} was not kept with what it came from. What you have of it is what you remember."
                ));
            }
        };
        let key = match &behind {
            Behind::Place(key) | Behind::Person(key) => key.clone(),
            Behind::Channel(channel) => format!("{channel}:"),
        };
        if let Some(elsewhere) = not_here(&key, here) {
            return Ok(elsewhere);
        }
        let lines: Vec<(String, Line)> = match &behind {
            Behind::Place(place) => memory
                .thread_before(place, sources.upto, SCANNED)?
                .into_iter()
                .filter(|l| l.seq >= sources.from)
                .map(|l| (place.clone(), l))
                .collect(),
            Behind::Person(person) => {
                let direct = memory.person(person)?.and_then(|p| p.direct);
                memory
                    .lines_on(&here.channel, sources.from, sources.upto, SCANNED)?
                    .into_iter()
                    .filter(|(place, l)| {
                        l.person.as_deref() == Some(person.as_str())
                            || direct.as_deref() == Some(place.as_str())
                    })
                    .collect()
            }
            Behind::Channel(channel) => {
                memory.lines_on(channel, sources.from, sources.upto, SCANNED)?
            }
        };
        let clock = self.life.clock();
        self.as_shown(
            &format!(
                "What memory #{n} came from, {} until {}:",
                when(sources.since, now, clock),
                when(sources.until, now, clock)
            ),
            lines,
            here,
            now,
        )
    }

    /// Messages as she is shown them, a conversation at a time, with
    /// other people's private conversations left out.
    fn as_shown(
        &self,
        header: &str,
        lines: Vec<(String, Line)>,
        here: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<String> {
        let memory = self.ghost.memory();
        let mut withheld = 0;
        let mut kept = Vec::with_capacity(lines.len());
        for (place, line) in lines {
            let private_elsewhere = place != here.key
                && memory
                    .place(&place)?
                    .is_some_and(|p| p.kind == PlaceKind::Direct);
            if private_elsewhere {
                withheld += 1;
            } else {
                kept.push((place, line));
            }
        }
        let skipped = kept.len().saturating_sub(SHOWN);
        kept.drain(..skipped);
        if kept.is_empty() {
            return Ok(if withheld > 0 {
                "What that came from was said in private conversations with other people, and none of it is for here.".to_owned()
            } else {
                "Nothing of what that came from is left to read.".to_owned()
            });
        }
        let clock = self.life.clock();
        let mut out = vec![header.to_owned()];
        if skipped > 0 {
            out.push(format!("({skipped} earlier messages are not shown.)"));
        }
        let mut i = 0;
        while i < kept.len() {
            let place = kept[i].0.clone();
            let mut run = Vec::new();
            while i < kept.len() && kept[i].0 == place {
                run.push(kept[i].1.clone());
                i += 1;
            }
            if place != here.key
                && let Some(state) = memory.place(&place)?
            {
                out.push(format!("In {}:", self.place_words(&state)?));
            }
            out.push(narrate::transcript(
                &run,
                i64::MAX,
                self.prompts.text("directive_mark"),
                clock,
                now,
            ));
        }
        if withheld > 0 {
            out.push(format!(
                "({withheld} messages from private conversations with other people are left out: they are not for here.)"
            ));
        }
        Ok(out.join("\n"))
    }

    /// Her own day, for a look back on it: what she did, and posted.
    fn own_day(&self, n: i64, sources: Sources, now: DateTime<Utc>) -> Result<String> {
        let clock = self.life.clock();
        let span = usize::try_from(sources.upto - sources.from + 1).unwrap_or(0);
        let rows: Vec<(String, String)> = self
            .ghost
            .journal()
            .read(sources.from, span.min(2000))?
            .into_iter()
            .filter(|e| e.seq <= sources.upto)
            .filter_map(|e| {
                let text = match e.event {
                    Event::Lived { text, .. } => text,
                    Event::Posted { text, .. } => format!("you posted: {text}"),
                    _ => return None,
                };
                Some((when(e.at, now, clock), text))
            })
            .collect();
        if rows.is_empty() {
            return Ok(format!(
                "Memory #{n} was a look back on a day with nothing else written down."
            ));
        }
        Ok(format!(
            "The day memory #{n} looked back on:\n{}",
            narrate::grouped(rows)
        ))
    }
}

/// Why she cannot reread something from another platform here, or
/// nothing if she can.
fn not_here(key: &str, here: &PlaceState) -> Option<String> {
    let channel = split_key(key).map_or(key.trim_end_matches(':'), |(c, _)| c);
    (channel != here.channel).then(|| {
        format!(
            "That was on {channel}. Its messages are kept with {channel} and are not in front of you here: what you have of it is what you remember."
        )
    })
}
