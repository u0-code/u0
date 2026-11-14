//! Doing things.
//!
//! When she decides to take something on, it becomes an undertaking: a
//! directory with a brief, a plan and notes, and an entry in her journal.
//! She works on it in sittings. A sitting is a bounded run of the loop in
//! `ghost-work`, sent off as a job like any brain call, so the actor stays
//! the only writer of her journal and stays free to notice everything else.
//!
//! Work is one foreground activity among others. While a sitting is running
//! she is busy, and messages wait as they would for anyone at a desk. When
//! she does look at her phone, the sitting is asked to stop after the step
//! it is on; everything it needs to go on is on disk. She returns to it when
//! she is done talking.
//!
//! The work and she talk while it runs, and only through its record in
//! `work.db`, which both the worker here and the tool server a command-line
//! agent starts can write. The work leaves there what it chose to tell her
//! and what it kept; she glances at it every so often while a sitting runs,
//! and once more when it ends, and writes into her journal what she has not
//! had yet. Nothing of what it did to get there reaches her. She speaks to
//! it the same way: what she adds goes into its inbox, taken before its
//! next step, and stopping it is an outcome written into its record, which
//! it sees before its next step too.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{CliAgent, CliKind};
use ghost_core::{
    journal::Event,
    life::Phase,
    memory::{Person, PlaceState},
    will::{self, Stage},
};
use ghost_work::{
    Ended, Limits, Report, Undertaking, WORK_DB, Worker, claude, codex, tools,
    undertaking::Kept,
    worker::{opening_text, system_prompt},
};

/// How often she glances at what a sitting in progress has told her.
const GLANCE: Duration = Duration::seconds(30);
use tracing::{info, warn};

use super::{Mind, Outcome};
use crate::{Result, actions::clip};

/// A sitting in progress.
pub(super) struct Working {
    pub id: String,
    /// Set to stop the sitting after the step it is on.
    pub pause: Arc<AtomicBool>,
    /// Its record, which the sitting writes to as it goes.
    pub record: Arc<Undertaking>,
}

/// What moves a sitting from one step to the next.
enum Stepper {
    /// Our own loop, over a brain that can take tools.
    Own(Arc<dyn ghost_brain::Brain>),
    /// A command-line agent that brings its loop and is given our tools.
    Cli(CliAgent),
}

impl Mind {
    /// How she would work, if she can in this body.
    fn stepper(&self) -> Option<Stepper> {
        if let Some(brain) = [&self.think, &self.tick].into_iter().find(|b| b.works()) {
            return Some(Stepper::Own(brain.clone()));
        }
        self.workshop.runner.as_ref()?;
        self.think.cli().map(Stepper::Cli)
    }

    /// The undertaking `id` in her work database, started from `brief` if
    /// it has not been opened before.
    pub(super) fn undertaking(&self, id: &str, brief: &str) -> std::io::Result<Undertaking> {
        Undertaking::open(&self.ghost.dir().join(WORK_DB), id, Some(brief))
    }

    /// Whether taking things on is possible at all in this body.
    pub(super) fn can_work(&self) -> bool {
        self.cfg.work.enabled && self.stepper().is_some()
    }

