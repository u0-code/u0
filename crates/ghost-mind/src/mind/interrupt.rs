//! Being stopped: someone who runs her body, at a console, calls off what
//! she is in the middle of in a place, as one stops a coding agent after
//! sending it something.
//!
//! A stop ends the look and what it was about to say, not her work. A look
//! whose brain call is out is aborted, and if its answer was already back
//! and not yet heard, it is let go when it comes in: each look has a
//! number, and only the place's current look is acted on. What she had
//! decided to say and not yet handed to the body is dropped. Work she took
//! on there goes on: it is stopped from its own card, with `work.stop`, as
//! a coding agent's background tasks outlive a stopped turn, since whoever
//! stops her usually means to put it another way, not to throw away an
//! hour of hers. What was unread there is left read, so that she does not
//! look again at the same line, and her journal says who stopped her and
//! in the middle of what, so that the next look knows it was interrupted
//! on purpose.

use chrono::{DateTime, Utc};
use ghost_core::{
    journal::Event,
    memory::{Line, PlaceState},
    narrate::capital,
    social,
    when::when,
};
use tracing::info;

use super::{Mind, Operator};
use crate::{Result, actions::clip};

/// How much of a work's brief she is reminded of when she was stopped at it.
const BRIEF: usize = 120;

impl Mind {
    /// The operator `by` stops her in the place `key`. Returns whether
    /// anything was stopped: a look she was in the middle of or about to
    /// take, or something she had not yet said.
    pub(super) fn interrupt(
        &mut self,
        key: &str,
        by: &Operator,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        self.agenda.attend.remove(key);
        self.retries.remove(key);
        let under_way = self.thinking.remove(key);
        if let Some(looking) = &under_way {
            looking.abort.abort();
        }
        let unsaid = match self.agenda.speaking.get_mut(key) {
            Some(speaking) => {
                let unsaid = speaking.cut();
                if speaking.is_empty() {
                    self.agenda.speaking.remove(key);
                }
                unsaid
            }
            None => Vec::new(),
        };
        let place = self.ghost.memory().place(key)?;
        let unread = place.as_ref().filter(|p| p.unread > 0);
        let looking = under_way.is_some() || unread.is_some();
        if !looking && unsaid.is_empty() {
            return Ok(false);
        }
        info!("{key}: {} stops her", by.key);
        if let Some(place) = unread {
            self.record(Event::Read {
                channel: place.channel.clone(),
                place: place.id.clone(),
                upto: place.last_in_seq,
            })?;
        }
        let stopped = self.record(Event::Stopped {
            place: key.to_owned(),
            by: by.key.clone(),
            looking,
            unsaid,
            // Her work there goes on; the journal keeps the field for the
            // days it did not.
            works: Vec::new(),
        })?;
        // A look whose rounds a console can read ends with this entry, as
        // one she finished ends with what she decided.
        if let Some(looking) = &under_way {
            self.end_kept(key, looking.kept, stopped.seq);
        }
        // Something she meant to do there, and was doing when she was
        // stopped, is not taken up again unasked.
        if let Some(intention) = under_way.and_then(|l| l.intention) {
            self.pursuing.remove(&intention);
            self.record(Event::Settled {
                intention,
                outcome: format!("{} stopped you in the middle of it", by.name),
            })?;
            self.refresh_intentions()?;
        }
        let _ = now;
        Ok(true)
    }

    /// What she is told of having been stopped in `place`, the last time,
    /// if it was within the stretch of the conversation she has in front
    /// of her, `lines`; otherwise nothing.
    pub(super) fn stopped_here(
        &self,
        place: &PlaceState,
        lines: &[Line],
        now: DateTime<Utc>,
    ) -> Result<String> {
        let Some(first) = lines.first() else {
            return Ok(String::new());
        };
        let journal = self.ghost.journal();
        let last = journal
            .of_kinds_after(&["stopped"], first.seq - 1)?
            .into_iter()
            .rev()
            .find_map(|e| match e.event {
                Event::Stopped {
                    place: at,
                    by,
                    looking,
                    unsaid,
                    works,
                } if at == place.key => Some((e.at, by, looking, unsaid, works)),
                _ => None,
            });
        let Some((at, by, looking, unsaid, works)) = last else {
            return Ok(String::new());
        };
        let memory = self.ghost.memory();
        let name = match memory.person(&by)? {
            Some(person) => person.name,
            None => social::split_key(&by).map_or(by.clone(), |(_, id)| id.to_owned()),
        };
        let mut what = String::new();
        if !unsaid.is_empty() {
            let quoted: Vec<String> = unsaid.iter().map(|u| format!("\"{}\"", u.trim())).collect();
            what.push_str(
                &self
                    .prompts
                    .fill("stopped_unsaid", &[("unsaid", quoted.join(" "))]),
            );
        } else if looking {
            what.push_str(self.prompts.text("stopped_answering"));
        }
        let mut briefs = Vec::new();
        for id in &works {
            let brief = memory
                .undertaking(id)?
                .map_or_else(|| id.clone(), |u| u.brief);
            briefs.push(clip(brief.lines().next().unwrap_or_default().trim(), BRIEF));
        }
        if !briefs.is_empty() {
            what.push_str(
                &self
                    .prompts
                    .fill("stopped_work", &[("briefs", briefs.join("; "))]),
            );
        }
        Ok(self.prompts.fill(
            "stopped_here",
            &[
                ("when", capital(&when(at, now, self.life.clock()))),
                ("name", name),
                ("what", what),
            ],
        ))
    }
}
