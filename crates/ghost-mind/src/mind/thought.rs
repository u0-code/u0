//! What a brain made of a look, as it comes back: an answer, a turn of tool
//! calls to be answered, what she went to look up, or a failure to retry.

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Call, Item, Step};
use ghost_core::{
    journal::Event, media, memory::PlaceState, narrate, social::PlaceKind, when::when,
};
use ghost_work::{
    catalog::{self, Need},
    undertaking::placeholder,
};
use tracing::warn;

use super::{
    Attend, Mind, Outcome,
    attend::{Found, Thinking, Thought, Why},
};
use crate::{
    Error, Result,
    actions::{self, Action, Deliberation, clip},
};

/// How many times a brain may ask to recall before it has to answer.
const RECALL_ROUNDS: u8 = 2;
/// How many times a failed deliberation is retried.
const THINK_RETRIES: u8 = 3;
/// How much of what a search returned, or of a page she opened, is kept in
/// her journal.
const LOOKED_UP: usize = 3000;
/// How much of what reached beyond the conversation she is shown in it: a
/// page is read at a glance, and the rest of it is work.
const TALK_RESULT: usize = 6000;

impl Mind {
    /// A brain answered, or failed to.
    pub(super) fn on_thought(
        &mut self,
        mut job: Thinking,
        result: Result<Thought>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.thinking.remove(&job.place);
        let Some(place) = self.ghost.memory().place(&job.place)? else {
            return Ok(());
        };
        let thought = match result {
            Ok(thought) => thought,
            Err(e) => return self.could_not_think(&place, &job, &e, now),
        };
        self.retries.remove(&job.place);
        let deliberation = match thought {
            Thought::Whole(deliberation) => deliberation,
            Thought::Step(step) => return self.on_turn(job, step, &place, now),
            Thought::Looked(found) => {
                let sees = if job.cheap {
                    self.tick.sees()
                } else {
                    self.think.sees()
                };
                for Found { call, outcome } in found {
                    let item = self.took_in(&place, call, outcome, sees)?;
                    job.items.push(item);
                }
                return self.go_on(job, &place, now);
            }
        };
        if job.round < RECALL_ROUNDS {
            let found = match (deliberation.recall(), deliberation.reread()) {
                (Some(query), _) => Some(self.remembered(query, &place, now)?),
                (None, Some(n)) => Some(self.reread(n, &place, now)?),
                (None, None) => None,
            };
            if let Some(found) = found {
                // What she went back to is added to what she already found.
                job.recalled = Some(match job.recalled.take() {
                    Some(before) => format!("{before}\n\n{found}"),
                    None => found,
                });
                job.round += 1;
                return self.ponder(job, &place, now);
            }
        }
        self.apply(&place, &job, deliberation, now)
    }

    /// What a search of her memory turns up, as she would be told it.
    fn remembered(&self, query: &str, place: &PlaceState, now: DateTime<Utc>) -> Result<String> {
        let hits = self.ghost.memory().search(&place.channel, query, 8)?;
        if hits.is_empty() {
            return Ok(format!(
                "You searched your memory for \"{query}\" and found nothing."
            ));
        }
        let clock = self.life.clock();
        let mut found = Vec::with_capacity(hits.len());
        for h in &hits {
            // Something she keeps is shown as what it is, so that she can
            // refer to it by its number.
            let kept = match h.kind.as_str() {
                "kept" => self.ghost.memory().belonging(h.seq)?,
                _ => None,
            };
            let text = match kept {
                Some(kept) => format!(
                    "you kept an image ({}): {}",
                    kept.file,
                    clip(&kept.note, 300)
                ),
                None => clip(&h.text, 300),
            };
            found.push(format!(
                "- [#{}] {}{}{}",
                h.seq,
                h.at.map(|at| format!("{}: ", narrate::capital(&when(at, now, clock))))
                    .unwrap_or_default(),
                text,
                self.learned_where(h.place.as_deref(), &place.key)?
            ));
        }
        Ok(found.join("\n"))
    }