    /// She decided to take something on. Returns why not, if it is refused.
    pub(super) fn undertake(
        &mut self,
        brief: &str,
        asked_by: Option<&Person>,
        place: Option<&PlaceState>,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        let brief = brief.trim();
        if brief.is_empty() {
            return Ok(Some("there was nothing to take on".into()));
        }
        if !self.can_work() {
            return Ok(Some("this body has nothing to work with".into()));
        }
        let memory = self.ghost.memory();
        // More about something she is already doing for the same person is
        // added to it, not started as a second thing.
        if let Some(person) = asked_by
            && let Some(open) = memory
                .undertakings(true, 20)?
                .into_iter()
                .find(|u| u.asked_by.as_deref() == Some(person.key.as_str()))
        {
            let added = format!(
                "{brief}{}",
                match place {
                    Some(place) => self.their_words(place, person)?,
                    None => String::new(),
                }
            );
            return Ok(
                match self
                    .undertaking(&open.id, &open.brief)
                    .and_then(|u| u.add(&added))
                {
                    Ok(()) => {
                        info!("added to {}: {}", open.id, clip(brief, 80));
                        self.record(Event::Added {
                            id: open.id,
                            text: added,
                        })?;
                        self.agenda.work.get_or_insert(now);
                        None
                    }
                    Err(e) => Some(format!(
                        "it could not be added to what she is already doing: {e}"
                    )),
                },
            );
        }
        if memory.undertaken_since(now - Duration::hours(24))? >= i64::from(self.cfg.work.per_day) {
            return Ok(Some("she has taken on enough for one day".into()));
        }
        // Her will decides what she agrees to. This is only the floor under
        // it: a stranger does not get her hands on a shell.
        if let Some(person) = asked_by {
            let stage = will::standing(person, now).stage;
            let needed = if self.workshop.machine.is_some() {
                Stage::Friend
            } else {
                Stage::Stranger
            };
            if stage < needed {
                return Ok(Some(format!(
                    "{} is {} to her, which is not enough for this",
                    person.name,
                    stage.word()
                )));
            }
        }
        let id = format!("{}-{:04x}", now.format("%Y%m%d-%H%M%S"), self.rng.u16(..));
        // Her own account of what is wanted, and under it what they said.
        let brief = format!(
            "{brief}{}",
            match (place, asked_by) {
                (Some(place), Some(person)) => self.their_words(place, person)?,
                _ => String::new(),
            }
        );
        self.record(Event::Undertook {
            id,
            brief,
            asked_by: asked_by.map(|p| p.key.clone()),
            place: place.map(|p| p.key.clone()),
        })?;
        self.agenda.work.get_or_insert(now);
        Ok(None)
    }

