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
use ghost_core::{
    journal::Event,
    life::Phase,
    memory::{Person, PlaceState},
    will::{self, Stage},
};
use ghost_work::{Ended, Limits, Report, Undertaking, Worker, tools};
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

impl Mind {
    /// The brain she works with, if she has one that can.
    fn work_brain(&self) -> Option<Arc<dyn ghost_brain::Brain>> {
        [&self.think, &self.tick]
            .into_iter()
            .find(|b| b.works())
            .cloned()
    }

    /// Whether taking things on is possible at all in this body.
    pub(super) fn can_work(&self) -> bool {
        self.cfg.work.enabled && self.work_brain().is_some()
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
        self.record(Event::Undertook {
            id,
            brief: brief.to_owned(),
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
        let Some(brain) = self.work_brain().filter(|_| self.cfg.work.enabled) else {
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
        // Whatever has waited longest since it was last touched.
        let open = self.ghost.memory().undertakings(true, 20)?;
        let Some(next) = open.into_iter().min_by_key(|u| u.last_at) else {
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

        let dir = self.ghost.dir().join("work").join(&next.id);
        let undertaking = match Undertaking::open(dir, Some(&next.brief)) {
            Ok(u) => Arc::new(u),
            Err(e) => {
                warn!("cannot open the undertaking {}: {e}", next.id);
                self.record(Event::Concluded {
                    id: next.id,
                    outcome: "blocked".into(),
                    summary: format!("Its files could not be opened: {e}"),
                })?;
                return Ok(());
            }
        };
        let before = undertaking.state().steps;
        let (tools, place) = match &self.workshop.machine {
            Some(machine) => (
                tools::standard(machine.clone(), &undertaking),
                machine.describe(),
            ),
            None => (
                tools::desk(&undertaking),
                "You have no computer to run things on for this: only what you can look up and think through."
                    .to_owned(),
            ),
        };
        let pause = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            brain,
            tools: tools::with_web(tools, &self.workshop.web),
            undertaking,
            limits: Limits {
                max_steps: self.cfg.work.sitting_steps,
                max_seconds: u64::from(self.cfg.work.sitting_minutes) * 60,
                context_tokens: self.cfg.work.context_tokens,
                ..Limits::default()
            },
            who: format!("{}\n\n{WHO}", self.self_prompt(now)?),
            machine: place,
            pause: pause.clone(),
        };
        info!("sitting down to {} ({})", next.id, clip(&next.brief, 80));
        self.working = Some(Working {
            id: next.id.clone(),
            pause,
        });
        let id = next.id;
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
            self.intend(
                now + Duration::minutes(1),
                format!(
                    "tell them what came of what you took on (\"{}\"). {how}. What you found: {}",
                    clip(&u.brief, 160),
                    clip(&summary, 900)
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
