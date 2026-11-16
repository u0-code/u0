//! The desk: the whole window, and everything it knows.
//!
//! A narrow rail with the ghost on it; a thin bar with her name and her
//! clock; then three columns: the characters down the left, the one on
//! this machine first; the thread with whoever is chosen in the middle,
//! with the line you speak into; and, down the right, her environment and
//! her work. Choosing a character changes the whole environment to hers.

use std::{sync::mpsc, time::Duration};

use chrono::Utc;
use chrono_tz::Tz;
use gpui::{Context, Entity, FontWeight, MouseButton, SharedString, Window, div, prelude::*, px};
use serde_json::Value;

use crate::{
    fonts::{self, Faces},
    input::{Submitted, TextInput},
    link::{self, Discovered, Outgoing},
    model::{Entry, LinkState, Message, PersonView, PlaceView, Status, Update},
    settings::{CharacterSeed, DaemonSeed, Settings},
    theme, thread, ui,
};

/// The rail's width, the side columns' widths, the top bar's height.
pub const RAIL: f32 = 56.;
pub const LEFT: f32 = 248.;
pub const RIGHT: f32 = 288.;
pub const TOPBAR: f32 = 44.;

/// One character the desk follows.
pub struct Character {
    pub seed: CharacterSeed,
    rx: mpsc::Receiver<Update>,
    tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
    /// Not a character on a daemon: a stand-in to show the desk with, and
    /// to switch to, until real ones are followed.
    pub mock: bool,
    pub link: LinkState,
    pub card: Option<ghost_link::protocol::GhostCard>,
    pub status: Option<Status>,
    pub agenda: Value,
    pub places: Vec<PlaceView>,
    pub people: Vec<PersonView>,
    /// Her journal's tail, oldest first. The thread is read from it.
    pub journal: Vec<Entry>,
    /// What was said on the desk that her journal has not shown yet: your
    /// lines as you type them, hers as they arrive.
    pub pending: Vec<Message>,
    pub typing: bool,
    pub desk_online: bool,
    /// The last entry you had on screen, by seq: what came after counts as new.
    pub seen: i64,
}

impl Character {
    fn start(seed: CharacterSeed, who: &str, token: Option<String>) -> Self {
        let (rx, tx) = link::start(link::Config {
            url: seed.url.clone(),
            token,
            ghost: seed.ghost.clone(),
            who: who.to_owned(),
        });
        Self::blank(seed, rx, tx, false)
    }

    /// A stand-in with a few lines already said, so the desk has someone
    /// to switch to while the look is being settled.
    fn mock(name: &str, host: &str, lines: &[(bool, &str)]) -> Self {
        let (_, rx) = mpsc::channel();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let seed = CharacterSeed {
            url: format!("ws://{host}"),
            ghost: Some(name.to_owned()),
            local: false,
        };
        let mut c = Self::blank(seed, rx, tx, true);
        let now = Utc::now();
        c.pending = lines
            .iter()
            .enumerate()
            .map(|(i, (hers, text))| Message {
                hers: *hers,
                text: (*text).to_owned(),
                at: now
                    - chrono::Duration::minutes(i64::try_from(lines.len() - i).unwrap_or(0) * 7),
            })
            .collect();
        c
    }

    fn blank(
        seed: CharacterSeed,
        rx: mpsc::Receiver<Update>,
        tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
        mock: bool,
    ) -> Self {
        Self {
            seed,
            rx,
            tx,
            mock,
            link: if mock {
                LinkState::Lost("mock".into())
            } else {
                LinkState::Connecting
            },
            card: None,
            status: None,
            agenda: Value::Null,
            places: Vec::new(),
            people: Vec::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            typing: false,
            desk_online: false,
            seen: 0,
        }
    }