    /// Time to sit down to something, if there is something.
    pub(super) fn work(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.agenda.work = None;
        // At a sitting already, this is when she glances at what it has
        // told her so far.
        if let Some(working) = &self.working {
            let (id, record) = (working.id.clone(), working.record.clone());
            self.agenda.work = Some(now + GLANCE);
            return self.hear_from_work(&id, &record, now);
        }
        let Some(stepper) = self.stepper().filter(|_| self.cfg.work.enabled) else {
            return Ok(());
        };
        let moment = self.life.moment(self.ghost.journal(), now)?;
        if moment.phase == Phase::Asleep && !self.tempo.instant() {
            self.agenda.work = Some(moment.until + Duration::minutes(20));
            return Ok(());
        }
        // A conversation in progress comes first.
        if !self.agenda.speaking.is_empty() || !self.thinking.is_empty() {
            self.agenda.work = Some(now + Duration::seconds(30));
            return Ok(());
        }
        // Whatever has waited longest since it was last touched, leaving
        // aside what is waiting on somebody's answer.
        let open = self.ghost.memory().undertakings(true, 20)?;
        let patience = Duration::hours(i64::from(self.cfg.work.ask_hours));
        let unanswered = open
            .iter()
            .find(|u| u.asked_at.is_some_and(|at| now - at >= patience))
            .cloned();
        if let Some(stale) = unanswered {
            // Nobody answered. She goes on without, and says so in the work.
            let said = self.prompts.text("no_answer").to_owned();
            if let Err(e) = self
                .undertaking(&stale.id, &stale.brief)
                .and_then(|u| u.add(&said))
            {
                warn!("cannot go on with {} unanswered: {e}", stale.id);
            }
            self.record(Event::Added {
                id: stale.id,
                text: said,
            })?;
            self.agenda.work = Some(now);
            return Ok(());
        }
        // If all that is left is waiting to be answered, come back when
        // the first of those has waited long enough.
        let give_up_at = open
            .iter()
            .filter_map(|u| u.asked_at)
            .min()
            .map(|at| at + patience);
        let Some(next) = open
            .into_iter()
            .filter(|u| u.waiting.is_none())
            .min_by_key(|u| u.last_at)
        else {
            self.agenda.work = give_up_at;
            return Ok(());
        };
        if next.sittings >= i64::from(self.cfg.work.max_sittings) {
            self.record(Event::Concluded {
                id: next.id.clone(),
                outcome: "dropped".into(),
                summary: format!(
                    "Let go after {} sittings without finishing. {}",
                    next.sittings,
                    next.summary.unwrap_or_default()
                ),
            })?;
            self.agenda.work = Some(now);
            return Ok(());
        }

        let undertaking = match self.undertaking(&next.id, &next.brief) {
            Ok(u) => Arc::new(u),
            Err(e) => {
                warn!("cannot open the undertaking {}: {e}", next.id);
                self.record(Event::Concluded {
                    id: next.id,
                    outcome: "blocked".into(),
                    summary: format!("Its record could not be opened: {e}"),
                })?;
                return Ok(());
            }
        };
        let before = undertaking.state().steps;
        let place = match &self.workshop.machine {
            Some(machine) => machine.describe(),
            None => self.prompts.text("no_machine").to_owned(),
        };
        // What she works with, for this and for any part of it she hands
        // to a helper: her machine if she has one, and the web.
        let equip: ghost_work::worker::ToolsFor = {
            let (machine, web) = (self.workshop.machine.clone(), self.workshop.web.clone());
            Arc::new(move |undertaking| {
                let tools = match &machine {
                    Some(machine) => tools::standard(machine.clone(), undertaking),
                    None => tools::desk(undertaking),
                };
                tools::with_web(tools, &web)
            })
        };
        // What she keeps while she works is hers to have again, and so is
        // what she tells herself from it. A helper given part of the work
        // does neither: it reports to the work.
        let tools = equip(&undertaking)
            .with(tools::Keep::new(undertaking.clone()))
            .with(tools::Tell::new(undertaking.clone()));
        // With someone to ask and her own loop to carry the question, she
        // can stop and ask.
        let tools = if next.asked_by.is_some() && matches!(stepper, Stepper::Own(_)) {
            tools.with(tools::Ask::new(undertaking.clone()))
        } else {
            tools
        };
        let pause = Arc::new(AtomicBool::new(false));
        let limits = Limits {
            max_steps: self.cfg.work.sitting_steps,
            max_seconds: u64::from(self.cfg.work.sitting_minutes) * 60,
            context_tokens: self.cfg.work.context_tokens,
            ..Limits::default()
        };
        // Who she is, without what moment it is: the same words at every
        // sitting, so that a sitting picked up later reads its transcript
        // from the provider's cache instead of afresh.
        let who = format!(
            "{}\n\n{}{}",
            self.who(now)?,
            self.prompts.working.trim_end(),
            self.practice_block()?
        );
        // What the work is told comes from the same files as what she is
        // told, and is looked at again before each step.
        let words = self.prompts.for_work();
        info!("sitting down to {} ({})", next.id, clip(&next.brief, 80));
        self.working = Some(Working {
            id: next.id.clone(),
            pause: pause.clone(),
            record: undertaking.clone(),
        });
        self.agenda.work = Some(now + GLANCE);
        let id = next.id;
        match stepper {
            Stepper::Own(brain) => {
                let worker = Worker {
                    brain,
                    tools,
                    undertaking,
                    limits,
                    who,
                    machine: place,
                    pause,
                    helpers: Some(equip),
                    prompts: words.shared(),
                };
                self.jobs.spawn(async move {
                    let report = worker.run().await;
                    Outcome::Worked {
                        id,
                        before,
                        report,
                        progress: worker.undertaking.state().progress(),
                        record: worker.undertaking,
                    }
                });
            }
            Stepper::Cli(agent) => {
                // The agent is handed her tools by a server it starts itself.
                let mut tool_server = vec![
                    self.workshop
                        .runner
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    "--serve-mcp".to_owned(),
                    // Her work database, and which undertaking in it.
                    // Absolute: the agent starts the server from elsewhere.
                    "--state".to_owned(),
                    self.ghost
                        .dir()
                        .canonicalize()
                        .unwrap_or_else(|_| self.ghost.dir().to_path_buf())
                        .display()
                        .to_string(),
                    "--id".to_owned(),
                    id.clone(),
                    // She follows it as it runs.
                    "--tell".to_owned(),
                ];
                // Her own `prompts/` is found in her directory; the
                // configured one has to be named.
                if let Some(dir) = &self.cfg.prompts {
                    tool_server.extend([
                        "--prompts".to_owned(),
                        dir.canonicalize()
                            .unwrap_or_else(|_| dir.clone())
                            .display()
                            .to_string(),
                    ]);
                }
                tool_server.extend(self.workshop.runner_args.iter().cloned());
                let seat = claude::Seat {
                    agent,
                    system: system_prompt(&words, &who, &place, &limits),
                    opening: opening_text(&words, &undertaking.state()),
                    undertaking,
                    tool_server,
                    max_turns: limits.max_steps,
                    max_seconds: limits.max_seconds,
                    stop: pause,
                };
                self.jobs.spawn(async move {
                    let report = match seat.agent.kind {
                        CliKind::Claude => claude::sit(&seat).await,
                        CliKind::Codex => codex::sit(&seat).await,
                    };
                    Outcome::Worked {
                        id,
                        before,
                        report,
                        progress: seat.undertaking.state().progress(),
                        record: seat.undertaking,
                    }
                });
            }
        }
        Ok(())
    }

