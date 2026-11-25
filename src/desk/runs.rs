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
//! it is made.

use std::path::Path;

use gpui::{ClipboardItem, Context, Focusable as _, KeyDownEvent, Window};
use serde::Deserialize;

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
            .iter()
            .find(|s| s.key == key)
            .and_then(|s| crate::remote::parse(&s.project))
            .map(|(host, _)| host);
        let id = self.take_key();
        let size = self.terminals.last().map_or(FIRST_SIZE, Terminal::size);
        match Terminal::open(
            (id, key),
            dir,
            host.as_deref(),
            first,
            size,
            self.tx.clone(),
        ) {
            Ok(t) => {
                self.terminals.push(t);
                self.term_on = Some(id);
                self.drawer = true;
            }
            Err(why) => tracing::warn!("{why}"),
        }
        cx.notify();
    }

    /// Open a terminal beside the session on screen, in its folder; `first`
    /// is typed into it.
    pub fn open_terminal(
        &mut self,
        first: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((key, dir)) = self.session().map(|s| (s.key, s.folder.clone())) else {
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
    pub(crate) fn run_worktree_scripts(&mut self, key: u64, dir: &Path, cx: &mut Context<Self>) {
        for script in config(dir).scripts.into_iter().filter(|s| s.worktree) {
            self.open_terminal_in(key, dir, Some(&script.command), cx);
            if let Some(s) = self.sessions.iter_mut().find(|s| s.key == key) {
                s.setup.push(script.name);
            }
        }
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
