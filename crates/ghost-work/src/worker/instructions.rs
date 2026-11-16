//! What the worker is told: the working instructions, the opening a
//! sitting starts from, and the words both are made of.
//!
//! The words are looked at again before every step, so that an edit to the
//! files they come from reaches work already running. What a sitting shows
//! the model before its transcript is kept together as one [`Shown`], since
//! its size counts against the context like the transcript's does.

use ghost_brain::{Item, ToolSpec};

use super::{Limits, Worker};
use crate::{
    prompts::{self, Loop, Prompts},
    tool::Toolbox,
    undertaking::State,
};

/// The share of the notes a new sitting is shown, in characters.
const NOTES_SHOWN: usize = 2500;

/// What every request of a sitting shows the model before its transcript:
/// the words they come from, the instructions, and the tools.
pub(super) struct Shown {
    pub(super) words: Prompts,
    pub(super) system: String,
    pub(super) specs: Vec<ToolSpec>,
}

impl Shown {
    /// Its size in characters.
    pub(super) fn weight(&self) -> usize {
        self.system.len()
            + self
                .specs
                .iter()
                .map(|s| s.description.len() + s.parameters.to_string().len())
                .sum::<usize>()
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
pub(super) fn opening_with(words: &Prompts, state: &State, tail: bool) -> String {
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
            ("plan", state.plan_text(words)),
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
    pub(super) fn opening(&self, words: &Prompts) -> Vec<Item> {
        vec![Item::user(opening_text(words, &self.undertaking.state()))]
    }

    /// What the model is shown, in `words`, with `tools`.
    pub(super) fn shown(&self, tools: &Toolbox, words: Prompts) -> Shown {
        Shown {
            system: self.system(&words),
            specs: tools.specs(&words, Loop::Work),
            words,
        }
    }

    /// The words as they are now, looked at again first, and whether they
    /// changed.
    pub(super) fn words(&self) -> (Prompts, bool) {
        let mut words = prompts::hold(&self.prompts);
        let changed = words.refresh_now();
        (words.clone(), changed)
    }
}
