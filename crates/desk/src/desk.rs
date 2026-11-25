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
//! laid out. The agents switched on are in `desk/agents.rs`; what you say
//! in a session, and stopping it, in `desk/send.rs`. A turn's checkpoints,
//! undo, worktrees and commits are in `desk/turns.rs`; what comes back
//! from agents and from work off the window's thread in `desk/replies.rs`. The columns are drawn in
//! `sidebar.rs`, `thread.rs`, `aside.rs`, and the pages for an agent and
//! for the settings in `pages.rs`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::component::input::{InputEvent, InputState, TextareaState};
use gpui::component::popover::Popover;
use gpui::component::{Selectable, WindowExt as _};
use gpui::{
    Context, Entity, Focusable as _, MouseButton, PathPromptOptions, SharedString, Window, div,
    prelude::*, px,
};
use tokio::sync::mpsc;

mod agents;
pub(crate) mod forge;
mod gitview;
mod kept;
pub(crate) mod lists;
pub(crate) mod mention;
pub(crate) mod notices;
pub(crate) mod palette;
pub(crate) mod projects;
pub(crate) mod rank;
pub(crate) mod runs;

pub use lists::{Later, Renaming};
mod replies;
mod send;
mod turns;
mod writing;

use crate::{
    fonts,
    hub::{self, Slot, Stage},
    proto::Call,
    registry::{self, Registry},
    session::{Session, State},
    settings::{self, Settings},
    style::BODY,
    theme,
};

/// How narrow the sidebar can be dragged, and how wide the middle stays
/// beside it: t3code's.
const LEFT_LEAST: f32 = 208.;
const MIDDLE_LEAST: f32 = 640.;
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
    /// A project's pull requests, or its issues.
    Forge(crate::github::Kind),
}

pub use crate::aside::{Column, Surface, Viewer};

/// A small menu open over the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Closed,
    /// The session's modes.
    Mode,
    /// The model picker: the agents, their models, the session's options.
    Model,
    /// The session's modes, from the model picker's foot.
    ModelMode,
    /// What can go with what you write: the composer's +.
    Add,
    /// The session's menu, under its title's caret.
    Title,
    /// The choices of one of the session's options, by id, beside it in
    /// the model picker.
    Option(String),
    /// The branches to check out, in the column beside.
    Branch,
    /// What you put aside from the composer, to take back.
    Stash,
    /// What is to be said later in the session, and when to say it.
    Later,
    /// The branch a new worktree begins from.
    Base,
    /// The branch the session's branch is compared with.
    Compare,
    /// How to take a session back to one of your messages: by key and item.
    Rewind(u64, usize),
    /// The projects the sidebar shows.
    Projects,
    /// The project a new session works in, from its chip.
    StartIn,
    /// The writer: an agent and its model, in the settings.
    Writer,
    /// What the right column can open as a tab.
    Surfaces,
    /// What a session's details show.
    Details,
    /// The recent commits, to see one's changes.
    Commits,
    /// The project the PRs or Issues page is of.
    ForgeProject,
    /// What can be done with the pull request or issue read.
    ForgeActions,
}

