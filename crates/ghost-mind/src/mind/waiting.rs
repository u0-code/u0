//! What is waiting on the people who run her: the answer to a console's
//! `attention.list`. Read-only, and made afresh from her journal, her
//! memory and her work's records each time it is asked. Which of it a
//! person has already seen is theirs to keep, by each item's id.
//!
//! - **question**: open work waiting on an answer, with what answering
//!   takes. It stays as long as the work waits, however long ago it asked.
//! - **ended**: work that ended blocked or given up lately, with why.
//! - **told**: what work done for an operator, or directed by one, told
//!   her lately while it ran.
//! - **report**: what she said lately when she told an operator about her
//!   day, as their report.
//!
//! "Lately" is [`DAYS`] days.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::{Entry, Event},
    social,
};
use ghost_protocol::{Attention, AttentionKind};

use super::reader::{Open, stamp};
use crate::Result;

/// How many days back what has ended, been told and been reported is shown.
pub(super) const DAYS: i64 = 3;

/// A report she is in the middle of saying: the intention it carries out,
/// and what she has said of it so far.
struct Saying {
    intention: i64,
    at: Option<DateTime<Utc>>,
    lines: Vec<String>,
}

impl Open<'_> {
    /// Everything waiting on the people who run her, newest first.
    pub(super) fn waiting(&self, now: DateTime<Utc>) -> Result<Vec<Attention>> {
        let mut items: Vec<(DateTime<Utc>, Attention)> = self.questions()?;
        items.extend(self.lately(now - Duration::days(DAYS))?);
        items.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        Ok(items.into_iter().map(|(_, item)| item).collect())
    }

    /// Each piece of open work waiting on an answer, by the entry that
    /// asked, with what answering it takes from its own record.
    fn questions(&self) -> Result<Vec<(DateTime<Utc>, Attention)>> {
        let memory = self.memory();
        let waiting: Vec<_> = memory
            .undertakings(true, 500)?
            .into_iter()
            .filter(|u| u.waiting.is_some())
            .collect();
        if waiting.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = waiting.iter().map(|u| u.id.clone()).collect();
        let looks = self.records_of(&ids)?;
        let mut items = Vec::new();
        for u in waiting {
            let asked = self
                .journal()
                .of_kinds_after(&["asked"], u.seq)?
                .into_iter()
                .rev()
                .find(|e| matches!(&e.event, Event::Asked { id, .. } if *id == u.id));
            let Some(asked) = asked else { continue };
            let state = looks.iter().find(|l| l.id == u.id).map(|l| &l.state);
            items.push((
                asked.at,
                Attention {
                    ask: state
                        .and_then(|s| s.waiting_kind)
                        .and_then(|kind| serde_json::from_value(serde_json::json!(kind)).ok()),
                    options: state.map(|s| s.waiting_options.clone()).unwrap_or_default(),
                    work: Some(u.id),
                    brief: Some(u.brief),
                    text: u.waiting.unwrap_or_default(),
                    ..item(AttentionKind::Question, "asked", asked.seq, asked.at)
                },
            ));
        }
        Ok(items)
    }

    /// What ended badly, was told, and was reported since `since`.
    fn lately(&self, since: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, Attention)>> {
        let memory = self.memory();
        let journal = self.journal();
        // Only the days asked about are read, however long she has lived.
        let from = journal.first_seq_since(since)?;
        let entries: Vec<Entry> = journal
            .of_kinds_after(
                &[
                    "concluded",
                    "reported",
                    "intended",
                    "settled",
                    "said",
                    "heard",
                    "decided",
                ],
                from - 1,
            )?
            .into_iter()
            .filter(|e| e.at >= since)
            .collect();
        let mut items = Vec::new();
        // Report intentions by seq, with the place each is to be said in.
        let mut reports: HashMap<i64, String> = HashMap::new();
        // Reports being said, by place.
        let mut saying: HashMap<String, Saying> = HashMap::new();
        let mut done: Vec<(String, Saying)> = Vec::new();
        for entry in entries {
            match entry.event {
                Event::Concluded {
                    id,
                    outcome,
                    summary,
                } if outcome == "blocked" || outcome == "gave_up" => {
                    let brief = memory.undertaking(&id)?.map(|u| u.brief);
                    items.push((
                        entry.at,
                        Attention {
                            work: Some(id),
                            brief,
                            text: summary,
                            outcome: Some(outcome),
                            ..item(AttentionKind::Ended, "ended", entry.seq, entry.at)
                        },
                    ));
                }
                Event::Reported { id, text, files } => {
                    let Some(u) = memory.undertaking(&id)? else {
                        continue;
                    };
                    let operators = u.directed
                        || u.asked_by
                            .as_deref()
                            .and_then(social::split_key)
                            .is_some_and(|(channel, who)| self.operator(channel, who));
                    if !operators {
                        continue;
                    }
                    items.push((
                        entry.at,
                        Attention {
                            work: Some(id),
                            brief: Some(u.brief),
                            text,
                            files,
                            ..item(AttentionKind::Told, "told", entry.seq, entry.at)
                        },
                    ));
                }
                Event::Intended {
                    report: true,
                    place: Some(place),
                    ..
                } => {
                    reports.insert(entry.seq, place);
                }
                Event::Settled { intention, .. } => {
                    if let Some(place) = reports.get(&intention) {
                        if let Some(before) = saying.remove(place) {
                            done.push((place.clone(), before));
                        }
                        saying.insert(
                            place.clone(),
                            Saying {
                                intention,
                                at: None,
                                lines: Vec::new(),
                            },
                        );
                    }
                }
                Event::Said {
                    channel,
                    place,
                    text,
                    ..
                } => {
                    if let Some(report) = saying.get_mut(&social::key(&channel, &place)) {
                        report.at.get_or_insert(entry.at);
                        report.lines.push(text);
                    }
                }
                // Anything else that happens there, after she has spoken,
                // is the conversation going on, not the report.
                Event::Heard { channel, place, .. } => {
                    let key = social::key(&channel, &place.id);
                    if let Some(report) = saying.remove(&key) {
                        done.push((key, report));
                    }
                }
                Event::Decided { about, .. } => {
                    if saying.get(&about).is_some_and(|r| !r.lines.is_empty())
                        && let Some(report) = saying.remove(&about)
                    {
                        done.push((about, report));
                    }
                }
                _ => {}
            }
        }
        done.extend(saying);
        for (place, report) in done {
            let Some(at) = report.at else { continue };
            items.push((
                at,
                Attention {
                    id: format!("report-{}", report.intention),
                    kind: AttentionKind::Report,
                    at: stamp(at),
                    seq: report.intention,
                    work: None,
                    brief: None,
                    text: report.lines.join("\n"),
                    ask: None,
                    options: Vec::new(),
                    outcome: None,
                    files: Vec::new(),
                    place: Some(place),
                },
            ));
        }
        Ok(items)
    }
}

/// An item of `kind` that comes of the journal entry `seq`, written `at`,
/// known by `name` and that seq, with nothing else said of it yet.
fn item(kind: AttentionKind, name: &str, seq: i64, at: DateTime<Utc>) -> Attention {
    Attention {
        id: format!("{name}-{seq}"),
        kind,
        at: stamp(at),
        seq,
        work: None,
        brief: None,
        text: String::new(),
        ask: None,
        options: Vec::new(),
        outcome: None,
        files: Vec::new(),
        place: None,
    }
}
