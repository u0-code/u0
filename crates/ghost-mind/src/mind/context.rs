//! What she brings to a conversation besides the conversation itself.
//!
//! A conversation on one platform is shown that platform's record: its
//! messages, and the other places on it where something is going on. What
//! she remembers is shown whole, from whichever platform it was formed on,
//! and each memory says where that was, so that what was told to her in
//! private is marked as such wherever it comes up.

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Sources,
    memory::{Note, PlaceState},
    narrate::capital,
    social::PlaceKind,
    when::when,
};

use super::Mind;
use crate::{Result, actions::clip};

/// How far back "the rest of your day" reaches.
const WINDOW: Duration = Duration::hours(6);
/// How long a finished undertaking stays on her mind.
const FRESH: Duration = Duration::hours(12);

impl Mind {
    /// A place in a few words, as she would refer to it from elsewhere.
    pub(super) fn place_words(&self, place: &PlaceState) -> Result<String> {
        Ok(match place.kind {
            PlaceKind::Direct => {
                let who = self
                    .ghost
                    .memory()
                    .person_of_direct(&place.key)?
                    .map_or_else(|| "someone".to_owned(), |p| p.name);
                format!("a private chat with {who} on {}", place.channel)
            }
            PlaceKind::Group => format!(
                "the group {} on {}",
                place
                    .name
                    .as_deref()
                    .map_or_else(|| "chat".to_owned(), |n| format!("\"{n}\"")),
                place.channel
            ),
            PlaceKind::Public => format!("public replies on {}", place.channel),
        })
    }

    /// Where something was said or learned, relative to where she is now:
    /// a few words to put after it, or nothing if it has no place.
    pub(super) fn learned_where(&self, learned: Option<&str>, here: &str) -> Result<String> {
        let Some(key) = learned else {
            return Ok(String::new());
        };
        if key == here {
            return Ok(" (here)".to_owned());
        }
        let Some(place) = self.ghost.memory().place(key)? else {
            return Ok(String::new());
        };
        Ok(match place.kind {
            PlaceKind::Direct => {
                format!(" (in {}; told to you privately)", self.place_words(&place)?)
            }
            _ => format!(" (in {})", self.place_words(&place)?),
        })
    }

    /// Mark the notes that came from a private conversation other than this
    /// one, so she knows not to bring them up here.
    pub(super) fn with_provenance(&self, notes: Vec<Note>, here: &str) -> Result<Vec<Note>> {
        let memory = self.ghost.memory();
        notes
            .into_iter()
            .map(|mut note| {
                if let Some(key) = note.place.as_deref()
                    && key != here
                    && memory
                        .place(key)?
                        .is_some_and(|p| p.kind == PlaceKind::Direct)
                {
                    note.text.push_str(" (they told you this in private)");
                }
                Ok(note)
            })
            .collect()
    }

    /// What she knows of someone, for a place: her standing account of
    /// them and the notes written since. Outside the private conversation
    /// with that person, the part they told her alone is left out, and she
    /// is told only that there is one.
    pub(super) fn known_of(
        &self,
        person: &ghost_core::memory::Person,
        here: &PlaceState,
        limit: usize,
        now: DateTime<Utc>,
    ) -> Result<(String, Vec<Note>)> {
        let memory = self.ghost.memory();
        let alone_with_them =
            here.kind == PlaceKind::Direct && person.direct.as_deref() == Some(here.key.as_str());
        let profile = memory.profile(&person.key)?;
        let since = profile.as_ref().map_or(0, |p| p.seq);
        let mut notes = memory.notes_after(&person.key, since, limit)?;
        let skip = notes.len().saturating_sub(limit);
        let notes = self.with_provenance(notes.split_off(skip), &here.key)?;
        let mut text = String::new();
        let Some(profile) = profile else {
            return Ok((text, notes));
        };
        let as_of = profile.sources.map_or_else(String::new, |s| {
            format!(" (as of {})", when(s.until, now, self.life.clock()))
        });
        if !profile.shared.trim().is_empty() {
            text.push_str(&format!(
                "\n  What you know of them: {}{as_of}",
                profile.shared.trim()
            ));
        }
        if !profile.private.trim().is_empty() {
            if alone_with_them {
                text.push_str(&format!(
                    "\n  What they have told you in private: {}{as_of}",
                    profile.private.trim()
                ));
            } else {
                text.push_str("\n  (They have told you more in private. None of it is for here.)");
            }
        }
        Ok((text, notes))
    }

