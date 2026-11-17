//! Parts of a piece of work, each done by a worker of its own while the
//! one that handed it out goes on with its share.
//!
//! `delegate` starts a part and returns at once with its number. A part is
//! an undertaking in the same database, `<parent>/<n>`, with a transcript,
//! a record and a step budget of its own, so it is picked up where it
//! stopped like any other work. It talks to its parent through the
//! parent's inbox, the one whoever the parent is for adds to: what it
//! tells, what it asks and, when it ends, its report. The parent takes
//! these before each of its steps, as it takes what is added to its task.
//! It answers or redirects a part through the part's own inbox with
//! `steer`, and ends one with `stop`, which writes the same dropped outcome
//! as stopping any work from outside.
//!
//! Parts run while the sitting that handed them out runs. When it ends,
//! each stops after the step it is on, and one that has not ended starts
//! again when the parent's next sitting does. Only so many run at once; the
//! rest wait their turn, in the order they were handed out.
//!
//! A part can be handed a step of the plan. The step is marked in progress
//! with the part's number on it, and when the part ends the step takes its
//! outcome: done, with the first line of the part's report as its result,
//! or failed. The parent still has the whole report in its inbox.
//!
//! A part hands out no parts of its own. With one level the number of
//! workers is never more than the cap, and every part is where its parent,
//! and whoever the parent is for, can see it.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use ghost_brain::{Brain, ToolSpec, Usage};
use serde_json::{Value, json};

use crate::{
    agents::{self, Agent},
    prompts::{self, Loop, Shared, Store},
    tool::{self, Tool, Toolbox},
    tools::{self, clip, list, outcome_of, step_number},
    undertaking::{Finish, MailKind, Outcome, Progress, Undertaking},
    worker::{Ended, Limits, ToolsFor, Worker, preview},
};

/// How often a worker waiting on its parts looks at its inbox.
const POLL: Duration = Duration::from_millis(100);
/// How much of a part's report reaches its parent.
const REPORT: usize = 8000;
/// The longest a single `wait` may be.
const LONGEST_WAIT: f64 = 3600.0;

/// The parts of one piece of work, for one sitting of it.
pub(crate) struct Parts {
    parent: Arc<Undertaking>,
    brain: Arc<dyn Brain>,
    equip: ToolsFor,
    limits: Limits,
    machine: String,
    prompts: Shared,
    /// The parent's own pause, set from outside.
    pause: Arc<AtomicBool>,
    /// Set when the sitting ends: each part stops after the step it is on,
    /// and none starts.
    halt: Arc<AtomicBool>,
    /// When the sitting's time is up. No wait goes past it.
    deadline: Instant,
    /// The parts running now, by number.
    running: Mutex<HashMap<u64, tokio::task::JoinHandle<()>>>,
    /// Agents installed here that a part can be handed to.
    agents: Vec<Agent>,
}

impl Parts {
    /// The parts of the work `worker` does, which it equips with `equip`,
    /// for the sitting that began at `started`.
    pub(crate) fn new(worker: &Worker, equip: ToolsFor, started: Instant) -> Arc<Self> {
        Arc::new(Self {
            parent: worker.undertaking.clone(),
            brain: worker.brain.clone(),
            equip,
            limits: worker.limits.clone(),
            machine: worker.machine.clone(),
            prompts: worker.prompts.clone(),
            pause: worker.pause.clone(),
            halt: Arc::new(AtomicBool::new(false)),
            deadline: started + Duration::from_secs(worker.limits.max_seconds),
            running: Mutex::new(HashMap::new()),
            agents: worker.agents.clone(),
        })
    }

    /// `tools`, with what a worker that hands out parts needs: `delegate`,
    /// `steer`, `stop`, `wait` and `parts`, and a `done` that will not call
    /// the work finished while a part of it has not ended.
    pub(crate) fn equip(self: &Arc<Self>, tools: Toolbox) -> Toolbox {
        let parts = self.clone();
        tools
            .wrapping("done", move |done| Arc::new(Finishing { done, parts }))
            .with(Delegate(self.clone()))
            .with(Steer(self.clone()))
            .with(Stop(self.clone()))
            .with(Wait(self.clone()))
            .with(List(self.clone()))
    }

