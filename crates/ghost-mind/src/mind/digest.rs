//! Keeping the earlier part of a long conversation.
//!
//! She sees the recent messages of a place as they were written. What came
//! before them she keeps as a note in her own words, rewritten each time
//! enough new messages have gone by: the old note and the stretch since are
//! merged into one that replaces it. Nothing is lost to this. The messages
//! are all still in her journal, and `recall` searches them.

use chrono::{DateTime, Utc};
use ghost_brain::{Completion, Effort};
use ghost_core::{
    journal::{Event, Sources},
    memory::PlaceState,
    narrate,
};
use tracing::{debug, warn};

use super::{Mind, Outcome};
use crate::{Error, Result};

/// The most messages one note is asked to take in.
const AT_MOST: usize = 400;
/// Room for the note, and for a little thought before it.
const NOTE_TOKENS: u32 = 1500;

impl Mind {
    /// If more of this conversation has gone by than she is shown, have her
    /// write down what to keep of the part she is about to stop seeing.
    pub(super) fn consider_digest(&mut self, place: &PlaceState, now: DateTime<Utc>) -> Result<()> {
        let window = self.cfg.thread_window.max(1);
        if self.digesting.contains(&place.key) {
            return Ok(());
        }
        let memory = self.ghost.memory();
        let kept = memory.digest(&place.key)?;
        let after = kept.as_ref().map_or(0, |d| d.upto);
        let unkept = usize::try_from(memory.lines_after(&place.key, after)?).unwrap_or(0);
        if unkept <= window * 2 {
            return Ok(());
        }
        // Everything older than what she will go on seeing.
        let Some(first_seen) = memory.thread(&place.key, window)?.first().map(|l| l.seq) else {
            return Ok(());
        };
        let stretch = memory.thread_between(&place.key, after, first_seen - 1, AT_MOST)?;
        let (Some(first), Some(last)) = (stretch.first(), stretch.last()) else {
            return Ok(());
        };
        let upto = last.seq;
        // The new note stands for everything the old one did, and this
        // stretch besides.
        let before = kept.as_ref().and_then(|d| d.sources);
        let sources = Sources {
            from: before.map_or(first.seq, |s| s.from),
            upto,
            since: before.map_or(first.at, |s| s.since),
            until: last.at,
        };
        if !self.budget.allow(now) {
            return Ok(());
        }
        let people = memory.people_in(&place.key, 6)?;
        let prompt = narrate::fill(
            &self.prompts.digest,
            &[
                ("where", self.place_words(place, &people, true)?),
                (
                    "before",
                    self.prompts
                        .or(kept.map(|d| d.text).unwrap_or_default(), "nothing_yet"),
                ),
                (
                    "transcript",
                    crate::narrate::transcript(
                        &self.prompts,
                        &stretch,
                        i64::MAX,
                        self.life.clock(),
                        now,
                    ),
                ),
            ],
        );
        let request = Completion::ask(self.who(now)?, prompt, NOTE_TOKENS).thinking(Effort::Low);
        let brain = self.tick.clone();
        self.budget.spend(now);
        self.digesting.insert(place.key.clone());
        let place = place.key.clone();
        self.jobs.spawn(async move {
            let result = brain
                .complete(&request)
                .await
                .map(|reply| reply.text)
                .map_err(Error::from);
            Outcome::Digested {
                place,
                upto,
                sources,
                result,
            }
        });
        Ok(())
    }

    pub(super) fn on_digested(
        &mut self,
        place: &str,
        upto: i64,
        sources: Sources,
        result: Result<String>,
    ) -> Result<()> {
        self.digesting.remove(place);
        match result {
            Ok(text) if !text.trim().is_empty() => {
                debug!("{place}: kept the conversation up to #{upto}");
                self.record(Event::Digested {
                    place: place.to_owned(),
                    upto,
                    text: text.trim().to_owned(),
                    sources: Some(sources),
                })?;
            }
            Ok(_) => {}
            // Nothing is lost by this: she goes on seeing what she saw,
            // and tries again the next time she looks.
            Err(e) => warn!("{place}: could not keep the earlier conversation: {e}"),
        }
        Ok(())
    }
}
