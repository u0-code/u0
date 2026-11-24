//! The desk: the whole window, and everything it knows.
//!
//! Down the left, the agents switched on, each with its sessions under it,
//! and the desk's settings at the foot. In the middle, the session on
//! screen: its thread, and the line you type into, with the agent's own
//! choices (its mode, its model) under your hand. Down the right, what the
//! session has come to: its plan, the files it changed, how full its
//! context is. An agent that cannot run yet shows, in the middle, what it
//! needs and the button that gets it.
//!
//! This file holds the desk's state, what it does when an agent or you say
//! something, and how the window is laid out. The columns are drawn in
//! `sidebar.rs`, `thread.rs`, `aside.rs`, and the pages for an agent and
//! for the settings in `pages.rs`.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use agent_client_protocol_schema::v1::SessionUpdate;
use gpui::{
    Context, Entity, Focusable as _, MouseButton, PathPromptOptions, ScrollHandle, Window, div,
    prelude::*, px,
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    acp::Event,
    fonts,
    hub::{self, Msg, Slot, Stage},
    input::{Submitted, TextInput},
    registry::{self, Entry, Registry},
    session::{Item, Session, State},
    settings::{self, Settings},
    style::BODY,
    theme,
};

/// The side columns' widths, the top bar's height.
pub const LEFT: f32 = 264.;
pub const RIGHT: f32 = 320.;
pub const TOPBAR: f32 = 52.;
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

/// A small menu open over the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Closed,
    /// The session's modes.
    Mode,
    /// One of the session's options (a model, a level of thought), by id.
    Option(String),
}

/// Which side columns show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub left: bool,
    pub right: bool,
}

pub struct Desk {
    pub home: PathBuf,
    pub settings: Settings,
    /// Why the settings could not be read, while that is so: nothing is
    /// saved over them.
    pub settings_refused: Option<String>,
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
    pub thread_scroll: ScrollHandle,
    /// The agent being signed in to, while it is.
    pub signing: Option<String>,
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
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let home = settings::home();
        let (settings, settings_refused) = Settings::load(&home);
        let input = cx.new(|cx| {
            TextInput::new(cx, "Ask for anything, or give a task")
                .wrapping(8)
                .padded(5.)
        });
        cx.subscribe(&input, |this, _, Submitted(text), cx| this.send(text, cx))
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

        let registry = registry::kept(&home).unwrap_or_default();
        hub::fetch(home.clone(), tx.clone());
        let mut desk = Self {
            home,
            settings,
            settings_refused,
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
            thread_scroll: ScrollHandle::new(),
            signing: None,
            tx,
        };
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

    /// The folder a new session starts in: the last one used, else home.
    fn folder(&self) -> PathBuf {
        self.settings.folders.first().cloned().unwrap_or_else(|| {
            std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
        })
    }

    // What you do.

    /// Begin a session with `agent`, in the folder last used. Nothing is
    /// asked of the agent until you say something.
    pub fn new_session(&mut self, agent: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(
            self.slot(agent).map(|s| &s.stage),
            Some(Stage::Ready | Stage::Starting | Stage::Running { .. })
        ) {
            self.page = Page::Agent(agent.to_owned());
            cx.notify();
            return;
        }
        // A session begun and not used yet is reused, not doubled.
        let folder = self.folder();
        let key = if let Some(s) = self
            .sessions
            .iter()
            .find(|s| s.agent == agent && s.state == State::Draft)
        {
            s.key
        } else {
            let key = self.next_key;
            self.next_key += 1;
            self.sessions.push(Session::new(key, agent, folder));
            key
        };
        self.show(key, window, cx);
    }

    /// Put the session `key` on screen.
    pub fn show(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.current = Some(key);
        self.page = Page::Thread;
        self.popup = Popup::Closed;
        self.thread_scroll.scroll_to_bottom();
        window.focus(&self.input.focus_handle(cx));
        cx.notify();
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
                        *s = Session::new(key, &s.agent.clone(), folder.clone());
                    }
                    this.settings.used(&folder);
                    this.save_settings();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Say `text` in the session on screen.
    fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim().to_owned();
        let Some(key) = self.current else { return };
        if text.is_empty() {
            return;
        }
        let Some(session) = self.session_mut(key) else {
            return;
        };
        if session.state == State::Working || session.state == State::Opening {
            // Not taken while a turn goes on: it stays in the line.
            self.input.update(cx, |input, cx| input.set_text(&text, cx));
            return;
        }
        let session = self.session_mut(key).expect("the session on screen");
        session.said(&text);
        let (agent, folder, id) = (
            session.agent.clone(),
            session.folder.clone(),
            session.id.clone(),
        );
        if let Some(id) = id {
            if let Some(running) = self.slot(&agent).and_then(Slot::running).cloned() {
                hub::prompt(running, key, id, text, self.tx.clone());
            } else {
                self.session_mut(key).expect("on screen").ended(Err(
                    "The agent is not running; start a new session with it.".into(),
                ));
            }
        } else {
            let session = self.session_mut(key).expect("on screen");
            session.state = State::Opening;
            session.pending = Some(text);
            self.settings.used(&folder);
            self.save_settings();
            self.open_when_running(&agent, key);
        }
        self.thread_scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Open the session `key` once its agent runs, starting it if need be.
    fn open_when_running(&mut self, agent: &str, key: u64) {
        let home = self.home.clone();
        let tx = self.tx.clone();
        let folder = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.folder.clone())
            .unwrap_or_default();
        let Some(slot) = self.slot_mut(agent) else {
            return;
        };
        match &slot.stage {
            Stage::Running { agent, .. } => {
                hub::open(agent.clone(), key, folder, tx);
            }
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
        option: String,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.current else { return };
        let Some(agent) = self.session().map(|s| s.agent.clone()) else {
            return;
        };
        if let Some(running) = self.slot(&agent).and_then(Slot::running) {
            hub::answer(running, request, Some(option));
        }
        if let Some(s) = self.session_mut(key) {
            s.answered(request, name);
        }
        cx.notify();
    }

