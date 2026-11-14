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
    worker::{opening_text, system_prompt},
};
use tracing::{info, warn};

use super::{Mind, Outcome};
use crate::{Result, actions::clip};

const WHO: &str = "What follows is something you took on yourself. Do it the way you would do \
anything you care about: properly, and in your own way. The result is yours and will be read as \
yours. When you write the final summary, write it as what you would tell the person who asked, \
or yourself if nobody did.";

/// A sitting in progress.
pub(super) struct Working {
    pub id: String,
    /// Set to stop the sitting after the step it is on.
    pub pause: Arc<AtomicBool>,
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
        if self.working.is_some() {
            return Ok(());
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
            let said = "No answer came to your question. Go on with your best judgement, \
                        and say in your summary what you assumed.";
            if let Err(e) = self
                .undertaking(&stale.id, &stale.brief)
                .and_then(|u| u.add(said))
            {
                warn!("cannot go on with {} unanswered: {e}", stale.id);
            }
            self.record(Event::Added {
                id: stale.id,
                text: said.to_owned(),
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
            None => "You have no computer to run things on for this: only what you can look up and think through."
                .to_owned(),
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
        let tools = equip(&undertaking);
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
        let who = format!("{}\n\n{WHO}{}", self.who(now)?, self.practice_block()?);
        info!("sitting down to {} ({})", next.id, clip(&next.brief, 80));
        self.working = Some(Working {
            id: next.id.clone(),
            pause: pause.clone(),
        });
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
                };
                self.jobs.spawn(async move {
                    let report = worker.run().await;
                    let progress = worker.undertaking.state().progress();
                    Outcome::Worked {
                        id,
                        before,
                        report,
                        progress,
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
                ];
                tool_server.extend(self.workshop.runner_args.iter().cloned());
                let seat = claude::Seat {
                    agent,
                    system: system_prompt(&who, &place, &limits),
                    opening: opening_text(&undertaking.state()),
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
                    let progress = seat.undertaking.state().progress();
                    Outcome::Worked {
                        id,
                        before,
                        report,
                        progress,
                    }
                });
            }
        }
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
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.working = None;
        let steps = u32::try_from(report.steps.saturating_sub(before)).unwrap_or(u32::MAX);
        let secs = f64::from(self.cfg.work.rest_minutes) * 60.0 * f64::from(self.cfg.tempo);
        #[allow(clippy::cast_possible_truncation)]
        let rest = Duration::seconds(secs as i64);
        let outcome = match &report.ended {
            Ended::Finished => "finished",
            Ended::Blocked => "blocked",
            Ended::GaveUp => "gave_up",
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
                    self.intend(
                        now + Duration::minutes(1),
                        format!(
                            "ask them what you need to know to go on with what you took on (\"{}\"). \
                             Your question, to put in your own words: {}",
                            clip(&u.brief, 160),
                            clip(&question, 600)
                        ),
                        u.asked_by,
                        u.place,
                        true,
                    )?;
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
            let how = match outcome {
                "finished" => "It is done",
                "blocked" => "You could not finish it",
                _ => "You gave up on it",
            };
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
                format!(
                    " What you made is in: {}. {}",
                    report.files.join(", "),
                    if can_send {
                        "Hand it over with `send_file`."
                    } else {
                        "It cannot be sent as a file here, so tell them what is in it."
                    }
                )
            };
            self.intend(
                now + Duration::minutes(1),
                format!(
                    "tell them what came of what you took on (\"{}\"). {how}. What you found: {}{made}",
                    clip(&u.brief, 160),
                    clip(&summary, 6000)
                ),
                u.asked_by,
                u.place,
                true,
            )?;
            self.refresh_intentions()?;
        }
        // On to the next thing, if there is one.
        self.agenda.work = Some(now + rest);
        Ok(())
    }
}
