//! The desk: the whole window, and everything it knows.
//!
//! The first page is about tasks, not about anyone. Down the left, the way
//! a coding agent's app has it: controls under the window's lights, New,
//! Search and Attention, then the daemons the desk follows: this
//! machine's, with your sessions with Ghost, then each
//! connection with the characters it runs; and at the bottom what the
//! Ghost is running on. In the middle, the session's thread and the
//! line you type into, with how it is said and the brain it thinks with
//! under your hand. Down the right, Ghost's environment, its
//! context and its work. Opening a character puts her thread in the
//! middle, one place at a time, and her daemon and her work down the
//! right. Attention puts in the middle what every ghost is waiting on you
//! for.
//!
//! Nothing on screen is made up: every line is the daemon's, the
//! journal's or the deployment's, or a control that does what it says.
//! Where there is nothing yet, the page says so. The two side columns are
//! drawn in `sidebar.rs` and `aside.rs`.
//!
//! This file holds the desk's state, how it starts, and how the window is
//! laid out. What it follows on each daemon is in `desk/watched.rs`; going
//! between sessions and characters in `desk/navigate.rs`; the top bar in
//! `desk/topbar.rs`; the line you type into in `desk/composer.rs`; the
//! brain menu in `desk/brain_menu.rs`; and what the desk does to its
//! connections and the core it runs in `desk/actions.rs`. Answering and
//! stopping work is in `answer.rs`, and Attention in `attention.rs`.

mod actions;
mod brain_menu;
mod composer;
mod navigate;
mod topbar;
mod watched;

use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

use gpui::{
    Context, Entity, FocusHandle, Focusable as _, MouseButton, ScrollHandle, Window, div,
    prelude::*, px,
};
use tracing::warn;

pub(crate) use self::{
    brain_menu::{At, Menu, Picker},
    composer::Notice,
    watched::{Open, Watched},
};
use crate::{
    answer::{Replies, Whom},
    attention,
    brains::Preset,
    daemons::{Daemon, Host},
    follow::PlaceLooks,
    fonts,
    input::{Cancelled, Changed, Submitted, TextInput},
    link,
    model::LinkState,
    sessions::{Session, Sessions},
    settings::Settings,
    sheet::Sheet,
    solver::{Deployment, Provider},
    style::{BODY, COLUMN},
    theme, thread,
};

/// What the desk calls its own ghost until her daemon says her name.
pub const MASCOT: &str = "Ghost";

/// What the middle shows: the thread on screen, with the line you type
/// into, or Attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Thread,
    Attention,
}

/// The side columns' widths, the top bar's height.
pub const LEFT: f32 = 272.;
pub const RIGHT: f32 = 304.;
pub const TOPBAR: f32 = 52.;
/// Room the window's lights take at the left of the top row.
const LIGHTS: f32 = 76.;
/// What is left at the left of the top row in full screen, where there are
/// no lights to make room for.
const EDGE: f32 = 12.;

/// The room to leave for the window's lights. In full screen the system
/// hides them, and brings them back only with the title bar it slides down
/// over the window when the pointer reaches the top of the screen, so room
/// kept for them would be an empty hole. Going into or out of full screen
/// resizes the window, and a resize draws it again, so asking while drawing
/// is enough to follow it.
pub(crate) fn lights_room(window: &Window) -> f32 {
    if window.is_fullscreen() { EDGE } else { LIGHTS }
}

/// Which of the side columns show: the sidebar, and the right column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Panes {
    pub left: bool,
    pub right: bool,
}

/// A small menu of the desk's own, at most one open at a time. The brain
/// menu is apart: it opens from three places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Popup {
    Closed,
    /// The places of the character on screen, in the top bar.
    Places,
    /// A connection's menu, by its name.
    Connection(String),
    /// A session's menu, by its id, under its row in the sidebar.
    Session(String),
    /// The same menu under the session's name in the top bar.
    Title(String),
}

/// When the desk last asked itself what of her steps to read for the
/// thread on screen, and whether that thread shows something going on,
/// which breathes: the desk draws again a few times a second while it does.
struct OnScreen {
    asked: Instant,
    live: bool,
}

/// The form for adding a connection: whether it shows, its fields (name,
/// URL, token variable), and why the last try was refused.
pub(crate) struct Form {
    pub open: bool,
    pub fields: [Entity<TextInput>; 3],
    pub refused: Option<String>,
}

