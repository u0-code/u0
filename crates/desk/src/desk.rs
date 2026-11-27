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
use gpui::component::menu::{PopupMenu, PopupMenuItem};
use gpui::component::popover::Popover;
use gpui::component::{Selectable, WindowExt as _};
use gpui::{
    App, Context, DismissEvent, Entity, Focusable as _, MouseButton, PathPromptOptions,
    SharedString, Window, div, prelude::*, px,
};
use tokio::sync::mpsc;

mod agents;
pub(crate) mod banners;
pub(crate) mod dictation;
pub(crate) mod forge;
mod gitview;
mod kept;
pub(crate) mod links;
pub(crate) mod lists;
mod looks;
pub(crate) mod mention;
pub(crate) mod notices;
pub(crate) mod palette;
pub(crate) mod projects;
pub(crate) mod rank;
pub(crate) mod runs;
pub(crate) mod updates;

pub use lists::{Later, Renaming};
pub mod flows;
pub(crate) mod pr;
mod replies;
mod send;
pub mod tasks;
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
/// beside it (`tokens.json` `"layout"`).
const LEFT_LEAST: f32 = crate::style::SIDEBAR_MIN_WIDTH;
const MIDDLE_LEAST: f32 = crate::style::THREAD_MIN_WIDTH;
/// How narrow the session column may get, at any size or zoom: under it
/// the columns beside it stop taking room and lie over it instead, the
/// right one first.
const THREAD_LEAST: f32 = crate::style::THREAD_MIN_WIDTH;
/// How narrow the right column can be dragged.
const RIGHT_LEAST: f32 = crate::style::RIGHT_COLUMN_MIN_WIDTH;
pub const TOPBAR: f32 = crate::style::TITLEBAR_HEIGHT;
/// Room the window's lights take at the left of the top row.
const LIGHTS: f32 = crate::style::TRAFFIC_LIGHT_INSET;

/// What the middle shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// The session on screen, by its key; or, with none, the empty start.
    Thread,
    /// An agent's page, on a server: what it is, what it needs, how to
    /// install it.
    Agent(hub::Host, String),
    /// A project's pull requests, or its issues.
    Forge(crate::github::Kind),
    /// The workflows workspace view (`Workflows` board).
    Workflows,
    /// The usage workspace view (`Usage` board).
    Usage,
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
    Repository,
    /// How to take a session back to one of your messages: by key and item.
    Rewind(u64, usize),
    /// How the sidebar sorts threads and projects.
    SidebarSort,
    /// The project a new session works in, from its chip.
    StartIn,
    /// The writer: an agent and its model, in the settings.
    Writer,
    /// What the right column can open as a tab.
    Surfaces,
    /// What a session's details show.
    Details,
    /// The project the PRs or Issues page is of.
    ForgeProject,
    /// What can be done with the pull request or issue read.
    ForgeActions,
    ForgeViews,
    /// A scheduled task's menu, by its id.
    Task(u64),
    /// The agent and model of the task being written.
    TaskAgent,
    /// The project of the task being written.
    TaskProject,
    TaskFlow,
    /// The project Settings' choices a project may hold are set for.
    Scope,
    /// The menu of the page in the Preview tab, from its More.
    #[cfg(target_os = "macos")]
    Page,
    /// The dev servers the page in the Preview tab can switch to.
    #[cfg(target_os = "macos")]
    PageServers,
}

/// Which side columns show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is an independent column toggle (docked left, overlay left, right, whole)"
)]
pub struct Panes {
    pub left: bool,
    /// Sidebar opened over the thread (⌘B) while the window has no room
    /// beside it (`WindowSizes`).
    pub left_over: bool,
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
    /// Why `keybindings.json` could not be read, while that is so: u0 uses
    /// its own keys and leaves the file untouched.
    pub keys_refused: Option<String>,
    /// What is being renamed, and the line for it.
    pub renaming: Option<lists::Renaming>,
    pub rename: Entity<InputState>,
    /// A message open to choose words from: the session's key and its
    /// item, and the line it is in.
    pub reading: Option<(u64, usize)>,
    pub reader: Entity<TextareaState>,
    /// A permission card's inline reply (`(session, request, is_revise)`),
    /// and the text area typed in on that card.
    pub telling: Option<(u64, u64, bool)>,
    pub tell_input: Entity<TextareaState>,
    /// What is typed to narrow a long menu of choices (a model's).
    pub menu_search: Entity<InputState>,
    /// Moved when what the menu open lists changed while it is open (one
    /// let go from it, pull requests come): it is built again.
    pub menu_changed: u64,
    /// The session asked to stop, and when: past a while, it can be
    /// stopped by force.
    pub stopping: Option<(u64, std::time::Instant)>,
    /// Whether the window was last set blurred behind, for glass, and the
    /// title last given macOS for the Window menu (`MacChrome`).
    glassed: bool,
    window_title: String,
    /// What you dictate, while you do.
    pub dictation: Option<dictation::Dictation>,
    /// The speech recognition it goes through, as your settings say.
    pub speech: Option<dictation::Speech>,
    /// Where an update of u0's own stands.
    pub updates: updates::Updates,
    /// What sessions used, by key, as their records say: read for the
    /// usage page.
    pub used: std::collections::HashMap<u64, Vec<crate::usage::Used>>,
    /// The forms agents asked for, by the number each answer goes back
    /// under, as questions to be answered over the composer.
    pub forms:
        std::collections::HashMap<u64, Entity<gpui::component::questionnaire::QuestionnaireState>>,
    /// When the projects were last fetched from their remotes.
    pub fetched: Option<std::time::Instant>,
    /// When ⌘Q was last pressed, not held.
    pub quit_asked: Option<std::time::Instant>,
    /// The programs the desk offers that are on the PATH (`hub::on_path`).
    pub on_path: Vec<&'static str>,
    /// How many sessions need you, as the Dock last said.
    pub badge: usize,
    /// The sessions shown, to go back and ahead through as a browser
    /// does; going back or ahead is under way.
    pub trail: gpui::base::History<u64>,
    pub trailing: bool,
    /// The sessions chosen with ⌘-click in the sidebar.
    pub chosen: std::collections::BTreeSet<u64>,
    /// The sessions a Settle pressed and dragged down the sidebar has
    /// passed over, to settle as it is let go.
    pub sweep: Vec<u64>,
    /// The projects the sidebar shows; none chosen, all.
    pub filter: Vec<Place>,
    /// The `#tag`s the sidebar shows; none chosen, all.
    pub tag_filter: Vec<String>,
    /// Whether the sidebar's inline search bar is open.
    pub search_open: bool,
    /// Sessions deleted whose worktree you kept.
    pub keep_worktree: HashSet<u64>,
    /// The image looked at large: the session's key and its item.
    pub zoom: Option<(u64, usize)>,
    /// What was just done to sessions, while it can be taken back.
    pub undo: Option<lists::Undo>,
    /// What waits to be asked before it is done: the question, the
    /// action and its sessions.
    pub confirming: Option<(String, lists::Undone, Vec<u64>)>,
    /// The session being taken back to one of your messages: the fork
    /// made for it takes its place.
    pub rewinding: Option<u64>,
    /// The session a side question is being branched off.
    pub asking_beside: Option<u64>,
    /// The subagents' tabs over the thread you opened (true) or closed
    /// (false; hidden, still at work). One you have done neither to has a
    /// tab while it works or waits on you.
    pub tabs: HashMap<u64, bool>,
    /// What waits for an agent to start, by its server and id: a fork, a
    /// delete.
    pub waiting: Vec<(hub::Host, String, lists::Waiting)>,
    /// Each session's meta as its server keeps it, by server, agent and
    /// id: what is sent is what differs.
    pub kept: HashMap<
        (
            hub::Host,
            String,
            agent_client_protocol::v1::schema::SessionId,
        ),
        crate::store::Meta,
    >,
    /// What u0 tells you at the foot of the window, the oldest first.
    pub notices: Vec<notices::Notice>,
    /// The sessions a notice says ask before they go on, shown till
    /// they no longer do.
    pub asking: std::collections::BTreeSet<u64>,
    pub pages: crate::pages::Pages,
    pub adding: projects::Adding,
    /// No window of u0's is in front.
    pub behind: bool,
    /// What keeps the Mac awake while an agent works, when it is kept so.
    pub awake: Option<std::process::Child>,
    pub drawer: runs::Drawer,
    /// Minutes counted, for what is read every other one, or checked every
    /// five.
    pub pr_tick: u32,
    pub health_tick: u32,
    pub review: gitview::Review,
    pub sims: crate::aside::Sims,
    pub palette: palette::Palette,
    /// What `@` can name: each folder's files, each project's open pull
    /// requests, read once when first wanted.
    pub files: HashMap<Place, Vec<String>>,
    pub prs: HashMap<Place, Vec<(u64, String)>>,
    pub issues: HashMap<Place, Vec<(u64, String)>>,
    pub registry: Registry,
    /// The last registry read failed: why.
    pub registry_trouble: Option<String>,
    /// The agents switched on, in the order listed.
    pub slots: Vec<Slot>,
    pub sessions: crate::sessions::Sessions,
    /// The session on screen, by key.
    pub current: Option<u64>,
    /// A second thread open beside `current` (`SplitSideChat`).
    pub split: Option<crate::thread::Split>,
    /// Modal dialog state for thread actions (`Dialogs`).
    pub dialogs: crate::thread::ThreadDialogs,
    /// First-run setup and empty-projects state (`Welcome`, `NoProjects`).
    pub onboarding: crate::thread::Onboarding,
    pub page: Page,
    pub popup: Popup,
    /// The scheduled tasks, and the one being written.
    pub tasks: tasks::Tasks,
    /// The workflows and their runs.
    pub flows: flows::Flows,
    pub picker: crate::thread::Picker,
    pub panes: Panes,
    pub composer: mention::Composer,
    /// Cards and lines opened in the thread, by key.
    pub expanded: HashSet<String>,
    /// Projects folded shut down the left, and servers.
    pub closed: HashSet<Place>,
    /// The sidebar's list: its tree, the shape it was last given, the
    /// session open in it then, the sessions in the order it lists them,
    /// and how many settled sessions it shows.
    pub side: Entity<gpui::component::tree::TreeState>,
    pub side_shape: String,
    pub side_current: Option<u64>,
    pub side_order: Vec<u64>,
    pub settled_shown: usize,
    /// Where the session's column was laid out last, in the window: what
    /// opens from the composer stays in it.
    pub column: gpui::Bounds<gpui::Pixels>,
    /// When ⌘ went down alone, and whether it has been held long enough
    /// for the sessions' numbers to show.
    pub command_down: Option<std::time::Instant>,
    pub numbers_shown: bool,
    /// What each project's folder holds, once read (see `read_projects`).
    pub held: HashMap<Place, crate::repo::Held>,
    /// The thread as a list drawn where it is seen, its lines, and the
    /// session they are of; an item to bring into view once it is drawn.
    pub list: Entity<gpui::component::message_scroller::MessageScrollerState>,
    pub lines: Vec<crate::thread::Line>,
    pub lines_of: Option<u64>,
    pub reveal: Option<usize>,
    pub find: crate::thread::Find,
    /// The agent being signed in to, while it is.
    pub signing: Option<(hub::Host, String)>,
    /// The right column's tabs, by session.
    pub columns: HashMap<u64, Column>,
    /// The pages the Preview tabs show, in the system's webview.
    #[cfg(target_os = "macos")]
    pub web: crate::aside::Web,
    /// The line a side chat in the column is asked in.
    /// The Files tab and the file read in the column.
    pub viewer: Viewer,
    /// The PRs and Issues pages.
    pub forge: forge::Forge,
    /// How far the column's tabs are scrolled, to bring one opened into view.
    pub tab_scroll: gpui::ScrollHandle,
    pub output_scroll: gpui::UniformListScrollHandle,
    /// gpui-kit's view of each edit opened in the thread, by its card.
    pub edits: std::cell::RefCell<HashMap<String, Entity<gpui::component::diff::DiffState>>>,
    pub search: lists::Search,
    /// When the registry was last read.
    pub registry_read: Option<std::time::SystemTime>,
    pub(crate) tx: hub::Sender,
    /// The servers the window holds, which run the agents: this Mac's
    /// own first.
    pub machines: Vec<hub::Machine>,
    /// The last number given a server held (`hub::Host`).
    pub last_host: u16,
}

