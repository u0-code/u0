//! The desk: the whole window, and everything it knows.
//!
//! The first page is about tasks, not about anyone. Down the left, the way
//! a coding agent's app has it: controls under the window's lights, New
//! and Search, then your sessions with the solver on this machine, and at
//! the bottom what the solver is running on. In the middle, the session's
//! thread and the line you type into, with the brain it thinks with under
//! your hand. Down the right, the solver's environment, its context and
//! its work. Characters, who live online and are somebody, come later.

use std::{
    collections::HashSet,
    sync::mpsc,
    time::{Duration, Instant},
};

use chrono::Utc;
use chrono_tz::Tz;
use gpui::{
    Context, Corner, Entity, FontWeight, MouseButton, SharedString, Window, anchored, deferred,
    div, prelude::*, px,
};
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    fonts::{self, Faces},
    input::{Submitted, TextInput},
    link::{self, Outgoing},
    model::{Entry, LinkState, Message, PersonView, Status, Update},
    settings::{SOLVER_URL, Session, Settings},
    solver::{Deployment, Provider},
    theme,
    thread::{self, State},
    ui,
};

/// The side columns' widths, the top bar's height.
pub const LEFT: f32 = 256.;
pub const RIGHT: f32 = 288.;
pub const TOPBAR: f32 = 44.;
/// The session that shows how the thread draws things, with nothing sent.
pub const DEMO: &str = "demo";

/// The solver, as the desk follows it: one ghost on one daemon.
pub struct Solver {
    rx: mpsc::Receiver<Update>,
    tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
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
}

impl Solver {
    fn start(url: &str, who: &str, token: Option<String>) -> Self {
        let (rx, tx) = link::start(link::Config {
            url: url.to_owned(),
            token,
            ghost: None,
            who: who.to_owned(),
        });
        Self {
            rx,
            tx,
            link: LinkState::Connecting,
            card: None,
            status: None,
            people: Vec::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            typing: None,
            desk_online: false,
        }
    }

    pub fn name(&self) -> String {
        self.status
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| self.card.as_ref().map(|c| c.name.clone()))
            .unwrap_or_else(|| "Solver".to_owned())
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
        }
    }
}

/// The kind of API being added from the brain menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKind {
    OpenAi,
    Anthropic,
}

/// The brain menu over the line you type into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Closed,
    Brains,
    AddApi(ApiKind),
}

pub struct Desk {
    pub settings: Settings,
    pub solver: Solver,
    deployment: Deployment,
    /// The brains the deployment names, and which one it thinks with.
    providers: Vec<Provider>,
    brain: Option<String>,
    /// Index into the sessions, the demo last.
    pub current: usize,
    pub input: Entity<TextInput>,
    /// The fields of an API being added: name, model, base URL, key variable.
    api: [Entity<TextInput>; 4],
    menu: Menu,
    /// Cards and lines opened in the thread, by key.
    pub expanded: HashSet<String>,
    /// A line at the foot of the composer: what the desk just did or could not.
    notice: Option<String>,
    demo: Vec<Entry>,
    last_tick: Instant,
}

/// The daemon's token, from the environment or `.env`: never from a file
/// of the desk's own.
fn token() -> Option<String> {
    std::env::var("GHOSTD_TOKEN").ok().filter(|t| !t.is_empty())
}

