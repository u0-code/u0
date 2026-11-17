//! What an undertaking's record holds: the brief, the plan, the notes, how
//! it ended, and what it kept and told along the way, with the ways the
//! worker and others are shown it.
//!
//! This is written to the database as JSON, so it is a format that has to
//! go on loading: fields are added, optional, and never taken away.

use chrono::{DateTime, Utc};
use ghost_brain::Usage;
use serde::{Deserialize, Serialize};

use crate::prompts::Prompts;

/// What a question put to whoever the work is for asks of them, so that
/// whoever shows it to them can show it as what it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    /// Something to say: a fact, a preference, a yes or no.
    #[default]
    Answer,
    /// A file only they have.
    File,
    /// Something only they can do, such as signing in or approving a
    /// payment, after which they say it is done.
    Act,
    /// One of the options it gives.
    Choose,
}

impl AskKind {
    /// The kind a worker names, by its word.
    pub fn from_word(word: &str) -> Option<Self> {
        match word.trim() {
            "answer" => Some(Self::Answer),
            "file" => Some(Self::File),
            "act" => Some(Self::Act),
            "choose" => Some(Self::Choose),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    #[default]
    Todo,
    Doing,
    Done,
    /// No longer needed, for the reason in the step's result.
    Dropped,
    /// Tried, and it did not come off: the part given it ended without
    /// finishing. Still to be done another way, or dropped.
    Failed,
}

impl Progress {
    /// Whether the step is out of the way: done, or dropped for a reason.
    /// Anything else is still open, and the work is not finished while it
    /// is.
    pub fn closed(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

/// One step of the plan. The plan is a small graph: a step can be after
/// others, and carries how it will be known to be done and, once it is,
/// how that was shown. Everything but its text and status came later and
/// is optional, so that plans saved before still load.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub status: Progress,
    /// How it will be known to be done: a command and what it prints, a
    /// file that exists, a page that says something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The steps, numbered from 1, that have to be done before it starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<usize>,
    /// The part handed this step, by its number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u64>,
    /// How its check was met, why it was dropped, or how the part given it
    /// ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: DateTime<Utc>,
    pub text: String,
}

/// How an undertaking ended, in the worker's own judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Done, and checked.
    Finished,
    /// Something outside the worker's control is in the way.
    Blocked,
    /// Out of ideas.
    GaveUp,
    /// Ended from outside before it was done: whoever it is for no longer
    /// wants it. What it kept and told stays theirs.
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finish {
    pub outcome: Outcome,
    pub summary: String,
    /// The files that are the result, where the result is files: paths as
    /// the machine knows them.
    #[serde(default)]
    pub files: Vec<String>,
    /// What doing this taught about doing things like it, if anything.
    #[serde(default)]
    pub lesson: Option<String>,
    pub at: DateTime<Utc>,
}

/// An image the worker was shown and kept, to be had again after the work
/// is over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// Its name under `media/`, beside the database.
    pub file: String,
    /// What it is, and when it would be wanted again.
    pub note: String,
    pub at: DateTime<Utc>,
}

/// Something the worker told whoever the work is for while it ran: worth
/// their knowing before the work is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Told {
    pub text: String,
    /// Files it made that are ready to hand over: paths as the machine
    /// knows them.
    #[serde(default)]
    pub files: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// What was asked, as it was asked.
    pub brief: String,
    pub plan: Vec<PlanStep>,
    pub notes: Vec<Note>,
    /// What the worker wrote for itself the last time its context was
    /// cleared. One document, replaced each time.
    #[serde(default)]
    pub handoff: Option<String>,
    pub finish: Option<Finish>,
    /// Files written or edited most recently, newest last.
    #[serde(default)]
    pub touched: Vec<String>,
    /// Things added to the task after it began, in order. The brief itself
    /// never changes; these extend it.
    #[serde(default)]
    pub additions: Vec<String>,
    /// The session of a command-line agent that has been stepping this
    /// work, so its next sitting can pick its own context back up.
    #[serde(default)]
    pub session: Option<String>,
    /// The installed agent this part was handed to, by name, when it is
    /// not done by a worker of ours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// A question the worker put to whoever the work is for. The work
    /// waits here until something is added to the task from outside.
    #[serde(default)]
    pub waiting: Option<String>,
    /// What kind of answer the question in `waiting` asks for, when the
    /// worker said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_kind: Option<AskKind>,
    /// For a question that asks them to choose, what to choose from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_options: Vec<String>,
    /// The model once refused a context of about this many tokens. The
    /// transcript is kept well under it from then on.
    #[serde(default)]
    pub refused_at: Option<u64>,
    /// What the worker kept, in the order it was kept. Keeping a file again
    /// replaces its note and moves it to the end.
    #[serde(default)]
    pub kept: Vec<Kept>,
    /// What the worker told whoever the work is for, in the order it told
    /// them. Only ever appended to.
    #[serde(default)]
    pub told: Vec<Told>,
    /// The undertaking this is a part of, for a part handed out by a
    /// worker. Helpers from before parts ran on their own have none, and
    /// are not counted as parts: each of them ended in the step that
    /// started it.
    #[serde(default)]
    pub parent: Option<String>,
    /// How many times work was picked up with a fresh context.
    pub sittings: u32,
    pub steps: u64,
    pub usage: Usage,
    pub seconds: f64,
}

impl State {
    /// Put `question` to whoever the work is for, and wait.
    pub fn ask(&mut self, question: &str, kind: Option<AskKind>, options: Vec<String>) {
        self.waiting = Some(question.to_owned());
        self.waiting_kind = kind;
        self.waiting_options = options;
    }