    fn running(&self) -> MutexGuard<'_, HashMap<u64, tokio::task::JoinHandle<()>>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn text(&self, key: &str) -> String {
        prompts::hold(&self.prompts).text(key).to_owned()
    }

    fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        prompts::hold(&self.prompts).fill(key, pairs)
    }

    /// Start whatever parts are ready, as far as there is room.
    pub(crate) fn pump(self: &Arc<Self>) {
        let mut running = self.running();
        self.start(&mut running);
    }

    /// Whether a part is running now.
    fn busy(&self) -> bool {
        !self.running().is_empty()
    }

    fn start(self: &Arc<Self>, running: &mut HashMap<u64, tokio::task::JoinHandle<()>>) {
        if self.halt.load(Ordering::Relaxed) {
            return;
        }
        let room = self.limits.parts.max(1).saturating_sub(running.len());
        if room == 0 {
            return;
        }
        // Not ended and not running, and not waiting on an answer that has
        // not come: oldest first.
        let ready: Vec<(u64, Undertaking)> = self
            .parent
            .parts()
            .into_iter()
            .filter(|(n, state)| state.finish.is_none() && !running.contains_key(n))
            .filter_map(|(n, state)| {
                let part = self.parent.part(n).ok()?;
                (state.waiting.is_none() || part.pending() > 0).then_some((n, part))
            })
            .take(room)
            .collect();
        for (n, part) in ready {
            let this = self.clone();
            let part = Arc::new(part);
            running.insert(
                n,
                tokio::spawn(tracing::Instrument::in_current_span(async move {
                    this.sit(n, part).await;
                })),
            );
        }
    }

    /// One part's run, from where it stands until it ends or the sitting
    /// does.
    async fn sit(self: Arc<Self>, n: u64, part: Arc<Undertaking>) {
        let before = part.state();
        let left = u64::from(self.limits.part_steps).saturating_sub(before.steps);
        let ended = if left == 0 {
            Ended::OutOfSteps
        } else if let Some(name) = before.agent.clone() {
            // Handed to an agent installed here, which steps it itself.
            match self.agents.iter().find(|a| a.name == name) {
                Some(agent) => {
                    let task = self.fill("agent_task", &[("task", before.task_text())]);
                    let go_on = self.text("agent_go_on");
                    agents::run(agent, &part, &task, &go_on, &self.halt, self.deadline).await
                }
                None => Ended::Failed(self.fill("no_agent", &[("agent", name)])),
            }
        } else {
            let tools = (self.equip)(&part)
                .with(Told {
                    tell: tools::Tell::new(part.clone()),
                    part: n,
                    parent: self.parent.clone(),
                    prompts: self.prompts.clone(),
                })
                .with(Asks {
                    ask: tools::Ask::new(part.clone()),
                    part: n,
                    parent: self.parent.clone(),
                    prompts: self.prompts.clone(),
                });
            let worker = Worker {
                brain: self.brain.clone(),
                tools,
                undertaking: part.clone(),
                limits: Limits {
                    max_steps: u32::try_from(left).unwrap_or(u32::MAX),
                    ..self.limits.clone()
                },
                who: self.text("part_who"),
                machine: self.machine.clone(),
                pause: self.halt.clone(),
                helpers: None,
                prompts: self.prompts.clone(),
                agents: Vec::new(),
            };
            worker.run().await.ended
        };
        // What it spent is spent on the work it is part of.
        let after = part.state();
        self.parent
            .update(|state| state.usage.add(spent(before.usage, after.usage)));
        if let Some(report) = self.ending(n, &part, &ended)
            && let Err(e) = self.parent.post(MailKind::Ended, &report)
        {
            tracing::warn!("part {n} could not report to {}: {e}", self.parent.id());
        }
        // Its report is posted before it leaves the running, so that a
        // wait that finds nothing running has already seen it.
        let mut running = self.running();
        running.remove(&n);
        self.start(&mut running);
    }

    /// What a part that ended reports to its parent, if it ended. A part
    /// that ran out of steps, kept talking without acting or could not
    /// reach its model is ended here, so that it is not started again.
    fn ending(&self, n: u64, part: &Undertaking, ended: &Ended) -> Option<String> {
        let end = |outcome: Outcome| {
            part.update(|state| {
                if state.finish.is_none() {
                    state.finish = Some(Finish {
                        outcome,
                        summary: String::new(),
                        files: Vec::new(),
                        lesson: None,
                        at: Utc::now(),
                    });
                }
            });
        };
        let how = match ended {
            // It goes on later: in this sitting once it is answered, or in
            // the next.
            Ended::Paused | Ended::OutOfTime | Ended::Waiting => return None,
            Ended::Finished => self.text("part_finished"),
            Ended::Blocked => self.text("part_blocked"),
            Ended::GaveUp => self.text("part_gave_up"),
            Ended::Dropped => self.text("part_dropped"),
            Ended::OutOfSteps => {
                end(Outcome::GaveUp);
                self.text("part_ran_out")
            }
            Ended::Stalled => {
                end(Outcome::GaveUp);
                self.text("part_stalled")
            }
            Ended::Failed(why) => {
                end(Outcome::Blocked);
                self.fill("part_failed", &[("why", preview(why, 160))])
            }
        };
        let state = part.state();
        let latest = state.latest();
        let finished = state.finish.as_ref().map(|f| f.outcome) == Some(Outcome::Finished);
        let result = self.fill(
            "part_result",
            &[
                ("part", n.to_string()),
                ("how", how.clone()),
                (
                    "line",
                    preview(
                        latest.lines().find(|l| !l.trim().is_empty()).unwrap_or(""),
                        240,
                    ),
                ),
            ],
        );
        let mut report = self.fill(
            "part_ended",
            &[
                ("part", n.to_string()),
                ("how", how),
                ("steps", state.steps.to_string()),
                ("report", clip(&latest, REPORT / 2, REPORT / 2)),
            ],
        );
        for step in self.mark(n, finished, &result) {
            let key = if finished {
                "part_step_done"
            } else {
                "part_step_failed"
            };
            report.push_str(&self.fill(key, &[("step", step.to_string())]));
        }
        Some(report)
    }

    /// Mark the steps of the plan part `n` was handed, and that are still
    /// open, done if it `finished` and failed if not, with `result`.
    /// Returns their numbers, counted from 1. A step closed meanwhile by
    /// the parent itself is left as the parent left it.
    fn mark(&self, n: u64, finished: bool, result: &str) -> Vec<usize> {
        self.parent.update(|state| {
            let mut marked = Vec::new();
            for (i, step) in state.plan.iter_mut().enumerate() {
                if step.part == Some(n) && !step.status.closed() {
                    step.status = if finished {
                        Progress::Done
                    } else {
                        Progress::Failed
                    };
                    step.result = Some(result.to_owned());
                    marked.push(i + 1);
                }
            }
            marked
        })
    }

    /// Why step `step` of the plan, counted from 1, cannot be handed to a
    /// part now, if it cannot.
    fn unready(&self, step: usize) -> Option<String> {
        let state = self.parent.state();
        let number = ("step", step.to_string());
        let Some(i) = step.checked_sub(1).filter(|&i| i < state.plan.len()) else {
            return Some(self.fill("no_step", &[number]));
        };
        let waiting = state.waiting_on(i);
        if !waiting.is_empty() {
            return Some(self.fill("step_not_ready", &[number, ("waiting", list(&waiting))]));
        }
        let taken = state.plan[i].part.filter(|&p| {
            state.plan[i].status == Progress::Doing
                && self
                    .parent
                    .part(p)
                    .is_ok_and(|part| part.state().finish.is_none())
        });
        taken.map(|p| self.fill("step_taken", &[number, ("part", p.to_string())]))
    }

    /// The sitting is over. Every part running stops after the step it is
    /// on, and is waited for. If the work itself has ended, its parts end
    /// with it; otherwise they are picked up at its next sitting.
    pub(crate) async fn settle(&self) {
        self.halt.store(true, Ordering::Relaxed);
        loop {
            let handles: Vec<_> = self.running().drain().map(|(_, h)| h).collect();
            if handles.is_empty() {
                break;
            }
            for handle in handles {
                if let Err(e) = handle.await {
                    tracing::warn!("a part of {} fell over: {e}", self.parent.id());
                }
            }
        }
        if self.parent.state().finish.is_some() {
            self.parent.stop_parts();
        }
    }

    /// The part a call names, or why there is none it can act on.
    fn named(&self, args: &Value) -> Result<(u64, Undertaking), tool::Outcome> {
        let n = match &args["part"] {
            Value::Number(n) => n.as_u64(),
            // "2", "part 2", "#2".
            Value::String(s) => s
                .trim()
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok(),
            _ => None,
        }
        .ok_or_else(|| tool::Outcome::fail("missing argument `part` (a number)"))?;
        let part = self
            .parent
            .part(n)
            .map_err(|_| tool::Outcome::fail(self.fill("no_part", &[("part", n.to_string())])))?;
        if part.state().finish.is_some() {
            return Err(tool::Outcome::fail(
                self.fill("part_over", &[("part", n.to_string())]),
            ));
        }
        Ok((n, part))
    }
}