    /// The rest of her life at this moment, for the top of a conversation:
    /// what she has taken on, and what has been going on in other places.
    /// Empty when there is nothing to say.
    pub(super) fn elsewhere(&self, here: &PlaceState, now: DateTime<Utc>) -> Result<String> {
        let memory = self.ghost.memory();
        let mut lines = Vec::new();

        for u in memory.undertakings(false, 6)? {
            let whom = match u.asked_by.as_deref().map(|key| memory.person(key)) {
                Some(Ok(Some(person))) => format!(" for {}", person.name),
                _ => String::new(),
            };
            match u.outcome.as_deref() {
                None => {
                    let state = if let Some(question) = &u.waiting {
                        format!(
                            "You stopped to ask them: \"{}\". It waits for their answer. When they give one, \
                             take it on again with just the answer, so that it reaches the work.",
                            clip(question, 200)
                        )
                    } else if self.working.as_ref().is_some_and(|w| w.id == u.id) {
                        "You were in the middle of it just now.".to_owned()
                    } else if u.sittings == 0 {
                        "You have not started yet.".to_owned()
                    } else {
                        format!(
                            "{} sitting{} so far. {}",
                            u.sittings,
                            if u.sittings == 1 { "" } else { "s" },
                            clip(u.summary.as_deref().unwrap_or_default(), 200)
                        )
                    };
                    lines.push(format!(
                        "- You have taken something on{whom}: \"{}\". {state}",
                        clip(&u.brief, 160)
                    ));
                }
                Some(outcome) if now - u.last_at < FRESH => lines.push(format!(
                    "- {} you {} what you took on{whom} (\"{}\"): {}",
                    capital(&when(u.last_at, now, self.life.clock())),
                    match outcome {
                        "finished" => "finished",
                        "blocked" => "got stuck on",
                        _ => "let go of",
                    },
                    clip(&u.brief, 100),
                    clip(u.summary.as_deref().unwrap_or_default(), 240)
                )),
                Some(_) => {}
            }
        }

        for other in memory.elsewhere(now - WINDOW, &here.key, 5)? {
            let unread = match other.place.unread {
                0 => String::new(),
                n => format!(", {n} you have not read"),
            };
            let said = match (other.heard, other.said) {
                (heard, 0) => format!("{heard} to read, nothing from you"),
                (0, said) => format!("{said} from you"),
                (heard, said) => format!("{heard} from them, {said} from you"),
            };
            lines.push(format!(
                "- {}: {said}, latest {}{unread}.",
                capital(&self.place_words(&other.place)?),
                when(other.last.at, now, self.life.clock())
            ));
        }

        if lines.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "# The rest of your day\n\n{}\n\nThose are other conversations and your own business. \
             What someone told you in private is not repeated here, and you do not let on that you know it.\n\n",
            lines.join("\n")
        ))
    }

    /// The messages she has in front of her in a place, as the stretch of
    /// her journal they are: what a note written there was made from.
    pub(super) fn shown(&self, place: &PlaceState) -> Result<Option<Sources>> {
        let lines = self
            .ghost
            .memory()
            .thread(&place.key, self.cfg.thread_window.max(1))?;
        Ok(lines
            .first()
            .zip(lines.last())
            .map(|(first, last)| Sources {
                from: first.seq,
                upto: last.seq,
                since: first.at,
                until: last.at,
            }))
    }

    /// What someone here wrote lately, word for word: so that work taken
    /// on from a conversation starts from what was asked and not only from
    /// her own account of it.
    pub(super) fn their_words(
        &self,
        place: &PlaceState,
        person: &ghost_core::memory::Person,
    ) -> Result<String> {
        let mut said: Vec<String> = self
            .ghost
            .memory()
            .thread(&place.key, 16)?
            .into_iter()
            .filter(|l| l.incoming && l.person.as_deref() == Some(person.key.as_str()))
            .map(|l| format!("> {}", l.text.replace('\n', "\n> ")))
            .collect();
        let from = said.len().saturating_sub(5);
        said.drain(..from);
        if said.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "\n\nWhat {} wrote, word for word:\n{}",
            person.name,
            said.join("\n")
        ))
    }

    /// The files she made for the people here and has not long finished:
    /// what she could hand over in this place.
    pub(super) fn deliverables(&self, place: &PlaceState) -> Result<Vec<String>> {
        if !self.can(&place.channel, crate::Can::Files) || self.workshop.machine.is_none() {
            return Ok(Vec::new());
        }
        let memory = self.ghost.memory();
        let here: Vec<String> = memory
            .people_in(&place.key, 12)?
            .into_iter()
            .map(|p| p.key)
            .chain(memory.person_of_direct(&place.key)?.map(|p| p.key))
            .collect();
        let mut files = Vec::new();
        for u in memory.undertakings(false, 12)? {
            let theirs = u.place.as_deref() == Some(place.key.as_str())
                || u.asked_by.as_ref().is_some_and(|who| here.contains(who));
            if !theirs || u.outcome.as_deref() != Some("finished") {
                continue;
            }
            if let Ok(done) = self.undertaking(&u.id, &u.brief)
                && let Some(finish) = done.state().finish
            {
                files.extend(finish.files);
            }
        }
        Ok(files)
    }

    /// The actions that exist only in a body that can do them.
    pub(super) fn abilities(&self) -> String {
        if !self.can_work() {
            return String::new();
        }
        let means = if self.workshop.machine.is_some() {
            "at your computer"
        } else {
            "by looking things up and thinking them through"
        };
        format!(
            "- {{\"do\":\"undertake\",\"brief\":\"...\",\"for\":\"<name>\"}}\n  \
             Take on something that needs real work {means}: finding something out properly, making something, checking something. \
             It is not done in this reply. You will work on it in your own time and come back with what you found. \
             Write the brief as a complete instruction to yourself, with everything needed to do it without this conversation in front of you. \
             If you have already taken something on for them and they add to it or change it, use this again with just what is new: it is added to what you are doing. \
             Leave out \"for\" when it is for yourself. Only for things you want to do or have agreed to do: nobody is owed your afternoon. \
             Say something as well, the way you would.\n"
        )
    }
}
