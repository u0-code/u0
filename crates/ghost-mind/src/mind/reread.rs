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
    social::split_key,
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
        let number = ("n", n.to_string());
        // A message she remembers: the conversation around it.
        if let Some((place, _)) = memory.line(n)? {
            if let Some(elsewhere) = self.not_here(&place, here) {
                return Ok(elsewhere);
            }
            let lines = memory
                .around(&place, n, AROUND)?
                .into_iter()
                .map(|l| (place.clone(), l))
                .collect();
            return self.as_shown(
                &self.prompts.fill("reread_around", &[number]),
                lines,
                here,
                now,
            );
        }
        let Some(entry) = self.ghost.journal().entry(n)? else {
            return Ok(self.prompts.fill("no_memory", &[number]));
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
                return Ok(self
                    .prompts
                    .fill("reread_web", &[number, ("query", query), ("found", found)]));
            }
            _ => return Ok(self.prompts.fill("reread_unkept", &[number])),
        };
        let key = match &behind {
            Behind::Place(key) | Behind::Person(key) => key.clone(),
            Behind::Channel(channel) => format!("{channel}:"),
        };
        if let Some(elsewhere) = self.not_here(&key, here) {
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
            &self.prompts.fill(
                "reread_from",
                &[
                    number,
                    ("since", when(sources.since, now, clock)),
                    ("until", when(sources.until, now, clock)),
                ],
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
            if self.kept_from(&place, &here.key)? {
                withheld += 1;
            } else {
                kept.push((place, line));
            }
        }
        let skipped = kept.len().saturating_sub(SHOWN);
        kept.drain(..skipped);
        if kept.is_empty() {
            return Ok(self
                .prompts
                .text(if withheld > 0 {
                    "reread_all_private"
                } else {
                    "reread_gone"
                })
                .to_owned());
        }
        let clock = self.life.clock();
        let mut out = vec![header.to_owned()];
        if skipped > 0 {
            out.push(
                self.prompts
                    .fill("reread_skipped", &[("count", skipped.to_string())]),
            );
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
                out.push(self.prompts.fill(
                    "reread_in",
                    &[("place", self.place_words(&state, &[], false)?)],
                ));
            }
            out.push(crate::narrate::transcript(
                &self.prompts,
                &run,
                i64::MAX,
                clock,
                now,
            ));
        }
        if withheld > 0 {
            out.push(
                self.prompts
                    .fill("reread_withheld", &[("count", withheld.to_string())]),
            );
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
                    Event::Posted { text, .. } => crate::narrate::posted(&self.prompts, &text),
                    _ => return None,
                };
                Some((when(e.at, now, clock), text))
            })
            .collect();
        let number = ("n", n.to_string());
        if rows.is_empty() {
            return Ok(self.prompts.fill("reread_day_empty", &[number]));
        }
        Ok(self
            .prompts
            .fill("reread_day", &[number, ("day", narrate::grouped(rows))]))
    }

    /// Why she cannot reread something from another platform here, or
    /// nothing if she can.
    fn not_here(&self, key: &str, here: &PlaceState) -> Option<String> {
        let channel = split_key(key).map_or(key.trim_end_matches(':'), |(c, _)| c);
        (channel != here.channel).then(|| {
            self.prompts
                .fill("reread_elsewhere", &[("channel", channel.to_owned())])
        })
    }
}
