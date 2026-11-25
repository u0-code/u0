//! Your terminals in a drawer under the thread (⌘J), as t3code's: each a
//! real shell (`crate::terminal`) opened beside a session, in its folder,
//! its worktree, or on the machine a remote project is on; one shown, or
//! two side by side. A local address one prints (a dev server's) can be
//! opened, and what one shows can be handed to the session's agent.
//!
//! A repository can name its own commands in `ghost.json` at its root:
//!
//! ```json
//! { "scripts": [ { "name": "dev", "command": "npm run dev" },
//!                { "name": "setup", "command": "npm install", "worktree": true } ] }
//! ```
//!
//! Each shows as a button in the drawer and runs in a terminal of its own;
//! one marked `worktree` runs in each new worktree made for a session, as
//! it is made, and one marked `wait` as well holds the session's first
//! message until it has run. The drawer adds, changes and takes away
//! scripts, written back to the file.

use std::path::{Path, PathBuf};

use gpui::{ClipboardItem, Context, Focusable as _, KeyDownEvent, Window};
use serde::{Deserialize, Serialize};

use super::Desk;
use crate::terminal::{self, Terminal};

/// The size a terminal opens at before the drawer has measured it.
const FIRST_SIZE: (u16, u16) = (14, 100);

/// The last local address `text` shows (a dev server's), to open.
pub fn address(text: &str) -> Option<String> {
    let mut found: Option<(usize, String)> = None;
    for start in ["http://localhost:", "http://127.0.0.1:", "http://0.0.0.0:"] {
        let mut from = 0;
        while let Some(at) = text[from..].find(start) {
            let begin = from + at;
            let end = text[begin..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
                .map_or(text.len(), |e| begin + e);
            if found.as_ref().is_none_or(|(at, _)| begin > *at) {
                found = Some((begin, text[begin..end].replace("0.0.0.0", "localhost")));
            }
            from = end;
        }
    }
    found.map(|(_, a)| a)
}

/// A repository's own commands, from `ghost.json` at its root.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub name: String,
    pub command: String,
    /// Run in each new worktree, as it is made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktree: bool,
    /// Run so in a new worktree, the session's first message waits until
    /// it has run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
}

/// The script being written in the drawer: the one at `at` changed, or a
/// new one; its name and command are in the drawer's two fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct Writing {
    pub at: Option<usize>,
    pub worktree: bool,
    pub wait: bool,
}

/// `ghost.json` of the repository at `dir`, and what it says: nothing
/// where there is no such file; refused where it does not read, so that
/// nothing is written over it.
fn read_config(dir: &Path) -> Result<(PathBuf, Config), String> {
    let root = crate::git::root(dir).ok_or("the folder is not in a git repository")?;
    let path = root.join("ghost.json");
    match std::fs::read_to_string(&path) {
        Err(_) => Ok((path, Config::default())),
        Ok(text) => serde_json::from_str(&text)
            .map(|c| (path.clone(), c))
            .map_err(|e| {
                format!(
                    "{} does not read ({e}); move it away or mend it",
                    path.display()
                )
            }),
    }
}

/// Put `script` at `at` in `ghost.json` (a new one at the end), or take
/// the one at `at` away when `script` is none.
pub fn write_script(dir: &Path, at: Option<usize>, script: Option<Script>) -> Result<(), String> {
    let (path, mut config) = read_config(dir)?;
    match (at, script) {
        (Some(at), Some(script)) if at < config.scripts.len() => config.scripts[at] = script,
        (Some(at), None) if at < config.scripts.len() => {
            config.scripts.remove(at);
        }
        (None, Some(script)) => config.scripts.push(script),
        _ => return Err("the script is no longer in ghost.json".into()),
    }
    let mut text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    text.push('\n');
    crate::whole::write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// The commands `ghost.json` names in the repository whose root is `root`;
/// none where there is no such file, and none (said once in the log) where
/// it does not read.
pub fn config(root: &Path) -> Config {
    match std::fs::read_to_string(root.join("ghost.json")) {
        Err(_) => Config::default(),
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{} does not read: {e}", root.join("ghost.json").display());
            Config::default()
        }),
    }
}

impl Desk {
    /// Open a terminal beside the session `key`, in `dir`; `first` is
    /// typed into it (a script's command). The drawer opens on it.
    pub(crate) fn open_terminal_in(
        &mut self,
        key: u64,
        dir: &Path,
        first: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let host = self
            .sessions
            .at(key)
            .and_then(|s| crate::remote::parse(&s.meta.project))
            .map(|(host, _)| host);
        let id = self.sessions.take_key();
        let size = self.terminals.last().map_or(FIRST_SIZE, Terminal::size);
        let first = first.map(str::to_owned);
        crate::hub::open_terminal(
            id,
            key,
            dir.to_path_buf(),
            host,
            first,
            size,
            self.tx.clone(),
        );
        self.term_on = Some(id);
        self.drawer = true;
        cx.notify();
    }

