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
    tool::{Outcome, Tool, spec, text},
    undertaking::{self, Finish, PlanStep, Progress, Undertaking},
};

pub struct Plan {
    undertaking: Arc<Undertaking>,
}

impl Plan {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

/// Models name the same four states many ways.
fn progress(word: &str) -> Progress {
    match word.trim().to_lowercase().as_str() {
        "doing" | "in_progress" | "in progress" | "active" | "current" => Progress::Doing,
        "done" | "completed" | "complete" | "finished" => Progress::Done,
        "dropped" | "cancelled" | "canceled" | "skipped" => Progress::Dropped,
        _ => Progress::Todo,
    }
}

#[async_trait]
impl Tool for Plan {
    fn spec(&self) -> ToolSpec {
        spec(
            "plan",
            "Write or rewrite the plan: the whole list of steps, each with its status. Use it at the start of anything that takes more than a few steps, and again whenever a step is finished or the plan changes. The plan survives when older context is dropped.",
            json!({
                "steps": {
                    "type": "array",
                    "description": "Every step of the plan, in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string", "description": "What the step is, concretely."},
                            "status": {"type": "string", "enum": ["todo", "doing", "done", "dropped"]},
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
        let plan: Vec<PlanStep> = steps
            .iter()
            .filter_map(|step| {
                // A bare string is a step that has not been started.
                let (words, status) = match step {
                    Value::String(words) => (words.as_str(), Progress::Todo),
                    other => (
                        other["text"].as_str()?,
                        progress(other["status"].as_str().unwrap_or("todo")),
                    ),
                };
                let words = words.trim();
                (!words.is_empty()).then(|| PlanStep {
                    text: words.to_owned(),
                    status,
                })
            })
            .collect();
        if plan.is_empty() {
            return Outcome::fail("the plan has no steps");
        }
        let shown = self.undertaking.update(|state| {
            state.plan = plan;
            state.plan_text()
        });
        Outcome::ok(format!("The plan is now:\n{shown}"))
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
    fn spec(&self) -> ToolSpec {
        spec(
            "note",
            "Write down one thing you would not want to find out twice: a path, a version, a command that worked, a decision and its reason, a dead end. A line or two. Notes survive when older context is dropped; tool output does not.",
            json!({
                "text": {"type": "string", "description": "The fact, stated so it makes sense on its own later."},
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
            return Outcome::fail("the note is empty");
        }
        let count = self.undertaking.update(|state| {
            state.notes.push(undertaking::Note {
                at: Utc::now(),
                text: words.to_owned(),
            });
            state.notes.len()
        });
        Outcome::ok(format!("noted ({count} so far)"))
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
    fn spec(&self) -> ToolSpec {
        spec(
            "handoff",
            "Only when you are told your context is about to be cleared: write the one document you will continue from afterwards. It replaces any earlier handoff, so it has to stand on its own. For things worth keeping at any other time, use `note`.",
            json!({
                "text": {"type": "string", "description": "What is finished and checked, what is in progress and exactly where it stopped, the paths and commands that matter, what was not obvious, and the next actions in order."},
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
            return Outcome::fail("the handoff is empty");
        }
        self.undertaking
            .update(|state| state.handoff = Some(words.to_owned()));
        Outcome::ok("kept")
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
    fn spec(&self) -> ToolSpec {
        spec(
            "ask",
            "Put a question to the person this work is for, and stop until they answer. \
             Only for what you cannot find out and should not decide alone: a preference, a fact only they have, \
             a choice between ways that would give them a different result. Never for what you could look up or try. \
             The answer may take hours, so do everything that does not depend on it first, and ask once, with \
             everything you need in the one question.",
            json!({
                "question": {"type": "string", "description": "The question, as you would put it to them: short, and answerable without having seen your work."},
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
        self.undertaking
            .update(|state| state.waiting = Some(question.to_owned()));
        Outcome::ok(
            "Asked. The work stops here until they answer; their answer will be added to the task.",
        )
    }
}

pub struct Done {
    undertaking: Arc<Undertaking>,
}

impl Done {
    pub fn new(undertaking: Arc<Undertaking>) -> Self {
        Self { undertaking }
    }
}

#[async_trait]
impl Tool for Done {
    fn spec(&self) -> ToolSpec {
        spec(
            "done",
            "End the work. `finished` means every requirement has been checked against the real state, not just attempted. `blocked` means something outside your control is in the way. `gave_up` means you are out of ideas. Nothing can be done after this.",
            json!({
                "outcome": {"type": "string", "enum": ["finished", "blocked", "gave_up"]},
                "summary": {"type": "string", "description": "What was done, or what is in the way, in a few sentences."},
                "evidence": {"type": "string", "description": "For `finished`: each requirement and how you checked it holds (the command you ran and what it showed)."},
                "files": {"type": "array", "items": {"type": "string"}, "description": "If what was asked for is one or more files, their absolute paths: these are what gets handed over. Leave out when the result is what you say in the summary."},
                "lesson": {"type": "string", "description": "Optional, and usually left out. One sentence you would want in front of you the next time you do work like this: what you would do differently, or a way of going about it that worked and was not obvious. About how to work, not about this task's subject."},
            }),
            &["outcome", "summary"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (outcome, summary) = match (text(args, "outcome"), text(args, "summary")) {
            (Ok(o), Ok(s)) => (o, s),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        let outcome = match outcome.trim().to_lowercase().as_str() {
            "finished" | "done" | "success" | "completed" => undertaking::Outcome::Finished,
            "blocked" => undertaking::Outcome::Blocked,
            "gave_up" | "gave up" | "failed" => undertaking::Outcome::GaveUp,
            other => {
                return Outcome::fail(format!(
                    "`{other}` is not an outcome. Use finished, blocked or gave_up."
                ));
            }
        };
        let evidence = args["evidence"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let checked = self.undertaking.state().checked;
        if outcome == undertaking::Outcome::Finished && checked && evidence.chars().count() < 20 {
            return Outcome::fail(
                "`finished` needs evidence: for each requirement, the command you ran just now and what it showed.",
            );
        }

        // The commonest way to fail is to stop one check too early. The
        // first claim of success is answered with the task itself.
        if outcome == undertaking::Outcome::Finished {
            let brief = self.undertaking.update(|state| {
                (!std::mem::replace(&mut state.checked, true)).then(|| state.task_text())
            });
            if let Some(brief) = brief {
                return Outcome::ok(format!(
                    "Not recorded yet. Before this is final, check your work against the task as it was given:\n\n\
                     <task>\n{brief}\n</task>\n\n\
                     Go through it requirement by requirement. For each one, look at the real state now \
                     (run the command, read the file, call the program) instead of relying on what you remember doing. \
                     Check exact names, paths and formats, and that nothing the task did not ask for was left behind or broken. \
                     Fix whatever is off. Then call `done` again."
                ));
            }
        }
        self.undertaking.update(|state| {
            state.finish = Some(Finish {
                outcome,
                summary: summary.trim().to_owned(),
                evidence,
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
        });
        Outcome::ok("recorded")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undertaking() -> (tempfile::TempDir, Arc<Undertaking>) {
        let dir = tempfile::tempdir().unwrap();
        let u = Arc::new(Undertaking::open(dir.path(), Some("make a.txt say hi")).unwrap());
        (dir, u)
    }

    #[tokio::test]
    async fn a_plan_is_kept_whole() {
        let (_dir, u) = undertaking();
        let out = Plan::new(u.clone())
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
    async fn finishing_is_asked_twice_and_giving_up_once() {
        let (_dir, u) = undertaking();
        let done = Done::new(u.clone());
        let first = done
            .call(&json!({"outcome": "finished", "summary": "wrote it"}))
            .await;
        assert!(
            first.output.contains("make a.txt say hi"),
            "the task comes back"
        );
        assert!(u.state().finish.is_none());
        let second = done
            .call(&json!({"outcome": "finished", "summary": "wrote it", "evidence": "ran `cat a.txt` just now, it printed hi"}))
            .await;
        assert_eq!(second.output, "recorded");
        assert_eq!(
            u.state().finish.unwrap().outcome,
            undertaking::Outcome::Finished
        );

        let (_dir, other) = undertaking();
        Done::new(other.clone())
            .call(&json!({"outcome": "blocked", "summary": "no network"}))
            .await;
        assert_eq!(
            other.state().finish.unwrap().outcome,
            undertaking::Outcome::Blocked
        );
    }
}