/// Which side columns show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub left: bool,
    pub right: bool,
    /// The right column takes the thread's place too.
    pub whole: bool,
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
    /// What is being renamed, and the line for it.
    pub renaming: Option<lists::Renaming>,
    pub rename: Entity<InputState>,
    /// A message open to choose words from: the session's key and its
    /// item, and the line it is in.
    pub reading: Option<(u64, usize)>,
    pub reader: Entity<TextareaState>,
    /// What is typed to narrow a long menu of choices (a model's).
    pub menu_search: Entity<InputState>,
    /// The session asked to stop, and when: past a while, it can be
    /// stopped by force.
    pub stopping: Option<(u64, std::time::Instant)>,
    /// Whether the window was last set blurred behind, for glass.
    glassed: bool,
    /// What sessions cost, by key, as their records say: read for the
    /// usage page.
    pub spent: std::collections::HashMap<u64, Vec<crate::store::Spend>>,
    /// The forms agents asked for, by the number each answer goes back
    /// under, as questions to be answered over the composer.
    pub forms:
        std::collections::HashMap<u64, Entity<gpui::component::questionnaire::QuestionnaireState>>,
    /// When quitting was last asked while agents worked.
    pub quit_asked: Option<std::time::Instant>,
    /// Each running agent's process and the memory its group holds (kB),
    /// as last weighed.
    pub weights: Vec<(String, u32, u64)>,
    /// What of each agent the registry lists is here, by its id.
    pub here: HashMap<String, crate::proto::Here>,
    /// The programs the desk offers that are on the PATH (`hub::on_path`).
    pub on_path: Vec<&'static str>,
    /// How many sessions need you, as the Dock last said.
    pub badge: usize,
    /// The sessions shown before the one on screen, and those gone back
    /// from; going back or ahead is under way.
    pub trail: Vec<u64>,
    pub ahead: Vec<u64>,
    pub trailing: bool,
    /// Ports brought here from another machine (ssh -L), each its tunnel.
    pub tunnels: Vec<(u16, std::process::Child)>,
    /// The sessions chosen with ⌘-click in the sidebar.
    pub chosen: std::collections::BTreeSet<u64>,
    /// The projects the sidebar shows; none chosen, all.
    pub filter: Vec<PathBuf>,
    /// Sessions deleted whose worktree you kept.
    pub keep_worktree: HashSet<u64>,
    /// The image looked at large: the session's key and its item.
    pub zoom: Option<(u64, usize)>,
    /// What was just deleted or archived, while it can be taken back.
    pub undo: Option<lists::Undo>,
    /// The session being taken back to one of your messages: the fork
    /// made for it takes its place.
    pub rewinding: Option<u64>,
    /// The session a side question is being branched off.
    pub asking_beside: Option<u64>,
    /// The subagents' tabs over the thread you closed: hidden, still at
    /// work.
    pub closed_tabs: HashSet<u64>,
    /// What waits for an agent to start, by agent: a fork, a delete.
    pub waiting: Vec<(String, lists::Waiting)>,
    /// Sessions deleted whose agent cannot delete them (see `store::Index`).
    pub gone: Vec<(String, String)>,
    /// Each session's meta as the server keeps it, by agent and id: what
    /// is sent is what differs.
    pub kept: HashMap<(String, agent_client_protocol::v1::schema::SessionId), crate::store::Meta>,
    /// What Ghost tells you at the foot of the window, the oldest first.
    pub notices: Vec<notices::Notice>,
    pub pages: crate::pages::Pages,
    pub adding: projects::Adding,
    /// No window of Ghost's is in front.
    pub behind: bool,
    /// What keeps the Mac awake while an agent works, when it is kept so.
    pub awake: Option<std::process::Child>,
    pub drawer: runs::Drawer,
    /// Minutes counted, for what is read every other one, or checked every
    /// five.
    pub pr_tick: u32,
    pub health_tick: u32,
    pub review: gitview::Review,
    pub palette: palette::Palette,
    /// The folders each agent has sessions in that are not projects.
    pub elsewhere: std::collections::BTreeMap<String, Vec<(PathBuf, usize)>>,
    /// What `@` can name: each folder's files, each project's open pull
    /// requests, read once when first wanted.
    pub files: HashMap<PathBuf, Vec<String>>,
    pub prs: HashMap<PathBuf, Vec<(u64, String)>>,
    pub issues: HashMap<PathBuf, Vec<(u64, String)>>,
    /// What each CLI of yours an agent runs says its version is.
    pub cli_versions: HashMap<PathBuf, String>,
    pub registry: Registry,
    /// The last registry read failed: why.
    pub registry_trouble: Option<String>,
    /// The agents switched on, in the order listed.
    pub slots: Vec<Slot>,
    pub sessions: crate::sessions::Sessions,
    /// The session on screen, by key.
    pub current: Option<u64>,
    pub page: Page,
    pub popup: Popup,
    pub picker: crate::thread::Picker,
    pub panes: Panes,
    pub composer: mention::Composer,
    /// Cards and lines opened in the thread, by key.
    pub expanded: HashSet<String>,
    /// Projects folded shut down the left.
    pub closed: HashSet<PathBuf>,
    /// The sidebar's list: its tree, the shape it was last given, the
    /// session open in it then, the sessions in the order it lists them,
    /// and how many settled sessions it shows.
    pub side: Entity<gpui::component::tree::TreeState>,
    pub side_shape: String,
    pub side_current: Option<u64>,
    pub side_order: Vec<u64>,
    pub settled_shown: usize,
    /// When ⌘ went down alone, and whether it has been held long enough
    /// for the sessions' numbers to show.
    pub command_down: Option<std::time::Instant>,
    pub numbers_shown: bool,
    /// What each project's folder holds, once read (see `read_projects`).
    pub held: HashMap<PathBuf, crate::repo::Held>,
    /// The thread as a list drawn where it is seen, its lines, and the
    /// session they are of; an item to bring into view once it is drawn.
    pub list: gpui::ListState,
    pub lines: Vec<crate::thread::Line>,
    pub lines_of: Option<u64>,
    pub reveal: Option<usize>,
    pub find: crate::thread::Find,
    /// The agent being signed in to, while it is.
    pub signing: Option<String>,
    /// The right column's tabs, by session.
    pub columns: HashMap<u64, Column>,
    /// The line a side chat in the column is asked in.
    pub side_line: Entity<InputState>,
    /// The Files tab and the file read in the column.
    pub viewer: Viewer,
    /// The PRs and Issues pages.
    pub forge: forge::Forge,
    /// How far the column's tabs are scrolled, to bring one opened into view.
    pub tab_scroll: gpui::ScrollHandle,
    /// gpui-kit's view of each edit opened in the thread, by its card.
    pub edits: std::cell::RefCell<HashMap<String, Entity<gpui::component::diff::DiffState>>>,
    pub search: lists::Search,
    /// When the registry was last read.
    pub registry_read: Option<std::time::SystemTime>,
    tx: hub::Sender,
    /// The way to the server, which runs the agents.
    transport: hub::Transport,
}

/// A dialog's view of the desk, drawn by `draw`. gpui-kit's dialog layer
/// is the window root's, drawn apart from the desk, so this view is drawn
/// again whenever the desk is.
pub struct Over {
    desk: Entity<Desk>,
    draw: fn(&mut Desk, &mut Window, &mut Context<Desk>) -> gpui::AnyElement,
}

impl Over {
    pub fn new(
        desk: Entity<Desk>,
        draw: fn(&mut Desk, &mut Window, &mut Context<Desk>) -> gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        Self { desk, draw }
    }
}

