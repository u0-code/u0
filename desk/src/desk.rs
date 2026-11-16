//! The desk: the whole window, and everything it knows.
//!
//! The first page is about tasks, not about anyone. Down the left, the way
//! a coding agent's app has it: controls under the window's lights, New
//! and Search, then the daemons the desk follows: this machine's, with
//! your sessions with the solver, then each connection with the characters
//! it runs; and at the bottom what the solver is running on. In the
//! middle, the session's thread and the line you type into, with the brain
//! it thinks with under your hand. Down the right, the solver's
//! environment, its context and its work. Opening a character puts her
//! thread in the middle, one place at a time, and her daemon and her work
//! down the right.
//!
//! Nothing on screen is made up: every line is the daemon's, the
//! journal's or the deployment's, or a control that does what it says.
//! Where there is nothing yet, the page says so. The two side columns are
//! drawn in `sidebar.rs` and `aside.rs`.

use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

use chrono::Utc;
use chrono_tz::Tz;
use ghost_link::protocol::Work;
use gpui::{
    AnyElement, Context, Corner, Div, Entity, Focusable as _, FontWeight, MouseButton,
    SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use tracing::warn;

use crate::{
    daemons::{Daemon, Host},
    fonts::{self, Faces},
    input::{Cancelled, Changed, Submitted, TextInput},
    link::{self, Link},
    model::{Entry, LinkState, Message, PersonView, Status, Update},
    settings::{self, Connection, SOLVER_URL, Session, Settings},
    solver::{Abilities, Deployment, Provider},
    theme, thread, ui,
};

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

/// One ghost the desk follows on one daemon: the solver, or a character
/// opened from the left.
pub struct Watched {
    conn: Link,
    /// What to call her until the daemon says.
    fallback: String,
    pub link: LinkState,
    pub card: Option<ghost_link::protocol::GhostCard>,
    pub status: Option<Status>,
    pub people: Vec<PersonView>,
    /// Its journal's tail, oldest first. Every thread is read from it.
    pub journal: Vec<Entry>,
    /// What was said on the desk that the journal has not shown yet: your
    /// lines as you type them, its answers as they arrive.
    pub pending: Vec<Message>,
    /// The session it is typing in, if any.
    pub typing: Option<String>,
    pub desk_online: bool,
    /// Her work and what ended lately, as the daemon keeps it.
    pub work: Vec<Work>,
}

impl Watched {
    fn start(cfg: link::Config, fallback: &str) -> Self {
        Self {
            conn: Link::watch(cfg),
            fallback: fallback.to_owned(),
            link: LinkState::Connecting,
            card: None,
            status: None,
            people: Vec::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            typing: None,
            desk_online: false,
            work: Vec::new(),
        }
    }

    pub fn name(&self) -> String {
        self.status
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| self.card.as_ref().map(|c| c.name.clone()))
            .unwrap_or_else(|| self.fallback.clone())
    }

    /// Take in everything its link has learned. Says whether anything did.
    fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Ok(update) = self.conn.rx.try_recv() {
            self.take(update);
            changed = true;
        }
        changed
    }

    /// Say `text` to her in the place `place` on the desk channel, and
    /// show it until her journal does.
    fn say(&mut self, place: String, text: String) {
        self.pending.push(Message {
            place: place.clone(),
            hers: false,
            text: text.clone(),
            at: Utc::now(),
        });
        self.conn.say(place, text);
    }

    pub fn tz(&self) -> Tz {
        self.status.as_ref().map_or(chrono_tz::UTC, |s| s.tz)
    }

    /// How it stands with you: the person record for you on the desk.
    pub fn you(&self, who: &str) -> Option<&PersonView> {
        let key = link::person_key(who);
        self.people.iter().find(|p| p.key == key)
    }

    fn take(&mut self, update: Update) {
        match update {
            Update::Link(state) => {
                if state != LinkState::Connected {
                    self.desk_online = false;
                }
                self.link = state;
            }
            Update::Welcome(card) => self.card = Some(card),
            Update::Status(status) => self.status = Some(*status),
            Update::People(people) => self.people = people,
            Update::Journal(entries) => {
                self.journal = entries;
                let journal = &self.journal;
                self.pending.retain(|m| {
                    !journal
                        .iter()
                        .any(|e| thread::desk_message(e).as_ref() == Some(m))
                });
            }
            Update::Entry(entry) => {
                if self.journal.last().is_none_or(|last| last.seq < entry.seq) {
                    if let Some(m) = thread::desk_message(&entry)
                        && let Some(i) = self.pending.iter().position(|p| *p == m)
                    {
                        self.pending.remove(i);
                    }
                    if self.journal.len() >= link::TAIL as usize * 4 {
                        self.journal.remove(0);
                    }
                    self.journal.push(entry);
                }
            }
            Update::HerSay { place, text } => {
                self.typing = None;
                self.pending.push(Message {
                    place,
                    hers: true,
                    text,
                    at: Utc::now(),
                });
            }
            Update::HerTyping { place } => self.typing = Some(place),
            Update::DeskOnline(on) => self.desk_online = on,
            Update::Work(work) => self.work = work,
            Update::Roster(_) => {}
        }
    }
}

