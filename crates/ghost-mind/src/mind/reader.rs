//! Answering whoever looks at her from outside, off her mind.
//!
//! A console polls her journal, her work and the rounds of her looks every
//! second or so while she thinks. Answered on her mind, each question would
//! wait behind what she is doing and hold up what she does next. So they
//! are answered here instead, on connections of their own opened to read:
//! her journal and her memory as she last wrote them, and her work's
//! records. SQLite's WAL lets these read while she writes, without waiting
//! on her or making her wait. Nothing here can change her.
//!
//! What only her mind knows, which sittings and looks are under way and
//! which channels she can be reached on, it publishes as [`Live`] each
//! time it has handled something; that is read here as it last said.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use chrono::{DateTime, Utc};
use ghost_core::{
    journal::{Entry, JOURNAL_FILE, Journal},
    memory::{Memory, Session, Undertaking},
    will,
};
use ghost_protocol as wire;
use ghost_work::undertaking::{Look, Outcome, Records};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::{Error, Operators, Result};

/// How many pieces of work that have ended are shown beside those still
/// open.
const ENDED: usize = 20;
/// The most steps of a piece of work one question is answered with.
const STEPS_MOST: usize = 500;
/// The most journal entries one question is answered with.
const ENTRIES_MOST: usize = 1000;
/// The most entries of one place one question is answered with.
const THREAD_MOST: usize = 2000;
/// The file of her work's records in her directory.
const WORK_DB: &str = "work.db";

/// What her mind is in the middle of, as it last said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Live {
    /// The work with a sitting at it, by id.
    pub sittings: BTreeSet<String>,
    /// The places she is looking at, each with when its look began keeping
    /// rounds, once it has kept one.
    pub looks: BTreeMap<String, Option<DateTime<Utc>>>,
    /// The channels she can be reached on.
    pub online: BTreeSet<String>,
}

/// Her journal, memory and work's records, opened to read. Each is opened
/// the first time it is wanted, and opened again while it cannot be: her
/// memory while it is being rebuilt, her work's records until there are
/// any.
pub struct Reader {
    dir: PathBuf,
    operators: Operators,
    live: watch::Receiver<Live>,
    journal: Option<Journal>,
    memory: Option<Memory>,
    records: Option<Records>,
}

/// Everything a question is answered from, open.
pub(super) struct Open<'a> {
    journal: &'a Journal,
    memory: &'a Memory,
    records: Option<&'a Records>,
    live: Live,
    operators: &'a Operators,
}

impl Reader {
    pub(crate) fn new(dir: PathBuf, operators: Operators, live: watch::Receiver<Live>) -> Self {
        Self {
            dir,
            operators,
            live,
            journal: None,
            memory: None,
            records: None,
        }
    }

