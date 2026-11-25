//! Your terminals in a drawer under the thread (⌘J), as t3code's: each a
//! real shell (`crate::terminal`) opened beside a session, in its folder,
//! its worktree, or on the machine a remote project is on; one shown, or
//! two side by side. A local address one prints (a dev server's) can be
//! opened, and what one shows can be handed to the session's agent. A
//! repository's own commands (`crate::scripts`) show as buttons in it, and
//! it adds, changes and takes them away.

use std::path::Path;

use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable as _, KeyDownEvent,
    Window,
};

use gpui::component::input::InputState;

use super::Desk;
use crate::{
    proto::Call,
    scripts::Script,
    terminal::{self, Terminal},
};

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

/// Your terminals in the drawer under the thread: whether it is open, where
/// keys go in it, the one they go to, and whether two show side by side;
/// one asked to close with something running in it, to close at the
/// second asking; a script being written, and its name and command.
pub struct Drawer {
    pub terminals: Vec<Terminal>,
    pub open: bool,
    pub focus: FocusHandle,
    pub on: Option<u64>,
    pub split: bool,
    pub closing: Option<u64>,
    pub writing: Option<Writing>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
}

impl Drawer {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        Self {
            terminals: Vec::new(),
            open: false,
            focus: cx.focus_handle(),
            on: None,
            split: false,
            closing: None,
            writing: None,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("dev")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder("npm run dev")),
        }
    }
}

/// The script being written in the drawer: the one at `at` changed, or a
/// new one; its name and command are in the drawer's two fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct Writing {
    pub at: Option<usize>,
    pub worktree: bool,
    pub wait: bool,
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
        let size = self
            .drawer
            .terminals
            .last()
            .map_or(FIRST_SIZE, Terminal::size);
        self.transport.send(Call::TermOpen {
            id,
            dir: dir.to_path_buf(),
            host,
            first: first.map(str::to_owned),
            size,
        });
        self.drawer.terminals.push(Terminal::new(id, key, size));
        self.drawer.on = Some(id);
        self.drawer.open = true;
        cx.notify();
    }

    /// A terminal opened, called `title`, or why not: one that did not is
    /// put away, and a session's first message waited for it no longer.
    pub(crate) fn terminal_opened(&mut self, id: u64, result: Result<String, String>) {
        match result {
            Ok(title) => {
                if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) {
                    t.title = title;
                }
            }
            Err(why) => {
                tracing::warn!("{why}");
                self.put_away(id);
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
        window.focus(&self.drawer.focus, cx);
    }

    /// The terminals beside the session on screen.
    pub(crate) fn session_terminals(&self) -> Vec<&Terminal> {
        let key = self.current;
        self.drawer
            .terminals
            .iter()
            .filter(|t| Some(t.session) == key)
            .collect()
    }

    /// Close a terminal, stopping what runs in it; with a command still
    /// running in it, only once you have closed it twice.
    pub fn close_terminal(&mut self, id: u64, cx: &mut Context<Self>) {
        let force = self.drawer.closing == Some(id);
        self.transport.send(Call::TermClose { id, force });
        cx.notify();
    }

    /// The terminal `id` closed; or not, `busy` with a command, until it
    /// is closed again.
    pub(crate) fn terminal_closed(&mut self, id: u64, busy: bool, cx: &mut Context<Self>) {
        if busy {
            self.drawer.closing = Some(id);
            self.trouble(
                "Something is still running in that terminal",
                Some("Close it again to stop it.".into()),
                None,
            );
        } else {
            self.drawer.closing = None;
            self.put_away(id);
        }
        cx.notify();
    }

    /// Take the terminal `id` out of the drawer.
    fn put_away(&mut self, id: u64) {
        self.drawer.terminals.retain(|t| t.id != id);
        if self.drawer.on == Some(id) {
            self.drawer.on = self.session_terminals().last().map(|t| t.id);
        }
    }

    /// Open or close the drawer (⌘J); opened with no terminal beside the
    /// session, it opens one.
    pub fn toggle_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drawer.open = !self.drawer.open;
        if self.drawer.open {
            if self.session_terminals().is_empty() {
                self.open_terminal(None, window, cx);
            } else {
                window.focus(&self.drawer.focus, cx);
            }
        } else {
            window.focus(&self.composer.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// A key pressed in the drawer: sent to the terminal it is on, as a
    /// terminal sends it; ⌘V pastes. The window's own keys pass.
    pub(crate) fn terminal_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(id) = self.drawer.on else { return };
        let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let k = &e.keystroke;
        if k.modifiers.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.transport.send(t.paste(&text));
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
            self.transport.send(t.send(bytes));
            cx.stop_propagation();
        }
    }

    /// Hand what is chosen in the terminal `id` to the session's agent:
    /// put in what you are writing, to say what to do with it.
    pub fn ask_about_terminal(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Some(chosen) = t.chosen_text() else {
            return;
        };
        t.selection = None;
        let typed = self.typed(cx);
        let text = format!("{typed}\n\nFrom my terminal:\n```\n{chosen}\n```\n");
        self.put_line(text.trim_start(), window, cx);
        window.focus(&self.composer.input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Copy what is chosen in the terminal `id`, and let it go.
    pub(crate) fn copy_chosen(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id)
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
            let term = self.drawer.on;
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
        self.drawer
            .name
            .update(cx, |i, cx| i.set_value(&script.name, window, cx));
        self.drawer
            .command
            .update(cx, |i, cx| i.set_value(&script.command, window, cx));
        self.drawer.writing = Some(Writing {
            at,
            worktree: script.worktree,
            wait: script.wait,
        });
        window.focus(&self.drawer.name.focus_handle(cx), cx);
        cx.notify();
    }

    /// Keep the script being written in `ghost.json`, or (`keep` false)
    /// take it away.
    pub fn save_script(&mut self, keep: bool, cx: &mut Context<Self>) {
        let (Some(w), Some((key, dir))) = (
            self.drawer.writing,
            self.session().map(|s| (s.key, s.meta.folder.clone())),
        ) else {
            return;
        };
        let script = keep.then(|| Script {
            name: self.drawer.name.read(cx).value().trim().to_owned(),
            command: self.drawer.command.read(cx).value().trim().to_owned(),
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
        self.drawer.writing = None;
        self.transport.send(crate::proto::Call::SaveScript {
            dir,
            at: w.at,
            script,
            key,
        });
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

    /// Fit the terminal `id` to its pane as laid out: `cols` by `rows`.
    pub(crate) fn fit_terminal(&mut self, id: u64, rows: u16, cols: u16, cx: &mut Context<Self>) {
        if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id)
            && let Some(call) = t.resize(rows, cols)
        {
            self.transport.send(call);
            cx.notify();
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
