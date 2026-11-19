//! `plan`, `note`, `done`: the tools that write to the undertaking itself.
//!
//! These are what survive when a transcript is folded away, so they are how
//! a worker talks to its later self.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use crate::{
    prompts::{self, Loop, Shared, Store},
    tool::{Outcome, Tool, text},
    undertaking::{self, AskKind, Finish, PlanStep, Progress, State, Undertaking},
};

/// The plan: what is left to do, how each step will be known to be done,
/// and what shows the ones that are.
pub struct Plan {
    undertaking: Arc<Undertaking>,
    words: Shared,
}

impl Plan {
    pub fn new(undertaking: Arc<Undertaking>, words: Shared) -> Self {
        Self { undertaking, words }
    }
}

/// What a tool says when its change to the record could not be written:
/// nothing was changed, and the worker is told so rather than left to go
/// on from a record it does not have.
fn unwritten(e: &std::io::Error) -> Outcome {
    Outcome::fail(format!(
        "the work's record could not be written, so nothing was changed: {e}"
    ))
}

/// Models name the same states many ways.
fn progress(word: &str) -> Progress {
    match word.trim().to_lowercase().as_str() {
        "doing" | "in_progress" | "in progress" | "active" | "current" => Progress::Doing,
        "done" | "completed" | "complete" | "finished" => Progress::Done,
        "dropped" | "cancelled" | "canceled" | "skipped" => Progress::Dropped,
        "failed" | "failure" => Progress::Failed,
        _ => Progress::Todo,
    }
}

/// A step's number as a model writes it: 2, "2", "step 2", "#2".
pub(crate) fn step_number(value: &Value) -> Option<usize> {
    match value {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s
            .trim()
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .ok(),
        _ => None,
    }
}

/// A string argument of a step, or `kept` when it was left out. Given
/// empty, it is cleared.
fn given(step: &Value, key: &str, kept: impl FnOnce() -> Option<String>) -> Option<String> {
    match step.get(key) {
        None | Some(Value::Null) => kept(),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    }
}

/// The plan `steps` describe, built on `old`. A plan is written whole each
/// time, and a model bringing one step's status up to date seldom repeats
/// everything else about every step. So a step with the same text as one
/// before keeps what it does not say again: its check, what it is after
/// (renumbered to where those steps are now), the part on it, and its
/// result while its status stays the same.
fn rewrite(steps: &[Value], old: &[PlanStep]) -> Vec<PlanStep> {
    // Each step as given, with the step of the old plan it continues, if
    // any, and what it is after if it says.
    let mut given_steps: Vec<(PlanStep, Option<usize>, Option<Vec<usize>>)> = Vec::new();
    for step in steps {
        // A bare string is a step that has not been started.
        let (words, status) = match step {
            Value::String(words) => (words.as_str(), Progress::Todo),
            other => match other["text"].as_str() {
                Some(words) => (words, progress(other["status"].as_str().unwrap_or("todo"))),
                None => continue,
            },
        };
        let words = words.trim();
        if words.is_empty() {
            continue;
        }
        let was = (0..old.len())
            .find(|&i| old[i].text == words && !given_steps.iter().any(|(_, w, _)| *w == Some(i)));
        let before = was.map(|i| &old[i]);
        let after = step.get("after").filter(|a| !a.is_null()).map(|a| match a {
            Value::Array(list) => list.iter().filter_map(step_number).collect(),
            one => step_number(one).into_iter().collect(),
        });
        let check = given(step, "check", || before.and_then(|b| b.check.clone()));
        let result = given(step, "result", || {
            before
                .filter(|b| b.status == status)
                .and_then(|b| b.result.clone())
        });
        let step = PlanStep {
            text: words.to_owned(),
            status,
            check,
            after: Vec::new(),
            part: before.and_then(|b| b.part),
            result,
        };
        given_steps.push((step, was, after));
    }
    // Where the old plan's step `n` is now, counted from 1.
    let now_at = |n: usize| {
        given_steps
            .iter()
            .position(|(_, was, _)| *was == Some(n.wrapping_sub(1)))
            .map(|at| at + 1)
    };
    let afters: Vec<Vec<usize>> = given_steps
        .iter()
        .map(|(_, was, after)| match (after, was) {
            (Some(after), _) => after.clone(),
            (None, Some(i)) => old[*i].after.iter().filter_map(|&n| now_at(n)).collect(),
            (None, None) => Vec::new(),
        })
        .collect();
    let len = given_steps.len();
    given_steps
        .into_iter()
        .zip(afters)
        .enumerate()
        .map(|(i, ((mut step, _, _), mut after))| {
            // Only other steps of this plan, each once.
            after.retain(|&n| n >= 1 && n <= len && n != i + 1);
            after.sort_unstable();
            after.dedup();
            step.after = after;
            step
        })
        .collect()
}