/// What was spent between `before` and `after`.
fn spent(before: Usage, after: Usage) -> Usage {
    Usage {
        input: after.input.saturating_sub(before.input),
        cached: after.cached.saturating_sub(before.cached),
        output: after.output.saturating_sub(before.output),
        thought: after.thought.saturating_sub(before.thought),
    }
}

/// Hand part of the work to a worker of its own.
struct Delegate(Arc<Parts>);

#[async_trait]
impl Tool for Delegate {
    fn name(&self) -> &'static str {
        "delegate"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        // An agent can be named only where one is installed, and only one
        // of those.
        let mut arguments = json!({"task": {"type": "string"}, "step": {"type": "integer"}});
        if !self.0.agents.is_empty() {
            let names: Vec<&str> = self.0.agents.iter().map(|a| a.name.as_str()).collect();
            arguments["agent"] = json!({"type": "string", "enum": names});
        }
        prompts::spec(words, lp, "delegate", arguments, &["task"])
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let task = match tool::text(args, "task") {
            Ok(task) => task.trim(),
            Err(refusal) => return refusal,
        };
        if task.is_empty() {
            return tool::Outcome::fail("the task is empty");
        }
        let step = args.get("step").filter(|s| !s.is_null()).map(step_number);
        let step = match step {
            None => None,
            Some(Some(step)) => Some(step),
            Some(None) => {
                return tool::Outcome::fail(
                    self.0
                        .fill("no_step", &[("step", args["step"].to_string())]),
                );
            }
        };
        if let Some(why) = step.and_then(|step| self.0.unready(step)) {
            return tool::Outcome::fail(why);
        }
        let agent = match args.get("agent").and_then(Value::as_str).map(str::trim) {
            None | Some("") => None,
            Some(name) if self.0.agents.iter().any(|a| a.name == name) => Some(name.to_owned()),
            Some(name) => {
                return tool::Outcome::fail(self.0.fill("no_agent", &[("agent", name.to_owned())]));
            }
        };
        // Kept beside the work it is part of, under an id that says so.
        let part = match self.0.parent.open_part(task) {
            Ok(part) => part,
            Err(e) => return tool::Outcome::fail(format!("no part could be set up: {e}")),
        };
        let Some(n) = part.number() else {
            return tool::Outcome::fail("no part could be set up");
        };
        if agent.is_some() {
            part.update(|state| state.agent.clone_from(&agent));
        }
        if let Some(step) = step {
            self.0.parent.update(|state| {
                if let Some(on) = state.plan.get_mut(step - 1) {
                    on.status = Progress::Doing;
                    on.part = Some(n);
                    on.result = None;
                }
            });
        }
        self.0.pump();
        let number = ("part", n.to_string());
        let mut said = if self.0.running().contains_key(&n) {
            self.0.fill("part_started", &[number])
        } else {
            self.0.fill(
                "part_queued",
                &[number, ("running", self.0.limits.parts.max(1).to_string())],
            )
        };
        if let Some(step) = step {
            said.push_str(&self.0.fill("part_on_step", &[("step", step.to_string())]));
        }
        tool::Outcome::ok(said)
    }
}