/// A strip dragging which moves the window; a double click zooms it.
fn draggable() -> gpui::Div {
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
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut settings = Settings::load();
        if settings.sessions.is_empty() {
            settings.sessions.push(Session::new());
        }
        let input = cx.new(|cx| TextInput::new(cx, "Ask for anything on this machine"));
        cx.subscribe(&input, |this, _, Submitted(text), cx| {
            this.say(text.clone(), cx);
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

        let solver = Solver::start(&settings.solver_url, &settings.who, token());
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
            deployment,
            providers,
            brain,
            current: 0,
            input,
            api,
            menu: Menu::Closed,
            expanded: HashSet::new(),
            notice,
            demo: demo_journal(),
            last_tick: Instant::now(),
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
        let mut changed = false;
        while let Ok(update) = self.solver.rx.try_recv() {
            self.solver.take(update);
            changed = true;
        }
        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// The session on screen: one of yours, or the demo after them.
    pub fn session(&self) -> Session {
        self.settings
            .sessions
            .get(self.current)
            .cloned()
            .unwrap_or_else(|| Session {
                id: DEMO.to_owned(),
                title: "How the thread draws things".to_owned(),
                at: Utc::now(),
            })
    }

    fn on_demo(&self) -> bool {
        self.current >= self.settings.sessions.len()
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

    pub fn new_session(&mut self, cx: &mut Context<Self>) {
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
        cx.notify();
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.current = index.min(self.settings.sessions.len());
        cx.notify();
    }

    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        if self.on_demo() {
            self.notice =
                Some("The demo session sends nothing. Start a new task to work.".to_owned());
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
        self.solver.pending.push(Message {
            place: place.clone(),
            hers: false,
            text: text.clone(),
            at: Utc::now(),
        });
        let _ = self.solver.tx.send(Outgoing { place, text });
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    fn open_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == menu {
            Menu::Closed
        } else {
            menu
        };
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

    /// A small square control in the sidebar's top row.
    fn control(
        id: &'static str,
        faces: &Faces,
        glyph: &'static str,
        on: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(ui::mono(faces, glyph).text_size(px(14.)).text_color(if on {
                theme::text2()
            } else {
                theme::ghost_line()
            }))
    }

    /// A row of the sidebar's menu: a glyph and a word.
    fn menu_row(
        id: &'static str,
        faces: &Faces,
        glyph: &'static str,
        label: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(32.))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(
                ui::mono(faces, glyph)
                    .w(px(16.))
                    .text_center()
                    .text_size(px(15.))
                    .text_color(theme::muted()),
            )
            .child(
                ui::text(faces, label)
                    .text_size(px(13.5))
                    .text_color(theme::text2()),
            )
    }

    /// How a session is doing, as its row shows it: running work, a
    /// question waiting on you, or nothing.
    fn session_state(&self, id: &str) -> Option<State> {
        let journal = if id == DEMO {
            &self.demo
        } else {
            &self.solver.journal
        };
        let place = thread::place_key(id);
        thread::tasks(journal)
            .iter()
            .filter(|t| {
                journal.iter().any(|e| {
                    e.kind == "undertook"
                        && e.data.get("id").and_then(Value::as_str) == Some(t.id.as_str())
                        && e.data.get("place").and_then(Value::as_str) == Some(place.as_str())
                })
            })
            .map(thread::Task::state)
            .find(|s| matches!(s, State::Running | State::Waiting))
    }

    fn session_row(
        &self,
        faces: &Faces,
        index: usize,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on = index == self.current;
        let state = self.session_state(&s.id);
        div()
            .id(SharedString::from(format!("session-{}", s.id)))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .h(px(32.))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .hover(|x| x.bg(theme::hover()))
            .child(ui::one_line(
                ui::text(faces, s.label())
                    .text_size(px(13.5))
                    .text_color(if on { theme::text() } else { theme::text2() })
                    .when(s.title.is_empty(), |d| d.text_color(theme::muted())),
            ))
            .children(state.map(|st| {
                ui::dot(match st {
                    State::Waiting => theme::purple(),
                    _ => theme::accent(),
                })
                .size(px(6.))
            }))
            .when(s.id == DEMO, |d| {
                d.child(
                    ui::mono(faces, "demo")
                        .text_size(px(10.5))
                        .text_color(theme::faint()),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }

    fn left(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        // The window's lights sit in this row; the controls follow them.
        let top = draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(52.))
            .pl(px(76.))
            .pr(px(10.))
            .child(Self::control("sidebar", faces, "▯", true))
            .child(Self::control("back", faces, "‹", true))
            .child(Self::control("forward", faces, "›", false));

        let menu = div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(8.))
            .pb(px(8.))
            .child(
                Self::menu_row("new", faces, "+", "New task")
                    .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
            )
            .child(Self::menu_row("search", faces, "⌕", "Search"));

        let mut list = div()
            .id("sessions")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(8.))
            .gap(px(1.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .pt(px(12.))
                    .pb(px(6.))
                    .child(
                        ui::text(faces, "This machine")
                            .text_size(px(12.5))
                            .text_color(theme::muted()),
                    )
                    .child(
                        ui::mono(faces, "⌄")
                            .text_size(px(11.))
                            .text_color(theme::faint()),
                    ),
            );
        for (i, s) in self.settings.sessions.iter().enumerate() {
            list = list.child(self.session_row(faces, i, s, cx));
        }
        let demo = Session {
            id: DEMO.to_owned(),
            title: "How the thread draws things".to_owned(),
            at: Utc::now(),
        };
        list = list
            .child(self.session_row(faces, self.settings.sessions.len(), &demo, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .pt(px(18.))
                    .pb(px(6.))
                    .child(
                        ui::text(faces, "Characters")
                            .text_size(px(12.5))
                            .text_color(theme::muted()),
                    )
                    .child(
                        ui::mono(faces, "›")
                            .text_size(px(11.))
                            .text_color(theme::faint()),
                    ),
            );

        let provider = self.current_provider();
        let link_color = match &self.solver.link {
            LinkState::Connected => theme::green(),
            LinkState::Connecting => theme::muted(),
            LinkState::Lost(_) => theme::coral(),
        };
        let bottom = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(px(52.))
            .px(px(16.))
            .border_t_1()
            .border_color(theme::border())
            .child(ui::icon("ghost", 18.).text_color(theme::text()))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        ui::text(faces, self.solver.name())
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(ui::one_line(
                        ui::mono(
                            faces,
                            format!(
                                "· {}",
                                provider
                                    .as_ref()
                                    .map_or_else(|| "no brain".to_owned(), Provider::label)
                            ),
                        )
                        .text_size(px(11.5))
                        .text_color(theme::muted()),
                    )),
            )
            .child(ui::dot(link_color).size(px(6.)));

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LEFT))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .child(top)
            .child(menu)
            .child(list)
            .child(bottom)
    }

    /// The bar across the top of the thread: the session's name, the clock.
    fn topbar(&self, faces: &Faces) -> impl IntoElement {
        let title = self.session().label();
        let right = self.solver.status.as_ref().map(|s| {
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
            .px(px(24.))
            .border_b_1()
            .border_color(theme::border())
            .child(ui::one_line(
                ui::text(faces, title)
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(13.5)),
            ))
            .children(right)
    }

    fn current_provider(&self) -> Option<Provider> {
        let brain = self.brain.as_deref()?;
        self.providers.iter().find(|p| p.name == brain).cloned()
    }

    /// The solver's environment and its work, down the right.
    fn right(&self, faces: &Faces) -> impl IntoElement {
        let sv = &self.solver;
        let s = sv.status.as_ref();
        let row = |label: &str, value: String, color| {
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(px(12.))
                .py(px(4.))
                .child(
                    ui::text(faces, label.to_owned())
                        .text_size(px(13.))
                        .text_color(theme::text2()),
                )
                .child(ui::one_line(
                    ui::mono(faces, value).text_color(color).text_right(),
                ))
        };
        let head = |title: &str, aside: Option<String>| {
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .pb(px(6.))
                .child(
                    ui::text(faces, title.to_owned())
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM),
                )
                .children(aside.map(|a| ui::mono(faces, a).text_size(px(11.5))))
        };
        let dash = || "—".to_owned();
        let link = match &sv.link {
            LinkState::Connected => ("connected".to_owned(), theme::green()),
            LinkState::Connecting => ("connecting".to_owned(), theme::muted()),
            LinkState::Lost(why) => (why.clone(), theme::coral()),
        };
        let host = self
            .settings
            .solver_url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .to_owned();
        let provider = self.current_provider();
        let environment = div()
            .flex()
            .flex_col()
            .child(head("Environment", None))
            .child(row("Daemon", host, theme::text()))
            .child(row("Link", link.0, link.1))
            .child(row(
                "Brain",
                provider
                    .as_ref()
                    .map_or_else(|| s.map_or_else(dash, |s| s.think.clone()), Provider::label),
                theme::text(),
            ))
            .child(row(
                "Machine",
                self.deployment
                    .ghost_dir()
                    .join("home")
                    .strip_prefix(&self.deployment.root)
                    .map_or_else(|_| dash(), |p| p.display().to_string()),
                theme::text(),
            ))
            .child(row(
                "Desk",
                if sv.desk_online {
                    "registered".to_owned()
                } else {
                    "not yet".to_owned()
                },
                theme::text(),
            ));

        let context = div()
            .flex()
            .flex_col()
            .child(head("Context", s.map(|s| format!("tempo {:.2}", s.tempo))))
            .child(row(
                "Journal",
                s.map_or_else(dash, |s| format!("{} events", s.journal)),
                theme::text(),
            ))
            .child(row(
                "Sessions",
                self.settings.sessions.len().to_string(),
                theme::text(),
            ))
            .child(row(
                "You",
                sv.you(self.who()).map_or_else(
                    || "not yet known".to_owned(),
                    |p| format!("operator · {} lines", p.heard),
                ),
                theme::text(),
            ));

        let journal = if self.on_demo() {
            &self.demo
        } else {
            &sv.journal
        };
        let tasks = thread::tasks(journal);
        let mut work = div()
            .flex()
            .flex_col()
            .child(head("Work", Some(tasks.len().to_string())));
        if tasks.is_empty() {
            work = work.child(
                ui::text(faces, "Nothing taken on.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        for t in tasks.iter().rev().take(8) {
            let st = t.state();
            let running = t
                .parts
                .iter()
                .filter(|p| p.state() == State::Running)
                .count();
            let mut meta = format!("{} steps", t.steps());
            if !t.parts.is_empty() {
                meta = format!("{meta} · {} parts", t.parts.len());
                if running > 0 {
                    meta = format!("{meta}, {running} running");
                }
            }
            work = work.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .py(px(5.))
                    .child(
                        ui::dot(match st {
                            State::Running => theme::accent(),
                            State::Waiting => theme::purple(),
                            State::Finished => theme::green(),
                            State::Blocked => theme::coral(),
                            State::Dropped => theme::muted(),
                        })
                        .size(px(6.))
                        .mt(px(7.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::one_line(
                                ui::text(faces, t.brief.clone()).text_size(px(13.)),
                            ))
                            .child(
                                ui::mono(faces, meta)
                                    .text_size(px(11.))
                                    .text_color(theme::faint()),
                            ),
                    ),
            );
        }

        div()
            .id("environment")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(RIGHT))
            .h_full()
            .overflow_y_scroll()
            .gap(px(20.))
            .px(px(20.))
            .py(px(18.))
            .border_l_1()
            .border_color(theme::border())
            .child(environment)
            .child(context)
            .child(work)
    }

    /// The menu of brains, over the brain chip.
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
                .when(self.menu != Menu::Closed, |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon(icon, 14.).text_color(theme::brand(icon)))
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .child(
                    ui::mono(faces, "⌄")
                        .text_size(px(11.))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, cx))),
        );
        if self.menu != Menu::Closed {
            chip = chip.child(
                div().absolute().top(px(-8.)).left(px(0.)).child(deferred(
                    anchored()
                        .anchor(Corner::BottomLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(self.brain_menu(faces, cx)),
                )),
            );
        }
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
                            .child(chip)
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = fonts::faces(cx).clone();
        let session = self.session();
        let typing = self.solver.typing.as_deref() == Some(session.id.as_str());
        let no_pending: &[Message] = &[];
        let view = thread::View {
            journal: if self.on_demo() {
                &self.demo
            } else {
                &self.solver.journal
            },
            pending: if self.on_demo() {
                no_pending
            } else {
                &self.solver.pending
            },
            session: &session.id,
            name: self.solver.name(),
            who: self.who().to_owned(),
            tz: self.solver.tz(),
            typing,
            expanded: &self.expanded,
        };
        let thread = thread::render(&view, &faces, cx);
        let middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(self.topbar(&faces))
            .child(thread)
            .child(self.composer(&faces, cx));
        div()
            .flex()
            .size_full()
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(14.))
            .text_color(theme::text())
            .child(self.left(&faces, cx))
            .child(middle)
            .child(self.right(&faces))
    }
}