    pub fn name(&self) -> String {
        self.status
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| self.card.as_ref().map(|c| c.name.clone()))
            .or_else(|| self.seed.ghost.clone())
            .unwrap_or_else(|| "her".to_owned())
    }

    pub fn tz(&self) -> Tz {
        self.status.as_ref().map_or(chrono_tz::UTC, |s| s.tz)
    }

    pub fn connected(&self) -> bool {
        self.link == LinkState::Connected
    }

    pub fn awake(&self) -> bool {
        self.status.as_ref().is_some_and(Status::awake)
    }

    /// The host part of her daemon's address.
    pub fn host(&self) -> String {
        self.seed
            .url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .to_owned()
    }

    /// Entries that arrived after you last looked at her thread.
    pub fn unseen(&self) -> usize {
        self.journal.iter().filter(|e| e.seq > self.seen).count()
    }

    /// Among them, what is hers to you: a reply, a report, a conclusion.
    pub fn unseen_for_you(&self) -> usize {
        self.journal
            .iter()
            .filter(|e| e.seq > self.seen)
            .filter(|e| {
                matches!(e.kind.as_str(), "reported" | "concluded")
                    || thread::desk_message(e).is_some_and(|m| m.hers)
            })
            .count()
    }

    pub fn mark_seen(&mut self) {
        self.seen = self.journal.last().map_or(self.seen, |e| e.seq);
    }

    /// The last thing between you, as a line for her row: (time, text).
    pub fn preview(&self) -> Option<(String, String)> {
        let tz = self.tz();
        if let Some(m) = self.pending.last() {
            return Some((thread::clock(m.at, tz), m.text.clone()));
        }
        self.journal
            .iter()
            .rev()
            .find_map(|e| thread::desk_message(e).map(|m| (thread::clock(e.at, tz), m.text)))
    }

    /// The pieces of work she took on, newest first, each with how far it got.
    pub fn work(&self) -> Vec<WorkRow> {
        let mut rows: Vec<WorkRow> = Vec::new();
        for e in &self.journal {
            let id = e.data.get("id").and_then(Value::as_str).unwrap_or_default();
            match e.kind.as_str() {
                "undertook" => rows.push(WorkRow {
                    id: id.to_owned(),
                    brief: e
                        .data
                        .get("brief")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    steps: 0,
                    reports: 0,
                    outcome: None,
                }),
                "worked" => {
                    if let Some(r) = rows.iter_mut().rev().find(|r| r.id == id) {
                        r.steps += e
                            .data
                            .get("steps")
                            .and_then(Value::as_i64)
                            .unwrap_or_default();
                    }
                }
                "reported" => {
                    if let Some(r) = rows.iter_mut().rev().find(|r| r.id == id) {
                        r.reports += 1;
                    }
                }
                "concluded" => {
                    if let Some(r) = rows.iter_mut().rev().find(|r| r.id == id) {
                        r.outcome = Some(
                            e.data
                                .get("outcome")
                                .and_then(Value::as_str)
                                .unwrap_or("concluded")
                                .to_owned(),
                        );
                    }
                }
                _ => {}
            }
        }
        rows.reverse();
        rows
    }

    /// How she stands with you: the person record for you on the desk.
    pub fn you(&self, who: &str) -> Option<&PersonView> {
        let key = link::place_key(who);
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
            Update::Agenda(agenda) => self.agenda = agenda,
            Update::Places(places) => self.places = places,
            Update::People(people) => self.people = people,
            Update::Journal(entries) => {
                self.journal = entries;
                if self.seen == 0 {
                    // Nothing is new on first sight; only what arrives after.
                    self.mark_seen();
                }
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
            Update::HerSay(text) => {
                self.typing = false;
                self.pending.push(Message {
                    hers: true,
                    text,
                    at: Utc::now(),
                });
            }
            Update::HerTyping => self.typing = true,
            Update::DeskOnline(on) => self.desk_online = on,
        }
    }
}

/// One piece of work, as the right column lists it.
pub struct WorkRow {
    pub id: String,
    pub brief: String,
    pub steps: i64,
    pub reports: usize,
    pub outcome: Option<String>,
}