/// Pass something on to a part.
struct Steer(Arc<Parts>);

#[async_trait]
impl Tool for Steer {
    fn name(&self) -> &'static str {
        "steer"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "steer",
            json!({"part": {"type": "integer"}, "text": {"type": "string"}}),
            &["part", "text"],
        )
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let text = match tool::text(args, "text") {
            Ok(text) => text.trim(),
            Err(refusal) => return refusal,
        };
        if text.is_empty() {
            return tool::Outcome::fail("there is nothing to pass on");
        }
        let (n, part) = match self.0.named(args) {
            Ok(named) => named,
            Err(refusal) => return refusal,
        };
        if let Err(e) = part.add(text) {
            return tool::Outcome::fail(format!("it could not be passed on: {e}"));
        }
        // A part that stopped to ask goes on now that it has its answer.
        self.0.pump();
        tool::Outcome::ok(self.0.fill("steered", &[("part", n.to_string())]))
    }
}

/// End a part.
struct Stop(Arc<Parts>);

#[async_trait]
impl Tool for Stop {
    fn name(&self) -> &'static str {
        "stop"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "stop",
            json!({"part": {"type": "integer"}}),
            &["part"],
        )
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let (n, part) = match self.0.named(args) {
            Ok(named) => named,
            Err(refusal) => return refusal,
        };
        let number = [("part", n.to_string())];
        if part.stop() {
            tool::Outcome::ok(self.0.fill("part_stopped", &number))
        } else {
            tool::Outcome::fail(self.0.fill("part_over", &number))
        }
    }
}