/// The kind of API being added from the brain menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiKind {
    OpenAi,
    Anthropic,
}

/// The brain menu, and the page of it that shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Menu {
    Closed,
    Brains,
    AddApi(ApiKind),
}

/// The control the brain menu opened from. It is one menu, anchored to
/// whichever of the three places to change the brain was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// The chip under the line you type into.
    Composer,
    /// The solver's row at the foot of the sidebar.
    Account,
    /// The Brain row of the environment, down the right.
    Aside,
}

/// A character opened from the left: who, on which daemon, the link that
/// follows her, and the place her thread shows.
pub struct Open {
    pub host: Host,
    pub id: String,
    pub url: String,
    pub her: Watched,
    /// The place on screen, by key, once one is chosen.
    pub place: Option<String>,
}

impl Open {
    /// The place her thread shows: the one chosen, or else the one
    /// something was last said in, or else yours with her on the desk.
    pub fn place(&self, who: &str) -> String {
        self.place
            .clone()
            .or_else(|| {
                thread::places(&self.her.journal)
                    .first()
                    .map(|s| s.key.clone())
            })
            .unwrap_or_else(|| link::person_key(who))
    }

    /// The places to pick from, the most recent first, with yours with her
    /// on the desk among them even before you have said anything.
    pub fn places(&self, who: &str) -> Vec<thread::Spot> {
        let mut places = thread::places(&self.her.journal);
        let yours = link::person_key(who);
        if !places.iter().any(|p| p.key == yours) {
            places.push(thread::Spot {
                key: yours,
                label: format!("{who} · {}", link::CHANNEL),
                at: chrono::DateTime::<Utc>::MIN_UTC,
            });
        }
        places
    }
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
    /// The brains the deployment names, and which one it thinks with.
    providers: Vec<Provider>,
    brain: Option<String>,
    /// What the deployment lets the solver do besides talk.
    pub(crate) abilities: Abilities,
    /// Index into the sessions.
    pub current: usize,
    pub input: Entity<TextInput>,
    /// The sidebar's search, and whether it is open in place of its row.
    pub(crate) search: Entity<TextInput>,
    pub(crate) searching: bool,
    /// Whether anything has been typed into the search since it opened:
    /// emptying it again closes it, opening it does not.
    search_typed: bool,
    /// The fields of an API being added: name, model, base URL, key variable.
    api: [Entity<TextInput>; 4],
    pub(crate) menu: Menu,
    pub(crate) menu_at: At,
    /// Cards and lines opened in the thread, and lists shown in full, by key.
    pub expanded: HashSet<String>,
    /// Groups and sections folded away, by key, for as long as the desk runs.
    pub(crate) collapsed: HashSet<String>,
    /// A line at the foot of the composer: what the desk just did or could not.
    notice: Option<String>,
    last_tick: Instant,
    /// Whether the sidebar shows.
    sidebar: bool,
    /// Sessions visited before and after the one on screen, by id, for
    /// back and forward.
    back: Vec<String>,
    forward: Vec<String>,
}

/// What the line you type into says while it is empty, with the solver.
const ASK: &str = "Ask for anything on this machine";