pub struct Desk {
    pub settings: Settings,
    pub characters: Vec<Character>,
    pub current: usize,
    pub input: Entity<TextInput>,
    pub url_input: Entity<TextInput>,
    discoveries: mpsc::Receiver<Discovered>,
    discover_tx: mpsc::Sender<Discovered>,
    pub discovering: Option<String>,
    pub discovery_error: Option<String>,
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

impl Desk {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let input = cx.new(|cx| TextInput::new(cx, "Work with her"));
        let url_input = cx.new(|cx| TextInput::new(cx, "host:7437"));
        cx.subscribe(&input, |this, _, Submitted(text), cx| {
            this.say(text.clone(), cx);
        })
        .detach();
        cx.subscribe(&url_input, |this, _, Submitted(text), cx| {
            this.add_daemon(text, cx);
        })
        .detach();
        let mut characters: Vec<Character> = settings
            .characters
            .iter()
            .cloned()
            .map(|seed| Character::start(seed, &settings.who, token()))
            .collect();
        // Stand-ins to switch to while the look is settled. They are not
        // saved and never reach a daemon.
        characters.push(Character::mock(
            "Weiwei",
            "192.168.1.20:7437",
            &[
                (false, "How is the thread coming?"),
                (true, "Slowly, the way notes should. The second section wants the Yunnan thread I kept yesterday."),
                (false, "Keep it under eight posts."),
                (true, "Eight is right. The thread was getting fond of itself."),
            ],
        ));
        characters.push(Character::mock(
            "大肥鱼",
            "192.168.1.20:7437",
            &[
                (false, "今天群里怎么样？"),
                (true, "吵了一下午猫粮，我没插嘴。"),
            ],
        ));
        // The desk opens on the one that works on this machine.
        let current = characters.iter().position(|c| c.seed.local).unwrap_or(0);
        let (discover_tx, discoveries) = mpsc::channel();
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
            characters,
            current,
            input,
            url_input,
            discoveries,
            discover_tx,
            discovering: None,
            discovery_error: None,
        };
        desk.set_placeholder(cx);
        desk
    }

    /// Take in everything the links have learned since last time.
    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for (i, character) in self.characters.iter_mut().enumerate() {
            while let Ok(update) = character.rx.try_recv() {
                character.take(update);
                changed = true;
            }
            if i == self.current {
                // What you are looking at is seen as it arrives.
                character.mark_seen();
            }
        }
        while let Ok(found) = self.discoveries.try_recv() {
            changed = true;
            self.discovering = None;
            if let Some(error) = found.error {
                self.discovery_error = Some(error);
                continue;
            }
            self.discovery_error = None;
            for ghost in found.ghosts {
                let seed = CharacterSeed {
                    url: found.url.clone(),
                    ghost,
                    local: false,
                };
                if !self.settings.characters.contains(&seed) {
                    self.settings.characters.push(seed.clone());
                    let real = self.characters.iter().filter(|c| !c.mock).count();
                    self.characters
                        .insert(real, Character::start(seed, &self.settings.who, token()));
                }
            }
            self.settings.save();
        }
        if changed {
            cx.notify();
        }
    }

    pub fn current(&self) -> Option<&Character> {
        self.characters.get(self.current)
    }

    pub fn who(&self) -> &str {
        &self.settings.who
    }

    fn save(&mut self) {
        self.settings.characters = self
            .characters
            .iter()
            .filter(|c| !c.mock)
            .map(|c| c.seed.clone())
            .collect();
        let urls: Vec<String> = self
            .settings
            .characters
            .iter()
            .map(|c| c.url.clone())
            .collect();
        self.settings.daemons.retain(|d| urls.contains(&d.url));
        self.settings.save();
    }

    fn set_placeholder(&mut self, cx: &mut Context<Self>) {
        let placeholder = self.current().map_or_else(
            || "Add a daemon first".to_owned(),
            |c| format!("Work with {}", c.name()),
        );
        self.input
            .update(cx, |input, _| input.set_placeholder(placeholder));
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.characters.len() {
            self.current = index;
            self.characters[index].mark_seen();
            self.set_placeholder(cx);
            cx.notify();
        }
    }

    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(c) = self.characters.get_mut(self.current) {
            c.pending.push(Message {
                hers: false,
                text: text.clone(),
                at: Utc::now(),
            });
            let _ = c.tx.send(Outgoing(text));
            cx.notify();
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    pub fn add_daemon(&mut self, url: &str, cx: &mut Context<Self>) {
        let url = url.trim().to_owned();
        if url.is_empty() {
            return;
        }
        let url = if url.contains("://") {
            url
        } else {
            format!("ws://{url}")
        };
        let seed = DaemonSeed { url: url.clone() };
        if !self.settings.daemons.contains(&seed) {
            self.settings.daemons.push(seed);
            self.settings.save();
        }
        self.discover(url, cx);
    }

    pub fn discover(&mut self, url: String, cx: &mut Context<Self>) {
        self.discovering = Some(url.clone());
        self.discovery_error = None;
        link::discover(url, token(), self.discover_tx.clone());
        cx.notify();
    }

    pub fn toggle_local(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(c) = self.characters.get(index) else {
            return;
        };
        let local = !c.seed.local;
        for (i, other) in self.characters.iter_mut().enumerate() {
            other.seed.local = local && i == index;
        }
        self.save();
        self.set_placeholder(cx);
        cx.notify();
    }

    pub fn forget(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.characters.len() {
            return;
        }
        self.characters.remove(index);
        self.save();
        if self.current >= self.characters.len() {
            self.current = self.characters.len().saturating_sub(1);
        }
        self.set_placeholder(cx);
        cx.notify();
    }

    fn rail(faces: &Faces) -> impl IntoElement {
        draggable()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .w(px(RAIL))
            .h_full()
            .pt(px(44.))
            .pb(px(14.))
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .child(ui::icon("ghost", 22.).text_color(theme::text()))
            .child(div().flex_1())
            .child(
                ui::mono(faces, "desk")
                    .text_size(px(9.5))
                    .text_color(theme::faint()),
            )
    }

    /// The bar across the top: her name, and her clock.
    fn topbar(&self, faces: &Faces) -> impl IntoElement {
        let c = self.current();
        let link_color = match c.map(|c| &c.link) {
            Some(LinkState::Connected) => theme::green(),
            Some(LinkState::Connecting) => theme::muted(),
            Some(LinkState::Lost(_)) => theme::coral(),
            None => theme::ghost_line(),
        };
        let mut left = div().flex().items_center().gap(px(10.)).min_w(px(0.));
        if let Some(c) = c {
            left = left
                .child(ui::dot(link_color))
                .child(
                    ui::text(faces, c.name())
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(13.5)),
                )
                .when(c.seed.local, |d| {
                    d.child(
                        ui::mono(faces, "this machine")
                            .text_size(px(11.))
                            .text_color(theme::accent()),
                    )
                })
                .when(c.mock, |d| {
                    d.child(
                        ui::mono(faces, "mock")
                            .text_size(px(11.))
                            .text_color(theme::faint()),
                    )
                });
        }
        let right = c.and_then(|c| c.status.as_ref()).map(|s| {
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
            .px(px(20.))
            .border_b_1()
            .border_color(theme::border())
            .child(left)
            .children(right)
    }

    fn character_row(
        &self,
        faces: &Faces,
        index: usize,
        c: &Character,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on = index == self.current;
        let unseen = c.unseen();
        let for_you = c.unseen_for_you();
        let (when, line) = c.preview().unwrap_or_default();
        let state = if c.awake() {
            ui::dot(theme::accent()).size(px(5.))
        } else if c.connected() || c.mock {
            ui::dot(theme::ghost_line()).size(px(5.))
        } else if c.link == LinkState::Connecting {
            ui::hollow().size(px(5.))
        } else {
            ui::dot(theme::coral()).size(px(5.))
        };
        div()
            .id(SharedString::from(format!("character-{index}")))
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w(px(0.))
                            .child(state)
                            .child(ui::one_line(
                                ui::text(faces, c.name())
                                    .text_size(px(13.5))
                                    .font_weight(if on {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if on { theme::text() } else { theme::text2() }),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_none()
                            .when(unseen > 0 && !on, |d| {
                                d.child(
                                    ui::dot(if for_you > 0 {
                                        theme::accent()
                                    } else {
                                        theme::muted()
                                    })
                                    .size(px(5.)),
                                )
                            })
                            .child(ui::mono(faces, when).text_size(px(11.))),
                    ),
            )
            .child(ui::one_line(
                ui::text(faces, line)
                    .text_size(px(12.))
                    .text_color(theme::muted()),
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }

    fn left(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let local = self.characters.iter().position(|c| c.seed.local);
        let mut list = div()
            .id("characters")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(8.))
            .py(px(12.))
            .gap(px(2.));
        list = list.child(ui::eyebrow(faces, "this machine").px(px(12.)).py(px(6.)));
        if let Some(i) = local {
            list = list.child(self.character_row(faces, i, &self.characters[i], cx));
        }
        list = list.child(
            ui::eyebrow(faces, "characters")
                .px(px(12.))
                .pt(px(16.))
                .pb(px(6.)),
        );
        for i in (0..self.characters.len()).filter(|i| Some(*i) != local) {
            list = list.child(self.character_row(faces, i, &self.characters[i], cx));
        }

        let footer = div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .min_h(px(32.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(theme::border2())
                    .child(ui::mono(faces, "+").text_color(theme::faint()))
                    .child(self.url_input.clone()),
            )
            .children(self.discovering.as_ref().map(|u| {
                ui::mono(faces, format!("asking {u}…"))
                    .text_size(px(11.))
                    .text_color(theme::faint())
            }))
            .children(self.discovery_error.as_ref().map(|e| {
                ui::text(faces, e.clone())
                    .text_size(px(11.5))
                    .text_color(theme::coral())
                    .line_height(px(16.))
            }));

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LEFT))
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .child(list)
            .child(footer)
    }

    /// Her environment and her work, down the right.
    fn right(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.current;
        let c = self.current();
        let s = c.and_then(|c| c.status.as_ref());
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

        let mut environment = div().flex().flex_col().child(head("Environment", None));
        if let Some(c) = c {
            let link = match &c.link {
                LinkState::Connected => ("connected".to_owned(), theme::green()),
                LinkState::Connecting => ("connecting".to_owned(), theme::muted()),
                LinkState::Lost(why) => (why.clone(), theme::coral()),
            };
            environment = environment
                .child(row("Daemon", c.host(), theme::text()))
                .child(row("Link", link.0, link.1))
                .child(row(
                    "Character",
                    c.seed.ghost.clone().unwrap_or_else(|| c.name()),
                    theme::text(),
                ))
                .child(row(
                    "Brain",
                    s.map_or_else(dash, |s| s.think.clone()),
                    theme::text(),
                ))
                .child(row(
                    "Now",
                    s.map_or_else(dash, |s| {
                        if s.awake() {
                            s.doing
                                .as_ref()
                                .map_or_else(|| format!("awake · {}", s.reach), |d| d.what.clone())
                        } else {
                            s.until.map_or_else(
                                || "asleep".to_owned(),
                                |u| format!("asleep until {}", thread::clock(u, s.tz)),
                            )
                        }
                    }),
                    if c.awake() {
                        theme::accent()
                    } else {
                        theme::muted()
                    },
                ))
                .child(row(
                    "Places",
                    s.map_or_else(dash, |s| {
                        let names: Vec<&str> =
                            s.channels.iter().map(|ch| ch.name.as_str()).collect();
                        if names.is_empty() {
                            dash()
                        } else {
                            names.join(" · ")
                        }
                    }),
                    theme::text(),
                ));
        }

        let unread: i64 = c.map_or(0, |c| c.places.iter().map(|p| p.unread).sum());
        let context = div()
            .flex()
            .flex_col()
            .child(head("Context", s.map(|s| format!("tempo {:.1}", s.tempo))))
            .child(row(
                "Journal",
                s.map_or_else(dash, |s| format!("{} events", s.journal)),
                theme::text(),
            ))
            .child(row("Unread elsewhere", unread.to_string(), theme::text()))
            .child(row(
                "You",
                c.and_then(|c| c.you(self.who())).map_or_else(
                    || "not yet known".to_owned(),
                    |p| format!("{} exchanges", p.heard),
                ),
                theme::text(),
            ));

        let work = c.map(Character::work).unwrap_or_default();
        let mut changes = div()
            .flex()
            .flex_col()
            .child(head("Work", Some(work.len().to_string())));
        if work.is_empty() {
            changes = changes.child(
                ui::text(faces, "Nothing taken on.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        for w in work.iter().take(6) {
            let (color, state) = match &w.outcome {
                Some(o) => (theme::green(), o.clone()),
                None => (theme::accent(), "at it".to_owned()),
            };
            changes = changes.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .py(px(5.))
                    .child(ui::dot(color).size(px(6.)).mt(px(7.)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::one_line(
                                ui::text(faces, w.brief.clone()).text_size(px(13.)),
                            ))
                            .child(
                                ui::mono(
                                    faces,
                                    format!(
                                        "[{}] · {} steps · {} reports · {state}",
                                        w.id, w.steps, w.reports
                                    ),
                                )
                                .text_size(px(11.))
                                .text_color(theme::faint()),
                            ),
                    ),
            );
        }

        let mut aside = div()
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
            .child(changes)
            .child(div().flex_1());
        if let Some(c) = c.filter(|c| !c.mock) {
            aside = aside.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::button(
                            "local",
                            faces,
                            if c.seed.local {
                                "not local"
                            } else {
                                "mark local"
                            },
                        )
                        .min_h(px(26.))
                        .px(px(9.))
                        .text_size(px(11.5))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_local(index, cx))),
                    )
                    .child(
                        ui::button("forget", faces, "forget")
                            .min_h(px(26.))
                            .px(px(9.))
                            .text_size(px(11.5))
                            .on_click(cx.listener(move |this, _, _, cx| this.forget(index, cx))),
                    ),
            );
        }
        aside
    }

    /// The line you speak into, the way a coding agent's is: a box with
    /// the text, and a row of small controls under it.
    fn composer(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.current().and_then(|c| c.status.as_ref());
        let brain = s.map_or_else(|| "no brain".to_owned(), |s| s.think.clone());
        let empty = self.input.read(cx).is_empty();
        div()
            .flex()
            .flex_none()
            .justify_center()
            .px(px(32.))
            .pb(px(20.))
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
                    .rounded(px(14.))
                    .child(div().px(px(4.)).child(self.input.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .id("attach")
                                            .size(px(28.))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::hover()))
                                            .child(
                                                ui::mono(faces, "+")
                                                    .text_size(px(16.))
                                                    .text_color(theme::muted()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("direct")
                                            .flex()
                                            .items_center()
                                            .h(px(26.))
                                            .px(px(10.))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::hover()))
                                            .child(
                                                ui::text(faces, "Direct her")
                                                    .text_size(px(12.5))
                                                    .text_color(theme::muted()),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("brain")
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .h(px(26.))
                                            .px(px(10.))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::hover()))
                                            .child(ui::dot(theme::accent()).size(px(5.)))
                                            .child(
                                                ui::text(faces, brain)
                                                    .text_size(px(12.5))
                                                    .text_color(theme::text2()),
                                            )
                                            .child(
                                                ui::mono(faces, "⌄")
                                                    .text_size(px(12.))
                                                    .text_color(theme::muted()),
                                            ),
                                    )
                                    .child(
                                        div()
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
                                                ui::mono(faces, "↑").text_size(px(15.)).text_color(
                                                    if empty {
                                                        theme::muted()
                                                    } else {
                                                        theme::bg()
                                                    },
                                                ),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                                    ),
                            ),
                    ),
            )
    }
}

impl Render for Desk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = fonts::faces(cx).clone();
        let middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(thread::render(self, &faces))
            .child(self.composer(&faces, cx));
        div()
            .flex()
            .size_full()
            .bg(theme::bg())
            .font_family(faces.ui.clone())
            .text_size(px(14.))
            .text_color(theme::text())
            .child(Self::rail(&faces))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .child(self.topbar(&faces))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h(px(0.))
                            .child(self.left(&faces, cx))
                            .child(middle)
                            .child(self.right(&faces, cx)),
                    ),
            )
    }
}
