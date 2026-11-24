//! The desk: the whole window, and everything it knows.
//!
//! Down the left, the projects, each with its sessions under it, then the
//! agents switched on, and the desk's settings at the foot. In the middle, the session on
//! screen: its thread, and the line you type into, with the agent's own
//! choices (its mode, its model) under your hand. Down the right, what the
//! session has come to: its plan, the files it changed, how full its
//! context is. An agent that cannot run yet shows, in the middle, what it
//! needs and the button that gets it.
//!
//! This file holds the desk's state, what you do, and how the window is
//! laid out. A turn's checkpoints, undo, worktrees and commits are in
//! `desk/turns.rs`; what comes back from agents and from work off the
//! window's thread in `desk/replies.rs`. The columns are drawn in
//! `sidebar.rs`, `thread.rs`, `aside.rs`, and the pages for an agent and
//! for the settings in `pages.rs`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{
    Context, Entity, Focusable as _, MouseButton, PathPromptOptions, ScrollHandle, Window, div,
    prelude::*, px,
};
use tokio::sync::mpsc;
use tracing::warn;

mod kept;
mod lists;
pub(crate) mod mention;

pub use lists::Later;
mod replies;
mod turns;

use crate::{
    fonts,
    hub::{self, Slot, Stage},
    input::{Submitted, SubmittedAside, TextInput},
    registry::{self, Entry, Registry},
    session::{Item, Session, State},
    settings::{self, Settings},
    style::BODY,
    theme,
};

/// The side columns' widths, the top bar's height.
pub const LEFT: f32 = 264.;
pub const RIGHT: f32 = 320.;
pub const TOPBAR: f32 = 44.;
/// Room the window's lights take at the left of the top row.
const LIGHTS: f32 = 76.;

/// What the middle shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// The session on screen, by its key; or, with none, the empty start.
    Thread,
    /// An agent's page: what it is, what it needs, how to install it.
    Agent(String),
    Settings,
}

/// Which changes the right column shows: one turn's, or all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Turn(usize),
    All,
}

/// What the right column shows of the session on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aside {
    /// What it has come to: changes, plan, commands, context.
    Session,
    /// Every file it touched.
    Files,
    /// One file's changes, whole.
    File(PathBuf),
}

/// A small menu open over the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Closed,
    /// The session's modes.
    Mode,
    /// One of the session's options (a model, a level of thought), by id.
    Option(String),
    /// The agent a draft is begun with.
    Agent,
    /// What can be done with a session in the sidebar, by key.
    Session(u64),
    /// The project the sidebar shows, when it lists by status.
    Only,
}

/// Which side columns show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub left: bool,
    pub right: bool,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about the window, not a state among several"
)]
pub struct Desk {
    pub home: PathBuf,
    pub settings: Settings,
    /// Why the settings could not be read, while that is so: nothing is
    /// saved over them.
    pub settings_refused: Option<String>,
    /// Why the sessions kept from before could not be read, while that is
    /// so: nothing is saved over them.
    pub store_refused: Option<String>,
    /// The session being renamed in the sidebar, and the line for it.
    pub renaming: Option<u64>,
    pub rename: Entity<TextInput>,
    /// What was just deleted or archived, while it can be taken back.
    pub undo: Option<lists::Undo>,
    /// The sessions whose records hold what is searched for.
    pub found: HashSet<u64>,
    /// The settings page's tab.
    pub settings_tab: crate::pages::Tab,
    /// The draft on screen goes to every agent that can run, once sent.
    pub fanout: bool,
    /// No window of Ghost's is in front.
    pub behind: bool,
    /// What `@` can name: each folder's files, each project's open pull
    /// requests, read once when first wanted.
    pub files: HashMap<PathBuf, Vec<String>>,
    pub prs: HashMap<PathBuf, Vec<(u64, String)>>,
    /// What was named with `@` and images put in, waiting to be sent.
    pub mentions: Vec<mention::Mention>,
    pub pictures: Vec<mention::Picture>,
    pub registry: Registry,
    /// The last registry read failed: why.
    pub registry_trouble: Option<String>,
    /// The agents switched on, in the order listed.
    pub slots: Vec<Slot>,
    pub sessions: Vec<Session>,
    /// The session on screen, by key.
    pub current: Option<u64>,
    next_key: u64,
    pub page: Page,
    pub popup: Popup,
    pub panes: Panes,
    pub input: Entity<TextInput>,
    /// Cards and lines opened in the thread, by key.
    pub expanded: HashSet<String>,
    /// Projects folded shut down the left.
    pub closed: HashSet<PathBuf>,
    pub thread_scroll: ScrollHandle,
    /// The agent being signed in to, while it is.
    pub signing: Option<String>,
    /// What the right column shows.
    pub aside: Aside,
    /// The search over sessions, and whether it is open.
    pub search: Entity<TextInput>,
    pub searching: bool,
    /// When the registry was last read.
    pub registry_read: Option<std::time::SystemTime>,
    /// Which changes the right column shows.
    pub scope: Scope,
    /// The commit message being written, and whether its form is open.
    pub message: Entity<TextInput>,
    pub committing: bool,
    tx: hub::Sender,
}

