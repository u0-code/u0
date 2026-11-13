//! The tools a worker starts with.
//!
//! Few, and each one sharp. `shell` reaches everything a machine can do;
//! `read`, `write` and `edit` exist because quoting file contents through a
//! shell is where models lose the most time; `plan`, `note` and `done` are
//! how work is carried across context windows and brought to an end.

mod files;
mod record;
mod shell;
mod web;

use std::{io, sync::Arc, sync::Mutex, sync::atomic::AtomicU32, time::Duration};

pub use files::{Edit, Read, Write};
pub use record::{Ask, Done, Handoff, Note, Plan};
pub use shell::Shell;
pub use web::{Fetch, Search, Source, WebConfig};

use crate::{
    machine::{Machine, Output, quote},
    tool::Toolbox,
    undertaking::Undertaking,
};

/// A machine, and where on it the worker currently stands. The tools that
/// act on the machine share one of these, so a `cd` in the shell moves the
/// file tools too.
pub struct Desk {
    pub machine: Arc<dyn Machine>,
    cwd: Mutex<Option<String>>,
    jobs: AtomicU32,
    /// Sets this desk's files in `/tmp` apart from every other desk's.
    tag: String,
}

impl Desk {
    pub fn new(machine: Arc<dyn Machine>) -> Arc<Self> {
        Arc::new(Self {
            machine,
            cwd: Mutex::new(None),
            jobs: AtomicU32::new(0),
            tag: format!(
                "{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos())
            ),
        })
    }

    fn cwd(&self) -> Option<String> {
        self.cwd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn moved_to(&self, dir: String) -> bool {
        let mut cwd = self
            .cwd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = cwd.as_deref().is_some_and(|was| was != dir);
        *cwd = Some(dir);
        changed
    }

    /// The line that puts a script where the worker stands.
    fn prelude(&self) -> String {
        self.cwd()
            .map(|dir| format!("cd -- {} 2>/dev/null\n", quote(&dir)))
            .unwrap_or_default()
    }

    /// Run a script from where the worker stands.
    async fn exec(
        &self,
        script: &str,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> io::Result<Output> {
        self.machine
            .exec(&format!("{}{script}", self.prelude()), stdin, timeout)
            .await
    }
}

/// Keep the beginning and the end of long text, and say what was cut.
pub(crate) fn clip(text: &str, head: usize, tail: usize) -> String {
    let total = text.chars().count();
    if total <= head + tail {
        return text.to_owned();
    }
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(total - tail).collect();
    format!(
        "{start}\n[... {} characters cut from the middle ...]\n{end}",
        total - head - tail
    )
}

/// The standard set: a shell and files on `machine`, and the record of
/// `undertaking`.
pub fn standard(machine: Arc<dyn Machine>, undertaking: &Arc<Undertaking>) -> Toolbox {
    let desk = Desk::new(machine);
    Toolbox::new()
        .with(Shell::new(desk.clone()))
        .with(Read::new(desk.clone()))
        .with(Write::new(desk.clone()))
        .with(Edit::new(desk))
        .with(Plan::new(undertaking.clone()))
        .with(Note::new(undertaking.clone()))
        .with(Done::new(undertaking.clone()))
}

/// The record of `undertaking` and nothing else: for work that needs no
/// machine, such as finding something out.
pub fn desk(undertaking: &Arc<Undertaking>) -> Toolbox {
    Toolbox::new()
        .with(Plan::new(undertaking.clone()))
        .with(Note::new(undertaking.clone()))
        .with(Done::new(undertaking.clone()))
}

/// Add the web to a toolbox: `search` if there is somewhere to search, and
/// `fetch` always.
#[must_use]
pub fn with_web(tools: Toolbox, config: &WebConfig) -> Toolbox {
    let tools = match &config.search {
        Some(source) => tools.with(Search::new(source.clone())),
        None => tools,
    };
    tools.with(Fetch::new(config.reader_key.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_both_ends() {
        let text = "a".repeat(50) + &"b".repeat(50);
        let cut = clip(&text, 10, 10);
        assert!(cut.starts_with("aaaaaaaaaa\n[... 80 characters cut"));
        assert!(cut.ends_with("bbbbbbbbbb"));
        assert_eq!(clip("short", 10, 10), "short");
    }
}
