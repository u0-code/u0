//! The loop.
//!
//! One step: the model reads the transcript and either says something or
//! asks for tools; the tools run; their output joins the transcript. That is
//! all a step is. What makes it hold up over hours is everything around it:
//!
//! - **State is outside the context.** The plan, the notes and the outcome
//!   live in the [`Undertaking`], and the transcript is saved after every
//!   step. The process can stop anywhere and be picked up.
//! - **Context is folded only when it has to be.** The transcript may fill
//!   most of the model's window. When it nears that, long tool output is
//!   trimmed first, then old output is dropped; if that is not enough the
//!   worker writes itself a handoff and goes on from the task, the plan, its
//!   notes and the last stretch of the transcript, word for word. Nothing
//!   is lost to this: every item is also appended to a history that is
//!   never rewritten.
//! - **Pictures are shed first.** A tool may return images. They are kept
//!   on disk and named in the result, shown to a brain that sees, and taken
//!   out of the transcript once a few newer ones have been looked at, or
//!   before any text when the transcript is folded.
//! - **How to finish is said beforehand.** The instructions say what
//!   finished means and that checking is part of the work; work that has
//!   outlived a context is told again, at length, when it is picked up.
//!   `done` then ends the work as reported. Nothing is sent back and nobody
//!   reads it over.
//! - **Stalls are noticed.** Talking without acting, repeating the same call,
//!   and running low on steps each get said out loud to the model.
//! - **Parts run alongside.** A worker that can equip helpers hands out
//!   parts of the work that run while it goes on, and hears from them
//!   between its steps, as it hears from outside: see [`crate::parts`].
//!
//! The loop itself is here. What surrounds it is beside it: what the model
//! is told (`instructions`), how its calls are run and their results taken
//! in (`calls`), and how the transcript is kept within its window
//! (`context`).

mod calls;
mod context;
mod instructions;
#[cfg(test)]
mod tests;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use ghost_brain::{Brain, Effort, Item, Stop, Turn, Usage};
use serde::Serialize;
use serde_json::json;

use self::context::{Gauge, PICTURES_KEPT, chars, unsee};

use crate::{
    parts::Parts,
    prompts::{Prompts, Shared},
    tool::Toolbox,
    tools::Handoff,
    undertaking::{MailKind, Outcome, State, Undertaking},
};

#[derive(Debug, Clone)]
pub struct Limits {
    /// Steps in one run.
    pub max_steps: u32,
    /// Wall-clock seconds in one run.
    pub max_seconds: u64,
    /// The transcript is folded when it is about to pass this many tokens.
    /// Left unset, that is most of the model's context window, or a small
    /// fixed size for a model whose window is not known.
    pub context_tokens: Option<u64>,
    /// Output tokens per step, thinking included. Left unset, it is what
    /// the model is known to manage up to a generous ceiling, or a small
    /// fixed amount for a model nothing is known about.
    pub max_output: Option<u32>,
    /// How hard the model thinks at each step. `None` leaves it to the
    /// provider.
    pub effort: Option<Effort>,
    pub temperature: Option<f32>,
    /// Parts of the work that may run at once. A part handed out beyond
    /// this waits until one of them ends.
    pub parts: usize,
    /// Steps one part may take, over all the sittings it runs in. A part
    /// is a piece of work in its own right, carried across sittings like
    /// any other, so it is given what a whole run is given.
    pub part_steps: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 200,
            max_seconds: 3600,
            context_tokens: None,
            max_output: None,
            effort: None,
            temperature: None,
            parts: 4,
            part_steps: 200,
        }
    }
}

/// Why a run stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "why", content = "detail", rename_all = "snake_case")]
pub enum Ended {
    Finished,
    Blocked,
    GaveUp,
    OutOfSteps,
    OutOfTime,
    /// It kept talking without doing anything.
    Stalled,
    /// Asked to stop. Everything needed to go on is on disk.
    Paused,
    /// It asked whoever the work is for a question, and is waiting for
    /// the answer to be added to the task. The question is in the report.
    Waiting,
    /// Whoever it is for stopped it from outside. It ends here.
    Dropped,
    /// The model could not be reached, or refused.
    Failed(String),
}