/// Wait until something arrives from a part, or for so long. What arrived
/// is not returned: it is taken from the inbox before the next step, as
/// everything in it is.
struct Wait(Arc<Parts>);

#[async_trait]
impl Tool for Wait {
    fn name(&self) -> &'static str {
        "wait"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "wait",
            json!({"seconds": {"type": "number"}}),
            &[],
        )
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let parts = &self.0;
        let seconds = args["seconds"]
            .as_f64()
            .filter(|s| s.is_finite())
            .unwrap_or(60.0)
            .clamp(0.0, LONGEST_WAIT);
        let until = (Instant::now() + Duration::from_secs_f64(seconds)).min(parts.deadline);
        loop {
            if parts.parent.pending() > 0 {
                return tool::Outcome::ok(parts.text("wait_arrived"));
            }
            if !parts.busy() {
                return tool::Outcome::ok(parts.text("wait_idle"));
            }
            if parts.pause.load(Ordering::Relaxed) || Instant::now() >= until {
                return tool::Outcome::ok(
                    parts.fill("wait_timed_out", &[("seconds", seconds.to_string())]),
                );
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

/// Where each part stands.
struct List(Arc<Parts>);

#[async_trait]
impl Tool for List {
    fn name(&self) -> &'static str {
        "parts"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(words, lp, "parts", json!({}), &[])
    }

    async fn call(&self, _: &Value) -> tool::Outcome {
        let parts = &self.0;
        let running: Vec<u64> = parts.running().keys().copied().collect();
        let lines: Vec<String> = parts
            .parent
            .parts()
            .into_iter()
            .map(|(n, state)| {
                let status = match state.finish.as_ref().map(|f| f.outcome) {
                    Some(Outcome::Finished) => "status_finished",
                    Some(Outcome::Blocked) => "status_blocked",
                    Some(Outcome::GaveUp) => "status_gave_up",
                    Some(Outcome::Dropped) => "status_dropped",
                    None if running.contains(&n) => "status_running",
                    None if state.waiting.is_some() => "status_asking",
                    None => "status_queued",
                };
                parts.fill(
                    "parts_line",
                    &[
                        ("part", n.to_string()),
                        ("status", parts.text(status)),
                        ("steps", state.steps.to_string()),
                        ("task", preview(&state.brief, 120)),
                        ("latest", preview(&state.latest(), 200)),
                    ],
                )
            })
            .collect();
        if lines.is_empty() {
            tool::Outcome::ok(parts.text("parts_none"))
        } else {
            tool::Outcome::ok(lines.join("\n"))
        }
    }
}

/// `done`, except that the work is not called finished while a part of it
/// has not ended: its results are not in. Ending it any other way ends
/// the parts with it, when the sitting settles.
struct Finishing {
    done: Arc<dyn Tool>,
    parts: Arc<Parts>,
}

#[async_trait]
impl Tool for Finishing {
    fn name(&self) -> &'static str {
        "done"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        self.done.spec(words, lp)
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        if args["outcome"].as_str().and_then(outcome_of) == Some(Outcome::Finished) {
            let open: Vec<String> = self
                .parts
                .parent
                .parts()
                .into_iter()
                .filter(|(_, state)| state.finish.is_none())
                .map(|(n, _)| n.to_string())
                .collect();
            if !open.is_empty() {
                return tool::Outcome::fail(
                    self.parts.fill("parts_open", &[("parts", open.join(", "))]),
                );
            }
        }
        self.done.call(args).await
    }
}

