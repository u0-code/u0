//! Answering observers what only her mind knows: who she is this moment,
//! and what is on her agenda. Everything that is read from her journal,
//! her memory or her work's records is answered off her mind, by
//! [`super::reader`]. Nothing here can change her.

use chrono::{DateTime, Utc};
use ghost_protocol::{Abilities, GhostStatus, MachineInfo};
use serde_json::{Value, json};

use super::{Mind, Question};
use crate::Result;

impl Mind {
    pub(super) fn answer(&self, question: &Question) -> Value {
        match question {
            Question::Agenda => self.agenda_view(),
            Question::Identity => json!({
                "seed": self.ghost.identity().seed(),
                "seal": self.ghost.identity().seal(),
            }),
        }
    }

    pub(super) fn status(&self, now: DateTime<Utc>) -> Result<GhostStatus> {
        let identity = self.ghost.identity();
        let seed = identity.seed();
        let journal = self.ghost.journal();
        let moment = self.life.moment(journal, now)?;
        let tz = self.life.clock().tz();
        let mut channels: Vec<String> = self.channels.keys().cloned().collect();
        channels.sort();
        Ok(GhostStatus {
            name: seed.name.clone(),
            pronouns: seed.pronouns.clone(),
            city: seed.home.city.clone(),
            country: seed.home.country.clone(),
            seal: identity.seal().to_string(),
            timezone: tz.name().to_owned(),
            local_time: now.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
            awake: moment.phase == ghost_core::life::Phase::Awake,
            doing: moment
                .doing
                .map(|doing| doing.what.trim().to_owned())
                .filter(|what| !what.is_empty()),
            journal: journal.last_seq()?,
            channels,
            tempo: self.tempo.scale(),
            think: self.think.label(),
            tick: self.tick.label(),
            think_window: self.think.window(),
            tick_window: self.tick.window(),
            calls_this_hour: u32::try_from(self.budget.spent.len()).unwrap_or(u32::MAX),
            abilities: self.abilities(),
            core: None,
        })
    }

    /// What she can do besides talk: a console offers that, and nothing it
    /// would have to read her daemon's configuration to know.
    fn abilities(&self) -> Abilities {
        let work = &self.cfg.work;
        // Only a machine that was made counts, by the kind its spec names.
        let machine = self.workshop.machine.as_ref().and(work.machine.as_deref());
        let machine = machine.map(|spec| {
            let spec = spec.trim();
            let kind = spec.split_once(':').map_or(spec, |(kind, _)| kind);
            MachineInfo {
                kind: kind.to_owned(),
                spec: spec.to_owned(),
                dir: self
                    .workshop
                    .local
                    .as_ref()
                    .map(|dir| dir.display().to_string()),
            }
        });
        Abilities {
            work: self.can_work(),
            machine,
            web: work.web,
            agents: self
                .workshop
                .agents
                .iter()
                .map(|a| a.name.clone())
                .collect(),
        }
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
        let mut thinking: Vec<&String> = self.jobs.looks().map(|(place, _)| place).collect();
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
}
