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
    Context, Entity, Focusable as _, MouseButton, PathPromptOptions, Window, div, prelude::*, px,
};
use tokio::sync::mpsc;
use tracing::warn;

mod agents;
mod gitview;
mod kept;
mod lists;
pub(crate) mod mention;
pub(crate) mod notices;
pub(crate) mod palette;
mod projects;
pub(crate) mod runs;

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

/// How narrow and how wide the sidebar can be dragged.
const LEFT_MOST: (f32, f32) = (220., 440.);
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
    /// Adding a project: cloned from an address, or made new.
    NewProject,
    Settings,
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
    /// What can be done with a project in the sidebar.
    Project(PathBuf),
    /// The branches to check out, in the column beside.
    Branch,
    /// The session's options beyond its model and mode.
    More,
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
    /// The project the sidebar shows, when it lists by status.
    Only,
    /// The project a session begun from the start goes in.
    StartIn,
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
    /// The session (or the project) being renamed in the sidebar, and the
    /// line for it.
    pub renaming: Option<u64>,
    pub renaming_project: Option<PathBuf>,
    /// A message open to choose words from: the session's key and its
    /// item, and the line it is in.
    pub reading: Option<(u64, usize)>,
    pub reader: Entity<TextInput>,
    /// What is typed to narrow a long menu of choices (a model's).
    pub menu_search: Entity<TextInput>,
    /// The session asked to stop, and when: past a while, it can be
    /// stopped by force.
    pub stopping: Option<(u64, std::time::Instant)>,
    /// When quitting was last asked while agents worked.
    pub quit_asked: Option<std::time::Instant>,
    /// Each running agent's process and the memory its group holds (kB),
    /// as last weighed.
    pub weights: Vec<(String, u32, u64)>,
    /// How many sessions need you, as the Dock last said.
    pub badge: usize,
    /// The action whose keys are being recorded, in Settings, Keyboard.
    pub recording: Option<&'static str>,
    /// The sessions shown before the one on screen, and those gone back
    /// from; going back or ahead is under way.
    pub trail: Vec<u64>,
    pub ahead: Vec<u64>,
    pub trailing: bool,
    /// Ports brought here from another machine (ssh -L), each its tunnel.
    pub tunnels: Vec<(u16, std::process::Child)>,
    /// A terminal asked to close with something running in it, to close at
    /// the second asking.
    pub closing: Option<u64>,
    /// The sessions chosen with ⌘-click in the sidebar.
    pub chosen: std::collections::BTreeSet<u64>,
    /// The sidebar's edge is being dragged.
    pub resizing_left: bool,
    /// The composer opened tall, to write at length.
    pub tall: bool,
    /// The image looked at large: the session's key and its item.
    pub zoom: Option<(u64, usize)>,
    /// The session is renamed in the top bar, not in the sidebar.
    pub renaming_here: bool,
    pub rename: Entity<TextInput>,
    /// What was just deleted or archived, while it can be taken back.
    pub undo: Option<lists::Undo>,
    /// The session being taken back to one of your messages: the fork
    /// made for it takes its place.
    pub rewinding: Option<u64>,
    /// The session a side question is being branched off.
    pub asking_beside: Option<u64>,
    /// Words in the thread are being chosen with the mouse.
    pub choosing: bool,
    /// The subagents' tabs over the thread you closed: hidden, still at
    /// work.
    pub closed_tabs: HashSet<u64>,
    /// The part of an open menu shown beside it: "snooze", "copy".
    pub submenu: Option<&'static str>,
    /// What waits for an agent to start, by agent: a fork, a delete.
    pub waiting: Vec<(String, lists::Waiting)>,
    /// What you put aside from the composer (see `store::Index`).
    pub stashed: Vec<String>,
    /// Sessions deleted whose agent cannot delete them (see `store::Index`).
    pub gone: Vec<(String, String)>,
    /// What Ghost tells you at the foot of the window, the oldest first.
    pub notices: Vec<notices::Notice>,
    /// The sessions whose records hold what is searched for.
    pub found: HashSet<u64>,
    /// The settings page's tab.
    pub settings_tab: crate::pages::Tab,
    /// Adding a project: the address to clone, the name of one to make,
    /// the folder either goes in, a clone going on and what went wrong.
    pub clone_url: Entity<TextInput>,
    pub new_name: Entity<TextInput>,
    /// A project on another machine, typed as `host:/path`, and the
    /// machines your SSH configuration names.
    pub remote_at: Entity<TextInput>,
    pub hosts: Vec<String>,
    pub parent: PathBuf,
    pub cloning: Option<hub::Install>,
    pub adding_trouble: Option<String>,
    /// The draft on screen goes to every agent that can run, once sent.
    pub fanout: bool,
    /// No window of Ghost's is in front.
    pub behind: bool,
    /// What keeps the Mac awake while an agent works, when it is kept so.
    pub awake: Option<std::process::Child>,
    /// Your terminals in the drawer under the thread, whether it is open,
    /// where keys go in it, the one they go to, and whether two show side
    /// by side.
    pub terminals: Vec<crate::terminal::Terminal>,
    pub drawer: bool,
    pub term_focus: gpui::FocusHandle,
    pub term_on: Option<u64>,
    pub split: bool,
    /// Minutes counted, for what is read every other one, or checked every
    /// five.
    pub pr_tick: u32,
    pub health_tick: u32,
    /// What you say of a file's lines: the line being commented on, the
    /// line to write it in, and those said, waiting to go to the agent.
    pub commenting: Option<(PathBuf, usize)>,
    pub comment: Entity<TextInput>,
    pub comments: Vec<(PathBuf, usize, String)>,
    /// The folders folded in the review's list of files.
    pub folded: std::collections::HashSet<String>,
    /// Test files are left out of the review's list.
    pub no_tests: bool,
    /// On an agent's page: the command of yours that starts it, a model id
    /// to add.
    pub agent_command: Entity<TextInput>,
    pub agent_model: Entity<TextInput>,
    /// The command palette (⌘K): whether it is open, and its line.
    pub palette: bool,
    pub palette_input: Entity<TextInput>,
    /// The line of the palette Return takes.
    pub palette_at: usize,
    /// What the composer offers that Return or Tab takes.
    pub offer_at: usize,
    /// How much each part of Ghost's home takes, and what the last tidying
    /// did, for the settings.
    pub sizes: Vec<(&'static str, u64)>,
    pub tidied: Option<String>,
    /// The search over the registry's agents, in the settings.
    pub agent_search: Entity<TextInput>,
    /// What is looked for among the settings.
    pub settings_search: Entity<TextInput>,
    /// What worktree branches begin with, as you write it in settings.
    pub prefix_input: Entity<TextInput>,
    /// A script being written in the drawer, and its name and command.
    pub writing: Option<runs::Writing>,
    pub script_name: Entity<TextInput>,
    pub script_command: Entity<TextInput>,
    /// The folders each agent has sessions in that are not projects.
    pub elsewhere: std::collections::BTreeMap<String, Vec<(PathBuf, usize)>>,
    /// What `@` can name: each folder's files, each project's open pull
    /// requests, read once when first wanted.
    pub files: HashMap<PathBuf, Vec<String>>,
    /// What the palette looks through, and the words it found.
    pub through: palette::Through,
    pub grep: Vec<(String, String)>,
    pub prs: HashMap<PathBuf, Vec<(u64, String)>>,
    pub issues: HashMap<PathBuf, Vec<(u64, String)>>,
    /// What each CLI of yours an agent runs says its version is.
    pub cli_versions: HashMap<PathBuf, String>,
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
    /// The project chosen on the start, for a session begun there.
    pub start_in: Option<PathBuf>,
    pub panes: Panes,
    pub input: Entity<TextInput>,
    /// Cards and lines opened in the thread, by key.
    pub expanded: HashSet<String>,
    /// Projects folded shut down the left.
    pub closed: HashSet<PathBuf>,
    /// What each project's folder holds, once read (see `read_projects`).
    pub held: HashMap<PathBuf, hub::Held>,
    /// The thread as a list drawn where it is seen, its lines, and the
    /// session they are of; an item to bring into view once it is drawn.
    pub list: gpui::ListState,
    pub lines: Vec<crate::thread::Line>,
    pub lines_of: Option<u64>,
    pub reveal: Option<usize>,
    /// Find over the thread (⌘F): whether it is open, its line, and which
    /// of what it found is on screen.
    pub finding: bool,
    pub find: Entity<TextInput>,
    pub found_at: usize,
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
    pub scope: crate::git::Scope,
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
                        || d.offer_keys(&e.keystroke, window, cx)
                })
                .unwrap_or(false);
            if took {
                cx.stop_propagation();
            }
        })
        .detach();
        let cmd_sends = settings.cmd_sends;
        mirror(&settings, cx);
        let input = cx.new(|cx| {
            let mut i = TextInput::new(cx, "Ask for anything, or give a task")
                .wrapping(8)
                .padded(5.);
            i.set_enter_breaks(cmd_sends);
            i
        });
        cx.subscribe(&input, |this, _, Submitted(text), cx| this.send(text, cx))
            .detach();
        cx.subscribe(&input, |this, _, crate::input::Stashed(text), cx| {
            this.stashed.push(text.clone());
            this.save_index();
            this.done(
                "Put aside: take it back from the stack under the line",
                None,
            );
            cx.notify();
        })
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
            this.offer_at = 0;
            if input.read(cx).text().contains('@') {
                this.want_offers();
            }
            cx.notify();
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
        let search = cx.new(|cx| TextInput::new(cx, "Search sessions").padded(4.).keeping());
        let reader = cx.new(|cx| TextInput::new(cx, "").wrapping(24).keeping());
        let menu_search = cx.new(|cx| {
            TextInput::new(cx, "Search, or a while: 3h")
                .padded(2.)
                .keeping()
        });
        cx.subscribe_in(
            &menu_search,
            window,
            |this, _, Submitted(text), window, cx| {
                match this.popup {
                    Popup::Session(key) => this.snooze_for(key, text, cx),
                    Popup::Branch => this.new_branch(text, cx),
                    // A list narrowed by what is typed: the first left.
                    Popup::Mode | Popup::Option(_) => {
                        this.choose_nth(0, cx);
                        this.close_popup(window, cx);
                    }
                    _ => {}
                }
            },
        )
        .detach();
        cx.subscribe(&menu_search, |_, _, _: &crate::input::Changed, cx| {
            cx.notify();
        })
        .detach();
        let script_name = cx.new(|cx| TextInput::new(cx, "dev").padded(4.).keeping());
        let script_command = cx.new(|cx| TextInput::new(cx, "npm run dev").padded(4.).keeping());
        let prefix = cx.new(|cx| {
            let mut i = TextInput::new(cx, "ghost/").padded(4.).keeping();
            i.set_text(&settings.branch_prefix, cx);
            i
        });
        cx.subscribe(&prefix, |this, input, _: &crate::input::Changed, cx| {
            this.settings.branch_prefix = input.read(cx).text();
            this.save_settings();
        })
        .detach();
        let message = cx.new(|cx| TextInput::new(cx, "Commit message").padded(4.).keeping());
        let rename = cx.new(|cx| {
            TextInput::new(cx, "Name")
                .sized(13.5, 20.)
                .padded(0.)
                .submitting_empty()
        });
        let comment = cx.new(|cx| TextInput::new(cx, "What of this line?").padded(3.));
        cx.subscribe(&comment, |this, _, Submitted(text), cx| {
            this.add_comment(text, cx);
        })
        .detach();
        let agent_command = cx.new(|cx| {
            TextInput::new(cx, "/path/to/agent --acp")
                .padded(4.)
                .keeping()
        });
        let agent_model = cx.new(|cx| TextInput::new(cx, "a model id").padded(4.).keeping());
        let palette_input = cx.new(|cx| {
            TextInput::new(cx, "Go to a session, or do something")
                .padded(4.)
                .submitting_empty()
        });
        cx.subscribe(
            &palette_input,
            |this, input, _: &crate::input::Changed, cx| {
                this.palette_at = 0;
                if this.through == palette::Through::Words {
                    let words = input.read(cx).text();
                    this.search_words(&words);
                }
                cx.notify();
            },
        )
        .detach();
        cx.subscribe(&palette_input, |this, _, crate::input::Stepped(by), cx| {
            this.step_palette(*by, cx);
        })
        .detach();
        cx.subscribe_in(
            &palette_input,
            window,
            |this, _, Submitted(_), window, cx| {
                this.run_act(this.palette_at, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &palette_input,
            window,
            |this, _, _: &crate::input::Cancelled, window, cx| {
                this.toggle_palette(window, cx);
            },
        )
        .detach();
        let agent_search = cx.new(|cx| {
            TextInput::new(cx, "Search the ACP registry")
                .padded(4.)
                .keeping()
        });
        cx.subscribe(&agent_search, |_, _, _: &crate::input::Changed, cx| {
            cx.notify();
        })
        .detach();
        let settings_search =
            cx.new(|cx| TextInput::new(cx, "Search settings").padded(4.).keeping());
        cx.subscribe(&settings_search, |_, _, _: &crate::input::Changed, cx| {
            cx.notify();
        })
        .detach();
        let clone_url = cx.new(|cx| {
            TextInput::new(cx, "https://github.com/owner/repo.git")
                .padded(6.)
                .keeping()
        });
        cx.subscribe(&clone_url, |this, _, Submitted(_), cx| this.start_clone(cx))
            .detach();
        let new_name = cx.new(|cx| TextInput::new(cx, "my-project").padded(6.).keeping());
        cx.subscribe(&new_name, |this, _, Submitted(_), cx| {
            this.create_project(cx);
        })
        .detach();
        let remote_at = cx.new(|cx| {
            TextInput::new(cx, "devbox:/home/me/app")
                .padded(6.)
                .keeping()
        });
        cx.subscribe(&remote_at, |this, _, Submitted(_), cx| this.add_remote(cx))
            .detach();
        let find = cx.new(|cx| {
            TextInput::new(cx, "Find in this session")
                .padded(2.)
                .keeping()
        });
        cx.subscribe(&find, |this, _, Submitted(_), cx| this.find_next(false, cx))
            .detach();
        cx.subscribe(&find, |this, _, SubmittedAside(_), cx| {
            this.find_next(true, cx);
        })
        .detach();
        cx.subscribe(&find, |this, _, _: &crate::input::Changed, cx| {
            this.found_at = usize::MAX;
            cx.notify();
        })
        .detach();
        cx.subscribe_in(
            &find,
            window,
            |this, _, _: &crate::input::Cancelled, window, cx| {
                this.toggle_find(window, cx);
            },
        )
        .detach();
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
                if this
                    .update(cx, |this, cx| {
                        this.take(msg, cx);
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

        let registry = registry::kept(&home).unwrap_or_default();
        hub::fetch(home.clone(), tx.clone());
        let mut desk = Self {
            home,
            settings,
            settings_refused,
            store_refused: None,
            renaming: None,
            renaming_project: None,
            renaming_here: false,
            zoom: None,
            tall: false,
            resizing_left: false,
            chosen: std::collections::BTreeSet::new(),
            closing: None,
            tunnels: Vec::new(),
            trail: Vec::new(),
            ahead: Vec::new(),
            trailing: false,
            recording: None,
            stopping: None,
            quit_asked: None,
            badge: 0,
            weights: Vec::new(),
            menu_search,
            reading: None,
            reader,
            rename,
            undo: None,
            waiting: Vec::new(),
            rewinding: None,
            asking_beside: None,
            choosing: false,
            submenu: None,
            closed_tabs: HashSet::new(),
            gone: Vec::new(),
            stashed: Vec::new(),
            notices: Vec::new(),
            found: HashSet::new(),
            settings_tab: crate::pages::Tab::General,
            clone_url,
            new_name,
            remote_at,
            hosts: Vec::new(),
            parent: projects::made_here(),
            cloning: None,
            adding_trouble: None,
            fanout: false,
            behind: false,
            awake: None,
            files: HashMap::new(),
            elsewhere: std::collections::BTreeMap::new(),
            terminals: Vec::new(),
            agent_command,
            agent_model,
            agent_search,
            settings_search,
            prefix_input: prefix,
            writing: None,
            script_name,
            script_command,
            sizes: Vec::new(),
            palette: false,
            palette_input,
            through: palette::Through::Everything,
            grep: Vec::new(),
            palette_at: 0,
            offer_at: 0,
            tidied: None,
            pr_tick: 0,
            health_tick: 0,
            commenting: None,
            comment,
            comments: Vec::new(),
            folded: std::collections::HashSet::new(),
            no_tests: false,
            drawer: false,
            term_focus: cx.focus_handle(),
            term_on: None,
            split: false,
            prs: HashMap::new(),
            issues: HashMap::new(),
            cli_versions: HashMap::new(),
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
            start_in: None,
            panes: Panes {
                left: true,
                right: true,
                whole: false,
            },
            input,
            expanded: HashSet::new(),
            closed: HashSet::new(),
            held: HashMap::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(800.)),
            lines: Vec::new(),
            lines_of: None,
            reveal: None,
            finding: false,
            find,
            found_at: 0,
            signing: None,
            aside: Aside::Session,
            search,
            searching: false,
            registry_read: None,
            scope: crate::git::Scope::All,
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
        // The slots on other machines stay as they are.
        let remote: Vec<Slot> = old
            .extract_if(.., |s| crate::remote::split(&s.entry.id).1.is_some())
            .collect();
        for id in &self.settings.enabled {
            if let Some(at) = old.iter().position(|s| &s.entry.id == id) {
                self.slots.push(old.remove(at));
            } else if let Some(entry) = self.entry(id) {
                self.slots.push(Slot::new(
                    entry.clone(),
                    &self.home,
                    self.settings.commands.get(id).cloned(),
                ));
            }
        }
        self.slots.extend(remote);
        self.start_ready();
    }

    /// Start each agent switched on that can run and has not run yet, so
    /// that the sessions it keeps are listed. One let go after running
    /// idle starts again when it is wanted, not here.
    fn start_ready(&mut self) {
        let home = self.home.clone();
        // One on another machine starts when it is wanted.
        for slot in &mut self.slots {
            if matches!(slot.stage, Stage::Ready)
                && slot.known.is_none()
                && crate::remote::split(&slot.entry.id).1.is_none()
            {
                slot.stage = Stage::Starting;
                hub::start(slot, &home, self.tx.clone());
            }
        }
    }

    /// Ask `agent`, if it is running and can list them, for the sessions
    /// it keeps in the projects.
    pub(crate) fn list_sessions(&self, agent: &str) {
        let Some(slot) = self.slot(agent) else { return };
        let (Some(running), Some(caps)) = (slot.running(), slot.caps()) else {
            return;
        };
        if caps.session_capabilities.list.is_none() {
            return;
        }
        // An agent here lists the projects here; one on another machine,
        // the projects there, by their paths there.
        let host = crate::remote::split(agent).1;
        let places = self
            .settings
            .folders
            .iter()
            .filter_map(|f| match (crate::remote::parse(f), host) {
                (None, None) => Some((f.clone(), f.clone())),
                (Some((h, path)), Some(on)) if h == on => Some((path, f.clone())),
                _ => None,
            })
            .collect();
        hub::list(
            running.clone(),
            agent.to_owned(),
            places,
            self.home.clone(),
            self.tx.clone(),
        );
    }

    /// Read what a session of `agent` offers before one is opened, and
    /// whether it is signed in, by opening one and deleting it again.
    pub(crate) fn probe_agent(&self, agent: &str) {
        let Some(slot) = self.slot(agent) else { return };
        let (Some(running), Some(caps)) = (slot.running(), slot.caps()) else {
            return;
        };
        let caps = &caps.session_capabilities;
        // Opened in a folder that is there: Ghost's home here, /tmp on
        // another machine.
        let dir = if crate::remote::split(agent).1.is_some() {
            PathBuf::from("/tmp")
        } else {
            self.home.clone()
        };
        hub::probe(
            running.clone(),
            agent.to_owned(),
            dir,
            caps.delete.is_some(),
            caps.close.is_some(),
            self.tx.clone(),
        );
    }

    /// Sign in to `agent` the way it offers: its own method when it lists
    /// one, else its own program in a terminal, where it signs in.
    pub fn sign_in_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        let Some(slot) = self.slot(agent) else { return };
        let method = slot.sign_ins().next().map(|m| m.id().0.to_string());
        if let Some(method) = method {
            self.sign_in(agent, &method, cx);
            return;
        }
        match slot.entry.own_cli(&self.home) {
            Some(cli) => {
                if let Err(e) = crate::macos::in_terminal(&cli, &[], &[]) {
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

    /// Whether `agent` can delete the sessions it keeps, as it said when it
    /// last started.
    pub fn can_delete(&self, agent: &str) -> bool {
        self.slot(agent)
            .and_then(Slot::caps)
            .is_some_and(|k| k.session_capabilities.delete.is_some())
    }

    /// Start `agent` if it is not running or starting: something waits on
    /// it.
    pub(crate) fn start_for(&mut self, agent: &str) {
        let home = self.home.clone();
        let tx = self.tx.clone();
        if let Some(slot) = self.slot_mut(agent)
            && matches!(slot.stage, Stage::Ready)
        {
            slot.stage = Stage::Starting;
            hub::start(slot, &home, tx);
        }
    }

    /// A key no session has had.
    pub(crate) fn take_key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key - 1
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.registry
            .agents
            .iter()
            .find(|e| e.id == id)
            .or_else(|| self.slot(id).map(|s| &s.entry))
    }

    /// The slot of `agent` on the host of a project on another machine,
    /// made the first time it is wanted; its id.
    pub(crate) fn remote_slot(&mut self, agent: &str, host: &str) -> Option<String> {
        let id = crate::remote::agent_on(agent, host);
        if self.slot(&id).is_none() {
            let base = self.registry.agents.iter().find(|e| e.id == agent)?.clone();
            let command = crate::remote::command(&base, host)?;
            let mut entry = base;
            entry.name = format!("{} on {host}", self.name_of(agent));
            entry.id.clone_from(&id);
            let slot = Slot::new(entry, &self.home, Some(command));
            self.slots.push(slot);
        }
        Some(id)
    }

    pub fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.entry.id == id)
    }

    fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.entry.id == id)
    }

    pub fn session(&self) -> Option<&Session> {
        let key = self.current?;
        self.session_at(key)
    }

    pub(crate) fn session_at(&self, key: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.key == key)
    }

    pub(crate) fn session_mut(&mut self, key: u64) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.key == key)
    }

    /// The session `agent` knows as `id`.
    pub(crate) fn session_by_id(
        &mut self,
        agent: &str,
        id: &agent_client_protocol::v1::schema::SessionId,
    ) -> Option<&mut Session> {
        self.sessions
            .iter_mut()
            .find(|s| s.agent == agent && s.id.as_ref() == Some(id))
    }

    pub(crate) fn save_settings(&mut self) {
        if self.settings_refused.is_some() {
            return;
        }
        self.settings.save(&self.home);
    }

    /// The project a new session starts in: that of the session on
    /// screen, else the one chosen on the start, else the first listed.
    pub fn chosen_project(&self) -> Option<PathBuf> {
        self.session()
            .map(|s| s.project.clone())
            .or_else(|| self.start_in.clone())
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
        for s in &self.sessions {
            if !out.contains(&s.project) {
                out.push(s.project.clone());
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
                self.held.insert(folder.clone(), hub::Held::default());
                hub::read_project(folder, self.tx.clone());
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
                    self.open_agents(cx);
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
        let key = if let Some(s) = self
            .sessions
            .iter()
            .find(|s| s.agent == agent && s.project == folder && s.state == State::Draft)
        {
            s.key
        } else {
            let key = self.take_key();
            let s = self.begun_in(key, agent, folder);
            self.sessions.push(s);
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
            s.folder = path;
        } else {
            hub::look(s.folder.clone(), key, self.tx.clone());
        }
        if let Some(slot) = self.slot(agent) {
            slot.seed(&mut s);
        }
        s
    }

    /// Put the session `key` on screen.
    pub fn show(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.bring(key, cx);
        window.focus(&self.input.focus_handle(cx));
    }

    /// Put the session `key` on screen, opening it if it is closed.
    pub(crate) fn bring(&mut self, key: u64, cx: &mut Context<Self>) {
        if self.current != Some(key) {
            crate::thread::select::clear();
            // Where you were, to go back to; going back keeps the way ahead.
            if !self.trailing {
                if let Some(old) = self.current {
                    self.trail.push(old);
                }
                self.ahead.clear();
            }
            // What was being written stays with its session, and the
            // session shown brings back what was being written in it.
            let typed = self.input.read(cx).text();
            if let Some(old) = self.current
                && let Some(s) = self.session_mut(old)
            {
                s.unsent = typed;
                s.seen = Some(s.items.len());
            }
            let unsent = self
                .session_at(key)
                .map(|s| s.unsent.clone())
                .unwrap_or_default();
            self.input.update(cx, |i, cx| i.set_text(&unsent, cx));
            self.aside = Aside::Session;
            self.scope = crate::git::Scope::All;
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
        self.refresh_git(key);
        let closed = self
            .session_at(key)
            .is_some_and(|s| s.state == State::Closed && !s.recorded);
        if closed {
            self.look(key);
            self.read_record(key);
        }
        self.to_bottom();
        cx.notify();
    }

    /// Begin a session in the project on screen.
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
                .map(|s| crate::remote::split(&s.agent).0.to_owned());
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
                None => self.open_agents(cx),
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
            .or_else(|| self.session().and_then(|s| self.slot(&s.agent)))
            .filter(|s| s.usable())
            .or_else(|| self.slots.iter().find(|s| s.usable()))
            .or_else(|| self.slots.first())
            .map(|s| s.entry.id.clone());
        match agent {
            Some(agent) => self.new_session(&agent, folder, window, cx),
            None => self.open_agents(cx),
        }
    }

    /// Begin the draft on screen with another agent.
    pub fn switch_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        if let Some(key) = self.current
            && let Some(s) = self.session_mut(key)
            && s.state == State::Draft
        {
            // Where it works stays, and what git said of it.
            let mut new = Session::new(key, agent, s.folder.clone());
            new.project = std::mem::take(&mut s.project);
            (new.branch, new.repo) = (s.branch.take(), s.repo);
            new.scripts = std::mem::take(&mut s.scripts);
            new.wants_worktree = s.wants_worktree;
            *s = new;
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
        // Its subagents, and their records, go with it.
        let kids: Vec<u64> = self
            .sessions
            .iter()
            .filter(|c| c.parent == Some(key))
            .map(|c| c.key)
            .collect();
        for kid in kids {
            self.forget_session(kid, cx);
        }
        let Some(at) = self.sessions.iter().position(|s| s.key == key) else {
            return;
        };
        if let Some(id) = &self.sessions[at].id {
            let path = crate::store::record_path(&self.home, &self.sessions[at].agent, &id.0);
            crate::store::queue(crate::store::Job::Remove(path));
        }
        let s = self.sessions.remove(at);
        self.save_index();
        // Its checkpoints go with it.
        if s.repo {
            let (dir, uid) = (
                s.worktree
                    .as_ref()
                    .map_or_else(|| s.project.clone(), |w| w.repo.clone()),
                s.uid(),
            );
            crate::acp::runtime().spawn_blocking(move || crate::git::drop_checkpoints(&dir, &uid));
        }
        // Its worktree goes too, unless another session works in it (a
        // fork, or a new session begun there).
        if let Some(wt) = s.worktree
            && !self
                .sessions
                .iter()
                .any(|o| o.worktree.as_ref().is_some_and(|w| w.path == wt.path))
        {
            hub::git_action(
                wt.repo.clone(),
                hub::GitAction::RemoveWorktree(wt),
                self.settings.branch_prefix.clone(),
                Vec::new(),
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
    /// Whether the project `folder` can be taken off the list: no session
    /// of any kind is in it, or it would come back with that session.
    pub fn can_forget_project(&self, folder: &Path) -> bool {
        !self.sessions.iter().any(|s| s.project == folder)
    }

    pub fn forget_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        if !self.can_forget_project(folder) {
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
        let s = self.begun_in(key, &agent, folder);
        self.sessions.push(s);
        self.show(key, window, cx);
    }

    /// Say `text` in the session on screen, as if typed.
    pub fn say(&mut self, text: &str, cx: &mut Context<Self>) {
        self.send(text, cx);
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
                    if let Some(s) = this.session_mut(key)
                        && s.state == State::Draft
                    {
                        for p in paths {
                            if p != s.folder && !s.also.contains(&p) {
                                s.also.push(p);
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
                let _ = this.update(cx, |this, cx| {
                    if let Some(s) = this.session_mut(key)
                        && s.state == State::Draft
                    {
                        let wants = s.wants_worktree;
                        *s = Session::new(key, &s.agent.clone(), folder.clone());
                        s.wants_worktree = wants;
                        this.look(key);
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
        if let Some(project) = self.project() {
            self.new_in(project, window, cx);
        }
    }

    /// Begin, beside the session `key`, a session with each other agent
    /// that can run, in the same project, and say `text` in each.
    fn fan_out(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        let Some(origin) = self.session_at(key) else {
            return;
        };
        // Each goes as the first did: begun the same way (its worktree, the
        // machine it is on) and with the same images and files named.
        let (agent, folder, shared) = (
            origin.agent.clone(),
            origin.project.clone(),
            origin.attached.clone(),
        );
        let others: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.entry.id != agent && s.usable())
            .map(|s| s.entry.id.clone())
            .collect();
        for other in others {
            let new = self.take_key();
            let mut s = self.begun_in(new, &other, folder.clone());
            s.attached.clone_from(&shared);
            self.sessions.push(s);
            self.send_in(new, text, cx);
        }
    }

    /// Say what was written for later and is due now, in its session;
    /// one said each day is kept for the next.
    fn send_due(&mut self, cx: &mut Context<Self>) {
        let now = crate::store::secs(std::time::SystemTime::now());
        let mut due = Vec::new();
        for s in &mut self.sessions {
            for l in &mut s.later {
                if l.at <= now {
                    due.push((s.key, l.text.clone()));
                    while l.daily && l.at <= now {
                        l.at += 86_400;
                    }
                }
            }
            s.later.retain(|l| l.at > now);
        }
        if due.is_empty() {
            return;
        }
        for (key, text) in due {
            match self.session_mut(key) {
                // A turn going on: it waits its turn, as one queued.
                Some(s) if matches!(s.state, State::Working | State::Opening) => {
                    s.queued.push(text);
                }
                Some(_) => self.send_in(key, &text, cx),
                None => {}
            }
        }
        self.save_index();
        cx.notify();
    }

    /// Say what is in the composer later in the session on screen, when
    /// `when` says.
    pub fn send_later(&mut self, when: Later, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_owned();
        let now = std::time::SystemTime::now();
        let at = when.at(now);
        let Some(key) = self.current else { return };
        if text.is_empty() {
            return;
        }
        if let Some(s) = self.session_mut(key) {
            s.later.push(crate::store::Later {
                at: crate::store::secs(at),
                text,
                daily: when == Later::Daily,
            });
            s.later.sort_by_key(|l| l.at);
        }
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.popup = Popup::Closed;
        self.save_index();
        self.done(
            format!("To be said at {}", crate::when::said_at(at, now)),
            None,
        );
        cx.notify();
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
            // the one going on ends, with what goes with it.
            session.queued.push(text.clone());
            let last = session.queued.len() - 1;
            let attached = self.attach(key, &text);
            if let Some(s) = self.session_mut(key) {
                s.attached.extend(attached);
            }
            self.save_index();
            // Or it goes in now, the turn stopped for it, as you asked.
            if self.settings.steer {
                self.steer(last, cx);
            }
            cx.notify();
            return;
        }
        if session.state == State::Closed {
            // Its agent opens it again (resumed from the record, or loaded),
            // and then it goes; after what was queued before, if anything
            // was (kept from an earlier run).
            if !session.recorded {
                session.forget();
            }
            let text = if session.queued.is_empty() {
                text
            } else {
                session.queued.push(text);
                session.queued.remove(0)
            };
            session.pending = Some(text.clone());
            session.state = State::Opening;
            let agent = session.agent.clone();
            // What goes with it (images, files named) goes with it.
            if on_screen {
                let attached = self.attach(key, &text);
                if let Some(s) = self.session_mut(key) {
                    s.attached.extend(attached);
                }
            }
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
        let pictures: Vec<_> = attached
            .iter()
            .filter(|b| matches!(b, agent_client_protocol::v1::schema::ContentBlock::Image(_)))
            .cloned()
            .collect();
        session.attached.extend(attached);
        // Something new said in a session done for now brings it back.
        session.flags.settled = None;
        session.flags.snoozed = None;
        session.said(&text);
        // The images it took show under it, and are kept with its record.
        for p in pictures {
            session.apply(
                agent_client_protocol::v1::schema::SessionUpdate::UserMessageChunk(
                    agent_client_protocol::v1::schema::ContentChunk::new(p),
                ),
            );
        }
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
            let worktree = session.needs_worktree();
            self.settings.used(&folder);
            self.save_settings();
            if worktree {
                let from = self.session_at(key).and_then(|s| s.begin.clone());
                hub::make_worktree(
                    folder,
                    text,
                    self.home.clone(),
                    self.settings.branch_prefix.clone(),
                    from,
                    key,
                    self.tx.clone(),
                );
            } else {
                self.open_when_running(&agent, key);
            }
        }
        self.to_bottom();
        cx.notify();
    }

    /// Open the session `key` once its agent runs, starting it if need be.
    fn open_when_running(&mut self, agent: &str, key: u64) {
        // One on another machine has no slot after a restart until wanted.
        if self.slot(agent).is_none()
            && let (base, Some(host)) = crate::remote::split(agent)
        {
            let (base, host) = (base.to_owned(), host.to_owned());
            self.remote_slot(&base, &host);
        }
        let home = self.home.clone();
        let tx = self.tx.clone();
        let (dirs, id, recorded) = self
            .session_at(key)
            .map(|s| ((s.folder.clone(), s.also.clone()), s.id.clone(), s.recorded))
            .unwrap_or_default();
        let Some(slot) = self.slot_mut(agent) else {
            return;
        };
        let (resume, load) = slot.caps().map_or((false, false), |c| {
            (c.session_capabilities.resume.is_some(), c.load_session)
        });
        match &slot.stage {
            Stage::Running { agent } => match id {
                Some(id) if recorded && resume => {
                    hub::reopen(agent.clone(), key, id, dirs, true, tx);
                }
                Some(id) if load => {
                    let agent = agent.clone();
                    if recorded {
                        self.drop_record(key);
                    }
                    hub::reopen(agent, key, id, dirs, false, tx);
                }
                Some(_) => {
                    if let Some(s) = self.session_mut(key) {
                        s.state = State::Broken("The agent cannot open a session again".into());
                    }
                }
                None => hub::open(agent.clone(), key, dirs, tx),
            },
            Stage::Starting => {}
            _ => {
                slot.stage = Stage::Starting;
                hub::start(slot, &home, tx);
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
                    Item::Asked(a) if a.chosen.is_none() => Some(a.id),
                    _ => None,
                })
                .collect();
            for request in open {
                hub::answer(running, request, None);
            }
            hub::cancel(running, id);
            if self.stopping.is_none_or(|(k, _)| k != s.key) {
                self.stopping = Some((s.key, std::time::Instant::now()));
            }
        }
        cx.notify();
    }

    /// Stop the task `task` the agent left going in the session `key`.
    pub fn stop_task(&mut self, key: u64, task: String, cx: &mut Context<Self>) {
        let Some(s) = self.session_at(key) else {
            return;
        };
        if let (Some(running), Some(id)) =
            (self.slot(&s.agent).and_then(Slot::running), s.id.clone())
        {
            hub::stop_task(running.clone(), id, task);
        }
        cx.notify();
    }

    /// Stop the work of the subagent `key`, where its agent lets it be.
    pub fn stop_sub(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(c) = self.session_at(key) else {
            return;
        };
        if let (Some(running), Some(id)) =
            (self.slot(&c.agent).and_then(Slot::running), c.id.clone())
        {
            hub::cancel(running, id);
        }
        cx.notify();
    }

    /// Stop by force the agent of the session `key`, which would not stop
    /// its turn: its process is ended, and started again when wanted.
    pub fn force_stop(&mut self, key: u64, cx: &mut Context<Self>) {
        self.stopping = None;
        let agent = self.session_at(key).map(|s| s.agent.clone());
        if let Some(running) = agent
            .as_deref()
            .and_then(|a| self.slot(a))
            .and_then(Slot::running)
        {
            running.kill();
        }
        cx.notify();
    }

    /// Whether Ghost may quit now: at once with no agent at work, else at
    /// the second asking within a few seconds.
    pub fn may_quit(&mut self) -> bool {
        let working = self
            .sessions
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
            Some("Quitting stops their turns. ⌘Q again to quit.".into()),
            None,
        );
        false
    }

    /// Answer the question asked under `request` with the option `option`.
    pub fn choose(
        &mut self,
        request: u64,
        option: &agent_client_protocol::v1::schema::PermissionOption,
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
    /// With no turn going on (the session closed, as after a restart),
    /// it is said at once.
    pub fn steer(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let Some(s) = self.session_mut(key) else {
            return;
        };
        if n >= s.queued.len() {
            return;
        }
        let text = s.queued.remove(n);
        if matches!(s.state, State::Working | State::Opening) {
            s.queued.insert(0, text);
            s.items.push(Item::Note {
                text: "You stopped the turn to say what follows.".into(),
                trouble: false,
            });
            self.stop(cx);
        } else {
            self.send_in(key, &text, cx);
        }
    }

    /// Say the queued `n` one sooner.
    pub fn queue_earlier(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        if let Some(s) = self.session_mut(key)
            && n > 0
            && n < s.queued.len()
        {
            s.queued.swap(n, n - 1);
            self.save_index();
        }
        cx.notify();
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
                modes.current_mode_id = agent_client_protocol::v1::schema::SessionModeId::new(mode);
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
        self.submenu = None;
        // The menu's line says what it takes there.
        let says = match popup {
            Popup::Branch => "A new branch: its name, then Return",
            Popup::Session(_) => "Or for a while: 3h, 2d",
            _ => "Search",
        };
        self.menu_search.update(cx, |i, cx| {
            i.set_text("", cx);
            i.set_placeholder(says, cx);
        });
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
        let breaks = self.settings.cmd_sends;
        self.input.update(cx, |i, _| i.set_enter_breaks(breaks));
        mirror(&self.settings, cx);
        // Diffs read again, spacing in or out.
        if let Some(key) = self.current {
            self.refresh_diffs(key);
        }
    }

    /// Escape closes the menu open, else leaves the page open for the
    /// thread, wherever the keys go: true when it did.
    fn escape_closes(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if k.key != "escape" || k.modifiers.modified() {
            return false;
        }
        if self.popup != Popup::Closed {
            self.close_popup(window, cx);
        } else if self.page != Page::Thread && !self.palette {
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
        self.menu_search.update(cx, |i, cx| i.set_text("", cx));
        if self.menu_search.focus_handle(cx).is_focused(window) {
            window.focus(&self.input.focus_handle(cx));
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
        if k.modifiers.modified() || !self.input.focus_handle(cx).is_focused(window) {
            return false;
        }
        let text = self.input.read(cx).text();
        let Some(count) = self.offers(&text).map(|(_, o)| o.len()).filter(|n| *n > 0) else {
            return false;
        };
        match k.key.as_str() {
            "up" => self.offer_at = (self.offer_at + count - 1) % count,
            "down" => self.offer_at = (self.offer_at + 1) % count,
            "enter" | "tab" => self.take_offer(self.offer_at.min(count - 1), window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Take the keys pressed for the action being recorded: bound to it in
    /// your keybindings.json (a notice if another had them); Escape lets
    /// it be. Whether the keystroke was taken.
    pub(crate) fn record_key(&mut self, k: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(name) = self.recording else {
            return false;
        };
        if matches!(
            k.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function" | ""
        ) {
            return true;
        }
        self.recording = None;
        if k.key == "escape" && !k.modifiers.modified() {
            cx.notify();
            return true;
        }
        let key = k.unparse();
        if let Some(other) = crate::keys::taken(cx, name, &key) {
            self.trouble(
                format!("Those keys were “{other}”'s too"),
                Some("Yours now win; change that one if you want both.".into()),
                None,
            );
        }
        if let Err(why) = crate::keys::set_yours(cx, &self.home.clone(), name, Some(&key)) {
            self.trouble("Couldn't keep the keys", Some(why), None);
        }
        cx.notify();
        true
    }

    /// Settings, at the agents: to switch one on, or install one.
    pub fn open_agents(&mut self, cx: &mut Context<Self>) {
        self.settings_tab = crate::pages::Tab::Agents;
        self.open_page(Page::Settings, cx);
    }

    pub fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        // An agent's page shows the command of yours that starts it.
        if let Page::Agent(id) = &page {
            let c = self.settings.commands.get(id).cloned().unwrap_or_default();
            self.agent_command.update(cx, |i, cx| i.set_text(&c, cx));
            self.agent_model.update(cx, |i, cx| i.set_text("", cx));
            if let Some((_, cli)) = self.entry(id).and_then(crate::registry::Entry::your_cli) {
                hub::cli_version(cli, self.tx.clone());
            }
        }
        if page == Page::NewProject {
            self.hosts = crate::remote::hosts();
        }
        self.page = page;
        self.popup = Popup::Closed;
        cx.notify();
    }

    /// Install the agent `id` from its archive, or update it to the
    /// registry's version.
    pub fn install(&mut self, id: &str, cx: &mut Context<Self>) {
        // One running is let go first; it starts again once installed.
        if matches!(self.slot(id).map(|s| &s.stage), Some(Stage::Running { .. })) {
            self.let_go(id);
        }
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
                        slot.stage = Stage::Ready;
                        slot.restage(&home);
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
            slot.stage = Stage::Ready;
            slot.restage(&home);
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

    /// How wide the sidebar is now.
    pub fn left_width(&self) -> f32 {
        (self.settings.left as f32).clamp(LEFT_MOST.0, LEFT_MOST.1)
    }

    /// How wide the right column stands: wider for a file read in it, and
    /// wider again for its diff side by side.
    pub fn right_width(&self) -> f32 {
        match self.aside {
            Aside::File(_) if self.settings.side_by_side => 720.,
            Aside::File(_) => 560.,
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
        theme::set_light(match self.settings.look {
            settings::Look::Dark => false,
            settings::Look::Light => true,
            settings::Look::System => matches!(
                window.appearance(),
                gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
            ),
        });
        self.sync_lines();
        self.read_projects();
        let faces = fonts::faces(cx).clone();
        let lights = lights_room(window);
        let pane = f32::from(window.viewport_size().width)
            - if self.panes.left {
                self.left_width()
            } else {
                0.
            }
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
        // The terminals take the size the drawer gives them.
        let cell = crate::thread::drawer::cell_width(window, &faces);
        if self.drawer {
            let beside = if self.split {
                self.session_terminals()
                    .len()
                    .clamp(1, crate::thread::drawer::SIDE_BY_SIDE)
            } else {
                1
            };
            let (rows, cols) = crate::thread::drawer::fits(pane / beside as f32, cell);
            self.fit_terminals(rows, cols);
        }
        let middle = match self.page.clone() {
            Page::Thread => {
                let column = self.settings.width.column(pane);
                middle
                    .child(self.thread(&faces, column, cx))
                    .children(self.drawer(&faces, cell, cx))
                    .child(self.composer(&faces, column, window, cx))
            }
            Page::Agent(id) => middle.child(self.agent_page(&faces, &id, cx)),
            Page::Settings => middle.child(self.settings_page(&faces, cx)),
            Page::NewProject => middle.child(self.new_project_page(&faces, cx)),
        };
        let whole = self.panes.whole && self.page == Page::Thread && self.session().is_some();
        let right = match (&self.page, self.panes.right) {
            (Page::Thread, true) if whole => Some(self.aside(&faces, cx).flex_1()),
            (Page::Thread, true) if self.session().is_some() => {
                Some(self.aside(&faces, cx).w(px(self.right_width())))
            }
            (Page::Agent(id), _) => Some(self.about(&faces, id, cx)),
            _ => None,
        };
        fonts::with_fallbacks(div())
            .relative()
            .flex()
            .size_full()
            // The sidebar's edge dragged: its width follows, and is kept.
            .when(self.resizing_left, gpui::Styled::cursor_col_resize)
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                if this.resizing_left && e.pressed_button == Some(MouseButton::Left) {
                    let w = f32::from(e.position.x).clamp(LEFT_MOST.0, LEFT_MOST.1) as u32;
                    this.settings.left = w;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resizing_left {
                        this.resizing_left = false;
                        this.save_settings();
                        cx.notify();
                    }
                }),
            )
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
                d.child(self.sidebar(&faces, lights, cx))
            })
            .when(!whole, |d| d.child(middle))
            .children(right)
            // Under a menu or the palette, the rest of the window: a click
            // there closes it and does nothing else.
            .when(self.popup != Popup::Closed || self.palette, |d| {
                d.child(div().absolute().inset_0().occlude().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_popup(window, cx);
                        if this.palette {
                            this.toggle_palette(window, cx);
                        }
                    }),
                ))
            })
            .children(self.notices_view(&faces, cx))
            .children(self.palette_view(&faces, cx))
            .children(self.zoomed(&faces, window, cx))
            .children(self.reading_view(&faces, cx))
    }
}

impl Desk {
    /// Open a file the agent named (`src/a.rs`, `/x/y.rs:12`), from the
    /// folder of the session on screen, in your editor.
    pub fn open_named(&mut self, named: &str, cx: &mut Context<Self>) {
        let name = named.split(':').next().unwrap_or(named);
        let path = Path::new(name);
        let full = match self.session() {
            Some(s) if path.is_relative() => s.folder.join(path),
            _ => path.to_path_buf(),
        };
        if full.exists() {
            open_in_editor(&full, self.settings.editor.as_deref());
        } else {
            self.trouble(
                format!("There is no {name} here"),
                Some(full.display().to_string()),
                None,
            );
        }
        cx.notify();
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
    crate::thread::drawn::set_wrap(settings.diff_wrap);
    crate::git::leave_out_spacing(settings.no_spacing);
    crate::when::set_twelve(settings.twelve);
    theme::set_blue_orange(settings.blue_orange);
    theme::set_contrast(settings.contrast);
    fonts::set_mono(cx, settings.mono);
}

/// Open `path` in your editor: the one you chose (its command), else the
/// first of Cursor, VS Code, Zed and Sublime Text whose command is on the
/// PATH, else the app macOS opens text in.
pub fn open_in_editor(path: &Path, chosen: Option<&str>) {
    let editor = chosen
        .and_then(registry::which)
        .or_else(|| EDITORS.into_iter().find_map(|(c, _)| registry::which(c)));
    let spawned = match editor {
        Some(cmd) => std::process::Command::new(cmd).arg(path).spawn(),
        None => std::process::Command::new("open")
            .arg("-t")
            .arg(path)
            .spawn(),
    };
    if let Err(e) = spawned {
        warn!("{} could not be opened: {e}", path.display());
    }
}
