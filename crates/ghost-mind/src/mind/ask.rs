//! Answering observers. Read-only: nothing here can change her.

use chrono::{DateTime, Utc};
use ghost_core::{memory::Undertaking, will};
use ghost_work::undertaking::{Look, Outcome};
use serde_json::{Value, json};
use tracing::warn;

use super::{Mind, Question};
use crate::Result;

/// How many pieces of work that have ended are shown beside those still
/// open.
const ENDED: usize = 20;
/// The most steps of a piece of work one question is answered with.
const STEPS_MOST: usize = 500;

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
            Question::Work => self.work_view()?,
            Question::WorkSteps { work, after, limit } => self.steps_view(work, *after, *limit),
            Question::LookSteps { place, after } => self.looks_view(place, *after),
            Question::Waiting => self.waiting_view(now)?,
            Question::Sessions => json!(self.ghost.memory().sessions(crate::CONSOLE)?),
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
            // Each brain's context window in tokens, where it is known: a
            // console shows how full her looks keep it against this.
            "windows": {"think": self.think.window(), "tick": self.tick.window()},
            "calls_this_hour": self.budget.spent.len(),
            "abilities": self.abilities(),
        }))
    }

    /// What she can do besides talk, in the shape of the protocol's
    /// `Abilities`: a console offers that, and nothing it would have to
    /// read her daemon's configuration to know.
    fn abilities(&self) -> Value {
        let work = &self.cfg.work;
        // Only a machine that was made counts, by the kind its spec names.
        let machine = self.workshop.machine.as_ref().and(work.machine.as_deref());
        let machine = machine.map(|spec| {
            let spec = spec.trim();
            let kind = spec.split_once(':').map_or(spec, |(kind, _)| kind);
            json!({"kind": kind, "spec": spec, "dir": self.workshop.local})
        });
        let agents: Vec<&str> = self
            .workshop
            .agents
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        json!({
            "work": self.can_work(),
            "machine": machine,
            "web": work.web,
            "agents": agents,
        })
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
        let mut thinking: Vec<&String> = self.thinking.keys().collect();
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
    /// there, and what she read, decided, felt, noted and intended about
    /// it, as her memory sorted it when each entry was written.
    fn thread_view(&self, key: &str, limit: usize) -> Result<Value> {
        let seqs = self.ghost.memory().thread_of(key, limit.clamp(1, 2000))?;
        Ok(json!(self.ghost.journal().entries_at(&seqs)?))
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

    /// What she has taken on: every piece still open and the latest that
    /// ended, most recently active first. That she took it on, and how it
    /// ended, is her journal's; its steps, plan and parts are in the work's
    /// own record, which is only read here.
    fn work_view(&self) -> Result<Value> {
        let memory = self.ghost.memory();
        let mut taken = memory.undertakings(true, 500)?;
        taken.extend(
            memory
                .undertakings(false, 500)?
                .into_iter()
                .filter(|u| u.outcome.is_some())
                .take(ENDED),
        );
        let ids: Vec<String> = taken.iter().map(|u| u.id.clone()).collect();
        let looks = self.records_of(&ids);
        let mut works: Vec<(DateTime<Utc>, Value)> = taken
            .iter()
            .map(|u| self.work_of(u, looks.iter().find(|l| l.id == u.id)))
            .collect();
        works.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        Ok(json!(works.into_iter().map(|(_, w)| w).collect::<Vec<_>>()))
    }

    /// The steps of the work `id`, or of a part of it, after the `after`th,
    /// at most `limit` of them, with the parts it handed out: all from the
    /// work's own record, read without writing. `null` if there is no work
    /// by that id.
    fn steps_view(&self, id: &str, after: u64, limit: usize) -> Value {
        let Some(records) = self.records() else {
            return Value::Null;
        };
        let logged = match records.steps(id, after, limit.clamp(1, STEPS_MOST)) {
            Ok(Some(logged)) => logged,
            Ok(None) => return Value::Null,
            Err(e) => {
                warn!("could not read the steps of {id}: {e}");
                return Value::Null;
            }
        };
        let looked = self.records_of(&[id.to_owned()]);
        // A part is at it while the sitting of the work it belongs to is.
        let top = id.split('/').next().unwrap_or(id);
        let running = self.working.contains_key(top);
        let parts: Vec<Value> = looked
            .first()
            .map(|look| {
                look.parts
                    .iter()
                    .map(|(_, part)| {
                        json!({
                            "id": part.id,
                            "brief": part.state.brief,
                            "state": state_of(part, running),
                            "brain": brain_of(part),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({
            "steps": logged.steps,
            "parts": parts,
            "more": logged.more,
            "context": logged.context,
        })
    }

    /// The looks at `place` that ended after the journal entry `after`,
    /// and the one going on there, each with its rounds.
    fn looks_view(&self, place: &str, after: i64) -> Value {
        // The look in flight there, as the record knows it: the same look
        // her mind is waiting on, numbered where its rounds are kept.
        let open = self.thinking.get(place).and_then(|looking| looking.kept);
        let kept = self.records().map_or_else(Vec::new, |records| {
            records.looks(place, after, open).unwrap_or_else(|e| {
                warn!("could not read her looks at {place}: {e}");
                Vec::new()
            })
        });
        json!(
            kept.into_iter()
                .map(|look| json!({
                    "look": look.id,
                    "seq": look.seq,
                    "running": look.seq.is_none() && Some(look.id) == open,
                    "rounds": look.rounds,
                }))
                .collect::<Vec<_>>()
        )
    }

    /// One piece of work as she knows it, with what its record adds, and
    /// when anything last happened in it.
    fn work_of(&self, u: &Undertaking, look: Option<&Look>) -> (DateTime<Utc>, Value) {
        let running = self.working.contains_key(&u.id);
        let state = match (&u.outcome, look) {
            (Some(outcome), _) => outcome.clone(),
            (None, Some(look)) => state_of(look, running),
            (None, None) if u.waiting.is_some() => "waiting".to_owned(),
            (None, None) if running => "running".to_owned(),
            (None, None) => "open".to_owned(),
        };
        let mut last = u.last_at;
        let mut work = json!({
            "id": u.id,
            "seq": u.seq,
            "brief": u.brief,
            "state": state,
            "steps": 0,
            "started": u.at,
            "question": u.waiting,
            "summary": u.summary,
        });
        if let Some(look) = look {
            last = last_of(look).map_or(last, |at| at.max(last));
            let parts: Vec<Value> = look
                .parts
                .iter()
                .map(|(_, part)| {
                    let mut seen = seen(part);
                    seen["state"] = json!(state_of(part, running));
                    seen
                })
                .collect();
            let seen = seen(look);
            work["steps"] = seen["steps"].clone();
            work["plan"] = seen["plan"].clone();
            work["brain"] = seen["brain"].clone();
            work["parts"] = json!(parts);
            if u.waiting.is_none() {
                work["question"] = seen["question"].clone();
            }
            // What answering takes is only in the work's own record.
            work["kind"] = seen["kind"].clone();
            work["options"] = seen["options"].clone();
            if u.summary.is_none() {
                work["summary"] = seen["summary"].clone();
            }
        }
        work["last"] = json!(last);
        (last, work)
    }
}

/// Where a piece of work stands by its own record. A part is running
/// while the sitting it belongs to is.
fn state_of(look: &Look, running: bool) -> String {
    let state = &look.state;
    match state.finish.as_ref().map(|f| f.outcome) {
        Some(Outcome::Finished) => "finished",
        Some(Outcome::Blocked) => "blocked",
        Some(Outcome::GaveUp) => "gave_up",
        Some(Outcome::Dropped) => "dropped",
        None if state.waiting.is_some() => "waiting",
        None if running => "running",
        None => "open",
    }
    .to_owned()
}

/// When anything last happened in a piece of work, by its own record.
fn last_of(look: &Look) -> Option<DateTime<Utc>> {
    let state = &look.state;
    [
        look.last_step,
        state.finish.as_ref().map(|f| f.at),
        state.told.last().map(|t| t.at),
        state.notes.last().map(|n| n.at),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// A piece of work or a part as its own record has it, with no state: who
/// is at it is not in the record.
fn seen(look: &Look) -> Value {
    let state = &look.state;
    let plan: Vec<Value> = state
        .plan
        .iter()
        .map(|step| json!({"text": step.text, "status": step.status, "part": step.part}))
        .collect();
    json!({
        "id": look.id,
        "brief": state.brief,
        "steps": state.steps,
        "started": look.first_step,
        "last": last_of(look),
        "question": state.waiting,
        "kind": state.waiting_kind,
        "options": state.waiting_options,
        "summary": state.finish.as_ref().map(|f| &f.summary),
        "plan": plan,
        "brain": brain_of(look),
    })
}

/// What steps a piece of work or a part: the agent it was handed to, or
/// else the brain that stepped it last.
fn brain_of(look: &Look) -> Option<&str> {
    look.state.agent.as_deref().or(look.state.brain.as_deref())
}
