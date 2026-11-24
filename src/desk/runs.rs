//! Commands you run beside a session, in a drawer under its thread (⌘J), as
//! t3code's terminal drawer: each in the session's folder, through your
//! login shell, its output as it comes, stoppable; the last two side by
//! side. A local address one prints (a dev server's) can be opened.
//!
//! A repository can name its own commands in `ghost.json` at its root:
//!
//! ```json
//! { "scripts": [ { "name": "dev", "command": "npm run dev" },
//!                { "name": "setup", "command": "npm install", "worktree": true } ] }
//! ```
//!
//! Each shows as a button in the drawer; one marked `worktree` runs in each
//! new worktree made for a session, as it is made.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use gpui::{Context, Focusable as _, Window};
use serde::Deserialize;

use super::Desk;

/// How much of a command's output is kept: its last this many bytes.
const KEEP: usize = 64 * 1024;

/// A command run beside a session.
pub struct Run {
    pub id: u64,
    pub command: String,
    pub output: String,
    /// How it ended: its exit code, or why it could not run.
    pub done: Option<Result<i32, String>>,
    pub stop: Arc<AtomicBool>,
}

impl Run {
    /// A local address it printed, to open: the last one.
    pub fn address(&self) -> Option<String> {
        let mut found: Option<(usize, String)> = None;
        for start in ["http://localhost:", "http://127.0.0.1:", "http://0.0.0.0:"] {
            let mut from = 0;
            while let Some(at) = self.output[from..].find(start) {
                let begin = from + at;
                let end = self.output[begin..]
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
                    .map_or(self.output.len(), |e| begin + e);
                if found.as_ref().is_none_or(|(at, _)| begin > *at) {
                    found = Some((
                        begin,
                        self.output[begin..end].replace("0.0.0.0", "localhost"),
                    ));
                }
                from = end;
            }
        }
        found.map(|(_, a)| a)
    }
}

/// A repository's own commands, from `ghost.json` at its root.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub name: String,
    pub command: String,
    /// Run in each new worktree, as it is made.
    #[serde(default)]
    pub worktree: bool,
}

/// The commands `ghost.json` names in the repository at `dir`; none where
/// there is no such file, and none (said once in the log) where it does not
/// read.
pub fn config(dir: &Path) -> Config {
    let Some(root) = crate::git::root(dir) else {
        return Config::default();
    };
    match std::fs::read_to_string(root.join("ghost.json")) {
        Err(_) => Config::default(),
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{} does not read: {e}", root.join("ghost.json").display());
            Config::default()
        }),
    }
}

impl Desk {
    /// Run `command` in `dir`, in the drawer, which opens.
    pub fn run_command(&mut self, dir: PathBuf, command: String, cx: &mut Context<Self>) {
        let id = self.take_key();
        let stop = Arc::new(AtomicBool::new(false));
        crate::hub::run(id, dir, command.clone(), stop.clone(), self.tx.clone());
        self.runs.push(Run {
            id,
            command,
            output: String::new(),
            done: None,
            stop,
        });
        self.drawer = true;
        cx.notify();
    }

    /// What a run printed, kept to its last part.
    pub(crate) fn run_output(&mut self, id: u64, text: &str) {
        if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
            r.output.push_str(text);
            if r.output.len() > KEEP {
                let mut cut = r.output.len() - KEEP;
                while !r.output.is_char_boundary(cut) {
                    cut += 1;
                }
                r.output.drain(..cut);
            }
        }
    }

    pub(crate) fn run_done(&mut self, id: u64, result: Result<i32, String>) {
        if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
            r.done = Some(result);
        }
    }

    pub fn stop_run(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(r) = self.runs.iter().find(|r| r.id == id) {
            r.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Take a run out of the drawer, stopping it first.
    pub fn close_run(&mut self, id: u64, cx: &mut Context<Self>) {
        self.stop_run(id, cx);
        self.runs.retain(|r| r.id != id);
        cx.notify();
    }

    /// Open or close the drawer (⌘J).
    pub fn toggle_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drawer = !self.drawer;
        if self.drawer {
            window.focus(&self.command.focus_handle(cx));
        }
        cx.notify();
    }

    /// The command typed in the drawer, run in the session's folder.
    pub(crate) fn run_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        // On another machine it runs there, over SSH.
        let (dir, line) = match crate::remote::parse(&s.project) {
            Some((host, _)) => (
                self.home.clone(),
                crate::remote::run_in(&host, &s.folder, text),
            ),
            None => (s.folder.clone(), text.to_owned()),
        };
        self.run_command(dir, line, cx);
    }

    /// The scripts of a new worktree's repository marked to run in each new
    /// worktree, run there now.
    pub(crate) fn run_worktree_scripts(&mut self, dir: &Path, cx: &mut Context<Self>) {
        for script in config(dir).scripts.into_iter().filter(|s| s.worktree) {
            self.run_command(dir.to_path_buf(), script.command, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(output: &str) -> Run {
        Run {
            id: 1,
            command: String::new(),
            output: output.into(),
            done: None,
            stop: Arc::default(),
        }
    }

    #[test]
    fn the_last_local_address_a_command_printed_is_the_one_to_open() {
        assert_eq!(run("nothing here").address(), None);
        assert_eq!(
            run("  ➜  Local:   http://localhost:5173/\n")
                .address()
                .as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(
            run("listening on http://0.0.0.0:8080 (http://127.0.0.1:9000)")
                .address()
                .as_deref(),
            Some("http://127.0.0.1:9000")
        );
    }
}
