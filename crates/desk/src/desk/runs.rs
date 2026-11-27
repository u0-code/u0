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
    hub::Host,
    proto::Call,
    scripts::Script,
    terminal::{self, Terminal},
};

/// The key context of the drawer's terminals: keys bound in it (the next
/// terminal, ⇧⌘]) win over the window's while a terminal has the keys.
pub const CONTEXT: &str = "Terminal";

/// The size a terminal opens at before the drawer has measured it.
const FIRST_SIZE: (u16, u16) = (14, 100);

/// `address` at `port` on this Mac.
fn here_at(address: &str, port: u16) -> String {
    let Ok(mut url) = url::Url::parse(address) else {
        return address.to_owned();
    };
    if url.port_or_known_default() != Some(port) {
        let _ = url.set_host(Some("127.0.0.1"));
        let _ = url.set_port(Some(port));
    }
    url.into()
}

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
/// the tab the pointer is over, which shows its close button; one asked
/// to close with something running in it, to close at the second
/// asking; a script being written, and its name and command.
/// How the drawer's terminals stand: one at a time, side by side, or one
/// above another (several at most `SIDE_BY_SIDE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    One,
    Side,
    Stacked,
}

pub struct Drawer {
    pub terminals: Vec<Terminal>,
    pub open: bool,
    pub focus: FocusHandle,
    pub on: Option<u64>,
    pub layout: Layout,
    pub closing: Option<u64>,
    pub writing: Option<Writing>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub preview: Entity<InputState>,
}

impl Drawer {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        Self {
            terminals: Vec::new(),
            open: false,
            focus: cx.focus_handle(),
            on: None,
            layout: Layout::One,
            closing: None,
            writing: None,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("dev")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("npm run dev"))),
            preview: cx.new(|cx| {
                InputState::new(window, cx).placeholder(crate::tr("Preview: http://localhost:3000"))
            }),
        }
    }
}

/// The script being written in the drawer: the one at `at` changed, or a
/// new one; its name and command are in the drawer's two fields.
#[derive(Debug, Clone, Copy, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the script's own switches, as it is written"
)]
pub struct Writing {
    pub at: Option<usize>,
    pub worktree: bool,
    pub wait: bool,
    pub settle: bool,
    pub stop: bool,
}

/// Where an address is opened: in your browser, or in the Preview tab of
/// a session, by its key (on a system u0 shows no page on, your browser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    Browser,
    Preview(u64),
}

/// A running dev server printed by a terminal, local or forwarded.
pub struct DevServer {
    pub title: String,
    pub command: Option<String>,
    pub machine: String,
    pub host: Host,
    /// Where it is reached from this Mac: as printed, or at the port it
    /// is forwarded to.
    pub url: String,
    /// As its terminal printed it, on its own machine: what is opened.
    pub printed: String,
    /// `(remote_port, local_port)` when forwarded from a paired machine.
    pub forwarded: Option<(u16, u16)>,
}

impl DevServer {
    /// The port it listens on, as printed.
    pub fn port(&self) -> Option<u16> {
        url::Url::parse(&self.printed)
            .ok()
            .and_then(|u| u.port_or_known_default())
    }
}

impl Desk {
    /// Stop forwarding remote port `there` on `host`.
    pub fn stop_forward(&mut self, host: Host, there: u16, cx: &mut Context<Self>) {
        if let Some(m) = self.machine_mut(host) {
            m.forwards.retain(|f| f.there != there);
        }
        cx.notify();
    }