/// A strip dragging which moves the window; a double click zooms it.
pub fn draggable() -> gpui::Div {
    div().on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 2 {
            window.titlebar_double_click();
        } else {
            window.start_window_move();
        }
    })
}

/// The room to leave for the window's lights; none in full screen.
pub fn lights_room(window: &Window) -> f32 {
    if window.is_fullscreen() { 12. } else { LIGHTS }
}

impl Desk {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let home = settings::home();
        let (settings, settings_refused) = Settings::load(&home);
        let input = cx.new(|cx| {
            TextInput::new(cx, "Ask for anything, or give a task")
                .wrapping(8)
                .padded(5.)
        });
        cx.subscribe(&input, |this, _, Submitted(text), cx| this.send(text, cx))
            .detach();
        cx.subscribe_in(
            &input,
            window,
            |this, _, SubmittedAside(text), window, cx| {
                this.send_aside(text, window, cx);
            },
        )
        .detach();
        cx.subscribe(&input, |this, input, _: &crate::input::Changed, cx| {
            if input.read(cx).text().contains('@') {
                this.want_offers();
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, _: &crate::input::Complete, cx| {
            this.take_offer(0, cx);
        })
        .detach();
        cx.subscribe(&input, |this, _, crate::input::PastedImage(image), cx| {
            let mime = match image.format {
                gpui::ImageFormat::Png => "image/png",
                gpui::ImageFormat::Jpeg => "image/jpeg",
                gpui::ImageFormat::Gif => "image/gif",
                gpui::ImageFormat::Webp => "image/webp",
                _ => return,
            };
            this.add_picture(
                mention::Picture {
                    mime: mime.into(),
                    bytes: image.bytes.clone(),
                },
                cx,
            );
        })
        .detach();
        let search = cx.new(|cx| TextInput::new(cx, "Search sessions").padded(4.));
        let message = cx.new(|cx| TextInput::new(cx, "Commit message").padded(4.));
        let rename = cx.new(|cx| TextInput::new(cx, "Name").padded(2.));
        cx.subscribe(&rename, |this, _, Submitted(text), cx| {
            this.finish_rename(text, cx);
        })
        .detach();
        cx.subscribe(&rename, |this, _, _: &crate::input::Cancelled, cx| {
            this.cancel_rename(cx);
        })
        .detach();
        cx.subscribe(&search, |this, _, _: &crate::input::Changed, cx| {
            this.search_records(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |this, _, _: &crate::input::Cancelled, cx| {
            this.searching = false;
            cx.notify();
        })
        .detach();

        let (tx, mut rx) = mpsc::unbounded_channel();
        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.recv().await {
                if this.update(cx, |this, cx| this.take(msg, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // While something goes on, draw again a few times a second, so that
        // what breathes breathes and the time beside a turn counts.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let Ok(()) = this.update(cx, |this, cx| {
                    if this.busy() {
                        cx.notify();
                    }
                }) else {
                    break;
                };
            }
        })
        .detach();

        // Each minute, let go of agents idle long enough.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if this.update(cx, Self::release_idle).is_err() {
                    break;
                }
            }
        })
        .detach();

        let registry = registry::kept(&home).unwrap_or_default();
        hub::fetch(home.clone(), tx.clone());
        let mut desk = Self {
            home,
            settings,
            settings_refused,
            store_refused: None,
            renaming: None,
            rename,
            undo: None,
            found: HashSet::new(),
            settings_tab: crate::pages::Tab::Agents,
            fanout: false,
            behind: false,
            files: HashMap::new(),
            prs: HashMap::new(),
            mentions: Vec::new(),
            pictures: Vec::new(),
            registry,
            registry_trouble: None,
            slots: Vec::new(),
            sessions: Vec::new(),
            current: None,
            next_key: 1,
            page: Page::Thread,
            popup: Popup::Closed,
            panes: Panes {
                left: true,
                right: true,
            },
            input,
            expanded: HashSet::new(),
            closed: HashSet::new(),
            thread_scroll: ScrollHandle::new(),
            signing: None,
            aside: Aside::Session,
            search,
            searching: false,
            registry_read: None,
            scope: Scope::All,
            message,
            committing: false,
            tx,
        };
        desk.keep_from(crate::store::load(&desk.home));
        desk.settle_idle();
        Self::keep_on_quit(cx);
        desk.fill_slots();
        desk
    }

    /// Whether anything is going on that is drawn as it goes.
    fn busy(&self) -> bool {
        self.sessions
            .iter()
            .any(|s| matches!(s.state, State::Working | State::Opening))
            || self
                .slots
                .iter()
                .any(|s| matches!(s.stage, Stage::Installing(_) | Stage::Starting))
    }

    /// Make the slots match the agents switched on, keeping those already
    /// there as they are.
    fn fill_slots(&mut self) {
        let mut old: Vec<Slot> = std::mem::take(&mut self.slots);
        for id in &self.settings.enabled {
            if let Some(at) = old.iter().position(|s| &s.entry.id == id) {
                self.slots.push(old.remove(at));
            } else if let Some(entry) = self.entry(id) {
                self.slots.push(Slot::new(entry.clone(), &self.home));
            }
        }
        self.start_ready();
    }

    /// Start each agent switched on that can run and is not running, so
    /// that the sessions it keeps are listed.
    fn start_ready(&mut self) {
        let home = self.home.clone();
        for slot in &mut self.slots {
            if matches!(slot.stage, Stage::Ready) {
                slot.stage = Stage::Starting;
                hub::start(&slot.entry, &home, self.tx.clone());
            }
        }
    }

    /// Ask `agent`, if it is running and can list them, for the sessions
    /// it keeps in the projects.
    pub(crate) fn list_sessions(&self, agent: &str) {
        let Some(Stage::Running {
            agent: running,
            init,
        }) = self.slot(agent).map(|s| &s.stage)
        else {
            return;
        };
        if init.agent_capabilities.session_capabilities.list.is_none() {
            return;
        }
        hub::list(
            running.clone(),
            agent.to_owned(),
            self.settings.folders.clone(),
            self.home.clone(),
            self.tx.clone(),
        );
    }

    /// Read what a session of `agent` offers before one is opened, and
    /// whether it is signed in, by opening one and deleting it again.
    pub(crate) fn probe_agent(&self, agent: &str) {
        let Some(slot) = self.slot(agent) else { return };
        let Stage::Running {
            agent: running,
            init,
        } = &slot.stage
        else {
            return;
        };
        let caps = &init.agent_capabilities.session_capabilities;
        hub::probe(
            running.clone(),
            agent.to_owned(),
            self.home.clone(),
            caps.delete.is_some(),
            caps.close.is_some(),
            self.tx.clone(),
        );
    }

    /// Sign in to `agent` the way it offers: its own method when it lists
    /// one, else its own program in a terminal, where it signs in.
    pub fn sign_in_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        use agent_client_protocol_schema::v1::AuthMethod;
        let Some(slot) = self.slot(agent) else { return };
        if let Stage::Running { init, .. } = &slot.stage
            && let Some(method) = init.auth_methods.iter().find_map(|m| match m {
                AuthMethod::Agent(a) => Some(a.id.0.to_string()),
                AuthMethod::Terminal(t) => Some(t.id.0.to_string()),
                _ => None,
            })
        {
            self.sign_in(agent, &method, cx);
            return;
        }
        match slot.entry.own_cli(&self.home) {
            Some(cli) => {
                if let Err(e) = hub::in_terminal(&cli, &[], &[]) {
                    tracing::warn!("a terminal could not be opened: {e}");
                }
            }
            None => {
                if let Some(site) = &slot.entry.website {
                    cx.open_url(site);
                }
            }
        }
    }

    /// Read again whether `agent` is signed in.
    pub fn recheck_auth(&mut self, agent: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.slot_mut(agent) {
            slot.auth = hub::Auth::Unknown;
        }
        self.probe_agent(agent);
        cx.notify();
    }

    /// Sign `agent` out, when it can be.
    pub fn log_out(&mut self, agent: &str, cx: &mut Context<Self>) {
        if let Some(running) = self.slot(agent).and_then(Slot::running) {
            hub::logout(running.clone(), agent.to_owned(), self.tx.clone());
        }
        cx.notify();
    }

    /// Whether `agent` can delete the sessions it keeps.
    pub fn can_delete(&self, agent: &str) -> bool {
        matches!(self.slot(agent).map(|s| &s.stage),
            Some(Stage::Running { init, .. }) if init.agent_capabilities.session_capabilities.delete.is_some())
    }

    /// A key no session has had.
    pub(crate) fn take_key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key - 1
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.registry.agents.iter().find(|e| e.id == id)
    }

    pub fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.entry.id == id)
    }

    fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.entry.id == id)
    }

    pub fn session(&self) -> Option<&Session> {
        let key = self.current?;
        self.sessions.iter().find(|s| s.key == key)
    }

    fn session_mut(&mut self, key: u64) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.key == key)
    }

    fn save_settings(&mut self) {
        if self.settings_refused.is_some() {
            return;
        }
        if let Err(e) = self.settings.save(&self.home) {
            warn!("the settings could not be saved: {e}");
        }
    }

    /// The project a new session starts in: that of the session on
    /// screen, else the first, else home.
    pub fn project(&self) -> PathBuf {
        self.session()
            .map(|s| s.project.clone())
            .or_else(|| self.settings.folders.first().cloned())
            .unwrap_or_else(|| {
                std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
            })
    }

    /// The projects down the left: those kept, then the folder of any
    /// session in none of them.
    pub fn projects(&self) -> Vec<PathBuf> {
        let mut out = self.settings.folders.clone();
        for s in &self.sessions {
            if !out.contains(&s.project) {
                out.push(s.project.clone());
            }
        }
        out
    }

    // What you do.

    /// Begin a session with `agent` in the project `folder`. Nothing is
    /// asked of the agent until you say something.
    pub fn new_session(
        &mut self,
        agent: &str,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            self.slot(agent).map(|s| &s.stage),
            Some(Stage::Ready | Stage::Starting | Stage::Running { .. })
        ) {
            self.page = Page::Agent(agent.to_owned());
            cx.notify();
            return;
        }
        // A session begun and not used yet is reused, not doubled.
        let key = if let Some(s) = self
            .sessions
            .iter()
            .find(|s| s.agent == agent && s.project == folder && s.state == State::Draft)
        {
            s.key
        } else {
            let key = self.take_key();
            let mut s = Session::new(key, agent, folder);
            s.wants_worktree = s.repo && self.settings.worktree;
            if let Some(slot) = self.slot(agent) {
                slot.seed(&mut s);
            }
            self.sessions.push(s);
            key
        };
        self.show(key, window, cx);
    }

    /// Put the session `key` on screen.
    pub fn show(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.current != Some(key) {
            // What was being written stays with its session, and the
            // session shown brings back what was being written in it.
            let typed = self.input.read(cx).text();
            if let Some(old) = self.current
                && let Some(s) = self.session_mut(old)
            {
                s.unsent = typed;
            }
            let unsent = self
                .sessions
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.unsent.clone())
                .unwrap_or_default();
            self.input.update(cx, |i, cx| i.set_text(&unsent, cx));
            self.aside = Aside::Session;
            self.scope = Scope::All;
            self.committing = false;
        }
        self.current = Some(key);
        self.page = Page::Thread;
        self.popup = Popup::Closed;
        if let Some(s) = self.session_mut(key)
            && s.flags.unread
        {
            s.flags.unread = false;
            self.save_index();
        }
        let closed = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .is_some_and(|s| s.state == State::Closed && !s.recorded);
        // A session with a record reads from it at once, and its agent is
        // asked only when you write; one without comes from its agent.
        if closed
            && !self.read_record(key)
            && let Some(s) = self.session_mut(key)
        {
            s.items.clear();
            s.plan = None;
            s.turns.clear();
            s.total = None;
            s.redo = None;
            s.state = State::Opening;
            let agent = s.agent.clone();
            self.open_when_running(&agent, key);
        }
        self.thread_scroll.scroll_to_bottom();
        window.focus(&self.input.focus_handle(cx));
        cx.notify();
    }

    /// Begin a session in the project on screen.
    pub fn new_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.project();
        self.new_in(project, window, cx);
    }

    /// Begin a session in `folder`, with the agent of the session on screen
    /// if it can run, else the first that can.
    pub fn new_in(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let usable = |s: &Slot| {
            matches!(
                s.stage,
                Stage::Ready | Stage::Starting | Stage::Running { .. }
            )
        };
        let agent = self
            .session()
            .and_then(|s| self.slot(&s.agent))
            .filter(|s| usable(s))
            .or_else(|| self.slots.iter().find(|s| usable(s)))
            .or_else(|| self.slots.first())
            .map(|s| s.entry.id.clone());
        match agent {
            Some(agent) => self.new_session(&agent, folder, window, cx),
            None => self.open_page(Page::Settings, cx),
        }
    }

    /// Begin the draft on screen with another agent.
    pub fn switch_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        if let Some(key) = self.current
            && let Some(s) = self.session_mut(key)
            && s.state == State::Draft
        {
            let wants = s.wants_worktree;
            *s = Session::new(key, agent, s.folder.clone());
            s.wants_worktree = wants && s.repo;
        }
        if let Some(key) = self.current {
            let slot = self.slots.iter().find(|x| x.entry.id == agent);
            if let (Some(slot), Some(s)) = (slot, self.sessions.iter_mut().find(|s| s.key == key)) {
                slot.seed(s);
            }
        }
        cx.notify();
    }

    /// Open or close the search over sessions.
    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = !self.searching;
        if self.searching {
            window.focus(&self.search.focus_handle(cx));
        } else {
            self.search.update(cx, |s, cx| s.set_text("", cx));
        }
        cx.notify();
    }

    /// Show `aside` in the right column.
    pub fn show_aside(&mut self, aside: Aside, cx: &mut Context<Self>) {
        self.aside = aside;
        self.panes.right = true;
        cx.notify();
    }

    /// Ask for a folder to add as a project, and begin a session in it.
    pub fn add_project(cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.settings.used(&folder);
                    this.save_settings();
                    let agents: Vec<String> =
                        this.slots.iter().map(|s| s.entry.id.clone()).collect();
                    for agent in agents {
                        this.list_sessions(&agent);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Take the session `key` off the desk, and the worktree made for it.
    pub(crate) fn forget_session(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(at) = self.sessions.iter().position(|s| s.key == key) else {
            return;
        };
        if let Some(id) = &self.sessions[at].id {
            let path = crate::store::record_path(&self.home, &self.sessions[at].agent, &id.0);
            crate::store::queue(crate::store::Job::Remove(path));
        }
        let s = self.sessions.remove(at);
        self.save_index();
        if let Some(wt) = s.worktree {
            hub::git_action(
                wt.repo.clone(),
                hub::GitAction::RemoveWorktree(wt),
                key,
                self.tx.clone(),
            );
        }
        if self.current == Some(key) {
            self.current = None;
        }
        cx.notify();
    }

    /// Take `folder` off the projects; one with sessions open stays.
    pub fn forget_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        if self.sessions.iter().any(|s| s.project == folder) {
            return;
        }
        self.settings.forget(folder);
        self.closed.remove(folder);
        self.save_settings();
        cx.notify();
    }

    /// Fold the project `folder` shut, or open it.
    pub fn toggle_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        if !self.closed.remove(folder) {
            self.closed.insert(folder.to_path_buf());
        }
        cx.notify();
    }

    /// Begin again where the session on screen left off: a draft with the
    /// same agent, in the same project.
    pub fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (agent, folder) = (s.agent.clone(), s.project.clone());
        let key = self.take_key();
        self.sessions.push(Session::new(key, &agent, folder));
        self.show(key, window, cx);
    }

    /// Say `text` in the session on screen, as if typed.
    pub fn say(&mut self, text: &str, cx: &mut Context<Self>) {
        self.send(text, cx);
    }

    /// Ask for the folder the draft on screen works in.
    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Work here".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    if let Some(s) = this.session_mut(key)
                        && s.state == State::Draft
                    {
                        let wants = s.wants_worktree;
                        *s = Session::new(key, &s.agent.clone(), folder.clone());
                        s.wants_worktree = wants && s.repo;
                    }
                    this.settings.used(&folder);
                    this.save_settings();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Say `text` in the session on screen; a draft set to go to every
    /// agent goes to each of them, a session of its own for each.
    fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let draft = self
            .session()
            .is_some_and(|s| s.state == State::Draft && s.items.is_empty());
        self.send_in(key, text, cx);
        if draft && self.fanout {
            self.fanout = false;
            self.fan_out(key, text, cx);
        }
    }

    /// Say `text` and leave it to go on: a new session is begun beside it,
    /// in the same project, to write the next thing in (⌘Return).
    fn send_aside(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.send(text, cx);
        let project = self.project();
        self.new_in(project, window, cx);
    }

    /// Begin, beside the session `key`, a session with each other agent
    /// that can run, in the same project, and say `text` in each.
    fn fan_out(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        let Some(origin) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        let (agent, folder) = (origin.agent.clone(), origin.project.clone());
        let others: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.entry.id != agent)
            .filter(|s| {
                matches!(
                    s.stage,
                    Stage::Ready | Stage::Starting | Stage::Running { .. }
                )
            })
            .map(|s| s.entry.id.clone())
            .collect();
        for other in others {
            let new = self.take_key();
            let mut s = Session::new(new, &other, folder.clone());
            if let Some(slot) = self.slot(&other) {
                slot.seed(&mut s);
            }
            self.sessions.push(s);
            self.send_in(new, text, cx);
        }
    }

    /// Say `text` in the session `key`.
    fn send_in(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let on_screen = self.current == Some(key);
        let Some(session) = self.session_mut(key) else {
            return;
        };
        if session.state == State::Working || session.state == State::Opening {
            if !on_screen {
                return;
            }
            // Said while a turn goes on: it waits its turn, and is said as
            // the one going on ends.
            session.queued.push(text);
            self.save_index();
            cx.notify();
            return;
        }
        if session.state == State::Closed {
            // Its agent opens it again (resumed from the record, or loaded),
            // and then it goes.
            if !session.recorded {
                session.items.clear();
            }
            session.pending = Some(text);
            session.state = State::Opening;
            let agent = session.agent.clone();
            self.open_when_running(&agent, key);
            cx.notify();
            return;
        }
        let attached = if on_screen {
            self.attach(key, &text)
        } else {
            Vec::new()
        };
        let session = self.session_mut(key).expect("found above");
        session.unsent.clear();
        session.attached.extend(attached);
        // Something new said in a session done for now brings it back.
        session.flags.settled = None;
        session.flags.snoozed = None;
        session.said(&text);
        let (agent, folder, id) = (
            session.agent.clone(),
            session.folder.clone(),
            session.id.clone(),
        );
        if id.is_some() {
            self.begin_turn(key, text);
        } else {
            let session = self.session_mut(key).expect("on screen");
            session.state = State::Opening;
            session.pending = Some(text.clone());
            let worktree = session.wants_worktree && session.worktree.is_none();
            self.settings.used(&folder);
            self.save_settings();
            if worktree {
                hub::make_worktree(folder, text, self.home.clone(), key, self.tx.clone());
            } else {
                self.open_when_running(&agent, key);
            }
        }
        self.thread_scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Open the session `key` once its agent runs, starting it if need be.
    fn open_when_running(&mut self, agent: &str, key: u64) {
        let home = self.home.clone();
        let tx = self.tx.clone();
        let (folder, id, recorded) = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .map(|s| (s.folder.clone(), s.id.clone(), s.recorded))
            .unwrap_or_default();
        let Some(slot) = self.slot_mut(agent) else {
            return;
        };
        match &slot.stage {
            Stage::Running { agent, init } => match id {
                Some(id)
                    if recorded
                        && init
                            .agent_capabilities
                            .session_capabilities
                            .resume
                            .is_some() =>
                {
                    hub::resume(agent.clone(), key, id, folder, tx);
                }
                Some(id) => {
                    let agent = agent.clone();
                    if recorded {
                        self.drop_record(key);
                    }
                    hub::load(agent, key, id, folder, tx);
                }
                None => hub::open(agent.clone(), key, folder, tx),
            },
            Stage::Starting => {}
            _ => {
                slot.stage = Stage::Starting;
                hub::start(&slot.entry, &home, tx);
            }
        }
    }

    /// Stop the turn going on in the session on screen.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        if let (Some(running), Some(id)) = (self.slot(&agent).and_then(Slot::running), id) {
            // Questions still open are answered as cancelled, as ACP asks.
            let open: Vec<_> = s
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Asked(a) if a.chosen.is_none() => Some(a.id.clone()),
                    _ => None,
                })
                .collect();
            for request in open {
                hub::answer(running, &request, None);
            }
            hub::cancel(running, id);
        }
        cx.notify();
    }

    /// Answer the question asked under `request` with the option `option`.
    pub fn choose(
        &mut self,
        request: &serde_json::Value,
        option: &agent_client_protocol_schema::v1::PermissionOption,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.current else { return };
        let Some(agent) = self.session().map(|s| s.agent.clone()) else {
            return;
        };
        if let Some(running) = self.slot(&agent).and_then(Slot::running) {
            hub::answer(running, request, Some(option.option_id.0.to_string()));
        }
        if let Some(s) = self.session_mut(key) {
            s.answered(request, option);
        }
        cx.notify();
    }

    /// Say the next thing queued in the session `key`, if one is.
    pub(crate) fn say_queued(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        if s.state != State::Idle || s.queued.is_empty() {
            return;
        }
        let text = s.queued.remove(0);
        self.send_in(key, &text, cx);
    }

    /// Say the queued `n` now: the turn going on is stopped, and it is
    /// said as soon as that has ended.
    pub fn steer(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        if let Some(s) = self.session_mut(key)
            && n < s.queued.len()
        {
            let text = s.queued.remove(n);
            s.queued.insert(0, text);
        }
        self.stop(cx);
    }

    /// Take the queued `n` back into the line.
    pub fn unqueue(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let Some(s) = self.session_mut(key) else {
            return;
        };
        if n >= s.queued.len() {
            return;
        }
        let text = s.queued.remove(n);
        self.input.update(cx, |i, cx| i.set_text(&text, cx));
        self.save_index();
        cx.notify();
    }

    /// Allow, with `option`, the call asked about under `request`, and every
    /// call of its kind for the rest of the session.
    pub fn allow_for_session(
        &mut self,
        request: &serde_json::Value,
        option: &agent_client_protocol_schema::v1::PermissionOption,
        kind: agent_client_protocol_schema::v1::ToolKind,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = self.current
            && let Some(s) = self.session_mut(key)
            && !s.trusted.contains(&kind)
        {
            s.trusted.push(kind);
        }
        self.choose(request, option, cx);
    }

    /// Put the session on screen in the mode `mode`.
    pub fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        if id.is_none() {
            self.session_mut(key).expect("on screen").choose_mode(&mode);
        } else if let (Some(running), Some(id)) =
            (self.slot(&agent).and_then(Slot::running).cloned(), id)
        {
            hub::set_mode(running, key, id, mode.clone(), self.tx.clone());
            if let Some(modes) = &mut self.session_mut(key).expect("on screen").modes {
                modes.current_mode_id = agent_client_protocol_schema::v1::SessionModeId::new(mode);
            }
        }
        cx.notify();
    }

    /// Choose `value` for the session's option `config`.
    pub fn set_option(&mut self, config: String, value: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        if id.is_none() {
            self.session_mut(key)
                .expect("on screen")
                .choose(&config, &value);
        } else if let (Some(running), Some(id)) =
            (self.slot(&agent).and_then(Slot::running).cloned(), id)
        {
            hub::set_option(running, key, id, config, value, self.tx.clone());
        }
        cx.notify();
    }

    pub fn toggle_popup(&mut self, popup: Popup, cx: &mut Context<Self>) {
        self.popup = if self.popup == popup {
            Popup::Closed
        } else {
            popup
        };
        cx.notify();
    }

    /// Open or close the card, line or list under `key`.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.panes.left = !self.panes.left;
        cx.notify();
    }

    pub fn toggle_aside(&mut self, cx: &mut Context<Self>) {
        self.panes.right = !self.panes.right;
        cx.notify();
    }

    pub fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.popup = Popup::Closed;
        cx.notify();
    }

    /// Install the agent `id` from its archive, or update it to the
    /// registry's version.
    pub fn install(&mut self, id: &str, cx: &mut Context<Self>) {
        let home = self.home.clone();
        let tx = self.tx.clone();
        if let Some(slot) = self.slot_mut(id) {
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            slot.stage = Stage::Installing(hub::Install {
                what: "Starting".into(),
                share: None,
                stop: stop.clone(),
            });
            hub::install(slot.entry.clone(), home, stop, tx);
        }
        cx.notify();
    }

    /// Stop the install of `id` going on; nothing of it is kept.
    pub fn cancel_install(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(Stage::Installing(i)) = self.slot(id).map(|s| &s.stage) {
            i.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            cx.notify();
        }
    }

    /// Take the agent `id`'s download off this machine, once you say so;
    /// it stops first if it is running.
    pub fn uninstall(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        let name = self.name_of(id);
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Uninstall {name}?"),
            Some(&format!(
                "Its download in {} is removed. Its sessions and sign-in stay with {name}, and it can be installed again.",
                Desk::shown(&self.home.join("agents").join(id))
            )),
            &["Uninstall", "Cancel"],
            cx,
        );
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    let home = this.home.clone();
                    if let Err(why) = entry.uninstall(&home) {
                        if let Some(slot) = this.slot_mut(&id) {
                            slot.stage = Stage::Failed(why);
                        }
                    } else if let Some(slot) = this.slot_mut(&id) {
                        *slot = Slot::new(entry.clone(), &home);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Look again for what an agent needs (a runner installed meanwhile).
    pub fn recheck(&mut self, id: &str, cx: &mut Context<Self>) {
        let home = self.home.clone();
        if let Some(slot) = self.slot_mut(id) {
            *slot = Slot::new(slot.entry.clone(), &home);
        }
        cx.notify();
    }

    /// Switch the agent `id` on or off.
    pub fn toggle_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        self.settings.toggle(id);
        self.save_settings();
        self.fill_slots();
        cx.notify();
    }

    /// Read the registry again.
    pub fn refresh_registry(&mut self, cx: &mut Context<Self>) {
        hub::fetch(self.home.clone(), self.tx.clone());
        self.registry_trouble = None;
        cx.notify();
    }

    /// Keep the newest line in sight while the thread grows, unless you
    /// scrolled up to read.
    fn thread_scroll_follow(&self) {
        let offset = self.thread_scroll.offset().y;
        let max = self.thread_scroll.max_offset().height;
        if f32::from(-offset) >= f32::from(max) - 80. {
            self.thread_scroll.scroll_to_bottom();
        }
    }

    pub fn home_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    /// A path as a person writes it.
    pub fn shown(path: &Path) -> String {
        crate::ui::short_path(path, Self::home_path().as_deref())
    }
}