    /// What came back from beyond the conversation, as an item of the look,
    /// with what it was kept as in her journal: a search as `LookedUp`, a
    /// page or an image she opened as `Opened`. What came from outside is
    /// shown to her marked with where it came from (see
    /// [`ghost_work::outside`]); her journal keeps it as it was. An image
    /// is kept under `media/` and named in the result, after the mark, so
    /// that she can keep it or send it by that name; a brain that sees is
    /// shown it.
    fn took_in(
        &mut self,
        place: &PlaceState,
        call: Call,
        outcome: Option<ghost_work::Outcome>,
        sees: bool,
    ) -> Result<Item> {
        let Some(ghost_work::Outcome {
            output,
            failed,
            mut pictures,
            origin,
        }) = outcome
        else {
            let too_slow = self.prompts.fill(
                "too_slow",
                &[("seconds", self.cfg.talk_tool_seconds.to_string())],
            );
            return Ok(Item::result(call.id, call.name, too_slow, true));
        };
        let output = clip(&output, TALK_RESULT);
        let opened = call.args["url"]
            .as_str()
            .or_else(|| call.args["path"].as_str())
            .unwrap_or_default()
            .to_owned();
        if !failed && pictures.is_empty() {
            match call.name.as_str() {
                "search" => {
                    self.record(Event::LookedUp {
                        query: call.args["query"].as_str().unwrap_or_default().to_owned(),
                        found: clip(&output, LOOKED_UP),
                        place: Some(place.key.clone()),
                    })?;
                }
                "fetch" => {
                    self.record(Event::Opened {
                        url: opened.clone(),
                        text: clip(&output, LOOKED_UP),
                        file: None,
                        place: Some(place.key.clone()),
                    })?;
                }
                _ => {}
            }
        }
        let mut shown = ghost_work::outside::mark(
            self.prompts.store(),
            &call,
            origin.as_deref(),
            output.clone(),
        );
        for picture in &pictures {
            let file = media::store(self.ghost.dir(), &picture.bytes, &picture.mime)?;
            self.record(Event::Opened {
                url: opened.clone(),
                text: clip(&output, 300),
                file: Some(file.clone()),
                place: Some(place.key.clone()),
            })?;
            shown.push('\n');
            shown.push_str(&placeholder(&file));
        }
        if !pictures.is_empty() && !sees {
            shown.push('\n');
            shown.push_str(&ghost_brain::not_shown(pictures.len()));
            pictures.clear();
        }
        Ok(Item::Result {
            id: call.id,
            name: call.name,
            output: shown,
            failed,
            pictures,
        })
    }