    /// Put the session on screen in the mode `mode`.
    pub fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        if let (Some(running), Some(id)) = (self.slot(&agent).and_then(Slot::running).cloned(), id)
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
        if let (Some(running), Some(id)) = (self.slot(&agent).and_then(Slot::running).cloned(), id)
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

    /// Install the agent `id` from its archive.
    pub fn install(&mut self, id: &str, cx: &mut Context<Self>) {
        let home = self.home.clone();
        let tx = self.tx.clone();
        if let Some(slot) = self.slot_mut(id) {
            slot.stage = Stage::Installing("Starting".into());
            hub::install(slot.entry.clone(), home, tx);
        }
        cx.notify();
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

    /// Sign in to `agent` with its method `method`.
    pub fn sign_in(&mut self, agent: &str, method: &str, cx: &mut Context<Self>) {
        use agent_client_protocol_schema::v1::AuthMethod;
        let Some(slot) = self.slot(agent) else { return };
        let Stage::Running {
            agent: running,
            init,
        } = &slot.stage
        else {
            return;
        };
        let terminal = init.auth_methods.iter().find_map(|m| match m {
            AuthMethod::Terminal(t) if &*t.id.0 == method => Some(t.clone()),
            _ => None,
        });
        let running = running.clone();
        let launch = slot.entry.launch(&self.home);
        if let Some(t) = terminal {
            // A login typed in a terminal: once it is done there, opening
            // the session is tried again from its card.
            if let Ok(mut launch) = launch {
                launch.args.extend(t.args);
                launch.env.extend(t.env);
                if let Err(e) = hub::in_terminal(&launch.program, &launch.args, &launch.env) {
                    self.note_sessions(agent, &format!("A terminal could not be opened: {e}"));
                }
            }
        } else {
            self.signing = Some(agent.to_owned());
            hub::authenticate(
                running,
                agent.to_owned(),
                method.to_owned(),
                self.tx.clone(),
            );
        }
        cx.notify();
    }

    /// Try again to open the sessions of `agent` that wait to be signed in.
    pub fn retry_open(&mut self, agent: &str, cx: &mut Context<Self>) {
        let keys: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| s.agent == agent && s.state == State::SignIn)
            .map(|s| s.key)
            .collect();
        for key in keys {
            if let Some(s) = self.session_mut(key) {
                s.state = State::Opening;
            }
            self.open_when_running(agent, key);
        }
        cx.notify();
    }

    fn signed(&mut self, agent: &str, result: Result<(), String>) {
        self.signing = None;
        match result {
            Ok(()) => {
                let keys: Vec<u64> = self
                    .sessions
                    .iter()
                    .filter(|s| s.agent == agent && s.state == State::SignIn)
                    .map(|s| s.key)
                    .collect();
                for key in keys {
                    if let Some(s) = self.session_mut(key) {
                        s.state = State::Opening;
                    }
                    self.open_when_running(agent, key);
                }
            }
            Err(why) => self.note_sessions(agent, &format!("Signing in failed: {why}")),
        }
    }

    /// Put a line of trouble in each session of `agent` waiting to sign in.
    fn note_sessions(&mut self, agent: &str, text: &str) {
        for s in self
            .sessions
            .iter_mut()
            .filter(|s| s.agent == agent && s.state == State::SignIn)
        {
            s.items.push(Item::Note {
                text: text.to_owned(),
                trouble: true,
            });
        }
    }

    // What comes back.

