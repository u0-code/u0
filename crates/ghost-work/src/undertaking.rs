//! One thing being done, and everything about it that has to outlast a
//! context window.
//!
//! A transcript is short-lived: it fills up, it is folded away, the process
//! running it can stop at any moment. An undertaking is what remains: what
//! was asked, the plan as it stands, the notes written along the way, and how
//! it ended. It is a directory, and any sitting can be picked up from it.
//!
//! ```text
//! <dir>/state.json        brief, plan, notes, outcome, running totals
//! <dir>/transcript.json   the current sitting, saved after every step
//! <dir>/log.jsonl         one line per step: what was called, what it cost
//! ```

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use ghost_brain::{Item, Usage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Todo,
    Doing,
    Done,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub status: Progress,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finish {
    pub outcome: Outcome,
    pub summary: String,
    /// How each requirement was checked.
    pub evidence: String,
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
    /// The worker has been made to check its work once already.
    pub checked: bool,
    /// A reviewer has looked at a claim of being finished once already.
    #[serde(default)]
    pub audited: bool,
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
    /// How many times work was picked up with a fresh context.
    pub sittings: u32,
    pub steps: u64,
    pub usage: Usage,
    pub seconds: f64,
}

impl State {
    /// What was asked, with whatever was added to it since.
    pub fn task_text(&self) -> String {
        let mut text = self.brief.trim().to_owned();
        for added in &self.additions {
            text.push_str(&format!("\n\nAdded later: {}", added.trim()));
        }
        text
    }

    /// The plan as the worker should see it.
    pub fn plan_text(&self) -> String {
        if self.plan.is_empty() {
            return "(no plan written yet)".to_owned();
        }
        self.plan
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let mark = match step.status {
                    Progress::Todo => "[ ]",
                    Progress::Doing => "[>]",
                    Progress::Done => "[x]",
                    Progress::Dropped => "[-]",
                };
                format!("{mark} {}. {}", i + 1, step.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
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

pub struct Undertaking {
    dir: PathBuf,
    state: Mutex<State>,
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

impl Undertaking {
    /// Start something new in `dir`, or pick up what is already there. A
    /// brief is only taken the first time: what was asked does not change.
    pub fn open(dir: impl Into<PathBuf>, brief: Option<&str>) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        let state = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let brief = brief
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "a new undertaking needs a brief",
                        )
                    })?;
                State {
                    brief: brief.to_owned(),
                    ..State::default()
                }
            }
            Err(e) => return Err(e),
        };
        let this = Self {
            dir,
            state: Mutex::new(state),
        };
        this.save()?;
        Ok(this)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn state(&self) -> State {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&*self.lock()).map_err(io::Error::other)?;
        write_atomically(&self.dir.join("state.json"), &bytes)
    }

    /// Change the state and write it down. A failed write is logged, not
    /// fatal: the work in hand matters more than the record of it.
    pub fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> T {
        let out = change(&mut self.lock());
        if let Err(e) = self.save() {
            tracing::warn!("could not save {}: {e}", self.dir.display());
        }
        out
    }

    /// The sitting in progress, if one was left unfinished.
    pub fn transcript(&self) -> Option<Vec<Item>> {
        let bytes = fs::read(self.dir.join("transcript.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save_transcript(&self, items: &[Item]) {
        let saved = serde_json::to_vec(items)
            .map_err(io::Error::other)
            .and_then(|bytes| write_atomically(&self.dir.join("transcript.json"), &bytes));
        if let Err(e) = saved {
            tracing::warn!("could not save the transcript: {e}");
        }
    }

    /// Add something to the task from outside, while it may be under way.
    /// It goes into a file beside the state, because whoever is working on
    /// this may be another process; the worker picks it up before its next
    /// step.
    pub fn add(&self, text: &str) -> io::Result<()> {
        use std::io::Write as _;
        let mut inbox = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("inbox.jsonl"))?;
        writeln!(
            inbox,
            "{}",
            serde_json::Value::String(text.trim().to_owned())
        )
    }

    /// Take what has been added since the last look, and make it part of
    /// the task.
    pub fn take_added(&self) -> Vec<String> {
        let path = self.dir.join("inbox.jsonl");
        let taken = path.with_extension("taken");
        // Renamed first, so that something added during the read is not lost.
        if fs::rename(&path, &taken).is_err() {
            return Vec::new();
        }
        let added: Vec<String> = fs::read_to_string(&taken)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<String>(line).ok())
            .filter(|text| !text.is_empty())
            .collect();
        let _ = fs::remove_file(&taken);
        if !added.is_empty() {
            self.update(|state| state.additions.extend(added.iter().cloned()));
        }
        added
    }

    /// Append one line to the step log.
    pub fn log(&self, line: &serde_json::Value) {
        use std::io::Write as _;
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("log.jsonl"))
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = written {
            tracing::warn!("could not write the step log: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_written_is_there_when_picked_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let first = Undertaking::open(dir.path(), Some("build the thing")).unwrap();
        first.update(|s| {
            s.plan.push(PlanStep {
                text: "look around".into(),
                status: Progress::Done,
            });
            s.notes.push(Note {
                at: Utc::now(),
                text: "python is 3.12".into(),
            });
        });
        first.save_transcript(&[Item::user("build the thing")]);
        drop(first);

        let again = Undertaking::open(dir.path(), Some("something else")).unwrap();
        let state = again.state();
        assert_eq!(state.brief, "build the thing", "the brief does not change");
        assert_eq!(state.plan_text(), "[x] 1. look around");
        assert_eq!(state.notes_text(1000), "- python is 3.12");
        assert_eq!(again.transcript().unwrap().len(), 1);
    }

    #[test]
    fn only_the_newest_notes_are_shown_when_there_are_many() {
        let mut state = State::default();
        for i in 0..10 {
            state.notes.push(Note {
                at: Utc::now(),
                text: format!("fact number {i}"),
            });
        }
        let shown = state.notes_text(40);
        assert_eq!(
            shown,
            "(8 older notes are not shown)\n- fact number 8\n- fact number 9"
        );
    }

    #[test]
    fn what_is_added_from_outside_becomes_part_of_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let worker = Undertaking::open(dir.path(), Some("find when it closes")).unwrap();
        // Someone else, with their own handle on the same directory.
        let outside = Undertaking::open(dir.path(), None).unwrap();
        outside.add("weekends too").unwrap();
        outside.add("and whether it takes bookings").unwrap();
        assert_eq!(
            worker.take_added(),
            ["weekends too", "and whether it takes bookings"]
        );
        assert!(worker.take_added().is_empty(), "taken once");
        assert_eq!(
            worker.state().task_text(),
            "find when it closes\n\nAdded later: weekends too\n\nAdded later: and whether it takes bookings"
        );
    }

    #[test]
    fn nothing_starts_without_a_brief() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Undertaking::open(dir.path(), None).is_err());
    }
}
