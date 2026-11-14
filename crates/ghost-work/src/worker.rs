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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use async_trait::async_trait;
use ghost_brain::{Brain, Call, Effort, Item, Stop, ToolSpec, Turn, Usage};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    prompts::{self, Loop, Prompts, Shared, Store},
    tool::{self, Tool, Toolbox},
    tools::{Handoff, clip},
    undertaking::{Outcome, State, Undertaking, placeholder},
};

/// Tools that only look, and can be run side by side when the model asks
/// for several at once.
const SIDE_BY_SIDE: [&str; 4] = ["read", "search", "fetch", "delegate"];
/// What a helper given part of the work gets to do it in.
const HELPER_STEPS: u32 = 40;
const HELPER_SECONDS: u64 = 900;
/// How much of a helper's report comes back.
const HELPER_REPORT: usize = 8000;

/// What one step may write when nothing says and nothing is known about
/// the model.
const UNKNOWN_OUTPUT: u32 = 8192;
/// What one step may write on a model known to manage more: room to think
/// at length and still write a long file.
const KNOWN_OUTPUT: u32 = 32_000;
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
/// How many pictures the model has already looked at stay in the
/// transcript. Each one is sent again with every request, at the cost of
/// a long passage, and is seldom looked at twice; the line naming its file
/// stays, so it can be had again.
const PICTURES_KEPT: usize = 6;

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
    /// How to equip a helper. With this, the worker can hand a
    /// self-contained part of the work to another worker that has a
    /// context of its own and reports back in a few paragraphs.
    pub helpers: Option<ToolsFor>,
    /// What it is told, and what its tools are said to be. Looked at again
    /// before each step, so that an edit reaches work already running.
    pub prompts: Shared,
}

/// Builds the tools for work on a given undertaking: the same machine and
/// the same web, with a record of its own.
pub type ToolsFor = Arc<dyn Fn(&Arc<Undertaking>) -> Toolbox + Send + Sync>;

/// Part of the work, given to a helper.
struct Delegate {
    brain: Arc<dyn Brain>,
    equip: ToolsFor,
    parent: Arc<Undertaking>,
    limits: Limits,
    machine: String,
    pause: Arc<AtomicBool>,
    prompts: Shared,
}

#[async_trait]
impl Tool for Delegate {
    fn name(&self) -> &'static str {
        "delegate"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "delegate",
            json!({"task": {"type": "string"}}),
            &["task"],
        )
    }

    async fn call(&self, args: &Value) -> tool::Outcome {
        let task = match tool::text(args, "task") {
            Ok(task) => task.trim(),
            Err(refusal) => return refusal,
        };
        if task.is_empty() {
            return tool::Outcome::fail("the task is empty");
        }
        // Kept beside the work it is part of, under an id that says so.
        let helper = match self.parent.open_part(task) {
            Ok(helper) => Arc::new(helper),
            Err(e) => return tool::Outcome::fail(format!("no helper could be set up: {e}")),
        };
        let worker = Worker {
            brain: self.brain.clone(),
            tools: (self.equip)(&helper),
            undertaking: helper.clone(),
            limits: Limits {
                max_steps: HELPER_STEPS.min(self.limits.max_steps),
                max_seconds: HELPER_SECONDS.min(self.limits.max_seconds),
                ..self.limits.clone()
            },
            who: String::new(),
            machine: self.machine.clone(),
            pause: self.pause.clone(),
            helpers: None,
            prompts: self.prompts.clone(),
        };
        let report = worker.run().await;
        // What the helper spent is spent on this work.
        self.parent.update(|state| state.usage.add(report.usage));
        let said = if report.summary.trim().is_empty() {
            helper.state().progress()
        } else {
            report.summary.trim().to_owned()
        };
        let words = prompts::hold(&self.prompts);
        let how = match &report.ended {
            Ended::Finished => words.text("helper_finished").to_owned(),
            Ended::Blocked => words.text("helper_blocked").to_owned(),
            Ended::GaveUp => words.text("helper_gave_up").to_owned(),
            Ended::Failed(why) => words.fill("helper_failed", &[("why", preview(why, 160))]),
            _ => words.text("helper_ran_out").to_owned(),
        };
        tool::Outcome {
            failed: report.ended != Ended::Finished,
            ..tool::Outcome::ok(words.fill(
                "helper_report",
                &[
                    ("how", how),
                    ("steps", report.steps.to_string()),
                    ("report", clip(&said, HELPER_REPORT / 2, HELPER_REPORT / 2)),
                ],
            ))
        }
    }
}

/// What every request of a sitting shows the model before its transcript:
/// the words they come from, the instructions, and the tools.
struct Shown {
    words: Prompts,
    system: String,
    specs: Vec<ToolSpec>,
}