    /// What she kept while working on `id` becomes hers, kept as anything
    /// else she keeps is. The record of the work lists everything kept in
    /// it, in every sitting; what she has already kept from it with the
    /// same note, even if she has since let it go, is not kept twice.
    /// Returns the notes of what was kept just now.
    fn take_home(&mut self, id: &str, kept: &[Kept]) -> Result<Vec<String>> {
        let mut new = Vec::new();
        for k in kept {
            if self.ghost.memory().ever_kept(&k.file, &k.note, id)? {
                continue;
            }
            info!("kept {} from {id}", k.file);
            self.record(Event::Kept {
                file: k.file.clone(),
                note: k.note.clone(),
                from: Some(id.to_owned()),
            })?;
            new.push(k.note.clone());
        }
        Ok(new)
    }

    /// What the work `id` has told her and kept that she has not had yet,
    /// from its record, into her journal. Her journal is the receipt: what
    /// it already holds is not written twice, however often she looks and
    /// whichever process the work ran in.
    ///
    /// Most of it waits until she next looks at anything. Something she
    /// could hand over now, a kept image or a file made, also brings her
    /// back to whoever asked for the work, as something she means to do.
    fn hear_from_work(&mut self, id: &str, record: &Undertaking, now: DateTime<Utc>) -> Result<()> {
        let state = record.state();
        let heard = self.ghost.memory().reported(id)?;
        let mut to_hand_over = Vec::new();
        let mut made = false;
        for told in state.told.iter().skip(heard) {
            info!("{id} says: {}", clip(&told.text, 120));
            self.record(Event::Reported {
                id: id.to_owned(),
                text: told.text.clone(),
                files: told.files.clone(),
            })?;
            if !told.files.is_empty() {
                made = true;
                to_hand_over.push(format!(
                    "{} ({})",
                    clip(&told.text, 200),
                    told.files.join(", ")
                ));
            }
        }
        to_hand_over.extend(
            self.take_home(id, &state.kept)?
                .iter()
                .map(|note| clip(note, 200)),
        );
        if to_hand_over.is_empty() {
            return Ok(());
        }
        let Some(u) = self.ghost.memory().undertaking(id)? else {
            return Ok(());
        };
        if u.asked_by.is_none() && u.place.is_none() {
            return Ok(());
        }
        // A kept image is read from her own directory; a file made is read
        // off her machine, so it can only be sent where she has one.
        let can_send = (!made || self.workshop.machine.is_some())
            && u.place
                .as_deref()
                .and_then(ghost_core::social::split_key)
                .is_some_and(|(channel, _)| self.can(channel, crate::Can::Files));
        let share = self.prompts.fill(
            "share_found",
            &[
                ("brief", clip(&u.brief, 160)),
                ("found", to_hand_over.join("; ")),
                (
                    "send",
                    self.prompts
                        .text(if can_send { "made_send" } else { "made_tell" })
                        .to_owned(),
                ),
            ],
        );
        if self.intend(now + Duration::minutes(1), share, u.asked_by, u.place, true)? {
            self.refresh_intentions()?;
        }
        Ok(())
    }