impl Render for Desk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = fonts::faces(cx).clone();
        let lights = lights_room(window);
        let middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(self.topbar(&faces, lights, cx));
        let middle = match self.page.clone() {
            Page::Thread => {
                let pane = f32::from(window.viewport_size().width)
                    - if self.panes.left { LEFT } else { 0. }
                    - if self.panes.right { RIGHT } else { 0. };
                let column = (pane - 64.).clamp(240., crate::style::COLUMN);
                middle
                    .child(self.thread(&faces, column, cx))
                    .child(self.composer(&faces, column, window, cx))
            }
            Page::Agent(id) => middle.child(self.agent_page(&faces, &id, cx)),
            Page::Settings => middle.child(self.settings_page(&faces, cx)),
        };
        let right = match (&self.page, self.panes.right) {
            (Page::Thread, true) if self.session().is_some() => Some(self.aside(&faces, cx)),
            (Page::Agent(id), _) => Some(self.about(&faces, id, cx)),
            _ => None,
        };
        fonts::with_fallbacks(div())
            .relative()
            .flex()
            .size_full()
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(BODY))
            .text_color(theme::text())
            .when(self.panes.left, |d| {
                d.child(self.sidebar(&faces, lights, cx))
            })
            .child(middle)
            .children(right)
            .children(self.undo_toast(&faces, cx))
    }
}
