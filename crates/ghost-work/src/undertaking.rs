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
    pub finish: Option<Finish>,
    /// The worker has been made to check its work once already.
    pub checked: bool,
    /// How many times work was picked up with a fresh context.
    pub sittings: u32,
    pub steps: u64,
    pub usage: Usage,
    pub seconds: f64,
}

impl State {
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

    pub fn notes_text(&self) -> String {
        if self.notes.is_empty() {
            return "(no notes yet)".to_owned();
        }
        self.notes
            .iter()
            .map(|n| format!("- {}", n.text.trim()))
            .collect::<Vec<_>>()
            .join("\n")
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
        assert_eq!(state.notes_text(), "- python is 3.12");
        assert_eq!(again.transcript().unwrap().len(), 1);
    }

    #[test]
    fn nothing_starts_without_a_brief() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Undertaking::open(dir.path(), None).is_err());
    }
}