    /// One turn of a look made of tool calls came back. What she asked to
    /// know is answered, what she decided to do is kept for the end, and a
    /// call that could not be read is handed back to be put right.
    fn on_turn(
        &mut self,
        mut job: Thinking,
        step: Step,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let said = step.text.trim().to_owned();
        if job.thought.is_empty() {
            job.thought.clone_from(&said);
        }
        let calls = step.calls.clone();
        job.items.push(step.into_item());
        job.round += 1;
        if calls.is_empty() {
            // A brain new to this writes its answer as text now and then.
            // Text is a thought, and a thought reaches nobody: say so once,
            // where someone is waiting on her.
            let waited_on = place.kind == PlaceKind::Direct || place.unread_addressed > 0;
            if !said.is_empty()
                && job.took.is_empty()
                && !job.nudged
                && waited_on
                && job.round < self.cfg.talk_steps
            {
                job.nudged = true;
                job.items.push(Item::user(self.prompts.text("not_sent")));
                return self.ponder(job, place, now);
            }
            return self.conclude(job, place, now);
        }
        let mut again = false;
        let reaching = self.reaching(place, &job.loaded, now)?;
        let reachable = reaching.names();
        let mut outside = Vec::new();
        for call in calls {
            if reachable.contains(&call.name) {
                outside.push(call);
                continue;
            }
            // Something that reaches beyond the conversation, which she
            // does not have here.
            let beyond = catalog::find(&call.name).is_some_and(|e| {
                e.needs
                    .iter()
                    .any(|n| matches!(n, Need::Search | Need::Web | Need::Machine))
            });
            let (output, failed) = match actions::from_call(&call) {
                Err(_) if beyond => {
                    again = true;
                    (self.prompts.text("cannot_here").to_owned(), true)
                }
                Err(why) => {
                    again = true;
                    (why, true)
                }
                Ok(Action::Recall { query }) => {
                    again = true;
                    (self.remembered(&query, place, now)?, false)
                }
                Ok(Action::Reread { memory }) => {
                    again = true;
                    (self.reread(memory, place, now)?, false)
                }
                // Told at once when it cannot be done, so that she can
                // put it right, as with a call she got wrong.
                Ok(
                    action @ (Action::Keep { .. }
                    | Action::LetGo { .. }
                    | Action::SendFile { .. }
                    | Action::Steer { .. }
                    | Action::Stop { .. }),
                ) => {
                    if let Some(why) = self.refusal(place, &action)? {
                        again = true;
                        (why, true)
                    } else {
                        job.took.push(action);
                        ("ok".to_owned(), false)
                    }
                }
                Ok(action) => {
                    job.took.push(action);
                    ("ok".to_owned(), false)
                }
            };
            job.items
                .push(Item::result(call.id, call.name, output, failed));
        }
        if !outside.is_empty() {
            // The web and her machine are slow: they are reached off to
            // the side, each for a few seconds at most, and the look goes
            // on when what they return is in.
            let budget = std::time::Duration::from_secs(self.cfg.talk_tool_seconds);
            for call in &outside {
                tracing::info!(
                    "{}: {} {}",
                    place.key,
                    call.name,
                    clip(&call.args.to_string(), 80)
                );
            }
            self.thinking.insert(place.key.clone());
            self.jobs.spawn(async move {
                let mut found = Vec::with_capacity(outside.len());
                for call in outside {
                    let outcome = tokio::time::timeout(budget, reaching.run(&call)).await.ok();
                    found.push(Found { call, outcome });
                }
                Outcome::Thought {
                    job: Box::new(job),
                    result: Ok(Thought::Looked(found)),
                }
            });
            return Ok(());
        }
        if again {
            return self.go_on(job, place, now);
        }
        self.conclude(job, place, now)
    }

    /// Another turn, if she has any left; otherwise the look ends with
    /// what she has decided so far.
    fn go_on(&mut self, job: Thinking, place: &PlaceState, now: DateTime<Utc>) -> Result<()> {
        if job.round < self.cfg.talk_steps {
            self.ponder(job, place, now)
        } else {
            self.conclude(job, place, now)
        }
    }

    /// A look made of tool calls is over: do what she decided.
    fn conclude(
        &mut self,
        mut job: Thinking,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let deliberation = Deliberation {
            thought: std::mem::take(&mut job.thought),
            actions: std::mem::take(&mut job.took),
        };
        self.apply(place, &job, deliberation, now)
    }

    fn could_not_think(
        &mut self,
        place: &PlaceState,
        job: &Thinking,
        error: &Error,
        now: DateTime<Utc>,
    ) -> Result<()> {
        warn!("{}: {error}", place.key);
        if let Why::Intention(intention) = &job.why {
            // Try the intention again later rather than losing it.
            self.pursuing.remove(&intention.seq);
            self.held.insert(intention.seq, now + Duration::minutes(10));
            return self.refresh_intentions();
        }
        let tries = self.retries.entry(place.key.clone()).or_default();
        *tries += 1;
        if *tries < THINK_RETRIES {
            let due = now + Duration::seconds(20 * i64::from(*tries));
            self.agenda.attend.insert(
                place.key.clone(),
                Attend {
                    due,
                    since: now,
                    addressed: true,
                    backoff: 1.0,
                },
            );
            return Ok(());
        }
        // Give up on this look so she does not loop on it forever.
        self.retries.remove(&place.key);
        self.record(Event::Read {
            channel: place.channel.clone(),
            place: place.id.clone(),
            upto: job.upto,
        })?;
        self.record(Event::Decided {
            about: place.key.clone(),
            decision: "nothing".into(),
            reason: format!("could not think: {error}"),
        })?;
        Ok(())
    }
}