/// A part's `tell`: kept in its own record, and put in its parent's inbox.
struct Told {
    tell: tools::Tell,
    part: u64,
    parent: Arc<Undertaking>,
    prompts: Shared,
}

#[async_trait]
impl Tool for Told {
    fn name(&self) -> &'static str {
        "tell"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        self.tell.spec(words, lp)
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let out = self.tell.call(args).await;
        if out.failed {
            return out;
        }
        let files: Vec<&str> = args["files"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    .filter_map(|f| f.as_str().map(str::trim))
                    .filter(|f| !f.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let message = {
            let words = prompts::hold(&self.prompts);
            let files = if files.is_empty() {
                String::new()
            } else {
                words.fill("part_told_files", &[("files", files.join(", "))])
            };
            words.fill(
                "part_told",
                &[
                    ("part", self.part.to_string()),
                    (
                        "text",
                        args["text"].as_str().unwrap_or_default().trim().to_owned(),
                    ),
                    ("files", files),
                ],
            )
        };
        if let Err(e) = self.parent.post(MailKind::Told, &message) {
            tracing::warn!(
                "part {} could not tell {}: {e}",
                self.part,
                self.parent.id()
            );
        }
        out
    }
}

/// A part's `ask`: it stops until its parent answers with `steer`.
struct Asks {
    ask: tools::Ask,
    part: u64,
    parent: Arc<Undertaking>,
    prompts: Shared,
}

#[async_trait]
impl Tool for Asks {
    fn name(&self) -> &'static str {
        "ask"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        self.ask.spec(words, lp)
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let out = self.ask.call(args).await;
        if out.failed {
            return out;
        }
        let message = prompts::hold(&self.prompts).fill(
            "part_asked",
            &[
                ("part", self.part.to_string()),
                (
                    "text",
                    args["question"]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                ),
            ],
        );
        if let Err(e) = self.parent.post(MailKind::Asked, &message) {
            tracing::warn!("part {} could not ask {}: {e}", self.part, self.parent.id());
        }
        out
    }
}