    /// A terminal opened, or why not: one a session's first message
    /// waited for no longer holds it.
    pub(crate) fn terminal_opened(&mut self, id: u64, result: Result<Box<Terminal>, String>) {
        match result {
            Ok(t) => self.terminals.push(*t),
            Err(why) => {
                tracing::warn!("{why}");
                self.setup_ended(id);
            }
        }
    }

    /// Open a terminal beside the session on screen, in its folder; `first`
    /// is typed into it.
    pub fn open_terminal(
        &mut self,
        first: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((key, dir)) = self.session().map(|s| (s.key, s.meta.folder.clone())) else {
            return;
        };
        self.open_terminal_in(key, &dir, first, cx);
        window.focus(&self.term_focus);
    }

    /// The terminals beside the session on screen.
    pub(crate) fn session_terminals(&self) -> Vec<&Terminal> {
        let key = self.current;
        self.terminals
            .iter()
            .filter(|t| Some(t.session) == key)
            .collect()
    }

    /// Close a terminal, stopping what runs in it; with a command still
    /// running in it, only once you have closed it twice.
    pub fn close_terminal(&mut self, id: u64, cx: &mut Context<Self>) {
        let busy = self
            .terminals
            .iter()
            .find(|t| t.id == id)
            .is_some_and(Terminal::busy);
        if busy && self.closing != Some(id) {
            self.closing = Some(id);
            self.trouble(
                "Something is still running in that terminal",
                Some("Close it again to stop it.".into()),
                None,
            );
            cx.notify();
            return;
        }
        self.closing = None;
        self.terminals.retain(|t| t.id != id);
        if self.term_on == Some(id) {
            self.term_on = self.session_terminals().last().map(|t| t.id);
        }
        cx.notify();
    }