    /// Running dev servers detected across the current session's terminals.
    pub(crate) fn dev_servers(&self) -> Vec<DevServer> {
        let scripts = self
            .session()
            .map(|s| s.scripts.clone())
            .unwrap_or_default();
        let mut out = Vec::new();
        for t in self.session_terminals() {
            if t.ended.is_some() {
                continue;
            }
            let script = scripts.iter().find(|x| t.script.as_ref() == Some(&x.name));
            let Some(raw_url) = script
                .and_then(|x| x.preview.clone())
                .or_else(|| address(&t.text()))
            else {
                continue;
            };
            let port = url::Url::parse(&raw_url)
                .ok()
                .and_then(|u| u.port_or_known_default());
            let machine = self
                .machine(t.host)
                .map_or_else(|| crate::tr("This Mac").to_string(), |m| m.name.clone());
            let forwarded = self
                .machine(t.host)
                .filter(|_| t.host != Host::HERE)
                .zip(port)
                .and_then(|(m, p)| m.forwards.iter().find(|f| f.there == p))
                .map(|f| (f.there, f.port));
            let url = match forwarded {
                Some((_, local)) => here_at(&raw_url, local),
                None => raw_url.clone(),
            };
            out.push(DevServer {
                title: t.script.clone().unwrap_or_else(|| t.title.clone()),
                command: script.map(|s| s.command.clone()),
                machine,
                host: t.host,
                url,
                printed: raw_url,
                forwarded,
            });
        }
        out
    }
    /// Open `address`, a local one printed on the server `host` (a dev
    /// server's), where `to` says: as it is on this Mac's own; on another,
    /// once its port is brought here over a link of its own
    /// (`link::forward`), kept while that server is held.
    pub(crate) fn open_address(
        &mut self,
        host: Host,
        address: String,
        to: To,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.machine(host) else { return };
        let there = url::Url::parse(&address)
            .ok()
            .filter(|u| {
                let local = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"];
                u.host_str().is_some_and(|h| local.contains(&h))
            })
            .and_then(|u| u.port_or_known_default());
        let (Some(server), Some(there), false) = (m.paired.clone(), there, host == Host::HERE)
        else {
            return self.open_at(to, address, cx);
        };
        match m.forwards.iter().find(|f| f.there == there) {
            Some(f) => {
                let here = here_at(&address, f.port);
                self.open_at(to, here, cx);
            }
            None => crate::hub::forward(
                host,
                server,
                there,
                self.settings.forward_port,
                address,
                to,
                self.tx.clone(),
            ),
        }
    }

    /// Open `address`, reachable from this Mac, where `to` says.
    fn open_at(&mut self, to: To, address: String, cx: &mut Context<Self>) {
        match to {
            #[cfg(target_os = "macos")]
            To::Preview(key) => self.show_page(key, address, cx),
            _ => cx.open_url(&address),
        }
    }