impl From<Outcome> for Ended {
    /// How a run ends when the work has an outcome on record.
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Finished => Self::Finished,
            Outcome::Blocked => Self::Blocked,
            Outcome::GaveUp => Self::GaveUp,
            Outcome::Dropped => Self::Dropped,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ended: Ended,
    /// The worker's own account of how it went, if it gave one.
    pub summary: String,
    /// What it is waiting to be told, if it stopped to ask.
    pub asking: Option<String>,
    /// The files that are the result, if it finished and the result is files.
    pub files: Vec<String>,
    /// What the work taught about working, if the worker said.
    pub lesson: Option<String>,
    /// Totals for the whole undertaking, across runs.
    pub steps: u64,
    pub sittings: u32,
    pub usage: Usage,
    pub seconds: f64,
}

pub struct Worker {
    pub brain: Arc<dyn Brain>,
    pub tools: Toolbox,
    pub undertaking: Arc<Undertaking>,
    pub limits: Limits,
    /// Who is doing the work, if it matters: a paragraph put before the
    /// working instructions. Empty for work nobody in particular is doing.
    pub who: String,
    /// What the worker is told about where it is.
    pub machine: String,
    /// Set from outside to stop after the step in progress.
    pub pause: Arc<AtomicBool>,
    /// How to equip a helper. With this, the worker can hand
    /// self-contained parts of the work to other workers, each with a
    /// context of its own, that run alongside it and report back.
    pub helpers: Option<ToolsFor>,
    /// What it is told, and what its tools are said to be. Looked at again
    /// before each step, so that an edit reaches work already running.
    pub prompts: Shared,
    /// Agents installed on this machine that a part can be handed to
    /// instead of a helper of ours.
    pub agents: Vec<crate::agents::Agent>,
}

/// Builds the tools for work on a given undertaking: the same machine and
/// the same web, with a record of its own.
pub type ToolsFor = Arc<dyn Fn(&Arc<Undertaking>) -> Toolbox + Send + Sync>;

