//! The loop.
//!
//! One step: the model reads the transcript and either says something or
//! asks for tools; the tools run; their output joins the transcript. That is
//! all a step is. What makes it hold up over hours is everything around it:
//!
//! - **State is outside the context.** The plan, the notes and the outcome
//!   live in the [`Undertaking`], and the transcript is saved after every
//!   step. The process can stop anywhere and be picked up.
//! - **Context is folded, not grown.** When the transcript nears its budget,
//!   old tool output is dropped first. If that is not enough the worker
//!   writes itself a handoff and starts again from the task, the plan and its
//!   notes, with nothing else.
//! - **Finishing is checked.** Saying "done" the first time gets the task
//!   read back (see the `done` tool).
//! - **Stalls are noticed.** Talking without acting, repeating the same call,
//!   and running low on steps each get said out loud to the model.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use ghost_brain::{Brain, Call, Item, Stop, Turn, Usage};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    tool::Toolbox,
    tools::{Handoff, Plan},
    undertaking::{Outcome, Undertaking},
};

const PROMPT: &str = include_str!("prompts/work.md");

const NO_CALL: &str = "You wrote a message but called no tool, so nothing happened. \
If the work is complete, check it and call `done`. Otherwise take the next action now.";
const CUT_OFF: &str = "Your last message was cut off by the output limit before it finished. \
Continue. If you were writing a large file, write it in several smaller pieces.";
const REPEATING: &str = "You have made the same call three times and got the same result. \
It will not change. Stop and think about what that result is telling you, then do something different.";
const HANDOFF: &str = "Your context is almost full and is about to be cleared. \
You will continue this work with only the task, the plan, your handoff and your latest notes in front of you. \
Right now, call `handoff` once with a single document that stands on its own: what is finished and checked, \
what is in progress and exactly where you stopped, the exact paths and commands that matter, what you learned \
that was not obvious, and the next few actions in order. It replaces any earlier handoff, so carry forward \
whatever from the earlier one still matters. Keep it under about 500 words. Also bring `plan` up to date. \
Do nothing else in this step.";

const INTERRUPTED: &str = "This did not finish: the work was interrupted while it ran. \
It may have partly happened. Look at the current state before doing it again.";

const AUDITOR: &str = "You are reviewing someone's claim that a task is finished. You see the task, \
their summary and evidence, and what their most recent commands actually printed. You cannot run \
anything. Decide whether the task as written is demonstrably complete.\n\n\
Be strict about what the task literally asks for: names, paths, formats, exact output, things \
that must be running. A requirement with no direct evidence in the printed output counts as \
unproven. A claim contradicted by the output counts as false. Do not ask for more than the task \
asks for, and do not fail work for style.\n\n\
Answer with PASS or FAIL as the first word. After FAIL, list each unproven or failed requirement \
in one line each, saying what would prove it. After PASS, nothing more is needed.";

/// How much of the recent transcript the reviewer is shown, in characters.
const AUDIT_SHOWN: usize = 14_000;

/// How many of the newest items are left whole when old output is dropped:
/// tried in turn, each tighter than the last.
const KEEP_RECENT: [usize; 3] = [12, 6, 3];
/// The share of the notes a new sitting is shown, in characters.
const NOTES_SHOWN: usize = 2500;

