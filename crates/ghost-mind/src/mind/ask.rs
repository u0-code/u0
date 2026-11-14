//! Answering observers. Read-only: nothing here can change her.

use chrono::{DateTime, Utc};
use ghost_core::{
    journal::{Entry, Event},
    social, will,
};
use serde_json::{Value, json};

use super::{Mind, Question};
use crate::Result;

/// How far back in her journal a place's thread is looked for.
const SCAN: usize = 5000;

impl Mind {
    pub(super) fn answer(&self, question: &Question, now: DateTime<Utc>) -> Result<Value> {
        Ok(match question {
            Question::Status => self.status(now)?,
            Question::Agenda => self.agenda_view(),
            Question::People => self.people_view(now)?,
            Question::Identity => json!({
                "seed": self.ghost.identity().seed(),
                "seal": self.ghost.identity().seal(),
            }),
            Question::Canon => json!(self.ghost.memory().canon(500)?),
            Question::Places => self.places_view()?,
            Question::Thread { place, limit } => self.thread_view(place, *limit)?,
            Question::Journal { from, limit } => {
                let journal = self.ghost.journal();
                // Zero or less counts back from the newest entry.
                let from = if *from > 0 {
                    *from
                } else {
                    (journal.last_seq()? + from).max(1)
                };
                json!(journal.read(from, (*limit).min(1000))?)
            }
        })
    }

    fn status(&self, now: DateTime<Utc>) -> Result<Value> {
        let identity = self.ghost.identity();
        let seed = identity.seed();
        let journal = self.ghost.journal();
        let moment = self.life.moment(journal, now)?;
        let tz = self.life.clock().tz();
        let mut channels: Vec<Value> = self
            .channels
            .iter()
            .map(|(name, c)| json!({"channel": name, "me": c.me, "can": c.can}))
            .collect();
        channels.sort_by_key(|c| c["channel"].as_str().unwrap_or_default().to_owned());
        Ok(json!({
            "name": seed.name,
            "pronouns": seed.pronouns,
            "city": seed.home.city,
            "country": seed.home.country,
            "seal": identity.seal(),
            "timezone": tz.name(),
            "local_time": now.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
            "moment": moment,
            "today": self.life.today(journal, now)?,
            "journal": journal.last_seq()?,
            "channels": channels,
            "tempo": self.tempo.scale(),
            "brains": {"think": self.think.label(), "tick": self.tick.label()},
            "calls_this_hour": self.budget.spent.len(),
        }))
    }

    fn agenda_view(&self) -> Value {
        let mut attend: Vec<Value> = self
            .agenda
            .attend
            .iter()
            .map(|(place, a)| json!({"place": place, "due": a.due, "addressed": a.addressed}))
            .collect();
        attend.sort_by_key(|a| a["due"].as_str().unwrap_or_default().to_owned());
        let mut speaking: Vec<&String> = self.agenda.speaking.keys().collect();
        speaking.sort();
        let mut thinking: Vec<&String> = self.thinking.iter().collect();
        thinking.sort();
        json!({
            "attend": attend,
            "thinking": thinking,
            "speaking": speaking,
            "life": self.agenda.life,
            "intention": self.agenda.intention,
            "phone": self.agenda.phone,
            "report": self.agenda.report,
        })
    }

    fn places_view(&self) -> Result<Value> {
        let memory = self.ghost.memory();
        let mut places = Vec::new();
        for place in memory.places()? {
            // A private place is known by who is on the other side of it.
            let with = memory.person_of_direct(&place.key)?.map(|p| p.name);
            let online = self.channels.contains_key(&place.channel);
            places.push(json!({"place": place, "with": with, "online": online}));
        }
        Ok(json!(places))
    }

    /// Everything in her journal that belongs to one place: what was said
    /// there, and what she read, decided, felt, noted and intended about it.
    fn thread_view(&self, key: &str, limit: usize) -> Result<Value> {
        let memory = self.ghost.memory();
        let Some((channel, id)) = social::split_key(key) else {
            return Ok(json!([]));
        };
        let other = memory.person_of_direct(key)?.map(|p| p.key);
        let said_here = format!("said in {key}");
        let here = |c: &str, p: &str| c == channel && p == id;
        let mut intentions: Vec<i64> = Vec::new();
        let mut heard: Vec<i64> = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        for entry in self.ghost.journal().tail(SCAN)? {
            let belongs = match &entry.event {
                Event::Heard { channel, place, .. } => {
                    let mine = here(channel, &place.id);
                    if mine {
                        heard.push(entry.seq);
                    }
                    mine
                }
                Event::Viewed { message, .. } => heard.contains(message),
                Event::Read { channel, place, .. }
                | Event::Said { channel, place, .. }
                | Event::Reacted { channel, place, .. } => here(channel, place),
                Event::Decided { about, .. } => about == key,
                Event::Intended { place, about, .. } => {
                    let mine =
                        place.as_deref() == Some(key) || (about.is_some() && *about == other);
                    if mine {
                        intentions.push(entry.seq);
                    }
                    mine
                }
                Event::Settled { intention, .. } => intentions.contains(intention),
                Event::Felt { toward: person, .. }
                | Event::Noted { about: person, .. }
                | Event::Left { person, .. } => other.as_deref() == Some(person.as_str()),
                Event::Canon { source, .. } => *source == said_here,
                _ => false,
            };
            if belongs {
                entries.push(entry);
            }
        }
        let skip = entries.len().saturating_sub(limit.clamp(1, 2000));
        Ok(json!(entries[skip..]))
    }

    fn people_view(&self, now: DateTime<Utc>) -> Result<Value> {
        let memory = self.ghost.memory();
        let mut people = Vec::new();
        for person in memory.people()? {
            let standing = will::standing(&person, now);
            let notes = memory.notes(&person.key, 20)?;
            people.push(json!({"person": person, "standing": standing, "notes": notes}));
        }
        Ok(json!(people))
    }
}