impl Shown {
    /// Its size in characters.
    fn weight(&self) -> usize {
        self.system.len()
            + self
                .specs
                .iter()
                .map(|s| s.description.len() + s.parameters.to_string().len())
                .sum::<usize>()
    }
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

/// Take pictures out of the transcript, all but the newest `keep` of those
/// the model has already been shown. Pictures that came back since its
/// last step have not been looked at yet, and always stay. Returns how many
/// characters that freed.
fn unsee(items: &mut [Item], keep: usize, words: &Prompts) -> usize {
    let before = chars(items);
    let shown = items
        .iter()
        .rposition(|item| matches!(item, Item::Assistant { .. }))
        .unwrap_or(0);
    let mut left = keep;
    for item in items[..shown].iter_mut().rev() {
        let Item::Result {
            output, pictures, ..
        } = item
        else {
            continue;
        };
        if pictures.len() <= left {
            left -= pictures.len();
            continue;
        }
        // Older ones would fit where this did not, but are older still.
        left = 0;
        output.push('\n');
        output.push_str(words.text(if pictures.len() == 1 {
            "no_longer_shown_one"
        } else {
            "no_longer_shown_many"
        }));
        pictures.clear();
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

fn preview(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

/// The working instructions, with who is working and where filled in.
pub fn system_prompt(words: &Prompts, who: &str, machine: &str, limits: &Limits) -> String {
    let who = if who.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n\n", who.trim())
    };
    let budget = words.fill(
        "budget",
        &[
            ("steps", limits.max_steps.to_string()),
            ("minutes", (limits.max_seconds / 60).max(1).to_string()),
        ],
    );
    ghost_core::narrate::fill(
        words.work(),
        &[
            ("who", who),
            ("machine", machine.trim().to_owned()),
            ("budget", budget),
        ],
    )
}

/// What a sitting opens with: the task alone the first time, and the task
/// with what was left behind every time after.
pub fn opening_text(words: &Prompts, state: &State) -> String {
    opening_with(words, state, false)
}

/// The opening, for a context that was cleared. With `tail`, the last
/// stretch of the old transcript follows it word for word, and the worker
/// is told to go on from there instead of finding its feet again.
fn opening_with(words: &Prompts, state: &State, tail: bool) -> String {
    let task = words.fill("task", &[("task", state.task_text())]);
    if state.steps == 0 {
        return task;
    }
    let handoff = state.handoff.as_deref().map_or_else(String::new, |h| {
        words.fill("handoff_section", &[("handoff", h.trim().to_owned())])
    });
    let touched = if state.touched.is_empty() {
        String::new()
    } else {
        words.fill("touched_section", &[("files", state.touched.join("\n"))])
    };
    let (before, after) = if tail {
        ("cleared_before", "cleared_after")
    } else {
        ("picked_up_before", "picked_up_after")
    };
    words.fill(
        "opening",
        &[
            ("task", task),
            ("before", words.text(before).to_owned()),
            ("plan", state.plan_text()),
            ("handoff", handoff),
            ("touched", touched),
            ("notes", state.notes_text(NOTES_SHOWN)),
            ("after", words.text(after).to_owned()),
            ("audit", words.text("audit").to_owned()),
        ],
    )
}

impl Worker {
    fn system(&self, words: &Prompts) -> String {
        system_prompt(words, &self.who, &self.machine, &self.limits)
    }

    /// What a sitting starts from: the task alone the first time, and the
    /// task with what was left behind every time after.
    fn opening(&self, words: &Prompts) -> Vec<Item> {
        vec![Item::user(opening_text(words, &self.undertaking.state()))]
    }

    /// What the model is shown, in `words`, with `tools`.
    fn shown(&self, tools: &Toolbox, words: Prompts) -> Shown {
        Shown {
            system: self.system(&words),
            specs: tools.specs(&words, Loop::Work),
            words,
        }
    }

    /// The words as they are now, looked at again first, and whether they
    /// changed.
    fn words(&self) -> (Prompts, bool) {
        let mut words = prompts::hold(&self.prompts);
        let changed = words.refresh_now();
        (words.clone(), changed)
    }

    /// How many tokens the transcript may reach before it is folded: what
    /// was asked for, or most of the model's window with room left to
    /// answer in, and in either case well under any size the model has
    /// already refused.
    /// How much one step may write.
    fn max_output(&self) -> u32 {
        self.limits.max_output.unwrap_or_else(|| {
            self.brain
                .max_output()
                .map_or(UNKNOWN_OUTPUT, |most| most.min(KNOWN_OUTPUT))
        })
    }

    fn budget(&self) -> u64 {
        let wanted = self.limits.context_tokens.unwrap_or_else(|| {
            self.brain.window().map_or(UNKNOWN_WINDOW, |window| {
                let reserved = u64::from(self.max_output()) + HEADROOM.min(window / 8);
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
        // Several calls that only look are run side by side; anything
        // that changes something runs alone and in order.
        let together = calls.len() > 1
            && calls
                .iter()
                .all(|c| SIDE_BY_SIDE.contains(&c.name.as_str()));
        let mut outcomes: Vec<Option<(tool::Outcome, u128)>> = vec![None; calls.len()];
        if together {
            let mut running = tokio::task::JoinSet::new();
            for (at, call) in calls.iter().enumerate() {
                let (tools, call) = (tools.clone(), call.clone());
                running.spawn(async move {
                    let started = Instant::now();
                    let outcome = tools.run(&call).await;
                    (at, outcome, started.elapsed().as_millis())
                });
            }
            while let Some(done) = running.join_next().await {
                if let Ok((at, outcome, ms)) = done {
                    outcomes[at] = Some((outcome, ms));
                }
            }
        }
        for (at, call) in calls.iter().enumerate() {
            let (outcome, ms) = match outcomes[at].take() {
                Some(done) => done,
                None if together => (tool::Outcome::fail("the tool fell over"), 0),
                None => {
                    let started = Instant::now();
                    let outcome = tools.run(call).await;
                    (outcome, started.elapsed().as_millis())
                }
            };
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
                "pictures": outcome.pictures.len(),
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
            items.push(self.take_in(call, outcome));
        }
        (logged, signature)
    }

    /// A tool's outcome as a transcript item. Its pictures are kept on
    /// disk and named in its output; a brain that cannot see them is told
    /// how many there were, and the bytes go no further.
    fn take_in(&self, call: &Call, outcome: tool::Outcome) -> Item {
        let tool::Outcome {
            mut output,
            failed,
            mut pictures,
        } = outcome;
        if !pictures.is_empty() {
            for file in self.undertaking.keep(&pictures) {
                output.push('\n');
                output.push_str(&placeholder(&file));
            }
            if !self.brain.sees() {
                output.push('\n');
                output.push_str(&ghost_brain::not_shown(pictures.len()));
                pictures.clear();
            }
        }
        Item::Result {
            id: call.id.clone(),
            name: call.name.clone(),
            output,
            failed,
            pictures,
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
        shown: &Shown,
        tools: &Toolbox,
        gauge: &mut Gauge,
        base: u64,
        ceiling: u64,
        ask: bool,
    ) -> (Usage, Option<u64>) {
        let words = &shown.words;
        // What the sitting started at cannot be folded away, so the aim is
        // set in the room above it.
        let base = base.min(ceiling);
        let target = base + (ceiling - base) * FOLD_TO / 10;
        let mut trial = items.clone();
        let mut trial_gauge = Gauge {
            measured: gauge.measured,
            fixed: gauge.fixed,
        };
        // Pictures go before any text: they cost the most, and are worth
        // the least once they have been looked at.
        let mut freed = unsee(&mut trial, 0, words);
        trial_gauge.reweigh(&trial);
        if trial_gauge.tokens(&trial) > target {
            freed += prune(&mut trial, KEEP_RECENT[2]);
            trial_gauge.reweigh(&trial);
        }
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
            let asking = Item::user(words.text("handoff"));
            let mut asking_items = items.clone();
            asking_items.push(asking.clone());
            let turn = Turn {
                system: shown.system.clone(),
                items: asking_items,
                tools: shown.specs.clone(),
                max_tokens: self.max_output(),
                temperature: self.limits.temperature,
                effort: self.limits.effort,
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
        let opening = Item::user(opening_with(words, &state, !tail.is_empty()));
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

    /// Work until it is finished, or until something stops it.
    pub async fn run(&self) -> Report {
        if let Some(finish) = self.undertaking.state().finish {
            return self.report(finish.outcome.into());
        }
        let started = Instant::now();
        // The handoff tool is there at every step, not only when it is
        // wanted: a request whose tools differ from the last one's reads
        // nothing from the provider's cache.
        let tools = self
            .tools
            .clone()
            .with(Handoff::new(self.undertaking.clone()));
        let tools = match &self.helpers {
            Some(equip) => tools.with(Delegate {
                brain: self.brain.clone(),
                equip: equip.clone(),
                parent: self.undertaking.clone(),
                limits: self.limits.clone(),
                machine: self.machine.clone(),
                pause: self.pause.clone(),
                prompts: self.prompts.clone(),
            }),
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
                return self.report(finish.outcome.into());
            }
            if self.pause.load(Ordering::Relaxed) {
                return self.report(Ended::Paused);
            }
            if started.elapsed().as_secs() >= self.limits.max_seconds {
                return self.report(Ended::OutOfTime);
            }
            // Whatever was added to the task from outside since the last
            // step is put in front of the worker before it goes on.
            let added = self.undertaking.take_added();
            if !added.is_empty() {
                // Whatever it was waiting to hear, this is the answer.
                self.undertaking.update(|state| state.waiting = None);
            } else if self.undertaking.state().waiting.is_some() {
                self.save(&items, &mut recorded);
                return self.report(Ended::Waiting);
            }
            // What it is told, as the files say it now.
            if let (words, true) = self.words() {
                shown = self.shown(&tools, words);
                gauge.fixed = shown.weight();
            }
            for added in added {
                tracing::info!("added to the task: {}", preview(&added, 160));
                items.push(Item::user(shown.words.fill("added", &[("added", added)])));
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
                return self.report(Ended::OutOfTime);
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
                    return self.report(Ended::Failed(e.to_string()));
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
                    return self.report(Ended::Stalled);
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
                let (ran, signature) = self.run_calls(&tools, &calls, &mut items).await;
                logged = ran;
                if signature == last_signature {
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
                "said": preview(&said, 400),
                "calls": logged,
            }));
            self.save(&items, &mut recorded);

            if let Some(finish) = finish {
                return self.report(finish.outcome.into());
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
        /// What it says it can write in one answer; 0 for not known.
        max_output: AtomicU64,
        /// Whether it says it can see images.
        sees: AtomicBool,
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
                thought: 0,
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
        fn max_output(&self) -> Option<u32> {
            u32::try_from(self.max_output.load(Ordering::Relaxed))
                .ok()
                .filter(|m| *m > 0)
        }
        fn sees(&self) -> bool {
            self.sees.load(Ordering::Relaxed)
        }
        async fn step(&self, turn: &Turn) -> Result<Step> {
            // Like a real model, it reads as much as it is shown.
            let size = (chars(&turn.items) / 3).max(100) as u64;
            let limit = self.refuses_over.load(Ordering::Relaxed);
            if limit > 0 && size > limit {
                return Err(ghost_brain::Error::TooLong {
                    provider: "scripted".into(),
                    detail: "more than this model's window".into(),
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

    /// Kept apart from the directory the work is done in.
    fn state_db(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("state").join(crate::WORK_DB)
    }

    fn worker(dir: &std::path::Path, script: Vec<Step>, limits: Limits) -> (Worker, Arc<Scripted>) {
        let undertaking = Arc::new(
            Undertaking::open(&state_db(dir), "main", Some("make hi.txt say hi")).unwrap(),
        );
        let brain = Arc::new(Scripted {
            steps: Mutex::new(script),
            seen: Mutex::new(vec![]),
            window: AtomicU64::new(0),
            refuses_over: AtomicU64::new(0),
            max_output: AtomicU64::new(0),
            sees: AtomicBool::new(false),
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
            helpers: None,
            prompts: Prompts::shipped().shared(),
        };
        (worker, brain)
    }

    #[tokio::test]
    async fn work_ends_when_it_says_it_is_finished() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
            calls("shell", json!({"command": "cat hi.txt"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote it and read it back"}),
            ),
        ];
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Finished);
        assert_eq!(report.summary, "wrote it and read it back");
        assert_eq!(report.steps, 3);
        assert!(report.usage.input >= 300);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hi.txt")).unwrap(),
            "hi\n"
        );
        // Its word is taken: nothing was sent back, and nobody was asked to
        // read it over. Three steps, three requests.
        let seen = brain.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        // How to finish was said at the start, in the instructions.
        assert!(seen[0].system.contains("Nobody checks after you"));
        // The log has one line per step.
        assert_eq!(worker.undertaking.logged().len(), 3);
    }

    #[tokio::test]
    async fn something_added_mid_work_reaches_the_worker_before_its_next_step() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("shell", json!({"command": "echo first"})),
            calls("done", json!({"outcome": "gave_up", "summary": "enough"})),
        ];
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        // Added by someone with their own handle on the same database.
        Undertaking::open(&state_db(dir.path()), "main", None)
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
            Item::result("a", "shell", "ok", false),
        ];
        mend(&mut items, &Prompts::shipped());
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
        mend(&mut items, &Prompts::shipped());
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
        // Work that has outlived a context is told how to finish when it
        // is picked up, not caught at the end.
        assert!(matches!(&fresh.items[0], Item::User { text }
            if text.contains("treat that as unproven")));
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
        let commands = worker
            .undertaking
            .history()
            .iter()
            .filter(|item| matches!(item, Item::Assistant { calls, .. }
                if calls.iter().any(|c| c.args["command"].as_str().is_some_and(|c| c.starts_with("echo ")))))
            .count();
        assert_eq!(commands, 60);
    }

    #[test]
    fn long_output_is_trimmed_before_anything_is_dropped() {
        let result = |output: String| Item::result("a", "shell", output, false);
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
        // What a step may write is what the model is known to manage, up
        // to a ceiling; little, for a model nothing is known about.
        assert_eq!(worker.max_output(), 8192);
        brain.max_output.store(393_216, Ordering::Relaxed);
        assert_eq!(worker.max_output(), 32_000);
        brain.max_output.store(0, Ordering::Relaxed);
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
    async fn a_question_stops_the_work_until_it_is_answered() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("ask", json!({"question": "plain text or markdown?"})),
            // After the answer.
            calls("write", json!({"path": "out.md", "content": "# hi\n"})),
            calls(
                "done",
                json!({"outcome": "blocked", "summary": "enough", "files": ["/x/out.md"]}),
            ),
        ];
        let (mut first, brain) = worker(dir.path(), script, Limits::default());
        first.tools = first.tools.with(tools::Ask::new(first.undertaking.clone()));
        let report = first.run().await;
        assert_eq!(report.ended, Ended::Waiting);
        assert_eq!(report.asking.as_deref(), Some("plain text or markdown?"));
        assert_eq!(report.steps, 1);
        // Picked up with no answer, it is still waiting, and asks the
        // model for nothing.
        assert_eq!(first.run().await.ended, Ended::Waiting);
        assert_eq!(brain.seen.lock().unwrap().len(), 1);

        first.undertaking.add("markdown").unwrap();
        let report = first.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert_eq!(report.asking, None);
        assert_eq!(report.files, ["/x/out.md"]);
        // The answer was in front of it when it went on.
        let seen = brain.seen.lock().unwrap();
        assert!(seen[1].items.iter().any(|item| matches!(item,
            Item::User { text } if text.contains("markdown") && text.contains("added to the task"))));
    }

    #[tokio::test]
    async fn a_helper_does_part_of_the_work_and_only_its_report_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "needle-in-a-file").unwrap();
        let script = vec![
            calls("delegate", json!({"task": "say what is in a.txt"})),
            // The helper's own steps.
            calls("shell", json!({"command": "cat a.txt"})),
            calls(
                "done",
                json!({"outcome": "gave_up", "summary": "it holds one word about a needle"}),
            ),
            // Back with the one who asked.
            calls("done", json!({"outcome": "blocked", "summary": "enough"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        let root = dir.path().to_path_buf();
        worker.helpers = Some(Arc::new(move |undertaking| {
            tools::standard(Arc::new(Local::new(&root)), undertaking)
        }));
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert_eq!(report.steps, 2, "the helper's steps are its own");

        let seen = brain.seen.lock().unwrap();
        // The helper started from the task it was given and nothing else.
        assert!(matches!(&seen[1].items[..],
            [Item::User { text }] if text.contains("say what is in a.txt")));
        assert!(!seen[1].tools.iter().any(|t| t.name == "delegate"));
        // What it read stayed with it. Its report came back.
        let last = &seen.last().unwrap().items;
        assert!(last.iter().any(|item| matches!(item,
            Item::Result { name, output, .. }
                if name == "delegate" && output.contains("one word about a needle"))));
        assert!(!last.iter().any(|item| item.weight() > 0
            && matches!(item, Item::Result { output, .. } if output.contains("needle-in-a-file"))));
        // What it spent is counted with the work it was part of.
        assert!(report.usage.input >= seen.iter().map(|_| 100).sum::<u64>());
        // The helper's record is kept beside the work it was part of.
        let helper = Undertaking::open(&state_db(dir.path()), "main/1", None).unwrap();
        assert_eq!(helper.state().brief, "say what is in a.txt");
    }

    #[tokio::test]
    async fn a_helper_started_in_a_later_sitting_is_new_work_not_the_one_already_finished() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let equip: ToolsFor =
            Arc::new(move |undertaking| tools::standard(Arc::new(Local::new(&root)), undertaking));

        // The first sitting hands out one part, which finishes, and then
        // runs out of steps.
        let script = vec![
            calls("delegate", json!({"task": "count the files"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "there are three files"}),
            ),
        ];
        let limits = Limits {
            max_steps: 1,
            ..Limits::default()
        };
        let (mut first, _) = worker(dir.path(), script, limits);
        first.helpers = Some(equip.clone());
        assert_eq!(first.run().await.ended, Ended::OutOfSteps);

        // The second sitting hands out another.
        let script = vec![
            calls("delegate", json!({"task": "name the largest file"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "the largest is big.bin"}),
            ),
            calls("done", json!({"outcome": "blocked", "summary": "enough"})),
        ];
        let (mut second, brain) = worker(dir.path(), script, Limits::default());
        second.helpers = Some(equip);
        assert_eq!(second.run().await.ended, Ended::Blocked);

        let seen = brain.seen.lock().unwrap();
        // The new helper did work: it was asked, and it was asked the new task.
        assert!(matches!(&seen[1].items[..],
            [Item::User { text }] if text.contains("name the largest file")));
        // What came back is the new report, not the first sitting's.
        let last = &seen.last().unwrap().items;
        let answers: Vec<&str> = last
            .iter()
            .filter_map(|item| match item {
                Item::Result { name, output, .. } if name == "delegate" => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(answers.len(), 2);
        assert!(answers[0].contains("three files"));
        assert!(answers[1].contains("big.bin") && !answers[1].contains("three files"));
        // Both are on record, each under its own id.
        let db = state_db(dir.path());
        let one = Undertaking::open(&db, "main/1", None).unwrap();
        let two = Undertaking::open(&db, "main/2", None).unwrap();
        assert_eq!(one.state().brief, "count the files");
        assert_eq!(two.state().brief, "name the largest file");
    }

    #[tokio::test]
    async fn calls_that_only_look_are_answered_together_and_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "first").unwrap();
        std::fs::write(dir.path().join("b.txt"), "second").unwrap();
        let both = Step {
            calls: vec![
                Call {
                    id: "one".into(),
                    name: "read".into(),
                    args: json!({"path": "a.txt"}),
                },
                Call {
                    id: "two".into(),
                    name: "read".into(),
                    args: json!({"path": "b.txt"}),
                },
            ],
            stop: Stop::Calls,
            ..says("")
        };
        let script = vec![
            both,
            calls("done", json!({"outcome": "blocked", "summary": "enough"})),
        ];
        let (worker, brain) = worker(dir.path(), script, Limits::default());
        assert_eq!(worker.run().await.ended, Ended::Blocked);
        let seen = brain.seen.lock().unwrap();
        let answers: Vec<(&str, bool)> = seen[1]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Result { id, output, .. } => Some((
                    id.as_str(),
                    output.contains(if id == "one" { "first" } else { "second" }),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(answers, [("one", true), ("two", true)]);
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

    /// A tool that takes a while, during which something happens outside
    /// the work: what the closure does.
    struct Meanwhile(Box<dyn Fn() + Send + Sync>);

    #[async_trait]
    impl Tool for Meanwhile {
        fn name(&self) -> &'static str {
            "wait"
        }
        fn spec(&self, _: &Store, _: Loop) -> ToolSpec {
            tool::spec("wait", "Wait a moment.", json!({}), &[])
        }
        async fn call(&self, _: &Value) -> tool::Outcome {
            (self.0)();
            tool::Outcome::ok("waited")
        }
    }

    /// What happens in `meanwhile` happens from another handle on the
    /// same record, while the worker's first step is running.
    fn while_it_runs(
        dir: &std::path::Path,
        meanwhile: impl Fn(&Undertaking) + Send + Sync + 'static,
    ) -> Meanwhile {
        let db = state_db(dir);
        Meanwhile(Box::new(move || {
            meanwhile(&Undertaking::open(&db, "main", None).unwrap());
        }))
    }

    #[tokio::test]
    async fn words_added_while_a_step_runs_reach_the_worker_at_its_next_step() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("wait", json!({})),
            calls("done", json!({"outcome": "gave_up", "summary": "enough"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        worker.tools = worker.tools.with(while_it_runs(dir.path(), |outside| {
            outside.add("only the weekday hours, after all").unwrap();
        }));
        worker.run().await;
        let seen = brain.seen.lock().unwrap();
        assert!(!seen[0].items.iter().any(|item| matches!(item,
            Item::User { text } if text.contains("weekday hours"))));
        assert!(matches!(seen[1].items.last(), Some(Item::User { text })
            if text.contains("only the weekday hours, after all")));
    }

    #[tokio::test]
    async fn work_stopped_from_outside_ends_as_dropped_after_its_step_and_keeps_what_it_kept() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            Step {
                calls: vec![
                    Call {
                        id: "k".into(),
                        name: "keep".into(),
                        args: json!({"file": "PLACEHOLDER", "note": "a pixel"}),
                    },
                    Call {
                        id: "w".into(),
                        name: "wait".into(),
                        args: json!({}),
                    },
                ],
                stop: Stop::Calls,
                ..says("")
            },
            calls("write", json!({"path": "late.txt", "content": "too late"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        let file = worker.undertaking.keep(&[ghost_brain::Picture {
            mime: "image/png".into(),
            bytes: tools::testing::PIXEL.to_vec(),
        }])[0]
            .clone();
        brain.steps.lock().unwrap()[0].calls[0].args["file"] = json!(file);
        worker.tools = worker
            .tools
            .with(tools::Keep::new(worker.undertaking.clone()))
            .with(while_it_runs(dir.path(), |outside| {
                assert!(outside.stop(), "it was still going");
            }));
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Dropped);
        assert_eq!(report.steps, 1, "nothing after the step it was on");
        assert_eq!(brain.seen.lock().unwrap().len(), 1);
        assert!(!dir.path().join("late.txt").exists());
        let state = worker.undertaking.state();
        assert_eq!(state.finish.unwrap().outcome, Outcome::Dropped);
        assert_eq!(state.kept.len(), 1, "what it kept is still on record");
        // Picked up again, it does not start over.
        assert_eq!(worker.run().await.ended, Ended::Dropped);
        assert!(!worker.undertaking.stop(), "it had already ended");
    }

    /// A tool that returns a new picture every time it is called.
    struct Camera(AtomicU64);

    #[async_trait]
    impl Tool for Camera {
        fn name(&self) -> &'static str {
            "camera"
        }
        fn spec(&self, _: &Store, _: Loop) -> ToolSpec {
            tool::spec("camera", "Take a picture.", json!({}), &[])
        }
        async fn call(&self, _: &Value) -> tool::Outcome {
            let mut bytes = tools::testing::PIXEL.to_vec();
            bytes.extend(self.0.fetch_add(1, Ordering::Relaxed).to_be_bytes());
            tool::Outcome::seen(
                "A picture.",
                ghost_brain::Picture {
                    mime: "image/png".into(),
                    bytes,
                },
            )
        }
    }

    fn pictures_in(item: &Item) -> usize {
        match item {
            Item::Result { pictures, .. } => pictures.len(),
            _ => 0,
        }
    }

    #[tokio::test]
    async fn a_brain_that_sees_is_shown_a_fetched_image_and_it_is_kept_under_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let site = tools::testing::serve(vec![(
            "/dot.png",
            "image/png",
            tools::testing::PIXEL.to_vec(),
        )])
        .await;
        let script = vec![
            calls("fetch", json!({"url": format!("{site}/dot.png")})),
            calls("done", json!({"outcome": "blocked", "summary": "enough"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        worker.tools = tools::with_web(worker.tools, &tools::WebConfig::default());
        brain.sees.store(true, Ordering::Relaxed);
        assert_eq!(worker.run().await.ended, Ended::Blocked);

        let file = ghost_core::media::name(tools::testing::PIXEL, "image/png");
        let kept = dir.path().join("state").join("media").join(&file);
        assert_eq!(std::fs::read(kept).unwrap(), tools::testing::PIXEL);
        let seen = brain.seen.lock().unwrap();
        let Some(Item::Result {
            output, pictures, ..
        }) = seen[1].items.last()
        else {
            panic!("the fetch was answered")
        };
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].bytes, tools::testing::PIXEL);
        assert!(output.ends_with(&format!("[image: {file}]")), "{output}");
        // The history names the file and does not hold the bytes.
        let history = worker.undertaking.history();
        assert!(history.iter().all(|item| pictures_in(item) == 0));
        assert!(history.iter().any(|item| matches!(item,
            Item::Result { output, .. } if output.contains(&format!("[image: {file}]")))));
    }

    #[tokio::test]
    async fn a_worker_picked_up_from_disk_gets_its_pictures_back_from_media() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits {
            max_steps: 1,
            ..Limits::default()
        };
        let (mut first, eyes) = worker(dir.path(), vec![calls("camera", json!({}))], limits);
        first.tools = first.tools.with(Camera(AtomicU64::new(7)));
        eyes.sees.store(true, Ordering::Relaxed);
        assert_eq!(first.run().await.ended, Ended::OutOfSteps);
        // What was saved names the picture; the bytes are in media/ alone.
        let mut bytes = tools::testing::PIXEL.to_vec();
        bytes.extend(7u64.to_be_bytes());
        let file = ghost_core::media::name(&bytes, "image/png");
        let row: String = rusqlite::Connection::open(state_db(dir.path()))
            .unwrap()
            .query_row("SELECT items FROM transcript", [], |row| row.get(0))
            .unwrap();
        assert!(row.contains(&file) && !row.contains("\"data\""), "{row}");

        let script = vec![calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        )];
        let (second, brain) = worker(dir.path(), script, Limits::default());
        brain.sees.store(true, Ordering::Relaxed);
        assert_eq!(second.run().await.ended, Ended::Blocked);
        let seen = brain.seen.lock().unwrap();
        let Some(Item::Result { pictures, .. }) = seen[0].items.last() else {
            panic!("the picture's result was picked up")
        };
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].bytes, bytes);
    }

    #[tokio::test]
    async fn a_brain_that_cannot_see_is_told_how_many_images_it_was_not_shown() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            calls("camera", json!({})),
            calls("done", json!({"outcome": "blocked", "summary": "enough"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        worker.tools = worker.tools.with(Camera(AtomicU64::new(0)));
        assert_eq!(worker.run().await.ended, Ended::Blocked);
        let seen = brain.seen.lock().unwrap();
        let last = seen[1].items.last().unwrap();
        assert_eq!(pictures_in(last), 0);
        assert!(matches!(last, Item::Result { output, .. }
            if output.starts_with("A picture.\n[image: ")
                && output.ends_with("\n[1 image, not shown: this model cannot see images]")));
    }

    #[tokio::test]
    async fn old_pictures_are_shed_before_any_text_when_the_transcript_is_folded() {
        let dir = tempfile::tempdir().unwrap();
        // Long enough to be trimmed if room were made by trimming text.
        let mut script = vec![calls(
            "shell",
            json!({"command": "printf 't%.0s' $(seq 1 9000)"}),
        )];
        script.extend((0..6).map(|_| calls("camera", json!({}))));
        script.push(calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough"}),
        ));
        let limits = Limits {
            context_tokens: Some(12_000),
            ..Limits::default()
        };
        let (mut worker, brain) = worker(dir.path(), script, limits);
        worker.tools = worker.tools.with(Camera(AtomicU64::new(0)));
        brain.sees.store(true, Ordering::Relaxed);
        let report = worker.run().await;
        assert_eq!(report.ended, Ended::Blocked);
        assert_eq!(report.sittings, 1, "shedding pictures was enough");

        let seen = brain.seen.lock().unwrap();
        let shown = |t: &Turn| t.items.iter().map(pictures_in).sum::<usize>();
        // The pictures piled up until there was no room, then went.
        let most = seen.iter().map(shown).max().unwrap();
        assert!(most >= 3, "{most}");
        let after = seen
            .iter()
            .position(|t| {
                t.items.iter().any(|item| {
                    matches!(item,
                Item::Result { output, .. } if output.contains("no longer shown"))
                })
            })
            .expect("old pictures were shed");
        assert!(shown(&seen[after]) < most);
        // The one it had not looked at yet was still there to be looked at.
        assert_eq!(pictures_in(seen[after].items.last().unwrap()), 1);
        // And the long output, older than all of them, was never cut.
        for turn in seen.iter().skip(1) {
            assert!(turn.items.iter().any(|item| matches!(item,
                Item::Result { name, output, .. } if name == "shell" && output.len() >= 9000)));
        }
    }

    #[test]
    fn only_the_newest_pictures_already_looked_at_are_kept() {
        let picture = |n: u8| ghost_brain::Picture {
            mime: "image/png".into(),
            bytes: vec![n],
        };
        let result = |id: &str, n: usize| Item::Result {
            id: id.into(),
            name: "camera".into(),
            output: "A picture.".into(),
            failed: false,
            pictures: (0..n).map(|i| picture(u8::try_from(i).unwrap())).collect(),
        };
        let asked = Item::Assistant {
            text: String::new(),
            calls: vec![],
            carry: None,
        };
        let mut items = vec![
            Item::user("task"),
            asked.clone(),
            result("a", 1),
            asked.clone(),
            result("b", 2),
            asked.clone(),
            result("c", 1),
            asked,
            // Not looked at yet.
            result("d", 3),
        ];
        let freed = unsee(&mut items, 2, &Prompts::shipped());
        assert!(freed > 2 * ghost_brain::PICTURE_WEIGHT, "{freed}");
        let counts: Vec<usize> = items.iter().map(pictures_in).collect();
        // `b` has two and only one would fit: both go, and so does `a`.
        assert_eq!(counts, [0, 0, 0, 0, 0, 0, 1, 0, 3]);
        assert!(matches!(&items[4], Item::Result { output, .. }
            if output.ends_with("[These images are no longer shown, to save room. Ask for them again if you need to see them.]")));
        // Even with nothing kept, what has not been looked at stays.
        unsee(&mut items, 0, &Prompts::shipped());
        assert_eq!(items.iter().map(pictures_in).sum::<usize>(), 3);
    }

    /// Compare `got` with what was recorded under `name`, or record it when
    /// `GHOST_RECORD` is set.
    fn compare(name: &str, got: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/requests")
            .join(format!("{name}.txt"));
        if std::env::var_os("GHOST_RECORD").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, got).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path).unwrap();
        if want != got {
            let at = want
                .lines()
                .zip(got.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(want.lines().count().min(got.lines().count()));
            panic!(
                "{name} differs from what was recorded, first at line {}:\nrecorded: {:?}\nnow:      {:?}",
                at + 1,
                want.lines().nth(at),
                got.lines().nth(at)
            );
        }
    }

    #[tokio::test]
    async fn a_tool_described_anew_in_the_ghosts_tools_toml_reaches_work_already_running() {
        let dir = tempfile::tempdir().unwrap();
        // The work writes the file itself, between its first step and its
        // second, as a person editing it would.
        let script = vec![
            calls(
                "write",
                json!({
                    "path": "prompts/tools.toml",
                    "content": "[shell]\ndescription = \"Run it.\"\n\n[shell.parameters]\ncommand = \"What to run.\"\n",
                }),
            ),
            calls("done", json!({"outcome": "finished", "summary": "ok"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        worker.prompts = Prompts::load(dir.path(), None).shared();
        worker.run().await;
        let seen = brain.seen.lock().unwrap();
        let shell = |turn: &Turn| {
            turn.tools
                .iter()
                .find(|t| t.name == "shell")
                .cloned()
                .unwrap()
        };
        let shipped = Prompts::shipped();
        assert_eq!(
            shell(&seen[0]).description,
            shipped.store().tool("shell", Loop::Work)
        );
        let now = shell(&seen[1]);
        assert_eq!(now.description, "Run it.");
        assert_eq!(
            now.parameters["properties"]["command"]["description"],
            "What to run."
        );
        // What the file does not mention keeps its built-in words.
        assert_eq!(
            now.parameters["properties"]["timeout"]["description"],
            shipped
                .store()
                .parameter("shell", "timeout", Loop::Work)
                .unwrap()
        );
    }

    #[tokio::test]
    async fn what_a_worker_is_told_comes_from_the_files_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = dir.path().join("elsewhere");
        std::fs::create_dir_all(&prompts).unwrap();
        std::fs::write(
            prompts.join("work.md"),
            "{{who}}Work here: {{machine}}. {{budget}}",
        )
        .unwrap();
        std::fs::write(
            prompts.join("work_texts.toml"),
            "no_call = \"Call a tool.\"\nbudget = \"{{steps}} steps.\"\n",
        )
        .unwrap();
        let script = vec![
            says("I would."),
            calls("done", json!({"outcome": "finished", "summary": "ok"})),
        ];
        let (mut worker, brain) = worker(dir.path(), script, Limits::default());
        worker.prompts = Prompts::load(&dir.path().join("state"), Some(&prompts)).shared();
        worker.run().await;
        let seen = brain.seen.lock().unwrap();
        assert_eq!(seen[0].system, "Work here: a test directory. 200 steps.");
        assert!(
            matches!(seen[1].items.last(), Some(Item::User { text }) if text == "Call a tool.")
        );
    }

    #[tokio::test]
    async fn with_nothing_overridden_a_worker_asks_exactly_what_it_asked_before() {
        let dir = tempfile::tempdir().unwrap();
        let script = vec![
            says("I will write it."),
            calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
            calls("shell", json!({"command": "cat hi.txt"})),
            calls("delegate", json!({"task": "say what is in hi.txt"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "it says hi"}),
            ),
            calls("note", json!({"text": "hi.txt holds hi"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote it"}),
            ),
        ];
        let limits = Limits {
            max_steps: 5,
            ..Limits::default()
        };
        let (mut worker, brain) = worker(dir.path(), script, limits);
        let root = dir.path().to_path_buf();
        worker.helpers = Some(Arc::new(move |undertaking| {
            tools::standard(Arc::new(Local::new(&root)), undertaking)
        }));
        let u = worker.undertaking.clone();
        worker.tools = tools::with_web(worker.tools.clone(), &tools::WebConfig::default())
            .with(tools::Keep::new(u.clone()))
            .with(tools::Tell::new(u.clone()))
            .with(tools::Ask::new(u.clone()));
        worker.run().await;
        let mut out = String::new();
        for turn in brain.seen.lock().unwrap().iter() {
            out.push_str(&format!(
                "system: {}\nitems: {:#?}\ntools: {:#?}\n\n",
                turn.system, turn.items, turn.tools
            ));
        }
        let words = Prompts::shipped();
        let mut state = u.state();
        state.steps = 3;
        out.push_str(&format!(
            "opening: {}\n\ncleared: {}\n\n",
            opening_with(&words, &state, false),
            opening_with(&words, &state, true)
        ));
        let call = |id: &str| Call {
            id: id.into(),
            name: "shell".into(),
            args: json!({"command": "make"}),
        };
        let mut items = vec![Item::Assistant {
            text: String::new(),
            calls: vec![call("a"), call("b")],
            carry: None,
        }];
        mend(&mut items, &words);
        out.push_str(&format!("mended: {items:#?}\n\n"));
        for said in ["no_call", "cut_off", "thought_out", "repeating", "handoff"] {
            out.push_str(&format!("nudge: {}\n", words.text(said)));
        }
        compare("worker", &out);
    }
}
