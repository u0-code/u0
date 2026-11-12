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
            "Write down something you would not want to find out twice: a path, a version, a command that worked, a decision and its reason, a dead end. Notes survive when older context is dropped; tool output does not.",
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

        // The commonest way to fail is to stop one check too early. The
        // first claim of success is answered with the task itself.
        if outcome == undertaking::Outcome::Finished {
            let brief = self.undertaking.update(|state| {
                (!std::mem::replace(&mut state.checked, true)).then(|| state.brief.clone())
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
            .call(&json!({"outcome": "finished", "summary": "wrote it", "evidence": "cat a.txt -> hi"}))
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