    /// Whatever it was waiting to hear, it has heard.
    pub fn answered(&mut self) {
        self.waiting = None;
        self.waiting_kind = None;
        self.waiting_options.clear();
    }

    /// What was asked, with whatever was added to it since.
    pub fn task_text(&self) -> String {
        let mut text = self.brief.trim().to_owned();
        for added in &self.additions {
            text.push_str(&format!("\n\nAdded later: {}", added.trim()));
        }
        text
    }

    /// The plan as the worker should see it, in `words`: each step with
    /// what it is after, the part on it, its check and its result, so that
    /// a sitting picked up later knows what has been shown and what has
    /// not.
    pub fn plan_text(&self, words: &Prompts) -> String {
        if self.plan.is_empty() {
            return words.text("no_plan").to_owned();
        }
        (0..self.plan.len())
            .map(|i| self.step_text(words, i))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Step `i` of the plan, counted from 0, as the worker is shown it.
    pub fn step_text(&self, words: &Prompts, i: usize) -> String {
        let step = &self.plan[i];
        let mark = match step.status {
            Progress::Todo => "[ ]",
            Progress::Doing => "[>]",
            Progress::Done => "[x]",
            Progress::Dropped => "[-]",
            Progress::Failed => "[!]",
        };
        let part = |key: &str, name: &str, value: Option<String>| {
            value.map_or_else(String::new, |v| words.fill(key, &[(name, v)]))
        };
        let after = (!step.after.is_empty()).then(|| {
            step.after
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        });
        words.fill(
            "plan_step",
            &[
                ("mark", mark.to_owned()),
                ("step", (i + 1).to_string()),
                ("text", step.text.clone()),
                ("after", part("plan_after", "steps", after)),
                (
                    "part",
                    part("plan_part", "part", step.part.map(|p| p.to_string())),
                ),
                ("check", part("plan_check", "check", step.check.clone())),
                ("result", part("plan_result", "result", step.result.clone())),
            ],
        )
    }

    /// The numbers of the steps, counted from 1, that are neither done nor
    /// dropped.
    pub fn open_steps(&self) -> Vec<usize> {
        (0..self.plan.len())
            .filter(|&i| !self.plan[i].status.closed())
            .map(|i| i + 1)
            .collect()
    }

    /// The steps that step `i`, counted from 0, is after and that are not
    /// done: while there are any, it cannot be started.
    pub fn waiting_on(&self, i: usize) -> Vec<usize> {
        self.plan[i]
            .after
            .iter()
            .copied()
            .filter(|&n| {
                n.checked_sub(1)
                    .and_then(|at| self.plan.get(at))
                    .is_some_and(|s| s.status != Progress::Done)
            })
            .collect()
    }

    /// Where the work stands, in a sentence or two: for whoever asks how it
    /// is going between sittings.
    pub fn progress(&self) -> String {
        let done = self
            .plan
            .iter()
            .filter(|s| s.status == Progress::Done)
            .count();
        let now = self
            .plan
            .iter()
            .find(|s| s.status == Progress::Doing)
            .or_else(|| self.plan.iter().find(|s| s.status == Progress::Todo));
        let mut out = String::new();
        if !self.plan.is_empty() {
            out = format!("{done} of {} steps done", self.plan.len());
            if let Some(step) = now {
                out.push_str(&format!("; next: {}", step.text));
            }
            out.push('.');
        }
        if let Some(note) = self.notes.last() {
            if !out.is_empty() {
                out.push(' ');
            }
            let text: String = note.text.chars().take(240).collect();
            out.push_str(&format!("Latest note: {text}"));
        }
        if out.is_empty() {
            "Started; nothing written down yet.".to_owned()
        } else {
            out
        }
    }

    /// The latest thing it has to say for itself: its report if it has
    /// ended, the question it is waiting on, what it last told, or where it
    /// stands.
    pub fn latest(&self) -> String {
        if let Some(finish) = self
            .finish
            .as_ref()
            .filter(|f| !f.summary.trim().is_empty())
        {
            return finish.summary.trim().to_owned();
        }
        if let Some(question) = &self.waiting {
            return question.trim().to_owned();
        }
        match self.told.last() {
            Some(told) => told.text.trim().to_owned(),
            None => self.progress(),
        }
    }

    /// The newest notes that fit in `budget` characters, oldest first.
    pub fn notes_text(&self, budget: usize) -> String {
        if self.notes.is_empty() {
            return "(no notes yet)".to_owned();
        }
        let mut kept = Vec::new();
        let mut used = 0;
        for note in self.notes.iter().rev() {
            let line = format!("- {}", note.text.trim());
            if !kept.is_empty() && used + line.len() > budget {
                break;
            }
            used += line.len();
            kept.push(line);
        }
        let left_out = self.notes.len() - kept.len();
        kept.reverse();
        if left_out > 0 {
            kept.insert(0, format!("({left_out} older notes are not shown)"));
        }
        kept.join("\n")
    }
}
