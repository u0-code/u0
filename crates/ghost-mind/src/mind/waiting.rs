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
use serde_json::{Value, json};

use super::Mind;
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

impl Mind {
    /// Everything waiting on the people who run her, newest first.
    pub(super) fn waiting_view(&self, now: DateTime<Utc>) -> Result<Value> {
        let mut items: Vec<(DateTime<Utc>, Value)> = self.questions()?;
        items.extend(self.lately(now - Duration::days(DAYS))?);
        items.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        Ok(json!(items.into_iter().map(|(_, v)| v).collect::<Vec<_>>()))
    }

    /// Each piece of open work waiting on an answer, by the entry that
    /// asked, with what answering it takes from its own record.
    fn questions(&self) -> Result<Vec<(DateTime<Utc>, Value)>> {
        let memory = self.ghost.memory();
        let waiting: Vec<_> = memory
            .undertakings(true, 500)?
            .into_iter()
            .filter(|u| u.waiting.is_some())
            .collect();
        if waiting.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = waiting.iter().map(|u| u.id.clone()).collect();
        let looks = self.records_of(&ids);
        let mut items = Vec::new();
        for u in waiting {
            let asked = self
                .ghost
                .journal()
                .of_kinds_after(&["asked"], u.seq)?
                .into_iter()
                .rev()
                .find(|e| matches!(&e.event, Event::Asked { id, .. } if *id == u.id));
            let Some(asked) = asked else { continue };
            let state = looks.iter().find(|l| l.id == u.id).map(|l| &l.state);
            items.push((
                asked.at,
                json!({
                    "id": format!("asked-{}", asked.seq),
                    "kind": "question",
                    "at": asked.at,
                    "seq": asked.seq,
                    "work": u.id,
                    "brief": u.brief,
                    "text": u.waiting,
                    "ask": state.and_then(|s| s.waiting_kind),
                    "options": state.map(|s| s.waiting_options.clone()).unwrap_or_default(),
                }),
            ));
        }
        Ok(items)
    }

    /// What ended badly, was told, and was reported since `since`.
    fn lately(&self, since: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, Value)>> {
        let memory = self.ghost.memory();
        let journal = self.ghost.journal();
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
                        json!({
                            "id": format!("ended-{}", entry.seq),
                            "kind": "ended",
                            "at": entry.at,
                            "seq": entry.seq,
                            "work": id,
                            "brief": brief,
                            "text": summary,
                            "outcome": outcome,
                        }),
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
                            .is_some_and(|(channel, who)| {
                                self.cfg.operators.includes(channel, who)
                            });
                    if !operators {
                        continue;
                    }
                    items.push((
                        entry.at,
                        json!({
                            "id": format!("told-{}", entry.seq),
                            "kind": "told",
                            "at": entry.at,
                            "seq": entry.seq,
                            "work": id,
                            "brief": u.brief,
                            "text": text,
                            "files": files,
                        }),
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
                json!({
                    "id": format!("report-{}", report.intention),
                    "kind": "report",
                    "at": at,
                    "seq": report.intention,
                    "place": place,
                    "text": report.lines.join("\n"),
                }),
            ));
        }
        Ok(items)
    }
}