    fn open(&mut self) -> Result<Open<'_>> {
        let Self {
            dir,
            operators,
            live,
            journal,
            memory,
            records,
        } = self;
        let journal = match journal {
            Some(journal) => journal,
            none => none.insert(Journal::open(&dir.join(JOURNAL_FILE))?),
        };
        let memory = match memory {
            Some(memory) => memory,
            none => none.insert(
                Memory::glance(dir)?
                    .ok_or_else(|| Error::Record("her memory is being rebuilt".into()))?,
            ),
        };
        let db = dir.join(WORK_DB);
        // A reader does not make the database: until her mind has made
        // it, there is nothing to read.
        if records.is_none() && db.exists() {
            match Records::open(&db) {
                Ok(open) => *records = Some(open),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Record(e.to_string())),
            }
        }
        Ok(Open {
            journal,
            memory,
            records: records.as_ref(),
            live: live.borrow().clone(),
            operators,
        })
    }

    /// Journal entries from seq `from` on, at most `limit`. Zero or less
    /// counts back from the newest entry.
    pub fn journal(&mut self, from: i64, limit: usize) -> Result<Vec<Entry>> {
        let journal = self.open()?.journal;
        let from = if from > 0 {
            from
        } else {
            (journal.last_seq()? + from).max(1)
        };
        Ok(journal.read(from, limit.min(ENTRIES_MOST))?)
    }

    /// Everything in her journal that belongs to one place: what was said
    /// there, and what she read, decided, felt, noted and intended about
    /// it, as her memory sorted it when each entry was written.
    pub fn thread(&mut self, place: &str, limit: usize) -> Result<Vec<Entry>> {
        let open = self.open()?;
        let seqs = open.memory.thread_of(place, limit.clamp(1, THREAD_MOST))?;
        Ok(open.journal.entries_at(&seqs)?)
    }

    /// What is true about her life, oldest first.
    pub fn canon(&mut self) -> Result<Value> {
        Ok(json!(self.open()?.memory.canon(500)?))
    }

    /// Her conversations on `channel`, newest first.
    pub fn sessions(&mut self, channel: &str) -> Result<Vec<Session>> {
        Ok(self.open()?.memory.sessions(channel)?)
    }

    /// Every place she is in, most recently active first, each with who is
    /// on the other side of it if it is private, and whether she can be
    /// reached there now.
    pub fn places(&mut self) -> Result<Value> {
        let open = self.open()?;
        let mut places = Vec::new();
        for place in open.memory.places()? {
            let with = open.memory.person_of_direct(&place.key)?.map(|p| p.name);
            let online = open.live.online.contains(&place.channel);
            places.push(json!({"place": place, "with": with, "online": online}));
        }
        Ok(json!(places))
    }

    /// Everyone she knows, where she stands with them at `now`, and what
    /// she noted of them lately.
    pub fn people(&mut self, now: DateTime<Utc>) -> Result<Value> {
        let memory = self.open()?.memory;
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
    /// own record.
    pub fn work(&mut self) -> Result<Vec<wire::Work>> {
        let open = self.open()?;
        let mut taken = open.memory.undertakings(true, 500)?;
        taken.extend(
            open.memory
                .undertakings(false, 500)?
                .into_iter()
                .filter(|u| u.outcome.is_some())
                .take(ENDED),
        );
        let ids: Vec<String> = taken.iter().map(|u| u.id.clone()).collect();
        let looks = open.records_of(&ids)?;
        let mut works: Vec<(DateTime<Utc>, wire::Work)> = taken
            .iter()
            .map(|u| open.work_of(u, looks.iter().find(|l| l.id == u.id)))
            .collect();
        works.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        Ok(works.into_iter().map(|(_, w)| w).collect())
    }

    /// The steps of the work `id`, or of a part of it, after the `after`th,
    /// at most `limit` of them, with the parts it handed out. `None` if
    /// there is no work by that id.
    pub fn steps(&mut self, id: &str, after: u64, limit: usize) -> Result<Option<wire::WorkLog>> {
        let open = self.open()?;
        let Some(records) = open.records else {
            return Ok(None);
        };
        let Some(logged) = records
            .steps(id, after, limit.clamp(1, STEPS_MOST))
            .map_err(|e| Error::Record(e.to_string()))?
        else {
            return Ok(None);
        };
        let looked = open.records_of(&[id.to_owned()])?;
        // A part is at it while the sitting of the work it belongs to is.
        let top = id.split('/').next().unwrap_or(id);
        let running = open.live.sittings.contains(top);
        let parts = looked
            .first()
            .map(|look| {
                look.parts
                    .iter()
                    .map(|(_, part)| wire::WorkPart {
                        id: part.id.clone(),
                        brief: part.state.brief.clone(),
                        state: state_of(part, running),
                        brain: brain_of(part).map(str::to_owned),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(wire::WorkLog {
            steps: as_wire(&logged.steps)?,
            parts,
            more: logged.more,
            context: logged.context.as_ref().map(as_wire).transpose()?,
        }))
    }

    /// The looks at `place` that ended after the journal entry `after`,
    /// and the one going on there, each with its rounds.
    pub fn looks(&mut self, place: &str, after: i64) -> Result<Vec<wire::Look>> {
        let open = self.open()?;
        // When the look in flight there began keeping rounds: the record
        // has it as the one begun since then that has not ended.
        let going = open.live.looks.get(place).copied().flatten();
        let Some(records) = open.records else {
            return Ok(Vec::new());
        };
        let kept = records
            .looks(place, after, going)
            .map_err(|e| Error::Record(e.to_string()))?;
        kept.into_iter()
            .map(|look| {
                Ok(wire::Look {
                    look: u64::try_from(look.id).unwrap_or_default(),
                    seq: look.seq,
                    running: look.seq.is_none(),
                    rounds: as_wire(&look.rounds)?,
                })
            })
            .collect()
    }

    /// What is waiting on the people who run her, newest first.
    pub fn waiting(&mut self, now: DateTime<Utc>) -> Result<Vec<wire::Attention>> {
        self.open()?.waiting(now)
    }
}

impl Open<'_> {
    /// The records of the work `ids` as they are now, each with its parts,
    /// in that order. Work with no record is left out.
    pub(super) fn records_of(&self, ids: &[String]) -> Result<Vec<Look>> {
        let Some(records) = self.records else {
            return Ok(Vec::new());
        };
        records.look(ids).map_err(|e| Error::Record(e.to_string()))
    }

    /// Whether `channel:id` is an account of whoever runs her.
    pub(super) fn operator(&self, channel: &str, id: &str) -> bool {
        self.operators.includes(channel, id)
    }

    pub(super) fn journal(&self) -> &Journal {
        self.journal
    }

    pub(super) fn memory(&self) -> &Memory {
        self.memory
    }

    /// One piece of work as she knows it, with what its record adds, and
    /// when anything last happened in it.
    fn work_of(&self, u: &Undertaking, look: Option<&Look>) -> (DateTime<Utc>, wire::Work) {
        let running = self.live.sittings.contains(&u.id);
        let state = match (&u.outcome, look) {
            (Some(outcome), _) => {
                serde_json::from_value(json!(outcome)).unwrap_or(wire::WorkState::Other)
            }
            (None, Some(look)) => state_of(look, running),
            (None, None) if u.waiting.is_some() => wire::WorkState::Waiting,
            (None, None) if running => wire::WorkState::Running,
            (None, None) => wire::WorkState::Open,
        };
        let mut last = u.last_at;
        let mut work = wire::Work {
            id: u.id.clone(),
            seq: Some(u.seq),
            brief: u.brief.clone(),
            state,
            steps: 0,
            started: Some(stamp(u.at)),
            last: None,
            question: u.waiting.clone(),
            kind: None,
            options: Vec::new(),
            summary: u.summary.clone(),
            plan: Vec::new(),
            parts: Vec::new(),
            brain: None,
        };
        if let Some(look) = look {
            last = last_of(look).map_or(last, |at| at.max(last));
            let own = seen(look, running);
            work.steps = own.steps;
            work.plan = own.plan;
            work.brain = own.brain;
            work.parts = look
                .parts
                .iter()
                .map(|(_, part)| seen(part, running))
                .collect();
            if u.waiting.is_none() {
                work.question = own.question;
            }
            // What answering takes is only in the work's own record.
            work.kind = own.kind;
            work.options = own.options;
            if u.summary.is_none() {
                work.summary = own.summary;
            }
        }
        work.last = Some(stamp(last));
        (last, work)
    }
}

/// A piece of work or a part as its own record has it. A part is running
/// while the sitting it belongs to is.
fn seen(look: &Look, running: bool) -> wire::Work {
    let state = &look.state;
    wire::Work {
        id: look.id.clone(),
        seq: None,
        brief: state.brief.clone(),
        state: state_of(look, running),
        steps: state.steps,
        started: look.first_step.map(stamp),
        last: last_of(look).map(stamp),
        question: state.waiting.clone(),
        kind: state
            .waiting_kind
            .and_then(|kind| serde_json::from_value(json!(kind)).ok()),
        options: state.waiting_options.clone(),
        summary: state.finish.as_ref().map(|f| f.summary.clone()),
        plan: state
            .plan
            .iter()
            .map(|step| wire::WorkStep {
                text: step.text.clone(),
                status: serde_json::to_value(step.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                part: step.part,
            })
            .collect(),
        parts: Vec::new(),
        brain: brain_of(look).map(str::to_owned),
    }
}

/// Where a piece of work stands by its own record. A part is running
/// while the sitting it belongs to is.
fn state_of(look: &Look, running: bool) -> wire::WorkState {
    let state = &look.state;
    match state.finish.as_ref().map(|f| f.outcome) {
        Some(Outcome::Finished) => wire::WorkState::Finished,
        Some(Outcome::Blocked) => wire::WorkState::Blocked,
        Some(Outcome::GaveUp) => wire::WorkState::GaveUp,
        Some(Outcome::Dropped) => wire::WorkState::Dropped,
        None if state.waiting.is_some() => wire::WorkState::Waiting,
        None if running => wire::WorkState::Running,
        None => wire::WorkState::Open,
    }
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

/// What steps a piece of work or a part: the agent it was handed to, or
/// else the brain that stepped it last.
fn brain_of(look: &Look) -> Option<&str> {
    look.state.agent.as_deref().or(look.state.brain.as_deref())
}

/// A time as the protocol writes it.
pub(super) fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// What her work keeps, in the protocol's type for it. Her work's record
/// writes each step and round in the shape the protocol reads; this is
/// where that is checked, so that a record that drifted from it is an
/// error a console is told, not a field it silently goes without.
fn as_wire<T: serde::de::DeserializeOwned>(kept: &impl serde::Serialize) -> Result<T> {
    serde_json::to_value(kept)
        .and_then(serde_json::from_value)
        .map_err(|e| {
            Error::Record(format!(
                "her work's record is not in the protocol's shape: {e}"
            ))
        })
}
