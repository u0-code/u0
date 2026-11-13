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
    tools::{Handoff, clip},
    undertaking::{Outcome, State, Undertaking},
};

const PROMPT: &str = include_str!("prompts/work.md");

const NO_CALL: &str = "You wrote a message but called no tool, so nothing happened. \
If the work is complete, check it and call `done`. Otherwise take the next action now.";
const CUT_OFF: &str = "Your last message was cut off by the output limit before it finished. \
Continue. If you were writing a large file, write it in several smaller pieces.";
const REPEATING: &str = "You have made the same call three times and got the same result. \
It will not change. Stop and think about what that result is telling you, then do something different.";
const HANDOFF: &str = "Your context is almost full, and all of it but the most recent part is about to be cleared. \
You will continue this work with the task, the plan, your handoff, your latest notes and only the last stretch \
of this conversation in front of you. \
Right now, call `handoff` once with a single document that stands on its own: what is finished and checked, \
what is in progress and exactly where you stopped, the exact paths and commands that matter, what you learned \
that was not obvious, and the next few actions in order. It replaces any earlier handoff, so carry forward \
whatever from the earlier one still matters. Keep it under about 500 words. Also bring `plan` up to date. \
Do nothing else in this step.";

/// Calls run one after another, so of the calls left without a result when
/// the work was interrupted only the first can have begun.
const INTERRUPTED: &str = "This did not finish: the work was interrupted while it ran, and what it did is unknown. \
It may have partly happened. If it only reads, or doing it twice does no harm, run it again; \
otherwise look at the current state first.";
const NOT_STARTED: &str = "This was not run: the work was interrupted before it began. \
Run it again if it is still needed.";

const AUDITOR: &str = "You are reviewing someone's claim that a task is finished. You see the task, \
their summary and evidence, every command they ran in order (without its output), and what their \
most recent commands actually printed. You cannot run anything. Decide whether the task as written \
is demonstrably complete.\n\n\
Be strict about the result the task literally asks for: names, paths, formats, exact output, things \
that must be running. Such a requirement with no direct evidence in the printed output counts as \
unproven. A claim contradicted by the output counts as false. A requirement about how the work was \
to be done (in what order, in how many commands, with which tool) cannot be shown after the fact: \
judge it from the list of commands, and accept it unless the list contradicts it. Do not ask for \
more than the task asks for, and do not fail work for style.\n\n\
Answer with PASS or FAIL as the first word. After FAIL, list each unproven or failed requirement \
in one line each, saying what would prove it. After PASS, nothing more is needed.";

/// How much of the recent transcript the reviewer is shown, in characters.
const AUDIT_SHOWN: usize = 14_000;
/// How much of the list of commands the reviewer is shown, in characters.
const AUDIT_LEDGER: usize = 10_000;

/// Where a transcript is folded when nothing says and the model's window is
/// not known: small enough for any model worth working with.
const UNKNOWN_WINDOW: u64 = 60_000;
/// Room kept free in a known window beyond what one step may write.
const HEADROOM: u64 = 65_536;
/// The model once refused a context it was sent. Stay this far under the
/// size that was refused, as tenths.
const UNDER_REFUSAL: u64 = 7;
/// Folding is done when the transcript is back under this share of the
/// room between where its sitting started and its ceiling, as tenths: far
/// enough down that the next fold is a while away.
const FOLD_TO: u64 = 6;

/// A tool result longer than this many characters is trimmed to its
/// beginning and end when room is needed, wherever in the transcript it is.
const PRUNE_OVER: usize = 8192;
const PRUNE_HEAD: usize = 4096;
const PRUNE_TAIL: usize = 1024;
/// How many of the newest items are left whole when old output is dropped:
/// tried in turn, each tighter than the last.
const KEEP_RECENT: [usize; 3] = [12, 6, 3];
/// The share of the ceiling that is carried across a cleared context word
/// for word, as hundredths: the most recent stretch of the transcript.
const TAIL_SHARE: u64 = 16;
/// The share of the notes a new sitting is shown, in characters.
const NOTES_SHOWN: usize = 2500;

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
            context_tokens: None,
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

    /// The transcript was made smaller in place. Keep what the last
    /// measurement said about how many characters make a token here, and
    /// apply it to the new size.
    fn reweigh(&mut self, items: &[Item]) {
        let now = chars(items);
        if let Some((tokens, then)) = self.measured {
            let was = (then + self.fixed).max(1) as u128;
            let scaled = u128::from(tokens) * (now + self.fixed) as u128 / was;
            self.measured = Some((u64::try_from(scaled).unwrap_or(u64::MAX), now));
        }
    }
}