#[derive(Debug, Clone)]
pub struct Limits {
    /// Steps in one run.
    pub max_steps: u32,
    /// Wall-clock seconds in one run.
    pub max_seconds: u64,
    /// The transcript is folded when it is about to pass this many tokens.
    pub context_tokens: u64,
    /// Output tokens per step.
    pub max_output: u32,
    pub temperature: Option<f32>,
    /// Have a claim of being finished looked at by a reviewer who sees only
    /// the task, the evidence and the recent output.
    pub audit: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 200,
            max_seconds: 3600,
            context_tokens: 60_000,
            max_output: 8192,
            temperature: None,
            audit: true,
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
    /// The model could not be reached, or refused.
    Failed(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ended: Ended,
    /// The worker's own account of how it went, if it gave one.
    pub summary: String,
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
}

/// How full the context is, measured where possible and guessed otherwise.
struct Gauge {
    /// Tokens the model reported reading and writing at the last step, and
    /// the transcript's size in characters at that moment.
    measured: Option<(u64, usize)>,
    /// Characters outside the transcript: the system prompt and tool specs.
    fixed: usize,
}

fn chars(items: &[Item]) -> usize {
    items.iter().map(Item::weight).sum()
}

impl Gauge {
    fn tokens(&self, items: &[Item]) -> u64 {
        let now = chars(items);
        match self.measured {
            Some((tokens, then)) => tokens + (now.saturating_sub(then) / 3) as u64,
            None => ((now + self.fixed) / 3) as u64,
        }
    }
}

/// Shorten long strings inside a call's arguments (file contents, mostly).
fn thin(args: &mut Value) {
    if let Value::Object(map) = args {
        for value in map.values_mut() {
            if let Value::String(s) = value
                && s.len() > 400
            {
                let kept: String = s.chars().take(160).collect();
                *s = format!("{kept}\n[... the rest was dropped to save context ...]");
            }
        }
    }
}

/// Drop the bulk of everything but the most recent items. Returns how many
/// characters that freed.
fn shed(items: &mut [Item], keep: usize) -> usize {
    let before = chars(items);
    let old = items.len().saturating_sub(keep);
    for item in &mut items[..old] {
        match item {
            Item::Result { output, .. } if output.len() > 400 => {
                let kept: String = output.chars().take(200).collect();
                *output = format!(
                    "{kept}\n[... the rest of this output was dropped to save context; run it again if you need it ...]"
                );
            }
            // What the provider asked to have handed back (`carry`) stays:
            // some reject a transcript whose earlier thinking is missing.
            Item::Assistant { calls, .. } => calls.iter_mut().for_each(|c| thin(&mut c.args)),
            _ => {}
        }
    }
    before - chars(items)
}

/// Make a transcript picked up from disk whole again: every call the model
/// made has to have a result, even if the process died before one came.
fn mend(items: &mut Vec<Item>) {
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
        .map(|c| Item::Result {
            id: c.id.clone(),
            name: c.name.clone(),
            output: INTERRUPTED.to_owned(),
            failed: true,
        })
        .collect();
    // Results belong directly after the calls, before anything said since.
    let after = at + 1 + answered.len();
    items.splice(after..after, missing);
}

fn too_long(error: &ghost_brain::Error) -> bool {
    let ghost_brain::Error::Api {
        status: 400, body, ..
    } = error
    else {
        return false;
    };
    let body = body.to_lowercase();
    body.contains("context")
        || body.contains("too long")
        || body.contains("maximum") && body.contains("token")
}

fn preview(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

impl Worker {
    fn system(&self) -> String {
        let who = if self.who.trim().is_empty() {
            String::new()
        } else {
            format!("{}\n\n", self.who.trim())
        };
        let budget = format!(
            "You have at most {} steps and about {} minutes. That is enough to work carefully and not enough to wander.",
            self.limits.max_steps,
            (self.limits.max_seconds / 60).max(1)
        );
        PROMPT
            .replace("{who}", &who)
            .replace("{machine}", self.machine.trim())
            .replace("{budget}", &budget)
    }

    /// What a sitting starts from: the task alone the first time, and the
    /// task with what was left behind every time after.
    fn opening(&self) -> Vec<Item> {
        let state = self.undertaking.state();
        let task = format!("<task>\n{}\n</task>", state.brief.trim());
        if state.steps == 0 {
            return vec![Item::user(task)];
        }
        let handoff = state.handoff.as_deref().map_or_else(String::new, |h| {
            format!("<handoff>\n{}\n</handoff>\n\n", h.trim())
        });
        let touched = if state.touched.is_empty() {
            String::new()
        } else {
            format!(
                "<files_you_last_changed>\n{}\n</files_you_last_changed>\n\n",
                state.touched.join("\n")
            )
        };
        vec![Item::user(format!(
            "{task}\n\n\
             You have already been working on this, and your earlier context is gone. \
             This is what you left for yourself.\n\n\
             <plan>\n{}\n</plan>\n\n{handoff}{touched}<notes>\n{}\n</notes>\n\n\
             Clearing your context is routine and does not end the task. Do not start over. First look at the current state of the environment, which holds \
             whatever you already did. Then carry on from where you stopped.",
            state.plan_text(),
            state.notes_text(NOTES_SHOWN)
        ))]
    }

    /// The size at which a sitting is folded: the configured budget, or
    /// enough room above where the sitting started to get something done,
    /// whichever is larger. Without the second part a long task statement
    /// or handoff would have every step folding the one before it.
    fn ceiling(&self, base: u64) -> u64 {
        self.limits
            .context_tokens
            .max(base + self.limits.context_tokens / 2)
    }

    async fn run_calls(&self, calls: &[Call], items: &mut Vec<Item>) -> (Vec<Value>, String) {
        let mut logged = Vec::new();
        let mut signature = String::new();
        for call in calls {
            let started = Instant::now();
            let outcome = self.tools.run(call).await;
            let ms = started.elapsed().as_millis();
            tracing::info!(
                "  {}({}) → {}{}",
                call.name,
                preview(&call.args.to_string(), 100),
                preview(&outcome.output, 100),
                if outcome.failed { " [failed]" } else { "" }
            );
            logged.push(json!({
                "name": call.name,
                "args": preview(&call.args.to_string(), 400),
                "ms": ms,
                "failed": outcome.failed,
                "chars": outcome.output.len(),
            }));
            signature.push_str(&format!(
                "{}\u{1f}{}\u{1f}{}\u{1e}",
                call.name, call.args, outcome.output
            ));
            if !outcome.failed
                && matches!(call.name.as_str(), "write" | "edit")
                && let Some(path) = call.args["path"].as_str()
            {
                self.undertaking.update(|state| {
                    state.touched.retain(|p| p != path);
                    state.touched.push(path.to_owned());
                    let extra = state.touched.len().saturating_sub(8);
                    state.touched.drain(..extra);
                });
            }
            items.push(Item::Result {
                id: call.id.clone(),
                name: call.name.clone(),
                output: outcome.output,
                failed: outcome.failed,
            });
        }
        (logged, signature)
    }

    /// Ask a reviewer whether the work is demonstrably finished. Returns
    /// what it cost and, if the reviewer was not convinced, why.
    async fn audit(&self, items: &[Item]) -> (Usage, Option<String>) {
        let state = self.undertaking.state();
        let Some(finish) = &state.finish else {
            return (Usage::default(), None);
        };
        // The newest calls and what they printed, oldest first, within a
        // budget: what was actually seen, as opposed to what is claimed.
        let mut shown: Vec<String> = Vec::new();
        let mut used = 0;
        let mut calls: std::collections::HashMap<&str, &Call> = std::collections::HashMap::new();
        for item in items {
            if let Item::Assistant { calls: made, .. } = item {
                calls.extend(made.iter().map(|c| (c.id.as_str(), c)));
            }
        }
        for item in items.iter().rev() {
            let Item::Result {
                id, name, output, ..
            } = item
            else {
                continue;
            };
            if matches!(name.as_str(), "plan" | "note" | "done") {
                continue;
            }
            let asked = calls
                .get(id.as_str())
                .map_or_else(String::new, |c| preview(&c.args.to_string(), 300));
            let block = format!("$ {name} {asked}\n{}", crate::tools::clip(output, 500, 900));
            if used + block.len() > AUDIT_SHOWN {
                break;
            }
            used += block.len();
            shown.push(block);
        }
        shown.reverse();
        let turn = Turn {
            system: AUDITOR.to_owned(),
            items: vec![Item::user(format!(
                "<task>\n{}\n</task>\n\n<summary>\n{}\n</summary>\n\n<evidence>\n{}\n</evidence>\n\n\
                 <recent_output>\n{}\n</recent_output>",
                state.brief.trim(),
                finish.summary,
                finish.evidence,
                shown.join("\n\n")
            ))],
            tools: Vec::new(),
            max_tokens: 1200,
            temperature: self.limits.temperature,
        };
        match self.brain.step(&turn).await {
            Ok(step) => {
                let verdict = step.text.trim();
                let passed = verdict
                    .trim_start_matches(|c: char| !c.is_alphabetic())
                    .to_uppercase()
                    .starts_with("PASS");
                tracing::info!("reviewer: {}", preview(verdict, 200));
                (
                    step.usage,
                    (!passed && !verdict.is_empty()).then(|| verdict.to_owned()),
                )
            }
            Err(e) => {
                // No reviewer is not a reason to hold finished work back.
                tracing::warn!("the review could not be done: {e}");
                (Usage::default(), None)
            }
        }
    }

    /// Make room. Old output goes first, in tightening rounds; if that is
    /// not enough, the worker writes a handoff and the transcript starts
    /// again from the record. Returns what the handoff cost, and the size
    /// of the new sitting's opening if one began.
    async fn fold(
        &self,
        items: &mut Vec<Item>,
        system: &str,
        gauge: &mut Gauge,
        ceiling: u64,
        forced: bool,
    ) -> (Usage, Option<u64>) {
        let mut spent = Usage::default();
        let mut freed = 0;
        gauge.measured = None;
        for keep in KEEP_RECENT {
            freed += shed(items, keep);
            if gauge.tokens(items) * 10 <= ceiling * 6 {
                break;
            }
        }
        let after = gauge.tokens(items);
        tracing::info!(
            "folded: dropped {freed} characters of old output, about {after} tokens remain"
        );
        if !forced && after * 10 <= ceiling * 6 {
            return (spent, None);
        }

        // A handoff, unless the context is already too full to ask for one.
        if !forced {
            items.push(Item::user(HANDOFF));
            let record = Toolbox::new()
                .with(Handoff::new(self.undertaking.clone()))
                .with(Plan::new(self.undertaking.clone()));
            let turn = Turn {
                system: system.to_owned(),
                items: items.clone(),
                tools: record.specs(),
                max_tokens: self.limits.max_output,
                temperature: self.limits.temperature,
            };
            match self.brain.step(&turn).await {
                Ok(step) => {
                    spent = step.usage;
                    for call in &step.calls {
                        let _ = record.run(call).await;
                    }
                }
                Err(e) => tracing::warn!("no handoff could be written: {e}"),
            }
        }
        self.undertaking.update(|state| state.sittings += 1);
        *items = self.opening();
        gauge.measured = None;
        let base = gauge.tokens(items);
        tracing::info!(
            "a new sitting begins from the plan and the handoff, at about {base} tokens"
        );
        (spent, Some(base))
    }

    fn report(&self, ended: Ended) -> Report {
        let state = self.undertaking.state();
        Report {
            ended,
            summary: state.finish.map(|f| f.summary).unwrap_or_default(),
            steps: state.steps,
            sittings: state.sittings + 1,
            usage: state.usage,
            seconds: state.seconds,
        }
    }

    /// Work until it is finished, or until something stops it.
    pub async fn run(&self) -> Report {
        if let Some(finish) = self.undertaking.state().finish {
            return self.report(match finish.outcome {
                Outcome::Finished => Ended::Finished,
                Outcome::Blocked => Ended::Blocked,
                Outcome::GaveUp => Ended::GaveUp,
            });
        }
        let started = Instant::now();
        let system = self.system();
        let specs = self.tools.specs();
        let mut gauge = Gauge {
            measured: None,
            fixed: system.len()
                + specs
                    .iter()
                    .map(|s| s.description.len() + s.parameters.to_string().len())
                    .sum::<usize>(),
        };
        let mut items = self
            .undertaking
            .transcript()
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| self.opening());
        mend(&mut items);

        // Where this sitting started, in tokens. A transcript picked up from
        // disk counts as starting from the task alone.
        let mut base = gauge.tokens(&self.opening());
        let mut idle = 0;
        let mut last_signature = String::new();
        let mut repeats = 0;
        let mut overflowed = false;
        let warn_at = self.limits.max_steps * 4 / 5;

        for n in 1..=self.limits.max_steps {
            if self.pause.load(Ordering::Relaxed) {
                return self.report(Ended::Paused);
            }
            if started.elapsed().as_secs() >= self.limits.max_seconds {
                return self.report(Ended::OutOfTime);
            }

            let mut extra = Usage::default();
            if items.len() > 1 && gauge.tokens(&items) > self.ceiling(base) {
                let (spent, began) = self
                    .fold(&mut items, &system, &mut gauge, self.ceiling(base), false)
                    .await;
                extra = spent;
                base = began.unwrap_or(base);
            }

            let asked = Instant::now();
            let turn = Turn {
                system: system.clone(),
                items: items.clone(),
                tools: specs.clone(),
                max_tokens: self.limits.max_output,
                temperature: self.limits.temperature,
            };
            let step = match self.brain.step(&turn).await {
                Ok(step) => step,
                Err(e) if too_long(&e) && !overflowed => {
                    // The guess at the context's size was too low. Make room
                    // without asking the model for anything, and go again.
                    overflowed = true;
                    tracing::warn!("the model said the context is too long; folding hard");
                    let (_, began) = self
                        .fold(&mut items, &system, &mut gauge, self.ceiling(base), true)
                        .await;
                    base = began.unwrap_or(base);
                    continue;
                }
                Err(e) => {
                    self.undertaking.save_transcript(&items);
                    return self.report(Ended::Failed(e.to_string()));
                }
            };
            overflowed = false;
            let model_ms = asked.elapsed().as_millis();
            let mut usage = step.usage;
            usage.add(extra);
            let (said, calls, why) = (step.text.clone(), step.calls.clone(), step.stop);
            tracing::info!(
                "step {n}: read {} ({} cached), wrote {}{}",
                step.usage.input,
                step.usage.cached,
                step.usage.output,
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {}", preview(&said, 160))
                }
            );
            items.push(step.into_item());
            gauge.measured = Some((usage.input + usage.output, chars(&items)));

            let mut logged = Vec::new();
            if calls.is_empty() {
                idle += 1;
                if idle >= 3 {
                    self.undertaking.save_transcript(&items);
                    return self.report(Ended::Stalled);
                }
                items.push(Item::user(if why == Stop::Length {
                    CUT_OFF
                } else {
                    NO_CALL
                }));
            } else {
                idle = 0;
                let (ran, signature) = self.run_calls(&calls, &mut items).await;
                logged = ran;
                if signature == last_signature {
                    repeats += 1;
                    if repeats == 2 {
                        items.push(Item::user(REPEATING));
                        repeats = 0;
                    }
                } else {
                    repeats = 0;
                    last_signature = signature;
                }
            }

            let mut finish = self.undertaking.update(|state| {
                state.steps += 1;
                state.usage.add(usage);
                state.seconds += asked.elapsed().as_secs_f64();
                state.finish.clone()
            });
            // A claim of being finished gets one outside look.
            if self.limits.audit
                && finish
                    .as_ref()
                    .is_some_and(|f| f.outcome == Outcome::Finished)
                && !self.undertaking.state().audited
            {
                let (spent, objection) = self.audit(&items).await;
                self.undertaking.update(|state| {
                    state.audited = true;
                    state.usage.add(spent);
                    if objection.is_some() {
                        state.finish = None;
                    }
                });
                if let Some(objection) = objection {
                    finish = None;
                    items.push(Item::user(format!(
                        "Your claim that this is finished was read by a reviewer who saw the task, your evidence \
                         and what your recent commands printed. They were not convinced:\n\n{objection}\n\n\
                         Deal with each point: fix what is wrong, and show what was right by running the check \
                         that proves it. Then call `done` again."
                    )));
                }
            }
            self.undertaking.log(&json!({
                "at": chrono::Utc::now().to_rfc3339(),
                "step": n,
                "read": usage.input,
                "cached": usage.cached,
                "wrote": usage.output,
                "model_ms": model_ms,
                "said": preview(&said, 400),
                "calls": logged,
            }));
            self.undertaking.save_transcript(&items);

            if let Some(finish) = finish {
                return self.report(match finish.outcome {
                    Outcome::Finished => Ended::Finished,
                    Outcome::Blocked => Ended::Blocked,
                    Outcome::GaveUp => Ended::GaveUp,
                });
            }
            if n == warn_at && warn_at > 0 {
                items.push(Item::user(format!(
                    "You have used {n} of your {} steps. Spend what is left on getting the work into a finished, checked state, and call `done`.",
                    self.limits.max_steps
                )));
            }
        }
        self.report(Ended::OutOfSteps)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use ghost_brain::{Completion, Reply, Result, Step};

    use super::*;
    use crate::{machine::Local, tools};

    /// A brain that plays back a script, and remembers what it was shown.
    struct Scripted {
        steps: Mutex<Vec<Step>>,
        seen: Mutex<Vec<Turn>>,
    }

    fn says(text: &str) -> Step {
        Step {
            text: text.into(),
            calls: vec![],
            stop: Stop::End,
            usage: Usage {
                input: 100,
                cached: 0,
                output: 10,
            },
            model: "scripted".into(),
            carry: None,
        }
    }

    fn calls(name: &str, args: Value) -> Step {
        Step {
            calls: vec![Call {
                id: format!("c-{name}"),
                name: name.into(),
                args,
            }],
            stop: Stop::Calls,
            ..says("")
        }
    }

    #[async_trait]
    impl Brain for Scripted {
        fn label(&self) -> String {
            "scripted".into()
        }
        async fn complete(&self, _: &Completion) -> Result<Reply> {
            unreachable!()
        }
        async fn step(&self, turn: &Turn) -> Result<Step> {
            self.seen.lock().unwrap().push(turn.clone());
            let mut steps = self.steps.lock().unwrap();
            let mut step = if steps.is_empty() {
                says("")
            } else {
                steps.remove(0)
            };
            // Like a real model, it reads as much as it is shown.
            step.usage.input = (chars(&turn.items) / 3).max(100) as u64;
            Ok(step)
        }
    }

    fn worker(dir: &std::path::Path, script: Vec<Step>, limits: Limits) -> (Worker, Arc<Scripted>) {
        let undertaking =
            Arc::new(Undertaking::open(dir.join("state"), Some("make hi.txt say hi")).unwrap());
        let brain = Arc::new(Scripted {
            steps: Mutex::new(script),
            seen: Mutex::new(vec![]),
        });
        let machine = Arc::new(Local::new(dir));
        let worker = Worker {
            brain: brain.clone(),
            tools: tools::standard(machine, &undertaking),
            undertaking,
            limits,
            who: String::new(),
            machine: "a test directory".into(),
            pause: Arc::new(AtomicBool::new(false)),
        };
        (worker, brain)
    }

    #[tokio::test]
    async fn work_runs_to_a_checked_finish() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote it"}),
            ),
            calls("shell", json!({"command": "cat hi.txt"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote it", "evidence": "ran `cat hi.txt` just now, it printed hi"}),
            ),
        ];
        let (worker, brain) = worker(
            dir.path(),
            script,
            Limits {
                audit: false,
                ..Limits::default()
            },
        );
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Finished);
        assert_eq!(report.steps, 4);
        assert!(report.usage.input >= 400);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hi.txt")).unwrap(),
            "hi\n"
        );

        // The first `done` was answered with the task, and the model saw that.
        let seen = brain.seen.lock().unwrap();
        let Item::Result { output, .. } = &seen[2].items[4] else {
            panic!()
        };
        assert!(output.contains("make hi.txt say hi"));
        // The log has one line per step.
        let log = std::fs::read_to_string(dir.path().join("state/log.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 4);
    }

    #[tokio::test]
    async fn a_reviewer_can_send_finished_work_back_once() {
        let dir = tempfile::tempdir().unwrap();
        let done = |evidence: &str| {
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote it", "evidence": evidence}),
            )
        };
        let script = vec![
            calls("write", json!({"path": "hi.txt", "content": "hello\n"})),
            done("I wrote the file with the write tool"),
            done("I wrote the file with the write tool"),
            says("FAIL\n- hi.txt must say hi: nothing printed shows its content"),
            calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
            done("cat hi.txt printed hi, checked just now"),
        ];
        let (worker, brain) = worker(
            dir.path(),
            script,
            Limits {
                audit: true,
                ..Limits::default()
            },
        );
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Finished);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hi.txt")).unwrap(),
            "hi\n"
        );

        let seen = brain.seen.lock().unwrap();
        // The reviewer saw the task and the output, and had no tools.
        let review = seen
            .iter()
            .find(|t| t.tools.is_empty())
            .expect("a review happened");
        assert!(matches!(&review.items[0], Item::User { text }
            if text.contains("make hi.txt say hi") && text.contains("$ write")));
        // Its objection reached the worker, and it was only asked once.
        assert!(
            seen.iter()
                .any(|t| matches!(t.items.last(), Some(Item::User { text })
            if text.contains("nothing printed shows its content")))
        );
        assert_eq!(seen.iter().filter(|t| t.tools.is_empty()).count(), 1);
    }

    #[test]
    fn a_transcript_cut_off_mid_step_is_made_whole() {
        let call = |id: &str| Call {
            id: id.into(),
            name: "shell".into(),
            args: json!({"command": "make"}),
        };
        let mut items = vec![
            Item::user("task"),
            Item::Assistant {
                text: String::new(),
                calls: vec![call("a"), call("b")],
                carry: None,
            },
            Item::Result {
                id: "a".into(),
                name: "shell".into(),
                output: "ok".into(),
                failed: false,
            },
        ];
        mend(&mut items);
        assert_eq!(items.len(), 4);
        assert!(
            matches!(&items[3], Item::Result { id, failed: true, output, .. }
            if id == "b" && output.contains("interrupted"))
        );
        // Nothing to do when it is already whole.
        let before = items.clone();
        mend(&mut items);
        assert_eq!(items, before);
    }

    #[tokio::test]
    async fn talking_without_acting_is_nudged_then_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            says("I would write the file."),
            says("Then check it."),
            says("That is the plan."),
        ];
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        assert_eq!(worker.run().await.ended, Ended::Stalled);
        let seen = brain.seen.lock().unwrap();
        assert!(
            matches!(&seen[1].items[2], Item::User { text } if text.contains("called no tool"))
        );
    }

    #[tokio::test]
    async fn a_full_context_is_folded_into_a_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(9000);
        let mut script: Vec<Step> = (0..2)
            .map(|i| calls("shell", json!({"command": format!("echo {big}{i}")})))
            .collect();
        // Asked for a handoff, it writes one; then it finishes.
        script.push(calls("handoff", json!({"text": "the file goes in hi.txt"})));
        script.push(calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        ));
        let limits = Limits {
            context_tokens: 8000,
            ..Limits::default()
        };
        let (worker, brain) = worker(dir.path(), script, limits);
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert_eq!(report.sittings, 2);

        let seen = brain.seen.lock().unwrap();
        let at = seen
            .iter()
            .position(|t| t.tools.len() == 2)
            .expect("a handoff was asked for");
        assert!(
            matches!(seen[at].items.last(), Some(Item::User { text }) if text.contains("about to be cleared"))
        );
        // The sitting after it starts from the record alone.
        let fresh = &seen[at + 1];
        assert_eq!(fresh.items.len(), 1);
        assert!(matches!(&fresh.items[0], Item::User { text }
            if text.contains("the file goes in hi.txt") && text.contains("Do not start over")));
    }

    #[tokio::test]
    async fn a_paused_run_picks_up_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![calls("shell", json!({"command": "echo one"}))];
        let (first, _) = worker(
            dir.path(),
            script,
            Limits {
                max_steps: 1,
                ..Limits::default()
            },
        );
        assert_eq!(first.run().await.ended, Ended::OutOfSteps);

        let script = vec![calls(
            "done",
            json!({"outcome": "gave_up", "summary": "no"}),
        )];
        let (second, brain) = worker(dir.path(), script, Limits::default());
        let report = second.run().await;
        assert_eq!(report.ended, Ended::GaveUp);
        assert_eq!(report.steps, 2, "totals run across both");
        // It was shown the transcript it left, not a blank page.
        assert_eq!(brain.seen.lock().unwrap()[0].items.len(), 3);
    }
}
