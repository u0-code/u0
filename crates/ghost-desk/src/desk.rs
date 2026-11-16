//! The desk: the whole window, and everything it knows.
//!
//! A narrow rail with the ghost on it; a bar across the top saying which
//! daemon and which character this is and how she is; then three columns:
//! the characters down the left, the one on this machine first; the thread
//! with whoever is chosen in the middle, with the line you speak into; and
//! what she knows of you down the right. Choosing a character changes the
//! whole environment to hers: her daemon, her clock, her places, her
//! journal.

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
pub const LEFT: f32 = 272.;
pub const RIGHT: f32 = 300.;
pub const TOPBAR: f32 = 48.;

/// One character the desk follows.
pub struct Character {
    pub seed: CharacterSeed,
    rx: mpsc::Receiver<Update>,
    tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
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
        Self {
            seed,
            rx,
            tx,
            link: LinkState::Connecting,
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

    /// The last thing between you, or failing that the last thing she
    /// lived, as a line for her row: (time, text).
    pub fn preview(&self) -> Option<(String, String)> {
        let tz = self.tz();
        if let Some(m) = self.pending.last() {
            return Some((thread::clock(m.at, tz), self.preview_line(m)));
        }
        if let Some((e, m)) = self
            .journal
            .iter()
            .rev()
            .find_map(|e| thread::desk_message(e).map(|m| (e, m)))
        {
            return Some((thread::clock(e.at, tz), self.preview_line(&m)));
        }
        self.journal.last().map(|e| {
            (
                thread::clock(e.at, tz),
                format!("{} · {}", e.kind.replace('_', " "), e.describe(tz)),
            )
        })
    }

    fn preview_line(&self, m: &Message) -> String {
        if m.hers {
            format!("{}: {}", self.name(), m.text)
        } else {
            m.text.clone()
        }
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

/// The strip at the top of a column: dragging it moves the window, and a
/// double click zooms it, as a title bar does.
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
        let input = cx.new(|cx| TextInput::new(cx, "Say something"));
        let url_input = cx.new(|cx| TextInput::new(cx, "another daemon: host:7437"));
        cx.subscribe(&input, |this, _, Submitted(text), cx| {
            this.say(text.clone(), cx);
        })
        .detach();
        cx.subscribe(&url_input, |this, _, Submitted(text), cx| {
            this.add_daemon(text, cx);
        })
        .detach();
        let characters: Vec<Character> = settings
            .characters
            .iter()
            .cloned()
            .map(|seed| Character::start(seed, &settings.who, token()))
            .collect();
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
                    self.characters
                        .push(Character::start(seed, &self.settings.who, token()));
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

    fn set_placeholder(&mut self, cx: &mut Context<Self>) {
        let placeholder = match self.current() {
            Some(c) if c.seed.local => "What needs doing on this machine?",
            Some(_) => "Say something, or begin with the directive prefix to direct her",
            None => "Add a daemon on the left first",
        };
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
        self.settings.characters = self.characters.iter().map(|c| c.seed.clone()).collect();
        self.settings.save();
        self.set_placeholder(cx);
        cx.notify();
    }

    pub fn forget(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.characters.len() {
            return;
        }
        self.characters.remove(index);
        self.settings.characters = self.characters.iter().map(|c| c.seed.clone()).collect();
        let urls: Vec<String> = self.characters.iter().map(|c| c.seed.url.clone()).collect();
        self.settings.daemons.retain(|d| urls.contains(&d.url));
        self.settings.save();
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

    /// The bar across the top: which daemon, which character, how she is.
    fn topbar(&self, faces: &Faces) -> impl IntoElement {
        let c = self.current();
        let (link_color, host) = match c {
            Some(c) => (
                match &c.link {
                    LinkState::Connected => theme::green(),
                    LinkState::Connecting => theme::muted(),
                    LinkState::Lost(_) => theme::coral(),
                },
                c.host(),
            ),
            None => (theme::ghost_line(), "no daemon".to_owned()),
        };
        let slash = || ui::mono(faces, "/").text_color(theme::ghost_line());
        let mut left = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .min_w(px(0.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::dot(link_color))
                    .child(ui::mono(faces, format!("ghostd · {host}"))),
            );
        if let Some(c) = c {
            left = left
                .child(slash())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::initial(faces, &c.name(), 20.))
                        .child(
                            ui::mono(faces, c.name())
                                .text_color(theme::text())
                                .font_weight(FontWeight::MEDIUM),
                        )
                        .when(c.seed.local, |d| {
                            d.child(
                                ui::mono(faces, "this machine")
                                    .text_size(px(11.))
                                    .text_color(theme::accent()),
                            )
                        }),
                )
                .child(slash())
                .child(ui::mono(faces, format!("desk · {}", self.who())));
            if let LinkState::Lost(why) = &c.link {
                left = left
                    .child(slash())
                    .child(ui::mono(faces, why.clone()).text_color(theme::coral()));
            }
        }

        let mut right = div().flex().items_center().gap(px(18.)).flex_none();
        if let Some(s) = c.and_then(|c| c.status.as_ref()) {
            let state = if s.awake() {
                match &s.doing {
                    Some(d) => format!(
                        "awake · {} · reach {} · tempo {:.1}",
                        d.what, s.reach, s.tempo
                    ),
                    None => format!("awake · reach {} · tempo {:.1}", s.reach, s.tempo),
                }
            } else {
                format!(
                    "asleep until {}",
                    s.until.map(|u| thread::clock(u, s.tz)).unwrap_or_default()
                )
            };
            right = right
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::dot(if s.awake() {
                            theme::accent()
                        } else {
                            theme::ghost_line()
                        }))
                        .child(ui::mono(faces, state)),
                )
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .child(
                            ui::mono(faces, s.hhmm())
                                .text_color(theme::text())
                                .font_weight(FontWeight::MEDIUM),
                        )
                        .child(ui::mono(faces, s.city.clone())),
                );
        }

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
            .child(right)
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
        let (when, line) = c.preview().unwrap_or_else(|| {
            (
                String::new(),
                match &c.link {
                    LinkState::Connected => "nothing said yet".to_owned(),
                    LinkState::Connecting => "connecting".to_owned(),
                    LinkState::Lost(_) => "her daemon is away".to_owned(),
                },
            )
        });
        let state = if c.awake() {
            ui::dot(theme::accent()).size(px(5.))
        } else if c.connected() {
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
            .py(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.))
                            .min_w(px(0.))
                            .child(
                                ui::mono(faces, c.host())
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .flex_none(),
                            )
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
                            .child(state)
                            .child(ui::mono(faces, when).text_size(px(11.))),
                    ),
            )
            .child(ui::one_line(
                ui::text(faces, line)
                    .text_size(px(12.5))
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
            .px(px(12.))
            .py(px(16.))
            .gap(px(4.));
        list = list.child(ui::eyebrow(faces, "this machine").px(px(12.)).pb(px(6.)));
        match local {
            Some(i) => list = list.child(self.character_row(faces, i, &self.characters[i], cx)),
            None => {
                list = list.child(
                    ui::text(faces, "No one yet. Choose a character and mark her local at the top right: she then works for you here, on this machine.")
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .line_height(px(17.))
                        .px(px(12.))
                        .py(px(4.)),
                );
            }
        }
        let others: Vec<usize> = (0..self.characters.len())
            .filter(|i| Some(*i) != local)
            .collect();
        list = list.child(
            ui::eyebrow(faces, "characters · everywhere else")
                .px(px(12.))
                .pt(px(18.))
                .pb(px(6.)),
        );
        if others.is_empty() {
            list = list.child(
                ui::text(faces, "Nobody followed yet. A daemon's address below is enough; the desk asks what it runs.")
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .line_height(px(17.))
                    .px(px(12.))
                    .py(px(4.)),
            );
        }
        for i in others {
            list = list.child(self.character_row(faces, i, &self.characters[i], cx));
        }

        let footer = div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(8.))
            .px(px(16.))
            .py(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .min_h(px(34.))
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
            }))
            .child(
                ui::text(
                    faces,
                    "You read along in her other places from here. You never speak for her there.",
                )
                .text_size(px(12.))
                .text_color(theme::faint())
                .line_height(px(17.)),
            );

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

    /// What she knows of you, and this place.
    fn right(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let who = self.who().to_owned();
        let index = self.current;
        let c = self.current();
        let you = c.and_then(|c| c.you(&who));
        let tz = c.map_or(chrono_tz::UTC, Character::tz);
        let mut aside = div()
            .id("about-you")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(RIGHT))
            .h_full()
            .overflow_y_scroll()
            .gap(px(18.))
            .px(px(20.))
            .py(px(20.))
            .border_l_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(ui::eyebrow(faces, "what she knows of you"))
                    .child(
                        ui::serif(
                            faces,
                            format!(
                                "{}, on the desk",
                                you.map_or(who.as_str(), |p| p.name.as_str())
                            ),
                        )
                        .text_size(px(22.))
                        .line_height(px(28.)),
                    )
                    .child(
                        ui::mono(
                            faces,
                            match you {
                                Some(p) => format!(
                                    "{}known since {} · {} exchanges",
                                    if p.standing.is_empty() {
                                        String::new()
                                    } else {
                                        format!("{} · ", p.standing)
                                    },
                                    p.first_at
                                        .map(|t| t.with_timezone(&tz).format("%-d %B").to_string())
                                        .unwrap_or_default(),
                                    p.heard
                                ),
                                None => "not yet someone she has heard from".to_owned(),
                            },
                        )
                        .text_size(px(11.5)),
                    ),
            );
        if let Some(p) = you {
            let mut notes = div().flex().flex_col().gap(px(8.));
            if p.notes.is_empty() {
                notes = notes.child(
                    ui::text(faces, "No notes about you yet.")
                        .text_size(px(13.))
                        .text_color(theme::muted()),
                );
            }
            for n in p.notes.iter().take(8) {
                notes = notes.child(
                    ui::text(faces, n.clone())
                        .text_size(px(13.))
                        .text_color(theme::text2())
                        .line_height(px(19.)),
                );
            }
            aside = aside.child(notes);
        }
        aside = aside.child(
            ui::text(faces, "Derived from her journal, so it can be rebuilt and never quietly edited. One memory across every platform, with a separate record for each.")
                .text_size(px(12.))
                .text_color(theme::faint())
                .line_height(px(18.)),
        );

        let kv = |k: &str, v: String| {
            div()
                .flex()
                .gap(px(12.))
                .child(
                    ui::mono(faces, k.to_owned())
                        .w(px(64.))
                        .flex_none()
                        .text_color(theme::faint()),
                )
                .child(ui::one_line(ui::mono(faces, v).text_color(theme::text2())))
        };
        let mut place = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .mt(px(8.))
            .child(ui::eyebrow(faces, "this place"))
            .child(kv("channel", link::CHANNEL.to_owned()))
            .child(kv("can", "send · typing · read".to_owned()));
        if let Some(c) = c {
            let unread: i64 = c.places.iter().map(|p| p.unread).sum();
            place = place
                .child(kv(
                    "tempo",
                    c.status.as_ref().map_or_else(
                        || "—".to_owned(),
                        |s| format!("{:.1} · as a friend would", s.tempo),
                    ),
                ))
                .child(kv("elsewhere", format!("{unread} unread")))
                .child(kv(
                    "places",
                    c.status.as_ref().map_or_else(
                        || "—".to_owned(),
                        |s| {
                            let names: Vec<&str> =
                                s.channels.iter().map(|ch| ch.name.as_str()).collect();
                            if names.is_empty() {
                                "none on".to_owned()
                            } else {
                                names.join(" · ")
                            }
                        },
                    ),
                ))
                .child(kv(
                    "journal",
                    c.status
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |s| format!("{} events", s.journal)),
                ))
                .child(kv(
                    "brain",
                    c.status
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |s| s.think.clone()),
                ));
            place = place.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .pt(px(8.))
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
        aside.child(div().flex_1()).child(place)
    }

    /// The line you speak into, with what it means underneath.
    fn composer(&self, faces: &Faces) -> impl IntoElement {
        let c = self.current();
        let note = match c {
            Some(c) if c.seed.local => {
                "She takes it on, works in the background, and reports here as she goes. Every step is in her journal."
            }
            Some(_) => {
                "A directive is carried out, in her voice, and written down as Directed. Everything else is a message she may answer, or not."
            }
            None => "",
        };
        let hint = format!("as {} · ↵", self.who());
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .px(px(32.))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .min_h(px(50.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::border2())
                    .rounded(px(theme::RADIUS))
                    .child(
                        div()
                            .font_family(faces.mono.clone())
                            .text_size(px(16.))
                            .text_color(theme::accent())
                            .child("›"),
                    )
                    .child(self.input.clone())
                    .child(
                        ui::mono(faces, hint)
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .flex_none(),
                    ),
            )
            .child(
                ui::mono(faces, note.to_owned())
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .line_height(px(16.))
                    .px(px(4.)),
            )
    }
}

impl Render for Desk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = fonts::faces(cx).clone();
        let mut middle = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(thread::render(self, &faces));
        if let Some(standing) = thread::standing(self, &faces) {
            middle = middle.child(standing);
        }
        middle = middle.child(self.composer(&faces));
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