/// Trim every long tool result but the newest few to its beginning and
/// end. Returns how many characters that freed.
fn prune(items: &mut [Item], spare: usize) -> usize {
    let before = chars(items);
    let old = items.len().saturating_sub(spare);
    for item in &mut items[..old] {
        if let Item::Result { output, .. } = item
            && output.len() > PRUNE_OVER
        {
            *output = clip(output, PRUNE_HEAD, PRUNE_TAIL);
        }
    }
    before - chars(items)
}

/// The newest stretch of a transcript that fits in `budget` characters and
/// can stand on its own: it never opens on a result whose call is gone, and
/// never includes the first item, which is the opening it will follow.
fn tail_of(items: &[Item], budget: usize) -> Vec<Item> {
    let mut used = 0;
    let mut from = items.len();
    for (i, item) in items.iter().enumerate().skip(1).rev() {
        used += item.weight();
        if used > budget {
            break;
        }
        from = i;
    }
    while items
        .get(from)
        .is_some_and(|item| matches!(item, Item::Result { .. }))
    {
        from += 1;
    }
    items.get(from..).unwrap_or_default().to_vec()
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
        .enumerate()
        .map(|(nth, c)| Item::Result {
            id: c.id.clone(),
            name: c.name.clone(),
            output: if nth == 0 { INTERRUPTED } else { NOT_STARTED }.to_owned(),
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

/// The working instructions, with who is working and where filled in.
pub fn system_prompt(who: &str, machine: &str, limits: &Limits) -> String {
    let who = if who.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n\n", who.trim())
    };
    let budget = format!(
        "You have at most {} steps and about {} minutes. That is enough to work carefully and not enough to wander.",
        limits.max_steps,
        (limits.max_seconds / 60).max(1)
    );
    PROMPT
        .replace("{who}", &who)
        .replace("{machine}", machine.trim())
        .replace("{budget}", &budget)
}

/// What a sitting opens with: the task alone the first time, and the task
/// with what was left behind every time after.
pub fn opening_text(state: &State) -> String {
    opening_with(state, false)
}

/// The opening, for a context that was cleared. With `tail`, the last
/// stretch of the old transcript follows it word for word, and the worker
/// is told to go on from there instead of finding its feet again.
fn opening_with(state: &State, tail: bool) -> String {
    let task = format!("<task>\n{}\n</task>", state.task_text());
    if state.steps == 0 {
        return task;
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
    let (before, after) = if tail {
        (
            "You have already been working on this. Your context filled up, so all of it but the most \
             recent part has been cleared. This is what you left for yourself; after this message, the \
             last stretch of your work follows exactly as it happened.",
            "Clearing your context is routine and does not end the task. Do not start over and do not \
             redo what is finished. Treat what is above as settled, and carry on directly from where \
             the messages below leave off.",
        )
    } else {
        (
            "You have already been working on this, and your earlier context is gone. \
             This is what you left for yourself.",
            "Clearing your context is routine and does not end the task. Do not start over. \
             First look at the current state of the environment, which holds \
             whatever you already did. Then carry on from where you stopped.",
        )
    };
    format!(
        "{task}\n\n{before}\n\n\
         <plan>\n{}\n</plan>\n\n{handoff}{touched}<notes>\n{}\n</notes>\n\n{after}",
        state.plan_text(),
        state.notes_text(NOTES_SHOWN)
    )
}

impl Worker {
    fn system(&self) -> String {
        system_prompt(&self.who, &self.machine, &self.limits)
    }

    /// What a sitting starts from: the task alone the first time, and the
    /// task with what was left behind every time after.
    fn opening(&self) -> Vec<Item> {
        vec![Item::user(opening_text(&self.undertaking.state()))]
    }

    /// How many tokens the transcript may reach before it is folded: what
    /// was asked for, or most of the model's window with room left to
    /// answer in, and in either case well under any size the model has
    /// already refused.
    fn budget(&self) -> u64 {
        let wanted = self.limits.context_tokens.unwrap_or_else(|| {
            self.brain.window().map_or(UNKNOWN_WINDOW, |window| {
                let reserved = u64::from(self.limits.max_output) + HEADROOM.min(window / 8);
                (window * 4 / 5).min(window.saturating_sub(reserved))
            })
        });
        match self.undertaking.state().refused_at {
            Some(refused) => wanted.min(refused * UNDER_REFUSAL / 10),
            None => wanted,
        }
        .max(4000)
    }

    /// The size at which a sitting is folded: the budget, or enough room
    /// above where the sitting started to get something done, whichever is
    /// larger. Without the second part a long task statement or handoff
    /// would have every step folding the one before it.
    fn ceiling(&self, base: u64) -> u64 {
        let budget = self.budget();
        budget.max(base + budget / 2)
    }

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

    async fn run_calls(
        &self,
        tools: &Toolbox,
        calls: &[Call],
        items: &mut Vec<Item>,
    ) -> (Vec<Value>, String) {
        let mut logged = Vec::new();
        let mut signature = String::new();
        for call in calls {
            let started = Instant::now();
            let outcome = tools.run(call).await;
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
        // Everything that was run, from the history: the transcript may
        // have been cleared since, and what was done early is part of what
        // is being claimed.
        let mut ledger: Vec<String> = Vec::new();
        let mut room = AUDIT_LEDGER;
        let ran: Vec<Call> = self
            .undertaking
            .history()
            .into_iter()
            .filter_map(|item| match item {
                Item::Assistant { calls, .. } => Some(calls),
                _ => None,
            })
            .flatten()
            .filter(|c| !matches!(c.name.as_str(), "plan" | "note" | "done" | "handoff"))
            .collect();
        for call in ran.iter().rev() {
            let line = format!("{} {}", call.name, preview(&call.args.to_string(), 160));
            if line.len() > room {
                ledger.push(format!(
                    "({} earlier commands are not listed)",
                    ran.len() - ledger.len()
                ));
                break;
            }
            room -= line.len();
            ledger.push(line);
        }
        ledger.reverse();
        let turn = Turn {
            system: AUDITOR.to_owned(),
            items: vec![Item::user(format!(
                "<task>\n{}\n</task>\n\n<summary>\n{}\n</summary>\n\n<evidence>\n{}\n</evidence>\n\n\
                 <commands_run>\n{}\n</commands_run>\n\n<recent_output>\n{}\n</recent_output>",
                state.task_text(),
                finish.summary,
                finish.evidence,
                ledger.join("\n"),
                shown.join("\n\n")
            ))],
            tools: Vec::new(),
            // Room for a model that thinks before it answers: thinking is
            // counted against this too, and a verdict cut off by it is none.
            max_tokens: self.limits.max_output.max(6000),
            temperature: self.limits.temperature,
        };
        match self.brain.step(&turn).await {
            Ok(step) if step.text.trim().is_empty() => {
                tracing::warn!("the reviewer said nothing; the claim stands unreviewed");
                (step.usage, None)
            }
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

    /// Make room. First on a copy: long output is trimmed, then old output
    /// is dropped in tightening rounds. If that brings the transcript far
    /// enough down, the copy becomes the transcript. If it does not, the
    /// transcript is left as it was for one more request, in which the
    /// worker writes a handoff (`ask`), and then starts again from the
    /// record and the last stretch of what it was doing, untrimmed.
    ///
    /// Returns what the handoff cost, and the size the new sitting starts
    /// at if one began.
    #[allow(clippy::too_many_arguments)]
    async fn fold(
        &self,
        items: &mut Vec<Item>,
        recorded: &mut usize,
        system: &str,
        tools: &Toolbox,
        gauge: &mut Gauge,
        base: u64,
        ceiling: u64,
        ask: bool,
    ) -> (Usage, Option<u64>) {
        // What the sitting started at cannot be folded away, so the aim is
        // set in the room above it.
        let base = base.min(ceiling);
        let target = base + (ceiling - base) * FOLD_TO / 10;
        let mut trial = items.clone();
        let mut trial_gauge = Gauge {
            measured: gauge.measured,
            fixed: gauge.fixed,
        };
        let mut freed = prune(&mut trial, KEEP_RECENT[2]);
        trial_gauge.reweigh(&trial);
        for keep in KEEP_RECENT {
            if trial_gauge.tokens(&trial) <= target {
                break;
            }
            freed += shed(&mut trial, keep);
            trial_gauge.reweigh(&trial);
        }
        let after = trial_gauge.tokens(&trial);
        if after <= target {
            tracing::info!(
                "folded: dropped {freed} characters of old output, about {after} tokens remain"
            );
            *items = trial;
            *gauge = trial_gauge;
            return (Usage::default(), None);
        }

        // Not enough. The untouched transcript goes to the model once more,
        // with the same tools as every other step so that the request reads
        // from the provider's cache up to its last message.
        self.save(items, recorded);
        let mut spent = Usage::default();
        if ask {
            let asking = Item::user(HANDOFF);
            let mut shown = items.clone();
            shown.push(asking.clone());
            let turn = Turn {
                system: system.to_owned(),
                items: shown,
                tools: tools.specs(),
                max_tokens: self.limits.max_output,
                temperature: self.limits.temperature,
            };
            match self.brain.step(&turn).await {
                Ok(step) => {
                    spent = step.usage;
                    for call in &step.calls {
                        if matches!(call.name.as_str(), "handoff" | "plan") {
                            let _ = tools.run(call).await;
                        }
                    }
                    self.undertaking.remember(&[asking, step.into_item()]);
                }
                Err(e) => tracing::warn!("no handoff could be written: {e}"),
            }
        }
        let share = if ask { TAIL_SHARE } else { TAIL_SHARE / 2 };
        let room = usize::try_from(ceiling * share / 100 * 3).unwrap_or(usize::MAX);
        let tail = tail_of(items, room);
        let state = self.undertaking.update(|state| {
            state.sittings += 1;
            state.clone()
        });
        let opening = Item::user(opening_with(&state, !tail.is_empty()));
        self.undertaking.remember(std::slice::from_ref(&opening));
        tracing::info!(
            "a new sitting begins from the plan, the handoff and the last {} items",
            tail.len()
        );
        *items = vec![opening];
        items.extend(tail);
        *recorded = items.len();
        gauge.measured = None;
        (spent, Some(gauge.tokens(items)))
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
        // The handoff tool is there at every step, not only when it is
        // wanted: a request whose tools differ from the last one's reads
        // nothing from the provider's cache.
        let tools = self
            .tools
            .clone()
            .with(Handoff::new(self.undertaking.clone()));
        let specs = tools.specs();
        let mut gauge = Gauge {
            measured: None,
            fixed: system.len()
                + specs
                    .iter()
                    .map(|s| s.description.len() + s.parameters.to_string().len())
                    .sum::<usize>(),
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
            None => (self.opening(), 0),
        };
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
            // Whatever was added to the task from outside since the last
            // step is put in front of the worker before it goes on.
            for added in self.undertaking.take_added() {
                tracing::info!("added to the task: {}", preview(&added, 160));
                items.push(Item::user(format!(
                    "Something has been added to the task while you were working:\n\n{added}\n\n\
                     It extends or adjusts what was asked; it does not replace it. \
                     Take it into account from here, and update the plan if it changes what is left to do."
                )));
            }

            let mut extra = Usage::default();
            if items.len() > 1 && gauge.tokens(&items) > self.ceiling(base) {
                let ceiling = self.ceiling(base);
                let (spent, began) = self
                    .fold(
                        &mut items,
                        &mut recorded,
                        &system,
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
                            &system,
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
                    return self.report(Ended::Stalled);
                }
                items.push(Item::user(if why == Stop::Length {
                    CUT_OFF
                } else {
                    NO_CALL
                }));
            } else {
                idle = 0;
                let (ran, signature) = self.run_calls(&tools, &calls, &mut items).await;
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
            // A claim of being finished gets one outside look. The reviewer
            // reads the history, so this step has to be in it first.
            if finish.is_some() {
                self.save(&items, &mut recorded);
            }
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
            self.save(&items, &mut recorded);

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
    use std::sync::{Mutex, atomic::AtomicU64};

    use async_trait::async_trait;
    use ghost_brain::{Completion, Reply, Result, Step};

    use super::*;
    use crate::{machine::Local, tools};

    /// A brain that plays back a script, and remembers what it was shown.
    /// Asked for a handoff, it writes one without spending the script.
    struct Scripted {
        steps: Mutex<Vec<Step>>,
        seen: Mutex<Vec<Turn>>,
        /// What it says its context window is; 0 for not known.
        window: AtomicU64,
        /// It refuses a transcript of more than this many tokens; 0 for never.
        refuses_over: AtomicU64,
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
        fn window(&self) -> Option<u64> {
            Some(self.window.load(Ordering::Relaxed)).filter(|w| *w > 0)
        }
        async fn step(&self, turn: &Turn) -> Result<Step> {
            // Like a real model, it reads as much as it is shown.
            let size = (chars(&turn.items) / 3).max(100) as u64;
            let limit = self.refuses_over.load(Ordering::Relaxed);
            if limit > 0 && size > limit {
                return Err(ghost_brain::Error::Api {
                    provider: "scripted".into(),
                    status: 400,
                    body: "This model's maximum context length is less than what was sent".into(),
                });
            }
            self.seen.lock().unwrap().push(turn.clone());
            let asked_to_hand_off = matches!(turn.items.last(),
                Some(Item::User { text }) if text.contains("about to be cleared"));
            let mut step = if asked_to_hand_off {
                calls("handoff", json!({"text": "the file goes in hi.txt"}))
            } else {
                let mut steps = self.steps.lock().unwrap();
                if steps.is_empty() {
                    says("")
                } else {
                    steps.remove(0)
                }
            };
            step.usage.input = size;
            Ok(step)
        }
    }

    fn worker(dir: &std::path::Path, script: Vec<Step>, limits: Limits) -> (Worker, Arc<Scripted>) {
        let undertaking =
            Arc::new(Undertaking::open(dir.join("state"), Some("make hi.txt say hi")).unwrap());
        let brain = Arc::new(Scripted {
            steps: Mutex::new(script),
            seen: Mutex::new(vec![]),
            window: AtomicU64::new(0),
            refuses_over: AtomicU64::new(0),
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
        // And everything that was run, from the history and not from what
        // is left of the transcript.
        assert!(matches!(&review.items[0], Item::User { text }
            if text.contains("<commands_run>\nwrite {\"content\":\"hello\\n\"")));
        // Its objection reached the worker, and it was only asked once.
        assert!(
            seen.iter()
                .any(|t| matches!(t.items.last(), Some(Item::User { text })
            if text.contains("nothing printed shows its content")))
        );
        assert_eq!(seen.iter().filter(|t| t.tools.is_empty()).count(), 1);
    }

    #[tokio::test]
    async fn something_added_mid_work_reaches_the_worker_before_its_next_step() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("shell", json!({"command": "echo first"})),
            calls("done", json!({"outcome": "gave_up", "summary": "enough"})),
        ];
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        // Added by someone with their own handle on the same directory.
        Undertaking::open(dir.path().join("state"), None)
            .unwrap()
            .add("also make it say hi twice")
            .unwrap();
        worker.run().await;
        let seen = brain.seen.lock().unwrap();
        assert!(matches!(seen[0].items.last(), Some(Item::User { text })
            if text.contains("also make it say hi twice") && text.contains("does not replace it")));
        assert!(
            worker
                .undertaking
                .state()
                .task_text()
                .ends_with("Added later: also make it say hi twice")
        );
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
                calls: vec![call("a"), call("b"), call("c")],
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
        assert_eq!(items.len(), 5);
        // The first call without a result may have begun; the one after it
        // cannot have, and is safe to run.
        assert!(
            matches!(&items[3], Item::Result { id, failed: true, output, .. }
            if id == "b" && output.contains("what it did is unknown"))
        );
        assert!(
            matches!(&items[4], Item::Result { id, failed: true, output, .. }
            if id == "c" && output.contains("was not run"))
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
        script.push(calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        ));
        let limits = Limits {
            context_tokens: Some(8000),
            ..Limits::default()
        };
        let (worker, brain) = worker(dir.path(), script, limits);
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert_eq!(report.sittings, 2);

        let seen = brain.seen.lock().unwrap();
        let at = seen
            .iter()
            .position(|t| matches!(t.items.last(), Some(Item::User { text }) if text.contains("about to be cleared")))
            .expect("a handoff was asked for");
        // Asked with the tools of every other step, and with the transcript
        // as it was: nothing in front of the request has changed, so the
        // provider reads it from its cache.
        let names = |t: &Turn| t.tools.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&seen[at]), names(&seen[0]));
        assert!(seen[at].items.starts_with(&seen[at - 1].items));
        // What it had seen last was too large to carry over, so the sitting
        // after it starts from the record alone.
        let fresh = &seen[at + 1];
        assert_eq!(fresh.items.len(), 1);
        assert!(matches!(&fresh.items[0], Item::User { text }
            if text.contains("the file goes in hi.txt") && text.contains("Do not start over")));
    }

    #[tokio::test]
    async fn the_last_stretch_survives_a_cleared_context() {
        let dir = tempfile::tempdir().unwrap();
        // Many small steps: nothing in them is long enough to trim or drop.
        let line = "y".repeat(300);
        let mut script: Vec<Step> = (0..60)
            .map(|i| calls("shell", json!({"command": format!("echo {i} {line}")})))
            .collect();
        script.push(calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        ));
        let limits = Limits {
            context_tokens: Some(8000),
            max_steps: 80,
            ..Limits::default()
        };
        let (worker, brain) = worker(dir.path(), script, limits);
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert!(report.sittings >= 2);

        let seen = brain.seen.lock().unwrap();
        let at = seen
            .iter()
            .position(|t| matches!(t.items.last(), Some(Item::User { text }) if text.contains("about to be cleared")))
            .expect("a handoff was asked for");
        let before = &seen[at].items[..seen[at].items.len() - 1];
        let fresh = &seen[at + 1].items;
        assert!(matches!(&fresh[0], Item::User { text }
            if text.contains("the file goes in hi.txt") && text.contains("exactly as it happened")));
        // After the opening comes the end of what it was doing, unchanged,
        // beginning with something it did and not with a result cut loose.
        let tail = &fresh[1..];
        assert!(tail.len() >= 4, "{} items carried over", tail.len());
        assert!(matches!(tail[0], Item::Assistant { .. }));
        assert_eq!(tail, &before[before.len() - tail.len()..]);
        // And it is a small part of what there was.
        assert!(tail.len() < before.len() / 2);

        // Nothing was lost: the history holds every step, in order.
        let history = std::fs::read_to_string(dir.path().join("state/history.jsonl")).unwrap();
        let commands = history
            .lines()
            .filter(|l| l.contains(r#""is":"assistant""#) && l.contains("echo "))
            .count();
        assert_eq!(commands, 60);
    }

    #[test]
    fn long_output_is_trimmed_before_anything_is_dropped() {
        let result = |output: String| Item::Result {
            id: "a".into(),
            name: "shell".into(),
            output,
            failed: false,
        };
        let mut items = vec![
            Item::user("task"),
            result(format!("START{}END", "z".repeat(30_000))),
            result("short".into()),
            result("w".repeat(30_000)),
        ];
        let freed = prune(&mut items, 1);
        assert!(freed > 20_000);
        let Item::Result { output, .. } = &items[1] else {
            panic!()
        };
        assert!(output.starts_with("START") && output.ends_with("END"));
        assert!(output.contains("cut from the middle"));
        assert!(output.chars().count() < PRUNE_OVER);
        assert!(matches!(&items[2], Item::Result { output, .. } if output == "short"));
        // The newest is what the worker is looking at: left whole.
        assert!(matches!(&items[3], Item::Result { output, .. } if output.len() == 30_000));
        // Trimming what is already trimmed changes nothing.
        assert_eq!(prune(&mut items, 1), 0);
    }

    #[tokio::test]
    async fn the_budget_follows_the_window_and_stays_under_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let (mut worker, brain) = worker(dir.path(), vec![], Limits::default());
        assert_eq!(worker.budget(), UNKNOWN_WINDOW);
        brain.window.store(1_048_576, Ordering::Relaxed);
        assert_eq!(worker.budget(), 838_860);
        brain.window.store(128_000, Ordering::Relaxed);
        assert_eq!(worker.budget(), 102_400);
        // A small window keeps room to answer in.
        brain.window.store(32_000, Ordering::Relaxed);
        assert_eq!(worker.budget(), 32_000 - 8192 - 4000);
        worker.limits.context_tokens = Some(50_000);
        assert_eq!(worker.budget(), 50_000);
        worker.undertaking.update(|s| s.refused_at = Some(40_000));
        assert_eq!(worker.budget(), 28_000);
    }

    #[tokio::test]
    async fn a_refused_context_is_folded_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let some = "v".repeat(3000);
        let mut script: Vec<Step> = (0..10)
            .map(|i| calls("shell", json!({"command": format!("echo {i} {some}")})))
            .collect();
        script.push(calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        ));
        // It thinks it has room for 60,000 tokens. The model takes 6,000.
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        brain.refuses_over.store(6000, Ordering::Relaxed);
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        let refused = worker.undertaking.state().refused_at.expect("remembered");
        // (The scripted brain writes a long call and reports ten tokens for
        // it, so the worker's count runs a little under the brain's.)
        assert!(refused > 4000 && refused < 12_000, "{refused}");
        assert!(worker.budget() < 6000);
        // Every request it went on to make was one the model would take.
        assert_eq!(report.steps, 11);
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