    /// The open piece of work she knows by `number`, or, as a sentence for
    /// her, why there is none.
    pub(super) fn open_work(
        &self,
        number: i64,
    ) -> Result<std::result::Result<ghost_core::memory::Undertaking, String>> {
        let n = [("work", number.to_string())];
        Ok(match self.ghost.memory().undertaking_numbered(number)? {
            Some(u) if u.outcome.is_none() => Ok(u),
            Some(_) => Err(self.prompts.fill("work_over", &n)),
            None => Err(self.prompts.fill("no_work", &n)),
        })
    }

    /// She passes something on to work she has under way. It reaches the
    /// worker before its next step, in this sitting or the next. Returns
    /// why not, if it cannot.
    pub(super) fn steer(
        &mut self,
        number: i64,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Some(self.prompts.text("steer_nothing").to_owned()));
        }
        let u = match self.open_work(number)? {
            Ok(u) => u,
            Err(why) => return Ok(Some(why)),
        };
        if let Err(e) = self
            .undertaking(&u.id, &u.brief)
            .and_then(|record| record.add(text))
        {
            warn!("cannot pass anything on to {}: {e}", u.id);
            return Ok(Some(format!("its record could not be written to: {e}")));
        }
        info!("passed on to {}: {}", u.id, clip(text, 80));
        self.record(Event::Added {
            id: u.id,
            text: text.to_owned(),
        })?;
        self.agenda.work.get_or_insert(now);
        Ok(None)
    }

    /// She stops work she has under way. A sitting on it ends after the
    /// step it is on and comes back as dropped; with none running, it ends
    /// now. Either way what it kept and told is hers first. Returns why
    /// not, if it cannot.
    pub(super) fn stop_work(&mut self, number: i64, now: DateTime<Utc>) -> Result<Option<String>> {
        let u = match self.open_work(number)? {
            Ok(u) => u,
            Err(why) => return Ok(Some(why)),
        };
        if let Some(working) = self.working.as_ref().filter(|w| w.id == u.id) {
            working.record.stop();
            working.pause.store(true, Ordering::Relaxed);
            info!("stopping {}", u.id);
            return Ok(None);
        }
        let progress = match self.undertaking(&u.id, &u.brief) {
            Ok(record) => {
                record.stop();
                self.hear_from_work(&u.id, &record, now)?;
                record.state().progress()
            }
            Err(e) => {
                warn!("cannot open the undertaking {}: {e}", u.id);
                u.summary.unwrap_or_default()
            }
        };
        info!("stopped {}", u.id);
        self.dropped(&u.id, &progress)?;
        Ok(None)
    }

    /// The work `id` ends because she stopped it.
    fn dropped(&mut self, id: &str, progress: &str) -> Result<()> {
        self.record(Event::Concluded {
            id: id.to_owned(),
            outcome: "dropped".into(),
            summary: self
                .prompts
                .fill("stopped", &[("progress", progress.to_owned())]),
        })?;
        Ok(())
    }

    /// Ask a sitting in progress to stop after its current step. Returns
    /// whether there was one.
    pub(super) fn set_work_aside(&mut self) -> bool {
        match &self.working {
            Some(working) => {
                working.pause.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// A sitting ended.
    pub(super) fn on_worked(
        &mut self,
        id: &str,
        before: u64,
        report: &Report,
        progress: String,
        record: &Undertaking,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.working = None;
        // Whatever it said and kept up to the end.
        self.hear_from_work(id, record, now)?;
        let steps = u32::try_from(report.steps.saturating_sub(before)).unwrap_or(u32::MAX);
        let secs = f64::from(self.cfg.work.rest_minutes) * 60.0 * f64::from(self.cfg.tempo);
        #[allow(clippy::cast_possible_truncation)]
        let rest = Duration::seconds(secs as i64);
        let outcome = match &report.ended {
            Ended::Finished => "finished",
            Ended::Blocked => "blocked",
            Ended::GaveUp => "gave_up",
            Ended::Dropped => {
                info!("{id} stopped after {} steps", report.steps);
                self.dropped(id, &progress)?;
                self.agenda.work = Some(now + rest);
                return Ok(());
            }
            Ended::Failed(why) => {
                warn!("the sitting on {id} failed: {why}");
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: format!("Could not think: {}", clip(why, 200)),
                    steps,
                })?;
                self.agenda.work = Some(now + Duration::minutes(10).max(rest));
                return Ok(());
            }
            Ended::Waiting => {
                let question = report.asking.clone().unwrap_or_default();
                info!("{id} waits on an answer: {}", clip(&question, 120));
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: progress,
                    steps,
                })?;
                self.record(Event::Asked {
                    id: id.to_owned(),
                    question: question.clone(),
                })?;
                if let Some(u) = self.ghost.memory().undertaking(id)? {
                    let ask = self.prompts.fill(
                        "ask_them",
                        &[
                            ("brief", clip(&u.brief, 160)),
                            ("question", clip(&question, 600)),
                        ],
                    );
                    self.intend(now + Duration::minutes(1), ask, u.asked_by, u.place, true)?;
                    self.refresh_intentions()?;
                }
                // Something else may be waiting to be done meanwhile.
                self.agenda.work = Some(now + rest);
                return Ok(());
            }
            Ended::OutOfSteps | Ended::OutOfTime | Ended::Paused | Ended::Stalled => {
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: progress,
                    steps,
                })?;
                self.agenda.work = Some(now + rest);
                return Ok(());
            }
        };
        info!("{id} {outcome} after {} steps", report.steps);
        let summary = if report.summary.trim().is_empty() {
            progress
        } else {
            report.summary.trim().to_owned()
        };
        let undertaking = self.ghost.memory().undertaking(id)?;
        self.record(Event::Concluded {
            id: id.to_owned(),
            outcome: outcome.to_owned(),
            summary: summary.clone(),
        })?;
        if let Some(lesson) = &report.lesson {
            self.on_practice(lesson)?;
        }
        // What a review of her own writing came to becomes part of her craft.
        if id.starts_with(super::public::REVIEW) && outcome == "finished" {
            self.learn(super::public::conclusions(&summary))?;
        }
        // If someone asked, she means to tell them. How and whether is
        // decided when the moment comes, like any other intention.
        if let Some(u) = undertaking
            && (u.asked_by.is_some() || u.place.is_some())
        {
            let how = self.prompts.text(match outcome {
                "finished" => "how_finished",
                "blocked" => "how_blocked",
                _ => "how_gave_up",
            });
            // What she made, if it is files, and whether this place can
            // take a file at all.
            let made = if report.files.is_empty() {
                String::new()
            } else {
                let can_send = self.workshop.machine.is_some()
                    && u.place
                        .as_deref()
                        .and_then(ghost_core::social::split_key)
                        .is_some_and(|(channel, _)| self.can(channel, crate::Can::Files));
                self.prompts.fill(
                    "made",
                    &[
                        ("files", report.files.join(", ")),
                        (
                            "send",
                            self.prompts
                                .text(if can_send { "made_send" } else { "made_tell" })
                                .to_owned(),
                        ),
                    ],
                )
            };
            let tell = self.prompts.fill(
                "tell_them",
                &[
                    ("brief", clip(&u.brief, 160)),
                    ("how", how.to_owned()),
                    ("summary", clip(&summary, 6000)),
                    ("made", made),
                ],
            );
            self.intend(now + Duration::minutes(1), tell, u.asked_by, u.place, true)?;
            self.refresh_intentions()?;
        }
        // On to the next thing, if there is one.
        self.agenda.work = Some(now + rest);
        Ok(())
    }
}