/// What is wrong with `state`'s plan, a line each, in `words`: a step
/// with a check marked done without saying how the check was met, one
/// dropped without saying why, one in progress before what it is after is
/// done.
fn problems(state: &State, words: &prompts::Prompts) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, step) in state.plan.iter().enumerate() {
        let number = ("step", (i + 1).to_string());
        match step.status {
            Progress::Done if step.check.is_some() && step.result.is_none() => {
                problems.push(words.fill("plan_unproven", &[number]));
            }
            Progress::Dropped if step.result.is_none() => {
                problems.push(words.fill("plan_no_reason", &[number]));
            }
            Progress::Doing => {
                let waiting = state.waiting_on(i);
                if !waiting.is_empty() {
                    problems
                        .push(words.fill("plan_not_ready", &[number, ("waiting", list(&waiting))]));
                }
            }
            _ => {}
        }
    }
    problems
}

/// Step numbers, as a list a person would write.
pub(crate) fn list(numbers: &[usize]) -> String {
    numbers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[async_trait]
impl Tool for Plan {
    fn name(&self) -> &'static str {
        "plan"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "plan",
            json!({
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "status": {
                                "type": "string",
                                "enum": ["todo", "doing", "done", "dropped", "failed"],
                            },
                            "check": {"type": "string"},
                            "after": {"type": "array", "items": {"type": "integer"}},
                            "result": {"type": "string"},
                        },
                        "required": ["text", "status"],
                    },
                },
            }),
            &["steps"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let Some(steps) = args["steps"].as_array() else {
            return Outcome::fail("missing argument `steps` (a list of {text, status})");
        };
        let words = prompts::hold(&self.words).clone();
        // Read, checked and written in one go, so that a part ending
        // meanwhile and marking its step is built on and not lost.
        let said = self.undertaking.update(|state| {
            let plan = rewrite(steps, &state.plan);
            if plan.is_empty() {
                return Err("the plan has no steps".to_owned());
            }
            let was = std::mem::replace(&mut state.plan, plan);
            let problems = problems(state, &words);
            if problems.is_empty() {
                Ok(words.fill("plan_now", &[("plan", state.plan_text(&words))]))
            } else {
                state.plan = was;
                Err(words.fill("plan_refused", &[("problems", problems.join("\n"))]))
            }
        });
        match said {
            Ok(Ok(shown)) => Outcome::ok(shown),
            Ok(Err(why)) => Outcome::fail(why),
            Err(e) => unwritten(&e),
        }
    }
}

pub struct Note {
    undertaking: Arc<Undertaking>,
}