    fn take(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Registry(Ok(registry)) => {
                self.registry = registry;
                self.registry_trouble = None;
                // A newer version of an agent replaces its entry; one
                // running keeps running as it is.
                for slot in &mut self.slots {
                    if let Some(e) = self.registry.agents.iter().find(|e| e.id == slot.entry.id) {
                        let stage = match &slot.stage {
                            Stage::Missing(_) | Stage::Ready | Stage::Failed(_) => {
                                Slot::new(e.clone(), &self.home).stage
                            }
                            other => other.clone(),
                        };
                        *slot = Slot {
                            entry: e.clone(),
                            stage,
                        };
                    }
                }
                self.fill_slots();
            }
            Msg::Registry(Err(why)) => self.registry_trouble = Some(why),
            Msg::Installing { agent, stage } => {
                if let Some(slot) = self.slot_mut(&agent) {
                    slot.stage = Stage::Installing(stage);
                }
            }
            Msg::Installed { agent, result } => {
                let home = self.home.clone();
                if let Some(slot) = self.slot_mut(&agent) {
                    slot.stage = match result {
                        Ok(()) => Slot::new(slot.entry.clone(), &home).stage,
                        Err(why) => Stage::Failed(why),
                    };
                }
            }
            Msg::Started { agent, result } => self.started(&agent, result),
            Msg::Opened { key, result } => self.opened(key, result),
            Msg::Ended { key, result } => {
                if let Some(s) = self.session_mut(key) {
                    s.ended(result);
                }
            }
            Msg::Said { agent, event } => self.said(&agent, *event),
            Msg::Signed { agent, result } => self.signed(&agent, result),
            Msg::Options { key, options } => {
                if let Some(s) = self.session_mut(key) {
                    s.options = options;
                }
            }
            Msg::Refused { key, why } => {
                if let Some(s) = self.session_mut(key) {
                    s.items.push(Item::Note {
                        text: why,
                        trouble: true,
                    });
                }
            }
        }
        cx.notify();
    }

    fn started(
        &mut self,
        agent: &str,
        result: Result<
            (
                std::sync::Arc<crate::acp::Agent>,
                Box<agent_client_protocol_schema::v1::InitializeResponse>,
            ),
            crate::acp::Failure,
        >,
    ) {
        let waiting: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| s.agent == agent && s.state == State::Opening && s.id.is_none())
            .map(|s| s.key)
            .collect();
        match result {
            Ok((running, init)) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Running {
                        agent: running,
                        init,
                    };
                }
                for key in waiting {
                    self.open_when_running(agent, key);
                }
            }
            Err(f) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Failed(f.message.clone());
                }
                for key in waiting {
                    if let Some(s) = self.session_mut(key) {
                        s.state = State::Broken(f.message.clone());
                    }
                }
            }
        }
    }

    fn opened(
        &mut self,
        key: u64,
        result: Result<
            Box<agent_client_protocol_schema::v1::NewSessionResponse>,
            crate::acp::Failure,
        >,
    ) {
        let Some(s) = self.session_mut(key) else {
            return;
        };
        match result {
            Ok(r) => {
                s.id = Some(r.session_id.clone());
                s.modes = r.modes;
                s.options = r.config_options.unwrap_or_default();
                s.state = State::Idle;
                if let Some(text) = s.pending.take() {
                    s.state = State::Working;
                    let agent = s.agent.clone();
                    if let Some(running) = self.slot(&agent).and_then(Slot::running).cloned() {
                        hub::prompt(running, key, r.session_id, text, self.tx.clone());
                    }
                }
            }
            // The text waits: once signed in, the session opens and it goes.
            Err(f) if f.code == Some(crate::acp::AUTH_REQUIRED) => s.state = State::SignIn,
            Err(f) => {
                s.pending = None;
                s.since = None;
                s.state = State::Broken(f.message);
            }
        }
    }

    fn said(&mut self, agent: &str, event: Event) {
        match event {
            Event::Update(n) => {
                let Some(s) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.agent == agent && s.id.as_ref() == Some(&n.session_id))
                else {
                    return;
                };
                let title = matches!(n.update, SessionUpdate::SessionInfoUpdate(_));
                s.apply(n.update);
                if !title {
                    self.thread_scroll_follow();
                }
            }
            Event::Permission { id, request } => {
                if let Some(s) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.agent == agent && s.id.as_ref() == Some(&request.session_id))
                {
                    s.asked(id, request);
                    self.thread_scroll_follow();
                } else if let Some(running) = self.slot(agent).and_then(Slot::running) {
                    hub::answer(running, &id, None);
                }
            }
            Event::Exited(why) => {
                if let Some(slot) = self.slot_mut(agent) {
                    slot.stage = Stage::Failed(why.clone());
                }
                for s in self.sessions.iter_mut().filter(|s| s.agent == agent) {
                    if s.id.is_some() || s.state == State::Opening {
                        s.id = None;
                        s.pending = None;
                        s.since = None;
                        s.state = State::Broken(format!("The agent stopped: {why}"));
                    }
                }
            }
        }
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
                    .child(self.composer(&faces, window, cx))
            }
            Page::Agent(id) => middle.child(self.agent_page(&faces, &id, cx)),
            Page::Settings => middle.child(self.settings_page(&faces, cx)),
        };
        let show_right = self.panes.right && self.page == Page::Thread && self.session().is_some();
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
            .when(show_right, |d| d.child(self.aside(&faces, cx)))
    }
}
