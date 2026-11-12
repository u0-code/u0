//! Answering observers. Read-only: nothing here can change her.

use chrono::{DateTime, Utc};
use ghost_core::will;
use serde_json::{Value, json};

use super::{Mind, Question};
use crate::Result;

impl Mind {
    pub(super) fn answer(&self, question: &Question, now: DateTime<Utc>) -> Result<Value> {
        Ok(match question {
            Question::Status => self.status(now)?,
            Question::Agenda => self.agenda_view(),
            Question::People => self.people_view(now)?,
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
        })
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