/// A folder on one of the servers the window holds: a project's, a
/// worktree's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Place {
    pub host: hub::Host,
    pub folder: PathBuf,
}

impl Place {
    pub fn new(host: hub::Host, folder: impl Into<PathBuf>) -> Self {
        Self {
            host,
            folder: folder.into(),
        }
    }

    /// The project of the session `s`.
    pub fn project_of(s: &Session) -> Self {
        Self::new(s.host, s.meta.project.clone())
    }

    /// Where the session `s` works.
    pub fn folder_of(s: &Session) -> Self {
        Self::new(s.host, s.meta.folder.clone())
    }
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
        let (tx, mut rx) = mpsc::unbounded_channel();
        let machines = hub::Machine::open(&home, &tx);
        let (settings, settings_refused) = Settings::load(&home);
        // The system's look changed: re-project the theme outside `render`.
        cx.observe_window_appearance(window, |this, window, cx| {
            this.sync_appearance(window, cx);
            cx.notify();
        })
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
        for refused in crate::themes::load(&home, cx) {
            tracing::warn!("a theme did not read: {refused}");
        }
        theme::set_light(
            match settings.look {
                settings::Look::Dark => false,
                settings::Look::Light => true,
                settings::Look::System => matches!(
                    window.appearance(),
                    gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
                ),
            },
            cx,
        );
        mirror(&settings, cx);
        let composer = mention::Composer::new(
            settings.send.return_sends(""),
            settings.spell_check,
            window,
            cx,
        );
        let reader = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 24));
        let tell_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .submit_on_enter(settings.send.return_sends(""))
        });
        cx.subscribe_in(
            &tell_input,
            window,
            |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary,
                    shift: false,
                } = e
                    && (this.settings.send.return_sends("") || *secondary)
                    && let Some((key, request, revise)) = this.telling
                {
                    this.reject_and_tell(key, request, revise, window, cx);
                }
            },
        )
        .detach();
        let menu_search = cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Search")));
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
        let rename = cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Name")));
        cx.subscribe(&rename, |this, input, e: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = e {
                let text = input.read(cx).value().trim().to_owned();
                this.finish_rename(&text, cx);
            }
        })
        .detach();

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
        // While something goes on, draw again each second, so that the
        // time beside a turn counts.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    this.watch_device();
                    if this.busy() {
                        cx.notify();
                    }
                }) else {
                    break;
                };
            }
        })
        .detach();

        // Each minute, let go of agents idle long enough; at once and as
        // often as you said, fetch the projects.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, _| this.refresh_projects());
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.release_idle(cx);
                        this.refresh_projects();
                        this.update_due();
                        this.poll_prs();
                        this.check_agents();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let pages = crate::pages::Pages::new(&settings, window, cx);
        let dialogs = crate::thread::ThreadDialogs::new(window, cx);
        let onboarding = crate::thread::Onboarding::new(window, cx);
        let registry = registry::kept(&home).unwrap_or_default();
        let keys_refused = crate::keys::yours(&home).1;
        hub::on_path(tx.clone());
        let mut desk = Self {
            home,
            settings,
            settings_refused,
            keys_refused,
            renaming: None,
            zoom: None,
            filter: Vec::new(),
            tag_filter: Vec::new(),
            search_open: false,
            keep_worktree: HashSet::new(),
            chosen: std::collections::BTreeSet::new(),
            sweep: Vec::new(),
            trail: gpui::base::History::new(),
            trailing: false,
            stopping: None,
            glassed: false,
            window_title: String::from("u0"),
            dictation: None,
            speech: None,
            updates: updates::Updates::default(),
            used: std::collections::HashMap::new(),
            forms: std::collections::HashMap::new(),
            quit_asked: None,
            tasks: tasks::Tasks::default(),
            flows: flows::Flows::default(),
            fetched: None,
            badge: 0,
            on_path: Vec::new(),
            menu_search,
            menu_changed: 0,
            reading: None,
            reader,
            telling: None,
            tell_input,
            rename,
            undo: None,
            confirming: None,
            waiting: Vec::new(),
            rewinding: None,
            asking_beside: None,
            tabs: HashMap::new(),
            kept: HashMap::new(),
            notices: Vec::new(),
            asking: std::collections::BTreeSet::new(),
            pages,
            adding: projects::Adding::default(),
            behind: false,
            awake: None,
            files: HashMap::new(),
            drawer: runs::Drawer::new(window, cx),
            palette: palette::Palette::new(window, cx),
            pr_tick: 0,
            health_tick: 0,
            review: gitview::Review::new(window, cx),
            sims: crate::aside::Sims::new(window, cx),
            prs: HashMap::new(),
            issues: HashMap::new(),
            registry,
            registry_trouble: None,
            slots: Vec::new(),
            sessions: crate::sessions::Sessions::new(),
            current: None,
            split: None,
            dialogs,
            onboarding,
            page: Page::Thread,
            popup: Popup::Closed,
            picker: crate::thread::Picker::default(),
            panes: Panes {
                left: true,
                left_over: false,
                right: false,
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
            column: gpui::Bounds::default(),
            command_down: None,
            numbers_shown: false,
            held: HashMap::new(),
            list: cx.new(|cx| gpui::component::message_scroller::MessageScrollerState::new(0, cx)),
            lines: Vec::new(),
            lines_of: None,
            reveal: None,
            find: crate::thread::Find::new(window, cx),
            signing: None,
            columns: HashMap::new(),
            #[cfg(target_os = "macos")]
            web: crate::aside::Web::default(),
            viewer: Viewer::new(window, cx),
            forge: forge::Forge::new(&settings::home(), window, cx),
            tab_scroll: gpui::ScrollHandle::new(),
            output_scroll: gpui::UniformListScrollHandle::new(),
            edits: std::cell::RefCell::default(),
            search: lists::Search::new(window, cx),
            registry_read: None,
            tx,
            last_host: machines.iter().map(|m| m.host.0).max().unwrap_or(0),
            machines,
        };
        desk.update_due();
        // Where the window is, kept as u0 quits to open it there again.
        cx.observe_window_bounds(window, |this, window, _| {
            let b = window.bounds();
            this.settings.frame = Some(
                [b.origin.x, b.origin.y, b.size.width, b.size.height]
                    .map(|p| f32::from(p).round() as i32),
            );
        })
        .detach();
        // Once the window is up: count how it opened.
        cx.defer_in(window, |this, window, _| {
            let size = window.bounds().size;
            let shown = crate::telemetry::Window {
                width: f32::from(size.width).round() as u32,
                height: f32::from(size.height).round() as u32,
                zoom: u32::from(this.settings.scale),
                look: serde_json::to_value(this.settings.look)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                language: sys_locale::get_locale().unwrap_or_default(),
                startup_ms: crate::LAUNCHED.elapsed().as_millis() as u64,
            };
            this.call(
                hub::Host::HERE,
                Call::Telemetry(crate::telemetry::Ask::Window(shown)),
            );
        });
        Self::keep_on_quit(cx);
        // This Mac's server in this process is read now; one reached over
        // a link (its service, another Mac), once it is reached.
        if desk.machines[0].local().is_some() {
            desk.afresh(hub::Host::HERE);
        }
        desk
    }

    /// Ask the server `host` something; one not held, or out of reach
    /// with no way to it, is not asked.
    pub fn call(&self, host: hub::Host, call: Call) {
        if let Some(m) = self.machine(host) {
            m.send(call);
        }
    }

    /// The server the window holds as `host`.
    pub fn machine(&self, host: hub::Host) -> Option<&hub::Machine> {
        hub::Machine::of(&self.machines, host)
    }

    pub(crate) fn machine_mut(&mut self, host: hub::Host) -> Option<&mut hub::Machine> {
        self.machines.iter_mut().find(|m| m.host == host)
    }

    /// The settings of the server `host`: this Mac's are the desk's own.
    pub fn settings_on(&self, host: hub::Host) -> &Settings {
        match self.machine(host) {
            Some(m) if host != hub::Host::HERE => &m.settings,
            _ => &self.settings,
        }
    }

    pub fn settings_on_mut(&mut self, host: hub::Host) -> &mut Settings {
        match self.machines.iter_mut().find(|m| m.host == host) {
            Some(m) if host != hub::Host::HERE => &mut m.settings,
            _ => &mut self.settings,
        }
    }

    /// The settings the dialog's pages of a server's own show and change:
    /// those of the server it is on.
    pub fn edited(&self) -> &Settings {
        self.settings_on(self.pages.on)
    }

    pub fn edited_mut(&mut self) -> &mut Settings {
        self.settings_on_mut(self.pages.on)
    }

    /// Save the settings of the server the dialog is on, there.
    pub(crate) fn save_edited(&mut self) {
        self.save_settings_on(self.pages.on);
    }

    /// u0's home on the server `host`: where its worktrees and scratch
    /// folders are.
    pub fn home_on(&self, host: hub::Host) -> &Path {
        self.machine(host).map_or(&self.home, |m| &m.home)
    }

    /// Where u0 makes worktrees on the server `host`, as its settings
    /// say.
    pub fn worktree_folders(&self, host: hub::Host) -> Vec<PathBuf> {
        self.settings_on(host).worktree_folders(self.home_on(host))
    }

    /// What of each agent the registry lists is on the server `host`.
    pub fn here_on(&self, host: hub::Host, agent: &str) -> Option<&crate::proto::Here> {
        self.machine(host)?.here.get(agent)
    }

    /// The scheduled tasks of the server `host`, once it has said them.
    pub fn tasks_on(&self, host: hub::Host) -> Option<&Result<Vec<crate::tasks::Task>, String>> {
        self.machine(host)?.tasks.as_ref()
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
            || self.machines.iter().any(|m| m.retry_in().is_some())
    }

    pub fn session(&self) -> Option<&Session> {
        let key = self.current?;
        self.sessions.at(key)
    }

    /// Save this Mac's settings.
    pub(crate) fn save_settings(&mut self) {
        self.save_settings_on(hub::Host::HERE);
    }

    /// Save the settings of the server `host`, there: unless they did not
    /// read, when nothing is saved over them.
    pub(crate) fn save_settings_on(&mut self, host: hub::Host) {
        // Dictation listens as the settings now say, from the next on.
        if self.dictation.is_none() {
            self.speech = None;
        }
        let refused = match self.machine(host) {
            Some(m) if host != hub::Host::HERE => m.refused.is_some() || m.up != hub::Up::On,
            _ => self.settings_refused.is_some(),
        };
        if !refused {
            let settings = Box::new(self.settings_on(host).clone());
            self.call(host, Call::SaveSettings(settings));
        }
    }

    /// The project a new session starts in: that of the session on
    /// screen, else the first listed that is not a scratch session's.
    pub fn chosen_project(&self) -> Option<Place> {
        self.session()
            .map(Place::project_of)
            .or_else(|| self.projects().into_iter().find(|p| !self.is_scratch(p)))
    }

    /// The project a new session starts in; with none yet, a scratch
    /// folder of its own on this Mac, never your home. None when that
    /// cannot be made.
    pub fn project(&mut self) -> Option<Place> {
        self.chosen_project()
            .or_else(|| self.scratch(hub::Host::HERE))
    }

    /// The projects down the left, server by server, this Mac's first:
    /// those kept, then the folder of any session in none of them.
    pub fn projects(&self) -> Vec<Place> {
        let mut out: Vec<Place> = (self.machines.iter())
            .flat_map(|m| {
                let folders = &self.settings_on(m.host).folders;
                folders.iter().map(|f| Place::new(m.host, f.clone()))
            })
            .collect();
        for s in &self.sessions.all {
            let p = Place::project_of(s);
            if !out.contains(&p) {
                out.push(p);
            }
        }
        // Archived ones are out of the list; pinned ones lead their
        // server's.
        out.retain(|p| !self.prefs(p).archived);
        out.sort_by_key(|p| (p.host, !self.prefs(p).pinned));
        out
    }

    /// Ask what the folder of each project holds, once.
    fn read_projects(&mut self) {
        for p in self.projects() {
            if !self.held.contains_key(&p) {
                self.held.insert(p.clone(), crate::repo::Held::default());
                self.call(p.host, Call::ReadProject { folder: p.folder });
            }
        }
    }

    /// What you set for the project `p`, as its server keeps it.
    pub fn prefs(&self, p: &Place) -> settings::Prefs {
        (self.settings_on(p.host).prefs.get(&p.folder))
            .cloned()
            .unwrap_or_default()
    }

    /// The settings as they hold in `project` (`Settings::of`); none,
    /// everywhere's on this Mac.
    pub fn settings_in(&self, project: Option<&Place>) -> settings::Settings {
        project.map_or_else(
            || self.settings.clone(),
            |p| self.settings_on(p.host).of(&p.folder),
        )
    }

    /// The name the project `p` shows by.
    pub fn project_name(&self, p: &Place) -> String {
        self.prefs(p).name.unwrap_or_else(|| {
            let folder = &p.folder;
            if *folder == self.scratch_dir(p.host) {
                return crate::tr("Scratch").into();
            }
            if self.is_scratch(p) {
                let n = folder.file_name().unwrap_or_default().display();
                return crate::tr_with("Scratch %{n}", &[("n", &n)]).into();
            }
            folder
                .file_name()
                .map_or_else(|| Desk::shown(folder), |n| n.to_string_lossy().into_owned())
        })
    }

    /// The name of the server `host`, where something of it is shown beside
    /// another server's: none while the window holds this Mac's alone.
    pub fn server_chip(&self, host: hub::Host) -> Option<&str> {
        (self.machines.len() > 1)
            .then(|| self.machine(host).map(|m| m.name.as_str()))
            .flatten()
    }

    /// The name the project `p` shows by, with its server's after it where
    /// the window holds more than one: "app · studio".
    pub fn project_on(&self, p: &Place) -> String {
        match self.server_chip(p.host) {
            Some(on) => format!("{} · {on}", self.project_name(p)),
            None => self.project_name(p),
        }
    }

    // What you do.

    /// Begin a session with `agent` in the project `p`, on its server.
    /// Nothing is asked of the agent until you say something.
    pub fn new_session(
        &mut self,
        agent: &str,
        p: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.slot(p.host, agent).is_some_and(Slot::usable) {
            self.page = Page::Agent(p.host, agent.to_owned());
            cx.notify();
            return;
        }
        // A session begun and not used yet is reused, not doubled.
        let key = if let Some(s) =
            self.sessions.all.iter().find(|s| {
                s.of(p.host, agent) && s.meta.project == p.folder && s.state == State::Draft
            }) {
            s.key
        } else {
            let key = self.sessions.take_key();
            let s = self.begun_in(key, agent, p);
            self.sessions.all.push(s);
            key
        };
        self.show(key, window, cx);
    }

    /// A new session of `agent` in the project `p`, its folder looked at.
    fn begun_in(&self, key: u64, agent: &str, p: Place) -> Session {
        let s = self.fresh(key, agent, &p);
        self.call(
            p.host,
            Call::Look {
                folder: p.folder,
                key,
            },
        );
        s
    }

    /// A new session of `agent` in the project `p`, as the start's
    /// composer draws it and as it is begun: its worktree as the project
    /// or the settings say, the repositories its folder was found to hold
    /// (till its own look answers, which may come after it is sent), and
    /// what was last chosen.
    fn fresh(&self, key: u64, agent: &str, p: &Place) -> Session {
        let worktree = (self.prefs(p).worktree).unwrap_or(self.settings_on(p.host).worktree);
        let last = self.prefs(p).last.get(agent).cloned();
        let mut s = Session::new(key, agent, p.folder.clone());
        s.host = p.host;
        s.wants_worktree = worktree;
        if let Some(held) = self.held.get(p) {
            s.holds(held.repo, held.repos.clone());
        }
        if let Some(slot) = self.slot(p.host, agent) {
            slot.seed(&mut s);
        }
        // As t3code: what was last chosen in a session of this agent in
        // this project, its model, effort and mode.
        if let Some(last) = last {
            for (config, value) in &last.options {
                s.choose(config, value);
            }
            if let Some(mode) = &last.mode {
                s.choose_mode(mode);
            }
        }
        s
    }

    /// Read what each session kept on each server used, from its record.
    pub(crate) fn read_used(&self) {
        for m in &self.machines {
            let records = (self.sessions.all.iter())
                .filter(|s| s.host == m.host)
                .filter_map(|s| Some((s.key, s.meta.agent.clone(), s.meta.id.clone()?)))
                .collect();
            m.send(Call::Used { records });
        }
    }

    /// Put the session `key` on screen.
    pub fn show(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.bring(key, window, cx);
        window.focus(&self.composer.input.focus_handle(cx), cx);
    }

    /// Put the session `key` on screen, opening it if it is closed.
    pub(crate) fn bring(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sp) = self.split
            && sp.other == key
            && let Some(old) = self.current
        {
            self.split = Some(sp.swapped(old));
        }
        if self.current != Some(key) {
            // Words chosen in the session left are not copied from this one.
            gpui::base::TextSelection::clear(window, cx);
            // Where you were, to go back to; going back keeps the way ahead.
            if !self.trailing {
                if let Some(old) = self.current
                    && self.trail.current() != Some(&old)
                {
                    self.trail.push(old);
                }
                self.trail.push(key);
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
            self.drop_queue_edit(cx);
            self.swap_drafts(self.current, key);
            self.review.scope = crate::git::Scope::All;
            self.review.committing = false;
        }
        self.current = Some(key);
        // A subagent shown keeps its tab till you close it.
        if self.sessions.at(key).is_some_and(|s| s.parent.is_some()) {
            self.tabs.insert(key, true);
        }
        self.page = Page::Thread;
        self.popup = Popup::Closed;
        self.close_settings(window, cx);
        // Looked at: read, and no longer just woken.
        if let Some(s) = self.sessions.at_mut(key)
            && (s.meta.flags.unread || lists::woke(s))
        {
            s.meta.flags.unread = false;
            s.meta.flags.snoozed = None;
            self.save_index();
        }
        self.refresh_git(key);
        // Its branch read again: it may have changed since it ran.
        self.look(key);
        let closed = self
            .sessions
            .at(key)
            .is_some_and(|s| s.state == State::Closed && !s.recorded);
        if closed {
            self.read_record(key);
        }
        self.to_bottom(cx);
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

    /// Begin a session in the project `p`, with the agent of the session on
    /// screen if it runs on its server and can run, else the first there
    /// that can.
    pub fn new_in(&mut self, p: Place, window: &mut Window, cx: &mut Context<Self>) {
        // Scratch, the sidebar's folder of scratch sessions: one more.
        if p.folder == self.scratch_dir(p.host) {
            self.new_scratch_on(p.host, window, cx);
            return;
        }
        match self.agent_for(&p) {
            Some(agent) => self.new_session(&agent, p, window, cx),
            None => self.open_tab(crate::pages::Tab::Agents, window, cx),
        }
    }

    /// The agent a new session in the project `p` begins with, on its
    /// server: the project's own, else that of the session on screen,
    /// else the first there that can run.
    fn agent_for(&self, p: &Place) -> Option<String> {
        let on = |s: &&Slot| s.host == p.host;
        self.prefs(p)
            .agent
            .and_then(|a| self.slot(p.host, &a))
            .filter(|s| s.usable())
            .or_else(|| (self.session()).and_then(|s| self.slot(p.host, &s.meta.agent)))
            .filter(|s| s.usable())
            .or_else(|| self.slots.iter().filter(on).find(|s| s.usable()))
            .or_else(|| self.slots.iter().find(on))
            .map(|s| s.entry.id.clone())
    }

    /// With no session on screen, the one the composer writes in: a new
    /// session in the chosen project with its agent, drawn but not begun,
    /// as Claude's home offers its composer. None where one cannot begin
    /// here (no project, no agent that can run).
    pub(crate) fn unbegun(&self) -> Option<Session> {
        if self.page != Page::Thread || self.current.is_some() {
            return None;
        }
        let p = self.chosen_project()?;
        let agent = self.agent_for(&p)?;
        self.slot(p.host, &agent).filter(|s| s.usable())?;
        Some(self.fresh(0, &agent, &p))
    }

    /// Begin the session the composer was drawn for with none on screen,
    /// what was typed in it kept.
    pub(crate) fn begin_unbegun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unbegun().is_none() {
            return;
        }
        let typed = self.typed(cx);
        self.new_default(window, cx);
        if !typed.is_empty() {
            self.put_line(&typed, window, cx);
        }
        // What went with it is the new session's: it was kept as session 0.
        if let Some(draft) = self.composer.drafts.remove(&0) {
            self.composer.draft = draft;
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
            new.host = s.host;
            new.meta.project = std::mem::take(&mut s.meta.project);
            (new.branch, new.repo) = (s.branch.take(), s.repo);
            new.scripts = std::mem::take(&mut s.scripts);
            new.wants_worktree = s.wants_worktree;
            *s = new;
        }
        if let Some(key) = self.current {
            let host = self.sessions.at(key).map(|s| s.host);
            let slot = (self.slots.iter()).find(|x| Some(x.host) == host && x.entry.id == agent);
            if let (Some(slot), Some(s)) = (slot, self.sessions.at_mut(key)) {
                slot.seed(s);
            }
        }
        cx.notify();
    }

    /// Ask for a folder of this Mac's to add as a project, and begin a
    /// session in it.
    pub fn add_project(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::tr("Add project")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.add_and_begin(&Place::new(hub::Host::HERE, folder), window, cx);
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
            self.unkeep(s.host, s.meta.agent.clone(), id, keep_out);
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
            self.call(s.host, Call::DropCheckpoints { dir, uid });
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
            self.remove_checkout(wt, key);
        }
        self.clean_split(key);
        if self.current == Some(key) {
            self.current = None;
        }
        cx.notify();
    }

    /// Begin again where the session on screen left off: a draft with the
    /// same agent, in the same project.
    pub fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (agent, project) = (s.meta.agent.clone(), Place::project_of(s));
        let key = self.sessions.take_key();
        let s = self.begun_in(key, &agent, project);
        self.sessions.all.push(s);
        self.show(key, window, cx);
    }

    /// Ask for a folder the draft on screen works in too, beside its own:
    /// one of this Mac's, where it is.
    pub fn add_folder(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.here_draft() else { return };
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(crate::tr("Work here too")),
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

    /// The draft on screen, where it is on this Mac: the folders macOS
    /// offers are this Mac's.
    fn here_draft(&self) -> Option<u64> {
        let s = self.session()?;
        (s.host == hub::Host::HERE && s.state == State::Draft).then_some(s.key)
    }

    /// Ask for the folder the draft on screen works in: one of this
    /// Mac's, where it is.
    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.here_draft() else { return };
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::tr("Work here")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let at = Place::new(hub::Host::HERE, folder);
                let _ = this.update(cx, |this, cx| this.work_in(key, &at, cx));
            }
        })
        .detach();
    }

    /// The draft `key` works in `p`, on its server, which becomes a project
    /// there; its agent is the same there, where it is switched on, else
    /// the first there that can run.
    pub fn work_in(&mut self, key: u64, p: &Place, cx: &mut Context<Self>) {
        let Some(agent) = self.sessions.at(key).map(|s| s.meta.agent.clone()) else {
            return;
        };
        let agent = (self.slot(p.host, &agent).map(|s| s.entry.id.clone()))
            .or_else(|| {
                (self.slots.iter())
                    .find(|s| s.host == p.host && s.usable())
                    .map(|s| s.entry.id.clone())
            })
            .unwrap_or(agent);
        if let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
        {
            let wants = s.wants_worktree;
            *s = Session::new(key, &agent, p.folder.clone());
            s.host = p.host;
            s.wants_worktree = wants;
            let slot = (self.slots.iter()).find(|x| x.host == p.host && x.entry.id == agent);
            if let Some(slot) = slot {
                slot.seed(s);
            }
            self.look(key);
        }
        self.settings_on_mut(p.host).used(&p.folder);
        self.save_settings_on(p.host);
        cx.notify();
    }

    /// Ask for files to go with what is written: pictures go as pictures,
    /// the others are named as `@` names them.
    pub fn attach_files(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(crate::tr("Add")),
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

    /// Whether u0 may quit now, as `confirm.quit` says of ⌘Q: `held`
    /// says whether this press is the key held down, and is none for Quit
    /// chosen in the menu, which quits at once.
    pub fn may_quit(&mut self, held: Option<bool>, cx: &gpui::App) -> bool {
        use crate::settings::Quit;
        use std::time::{Duration, Instant};
        let mode = self.settings.confirm.quit;
        let Some(held) = held.filter(|_| mode != Quit::Direct) else {
            return true;
        };
        let since = self.quit_asked.map(|t| t.elapsed());
        if held {
            return mode == Quit::Hold && since.is_some_and(|t| t >= Duration::from_millis(1200));
        }
        if since.is_some_and(|t| t <= Duration::from_millis(500)) {
            return true;
        }
        self.quit_asked = Some(Instant::now());
        let working = (self.sessions.all.iter()).any(|s| matches!(s.state, State::Working));
        let key = crate::keys::written(&crate::keys::keys_of(cx, "quit"));
        let how = match mode {
            Quit::Hold => {
                crate::tr_with("Hold %{key}, or press it twice, to quit", &[("key", &key)])
            }
            _ => crate::tr_with("Press %{key} twice to quit", &[("key", &key)]),
        };
        self.info(
            how,
            working.then(|| crate::tr("Agents are at work: quitting stops their turns.").into()),
        );
        false
    }

    /// The menu `popup` opens from `trigger`: gpui-kit's `PopupMenu`, in a
    /// Popover open while the desk says so. Its items are built by `items`
    /// from the desk as it opens, and again when `menu_changed` moves.
    pub fn menu(
        &self,
        popup: Popup,
        trigger: impl Selectable + IntoElement + 'static,
        items: impl Fn(&Self, PopupMenu, &Entity<Self>, &App) -> PopupMenu + 'static,
        cx: &mut Context<Self>,
    ) -> Popover {
        let desk = cx.entity();
        let items = std::rc::Rc::new(items);
        let id = SharedString::from(format!("menu:{popup:?}"));
        self.held_open(popup.clone(), trigger, cx)
            .appearance(false)
            .content(move |_, window, cx| {
                let changed = desk.read(cx).menu_changed;
                let held =
                    window.use_keyed_state(id.clone(), cx, |_, _| None::<(u64, Entity<PopupMenu>)>);
                let build = {
                    let (desk, items) = (desk.clone(), items.clone());
                    move |menu: PopupMenu, _: &mut Window, cx: &mut Context<PopupMenu>| {
                        items(desk.read(cx), menu, &desk, cx)
                    }
                };
                let menu = match held.read(cx).clone() {
                    Some((drawn, menu)) if drawn == changed => return menu,
                    Some((_, menu)) => {
                        menu.update(cx, |m: &mut PopupMenu, cx| m.rebuild(window, cx, build));
                        menu
                    }
                    None => {
                        let before = window.focused(cx);
                        let menu = PopupMenu::build(window, cx, move |m, window, cx| {
                            let m = build(m, window, cx).max_h(px(420.)).scrollable(true);
                            match before {
                                Some(f) => m.action_context(f),
                                None => m,
                            }
                        });
                        menu.focus_handle(cx).focus(window, cx);
                        let (desk, popup) = (desk.clone(), popup.clone());
                        window
                            .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
                                desk.update(cx, |d, cx| d.menu_opened(&popup, false, window, cx));
                            })
                            .detach();
                        menu
                    }
                };
                held.update(cx, |h, _| *h = Some((changed, menu.clone())));
                menu
            })
    }

    /// What `popup` opens from `trigger` that is more than a menu (a line
    /// to search or name in): gpui-kit's Popover, open while the desk says
    /// so, its body drawn by `body` from the desk each time it is drawn.
    pub fn popover<E: IntoElement>(
        &self,
        popup: Popup,
        trigger: impl Selectable + IntoElement + 'static,
        body: impl Fn(&mut Self, &mut Window, &mut Context<Self>) -> E + 'static,
        cx: &mut Context<Self>,
    ) -> Popover {
        let drawn = cx.entity();
        self.held_open(popup, trigger, cx)
            .p(px(6.))
            .content(move |_, window, cx| drawn.update(cx, |d, cx| body(d, window, cx)))
    }

    /// gpui-kit's Popover for `popup` from `trigger`, open while the desk
    /// says so, the desk told as it opens or closes itself.
    fn held_open(
        &self,
        popup: Popup,
        trigger: impl Selectable + IntoElement + 'static,
        cx: &mut Context<Self>,
    ) -> Popover {
        let desk = cx.entity();
        Popover::new(SharedString::from(format!("{popup:?}")))
            .open(self.popup == popup)
            .trigger(trigger)
            .on_open_change(move |open, window, cx| {
                desk.update(cx, |d, cx| d.menu_opened(&popup, *open, window, cx));
            })
    }

    /// An item of a menu, `label`, which does `f` to the desk.
    pub fn menu_item(
        desk: &Entity<Self>,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> PopupMenuItem {
        PopupMenuItem::new(label).on_click(Self::does(desk, f))
    }

    /// What a menu's item does when chosen: `f`, to the desk.
    pub fn does(
        desk: &Entity<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        let desk = desk.clone();
        move |_, window, cx| desk.update(cx, |d, cx| f(d, window, cx))
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
            match popup {
                // The picker's mode menu gives the picker back.
                Popup::ModelMode if self.popup == *popup => {
                    self.popup = Popup::Model;
                    cx.notify();
                }
                _ if self.popup == *popup => self.close_popup(window, cx),
                _ => {}
            }
            return;
        }
        // The pull requests a worktree can begin from are asked for once,
        // as their menu opens.
        if *popup == Popup::Base {
            self.want_offers();
        }
        // The menu's line says what it takes there; the picker's mode menu
        // leaves the picker's search as it is.
        let says = match popup {
            Popup::Branch => Some(crate::tr("A new branch: its name, then Return")),
            Popup::ModelMode => None,
            _ => Some(crate::tr("Search")),
        };
        if let Some(says) = says {
            self.menu_search.update(cx, |i, cx| {
                i.set_value("", window, cx);
                i.set_placeholder(says, window, cx);
            });
        }
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

    pub fn toggle_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) {
        let width = f32::from(window.viewport_size().width);
        if width - self.left_width() >= THREAD_LEAST {
            self.panes.left = !self.panes.left;
            self.panes.left_over = false;
        } else {
            self.panes.left_over = !self.panes.left_over;
        }
        cx.notify();
    }

    /// Which side columns stand beside vs over the thread at `width`.
    pub(crate) fn layout_columns(&self, width: f32) -> (f32, bool, bool, bool, bool) {
        let left = self.left_width();
        let thread_page = self.page == Page::Thread && self.session().is_some();
        // The split's rail stands beside the right column; the whole
        // review keeps the sidebar away.
        let rail = if self.split.is_some() { 40. } else { 0. };
        let (left_beside, left_over, right_shown, right_beside) = columns_for(
            width,
            left,
            self.right_width() + rail,
            self.panes,
            thread_page,
        );
        let review_whole = self.panes.whole && thread_page && self.surface() == Some(Surface::Diff);
        (
            left,
            left_beside && !review_whole,
            left_over && !review_whole,
            right_shown,
            right_beside,
        )
    }

    /// Whether the sidebar is currently visible (docked or overlaying).
    pub(crate) fn sidebar_shown(&self, window: &Window) -> bool {
        let (_, beside, over, _, _) = self.layout_columns(f32::from(window.viewport_size().width));
        beside || over
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

    /// Sync light/dark mode from `settings.look` and `window.appearance()`.
    pub(crate) fn sync_appearance(&self, window: &Window, cx: &mut Context<Self>) {
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
    }

    /// What a setting changes now, not only when next read: the keys the
    /// composer sends with.
    pub(crate) fn settings_took(&mut self, cx: &mut Context<Self>) {
        self.sync_send_key(cx);
        self.sync_spelling(cx);
        self.update_due();
        if self.settings.look != settings::Look::System {
            theme::set_light(self.settings.look == settings::Look::Light, cx);
        }
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
        } else if self.telling.is_some() {
            self.cancel_tell(window, cx);
        } else if self.find.open && typing_in(&self.find.input, cx) {
            self.toggle_find(window, cx);
        } else if typing_in(&self.search.input, cx) || self.search_open {
            self.search
                .input
                .update(cx, |i, cx| i.set_value("", window, cx));
            self.search_open = false;
            window.focus(&self.composer.input.focus_handle(cx), cx);
            cx.notify();
        } else if self.chosen.len() > 1 {
            self.chosen.clear();
            cx.notify();
        } else if typing_in(&self.pages.settings_search, cx)
            && !self.pages.settings_search.read(cx).value().is_empty()
        {
            self.pages
                .settings_search
                .update(cx, |i, cx| i.set_value("", window, cx));
            cx.notify();
        } else if self.pages.pairing_modal.is_some() {
            self.close_pairing_dialog(window, cx);
        } else if self.pages.open && self.pages.editing_theme.is_some() {
            self.close_theme_editor(cx);
        } else if self.pages.open {
            self.close_settings(window, cx);
        } else if self.panes.whole && self.page == Page::Thread {
            self.panes.whole = false;
            cx.notify();
        } else if self.page != Page::Thread {
            self.page = Page::Thread;
            cx.notify();
        } else if self.panes.left_over {
            self.panes.left_over = false;
            cx.notify();
        } else {
            let width = f32::from(window.viewport_size().width);
            let (_, _, _, right_shown, right_beside) = self.layout_columns(width);
            if right_shown && !right_beside {
                self.panes.right = false;
                cx.notify();
            } else {
                return false;
            }
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
                crate::tr_with("Those keys were “%{other}”'s too", &[("other", &other)]),
                Some(crate::tr("Yours now win; change that one if you want both.").into()),
                None,
            );
        }
        self.set_keys(name, crate::keys::Keys::Yours(key));
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
        if tab == crate::pages::Tab::Usage {
            self.open_page(Page::Usage, window, cx);
            return;
        }
        if tab == crate::pages::Tab::Workflows {
            self.open_page(Page::Workflows, window, cx);
            return;
        }
        self.pages.tab = tab;
        let page_ix = crate::pages::Tab::ALL
            .iter()
            .position(|t| *t == tab)
            .unwrap_or(0);
        self.pages
            .settings_search
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.pages.settings.update(cx, |s, cx| {
            s.set_query("", window, cx);
            s.set_selected_index(
                gpui::component::setting::SelectIndex {
                    page_ix,
                    group_ix: None,
                },
                cx,
            );
        });
        self.tab_shown(tab, window, cx);
        if !self.pages.open {
            // One dialog at a time: Escape closes the pairing dialog by
            // closing the topmost, which the settings would be.
            self.close_pairing_dialog(window, cx);
            self.pages.open = true;
            let desk = cx.entity();
            let shown = cx.new(|cx| {
                Over::new(
                    desk.clone(),
                    |d, window, cx| {
                        // Held to the dialog's height, not grown to what it
                        // holds, so each column scrolls on its own.
                        div()
                            .h(crate::pages::settings_height(window))
                            .child(d.settings_dialog(window, cx))
                            .into_any_element()
                    },
                    cx,
                )
            });
            window.open_dialog(cx, move |dialog, window, _| {
                let tall = crate::pages::settings_height(window);
                let desk = desk.clone();
                dialog
                    .w(px(crate::style::SETTINGS_DIALOG_WIDTH))
                    .h(tall)
                    .p_0()
                    .rounded(px(0.))
                    .close_button(false)
                    .on_close(move |_, window, cx| {
                        desk.update(cx, |d, cx| d.settings_closed(window, cx));
                    })
                    .child(shown.clone())
            });
        }
        self.popup = Popup::Closed;
        window.focus(&self.pages.settings_search.focus_handle(cx), cx);
        cx.notify();
    }

    /// What a tab shows, measured afresh as it is shown: Sessions and
    /// Usage, the themes, the hosts, sharing and the servers paired.
    pub(crate) fn tab_shown(
        &mut self,
        tab: crate::pages::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = self.pages.on;
        let page = format!("settings/{tab:?}").to_lowercase();
        self.call(on, Call::Telemetry(crate::telemetry::Ask::Opened(page)));
        match tab {
            crate::pages::Tab::Telemetry => {
                let level = self
                    .settings_on(on)
                    .telemetry
                    .unwrap_or(crate::telemetry::Level::Usage);
                let at = f32::from(u8::from(level));
                (self.pages.level).update(cx, |s, cx| s.set_value(at, window, cx));
                self.pages.sent = Default::default();
            }
            crate::pages::Tab::Archive => self.measure(),
            crate::pages::Tab::Appearance => self.read_themes(cx),
            crate::pages::Tab::SourceControl => self.call(on, Call::Forges),
            crate::pages::Tab::Usage => self.read_used(),
            crate::pages::Tab::Diagnostics => self.diagnose(on),
            crate::pages::Tab::Devices => {
                self.call(on, Call::Share(crate::share::Share::Look));
                self.read_servers();
            }
            crate::pages::Tab::Workflows => self.read_flows(window, cx),
            // Where a webhook task is called from.
            crate::pages::Tab::Tasks => self.call(on, Call::Share(crate::share::Share::Look)),
            crate::pages::Tab::Keybindings => self.reload_keys(cx),
            _ => {}
        }
    }

    /// Read `keybindings.json` again and rebind keys when it reads.
    pub(crate) fn reload_keys(&mut self, cx: &mut Context<Self>) {
        let (yours, refused) = crate::keys::yours(&self.home);
        if refused.is_none() {
            cx.clear_key_bindings();
            crate::keys::bind(cx, &yours);
        }
        self.keys_refused = refused;
    }

    /// Open the settings on the server `host`'s own: its agents first.
    pub fn settings_of(&mut self, host: hub::Host, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.on = host;
        self.open_tab(crate::pages::Tab::Agents, window, cx);
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
        self.pages.editing_theme = None;
        window.focus(&self.composer.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        // An agent's page shows the command of yours that starts it.
        match &page {
            Page::Agent(host, id) => {
                let host = *host;
                let c = (self.settings_on(host).commands.get(id))
                    .cloned()
                    .unwrap_or_default();
                self.pages
                    .command
                    .update(cx, |i, cx| i.set_value(&c, window, cx));
                self.pages
                    .model
                    .update(cx, |i, cx| i.set_value("", window, cx));
                if let Some(cli) = self.here_on(host, id).and_then(|h| h.cli.clone()) {
                    self.call(host, Call::CliVersion { cli });
                }
            }
            Page::Usage => {
                self.call(
                    self.pages.on,
                    Call::Telemetry(crate::telemetry::Ask::Opened("usage".into())),
                );
                self.read_used();
            }
            Page::Workflows => {
                self.call(
                    self.pages.on,
                    Call::Telemetry(crate::telemetry::Ask::Opened("workflows".into())),
                );
                self.read_flows(window, cx);
            }
            _ => {}
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

    /// How wide the right column stands: as you dragged it, whatever its
    /// tab shows.
    pub fn right_width(&self) -> f32 {
        (self.settings.right as f32).max(RIGHT_LEAST)
    }

    /// The right column's edge dragged to `x`: its width, leaving the
    /// session column its least.
    fn drag_right(&mut self, x: f32, window: &Window, cx: &mut Context<Self>) {
        let width = f32::from(window.viewport_size().width);
        let most = (width - THREAD_LEAST).max(RIGHT_LEAST);
        self.settings.right = (width - x).clamp(RIGHT_LEAST, most) as u32;
        cx.notify();
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
        // Speech recognition ready for the composer's speech button.
        self.speech(window, cx);
        let want_light = match self.settings.look {
            settings::Look::Dark => false,
            settings::Look::Light => true,
            settings::Look::System => matches!(
                window.appearance(),
                gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
            ),
        };
        if theme::is_light() != want_light {
            window.defer(cx, move |_, cx| theme::set_light(want_light, cx));
        }
        // The whole window as large as it is set, once this frame is drawn:
        // a zoom changes the size everything is laid out in.
        let zoom = f32::from(self.settings.scale.clamp(
            settings::SCALES[0],
            settings::SCALES[settings::SCALES.len() - 1],
        )) / 100.;
        if (window.zoom() - zoom).abs() > f32::EPSILON {
            // Outside the desk's update: a zoom tells the window's bounds
            // observers, the desk among them.
            window.defer(cx, move |window, cx| {
                window.set_zoom(zoom, cx);
                // macOS draws the lights at their own size: they stay in the
                // middle of the 38-point top row however large.
                #[cfg(target_os = "macos")]
                window.set_traffic_light_position(gpui::point(
                    gpui::px(crate::style::TRAFFIC_LIGHT_START),
                    gpui::px(crate::style::TRAFFIC_LIGHT_OFFSET_Y - 6. / zoom),
                ));
            });
        }
        // The window behind the grounds: blurred where glass is on.
        if self.glassed != theme::glass() {
            self.glassed = theme::glass();
            window.set_background_appearance(if self.glassed {
                gpui::WindowBackgroundAppearance::Blurred
            } else {
                gpui::WindowBackgroundAppearance::Opaque
            });
        }
        let wanted_title = match (&self.page, self.session()) {
            (Page::Thread, Some(s)) if !s.items.is_empty() || s.meta.renamed => {
                format!("u0 — {}", s.shown_title())
            }
            _ => String::from("u0"),
        };
        if self.window_title != wanted_title {
            self.window_title.clone_from(&wanted_title);
            window.set_window_title(&wanted_title);
        }
        self.sync_lines(cx);
        self.sync_diff(cx);
        self.sync_files(cx);
        self.read_projects();
        self.sync_side(window, cx);
        // ⌘ let go where the desk did not hear it (over a dialog): the
        // sessions' numbers go.
        if self.numbers_shown && !window.modifiers().platform {
            self.numbers_shown = false;
            self.command_down = None;
        }
        self.show_notices(window, cx);
        self.show_confirm(window, cx);
        let faces = fonts::faces(cx).clone();
        let lights = lights_room(window);
        if self.settings.needs_welcome() {
            return fonts::with_fallbacks(div())
                .relative()
                .flex()
                .size_full()
                .bg(theme::bg())
                .font_family(faces.ui.clone())
                .text_size(px(BODY))
                .text_color(theme::text())
                .child(self.welcome_view(&faces, lights, cx));
        }
        let width = f32::from(window.viewport_size().width);
        let thread_page = self.page == Page::Thread && self.session().is_some();
        let whole = self.panes.whole && thread_page;
        let review_whole = whole && self.surface() == Some(Surface::Diff);
        // The session column keeps its least (480), at any size or zoom: a
        // column beside it that would leave it less lies over it instead, the
        // right one first (`WindowSizes`).
        let (left, left_beside, left_over, right_shown, right_beside) = self.layout_columns(width);
        // The webview of a Preview tab is shown only while the column it is
        // in is drawn, before anything is laid out over it.
        #[cfg(target_os = "macos")]
        self.sync_web(thread_page && (whole || right_shown), window, cx);
        if left_beside && self.panes.left_over {
            self.panes.left_over = false;
        }
        let left_shown = left_beside || left_over;
        let rail_room = if right_shown && self.split.is_some() {
            40.
        } else {
            0.
        };
        let pane = width
            - if left_beside { left } else { 0. }
            - if right_beside {
                self.right_width() + rail_room
            } else {
                0.
            };
        self.column = gpui::Bounds::new(
            gpui::point(px(if left_beside { left } else { 0. }), px(0.)),
            gpui::size(px(pane), window.viewport_size().height),
        );
        let cell = crate::thread::drawer::grid(window, &faces, self.settings.terminal_size);
        let split_middle = (self.page == Page::Thread)
            .then(|| {
                self.split_middle(
                    &faces,
                    lights,
                    left_beside,
                    pane,
                    right_shown,
                    cell,
                    window,
                    cx,
                )
            })
            .flatten();
        let middle = if let Some(split_div) = split_middle {
            split_div
        } else {
            let middle = div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                // Nothing in it paints over the columns beside it.
                .overflow_hidden()
                // Your picture, faint under the thread and what is beside it.
                .children(self.settings.backdrop.clone().map(|path| {
                    gpui::img(path)
                        .absolute()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .opacity(0.16)
                }))
                .child(self.topbar(&faces, left_shown, cx))
                .children(self.thread_banners(&faces, cx));

            match self.page.clone() {
                Page::Thread => {
                    let column = self.settings.width.column(pane);
                    middle
                        .children(self.guest_thread_header(&faces, column))
                        .child(self.thread(&faces, column, window, cx))
                        .child(self.composer(&faces, column, window, cx))
                        .children(self.drawer(&faces, cell, cx))
                }
                Page::Agent(host, id) => middle.child(self.agent_page(&faces, host, &id, cx)),
                Page::Forge(_) => middle.child(self.forge_page(&faces, cx)),
                Page::Workflows => middle.child(self.workflows_page(window, cx)),
                Page::Usage => middle.child(self.usage_page(cx)),
            }
        };
        let right = match &self.page {
            Page::Thread if whole => Some(self.aside(&faces, cell, cx).flex_1()),
            Page::Thread if right_beside => {
                Some(self.aside(&faces, cell, cx).w(px(self.right_width())))
            }
            Page::Thread if right_shown => Some(over_right(
                self.aside(&faces, cell, cx)
                    .w(px(self.right_width().min(width - 48.))),
            )),
            Page::Agent(_, id) => Some(self.about(&faces, id, cx)),
            _ => None,
        };
        let split_rail = (self.page == Page::Thread && right_beside)
            .then(|| self.split_rail(cx))
            .flatten();
        fonts::with_fallbacks(div())
            .relative()
            .flex()
            .size_full()
            // A column's edge dragged: its width follows, and is kept once
            // let go.
            .on_drag_move(cx.listener(
                |this, e: &gpui::DragMoveEvent<crate::sidebar::LeftEdge>, window, cx| {
                    this.drag_left(f32::from(e.event.position.x), window, cx);
                },
            ))
            .on_drop(cx.listener(|this, _: &crate::sidebar::LeftEdge, _, _| {
                this.save_settings();
            }))
            .on_drag_move(cx.listener(
                |this, e: &gpui::DragMoveEvent<crate::aside::RightEdge>, window, cx| {
                    this.drag_right(f32::from(e.event.position.x), window, cx);
                },
            ))
            .on_drop(cx.listener(|this, _: &crate::aside::RightEdge, _, _| {
                this.save_settings();
            }))
            // The dictation key let go, which ends talking while it is held.
            .on_key_up(cx.listener(|this, e: &gpui::KeyUpEvent, _, cx| {
                let key = crate::keys::keys_of(cx, "dictate");
                if key.last().is_some_and(|k| k.key == e.keystroke.key) {
                    this.dictate_key_up(cx);
                }
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
            .when(left_beside, |d| {
                d.child(self.sidebar(&faces, lights, left, window, cx))
            })
            .when(!whole, |d| d.child(middle))
            .children(right)
            .children(split_rail)
            .when(left_over, |d| {
                d.child(over_left(self.sidebar(
                    &faces,
                    lights,
                    left.min(width - 48.),
                    window,
                    cx,
                )))
            })
            // Hidden, the sidebar is dragged back out from the left edge.
            .when(!left_shown && !review_whole, |d| {
                use gpui::component::resizable::{HandleEdge, resize_handle};
                d.child(
                    resize_handle::<crate::sidebar::LeftEdge, crate::sidebar::LeftEdge>(
                        "left-show",
                        gpui::Axis::Horizontal,
                    )
                    .inside(HandleEdge::Leading)
                    .on_drag(crate::sidebar::LeftEdge, |edge, _, _, cx| {
                        cx.new(|_| edge.clone())
                    }),
                )
            })
            .children(self.zoomed(&faces, window, cx))
            .children(self.reading_view(&faces, cx))
    }
}

impl Desk {
    /// Open a file the agent named (`src/a.rs`, `/x/y.rs:12`), from the
    /// folder of the session on screen, in your editor.
    pub fn open_named(&self, named: &str) {
        let (path, line) = (self.full_path(named), line_of(named).map(str::to_owned));
        hub::edit(
            path,
            line,
            self.settings.editor.clone(),
            None,
            self.tx.clone(),
        );
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
    /// none give it u0's keys back, off the window's thread.
    pub fn set_keys(&self, name: &'static str, keys: crate::keys::Keys) {
        hub::set_keys(self.home.clone(), name, keys, self.tx.clone());
    }

    /// Write `text` to `path` whole, off the window's thread, and say so.
    pub fn write(&self, path: PathBuf, text: String) {
        hub::write(path, text, self.tx.clone());
    }

    /// Put the `u0` command where your shell finds it, off the
    /// window's thread (`hub::install_command`).
    pub fn install_command(&self) {
        hub::install_command(self.tx.clone());
    }

    /// Open `path` in your editor, off the window's thread (`hub::edit`).
    pub fn edit(&self, path: PathBuf, start: Option<&'static [u8]>) {
        hub::edit(
            path,
            None,
            self.settings.editor.clone(),
            start,
            self.tx.clone(),
        );
    }
}

/// The line (`12`, or `12:5` with its column) a file named as a compiler
/// writes it (`src/a.rs:12:5:`) is at, where it says one.
fn line_of(named: &str) -> Option<&str> {
    let (_, at) = named.split_once(':')?;
    let at = at.trim_end_matches(':');
    (!at.is_empty() && at.split(':').all(|n| n.parse::<u32>().is_ok())).then_some(at)
}

/// Pure layout rule (`WindowSizes`): the thread never gets narrower than
/// `THREAD_LEAST` (`480`); side columns that do not fit beside it either fold
/// away (`left`, until opened with ⌘B as `left_over`) or lie over it (`right`).
fn columns_for(
    width: f32,
    left: f32,
    right: f32,
    panes: Panes,
    thread_page: bool,
) -> (bool, bool, bool, bool) {
    let fits_left = width - left >= THREAD_LEAST;
    let left_beside = panes.left && fits_left;
    let left_over = panes.left_over && !fits_left;
    let right_shown = panes.right && thread_page && !panes.whole;
    let right_beside =
        right_shown && width - if left_beside { left } else { 0. } - right >= THREAD_LEAST;
    (left_beside, left_over, right_shown, right_beside)
}

/// The sidebar opened over the thread when the window has no room beside it.
fn over_left(column: gpui::Div) -> gpui::Div {
    column
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .bg(u0ui::theme::color(u0ui::tokens::ColorRole::Layer01))
        .border_r_1()
        .border_color(theme::border())
        .shadow_lg()
}

/// The right column opened over the thread below the 38px unified title bar
/// (`WindowSizes` `.wsb-over`), leaving the title bar's panel toggles free.
fn over_right(column: gpui::Div) -> gpui::Div {
    column
        .absolute()
        .top(px(TOPBAR))
        .bottom_0()
        .right_0()
        .bg(u0ui::theme::color(u0ui::tokens::ColorRole::Layer01))
        .border_l_1()
        .border_color(theme::border())
        .shadow_lg()
}

/// The editors u0 can open a file in, by their command, and their
/// names.
pub const EDITORS: [(&str, &str); 4] = [
    ("cursor", "Cursor"),
    ("code", "VS Code"),
    ("zed", "Zed"),
    ("subl", "Sublime Text"),
];

/// Set what the settings say where it is read as the window is drawn,
/// not from the settings: as u0 starts and as they change.
fn mirror(settings: &settings::Settings, cx: &mut gpui::App) {
    theme::set_blue_orange(settings.blue_orange);
    crate::figures::set_math(settings.math);
    crate::i18n::set(settings.language.as_deref());
    theme::set_contrast(settings.contrast);
    theme::set_glass(settings.glass);
    u0ui::motion::set_reduced_motion(
        cx,
        match settings.motion {
            settings::Motion::System => u0ui::motion::ReducedMotion::System,
            settings::Motion::Reduce => u0ui::motion::ReducedMotion::Reduce,
            settings::Motion::Full => u0ui::motion::ReducedMotion::Allow,
        },
    );
    theme::set_themes(settings.theme_dark.clone(), settings.theme_light.clone());
    fonts::set_mono(cx, settings.mono);
    theme::kit(cx);
}

#[cfg(test)]
mod tests {
    use super::{Panes, columns_for, line_of};

    #[test]
    fn a_file_named_with_its_line_opens_at_that_line() {
        assert_eq!(line_of("src/main.rs:12:5:"), Some("12:5"));
        assert_eq!(line_of("/x/a.rs:40"), Some("40"));
        assert_eq!(line_of("src/main.rs"), None);
        assert_eq!(line_of("a.rs:warning"), None);
    }

    #[test]
    fn thread_keeps_its_480_minimum_at_800x520_and_200_percent_zoom() {
        let base = Panes {
            left: true,
            left_over: false,
            right: false,
            whole: false,
        };
        // 800×520, right closed: 256 + 544 (>= 480) -> sidebar stays beside.
        assert_eq!(
            columns_for(800., 256., 400., base, true),
            (true, false, false, false)
        );
        // 800×520, right open: 256 + 400 leaves 144 (< 480) -> right overlays.
        let with_right = Panes {
            right: true,
            ..base
        };
        assert_eq!(
            columns_for(800., 256., 400., with_right, true),
            (true, false, true, false)
        );
        // 1440×900 at 200% zoom -> 720 logical width (< 256 + 480 = 736):
        // sidebar folds away automatically; ⌘B opens `left_over` over thread.
        assert_eq!(
            columns_for(720., 256., 400., base, true),
            (false, false, false, false)
        );
        let toggled = Panes {
            left_over: true,
            ..base
        };
        assert_eq!(
            columns_for(720., 256., 400., toggled, true),
            (false, true, false, false)
        );
    }
}