impl Note {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Note {
    fn name(&self) -> &'static str {
        "note"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "note",
            json!({"text": {"type": "string"}}),
            &["text"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let words = match text(args, "text") {
            Ok(t) => t.trim(),
            Err(refusal) => return refusal,
        };
        if words.is_empty() {
            return Outcome::fail("the note is empty");
        }
        let count = self.undertaking.update(|state| {
            state.notes.push(undertaking::Note {
                at: Utc::now(),
                text: words.to_owned(),
            });
            state.notes.len()
        });
        match count {
            Ok(count) => Outcome::ok(format!("noted ({count} so far)")),
            Err(e) => unwritten(&e),
        }
    }
}

/// Offered only when a context is about to be cleared.
pub struct Handoff {
    undertaking: Arc<Undertaking>,
}

impl Handoff {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Handoff {
    fn name(&self) -> &'static str {
        "handoff"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "handoff",
            json!({"text": {"type": "string"}}),
            &["text"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let words = match text(args, "text") {
            Ok(t) => t.trim(),
            Err(refusal) => return refusal,
        };
        if words.is_empty() {
            return Outcome::fail("the handoff is empty");
        }
        match self
            .undertaking
            .update(|state| state.handoff = Some(words.to_owned()))
        {
            Ok(()) => Outcome::ok("kept"),
            Err(e) => unwritten(&e),
        }
    }
}

/// A question for whoever the work is for. Only offered when there is
/// someone to ask and a way for the answer to come back.
pub struct Ask {
    undertaking: Arc<Undertaking>,
}

impl Ask {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Ask {
    fn name(&self) -> &'static str {
        "ask"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "ask",
            json!({
                "question": {"type": "string"},
                "kind": {"type": "string", "enum": ["answer", "file", "act", "choose"]},
                "options": {"type": "array", "items": {"type": "string"}},
            }),
            &["question"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let question = match text(args, "question") {
            Ok(q) => q.trim(),
            Err(refusal) => return refusal,
        };
        if question.is_empty() {
            return Outcome::fail("the question is empty");
        }
        let options: Vec<String> = args["options"]
            .as_array()
            .map(|options| {
                options
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|o| !o.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        // Options given without a kind can only be a choice.
        let kind = match args["kind"].as_str().filter(|k| !k.trim().is_empty()) {
            Some(word) => match AskKind::from_word(word) {
                Some(kind) => Some(kind),
                None => {
                    return Outcome::fail(format!(
                        "there is no kind {word:?}: it is answer, file, act or choose"
                    ));
                }
            },
            None => (!options.is_empty()).then_some(AskKind::Choose),
        };
        if kind == Some(AskKind::Choose) && options.len() < 2 {
            return Outcome::fail("a choice needs at least two `options` to choose from");
        }
        if kind != Some(AskKind::Choose) && !options.is_empty() {
            return Outcome::fail("`options` are only for a question of kind choose");
        }
        match self
            .undertaking
            .update(|state| state.ask(question, kind, options))
        {
            Ok(()) => Outcome::ok(
                "Asked. The work stops here until they answer; their answer will be added to the task.",
            ),
            Err(e) => unwritten(&e),
        }
    }
}

/// Keep an image the work came across, to be had again. Whoever the work
/// is done for takes the list from the record as the work goes, not only
/// once it is over; the worker itself writes nowhere else.
pub struct Keep {
    undertaking: Arc<Undertaking>,
}

impl Keep {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Keep {
    fn name(&self) -> &'static str {
        "keep"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "keep",
            json!({"file": {"type": "string"}, "note": {"type": "string"}}),
            &["file", "note"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (file, note) = match (text(args, "file"), text(args, "note")) {
            (Ok(f), Ok(n)) => (f.trim(), n.trim()),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        // As it was shown, or as its place under `media/`.
        let file = file
            .strip_prefix("[image:")
            .and_then(|f| f.strip_suffix(']'))
            .unwrap_or(file)
            .trim();
        let file = file.strip_prefix("media/").unwrap_or(file);
        if note.is_empty() {
            return Outcome::fail("the note is empty: say what it is and when you would want it");
        }
        if self.undertaking.media(file).is_none() {
            return Outcome::fail(format!(
                "there is no image called `{file}` among those you were shown. \
                 Use the name a result gave it, as in `[image: <name>]`."
            ));
        }
        let count = self.undertaking.update(|state| {
            state.kept.retain(|k| k.file != file);
            state.kept.push(undertaking::Kept {
                file: file.to_owned(),
                note: note.to_owned(),
                at: Utc::now(),
            });
            state.kept.len()
        });
        match count {
            Ok(count) => Outcome::ok(format!("kept ({count} so far)")),
            Err(e) => unwritten(&e),
        }
    }
}

/// Something worth knowing before the work is done, for whoever it is
/// for. Only offered where someone follows the work as it runs and takes
/// what it tells them from the record.
pub struct Tell {
    undertaking: Arc<Undertaking>,
}

impl Tell {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Tell {
    fn name(&self) -> &'static str {
        "tell"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "tell",
            json!({
                "text": {"type": "string"},
                "files": {"type": "array", "items": {"type": "string"}},
            }),
            &["text"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let words = match text(args, "text") {
            Ok(t) => t.trim(),
            Err(refusal) => return refusal,
        };
        if words.is_empty() {
            return Outcome::fail("there is nothing to tell");
        }
        let files: Vec<String> = args["files"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    .filter_map(|f| f.as_str().map(str::trim))
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        match self.undertaking.update(|state| {
            state.told.push(undertaking::Told {
                text: words.to_owned(),
                files,
                at: Utc::now(),
            });
        }) {
            Ok(()) => Outcome::ok("Told. Go on with the work."),
            Err(e) => unwritten(&e),
        }
    }
}

/// The outcome a worker means by `word`, in any of the ways models put it.
pub(crate) fn outcome_of(word: &str) -> Option<undertaking::Outcome> {
    match word.trim().to_lowercase().as_str() {
        "finished" | "done" | "success" | "completed" => Some(undertaking::Outcome::Finished),
        "blocked" => Some(undertaking::Outcome::Blocked),
        "gave_up" | "gave up" | "failed" => Some(undertaking::Outcome::GaveUp),
        _ => None,
    }
}

/// The end of the work. `finished` is not taken while a step of the plan
/// is open: the plan is the standard the work set itself up front, and
/// each step of it is either shown done or dropped for a reason.
pub struct Done {
    undertaking: Arc<Undertaking>,
    words: Shared,
}

impl Done {
    pub fn new(undertaking: Arc<Undertaking>, words: Shared) -> Self {
        Self { undertaking, words }
    }
}

#[async_trait]
impl Tool for Done {
    fn name(&self) -> &'static str {
        "done"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "done",
            json!({
                "outcome": {"type": "string", "enum": ["finished", "blocked", "gave_up"]},
                "summary": {"type": "string"},
                "files": {"type": "array", "items": {"type": "string"}},
                "lesson": {"type": "string"},
            }),
            &["outcome", "summary"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (outcome, summary) = match (text(args, "outcome"), text(args, "summary")) {
            (Ok(o), Ok(s)) => (o, s),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        let Some(outcome) = outcome_of(outcome) else {
            return Outcome::fail(format!(
                "`{}` is not an outcome. Use finished, blocked or gave_up.",
                outcome.trim().to_lowercase()
            ));
        };
        let words = prompts::hold(&self.words).clone();
        let refused = self.undertaking.update(|state| {
            let open = state.open_steps();
            if outcome == undertaking::Outcome::Finished && !open.is_empty() {
                let steps: Vec<String> = open
                    .iter()
                    .map(|&n| state.step_text(&words, n - 1))
                    .collect();
                return Some(words.fill("steps_open", &[("steps", steps.join("\n"))]));
            }
            state.finish = Some(Finish {
                outcome,
                summary: summary.trim().to_owned(),
                files: args["files"]
                    .as_array()
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(|f| f.as_str().map(str::trim))
                            .filter(|f| !f.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                lesson: args["lesson"]
                    .as_str()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned),
                at: Utc::now(),
            });
            None
        });
        match refused {
            Ok(Some(open)) => Outcome::fail(open),
            Ok(None) => Outcome::ok("recorded"),
            Err(e) => unwritten(&e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undertaking() -> (tempfile::TempDir, Arc<Undertaking>) {
        let dir = tempfile::tempdir().unwrap();
        let u = Arc::new(
            Undertaking::open(
                &dir.path().join(crate::WORK_DB),
                "main",
                Some("make a.txt say hi"),
            )
            .unwrap(),
        );
        (dir, u)
    }

    fn words() -> Shared {
        crate::Prompts::shipped().shared()
    }

    #[tokio::test]
    async fn a_plan_is_kept_whole() {
        let (_dir, u) = undertaking();
        let out = Plan::new(u.clone(), words())
            .call(&json!({"steps": [
                {"text": "look", "status": "completed"},
                {"text": "write the file", "status": "in_progress"},
                "check it",
            ]}))
            .await;
        assert!(
            out.output
                .ends_with("[x] 1. look\n[>] 2. write the file\n[ ] 3. check it"),
            "{}",
            out.output
        );
        assert_eq!(u.state().plan.len(), 3);
    }

    #[tokio::test]
    async fn a_step_with_a_check_needs_a_result_to_be_marked_done() {
        let (_dir, u) = undertaking();
        let plan = Plan::new(u.clone(), words());
        let out = plan
            .call(&json!({"steps": [
                {"text": "write a.txt", "status": "done", "check": "cat a.txt prints hi"},
                {"text": "tidy up", "status": "done"},
            ]}))
            .await;
        assert!(out.failed);
        assert_eq!(
            out.output,
            "The plan was not changed:\n- Step 1 has a check, so marking it done needs a `result`: \
             what you saw that shows the check was met."
        );
        assert!(u.state().plan.is_empty(), "a refused plan was kept");

        let out = plan
            .call(&json!({"steps": [
                {"text": "write a.txt", "status": "done", "check": "cat a.txt prints hi",
                 "result": "cat a.txt printed hi"},
                {"text": "tidy up", "status": "done"},
            ]}))
            .await;
        assert!(!out.failed, "{}", out.output);
        let state = u.state();
        assert_eq!(
            state.plan[0].result.as_deref(),
            Some("cat a.txt printed hi")
        );
        // A step without a check is done on its own word.
        assert_eq!(state.plan[1].status, Progress::Done);
    }

    #[tokio::test]
    async fn a_step_cannot_start_before_the_steps_it_is_after_are_done() {
        let (_dir, u) = undertaking();
        let plan = Plan::new(u.clone(), words());
        let out = plan
            .call(&json!({"steps": [
                {"text": "build", "status": "doing"},
                {"text": "test", "status": "todo"},
                {"text": "ship", "status": "doing", "after": [1, "step 2"]},
            ]}))
            .await;
        assert!(out.failed);
        assert_eq!(
            out.output,
            "The plan was not changed:\n- Step 3 cannot be in progress yet: \
             it is after steps that are not done (1, 2)."
        );

        // Dropped is not done: what depends on a dropped step waits until
        // it no longer names it.
        plan.call(&json!({"steps": [
            {"text": "build", "status": "done"},
            {"text": "test", "status": "dropped", "result": "there are no tests"},
            {"text": "ship", "status": "todo", "after": [1, 2]},
        ]}))
        .await;
        let out = plan
            .call(&json!({"steps": [
                {"text": "build", "status": "done"},
                {"text": "test", "status": "dropped"},
                {"text": "ship", "status": "doing"},
            ]}))
            .await;
        assert!(out.output.contains("(2)"), "{}", out.output);
        let out = plan
            .call(&json!({"steps": [
                {"text": "build", "status": "done"},
                {"text": "test", "status": "dropped"},
                {"text": "ship", "status": "doing", "after": [1]},
            ]}))
            .await;
        assert!(!out.failed, "{}", out.output);
    }

    #[tokio::test]
    async fn a_step_said_again_without_its_check_keeps_it_and_what_it_is_after_follows_it() {
        let (_dir, u) = undertaking();
        let plan = Plan::new(u.clone(), words());
        plan.call(&json!({"steps": [
            {"text": "build", "status": "todo", "check": "make prints ok"},
            {"text": "ship", "status": "todo", "after": [1]},
        ]}))
        .await;
        // Only statuses, and a step put in front.
        let out = plan
            .call(&json!({"steps": [
                "look around",
                {"text": "build", "status": "doing"},
                {"text": "ship", "status": "todo"},
            ]}))
            .await;
        assert!(!out.failed, "{}", out.output);
        let state = u.state();
        assert_eq!(state.plan[1].check.as_deref(), Some("make prints ok"));
        assert_eq!(state.plan[2].after, [2]);
        // Said again as empty, a check is gone.
        plan.call(&json!({"steps": [
            "look around",
            {"text": "build", "status": "doing", "check": ""},
            {"text": "ship", "status": "todo"},
        ]}))
        .await;
        assert_eq!(u.state().plan[1].check, None);
    }

    #[tokio::test]
    async fn the_plan_shows_what_each_step_is_after_its_check_and_its_result() {
        let (_dir, u) = undertaking();
        let out = Plan::new(u.clone(), words())
            .call(&json!({"steps": [
                {"text": "write a.txt", "status": "done", "check": "cat a.txt prints hi",
                 "result": "it printed hi"},
                {"text": "survey the mirrors", "status": "dropped", "result": "one is enough"},
                {"text": "copy it to b.txt", "status": "doing", "after": [1],
                 "check": "cmp a.txt b.txt is silent"},
                {"text": "say so", "status": "todo", "after": [3, 1]},
            ]}))
            .await;
        assert!(!out.failed, "{}", out.output);
        assert_eq!(
            out.output,
            "The plan is now:\n\
             [x] 1. write a.txt\n      check: cat a.txt prints hi\n      result: it printed hi\n\
             [-] 2. survey the mirrors\n      result: one is enough\n\
             [>] 3. copy it to b.txt (after 1)\n      check: cmp a.txt b.txt is silent\n\
             [ ] 4. say so (after 1, 3)"
        );
    }

    #[tokio::test]
    async fn finishing_is_refused_while_steps_are_open_and_taken_once_they_are_done_or_dropped() {
        let (_dir, u) = undertaking();
        let (plan, done) = (Plan::new(u.clone(), words()), Done::new(u.clone(), words()));
        plan.call(&json!({"steps": [
            {"text": "write a.txt", "status": "done"},
            {"text": "check it", "status": "doing", "check": "cat a.txt prints hi"},
            {"text": "mirror it", "status": "todo"},
        ]}))
        .await;
        let finished = json!({"outcome": "finished", "summary": "wrote it"});
        let refused = done.call(&finished).await;
        assert!(refused.failed);
        assert_eq!(
            refused.output,
            "Not recorded. These steps of the plan are neither done nor dropped:\n\
             [>] 2. check it\n      check: cat a.txt prints hi\n\
             [ ] 3. mirror it"
        );
        assert!(u.state().finish.is_none());

        plan.call(&json!({"steps": [
            {"text": "write a.txt", "status": "done"},
            {"text": "check it", "status": "done", "result": "it printed hi"},
            {"text": "mirror it", "status": "dropped", "result": "nobody asked for it"},
        ]}))
        .await;
        assert_eq!(done.call(&finished).await.output, "recorded");
        assert_eq!(
            u.state().finish.unwrap().outcome,
            undertaking::Outcome::Finished
        );
    }

    #[tokio::test]
    async fn work_with_open_steps_can_still_end_blocked() {
        let (_dir, u) = undertaking();
        Plan::new(u.clone(), words())
            .call(&json!({"steps": ["write a.txt"]}))
            .await;
        let out = Done::new(u.clone(), words())
            .call(&json!({"outcome": "blocked", "summary": "the disk is read-only"}))
            .await;
        assert_eq!(out.output, "recorded");
    }

    #[tokio::test]
    async fn an_image_the_worker_was_shown_is_kept_with_its_note_and_kept_again_replaces_it() {
        let (_dir, u) = undertaking();
        let shown = u.keep(&[ghost_brain::Picture {
            mime: "image/png".into(),
            bytes: crate::tools::testing::PIXEL.to_vec(),
        }]);
        let file = &shown[0];
        let keep = Keep::new(u.clone());
        let out = keep
            .call(&json!({"file": format!("[image: {file}]"), "note": "one clear pixel"}))
            .await;
        assert!(!out.failed, "{}", out.output);
        let out = keep
            .call(&json!({"file": file, "note": "a pixel, for testing"}))
            .await;
        assert!(!out.failed, "{}", out.output);
        let kept = u.state().kept;
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].file.as_str(), kept[0].note.as_str()),
            (file.as_str(), "a pixel, for testing")
        );
    }

    #[tokio::test]
    async fn a_file_the_worker_never_stored_cannot_be_kept() {
        let (dir, u) = undertaking();
        // A file beside the database, and one that is nowhere at all.
        std::fs::write(dir.path().join("secret.txt"), b"x").unwrap();
        let keep = Keep::new(u.clone());
        for file in ["../secret.txt", "secret.txt", "nothing.png", "/etc/hosts"] {
            let out = keep.call(&json!({"file": file, "note": "mine"})).await;
            assert!(out.failed, "{file} was kept");
        }
        assert!(u.state().kept.is_empty());
    }

    #[tokio::test]
    async fn what_the_worker_tells_is_appended_to_the_record_in_order() {
        let (_dir, u) = undertaking();
        let tell = Tell::new(u.clone());
        let said = tell
            .call(&json!({"text": "  two sources agree  ", "files": [" /work/a.md ", ""]}))
            .await;
        assert!(!said.failed, "{}", said.output);
        tell.call(&json!({"text": "the third disagrees"})).await;
        assert!(tell.call(&json!({"text": "  "})).await.failed);
        let told: Vec<(String, Vec<String>)> = u
            .state()
            .told
            .into_iter()
            .map(|t| (t.text, t.files))
            .collect();
        assert_eq!(
            told,
            [
                (
                    "two sources agree".to_owned(),
                    vec!["/work/a.md".to_owned()]
                ),
                ("the third disagrees".to_owned(), vec![]),
            ]
        );
    }

    #[tokio::test]
    async fn finishing_is_recorded_as_it_is_reported() {
        let (_dir, u) = undertaking();
        let done = Done::new(u.clone(), words());
        let said = done
            .call(&json!({
                "outcome": "finished", "summary": "wrote it",
                "files": ["/work/a.txt"], "lesson": "print the file before saying so",
            }))
            .await;
        assert_eq!(said.output, "recorded");
        let finish = u.state().finish.unwrap();
        assert_eq!(finish.outcome, undertaking::Outcome::Finished);
        assert_eq!(finish.files, ["/work/a.txt"]);
        assert_eq!(
            finish.lesson.as_deref(),
            Some("print the file before saying so")
        );

        let (_dir, other) = undertaking();
        Done::new(other.clone(), words())
            .call(&json!({"outcome": "blocked", "summary": "no network"}))
            .await;
        assert_eq!(
            other.state().finish.unwrap().outcome,
            undertaking::Outcome::Blocked
        );
    }
}
