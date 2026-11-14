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
    fn spec(&self) -> ToolSpec {
        spec(
            "keep",
            "Keep an image you were shown, because you want to have it: something worth looking at or \
             using, now or later. It is had at once, while you go on working, and stays after the work is \
             over. What you do not keep is gone with the work. Keeping the same image again replaces the note.",
            json!({
                "file": {"type": "string", "description": "The image's name, as a result showed it in `[image: <name>]`."},
                "note": {"type": "string", "description": "In your own words: what it is, and when you would want it again. This is how it will be found later."},
            }),
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
//
// What this tool is said to be is still a literal here, like every other
// tool description in ghost-work, until ghost-work's prose moves to files
// of its own as ghost-mind's has.
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
    fn spec(&self) -> ToolSpec {
        spec(
            "tell",
            "Tell whoever this work is for something worth their knowing now, before the work is done: a \
             finding they would want straight away, a partial result, a file ready to hand over. It reaches \
             them while you go on working; it asks nothing and ends nothing. Most steps need none: the whole \
             account belongs in `done`, and what is only on the way to it belongs in `note`. For an image you \
             were shown, use `keep`.",
            json!({
                "text": {"type": "string", "description": "What they should know, in a sentence or two that make sense without your work in front of them."},
                "files": {"type": "array", "items": {"type": "string"}, "description": "Files you made that are ready to hand over now: their absolute paths. Leave out when there are none."},
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
    fn spec(&self) -> ToolSpec {
        spec(
            "done",
            "End the work and say how it ended. `finished` is your word that every requirement holds and that you have seen it hold in the real state; nothing checks it after you. `blocked` means something outside your control is in the way. `gave_up` means you are out of ideas. Nothing can be done after this.",
            json!({
                "outcome": {"type": "string", "enum": ["finished", "blocked", "gave_up"]},
                "summary": {"type": "string", "description": "What was done, or what is in the way, in a few sentences."},
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