pub struct Desk {
    pub settings: Settings,
    pub solver: Watched,
    /// Every daemon the desk follows, this machine's first.
    pub(crate) daemons: Vec<Daemon>,
    /// The character on screen, if one is.
    pub(crate) open: Option<Open>,
    /// The small menu open over the desk, if one is.
    pub(crate) popup: Popup,
    /// The form for adding a connection.
    pub(crate) form: Form,
    pub(crate) deployment: Deployment,
    /// Whether the deployment has been looked after as the desk opened:
    /// started if it was to be.
    core_ready: bool,
    /// The brains the core has, as it last said, and which one it thinks
    /// with; and the presets it offers to add one from.
    pub(crate) providers: Vec<Provider>,
    pub(crate) presets: Vec<Preset>,
    pub(crate) brain: Option<String>,
    /// Your sessions with the solver, as its core keeps them, and the one
    /// on screen.
    pub sessions: Sessions,
    pub input: Entity<TextInput>,
    /// The sidebar's search, and whether it is open in place of its row.
    pub(crate) search: Entity<TextInput>,
    pub(crate) searching: bool,
    /// Whether anything has been typed into the search since it opened:
    /// emptying it again closes it, opening it does not.
    search_typed: bool,
    /// The field a session is renamed in, and which session, while one is.
    pub(crate) rename: Entity<TextInput>,
    pub(crate) renaming: Option<String>,
    /// The sheet a brain is added or edited in.
    pub(crate) sheet: Sheet,
    pub(crate) menu: Menu,
    pub(crate) menu_at: At,
    /// Where the keyboard goes while a menu is open, so that its keys
    /// steer the menu and not the line under it.
    menu_focus: FocusHandle,
    /// What the brain menu is showing: the brain looked at, the models
    /// each offers, and a switch being checked.
    pub(crate) picker: Picker,
    /// Cards and lines opened in the thread, and lists shown in full, by key.
    pub expanded: HashSet<String>,
    /// Where the thread on screen is scrolled to, and how far it goes.
    pub thread_scroll: ScrollHandle,
    /// Groups and sections folded away, by key, for as long as the desk runs.
    pub(crate) collapsed: HashSet<String>,
    /// A box to answer in for each piece of work waiting on an answer.
    pub(crate) replies: Replies,
    /// What is in the middle.
    pub(crate) page: Page,
    /// A line at the foot of the composer: what the desk just did or could not.
    notice: Option<Notice>,
    last_tick: Instant,
    /// What the desk last made of her steps in the thread on screen.
    steps: OnScreen,
    /// Which of the side columns show.
    pub(crate) panes: Panes,
    /// Sessions visited before and after the one on screen, by id, for
    /// back and forward.
    back: Vec<String>,
    forward: Vec<String>,
}

/// The core on this machine, as it was looked after off the main thread
/// while the desk opened.
struct Core {
    deployment: Deployment,
    started: anyhow::Result<()>,
}

/// What the line you type into says while it is empty, with the little
/// ghost, before its daemon has said its name.
const ASK: &str = "Ask Ghost for anything on this machine";

/// The daemon's token, from the environment or `.env`: never from a file
/// of the desk's own.
pub(crate) fn token() -> Option<String> {
    std::env::var("GHOSTD_TOKEN").ok().filter(|t| !t.is_empty())
}

/// A strip dragging which moves the window; a double click zooms it.
pub(crate) fn draggable() -> gpui::Div {
    div().on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 2 {
            window.titlebar_double_click();
        } else {
            window.start_window_move();
        }
    })
}