    /// A port brought here, to open `address` on; or why not.
    pub(crate) fn forwarded(
        &mut self,
        host: Host,
        address: &str,
        to: To,
        result: Result<crate::link::Forward, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(forward) => {
                let here = here_at(address, forward.port);
                if let Some(m) = self.machine_mut(host) {
                    m.forwards.push(forward);
                }
                self.open_at(to, here, cx);
            }
            Err(why) => self.trouble(
                crate::tr_with("Couldn't open %{address} here", &[("address", &address)]),
                Some(why),
                None,
            ),
        }
    }

    /// Open a terminal beside the session `key`, in `dir` on its server;
    /// `first` is typed into it (a script's command). The drawer opens on
    /// it.
    pub(crate) fn open_terminal_in(
        &mut self,
        key: u64,
        dir: &Path,
        first: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.sessions.at(key).map(|s| s.host) {
            self.terminal_at(host, key, dir, first, cx);
        }
    }

    /// Open a terminal on the server `host`, in u0's home there, beside
    /// the session on screen; `first` is typed into it: a sign-in there.
    pub(crate) fn terminal_on(&mut self, host: Host, first: &str, cx: &mut Context<Self>) {
        let Some(key) = self.current else {
            self.trouble(crate::tr("Open a thread to sign in beside it"), None, None);
            return;
        };
        let dir = self.home_on(host).to_path_buf();
        self.terminal_at(host, key, &dir, Some(first), cx);
    }

    /// Open a terminal on the server `host`, beside the session `key`, in
    /// `dir` there; `first` typed into it.
    fn terminal_at(
        &mut self,
        host: Host,
        key: u64,
        dir: &Path,
        first: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let id = self.sessions.take_key();
        let size = self
            .drawer
            .terminals
            .last()
            .map_or(FIRST_SIZE, Terminal::size);
        let call = Call::TermOpen {
            id,
            dir: dir.to_path_buf(),
            shell: self.settings.shell.program().map(str::to_owned),
            first: first.map(str::to_owned),
            size,
        };
        self.call(host, call);
        let mut t = Terminal::new(id, key, size);
        t.host = host;
        self.drawer.terminals.push(t);
        self.drawer.on = Some(id);
        self.drawer.open = true;
        cx.notify();
    }

    /// Ask the server the terminal `id` runs on what `of` makes of it: a
    /// call to send it what was typed, pasted, or its size.
    fn ask_terminal(&mut self, id: u64, of: impl FnOnce(&mut Terminal) -> Option<Call>) {
        let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let host = t.host;
        if let Some(call) = of(t) {
            self.call(host, call);
        }
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
        let force = !self.settings.confirm.terminal || self.drawer.closing == Some(id);
        self.ask_terminal(id, |_| Some(Call::TermClose { id, force }));
        cx.notify();
    }

    /// The terminal `id` closed; or not, `busy` with a command, until it
    /// is closed again.
    pub(crate) fn terminal_closed(&mut self, id: u64, busy: bool, cx: &mut Context<Self>) {
        if busy {
            self.drawer.closing = Some(id);
            self.trouble(
                crate::tr("Something is still running in that terminal"),
                Some(crate::tr("Close it again to stop it.").into()),
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

    /// The next terminal of the session on screen, or the one before,
    /// round from the last to the first.
    pub fn next_terminal(&mut self, back: bool, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.session_terminals().iter().map(|t| t.id).collect();
        let n = ids.len();
        if n == 0 {
            return;
        }
        let at = ids
            .iter()
            .position(|id| Some(*id) == self.drawer.on)
            .unwrap_or(0);
        self.drawer.on = Some(ids[if back { (at + n - 1) % n } else { (at + 1) % n }]);
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
                self.ask_terminal(id, |t| Some(t.paste(&text)));
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
        // ⇧PageUp and ⇧PageDown scroll back a screen and down again;
        // ⇧⌘Home and ⇧⌘End go to the oldest line and the newest.
        let page = isize::try_from(t.size().0).unwrap_or(24);
        let by = match (k.key.as_str(), k.modifiers.shift, k.modifiers.platform) {
            ("pageup", true, false) => Some(page),
            ("pagedown", true, false) => Some(-page),
            ("home", true, true) => Some(isize::MAX / 2),
            ("end", true, true) => Some(isize::MIN / 2),
            _ => None,
        };
        if let Some(by) = by {
            t.scroll(by);
            cx.stop_propagation();
            cx.notify();
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
            self.ask_terminal(id, |t| Some(t.send(bytes)));
            cx.stop_propagation();
        }
    }

    /// Hand what is chosen in the terminal `id` (or its screen when nothing
    /// is chosen) to the session's agent.
    pub fn ask_about_terminal(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let snippet = t.chosen_text().or_else(|| {
            let trimmed = t.text().trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        let Some(chosen) = snippet else {
            return;
        };
        t.selection = None;
        self.add_context(crate::session::Context::Terminal, &chosen, window, cx);
    }

    /// Paste what is copied into the terminal `id`, as a terminal pastes.
    pub(crate) fn paste_terminal(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.ask_terminal(id, |t| Some(t.paste(&text)));
        }
    }

    /// Clear the terminal keys go to: what it printed forgotten, its shell
    /// asked (⌃L) to draw its prompt again.
    pub fn clear_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.drawer.on else { return };
        self.ask_terminal(id, |t| {
            t.clear();
            Some(t.send(vec![0x0c]))
        });
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
            self.ran(&script.name);
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
            let (host, agent, key) = (s.host, s.meta.agent.clone(), s.key);
            self.open_when_running(host, &agent, key);
        }
    }

    /// Run the repository's script `name` beside the session on screen,
    /// in a terminal of its own: the drawer's button, or a key bound to
    /// `script:<name>`.
    pub fn run_script(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self
            .session()
            .and_then(|s| s.scripts.iter().find(|x| x.name == name))
            .map(|x| x.command.clone())
        else {
            return;
        };
        self.open_terminal(Some(&command), window, cx);
        self.ran(name);
    }

    /// The terminal just opened runs the script `name`.
    fn ran(&mut self, name: &str) {
        let on = self.drawer.on;
        if let Some(t) = self.drawer.terminals.iter_mut().find(|t| Some(t.id) == on) {
            t.script = Some(name.to_owned());
        }
    }

    /// The sessions `keys` settled: the scripts marked to stop then are
    /// stopped, and those marked to run then run in each one's own
    /// worktree, the drawer left as it was.
    pub(crate) fn on_settle(&mut self, keys: &[u64], cx: &mut Context<Self>) {
        let open = self.drawer.open;
        for &key in keys {
            let Some(s) = self.sessions.at(key) else {
                continue;
            };
            let (scripts, worktree) = (s.scripts.clone(), s.meta.worktree.clone());
            let stopped: Vec<u64> = self
                .drawer
                .terminals
                .iter()
                .filter(|t| t.session == key)
                .filter(|t| {
                    scripts
                        .iter()
                        .any(|x| x.stop && t.script.as_ref() == Some(&x.name))
                })
                .map(|t| t.id)
                .collect();
            for id in stopped {
                self.ask_terminal(id, |_| Some(Call::TermClose { id, force: true }));
            }
            for script in scripts.iter().filter(|x| x.settle) {
                if let Some(wt) = &worktree {
                    let command = format!("{}; exit", script.command);
                    self.open_terminal_in(key, &wt.path, Some(&command), cx);
                    self.ran(&script.name);
                }
            }
        }
        self.drawer.open = open;
    }

    /// Stop the scripts the session `key`'s first message waits for, put
    /// their card away, and say it now, as t3code's Cancel on its setup
    /// card.
    pub fn cancel_setup(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        let held = std::mem::take(&mut s.holding);
        s.setup.clear();
        let (host, agent) = (s.host, s.meta.agent.clone());
        for id in held {
            self.ask_terminal(id, |_| Some(Call::TermClose { id, force: true }));
        }
        self.open_when_running(host, &agent, key);
        cx.notify();
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
        let preview = script.preview.clone().unwrap_or_default();
        self.drawer
            .preview
            .update(cx, |i, cx| i.set_value(&preview, window, cx));
        self.drawer.writing = Some(Writing {
            at,
            worktree: script.worktree,
            wait: script.wait,
            settle: script.settle,
            stop: script.stop,
        });
        window.focus(&self.drawer.name.focus_handle(cx), cx);
        cx.notify();
    }

    /// Keep the script being written in `u0.json`, or (`keep` false)
    /// take it away.
    pub fn save_script(&mut self, keep: bool, cx: &mut Context<Self>) {
        let (Some(w), Some((host, key, dir))) = (
            self.drawer.writing,
            self.session()
                .map(|s| (s.host, s.key, s.meta.folder.clone())),
        ) else {
            return;
        };
        let preview = self.drawer.preview.read(cx).value().trim().to_owned();
        let script = keep.then(|| Script {
            name: self.drawer.name.read(cx).value().trim().to_owned(),
            command: self.drawer.command.read(cx).value().trim().to_owned(),
            worktree: w.worktree,
            wait: w.worktree && w.wait,
            settle: w.settle,
            stop: w.stop,
            preview: (!preview.is_empty()).then_some(preview),
        });
        if script
            .as_ref()
            .is_some_and(|s| s.name.is_empty() || s.command.is_empty())
        {
            self.trouble(crate::tr("A script needs a name and a command"), None, None);
            cx.notify();
            return;
        }
        self.drawer.writing = None;
        let call = Call::SaveScript {
            dir,
            at: w.at,
            script,
            key,
        };
        self.call(host, call);
        cx.notify();
    }

    /// Fit the terminal `id` to its pane as laid out: `cols` by `rows`.
    pub(crate) fn fit_terminal(&mut self, id: u64, rows: u16, cols: u16, cx: &mut Context<Self>) {
        let mut resized = false;
        self.ask_terminal(id, |t| {
            let call = t.resize(rows, cols);
            resized = call.is_some();
            call
        });
        if resized {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_brought_here_at_another_port_is_opened_there() {
        assert_eq!(
            here_at("http://localhost:5173/app?x=1", 5173),
            "http://localhost:5173/app?x=1"
        );
        assert_eq!(
            here_at("http://localhost:5173/app?x=1", 60001),
            "http://127.0.0.1:60001/app?x=1"
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
