//! What a brain made of a look, as it comes back: an answer, a turn of tool
//! calls to be answered, what she went to look up, or a failure to retry.

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Call, Item, Step};
use ghost_core::{
    journal::Event, media, memory::PlaceState, narrate, social::PlaceKind, when::when,
};
use ghost_work::{
    WORK_DB,
    catalog::{self, Need},
    looks::Looks,
    steps,
    undertaking::placeholder,
};
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::{
    Attend, Mind, Outcome,
    attend::{Found, Looking, Thinking, Thought, Why},
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
        if self
            .thinking
            .get(&job.place)
            .is_none_or(|looking| looking.look != job.look)
        {
            // She was stopped in the middle of this look before it came
            // back: what it came to is not hers to act on.
            debug!("{}: a look she was stopped in came back", job.place);
            return Ok(());
        }
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
                let (at, mut calls) = job.out.take().unwrap_or((now, Vec::new()));
                let mut came = Vec::with_capacity(found.len());
                for Found { call, outcome, ms } in found {
                    let (item, kept) = self.took_in(&place, call, outcome, sees, ms)?;
                    job.items.push(item);
                    came.push(kept);
                }
                // What came back takes the places of the calls that were
                // out, in the order they were called.
                let mut came = came.into_iter();
                for call in &mut calls {
                    if call.is_none() {
                        *call = came.next();
                    }
                }
                self.keep_round(&mut job, at, calls);
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
            return Ok(self
                .prompts
                .fill("recall_nothing", &[("query", query.to_owned())]));
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
                Some(kept) => self.prompts.fill(
                    "recall_kept",
                    &[("file", kept.file), ("note", clip(&kept.note, 300))],
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
    /// shown it. Beside it, the call as the record of her looks keeps it,
    /// with what came back as it came, unmarked.
    fn took_in(
        &mut self,
        place: &PlaceState,
        call: Call,
        outcome: Option<ghost_work::Outcome>,
        sees: bool,
        ms: u128,
    ) -> Result<(Item, Value)> {
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
            let kept = steps::call(&call.name, &call.args, &too_slow, true, 0, ms);
            return Ok((Item::result(call.id, call.name, too_slow, true), kept));
        };
        let kept = steps::call(&call.name, &call.args, &output, failed, pictures.len(), ms);
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
        Ok((
            Item::Result {
                id: call.id,
                name: call.name,
                output: shown,
                failed,
                pictures,
            },
            kept,
        ))
    }

    /// Keep one round of the look `job`, called `at`, where looks keep
    /// their rounds, numbering the look if this is its first. A call still
    /// out, `None`, is left out. A round of no calls is kept only for what
    /// the brain's turn cost, so that a look that only answered still says
    /// what it read. The look goes on whether or not it could be kept: it
    /// is a record of her thinking, not a part of it.
    fn keep_round(
        &mut self,
        job: &mut Thinking,
        at: DateTime<Utc>,
        calls: impl IntoIterator<Item = Option<Value>>,
    ) {
        let calls: Vec<Value> = calls.into_iter().flatten().collect();
        if calls.is_empty() && job.usage.is_none() {
            return;
        }
        let mut round = json!({"at": at.to_rfc3339(), "calls": calls});
        if let Some(u) = job.usage.take() {
            round["usage"] = json!({"input": u.input, "cached": u.cached, "output": u.output});
        }
        if !job.context.is_empty() {
            round["context"] = json!(job.context);
        }
        if let Err(e) = self.kept_round(job, at, &round) {
            warn!("{}: could not keep a round of the look: {e}", job.place);
        }
    }

    fn kept_round(
        &mut self,
        job: &mut Thinking,
        at: DateTime<Utc>,
        round: &Value,
    ) -> std::io::Result<()> {
        let looks = match &mut self.kept_looks {
            Some(looks) => looks,
            none => none.insert(Looks::open(&self.ghost.dir().join(WORK_DB))?),
        };
        let kept = match job.kept {
            Some(kept) => kept,
            None => *job.kept.insert(looks.begin(&job.place, at)?),
        };
        looks.round(kept, round)
    }

    /// The look `job` ended with the journal entry `seq`: the record of
    /// her looks says where, if it kept the look.
    pub(super) fn end_look(&self, job: &Thinking, seq: i64) {
        self.end_kept(&job.place, job.kept, seq);
    }

    /// The look kept as `kept` in `place`, if one was, ended with the
    /// journal entry `seq`.
    pub(super) fn end_kept(&self, place: &str, kept: Option<i64>, seq: i64) {
        if let (Some(looks), Some(kept)) = (&self.kept_looks, kept)
            && let Err(e) = looks.end(kept, seq)
        {
            warn!("{place}: could not say where a look ended: {e}");
        }
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
        job.usage = Some(step.usage);
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
            // What this last turn cost is kept, with nothing called in it.
            self.keep_round(&mut job, now, []);
            return self.conclude(job, place, now);
        }
        let mut again = false;
        let reaching = self.reaching(&job.loaded);
        let reachable = reaching.names();
        let mut outside = Vec::new();
        // The round as the record of her looks keeps it, call by call;
        // what reaches beyond the conversation fills its place later.
        let mut kept = Vec::with_capacity(calls.len());
        for call in calls {
            if reachable.contains(&call.name) {
                outside.push(call);
                kept.push(None);
                continue;
            }
            let started = std::time::Instant::now();
            // Something that reaches beyond the conversation, which she
            // does not have here.
            let beyond = catalog::find(&call.name).is_some_and(|e| {
                e.needs
                    .iter()
                    .any(|n| matches!(n, Need::Search | Need::Web | Need::Machine))
            });
            let (output, failed) = match actions::from_call(&self.prompts, &call) {
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
            kept.push(Some(steps::call(
                &call.name,
                &call.args,
                &output,
                failed,
                0,
                started.elapsed().as_millis(),
            )));
            job.items
                .push(Item::result(call.id, call.name, output, failed));
        }
        if !outside.is_empty() {
            job.out = Some((now, kept));
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
            let (look, intention, kept) = (job.look, job.intention(), job.kept);
            let words = self.prompts.store().clone();
            let abort = self.jobs.spawn(async move {
                let mut found = Vec::with_capacity(outside.len());
                for call in outside {
                    let started = std::time::Instant::now();
                    let outcome = tokio::time::timeout(budget, reaching.run(&call, &words))
                        .await
                        .ok();
                    let ms = started.elapsed().as_millis();
                    found.push(Found { call, outcome, ms });
                }
                Outcome::Thought {
                    job: Box::new(job),
                    result: Ok(Thought::Looked(found)),
                }
            });
            self.thinking.insert(
                place.key.clone(),
                Looking {
                    look,
                    abort,
                    intention,
                    kept,
                },
            );
            return Ok(());
        }
        if again {
            // Something came back, or she was told what she got wrong:
            // that is a round worth seeing.
            self.keep_round(&mut job, now, kept);
            return self.go_on(job, place, now);
        }
        // A round of nothing but what she decided to do ends the look,
        // and her journal has what she decided; what the turn cost is
        // kept here.
        self.keep_round(&mut job, now, []);
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
        // A look tried again is a new look, and this one is left without
        // an end; one given up ends with the entry that says so, below.
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
        let decided = self.record(Event::Decided {
            about: place.key.clone(),
            decision: "nothing".into(),
            reason: format!("could not think: {error}"),
        })?;
        self.end_look(job, decided.seq);
        Ok(())
    }
}
