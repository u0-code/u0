//! Acting on her own intentions: coming back to a conversation, asking how
//! something went, saying the thing she meant to say.
//!
//! This is the only way she ever writes first. An intention exists because
//! she set it, in a conversation or while looking back on a day; nothing here
//! invents a reason to talk.

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    life::{Phase, Reach},
    memory::Intention,
    will,
};
use tracing::debug;

use super::{
    Mind,
    attend::{Thinking, Why},
};
use crate::Result;

impl Mind {
    /// Work out when the next intention needs her.
    pub(super) fn refresh_intentions(&mut self) -> Result<()> {
        let open = self.ghost.memory().open_intentions()?;
        self.held
            .retain(|seq, _| open.iter().any(|i| i.seq == *seq));
        self.agenda.intention = open
            .iter()
            .filter(|i| !self.pursuing.contains(&i.seq))
            .map(|i| self.held.get(&i.seq).map_or(i.due, |held| i.due.max(*held)))
            .min();
        Ok(())
    }

    /// Set an intention, unless she already holds one for the same person or
    /// place around the same time. Returns whether it was recorded.
    pub(super) fn intend(
        &mut self,
        due: DateTime<Utc>,
        what: String,
        about: Option<String>,
        place: Option<String>,
    ) -> Result<bool> {
        let held = self.ghost.memory().open_intentions()?.iter().any(|open| {
            let same = match (&about, &open.about) {
                (Some(a), Some(b)) => a == b,
                _ => place.is_some() && place == open.place,
            };
            same && (open.due - due).abs() < Duration::hours(12)
        });
        if held {
            debug!("already means to: {what}");
            return Ok(false);
        }
        self.record(Event::Intended {
            due,
            what,
            about,
            place,
        })?;
        Ok(true)
    }

    /// What she already means to do that concerns a place or the people in it.
    pub(super) fn intentions_about(
        &self,
        place: &str,
        people: &[String],
    ) -> Result<Vec<Intention>> {
        Ok(self
            .ghost
            .memory()
            .open_intentions()?
            .into_iter()
            .filter(|i| {
                i.place.as_deref() == Some(place)
                    || i.about.as_ref().is_some_and(|a| people.contains(a))
            })
            .collect())
    }

    /// An intention has come due. Decide whether this is a moment she could
    /// act on it, and if so, look at the place it belongs to.
    pub(super) fn pursue(&mut self, now: DateTime<Utc>) -> Result<()> {
        let open = self.ghost.memory().open_intentions()?;
        let Some(intention) = open.into_iter().find(|i| {
            !self.pursuing.contains(&i.seq)
                && self.held.get(&i.seq).map_or(i.due, |held| i.due.max(*held)) <= now
        }) else {
            return self.refresh_intentions();
        };
        if let Err(wait) = self.ready_for(&intention, now)? {
            match wait {
                Wait::Until(at) => {
                    self.held.insert(intention.seq, at);
                }
                Wait::Never(why) => {
                    debug!("letting go of intention {}: {why}", intention.seq);
                    self.record(Event::Settled {
                        intention: intention.seq,
                        outcome: format!("let it go: {why}"),
                    })?;
                }
            }
            return self.refresh_intentions();
        }
        let place_key = self.place_for(&intention)?.unwrap_or_default();
        let Some(place) = self.ghost.memory().place(&place_key)? else {
            return self.refresh_intentions();
        };
        self.pursuing.insert(intention.seq);
        self.refresh_intentions()?;
        self.ponder(
            Thinking {
                place: place.key.clone(),
                upto: 0,
                unread_chars: 0,
                round: 0,
                why: Why::Intention(intention),
                recalled: None,
                cheap: false,
            },
            &place,
            now,
        )
    }

    /// Where an intention would be carried out.
    fn place_for(&self, intention: &Intention) -> Result<Option<String>> {
        if let Some(place) = &intention.place {
            return Ok(Some(place.clone()));
        }
        Ok(match &intention.about {
            Some(person) => self.ghost.memory().person(person)?.and_then(|p| p.direct),
            None => None,
        })
    }

    fn ready_for(
        &mut self,
        intention: &Intention,
        now: DateTime<Utc>,
    ) -> Result<std::result::Result<(), Wait>> {
        let memory = self.ghost.memory();
        let Some(place) = self
            .place_for(intention)?
            .map(|key| memory.place(&key))
            .transpose()?
            .flatten()
        else {
            return Ok(Err(Wait::Never("nowhere to say it")));
        };
        if let Some(person) = memory.person_of_direct(&place.key)?
            && !will::may_reach_out(&person, &will::standing(&person, now), now)
        {
            return Ok(Err(Wait::Never("she does not chase")));
        }
        if !self.channels.contains_key(&place.channel) {
            return Ok(Err(Wait::Until(now + Duration::minutes(15))));
        }
        if self.thinking.contains(&place.key) || self.agenda.speaking.contains_key(&place.key) {
            return Ok(Err(Wait::Until(now + Duration::minutes(2))));
        }
        if !self.tempo.instant() {
            let moment = self.life.moment(self.ghost.journal(), now)?;
            if moment.phase == Phase::Asleep || moment.reach >= Reach::Busy {
                let slack = Duration::seconds(i64::from(self.rng.u32(60..600)));
                return Ok(Err(Wait::Until(moment.until + slack)));
            }
        }
        if !self.budget.allow(now) {
            return Ok(Err(Wait::Until(now + Duration::minutes(10))));
        }
        Ok(Ok(()))
    }
}

enum Wait {
    Until(DateTime<Utc>),
    Never(&'static str),
}