    /// Open or close the drawer (⌘J); opened with no terminal beside the
    /// session, it opens one.
    pub fn toggle_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drawer = !self.drawer;
        if self.drawer {
            if self.session_terminals().is_empty() {
                self.open_terminal(None, window, cx);
            } else {
                window.focus(&self.term_focus);
            }
        } else {
            window.focus(&self.input.focus_handle(cx));
        }
        cx.notify();
    }

    /// A key pressed in the drawer: sent to the terminal it is on, as a
    /// terminal sends it; ⌘V pastes. The window's own keys pass.
    pub(crate) fn terminal_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(id) = self.term_on else { return };
        let Some(t) = self.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let k = &e.keystroke;
        if k.modifiers.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                t.paste(&text);
            }
            cx.stop_propagation();
            return;
        }
        // ⌘T another terminal beside the session; ⌘W this one closed.
        if k.modifiers.platform && !k.modifiers.shift && k.key == "t" {
            let dir = self.session().map(|s| (s.key, s.meta.folder.clone()));
            if let Some((key, dir)) = dir {
                self.open_terminal_in(key, &dir, None, cx);
            }
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform && k.key == "w" {
            self.close_terminal(id, cx);
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform && k.key == "c" {
            if let Some(text) = t.chosen_text() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = terminal::key_bytes(k, t.app_cursor()) {
            t.send(&bytes);
            cx.stop_propagation();
        }
    }

    /// Hand what is chosen in the terminal `id` to the session's agent:
    /// put in what you are writing, to say what to do with it.
    pub fn ask_about_terminal(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Some(chosen) = t.chosen_text() else {
            return;
        };
        t.selection = None;
        let typed = self.input.read(cx).text();
        let text = format!("{typed}\n\nFrom my terminal:\n```\n{chosen}\n```\n");
        self.input
            .update(cx, |i, cx| i.set_text(text.trim_start(), cx));
        window.focus(&self.input.focus_handle(cx));
        cx.notify();
    }

    /// Copy what is chosen in the terminal `id`, and let it go.
    pub(crate) fn copy_chosen(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.terminals.iter_mut().find(|t| t.id == id)
            && let Some(text) = t.chosen_text()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            t.selection = None;
            cx.notify();
        }
    }

    /// The scripts of a new worktree's repository marked to run in each new
    /// worktree, each run there now in a terminal beside the session `key`.
    /// Those marked `wait` end their terminal as they end, and the
    /// session's first message is held until each has: true when one is.
    pub(crate) fn run_worktree_scripts(
        &mut self,
        key: u64,
        dir: &Path,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut held = false;
        let scripts = self.sessions.at(key).map(|s| s.scripts.clone());
        for script in scripts.into_iter().flatten().filter(|s| s.worktree) {
            let command = if script.wait {
                format!("{}; exit", script.command)
            } else {
                script.command.clone()
            };
            self.open_terminal_in(key, dir, Some(&command), cx);
            let term = self.term_on;
            if let Some(s) = self.sessions.at_mut(key) {
                s.setup.push(script.name);
                if script.wait {
                    s.holding.extend(term);
                    held = true;
                }
            }
        }
        held
    }

    /// A terminal ended: a session whose first message waited for it, and
    /// for nothing else now, is opened.
    pub(crate) fn setup_ended(&mut self, id: u64) {
        let Some(s) = self
            .sessions
            .all
            .iter_mut()
            .find(|s| s.holding.contains(&id))
        else {
            return;
        };
        s.holding.retain(|t| *t != id);
        if s.holding.is_empty() {
            let (agent, key) = (s.meta.agent.clone(), s.key);
            self.open_when_running(&agent, key);
        }
    }

    /// Begin writing a script in the drawer: the one at `at` of the
    /// repository on screen, or a new one.
    pub fn write_script(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let script = at
            .and_then(|n| self.session()?.scripts.get(n).cloned())
            .unwrap_or_default();
        self.script_name
            .update(cx, |i, cx| i.set_text(&script.name, cx));
        self.script_command
            .update(cx, |i, cx| i.set_text(&script.command, cx));
        self.writing = Some(Writing {
            at,
            worktree: script.worktree,
            wait: script.wait,
        });
        window.focus(&self.script_name.focus_handle(cx));
        cx.notify();
    }

    /// Keep the script being written in `ghost.json`, or (`keep` false)
    /// take it away.
    pub fn save_script(&mut self, keep: bool, cx: &mut Context<Self>) {
        let (Some(w), Some((key, dir))) = (
            self.writing,
            self.session().map(|s| (s.key, s.meta.folder.clone())),
        ) else {
            return;
        };
        let script = keep.then(|| Script {
            name: self.script_name.read(cx).text().trim().to_owned(),
            command: self.script_command.read(cx).text().trim().to_owned(),
            worktree: w.worktree,
            wait: w.worktree && w.wait,
        });
        if script
            .as_ref()
            .is_some_and(|s| s.name.is_empty() || s.command.is_empty())
        {
            self.trouble("A script needs a name and a command", None, None);
            cx.notify();
            return;
        }
        self.writing = None;
        crate::hub::save_script(dir, w.at, script, key, self.tx.clone());
        cx.notify();
    }

    /// Bring the port of `address` on the machine `host` here (ssh -L),
    /// and open it once it is: kept while Ghost runs.
    pub fn forward(&mut self, host: &str, address: &str, cx: &mut Context<Self>) {
        let Some(port) = address
            .rsplit(':')
            .next()
            .and_then(|p| p.split('/').next())
            .and_then(|p| p.parse::<u16>().ok())
        else {
            return;
        };
        if !self.tunnels.iter().any(|(p, _)| *p == port) {
            match std::process::Command::new("ssh")
                .args(["-N", "-L", &format!("{port}:localhost:{port}"), host])
                .stdin(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => self.tunnels.push((port, child)),
                Err(e) => {
                    self.trouble(
                        format!("Couldn't forward port {port} from {host}"),
                        Some(e.to_string()),
                        None,
                    );
                    cx.notify();
                    return;
                }
            }
        }
        self.done(
            format!("Port {port} on {host} is here now"),
            Some(super::notices::Way::Open(format!(
                "http://localhost:{port}"
            ))),
        );
        cx.notify();
    }

    /// Fit the terminals to the drawer: `cols` by `rows` each.
    pub(crate) fn fit_terminals(&mut self, rows: u16, cols: u16) {
        for t in &mut self.terminals {
            t.resize(rows, cols);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_written_is_kept_in_ghost_json_and_one_that_does_not_read_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git");
        };
        git(&["init", "-q"]);
        let script = |name: &str| Script {
            name: name.into(),
            command: "npm i".into(),
            worktree: true,
            wait: true,
        };
        write_script(dir, None, Some(script("setup"))).expect("written");
        write_script(dir, None, Some(script("dev"))).expect("written");
        write_script(dir, Some(0), None).expect("taken away");
        let names: Vec<_> = config(dir).scripts.into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["dev"]);
        let text = std::fs::read_to_string(dir.join("ghost.json")).expect("file");
        assert!(text.contains("\"wait\": true"), "{text}");
        std::fs::write(dir.join("ghost.json"), "{ not json").expect("broken");
        assert!(write_script(dir, None, Some(script("x"))).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("ghost.json")).expect("kept"),
            "{ not json"
        );
    }

    #[test]
    fn the_last_local_address_a_terminal_shows_is_the_one_to_open() {
        assert_eq!(address("nothing here"), None);
        assert_eq!(
            address("  ➜  Local:   http://localhost:5173/\n").as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(
            address("listening on http://0.0.0.0:8080 (http://127.0.0.1:9000)").as_deref(),
            Some("http://127.0.0.1:9000")
        );
    }
}