impl Render for Over {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draw = self.draw;
        self.desk.update(cx, |d, cx| draw(d, window, cx))
    }
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
        // The system's look changed: drawn again, for a look that follows it.
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();
        // Keys pressed while one is being recorded are its, not done.
        let me = cx.entity().downgrade();
        cx.intercept_keystrokes(move |e, window, cx| {
            let took = me
                .update(cx, |d, cx| {
                    d.record_key(&e.keystroke, cx)
                        || d.escape_closes(&e.keystroke, window, cx)
                        || d.palette_keys(&e.keystroke, window, cx)
                        || d.offer_keys(&e.keystroke, window, cx)
                        || d.picker_keys(&e.keystroke, window, cx)
                        || d.surface_key(&e.keystroke, cx)
                        || d.usage_keys(&e.keystroke, cx)
                })
                .unwrap_or(false);
            if took {
                cx.stop_propagation();
            }
        })
        .detach();
        mirror(&settings, cx);
        let composer = mention::Composer::new(settings.send.return_sends(""), window, cx);
        let reader = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 24));
        let menu_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe_in(
            &menu_search,
            window,
            |this, input, e: &InputEvent, _, cx| {
                if !matches!(e, InputEvent::PressEnter { .. }) {
                    cx.notify();
                    return;
                }
                let text = input.read(cx).value().trim().to_owned();
                if this.popup == Popup::Branch {
                    this.new_branch(&text, cx);
                }
            },
        )
        .detach();
        let side_line = cx.new(|cx| InputState::new(window, cx).placeholder("Ask on the side"));
        cx.subscribe_in(
            &side_line,
            window,
            |this, input, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    let text = input.read(cx).value().to_string();
                    this.ask_side(&text, window, cx);
                }
            },
        )
        .detach();
        let rename = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        cx.subscribe(&rename, |this, input, e: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = e {
                let text = input.read(cx).value().trim().to_owned();
                this.finish_rename(&text, cx);
            }
        })
        .detach();

        let (tx, mut rx) = mpsc::unbounded_channel();
        // In the window, for what comes back to change what is typed there.
        cx.spawn_in(window, async move |this, cx| {
            while let Some(msg) = rx.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.take(msg, window, cx);
                        this.stay_awake();
                        this.show_badge();
                    })
                    .is_err()
                {
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
                if this
                    .update(cx, |this, cx| {
                        this.release_idle(cx);
                        this.send_due(cx);
                        this.poll_prs();
                        this.check_agents(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let pages = crate::pages::Pages::new(&settings, window, cx);
        let registry = registry::kept(&home).unwrap_or_default();
        let transport = hub::Transport::local(home.clone(), tx.clone());
        transport.send(Call::Fetch);
        hub::on_path(tx.clone());
        let mut desk = Self {
            home,
            settings,
            settings_refused,
            store_refused: None,
            renaming: None,
            zoom: None,
            filter: Vec::new(),
            keep_worktree: HashSet::new(),
            chosen: std::collections::BTreeSet::new(),
            tunnels: Vec::new(),
            trail: Vec::new(),
            ahead: Vec::new(),
            trailing: false,
            stopping: None,
            glassed: false,
            spent: std::collections::HashMap::new(),
            forms: std::collections::HashMap::new(),
            quit_asked: None,
            badge: 0,
            weights: Vec::new(),
            here: HashMap::new(),
            on_path: Vec::new(),
            menu_search,
            reading: None,
            reader,
            rename,
            undo: None,
            waiting: Vec::new(),
            rewinding: None,
            asking_beside: None,
            closed_tabs: HashSet::new(),
            gone: Vec::new(),
            kept: HashMap::new(),
            notices: Vec::new(),
            pages,
            adding: projects::Adding::default(),
            behind: false,
            awake: None,
            files: HashMap::new(),
            elsewhere: std::collections::BTreeMap::new(),
            drawer: runs::Drawer::new(window, cx),
            palette: palette::Palette::new(window, cx),
            pr_tick: 0,
            health_tick: 0,
            review: gitview::Review::new(window, cx),
            prs: HashMap::new(),
            issues: HashMap::new(),
            cli_versions: HashMap::new(),
            registry,
            registry_trouble: None,
            slots: Vec::new(),
            sessions: crate::sessions::Sessions::default(),
            current: None,
            page: Page::Thread,
            popup: Popup::Closed,
            picker: crate::thread::Picker::default(),
            panes: Panes {
                left: true,
                right: true,
                whole: false,
            },
            composer,
            expanded: HashSet::new(),
            closed: HashSet::new(),
            side: crate::sidebar::side_tree(window, cx),
            side_shape: String::new(),
            side_current: None,
            side_order: Vec::new(),
            settled_shown: 0,
            command_down: None,
            numbers_shown: false,
            held: HashMap::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(800.)),
            lines: Vec::new(),
            lines_of: None,
            reveal: None,
            find: crate::thread::Find::new(window, cx),
            signing: None,
            columns: HashMap::new(),
            side_line,
            viewer: Viewer::new(window, cx),
            forge: forge::Forge::new(window, cx),
            tab_scroll: gpui::ScrollHandle::new(),
            edits: std::cell::RefCell::default(),
            search: lists::Search::new(window, cx),
            registry_read: None,
            tx,
            transport,
        };
        desk.transport.send(Call::Watch);
        Self::keep_on_quit(cx);
        desk.fill_slots();
        desk.look_here();
        desk
    }

    /// Whether anything is going on that is drawn as it goes.
    fn busy(&self) -> bool {
        self.sessions
            .all
            .iter()
            .any(|s| matches!(s.state, State::Working | State::Opening))
            || self
                .slots
                .iter()
                .any(|s| matches!(s.stage, Stage::Installing(_) | Stage::Starting))
    }

    pub fn session(&self) -> Option<&Session> {
        let key = self.current?;
        self.sessions.at(key)
    }

    pub(crate) fn save_settings(&mut self) {
        if self.settings_refused.is_some() {
            return;
        }
        self.settings.save(&self.home);
    }

    /// The project a new session starts in: that of the session on
    /// screen, else the first listed.
    pub fn chosen_project(&self) -> Option<PathBuf> {
        self.session()
            .map(|s| s.meta.project.clone())
            .or_else(|| self.projects().into_iter().next())
    }

    /// The project a new session starts in; with none yet, a scratch
    /// folder of its own, never your home. None when that cannot be made.
    pub fn project(&mut self) -> Option<PathBuf> {
        self.chosen_project().or_else(|| self.scratch())
    }

    /// The projects down the left: those kept, then the folder of any
    /// session in none of them.
    pub fn projects(&self) -> Vec<PathBuf> {
        let mut out = self.settings.folders.clone();
        for s in &self.sessions.all {
            if !out.contains(&s.meta.project) {
                out.push(s.meta.project.clone());
            }
        }
        // Archived ones are out of the list; pinned ones lead it.
        out.retain(|p| !self.prefs(p).archived);
        out.sort_by_key(|p| !self.prefs(p).pinned);
        out
    }

    /// Ask what the folder of each project here holds, once: not on
    /// another machine.
    fn read_projects(&mut self) {
        for folder in self.projects() {
            if !self.held.contains_key(&folder) && crate::remote::parse(&folder).is_none() {
                self.held
                    .insert(folder.clone(), crate::repo::Held::default());
                self.transport.send(Call::ReadProject { folder });
            }
        }
    }

    /// What you set for the project `folder`.
    pub fn prefs(&self, folder: &Path) -> settings::Prefs {
        self.settings.prefs.get(folder).cloned().unwrap_or_default()
    }

    /// The name the project `folder` shows by.
    pub fn project_name(&self, folder: &Path) -> String {
        self.prefs(folder).name.unwrap_or_else(|| {
            let name = folder
                .file_name()
                .map_or_else(|| Desk::shown(folder), |n| n.to_string_lossy().into_owned());
            match crate::remote::parse(folder) {
                Some((host, _)) => format!("{name} · {host}"),
                None => name,
            }
        })
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
        // In a project on another machine, the agent is the one there.
        let there = match crate::remote::parse(&folder) {
            Some((host, _)) if crate::remote::split(agent).1.is_none() => {
                let Some(id) = self.remote_slot(agent, &host) else {
                    self.open_tab(crate::pages::Tab::Agents, window, cx);
                    return;
                };
                Some(id)
            }
            _ => None,
        };
        let agent = there.as_deref().unwrap_or(agent);
        if !self.slot(agent).is_some_and(Slot::usable) {
            self.page = Page::Agent(agent.to_owned());
            cx.notify();
            return;
        }
        // A session begun and not used yet is reused, not doubled.
        let key = if let Some(s) =
            self.sessions.all.iter().find(|s| {
                s.meta.agent == agent && s.meta.project == folder && s.state == State::Draft
            }) {
            s.key
        } else {
            let key = self.sessions.take_key();
            let s = self.begun_in(key, agent, folder);
            self.sessions.all.push(s);
            key
        };
        self.show(key, window, cx);
    }

    /// A new session of `agent` in the project `folder`: on another
    /// machine it works in the path there, whose git the desk does not
    /// read.
    fn begun_in(&self, key: u64, agent: &str, folder: PathBuf) -> Session {
        let worktree = self
            .prefs(&folder)
            .worktree
            .unwrap_or(self.settings.worktree);
        let there = crate::remote::parse(&folder);
        let mut s = Session::new(key, agent, folder);
        s.wants_worktree = worktree;
        if let Some((_, path)) = there {
            s.meta.folder = path;
        } else {
            self.transport.send(Call::Look {
                folder: s.meta.folder.clone(),
                key,
            });
        }
        if let Some(slot) = self.slot(agent) {
            slot.seed(&mut s);
        }
        // As t3code: with the agent of the session open, its model too.
        let model = self.session().filter(|o| o.meta.agent == agent).and_then(|o| {
            o.options.iter().find_map(|opt| match &opt.kind {
                agent_client_protocol::v1::schema::SessionConfigKind::Select(sel)
                    if opt.category
                        == Some(agent_client_protocol::v1::schema::SessionConfigOptionCategory::Model) =>
                {
                    Some((opt.id.0.to_string(), sel.current_value.0.to_string()))
                }
                _ => None,
            })
        });
        if let Some((config, value)) = model {
            s.choose(&config, &value);
        }
        s
    }

    /// Read what each session kept here cost, from its record.
    pub(crate) fn read_spent(&self) {
        let records = (self.sessions.all.iter())
            .filter_map(|s| Some((s.key, s.meta.agent.clone(), s.meta.id.clone()?)))
            .collect();
        self.transport.send(crate::proto::Call::Spent { records });
    }

    /// Put the session `key` on screen.
    pub fn show(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.bring(key, window, cx);
        window.focus(&self.composer.input.focus_handle(cx), cx);
    }

    /// Put the session `key` on screen, opening it if it is closed.
    pub(crate) fn bring(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.current != Some(key) {
            // Words chosen in the session left are not copied from this one.
            gpui::base::TextSelection::clear(window, cx);
            // Where you were, to go back to; going back keeps the way ahead.
            if !self.trailing {
                if let Some(old) = self.current {
                    self.trail.push(old);
                }
                self.ahead.clear();
            }
            // What was being written stays with its session, and the
            // session shown brings back what was being written in it.
            let typed = self.typed(cx);
            if let Some(old) = self.current
                && let Some(s) = self.sessions.at_mut(old)
            {
                s.meta.unsent = typed;
                s.seen = Some(s.items.len());
            }
            let unsent = self
                .sessions
                .at(key)
                .map(|s| s.meta.unsent.clone())
                .unwrap_or_default();
            self.put_line(&unsent, window, cx);
            self.review.scope = crate::git::Scope::All;
            self.review.committing = false;
        }
        self.current = Some(key);
        self.page = Page::Thread;
        self.popup = Popup::Closed;
        self.close_settings(window, cx);
        if let Some(s) = self.sessions.at_mut(key)
            && s.meta.flags.unread
        {
            s.meta.flags.unread = false;
            self.save_index();
        }
        self.refresh_git(key);
        let closed = self
            .sessions
            .at(key)
            .is_some_and(|s| s.state == State::Closed && !s.recorded);
        if closed {
            self.look(key);
            self.read_record(key);
        }
        self.to_bottom();
        cx.notify();
    }

    /// ⌘N, as t3code's: with more than one project and the sidebar not
    /// narrowed to one, the palette's New session in…; else a session in
    /// the one project.
    pub fn new_anywhere(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.filter.as_slice() {
            [only] => self.new_in(only.clone(), window, cx),
            _ if self.projects().len() > 1 => {
                self.toggle_palette(window, cx);
                self.new_in_page(window, cx);
            }
            _ => self.new_default(window, cx),
        }
    }

    /// Begin a session in the project on screen (⇧⌘N): that of the session
    /// open, else the one chosen last, else the first.
    pub fn new_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(project) = self.project() {
            self.new_in(project, window, cx);
        }
    }

    /// Begin a session in `folder`, with the agent of the session on screen
    /// if it can run, else the first that can.
    pub fn new_in(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A project on another machine: the agent there, started over SSH.
        if let Some((host, _)) = crate::remote::parse(&folder) {
            let shown = self
                .session()
                .map(|s| crate::remote::split(&s.meta.agent).0.to_owned());
            let base = self
                .prefs(&folder)
                .agent
                .into_iter()
                .chain(shown)
                .chain(self.settings.enabled.clone())
                .find(|a| {
                    self.registry
                        .agents
                        .iter()
                        .any(|e| &e.id == a && crate::remote::command(e, &host).is_some())
                });
            match base.and_then(|a| self.remote_slot(&a, &host)) {
                Some(id) => self.new_session(&id, folder, window, cx),
                None => self.open_tab(crate::pages::Tab::Agents, window, cx),
            }
            return;
        }
        // The project's own agent, else that of the session on screen, else
        // the first that can run.
        let agent = self
            .prefs(&folder)
            .agent
            .and_then(|a| self.slot(&a))
            .filter(|s| s.usable())
            .or_else(|| self.session().and_then(|s| self.slot(&s.meta.agent)))
            .filter(|s| s.usable())
            .or_else(|| self.slots.iter().find(|s| s.usable()))
            .or_else(|| self.slots.first())
            .map(|s| s.entry.id.clone());
        match agent {
            Some(agent) => self.new_session(&agent, folder, window, cx),
            None => self.open_tab(crate::pages::Tab::Agents, window, cx),
        }
    }

    /// Begin the draft on screen with another agent.
    pub fn switch_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        if let Some(key) = self.current
            && let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
        {
            // Where it works stays, and what git said of it.
            let mut new = Session::new(key, agent, s.meta.folder.clone());
            new.meta.project = std::mem::take(&mut s.meta.project);
            (new.branch, new.repo) = (s.branch.take(), s.repo);
            new.scripts = std::mem::take(&mut s.scripts);
            new.wants_worktree = s.wants_worktree;
            *s = new;
        }
        if let Some(key) = self.current {
            let slot = self.slots.iter().find(|x| x.entry.id == agent);
            if let (Some(slot), Some(s)) = (slot, self.sessions.at_mut(key)) {
                slot.seed(s);
            }
        }
        cx.notify();
    }

    /// Ask for a folder to add as a project, and begin a session in it.
    pub fn add_project(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.add_and_begin(&folder, window, cx);
                });
            }
        })
        .detach();
    }

    /// Take the session `key` off the desk, and the worktree made for it;
    /// `keep_out`: its agent keeps it, and its list is not to bring it back.
    pub(crate) fn forget_session(&mut self, key: u64, keep_out: bool, cx: &mut Context<Self>) {
        // Its subagents, and their records, go with it.
        let kids: Vec<u64> = self
            .sessions
            .all
            .iter()
            .filter(|c| c.parent == Some(key))
            .map(|c| c.key)
            .collect();
        for kid in kids {
            self.forget_session(kid, false, cx);
        }
        let Some(at) = self.sessions.all.iter().position(|s| s.key == key) else {
            return;
        };
        let s = self.sessions.all.remove(at);
        if let Some(id) = s.meta.id.clone() {
            self.unkeep(s.meta.agent.clone(), id, keep_out);
        }
        // Its checkpoints go with it.
        if s.repo {
            let (dir, uid) = (
                s.meta
                    .worktree
                    .as_ref()
                    .map_or_else(|| s.meta.project.clone(), |w| w.repo.clone()),
                s.uid(),
            );
            self.transport.send(Call::DropCheckpoints { dir, uid });
        }
        // Its worktree goes too, unless another session works in it (a
        // fork, or a new session begun there), or you kept it.
        if let Some(wt) = s.meta.worktree
            && !self.keep_worktree.remove(&key)
            && !self
                .sessions
                .all
                .iter()
                .any(|o| o.meta.worktree.as_ref().is_some_and(|w| w.path == wt.path))
        {
            self.transport.send(Call::Git {
                dir: wt.repo.clone(),
                action: crate::repo::GitAction::RemoveWorktree(wt),
                prefix: self.settings.branch_prefix.clone(),
                left_out: Vec::new(),
                key,
            });
        }
        if self.current == Some(key) {
            self.current = None;
        }
        cx.notify();
    }

    /// Begin again where the session on screen left off: a draft with the
    /// same agent, in the same project.
    pub fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (agent, folder) = (s.meta.agent.clone(), s.meta.project.clone());
        let key = self.sessions.take_key();
        let s = self.begun_in(key, &agent, folder);
        self.sessions.all.push(s);
        self.show(key, window, cx);
    }

    /// Ask for the folder the draft on screen works in.
    /// Ask for a folder the draft on screen works in too, beside its own.
    pub fn add_folder(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Work here too".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(s) = this.sessions.at_mut(key)
                        && s.state == State::Draft
                    {
                        for p in paths {
                            if p != s.meta.folder && !s.meta.also.contains(&p) {
                                s.meta.also.push(p);
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

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
                let _ = this.update(cx, |this, cx| this.work_in(key, &folder, cx));
            }
        })
        .detach();
    }

    /// The draft `key` works in `folder`, which becomes a project.
    pub fn work_in(&mut self, key: u64, folder: &Path, cx: &mut Context<Self>) {
        if let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
        {
            let wants = s.wants_worktree;
            *s = Session::new(key, &s.meta.agent.clone(), folder.to_path_buf());
            s.wants_worktree = wants;
            self.look(key);
        }
        self.settings.used(folder);
        self.save_settings();
        cx.notify();
    }

    /// Ask for files to go with what is written: pictures go as pictures,
    /// the others are named as `@` names them.
    pub fn attach_files(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.drop_files(&paths, window, cx);
                });
            }
        })
        .detach();
    }

    /// Whether Ghost may quit now: at once with no agent at work, else at
    /// the second asking within a few seconds.
    pub fn may_quit(&mut self, cx: &gpui::App) -> bool {
        let working = self
            .sessions
            .all
            .iter()
            .any(|s| matches!(s.state, State::Working));
        let again = self
            .quit_asked
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
        if !working || again {
            return true;
        }
        self.quit_asked = Some(std::time::Instant::now());
        self.trouble(
            "Agents are at work",
            Some(crate::keys::hint(
                cx,
                "Quitting stops their turns. Quit again to quit",
                "quit",
            )),
            None,
        );
        false
    }

    /// The menu `popup` opens from `trigger`: gpui-kit's Popover, open
    /// while the desk says so, its body drawn by `body` from the desk each
    /// time the popover is drawn.
    pub fn menu<E: IntoElement>(
        &self,
        popup: Popup,
        trigger: impl Selectable + IntoElement + 'static,
        body: impl Fn(&mut Self, &mut Window, &mut Context<Self>) -> E + 'static,
        cx: &mut Context<Self>,
    ) -> Popover {
        let desk = cx.entity();
        let drawn = desk.clone();
        Popover::new(SharedString::from(format!("{popup:?}")))
            .open(self.popup == popup)
            .p(px(6.))
            .trigger(trigger)
            .content(move |_, window, cx| drawn.update(cx, |d, cx| body(d, window, cx)))
            .on_open_change(move |open, window, cx| {
                desk.update(cx, |d, cx| d.menu_opened(&popup, *open, window, cx));
            })
    }

    /// The popover of `popup` opened or closed itself (its trigger, a
    /// click outside it): the desk follows.
    fn menu_opened(
        &mut self,
        popup: &Popup,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !open {
            if self.popup == *popup {
                self.close_popup(window, cx);
            }
            return;
        }
        // The menu's line says what it takes there.
        let says = match popup {
            Popup::Branch => "A new branch: its name, then Return",
            _ => "Search",
        };
        self.menu_search.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.set_placeholder(says, window, cx);
        });
        self.popup = popup.clone();
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
        self.panes.whole = false;
        cx.notify();
    }

    /// Widen the right column over the thread, or give the thread back.
    pub fn toggle_whole(&mut self, cx: &mut Context<Self>) {
        self.panes.whole = !self.panes.whole;
        self.panes.right = true;
        cx.notify();
    }

    /// What a setting changes now, not only when next read: the keys the
    /// composer sends with.
    pub(crate) fn settings_took(&mut self, cx: &mut Context<Self>) {
        self.sync_send_key(cx);
        mirror(&self.settings, cx);
        // Diffs read again, spacing in or out.
        if let Some(key) = self.current {
            self.refresh_diffs(key);
        }
    }

    /// Escape closes the menu open, else the palette, the name being
    /// written, find or the search typed in, else the settings, else leaves
    /// the page open for the thread, wherever the keys go: true when it did.
    fn escape_closes(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if k.key != "escape" || k.modifiers.modified() {
            return false;
        }
        let typing_in = |input: &Entity<InputState>, cx: &Context<Self>| {
            input.focus_handle(cx).is_focused(window)
        };
        if self.popup != Popup::Closed {
            self.close_popup(window, cx);
        } else if self.palette.open {
            self.palette_back(window, cx);
        } else if self.renaming.is_some() {
            self.cancel_rename(cx);
        } else if self.find.open && typing_in(&self.find.input, cx) {
            self.toggle_find(window, cx);
        } else if typing_in(&self.search.input, cx) {
            self.search
                .input
                .update(cx, |i, cx| i.set_value("", window, cx));
            window.focus(&self.composer.input.focus_handle(cx), cx);
            cx.notify();
        } else if self.pages.open {
            self.close_settings(window, cx);
        } else if self.page != Page::Thread {
            self.page = Page::Thread;
            cx.notify();
        } else {
            return false;
        }
        true
    }

    /// Close the menu open; the keys its search had go back to the
    /// composer.
    pub(crate) fn close_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        self.menu_search
            .update(cx, |i, cx| i.set_value("", window, cx));
        if self.menu_search.focus_handle(cx).is_focused(window) {
            window.focus(&self.composer.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// While the composer offers something for the word being typed, ↑
    /// and ↓ choose among the offers and Return or Tab takes the one
    /// chosen: true when the key went to them.
    fn offer_keys(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if k.modifiers.modified() || !self.composer.input.focus_handle(cx).is_focused(window) {
            return false;
        }
        let text = self.typed(cx);
        let Some(count) = self.offers(&text).map(|(_, o)| o.len()).filter(|n| *n > 0) else {
            return false;
        };
        match k.key.as_str() {
            "up" => self.composer.offer = (self.composer.offer + count - 1) % count,
            "down" => self.composer.offer = (self.composer.offer + 1) % count,
            "enter" | "tab" => self.take_offer(self.composer.offer.min(count - 1), window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Take the keys pressed for the action being recorded: bound to it in
    /// your keybindings.json (a notice if another had them); Escape lets
    /// it be. Whether the keystroke was taken.
    pub(crate) fn record_key(&mut self, k: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(name) = self.pages.recording else {
            return false;
        };
        if matches!(
            k.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function" | ""
        ) {
            return true;
        }
        self.pages.recording = None;
        if k.key == "escape" && !k.modifiers.modified() {
            cx.notify();
            return true;
        }
        let key = k.unparse(cx.platform_info().os);
        if let Some(other) = crate::keys::taken(cx, name, k) {
            self.trouble(
                format!("Those keys were “{other}”'s too"),
                Some("Yours now win; change that one if you want both.".into()),
                None,
            );
        }
        self.set_keys(name, Some(key));
        cx.notify();
        true
    }

    /// The settings open over the window at the tab `tab`, with nothing
    /// searched for and the keys in their search; what Sessions and Usage
    /// show is measured afresh.
    pub fn open_tab(
        &mut self,
        tab: crate::pages::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages.tab = tab;
        self.pages
            .search
            .update(cx, |i, cx| i.set_value("", window, cx));
        match tab {
            crate::pages::Tab::Sessions => self.measure(),
            crate::pages::Tab::Usage => {
                self.weigh_agents();
                self.read_spent();
            }
            _ => {}
        }
        if !self.pages.open {
            self.pages.open = true;
            let desk = cx.entity();
            let shown = cx.new(|cx| {
                Over::new(
                    desk.clone(),
                    |d, window, cx| {
                        let faces = fonts::faces(cx).clone();
                        // Held to the dialog's height, not grown to what it
                        // holds, so each column scrolls on its own.
                        div()
                            .h(crate::pages::settings_height(window))
                            .child(d.settings_dialog(&faces, cx))
                            .into_any_element()
                    },
                    cx,
                )
            });
            window.open_dialog(cx, move |dialog, window, _| {
                let tall = crate::pages::settings_height(window);
                let desk = desk.clone();
                dialog
                    .w(px(1080.))
                    .h(tall)
                    .p_0()
                    .close_button(false)
                    .on_close(move |_, window, cx| {
                        desk.update(cx, |d, cx| d.settings_closed(window, cx));
                    })
                    .child(shown.clone())
            });
        }
        self.popup = Popup::Closed;
        window.focus(&self.pages.search.focus_handle(cx), cx);
        cx.notify();
    }

    /// Close the settings; the keys go back to the composer.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pages.open {
            window.close_dialog(cx);
        }
        self.settings_closed(window, cx);
    }

    /// The settings' dialog has gone, closed here or by a click beside it.
    fn settings_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.open = false;
        self.pages.recording = None;
        window.focus(&self.composer.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        // An agent's page shows the command of yours that starts it.
        if let Page::Agent(id) = &page {
            let c = self.settings.commands.get(id).cloned().unwrap_or_default();
            self.pages
                .command
                .update(cx, |i, cx| i.set_value(&c, window, cx));
            self.pages
                .model
                .update(cx, |i, cx| i.set_value("", window, cx));
            if let Some(cli) = self.here.get(id).and_then(|h| h.cli.clone()) {
                self.transport.send(Call::CliVersion { cli });
            }
        }
        self.page = page;
        self.popup = Popup::Closed;
        self.close_settings(window, cx);
        cx.notify();
    }

    /// How wide the sidebar is now.
    pub fn left_width(&self) -> f32 {
        (self.settings.left as f32).max(LEFT_LEAST)
    }

    /// The sidebar's edge dragged to `x`: its width, leaving the middle
    /// t3code's least; dragged to under a quarter of its least, hidden.
    fn drag_left(&mut self, x: f32, window: &Window, cx: &mut Context<Self>) {
        if x < LEFT_LEAST / 4. {
            self.panes.left = false;
        } else {
            let most = f32::from(window.viewport_size().width) - MIDDLE_LEAST;
            self.settings.left = x.clamp(LEFT_LEAST, most.max(LEFT_LEAST)) as u32;
            self.panes.left = true;
        }
        cx.notify();
    }

    /// How wide the right column stands: wider for a file read in it, and
    /// wider again for its diff side by side.
    pub fn right_width(&self) -> f32 {
        match self.surface() {
            Some(Surface::Diff) if self.settings.side_by_side => 720.,
            Some(Surface::Diff | Surface::Terminal | Surface::File(_)) => 560.,
            _ => RIGHT,
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
        // The look, as chosen or as the system is now.
        theme::set_light(
            match self.settings.look {
                settings::Look::Dark => false,
                settings::Look::Light => true,
                settings::Look::System => matches!(
                    window.appearance(),
                    gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
                ),
            },
            cx,
        );
        // The window behind the grounds: blurred where glass is on.
        if self.glassed != theme::glass() {
            self.glassed = theme::glass();
            window.set_background_appearance(if self.glassed {
                gpui::WindowBackgroundAppearance::Blurred
            } else {
                gpui::WindowBackgroundAppearance::Opaque
            });
        }
        self.sync_lines();
        self.sync_diff(cx);
        self.sync_files(cx);
        self.read_projects();
        self.sync_side(cx);
        // ⌘ let go where the desk did not hear it (over a dialog): the
        // sessions' numbers go.
        if self.numbers_shown && !window.modifiers().platform {
            self.numbers_shown = false;
            self.command_down = None;
        }
        self.show_notices(window, cx);
        let faces = fonts::faces(cx).clone();
        let lights = lights_room(window);
        let left = self
            .left_width()
            .min(f32::from(window.viewport_size().width) - MIDDLE_LEAST)
            .max(LEFT_LEAST);
        let pane = f32::from(window.viewport_size().width)
            - if self.panes.left { left } else { 0. }
            - if self.panes.right && self.page == Page::Thread {
                self.right_width()
            } else {
                0.
            };
        let middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(self.topbar(&faces, lights, pane, cx));
        let cell = crate::thread::drawer::grid(window, &faces, self.settings.terminal_size);
        let middle = match self.page.clone() {
            Page::Thread => {
                let column = self.settings.width.column(pane);
                middle
                    .child(self.thread(&faces, column, cx))
                    .children(self.drawer(&faces, cell, cx))
                    .child(self.composer(&faces, column, window, cx))
            }
            Page::Agent(id) => middle.child(self.agent_page(&faces, &id, cx)),
            Page::Forge(_) => middle.child(self.forge_page(&faces, cx)),
        };
        let whole = self.panes.whole && self.page == Page::Thread && self.session().is_some();
        let right = match (&self.page, self.panes.right) {
            (Page::Thread, true) if whole => Some(self.aside(&faces, cell, cx).flex_1()),
            (Page::Thread, true) if self.session().is_some() => {
                Some(self.aside(&faces, cell, cx).w(px(self.right_width())))
            }
            (Page::Agent(id), _) => Some(self.about(&faces, id, cx)),
            _ => None,
        };
        fonts::with_fallbacks(div())
            .relative()
            .flex()
            .size_full()
            // The sidebar's edge dragged: its width follows, and is kept
            // once let go.
            .on_drag_move(cx.listener(
                |this, e: &gpui::DragMoveEvent<crate::sidebar::LeftEdge>, window, cx| {
                    this.drag_left(f32::from(e.event.position.x), window, cx);
                },
            ))
            .on_drop(cx.listener(|this, _: &crate::sidebar::LeftEdge, _, _| {
                this.save_settings();
            }))
            .on_modifiers_changed(cx.listener(|this, e: &gpui::ModifiersChangedEvent, _, cx| {
                let m = e.modifiers;
                this.command_held(m.platform && !m.shift && !m.alt && !m.control, cx);
            }))
            // A swipe between pages, or a mouse's back and forward
            // buttons: back and ahead through the sessions seen.
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.step_trail(false, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.step_trail(true, window, cx)),
            )
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(BODY))
            .text_color(theme::text())
            .when(self.panes.left, |d| {
                d.child(self.sidebar(&faces, lights, left, cx))
            })
            .when(!whole, |d| d.child(middle))
            .children(right)
            .children(self.zoomed(&faces, window, cx))
            .children(self.reading_view(&faces, cx))
    }
}

impl Desk {
    /// Open a file the agent named (`src/a.rs`, `/x/y.rs:12`), from the
    /// folder of the session on screen, in your editor.
    pub fn open_named(&self, named: &str) {
        self.edit(self.full_path(named), None);
    }

    /// Where the file `named` (`src/a.rs`, `a.rs:12`) is: from the folder
    /// of the session on screen, where it is not whole.
    pub fn full_path(&self, named: &str) -> PathBuf {
        let name = named.split(':').next().unwrap_or(named);
        let path = Path::new(name);
        match self.session() {
            Some(s) if path.is_relative() => s.meta.folder.join(path),
            _ => path.to_path_buf(),
        }
    }

    /// Bind the action `name` to `key` in your keybindings.json, or with
    /// none give it Ghost's keys back, off the window's thread.
    pub fn set_keys(&self, name: &'static str, key: Option<String>) {
        hub::set_keys(self.home.clone(), name, key, self.tx.clone());
    }

    /// Write `text` to `path` whole, off the window's thread, and say so.
    pub fn write(&self, path: PathBuf, text: String) {
        hub::write(path, text, self.tx.clone());
    }

    /// Open `path` in your editor, off the window's thread (`hub::edit`).
    pub fn edit(&self, path: PathBuf, start: Option<&'static [u8]>) {
        hub::edit(path, self.settings.editor.clone(), start, self.tx.clone());
    }
}

/// The editors Ghost can open a file in, by their command, and their
/// names.
pub const EDITORS: [(&str, &str); 4] = [
    ("cursor", "Cursor"),
    ("code", "VS Code"),
    ("zed", "Zed"),
    ("subl", "Sublime Text"),
];

/// Set what the settings say where it is read as the window is drawn,
/// not from the settings: as Ghost starts and as they change.
fn mirror(settings: &settings::Settings, cx: &mut gpui::App) {
    theme::set_blue_orange(settings.blue_orange);
    theme::set_contrast(settings.contrast);
    theme::set_glass(settings.glass);
    fonts::set_mono(cx, settings.mono);
    theme::kit(cx);
}