/// The demo session: what a task looks like in the thread, a question, a
/// look at the machine, an answer, a piece of work with two parts, one
/// done and one still going. Nothing in it was lived; it is only drawn.
fn demo_journal() -> Vec<Entry> {
    let now = Utc::now();
    let ago = |mins: i64| now - chrono::Duration::minutes(mins);
    let place = thread::place_key(DEMO);
    let entry = |seq: i64, mins: i64, kind: &str, data: Value| Entry {
        seq,
        at: ago(mins),
        kind: kind.to_owned(),
        data,
    };
    vec![
        entry(
            1,
            18,
            "heard",
            json!({"channel": "desk", "place": {"id": DEMO, "kind": "direct"},
            "from": {"id": "you", "name": "you"}, "text": "My disk is nearly full. What is taking the space?"}),
        ),
        entry(
            2,
            17,
            "opened",
            json!({"url": "~/Library/Caches", "place": place,
            "text": "41G\t~/Library/Caches\n28G\t~/Library/Developer/Xcode/DerivedData\n9.1G\t~/Downloads\n3.4G\t~/Library/Caches/com.spotify.client"}),
        ),
        entry(
            3,
            17,
            "looked_up",
            json!({"query": "is it safe to delete Xcode DerivedData", "place": place,
            "found": "DerivedData holds build products and indexes. Xcode recreates it on the next build; deleting it is safe when Xcode is closed."}),
        ),
        entry(
            4,
            16,
            "said",
            json!({"channel": "desk", "place": DEMO, "text":
            "Three things take most of it:\n\n- **Xcode DerivedData**: 28 GB, rebuilt on demand, safe to clear\n- **Caches**: 41 GB in all, most of it the line above\n- **Downloads**: 9.1 GB, 540 files, never sorted\n\nTo clear DerivedData:\n\n```sh\nrm -rf ~/Library/Developer/Xcode/DerivedData\n```\n\nSay the word and I clear it, and sort `~/Downloads` by kind while I am at it."}),
        ),
        entry(
            5,
            12,
            "heard",
            json!({"channel": "desk", "place": {"id": DEMO, "kind": "direct"},
            "from": {"id": "you", "name": "you"}, "text": "Go ahead, both."}),
        ),
        entry(
            6,
            11,
            "undertook",
            json!({"id": "w-7", "brief": "Clear DerivedData and sort ~/Downloads by kind", "place": place}),
        ),
        entry(
            7,
            11,
            "undertook",
            json!({"id": "w-7/1", "brief": "Clear Xcode DerivedData"}),
        ),
        entry(
            8,
            11,
            "undertook",
            json!({"id": "w-7/2", "brief": "Sort ~/Downloads into folders by kind"}),
        ),
        entry(
            9,
            8,
            "worked",
            json!({"id": "w-7/1", "summary": "Removed 28 GB from DerivedData; Xcode was closed", "steps": 4}),
        ),
        entry(
            10,
            8,
            "concluded",
            json!({"id": "w-7/1", "outcome": "finished", "summary": "28 GB back"}),
        ),
        entry(
            11,
            6,
            "reported",
            json!({"id": "w-7", "text": "DerivedData cleared: 28 GB back. Downloads is half sorted."}),
        ),
        entry(
            12,
            3,
            "worked",
            json!({"id": "w-7/2", "summary": "312 of 540 files moved into Images, Documents, Archives", "steps": 9}),
        ),
        entry(
            13,
            2,
            "worked",
            json!({"id": "w-7", "summary": "Waiting on the sorting part", "steps": 3}),
        ),
    ]
}