/// Whether something listens at a local address, asked quickly.
fn listening(url: &str) -> bool {
    let addr = url
        .trim_start_matches("ws://")
        .trim_start_matches("wss://")
        .split('/')
        .next()
        .unwrap_or_default();
    addr.parse::<std::net::SocketAddr>()
        .is_ok_and(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
}

impl Desk {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut settings = Settings::load();
        let sessions = Sessions::new(settings.open.clone());
        let input = cx.new(|cx| TextInput::new(cx, ASK).wrapping(6).padded(5.));
        cx.subscribe(&input, |this, _, Submitted(text), cx| {
            this.say(text.clone(), cx);
        })
        .detach();
        let search = cx.new(|cx| TextInput::new(cx, "Search sessions").dense());
        cx.subscribe_in(&search, window, |this, _, _: &Changed, window, cx| {
            this.search_changed(window, cx);
        })
        .detach();
        cx.subscribe_in(&search, window, |this, _, _: &Cancelled, window, cx| {
            this.close_search(window, cx);
        })
        .detach();
        cx.subscribe_in(&search, window, |this, _, Submitted(query), window, cx| {
            this.open_first_match(query, window, cx);
        })
        .detach();
        let rename = cx.new(|cx| TextInput::new(cx, "Name").dense());
        cx.subscribe_in(&rename, window, |this, _, Submitted(name), window, cx| {
            this.finish_rename(name, window, cx);
        })
        .detach();
        cx.subscribe_in(&rename, window, |this, _, _: &Cancelled, window, cx| {
            this.cancel_rename(window, cx);
        })
        .detach();
        let sheet = Sheet::new(window, cx);

        // Ghost on this machine is the desk's to look after: if
        // nothing runs it, the desk starts it. Whoever sits here is its
        // operator by holding its daemon's token, or by being on this
        // machine when it has none, so nobody is listed for it.
        //
        // That is done off the main thread, once the window is up: the
        // core makes its home ready before it runs, which the first time
        // means a birth. The window must not wait with it. Until it is
        // done, the deployment the desk holds is one with nothing started
        // and nothing read.
        let deployment = Deployment::find();
        let start = settings.solver_url == ghost_link::default_url();
        let url = settings.solver_url.clone();
        let core = link::runtime().spawn_blocking(move || {
            let mut deployment = Deployment::find();
            let started = if start && !listening(&url) {
                deployment.start()
            } else {
                Ok(())
            };
            Core {
                deployment,
                started,
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(core) = core.await {
                let _ = this.update(cx, |this, cx| this.take_core(core, cx));
            }
        })
        .detach();

        let solver = Watched::start(
            link::Config {
                url: settings.solver_url.clone(),
                token: token(),
                ghost: None,
                who: settings.who.clone(),
                sessions: true,
            },
            MASCOT,
        );
        let mut daemons = vec![Daemon::local(&settings.solver_url, token())];
        daemons.extend(settings.connections.iter().map(Daemon::remote));
        let followed: Vec<&Host> = daemons.iter().map(|d| &d.host).collect();
        if attention::forget_unfollowed(&mut settings.seen, &followed) {
            settings.save();
        }
        let connection_fields = [
            cx.new(|cx| TextInput::new(cx, "studio").field()),
            cx.new(|cx| TextInput::new(cx, "wss://host:7437").field()),
            cx.new(|cx| TextInput::new(cx, "STUDIO_TOKEN · optional").field()),
        ];
        for field in &connection_fields {
            cx.subscribe(field, |this, _, Submitted(_), cx| this.add_connection(cx))
                .detach();
            // Escape in any field puts the form away, as Cancel does.
            cx.subscribe_in(field, window, |this, _, _: &Cancelled, window, cx| {
                if this.form.open {
                    this.toggle_adding(window, cx);
                }
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                if this.update(cx, Desk::pump).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut desk = Self {
            settings,
            solver,
            daemons,
            open: None,
            popup: Popup::Closed,
            form: Form {
                open: false,
                fields: connection_fields,
                refused: None,
            },
            deployment,
            core_ready: false,
            providers: Vec::new(),
            presets: Vec::new(),
            brain: None,
            sessions,
            input,
            search,
            searching: false,
            search_typed: false,
            rename,
            renaming: None,
            sheet,
            menu: Menu::Closed,
            menu_at: At::Composer,
            menu_focus: cx.focus_handle(),
            picker: Picker::default(),
            expanded: HashSet::new(),
            thread_scroll: ScrollHandle::new(),
            collapsed: HashSet::new(),
            replies: Replies::new(),
            page: Page::Thread,
            notice: None,
            last_tick: Instant::now(),
            steps: OnScreen {
                asked: Instant::now(),
                live: false,
            },
            panes: Panes {
                left: true,
                right: true,
            },
            back: Vec::new(),
            forward: Vec::new(),
        };
        // The daemon the desk started goes when the desk does.
        cx.on_app_quit(|this, _| {
            this.deployment.stop();
            async {}
        })
        .detach();
        // `ghost-desk <task>` starts with that task in a new session, the
        // way a coding agent's CLI takes its first prompt.
        let task = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        if !task.trim().is_empty() {
            desk.new_session(cx);
            desk.say(task.trim().to_owned(), cx);
        }
        desk
    }

    /// Take in the core as it was looked after off the main thread.
    fn take_core(&mut self, core: Core, cx: &mut Context<Self>) {
        if let Err(e) = core.started {
            warn!("could not start Ghost: {e:#}");
            self.notice = Some(Notice::trouble(format!("Could not start Ghost: {e:#}")));
        }
        self.deployment = core.deployment;
        self.core_ready = true;
        // Its brains are asked of it once it listens, which a core just
        // started does within a few seconds; their models after them.
        self.refresh_brains(40, cx);
        cx.notify();
    }

    /// Whether the core on this machine is still coming up: being looked
    /// after as the desk opens, or started and not yet listening.
    pub(crate) fn core_starting(&self) -> bool {
        !self.core_ready || self.deployment.starting()
    }

    /// Take in everything the links have learned since last time, and tick
    /// once a second so that what is running shows its time.
    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.solver.pump() | crate::media::arrived();
        let mut forgot = false;
        let mut rostered = Vec::new();
        for daemon in &mut self.daemons {
            changed |= daemon.pump();
            if std::mem::take(&mut daemon.rostered) {
                rostered.push(daemon.host.clone());
            }
            for ghost in std::mem::take(&mut daemon.attended) {
                if let Some(items) = daemon.attention.get(&ghost) {
                    forgot |= attention::forget_gone(
                        &mut self.settings.seen,
                        &daemon.host,
                        &ghost,
                        items,
                    );
                }
            }
            if let Some(why) = daemon.refused.take() {
                self.notice = Some(Notice::trouble(why));
            }
        }
        if !rostered.is_empty() {
            let followed: Vec<&Host> = self.daemons.iter().map(|d| &d.host).collect();
            for host in &rostered {
                let Some(daemon) = self.daemons.iter().find(|d| d.host == *host) else {
                    continue;
                };
                let running: Vec<&str> = daemon.cast.ghosts.iter().map(|g| g.id.as_str()).collect();
                forgot |=
                    attention::forget_unrun(&mut self.settings.seen, &followed, host, &running);
            }
        }
        if forgot {
            self.settings.save();
        }
        if let Some(open) = &mut self.open {
            changed |= open.her.pump();
            if let Some(why) = open.her.refused.take() {
                self.notice = Some(Notice::trouble(why));
            }
        }
        if let Some(why) = self.solver.refused.take() {
            self.notice = Some(Notice::trouble(why));
        }
        for id in std::mem::take(&mut self.solver.unsessioned) {
            self.sessions.refused(&id);
        }
        if let Some(listed) = self.solver.sessions.take() {
            self.take_sessions(&listed);
            changed = true;
        }
        if changed {
            self.sync_replies(cx);
            // Read at its end, the thread stays at its end as it grows,
            // the way a chat does; scrolled up to read, it stays put.
            if self.at_end() {
                self.thread_scroll.scroll_to_bottom();
            }
        }
        if self.steps.asked.elapsed() >= FOLLOW {
            self.steps.asked = Instant::now();
            self.follow();
        }
        if self.steps.live || self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// Ask the daemon of the thread on screen for what it would show and
    /// does not have: the place's entries, what she called in her looks
    /// there, and what the work whose cards are open did. Called every
    /// [`FOLLOW`], and as soon as what is on screen changes.
    fn follow(&mut self) {
        self.steps.live = false;
        if self.attending() {
            return;
        }
        let session = thread::place_key(&self.session().id);
        let who = self.settings.who.clone();
        let whom = if self.open.is_some() {
            Whom::Her
        } else {
            Whom::Solver
        };
        let (ghost, place) = match &mut self.open {
            Some(open) => {
                let place = open.place(&who);
                (&mut open.her, place)
            }
            None => (&mut self.solver, session),
        };
        let wanted = thread::wanted(&thread_view(
            ghost,
            &place,
            &who,
            whom,
            &self.expanded,
            &self.thread_scroll,
        ));
        ghost.follow(&wanted);
        if let Some((work, open)) = crate::aside::context_work(&ghost.journal, &ghost.work, &place)
        {
            ghost.follow_context(&work, open);
        }
        let looking = ghost
            .following
            .places
            .get(&wanted.place)
            .and_then(PlaceLooks::running)
            .is_some();
        self.steps.live = wanted.thinking || looking || wanted.works.iter().any(|(_, open)| *open);
    }

    /// The session on screen. There is always one: the desk opens a new
    /// one when it has none.
    pub fn session(&self) -> Session {
        self.sessions.current()
    }

    /// Take in the sessions as the solver's core lists them now.
    fn take_sessions(&mut self, listed: &[ghost_link::protocol::Session]) {
        let core: Vec<Session> = listed.iter().map(Session::listed).collect();
        let was = self.sessions.current_id().to_owned();
        self.sessions.take(core);
        if self.sessions.current_id() != was {
            self.follow();
        }
    }

    /// Whether Attention is in the middle.
    pub(crate) fn attending(&self) -> bool {
        self.page == Page::Attention
    }

    pub fn who(&self) -> &str {
        &self.settings.who
    }

    /// Whether the thread is scrolled to its end, or near enough that what
    /// comes next would be read there.
    pub(crate) fn at_end(&self) -> bool {
        let max = self.thread_scroll.max_offset().height;
        let at = -self.thread_scroll.offset().y;
        max - at <= px(END_SLACK)
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }

    /// Fold a group or a section away, or open it again.
    pub(crate) fn toggle_collapsed(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    /// Put a line into the composer, to be read, changed or sent.
    pub fn prefill(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(text, cx));
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Show a file or directory in Finder, if it is still there.
    pub(crate) fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        if path.exists() {
            cx.reveal_path(path);
        } else {
            self.notice = Some(Notice::trouble(format!("{} is not there.", path.display())));
            cx.notify();
        }
    }
}

/// How near its end, in pixels, the thread counts as read to the end.
const END_SLACK: f32 = 48.;

/// How often, at most, the desk asks itself what of her steps to read:
/// well under the second a look is followed at.
const FOLLOW: Duration = Duration::from_millis(300);

/// Everything the thread needs to draw the place `place` in `ghost`'s
/// journal.
fn thread_view<'a>(
    ghost: &'a Watched,
    place: &'a str,
    who: &str,
    whom: Whom,
    expanded: &'a HashSet<String>,
    scroll: &'a ScrollHandle,
) -> thread::View<'a> {
    thread::View {
        journal: &ghost.journal,
        pending: &ghost.pending,
        stops: &ghost.stops,
        place,
        name: ghost.name(),
        who: who.to_owned(),
        tz: ghost.tz(),
        connected: ghost.link == LinkState::Connected,
        expanded,
        following: &ghost.following,
        work: &ghost.work,
        whom,
        scroll: Some(scroll),
        column: COLUMN,
        media: ghost.media(),
        status: ghost.status.as_ref(),
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
        let middle = if self.attending() {
            middle.child(self.attention_view(&faces, cx))
        } else {
            // Ghost's session, or the place her thread shows;
            // either way a place, by its key, in one ghost's journal.
            let (ghost, place) = match &self.open {
                Some(open) => (&open.her, open.place(self.who())),
                None => (&self.solver, thread::place_key(&self.session().id)),
            };
            let whom = if self.open.is_some() {
                Whom::Her
            } else {
                Whom::Solver
            };
            let mut view = thread_view(
                ghost,
                &place,
                self.who(),
                whom,
                &self.expanded,
                &self.thread_scroll,
            );
            // The pane is the window less the panes beside it; the column
            // is that less the thread's margins, up to its full width.
            let pane = f32::from(window.viewport_size().width)
                - if self.panes.left { LEFT } else { 0. }
                - if self.panes.right { RIGHT } else { 0. };
            view.column = (pane - 64.).clamp(240., COLUMN);
            // Only a place on the console channel is yours to stop her in.
            let busy = place.starts_with(&thread::place_key("")) && thread::busy(&view);
            middle
                .child(thread::render(&view, &faces, cx))
                .child(self.composer(&faces, busy, window, cx))
        };

        fonts::with_fallbacks(div())
            .relative()
            .flex()
            .size_full()
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(BODY))
            .text_color(theme::text())
            .when(self.panes.left, |d| d.child(self.left(&faces, lights, cx)))
            .child(middle)
            .when(self.panes.right, |d| d.child(self.right(&faces, cx)))
            .children(self.sheet_overlay(&faces, cx))
    }
}