/// Make a transcript picked up from disk whole again: every call the model
/// made has to have a result, even if the process died before one came.
fn mend(items: &mut Vec<Item>, words: &Prompts) {
    let Some(at) = items
        .iter()
        .rposition(|item| matches!(item, Item::Assistant { .. }))
    else {
        return;
    };
    let Item::Assistant { calls, .. } = &items[at] else {
        return;
    };
    let answered: Vec<&str> = items[at + 1..]
        .iter()
        .filter_map(|item| match item {
            Item::Result { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let missing: Vec<Item> = calls
        .iter()
        .filter(|c| !answered.contains(&c.id.as_str()))
        .enumerate()
        .map(|(nth, c)| {
            let said = words.text(if nth == 0 {
                "interrupted"
            } else {
                "not_started"
            });
            Item::result(&c.id, &c.name, said, true)
        })
        .collect();
    // Results belong directly after the calls, before anything said since.
    let after = at + 1 + answered.len();
    items.splice(after..after, missing);
}

fn too_long(error: &ghost_brain::Error) -> bool {
    matches!(error, ghost_brain::Error::TooLong { .. })
}

/// What the model said at a step, as its log keeps it: as it was written,
/// line breaks and all, so that whoever reads the log can draw it the way
/// it was written (a list, a table), trimmed and cut at `max` characters.
pub(crate) fn as_written(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>().trim_end())
    }
}

/// How much of what the model said at a step its log keeps.
pub(crate) const SAID_KEPT: usize = 4000;

pub(crate) fn preview(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

impl Worker {
    /// Save the transcript, and add whatever in it is new to the history.
    fn save(&self, items: &[Item], recorded: &mut usize) {
        if let Some(new) = items.get(*recorded..)
            && !new.is_empty()
        {
            self.undertaking.remember(new);
        }
        *recorded = items.len();
        self.undertaking.save_transcript(items);
    }

    fn report(&self, ended: Ended) -> Report {
        let state = self.undertaking.state();
        Report {
            ended,
            asking: state.waiting,
            lesson: state.finish.as_ref().and_then(|f| f.lesson.clone()),
            files: state
                .finish
                .as_ref()
                .map(|f| f.files.clone())
                .unwrap_or_default(),
            summary: state.finish.map(|f| f.summary).unwrap_or_default(),
            steps: state.steps,
            sittings: state.sittings + 1,
            usage: state.usage,
            seconds: state.seconds,
        }
    }

    /// Work until it is finished, or until something stops it. Parts of
    /// the work that are running when it stops stop after the step they
    /// are on, and are waited for, so that what they spent is counted.
    pub async fn run(&self) -> Report {
        if let Some(finish) = self.undertaking.state().finish {
            return self.report(finish.outcome.into());
        }
        let started = Instant::now();
        let parts = self
            .helpers
            .as_ref()
            .map(|equip| Parts::new(self, equip.clone(), started));
        let ended = self.sit(started, parts.as_ref()).await;
        if let Some(parts) = &parts {
            parts.settle().await;
        }
        self.report(ended)
    }

    /// One sitting, until something ends it.
    async fn sit(&self, started: Instant, parts: Option<&Arc<Parts>>) -> Ended {
        // The handoff tool is there at every step, not only when it is
        // wanted: a request whose tools differ from the last one's reads
        // nothing from the provider's cache.
        let tools = self
            .tools
            .clone()
            .with(Handoff::new(self.undertaking.clone()));
        let tools = match parts {
            Some(parts) => parts.equip(tools),
            None => tools,
        };
        let mut shown = self.shown(&tools, self.words().0);
        let mut gauge = Gauge {
            measured: None,
            fixed: shown.weight(),
        };
        let (mut items, mut recorded) = match self
            .undertaking
            .transcript()
            .filter(|items| !items.is_empty())
        {
            // What was on disk is in the history already; what mending
            // adds to it is not.
            Some(items) => {
                let known = items.len();
                (items, known)
            }
            None => (self.opening(&shown.words), 0),
        };
        mend(&mut items, &shown.words);

        // Where this sitting started, in tokens. A transcript picked up from
        // disk counts as starting from the task alone.
        let mut base = gauge.tokens(&self.opening(&shown.words));
        let mut idle = 0;
        let mut last_signature = String::new();
        let mut repeats = 0;
        let mut overflowed = false;
        let warn_at = self.limits.max_steps * 4 / 5;

        for n in 1..=self.limits.max_steps {
            // Stopped from outside since the last step: it ends here, as
            // whoever stopped it said, and not as paused.
            if let Some(finish) = self.undertaking.state().finish {
                self.save(&items, &mut recorded);
                return finish.outcome.into();
            }
            if self.pause.load(Ordering::Relaxed) {
                return Ended::Paused;
            }
            if started.elapsed().as_secs() >= self.limits.max_seconds {
                return Ended::OutOfTime;
            }
            // Whatever reached the work since the last step, from outside
            // or from its parts, is put in front of the worker before it
            // goes on.
            let mail = self.undertaking.take_inbox();
            let answered = mail.iter().any(|m| m.kind == MailKind::Added);
            if answered {
                // Whatever it was waiting to hear, this is the answer.
                self.undertaking.update(State::answered);
            }
            // What it is told, as the files say it now.
            if let (words, true) = self.words() {
                shown = self.shown(&tools, words);
                gauge.fixed = shown.weight();
            }
            for mail in mail {
                tracing::info!("reached the work: {}", preview(&mail.text, 160));
                items.push(Item::user(match mail.kind {
                    MailKind::Added => shown.words.fill("added", &[("added", mail.text)]),
                    // Written by the part as it is to be read.
                    MailKind::Told | MailKind::Asked | MailKind::Ended => mail.text,
                }));
            }
            // Still waiting to be answered: what came from its parts
            // meanwhile is kept in the transcript for when it goes on.
            if !answered && self.undertaking.state().waiting.is_some() {
                self.save(&items, &mut recorded);
                return Ended::Waiting;
            }
            if let Some(parts) = parts {
                parts.pump();
            }

            let mut extra = Usage::default();
            if items.len() > 1 && gauge.tokens(&items) > self.ceiling(base) {
                let ceiling = self.ceiling(base);
                let (spent, began) = self
                    .fold(
                        &mut items,
                        &mut recorded,
                        &shown,
                        &tools,
                        &mut gauge,
                        base,
                        ceiling,
                        true,
                    )
                    .await;
                extra = spent;
                base = began.unwrap_or(base);
            }
            if unsee(&mut items, PICTURES_KEPT, &shown.words) > 0 {
                gauge.reweigh(&items);
            }

            let asked = Instant::now();
            let turn = Turn {
                system: shown.system.clone(),
                items: items.clone(),
                tools: shown.specs.clone(),
                max_tokens: self.max_output(),
                temperature: self.limits.temperature,
                effort: self.limits.effort,
            };
            // The time allowed is for the whole run, a model that does not
            // answer included: a step still waiting when it is up is left.
            let left = self
                .limits
                .max_seconds
                .saturating_sub(started.elapsed().as_secs())
                .max(30);
            let answered = tokio::time::timeout(
                std::time::Duration::from_secs(left + 30),
                self.brain.step(&turn),
            )
            .await;
            let Ok(answered) = answered else {
                tracing::warn!("the model had not answered when the time allowed ran out");
                self.save(&items, &mut recorded);
                return Ended::OutOfTime;
            };
            let step = match answered {
                Ok(step) => step,
                Err(e) if too_long(&e) && !overflowed => {
                    // The model's window is smaller than was thought, or the
                    // guess at the transcript's size was too low. Remember
                    // the size that was refused, so that the budget stays
                    // under it from now on, and make room without asking
                    // the model for anything.
                    overflowed = true;
                    let refused = gauge.tokens(&items);
                    tracing::warn!(
                        "the model refused a context of about {refused} tokens; folding under that"
                    );
                    self.undertaking.update(|state| {
                        state.refused_at =
                            Some(state.refused_at.map_or(refused, |was| was.min(refused)));
                    });
                    let ceiling = self.budget();
                    let (_, began) = self
                        .fold(
                            &mut items,
                            &mut recorded,
                            &shown,
                            &tools,
                            &mut gauge,
                            0,
                            ceiling,
                            false,
                        )
                        .await;
                    base = began.unwrap_or(base);
                    continue;
                }
                Err(e) => {
                    self.save(&items, &mut recorded);
                    return Ended::Failed(e.to_string());
                }
            };
            overflowed = false;
            let model_ms = asked.elapsed().as_millis();
            let mut usage = step.usage;
            usage.add(extra);
            let (said, calls, why) = (step.text.clone(), step.calls.clone(), step.stop);
            let thought_out = why == Stop::Length && said.is_empty() && calls.is_empty();
            tracing::info!(
                "step {n}: read {} ({} cached), wrote {} ({} thinking){}",
                step.usage.input,
                step.usage.cached,
                step.usage.output,
                step.usage.thought,
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {}", preview(&said, 160))
                }
            );
            // What this step read and wrote is the size of the transcript
            // now. What a handoff cost before it is not part of that.
            let size = step.usage.input + step.usage.output;
            items.push(step.into_item());
            gauge.measured = Some((size, chars(&items)));

            let mut logged = Vec::new();
            if calls.is_empty() {
                idle += 1;
                if idle >= 3 {
                    self.save(&items, &mut recorded);
                    return Ended::Stalled;
                }
                items.push(Item::user(shown.words.text(if thought_out {
                    "thought_out"
                } else if why == Stop::Length {
                    "cut_off"
                } else {
                    "no_call"
                })));
            } else {
                idle = 0;
                let (ran, signature) = self
                    .run_calls(&tools, &shown.words, &calls, &mut items)
                    .await;
                logged = ran;
                // A tool may have brought others, as `load_tools` does:
                // they are offered from the next step on.
                if tools.names().len() != shown.specs.len() {
                    shown = self.shown(&tools, shown.words.clone());
                    gauge.fixed = shown.weight();
                }
                if !signature.is_empty() && signature == last_signature {
                    repeats += 1;
                    if repeats == 2 {
                        items.push(Item::user(shown.words.text("repeating")));
                        repeats = 0;
                    }
                } else {
                    repeats = 0;
                    last_signature = signature;
                }
            }

            let finish = self.undertaking.update(|state| {
                state.steps += 1;
                state.usage.add(usage);
                state.seconds += asked.elapsed().as_secs_f64();
                state.finish.clone()
            });
            self.undertaking.log(&json!({
                "at": chrono::Utc::now().to_rfc3339(),
                "step": n,
                "read": usage.input,
                "cached": usage.cached,
                "wrote": usage.output,
                "thought": usage.thought,
                "model_ms": model_ms,
                "said": as_written(&said, SAID_KEPT),
                "calls": logged,
            }));
            self.save(&items, &mut recorded);

            if let Some(finish) = finish {
                return finish.outcome.into();
            }
            if n == warn_at && warn_at > 0 {
                items.push(Item::user(shown.words.fill(
                    "steps_left",
                    &[
                        ("used", n.to_string()),
                        ("steps", self.limits.max_steps.to_string()),
                    ],
                )));
            }
        }
        Ended::OutOfSteps
    }
}
