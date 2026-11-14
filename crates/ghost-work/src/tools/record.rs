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
    prompts::{self, Loop, Store},
    tool::{Outcome, Tool, text},
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
    fn name(&self) -> &'static str {
        "ask"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "ask",
            json!({"question": {"type": "string"}}),
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
        Outcome::ok(format!("kept ({count} so far)"))
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
        self.undertaking.update(|state| {
            state.told.push(undertaking::Told {
                text: words.to_owned(),
                files,
                at: Utc::now(),
            });
        });
        Outcome::ok("Told. Go on with the work.")
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
        self.undertaking.update(|state| {
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
        });
        Outcome::ok("recorded")
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
        let done = Done::new(u.clone());
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
        Done::new(other.clone())
            .call(&json!({"outcome": "blocked", "summary": "no network"}))
            .await;
        assert_eq!(
            other.state().finish.unwrap().outcome,
            undertaking::Outcome::Blocked
        );
    }
}