/// The daemon's token, from the environment or `.env`: never from a file
/// of the desk's own.
fn token() -> Option<String> {
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
        if settings.sessions.is_empty() {
            settings.sessions.push(Session::new());
        }
        let input = cx.new(|cx| TextInput::new(cx, ASK));
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
        let api = [
            cx.new(|cx| TextInput::new(cx, "name, e.g. openrouter")),
            cx.new(|cx| TextInput::new(cx, "model")),
            cx.new(|cx| TextInput::new(cx, "base URL")),
            cx.new(|cx| TextInput::new(cx, "key variable, e.g. OPENROUTER_API_KEY")),
        ];
        for field in &api {
            cx.subscribe(field, |this, _, Submitted(_), cx| this.add_api(cx))
                .detach();
        }

        // The solver on this machine is the desk's to look after: whoever
        // sits here may use its machine, and if nothing runs it, the desk
        // starts it.
        let mut deployment = Deployment::find();
        let mut notice = None;
        let local = settings.solver_url == SOLVER_URL;
        if local {
            match deployment.ensure_operator(link::CHANNEL, &settings.who) {
                Ok(true) if listening(&settings.solver_url) => {
                    notice = Some(
                        "You were made the solver's operator: restart its daemon for it to take effect."
                            .to_owned(),
                    );
                }
                Ok(_) => {}
                Err(e) => notice = Some(format!("{e:#}")),
            }
            if !listening(&settings.solver_url)
                && let Err(e) = deployment.start()
            {
                warn!("could not start the solver: {e:#}");
                notice = Some(format!("Could not start the solver: {e:#}"));
            }
        }
        let providers = deployment.providers();
        let brain = deployment.brain();
        let abilities = deployment.abilities();

        let mut solver = Watched::start(
            link::Config {
                url: settings.solver_url.clone(),
                token: token(),
                ghost: None,
                who: settings.who.clone(),
            },
            "Solver",
        );
        // The desk's channel is registered at once, so that the solver's
        // environment can say it is.
        solver.conn.speak();
        let mut daemons = vec![Daemon::local(&settings.solver_url, token())];
        daemons.extend(settings.connections.iter().map(Daemon::remote));
        let connection_fields = [
            cx.new(|cx| TextInput::new(cx, "name, e.g. studio")),
            cx.new(|cx| TextInput::new(cx, "wss://… or ws://127.0.0.1:…")),
            cx.new(|cx| TextInput::new(cx, "token variable, e.g. STUDIO_TOKEN")),
        ];
        for field in &connection_fields {
            cx.subscribe(field, |this, _, Submitted(_), cx| this.add_connection(cx))
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
            providers,
            brain,
            abilities,
            current: 0,
            input,
            search,
            searching: false,
            search_typed: false,
            api,
            menu: Menu::Closed,
            menu_at: At::Composer,
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            notice,
            last_tick: Instant::now(),
            sidebar: true,
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

    /// Take in everything the link has learned since last time, and tick
    /// once a second so that what is running shows its time.
    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.solver.pump();
        for daemon in &mut self.daemons {
            changed |= daemon.pump();
        }
        if let Some(open) = &mut self.open {
            changed |= open.her.pump();
        }
        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// The session on screen. There is always one: the desk opens a new
    /// one when it has none.
    pub fn session(&self) -> Session {
        self.settings
            .sessions
            .get(self.current)
            .or_else(|| self.settings.sessions.first())
            .cloned()
            .unwrap_or_else(Session::new)
    }

    pub fn who(&self) -> &str {
        &self.settings.who
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
            self.notice = Some(format!("{} is not there.", path.display()));
            cx.notify();
        }
    }

    /// Turn the Search row into a field and type into it.
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = true;
        self.search_typed = false;
        self.search.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Close the search, show every session again, and go back to the line
    /// you type into.
    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = false;
        self.search_typed = false;
        self.search.update(cx, |search, cx| search.set_text("", cx));
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    fn search_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = self.search.read(cx).is_empty();
        if empty && self.search_typed {
            self.close_search(window, cx);
            return;
        }
        self.search_typed = !empty;
        cx.notify();
    }

    /// Return in the search opens the first session it found.
    fn open_first_match(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&i) = settings::matching(&self.settings.sessions, query).first() {
            self.choose(i, cx);
        }
        self.close_search(window, cx);
    }

    pub fn new_session(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        let was = self.session().id;
        // An untouched new session is reused rather than stacked.
        if let Some(i) = self
            .settings
            .sessions
            .iter()
            .position(|s| s.title.is_empty())
        {
            self.current = i;
        } else {
            self.settings.sessions.insert(0, Session::new());
            self.current = 0;
            self.settings.save();
        }
        self.visited(was);
        cx.notify();
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_character(cx);
        let was = self.session().id;
        self.current = index.min(self.settings.sessions.len().saturating_sub(1));
        self.visited(was);
        cx.notify();
    }

    /// Remember the session just left, if it was left, for back.
    fn visited(&mut self, was: String) {
        if was != self.session().id {
            self.back.push(was);
            self.forward.clear();
        }
    }

    /// Where a session id is now.
    fn index_of(&self, id: &str) -> Option<usize> {
        self.settings.sessions.iter().position(|s| s.id == id)
    }

    /// Back to the session before, skipping any that are gone. From a
    /// character, back is the session behind her.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.close_character(cx);
            return;
        }
        while let Some(id) = self.back.pop() {
            if let Some(i) = self.index_of(&id) {
                self.forward.push(self.session().id);
                self.current = i;
                cx.notify();
                return;
            }
        }
    }

    /// Forward again, after going back.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        while let Some(id) = self.forward.pop() {
            if let Some(i) = self.index_of(&id) {
                self.back.push(self.session().id);
                self.current = i;
                cx.notify();
                return;
            }
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        cx.notify();
    }

    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        // To a character you speak in your own place with her on the desk
        // channel, and her thread turns to it.
        if let Some(open) = &mut self.open {
            let place = self.settings.who.to_lowercase();
            open.place = Some(thread::place_key(&place));
            open.her.say(place, text);
            cx.notify();
            return;
        }
        let Some(session) = self.settings.sessions.get_mut(self.current) else {
            return;
        };
        let place = session.id.clone();
        if session.title.is_empty() {
            session.title = text.chars().take(48).collect();
            self.settings.save();
        }
        self.solver.say(place, text);
        cx.notify();
    }

    /// Open the character `id` of the daemon `host`: follow her on a link
    /// of her own, and show her.
    pub(crate) fn open_character(&mut self, host: &Host, id: &str, cx: &mut Context<Self>) {
        if self
            .open
            .as_ref()
            .is_some_and(|o| o.host == *host && o.id == id)
        {
            return;
        }
        let Some(daemon) = self.daemons.iter().find(|d| d.host == *host) else {
            return;
        };
        let Ok(token) = daemon.token.clone() else {
            return;
        };
        let name = daemon
            .cast
            .ghosts
            .iter()
            .find(|g| g.id == id)
            .map_or_else(|| id.to_owned(), |g| g.card.name.clone());
        let her = Watched::start(
            link::Config {
                url: daemon.url.clone(),
                token,
                ghost: Some(id.to_owned()),
                who: self.settings.who.clone(),
            },
            &name,
        );
        self.open = Some(Open {
            host: host.clone(),
            id: id.to_owned(),
            url: daemon.url.clone(),
            her,
            place: None,
        });
        self.popup = Popup::Closed;
        self.input.update(cx, |input, cx| {
            input.set_placeholder(format!("Say something to {name}"), cx);
        });
        cx.notify();
    }

    /// Back to the solver: stop following the character on screen.
    fn close_character(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_some() {
            self.popup = Popup::Closed;
            self.input
                .update(cx, |input, cx| input.set_placeholder(ASK, cx));
            cx.notify();
        }
    }

    /// Show the place `key` in her thread.
    pub(crate) fn choose_place(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open {
            open.place = Some(key);
        }
        self.popup = Popup::Closed;
        cx.notify();
    }

    /// Add the connection the form describes, if it is a good one, and
    /// connect to it; or say in the form why not.
    pub(crate) fn add_connection(&mut self, cx: &mut Context<Self>) {
        let [name, url, var] = self.form.fields.clone().map(|f| f.read(cx).text());
        match Connection::check(&name, &url, &var, &self.settings.connections) {
            Ok(connection) => {
                self.daemons.push(Daemon::remote(&connection));
                self.settings.connections.push(connection);
                self.settings.save();
                for f in &self.form.fields {
                    f.update(cx, |f, cx| f.set_text("", cx));
                }
                self.form.open = false;
                self.form.refused = None;
            }
            Err(why) => self.form.refused = Some(why),
        }
        cx.notify();
    }

    /// Open the form for a connection, or close it.
    pub(crate) fn toggle_adding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.open = !self.form.open;
        self.form.refused = None;
        if self.form.open {
            self.form.fields[0].focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// Forget the connection `name`: the desk stops following it, and her
    /// thread closes if she was on it. Its daemon is not touched.
    pub(crate) fn remove_connection(&mut self, name: &str, cx: &mut Context<Self>) {
        let host = Host::Remote(name.to_owned());
        self.settings.connections.retain(|c| c.name != name);
        self.settings.save();
        self.daemons.retain(|d| d.host != host);
        if self.open.as_ref().is_some_and(|o| o.host == host) {
            self.close_character(cx);
        }
        self.popup = Popup::Closed;
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    /// Open the brain menu from one of the places it opens from, or close
    /// it if it is open there already.
    pub(crate) fn open_menu(&mut self, menu: Menu, at: At, cx: &mut Context<Self>) {
        let same = self.menu == menu && self.menu_at == at;
        self.menu = if same { Menu::Closed } else { menu };
        self.menu_at = at;
        if self.menu != Menu::Closed {
            self.providers = self.deployment.providers();
            self.brain = self.deployment.brain();
        }
        cx.notify();
    }

    /// Think with another brain from now on: written into the deployment,
    /// and the daemon restarted if the desk runs it.
    fn use_brain(&mut self, name: &str, cx: &mut Context<Self>) {
        self.menu = Menu::Closed;
        if let Err(e) = self.deployment.choose(name) {
            self.notice = Some(format!("{e:#}"));
            cx.notify();
            return;
        }
        self.brain = Some(name.to_owned());
        self.restart(cx);
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.notice = if self.deployment.owns_running() {
            match self.deployment.restart() {
                Ok(()) => Some("Restarting the solver with its new brain.".to_owned()),
                Err(e) => Some(format!("Could not restart the solver: {e:#}")),
            }
        } else {
            Some(
                "Saved. The solver's daemon was started elsewhere: restart it to use this brain."
                    .to_owned(),
            )
        };
        cx.notify();
    }

    /// Add one of the CLIs you are signed in to as a brain, and use it.
    fn add_cli(&mut self, kind: &str, cx: &mut Context<Self>) {
        let exists = self.providers.iter().any(|p| p.kind == kind);
        if !exists {
            let model = if kind == "claude-code" { "sonnet" } else { "" };
            if let Err(e) = self.deployment.add_provider(
                kind,
                &[("kind", kind), ("model", model), ("timeout_secs", "")],
            ) {
                self.notice = Some(format!("{e:#}"));
                cx.notify();
                return;
            }
        }
        let name = self
            .deployment
            .providers()
            .into_iter()
            .find(|p| p.kind == kind)
            .map_or_else(|| kind.to_owned(), |p| p.name);
        self.use_brain(&name, cx);
    }

    /// Add the API whose fields are filled in, and use it.
    fn add_api(&mut self, cx: &mut Context<Self>) {
        let Menu::AddApi(kind) = self.menu else {
            return;
        };
        let [name, model, base, key] = self.api.clone().map(|f| f.read(cx).text());
        if name.is_empty() || model.is_empty() {
            self.notice = Some("An API needs a name and a model.".to_owned());
            cx.notify();
            return;
        }
        let fields: Vec<(&str, &str)> = match kind {
            ApiKind::OpenAi => vec![
                ("kind", "openai"),
                ("model", &model),
                ("base_url", &base),
                ("api_key_env", &key),
            ],
            ApiKind::Anthropic => vec![
                ("kind", "anthropic"),
                ("model", &model),
                ("api_key_env", &key),
            ],
        };
        if let Err(e) = self.deployment.add_provider(&name, &fields) {
            self.notice = Some(format!("{e:#}"));
            cx.notify();
            return;
        }
        for f in &self.api {
            f.update(cx, |f, cx| f.set_text("", cx));
        }
        self.use_brain(&name, cx);
    }

    /// A small square control in the row beside the window's lights.
    fn control(id: &'static str, icon: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(enabled, |d| {
                d.cursor_pointer().hover(|s| s.bg(theme::hover()))
            })
            .child(ui::icon(icon, 16.).text_color(if enabled {
                theme::muted()
            } else {
                theme::ghost_line()
            }))
    }

    /// The controls beside the window's lights: the sidebar, back, forward.
    /// They sit at the top of the sidebar, or of the thread when it is hidden.
    pub(crate) fn controls(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                Self::control("sidebar", "sidebar", true)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                Self::control(
                    "back",
                    "chevron-left",
                    self.open.is_some() || !self.back.is_empty(),
                )
                .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
            )
            .child(
                Self::control("forward", "chevron-right", !self.forward.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
            )
    }

    /// The bar across the top of the thread: the session's name, the clock,
    /// and, when the sidebar is hidden, the controls beside the lights.
    fn topbar(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> impl IntoElement {
        // With a character open: her name, the place her thread shows, and
        // her clock; with the solver, the session's name and its clock.
        let (title, status, picker): (String, Option<&Status>, Option<Div>) = match &self.open {
            Some(open) => (
                open.her.name(),
                open.her.status.as_ref(),
                Some(self.place_picker(open, faces, cx)),
            ),
            None => (self.session().label(), self.solver.status.as_ref(), None),
        };
        let right = status.map(|s| {
            div()
                .flex()
                .items_baseline()
                .gap(px(6.))
                .child(ui::mono(faces, s.hhmm()).text_color(theme::text()))
                .child(ui::mono(faces, s.city.clone()))
        });
        draggable()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .h(px(TOPBAR))
            .pl(px(if self.sidebar { 24. } else { lights }))
            .pr(px(24.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .flex_1()
                    .min_w(px(0.))
                    .when(!self.sidebar, |d| d.child(self.controls(cx)))
                    .child({
                        let title = ui::text(faces, title)
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(13.5));
                        // Beside a picker her name keeps its width; alone,
                        // a session's name trails off.
                        if picker.is_some() {
                            title.flex_none()
                        } else {
                            ui::one_line(title)
                        }
                    })
                    .children(picker),
            )
            .children(right)
    }

    /// The place her thread shows, which opens a menu of every place in
    /// her journal's tail, the most recent first.
    fn place_picker(&self, open: &Open, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let who = self.who();
        let places = open.places(who);
        let on = open.place(who);
        let label = places
            .iter()
            .find(|p| p.key == on)
            .map_or_else(|| on.clone(), |p| p.label.clone());
        let chip = div()
            .id("place")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(10.))
            .min_w(px(0.))
            .rounded_full()
            .cursor_pointer()
            .when(self.popup == Popup::Places, |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(ui::one_line(
                ui::mono(faces, label)
                    .text_size(px(12.))
                    .text_color(theme::text2()),
            ))
            .child(ui::icon("chevron-down", 12.).text_color(theme::muted()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.popup = if this.popup == Popup::Places {
                    Popup::Closed
                } else {
                    Popup::Places
                };
                cx.notify();
            }));
        let mut picker = div().relative().flex_1().min_w(px(0.)).child(chip);
        if self.popup == Popup::Places {
            let mut menu = div()
                .id("place-menu")
                .occlude()
                .flex()
                .flex_col()
                .gap(px(2.))
                .w(px(300.))
                .max_h(px(420.))
                .overflow_y_scroll()
                .p(px(6.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .rounded(px(12.))
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.popup = Popup::Closed;
                    cx.notify();
                }));
            for (i, place) in places.into_iter().enumerate() {
                let here = place.key == on;
                let key = place.key.clone();
                menu = menu.child(
                    div()
                        .id(("place-item", i))
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(here, |d| d.bg(theme::panel2()))
                        .hover(|s| s.bg(theme::hover()))
                        .child(ui::one_line(
                            ui::text(faces, place.label).text_size(px(13.)),
                        ))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.choose_place(key.clone(), cx)),
                        ),
                );
            }
            // Under the chip, its left edge on the chip's.
            picker = picker.child(
                div().absolute().top(px(30.)).left_0().child(deferred(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu),
                )),
            );
        }
        picker
    }

    pub(crate) fn current_provider(&self) -> Option<Provider> {
        let brain = self.brain.as_deref()?;
        self.providers.iter().find(|p| p.name == brain).cloned()
    }

    pub(crate) fn menu_open_at(&self, at: At) -> bool {
        self.menu != Menu::Closed && self.menu_at == at
    }

    /// The brain menu, if it is open from `at`, anchored by `corner` to
    /// where the caller puts it.
    pub(crate) fn menu_over(
        &self,
        at: At,
        corner: Corner,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.menu_open_at(at).then(|| {
            deferred(
                anchored()
                    .anchor(corner)
                    .snap_to_window_with_margin(px(8.))
                    .child(self.brain_menu(faces, cx)),
            )
            .into_any_element()
        })
    }

    /// The menu of brains.
    fn brain_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let item = |id: SharedString, icon: &'static str, label: String, note: String, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(34.))
                .px(px(10.))
                .rounded(px(7.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon(icon, 15.).text_color(theme::brand(icon)))
                .child(ui::one_line(ui::text(faces, label).text_size(px(13.))))
                .child(
                    ui::mono(faces, note)
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .flex_none(),
                )
                .child(
                    ui::mono(faces, if on { "✓" } else { " " })
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .w(px(12.)),
                )
        };
        let mut menu = div()
            .id("brain-menu")
            .occlude()
            .flex()
            .flex_col()
            .gap(px(2.))
            .w(px(340.))
            .p(px(6.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .rounded(px(12.))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = Menu::Closed;
                cx.notify();
            }));

        if let Menu::AddApi(kind) = self.menu {
            let tab = |id: &'static str, label: &'static str, on: bool, k: ApiKind| {
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .h(px(26.))
                    .px(px(10.))
                    .rounded_full()
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        ui::text(faces, label)
                            .text_size(px(12.5))
                            .text_color(if on { theme::text() } else { theme::muted() }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = Menu::AddApi(k);
                        cx.notify();
                    }))
            };
            let field = |label: &'static str, input: &Entity<TextInput>| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        ui::mono(faces, label)
                            .text_size(px(10.5))
                            .text_color(theme::faint()),
                    )
                    .child(
                        div()
                            .px(px(10.))
                            .rounded(px(7.))
                            .bg(theme::bg())
                            .border_1()
                            .border_color(theme::border2())
                            .child(input.clone()),
                    )
            };
            menu = menu
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(4.))
                        .pb(px(4.))
                        .child(tab("api-openai", "OpenAI-compatible", kind == ApiKind::OpenAi, ApiKind::OpenAi))
                        .child(tab("api-anthropic", "Anthropic", kind == ApiKind::Anthropic, ApiKind::Anthropic)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .px(px(6.))
                        .child(field("NAME", &self.api[0]))
                        .child(field("MODEL", &self.api[1]))
                        .when(kind == ApiKind::OpenAi, |d| d.child(field("BASE URL", &self.api[2])))
                        .child(field("KEY VARIABLE", &self.api[3]))
                        .child(
                            ui::text(faces, "The key itself stays in your environment or .env; only its name is written.")
                                .text_size(px(11.5))
                                .text_color(theme::faint())
                                .line_height(px(16.)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .px(px(6.))
                        .py(px(6.))
                        .child(
                            div()
                                .id("api-back")
                                .flex()
                                .items_center()
                                .h(px(28.))
                                .px(px(12.))
                                .rounded(px(7.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::hover()))
                                .child(ui::text(faces, "Back").text_size(px(12.5)).text_color(theme::muted()))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.menu = Menu::Brains;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("api-add")
                                .flex()
                                .items_center()
                                .h(px(28.))
                                .px(px(12.))
                                .rounded(px(7.))
                                .cursor_pointer()
                                .bg(theme::accent())
                                .child(ui::text(faces, "Add and use").text_size(px(12.5)).text_color(theme::bg()))
                                .on_click(cx.listener(|this, _, _, cx| this.add_api(cx))),
                        ),
                );
        } else {
            menu = menu.child(
                ui::eyebrow(faces, "think with")
                    .px(px(10.))
                    .pt(px(4.))
                    .pb(px(2.)),
            );
            for p in &self.providers {
                // A CLI steps work with the solver's own tools, but cannot
                // step a conversation: it talks, and does the hands-on part
                // as work it takes on.
                let cli = p.kind == "claude-code" || p.kind == "codex";
                let note = if p.ready && cli {
                    "tools in tasks".to_owned()
                } else if p.ready {
                    p.name.clone()
                } else if cli {
                    "not installed".to_owned()
                } else {
                    format!("{} unset", p.key_env.as_deref().unwrap_or("key"))
                };
                let name = p.name.clone();
                menu = menu.child(
                    item(
                        SharedString::from(format!("brain-{}", p.name)),
                        p.icon(),
                        p.label(),
                        note,
                        self.brain.as_deref() == Some(p.name.as_str()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.use_brain(&name, cx))),
                );
            }
            let has = |kind: &str| self.providers.iter().any(|p| p.kind == kind);
            menu = menu.child(div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(theme::border()));
            menu = menu.child(ui::eyebrow(faces, "add").px(px(10.)).pt(px(2.)).pb(px(2.)));
            if !has("claude-code") {
                menu = menu.child(
                    item(
                        "add-claude".into(),
                        "claude",
                        "Claude Code".to_owned(),
                        "the CLI you are signed in to".to_owned(),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.add_cli("claude-code", cx))),
                );
            }
            if !has("codex") {
                menu = menu.child(
                    item(
                        "add-codex".into(),
                        "openai",
                        "Codex".to_owned(),
                        "the CLI you are signed in to".to_owned(),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.add_cli("codex", cx))),
                );
            }
            menu = menu.child(
                item(
                    "add-api".into(),
                    "openai",
                    "API…".to_owned(),
                    "OpenAI-compatible or Anthropic".to_owned(),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu = Menu::AddApi(ApiKind::OpenAi);
                    cx.notify();
                })),
            );
        }
        menu
    }

    /// The line you type into, the way a coding agent's is: a box with the
    /// text, and a row under it with the brain on the left and send on the
    /// right.
    fn composer(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = self.current_provider();
        let empty = self.input.read(cx).is_empty();
        // To a character the brain is hers, on her daemon; what the desk
        // can say under the line is who it speaks as.
        if self.open.is_some() {
            let note = format!("as {} · {} channel", self.who(), link::CHANNEL);
            return self.composer_with(
                ui::mono(faces, note).text_size(px(11.5)).px(px(6.)),
                empty,
                faces,
                cx,
            );
        }
        let (icon, label) = provider
            .as_ref()
            .map_or(("openai", "no brain".to_owned()), |p| (p.icon(), p.label()));
        let mut chip = div().relative().child(
            div()
                .id("brain")
                .flex()
                .items_center()
                .gap(px(7.))
                .h(px(28.))
                .px(px(10.))
                .rounded_full()
                .cursor_pointer()
                .when(self.menu_open_at(At::Composer), |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon(icon, 14.).text_color(theme::brand(icon)))
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .child(ui::icon("chevron-down", 12.).text_color(theme::muted()))
                .on_click(
                    cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, At::Composer, cx)),
                ),
        );
        if let Some(menu) = self.menu_over(At::Composer, Corner::BottomLeft, faces, cx) {
            chip = chip.child(div().absolute().top(px(-8.)).left(px(0.)).child(menu));
        }
        self.composer_with(chip, empty, faces, cx)
    }

    /// The composer's box, with `under` at the left of the row under the
    /// line and send at its right.
    fn composer_with(&self, under: Div, empty: bool, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let send = div()
            .id("send")
            .size(px(30.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if empty {
                theme::border2()
            } else {
                theme::accent()
            })
            .child(
                ui::mono(faces, "↑")
                    .text_size(px(15.))
                    .text_color(if empty { theme::muted() } else { theme::bg() }),
            )
            .on_click(cx.listener(|this, _, _, cx| this.send(cx)));

        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(32.))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(px(thread::COLUMN))
                    .px(px(14.))
                    .pt(px(4.))
                    .pb(px(10.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::border2())
                    .rounded(px(16.))
                    .child(div().px(px(4.)).child(self.input.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(under)
                            .child(send),
                    ),
            )
            .children(self.notice.as_ref().map(|n| {
                div()
                    .id("notice")
                    .w_full()
                    .max_w(px(thread::COLUMN))
                    .px(px(16.))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, n.clone())
                            .text_size(px(12.))
                            .text_color(theme::muted()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    }))
            }))
    }
}

impl Render for Desk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = fonts::faces(cx).clone();
        let lights = lights_room(window);
        // The solver's session, or the place her thread shows; either way
        // a place, by its key, in one ghost's journal.
        let (ghost, place, abilities) = match &self.open {
            Some(open) => (&open.her, open.place(self.who()), None),
            None => (
                &self.solver,
                thread::place_key(&self.session().id),
                Some(&self.abilities),
            ),
        };
        let typing = ghost
            .typing
            .as_deref()
            .is_some_and(|p| thread::place_key(p) == place);
        let view = thread::View {
            journal: &ghost.journal,
            pending: &ghost.pending,
            place: &place,
            name: ghost.name(),
            who: self.who().to_owned(),
            tz: ghost.tz(),
            typing,
            connected: ghost.link == LinkState::Connected,
            abilities,
            expanded: &self.expanded,
        };
        let thread = thread::render(&view, &faces, cx);
        let middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(self.topbar(&faces, lights, cx))
            .child(thread)
            .child(self.composer(&faces, cx));
        div()
            .flex()
            .size_full()
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(14.))
            .text_color(theme::text())
            .when(self.sidebar, |d| d.child(self.left(&faces, lights, cx)))
            .child(middle)
            .child(self.right(&faces, cx))
    }
}
