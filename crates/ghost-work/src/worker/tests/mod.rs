//! How a worker is driven in a test: a scripted brain that remembers what
//! it was shown, a worker on a directory of its own, and what parts of the
//! work need besides. The tests themselves are beside this, by what they
//! show.

mod context;
mod parts;
mod prompts;
mod results;
mod run;

use std::sync::{Mutex, atomic::AtomicU64};

use async_trait::async_trait;
use ghost_brain::{Call, Completion, Reply, Result, Step, ToolSpec};
use serde_json::Value;

use super::{
    context::{PRUNE_OVER, UNKNOWN_WINDOW, prune},
    instructions::opening_with,
    *,
};
use crate::{machine::Local, tools};
use crate::{
    prompts::{Loop, Store},
    tool::{self, Tool},
    undertaking::Progress,
};

/// A brain that plays back a script, and remembers what it was shown.
/// Asked for a handoff, it writes one without spending the script.
struct Scripted {
    steps: Mutex<Vec<Step>>,
    /// Scripts of their own for parts of the work: a part whose task
    /// holds the words is played those steps.
    parts: Mutex<Vec<(&'static str, Vec<Step>)>>,
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
        let task = match turn.items.first() {
            Some(Item::User { text }) => text.as_str(),
            _ => "",
        };
        let part = self
            .parts
            .lock()
            .unwrap()
            .iter_mut()
            .find(|(words, _)| task.contains(words))
            .map(|(_, script)| {
                if script.is_empty() {
                    says("")
                } else {
                    script.remove(0)
                }
            });
        let mut step = if asked_to_hand_off {
            calls("handoff", json!({"text": "the file goes in hi.txt"}))
        } else if let Some(step) = part {
            step
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
    let undertaking =
        Arc::new(Undertaking::open(&state_db(dir), "main", Some("make hi.txt say hi")).unwrap());
    let brain = Arc::new(Scripted {
        steps: Mutex::new(script),
        parts: Mutex::new(vec![]),
        seen: Mutex::new(vec![]),
        window: AtomicU64::new(0),
        refuses_over: AtomicU64::new(0),
        max_output: AtomicU64::new(0),
        sees: AtomicBool::new(false),
    });
    let machine = Arc::new(Local::new(dir));
    let prompts = Prompts::shipped().shared();
    let worker = Worker {
        brain: brain.clone(),
        tools: tools::standard(machine, &undertaking, &prompts),
        undertaking,
        limits,
        who: String::new(),
        machine: "a test directory".into(),
        pause: Arc::new(AtomicBool::new(false)),
        helpers: None,
        prompts,
        agents: Vec::new(),
    };
    (worker, brain)
}

impl Scripted {
    /// Play `script` to the part whose task holds `words`.
    fn part(&self, words: &'static str, script: Vec<Step>) {
        self.parts.lock().unwrap().push((words, script));
    }

    /// The requests made for the work whose task holds `words`, in
    /// order: the parent's, as `worker` sets it, or a part's.
    fn turns_of(&self, words: &str) -> Vec<Turn> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(
                |t| matches!(t.items.first(), Some(Item::User { text }) if text.contains(words)),
            )
            .cloned()
            .collect()
    }

    /// Where in all the requests the first and the last one for the
    /// work whose task holds `words` came.
    fn span_of(&self, words: &str) -> (usize, usize) {
        let seen = self.seen.lock().unwrap();
        let at: Vec<usize> = seen
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t.items.first(), Some(Item::User { text }) if text.contains(words)))
            .map(|(i, _)| i)
            .collect();
        (at[0], *at.last().unwrap())
    }
}

const PARENT: &str = "make hi.txt say hi";

/// A tool that does nothing until the gate it shares opens: what a part
/// is doing while its parent is watched.
struct Hold(Arc<AtomicBool>);

#[async_trait]
impl Tool for Hold {
    fn name(&self) -> &'static str {
        "hold"
    }
    fn spec(&self, _: &Store, _: Loop) -> ToolSpec {
        tool::spec("hold", "Wait to be let go.", json!({}), &[])
    }
    async fn call(&self, _: &Value) -> tool::Outcome {
        while !self.0.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        tool::Outcome::ok("let go")
    }
}

/// A tool that opens the gate every `hold` waits at, once `check` has
/// looked at how things stand.
struct Release(Arc<AtomicBool>, Box<dyn Fn() + Send + Sync>);

#[async_trait]
impl Tool for Release {
    fn name(&self) -> &'static str {
        "release"
    }
    fn spec(&self, _: &Store, _: Loop) -> ToolSpec {
        tool::spec("release", "Let them go.", json!({}), &[])
    }
    async fn call(&self, _: &Value) -> tool::Outcome {
        (self.1)();
        self.0.store(true, Ordering::Relaxed);
        tool::Outcome::ok("released")
    }
}

/// Let `worker` hand out parts, which work in `dir` with the standard
/// tools and, given a gate, a `hold` that waits for it to open. Returns
/// the gate, and gives the parent a `release` that opens it after
/// `check`.
fn with_parts(
    worker: &mut Worker,
    dir: &std::path::Path,
    check: Option<Box<dyn Fn() + Send + Sync>>,
) -> Arc<AtomicBool> {
    let gate = Arc::new(AtomicBool::new(false));
    let (root, held, words) = (dir.to_path_buf(), gate.clone(), worker.prompts.clone());
    worker.helpers = Some(Arc::new(move |undertaking| {
        tools::standard(Arc::new(Local::new(&root)), undertaking, &words).with(Hold(held.clone()))
    }));
    if let Some(check) = check {
        worker.tools = worker.tools.clone().with(Release(gate.clone(), check));
    }
    gate
}

/// What the call to `name` returned, the last time in `turn`.
fn result_of<'a>(turn: &'a Turn, name: &str) -> &'a str {
    turn.items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::Result {
                name: n, output, ..
            } if n == name => Some(output.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result of {name}"))
}

/// The messages put in front of the worker after its opening.
fn messages(turn: &Turn) -> Vec<&str> {
    turn.items[1..]
        .iter()
        .filter_map(|item| match item {
            Item::User { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn part_record(dir: &std::path::Path, n: u64) -> Undertaking {
    Undertaking::open(&state_db(dir), &format!("main/{n}"), None).unwrap()
}

fn both(first: Step, second: Step) -> Step {
    Step {
        calls: [first.calls, second.calls]
            .concat()
            .into_iter()
            .enumerate()
            .map(|(i, mut call)| {
                call.id = format!("c{i}");
                call
            })
            .collect(),
        stop: Stop::Calls,
        ..says("")
    }
}
