//! The thread of one session, in one stream by time: what passed between
//! you and the solver there, what it opened or looked up while answering,
//! and a card for each piece of work it took on, with its parts under it.
//! All of it is read from the solver's journal, so what shows here is what
//! is on record; what was said a moment ago and not yet written follows at
//! the end.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use gpui::{AnyElement, Context, Div, Hsla, SharedString, Stateful, div, prelude::*, px, relative};
use serde_json::Value;

use crate::{
    desk::Desk,
    fonts::Faces,
    link, markdown,
    model::{Entry, Message},
    solver::Abilities,
    theme, ui,
};

/// How wide the thread and the composer may grow.
pub const COLUMN: f32 = 760.;

/// A time of day in the solver's clock.
pub fn clock(at: DateTime<Utc>, tz: Tz) -> String {
    at.with_timezone(&tz).format("%H:%M").to_string()
}

/// A span of time in a word or two.
pub fn span(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

fn s<'a>(d: &'a Value, key: &str) -> &'a str {
    d.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The place key a session has in the solver's memory.
pub fn place_key(session: &str) -> String {
    format!("{}:{session}", link::CHANNEL)
}

/// The message a journal entry is, if it is one said on the desk.
pub fn desk_message(e: &Entry) -> Option<Message> {
    let d = &e.data;
    if d.get("channel").and_then(Value::as_str) != Some(link::CHANNEL) {
        return None;
    }
    let (hers, place) = match e.kind.as_str() {
        "heard" => (false, d.get("place")?.get("id")?.as_str()?),
        "said" => (true, d.get("place")?.as_str()?),
        _ => return None,
    };
    Some(Message {
        place: place.to_owned(),
        hers,
        text: d.get("text")?.as_str()?.to_owned(),
        at: e.at,
    })
}

/// Where a piece of work stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    /// It stopped to ask something and has not had an answer.
    Waiting,
    Finished,
    Blocked,
    Dropped,
}

impl State {
    pub fn color(self) -> Hsla {
        match self {
            State::Running => theme::accent(),
            State::Waiting => theme::purple(),
            State::Finished => theme::green(),
            State::Blocked => theme::coral(),
            State::Dropped => theme::muted(),
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Waiting => "waiting on you",
            State::Finished => "finished",
            State::Blocked => "blocked",
            State::Dropped => "dropped",
        }
    }
}

/// One piece of work, as its entries tell it.
pub struct Task<'a> {
    pub id: String,
    pub brief: String,
    pub started: DateTime<Utc>,
    /// Everything after `undertook` that names it, in order.
    pub events: Vec<&'a Entry>,
    /// Its parts, each a task of its own: `<id>/<n>`.
    pub parts: Vec<Task<'a>>,
}

impl Task<'_> {
    pub fn state(&self) -> State {
        if let Some(end) = self.events.iter().rev().find(|e| e.kind == "concluded") {
            return match s(&end.data, "outcome") {
                "finished" => State::Finished,
                "dropped" => State::Dropped,
                _ => State::Blocked,
            };
        }
        let asked = self.events.iter().rposition(|e| e.kind == "asked");
        let added = self.events.iter().rposition(|e| e.kind == "added");
        match (asked, added) {
            (Some(q), Some(a)) if a > q => State::Running,
            (Some(_), _) => State::Waiting,
            _ => State::Running,
        }
    }

    pub fn steps(&self) -> i64 {
        self.events
            .iter()
            .filter(|e| e.kind == "worked")
            .filter_map(|e| e.data.get("steps").and_then(Value::as_i64))
            .sum()
    }

    /// Whether it is still going: running, or waiting on you.
    pub fn open(&self) -> bool {
        matches!(self.state(), State::Running | State::Waiting)
    }

    /// When it last changed: its latest entry, or its start.
    pub fn changed(&self) -> DateTime<Utc> {
        self.events.last().map_or(self.started, |e| e.at)
    }

    /// One line about it in figures: steps, parts, how many still run.
    pub fn figures(&self) -> String {
        let mut meta = format!("{} steps", self.steps());
        if !self.parts.is_empty() {
            meta = format!("{meta} · {} parts", self.parts.len());
            let running = self.parts.iter().filter(|p| p.open()).count();
            if running > 0 {
                meta = format!("{meta}, {running} open");
            }
        }
        meta
    }

    pub fn ended(&self) -> Option<DateTime<Utc>> {
        self.events
            .iter()
            .rev()
            .find(|e| e.kind == "concluded")
            .map(|e| e.at)
    }

    /// The latest thing it said about itself.
    pub fn last_line(&self) -> String {
        self.events
            .iter()
            .rev()
            .find_map(|e| {
                let t = match e.kind.as_str() {
                    "worked" | "concluded" => s(&e.data, "summary"),
                    "reported" | "added" => s(&e.data, "text"),
                    "asked" => s(&e.data, "question"),
                    _ => "",
                };
                (!t.is_empty()).then(|| t.to_owned())
            })
            .unwrap_or_else(|| "starting".to_owned())
    }
}

/// Every task in a journal, by id, parts nested under their parent.
pub fn tasks(journal: &[Entry]) -> Vec<Task<'_>> {
    let mut all: Vec<Task<'_>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for e in journal {
        let id = s(&e.data, "id");
        if id.is_empty() {
            continue;
        }
        match e.kind.as_str() {
            "undertook" => {
                index.insert(id.to_owned(), all.len());
                all.push(Task {
                    id: id.to_owned(),
                    brief: s(&e.data, "brief").to_owned(),
                    started: e.at,
                    events: Vec::new(),
                    parts: Vec::new(),
                });
            }
            "worked" | "reported" | "asked" | "added" | "concluded" => {
                if let Some(i) = index.get(id) {
                    all[*i].events.push(e);
                }
            }
            _ => {}
        }
    }
    // Hand each part to its parent, last first so indices stay good.
    let mut top: Vec<Task<'_>> = Vec::new();
    let mut parts: Vec<Task<'_>> = Vec::new();
    for t in all {
        if t.id.contains('/') {
            parts.push(t);
        } else {
            top.push(t);
        }
    }
    for p in parts {
        let parent = p.id.rsplit_once('/').map(|(a, _)| a.to_owned());
        if let Some(t) = top.iter_mut().find(|t| Some(&t.id) == parent.as_ref()) {
            t.parts.push(p);
        }
    }
    top
}

/// The work taken on in one session, oldest first, parts under each: the
/// work whose `undertook` names the session's place. One pass over the
/// journal, since the sidebar asks this of every session on every tick.
pub fn session_tasks<'a>(journal: &'a [Entry], session: &str) -> Vec<Task<'a>> {
    let place = place_key(session);
    let ours: HashSet<&str> = journal
        .iter()
        .filter(|e| e.kind == "undertook" && s(&e.data, "place") == place)
        .map(|e| s(&e.data, "id"))
        .collect();
    if ours.is_empty() {
        return Vec::new();
    }
    tasks(journal)
        .into_iter()
        .filter(|t| ours.contains(t.id.as_str()))
        .collect()
}

/// The parts of a session's work still going, each with the work it is a
/// part of, for the rows nested under the session in the sidebar.
pub fn open_parts<'t, 'a>(tasks: &'t [Task<'a>]) -> Vec<&'t Task<'a>> {
    tasks
        .iter()
        .filter(|t| t.open())
        .flat_map(|t| t.parts.iter())
        .collect()
}

/// How a session stands at a glance: waiting on you if any of its work
/// asks something, running if any runs, otherwise nothing to show.
pub fn session_state(tasks: &[Task<'_>]) -> Option<State> {
    let states: Vec<State> = tasks.iter().map(Task::state).collect();
    [State::Waiting, State::Running]
        .into_iter()
        .find(|s| states.contains(s))
}

/// How many of a list to show, and how many are held back behind a "Show
/// more": everything once it has been asked for, else at most `limit`.
pub fn shown(total: usize, limit: usize, all: bool) -> (usize, usize) {
    if all || total <= limit {
        (total, 0)
    } else {
        (limit, total - limit)
    }
}

/// Where a file handed over by work came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A path on its machine, named by a `reported` entry.
    Report,
    /// A file it kept in its `media/` while working, from a `kept` entry.
    Kept,
}

/// A file a session's work handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// As the journal names it: a path on the machine for a report, a name
    /// in `media/` for something kept.
    pub file: String,
    pub source: Source,
}

impl Output {
    /// Its file name, without the directories.
    pub fn name(&self) -> &str {
        self.file.rsplit('/').next().unwrap_or(&self.file)
    }

    /// What sort of file it is, by its extension, and where it came from.
    pub fn kind(&self) -> String {
        let ext = self
            .name()
            .rsplit_once('.')
            .map_or("file", |(_, e)| e)
            .to_lowercase();
        let from = match self.source {
            Source::Report => "handed over",
            Source::Kept => "kept",
        };
        format!("{ext} · {from}")
    }
}

/// The files one session's work handed over, newest first and each once:
/// those its reports name, and those it kept along the way and has not
/// let go of. Read from the journal alone.
pub fn outputs(journal: &[Entry], session: &str) -> Vec<Output> {
    let tasks = session_tasks(journal, session);
    let ids: HashSet<&str> = tasks
        .iter()
        .flat_map(|t| std::iter::once(t.id.as_str()).chain(t.parts.iter().map(|p| p.id.as_str())))
        .collect();
    let let_go: HashSet<i64> = journal
        .iter()
        .filter(|e| e.kind == "let_go")
        .filter_map(|e| e.data.get("kept").and_then(Value::as_i64))
        .collect();
    let mut out: Vec<Output> = Vec::new();
    for e in journal.iter().rev() {
        let found: Vec<Output> = match e.kind.as_str() {
            "reported" if ids.contains(s(&e.data, "id")) => e
                .data
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|f| Output {
                    file: f.to_owned(),
                    source: Source::Report,
                })
                .collect(),
            "kept" if ids.contains(s(&e.data, "from")) && !let_go.contains(&e.seq) => {
                vec![Output {
                    file: s(&e.data, "file").to_owned(),
                    source: Source::Kept,
                }]
            }
            _ => Vec::new(),
        };
        for o in found {
            if !o.file.is_empty() && !out.iter().any(|x| x.file == o.file) {
                out.push(o);
            }
        }
    }
    out
}

/// One thing in the stream.
enum Item<'a> {
    Said { m: Message, seq: Option<i64> },
    Tool(&'a Entry),
    Task(Task<'a>),
}

/// What the journal says around the conversation: which of your messages
/// were directives.
fn directed(journal: &[Entry]) -> HashMap<i64, i64> {
    journal
        .iter()
        .filter(|e| e.kind == "directed")
        .filter_map(|e| Some((e.data.get("message")?.as_i64()?, e.seq)))
        .collect()
}

/// Things to ask for on an empty page. Clicking one only puts it in the
/// line you type into; nothing is sent until you send it.
pub const STARTERS: [&str; 3] = [
    "Find what is filling up my disk",
    "Summarise the last 20 commits in a repo",
    "Check whether port 7438 is in use",
];

/// The space between one turn and the next.
const TURN: f32 = 22.;

/// Everything the thread needs to draw one session.
pub struct View<'a> {
    pub journal: &'a [Entry],
    pub pending: &'a [Message],
    pub session: &'a str,
    pub name: String,
    pub who: String,
    pub tz: Tz,
    pub typing: bool,
    /// Whether the daemon is there to hear what you send.
    pub connected: bool,
    pub abilities: &'a Abilities,
    pub expanded: &'a HashSet<String>,
}

/// Everything in one session, in order of time: what was said, what was
/// opened or looked up there, the work taken on there, and what you sent
/// that the journal has not shown yet.
fn items<'a>(view: &View<'a>) -> Vec<(DateTime<Utc>, i64, Item<'a>)> {
    let place = place_key(view.session);
    let mut items: Vec<(DateTime<Utc>, i64, Item<'a>)> = Vec::new();
    for e in view.journal {
        if let Some(m) = desk_message(e) {
            if m.place == view.session {
                let seq = Some(e.seq);
                items.push((e.at, e.seq, Item::Said { m, seq }));
            }
        } else if matches!(e.kind.as_str(), "opened" | "looked_up")
            && e.data.get("place").and_then(Value::as_str) == Some(place.as_str())
        {
            items.push((e.at, e.seq, Item::Tool(e)));
        }
    }
    for t in session_tasks(view.journal, view.session) {
        items.push((t.started, 0, Item::Task(t)));
    }
    for m in view.pending.iter().filter(|m| m.place == view.session) {
        let said = Item::Said {
            m: m.clone(),
            seq: None,
        };
        items.push((m.at, i64::MAX, said));
    }
    items.sort_by_key(|(at, seq, _)| (*at, *seq));
    items
}

pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let items = items(view);
    if items.is_empty() && !view.typing {
        return empty(view, faces, cx);
    }
    let marks = directed(view.journal);
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(TURN))
        .w_full()
        .max_w(px(COLUMN))
        .mx_auto();

    // When you spoke last and nothing has answered since, the solver is
    // working on it.
    let mut waiting_since: Option<DateTime<Utc>> = None;
    let mut last_yours: Option<DateTime<Utc>> = None;
    let mut day: Option<String> = None;
    for (at, _, item) in items {
        let d = at.with_timezone(&view.tz).format("%A %-d %B").to_string();
        if day.as_ref() != Some(&d) {
            day = Some(d.clone());
            column = column.child(ui::eyebrow(faces, d).text_center());
        }
        let el = match item {
            Item::Said { m, .. } if m.hers => {
                let took = last_yours
                    .filter(|y| *y <= m.at)
                    .map(|y| format!(" · {}", span((m.at - y).num_seconds())))
                    .unwrap_or_default();
                waiting_since = None;
                hers(faces, &m, &view.name, view.tz, &took).into_any_element()
            }
            Item::Said { m, seq } => {
                last_yours = Some(m.at);
                waiting_since = Some(m.at);
                let directed = seq.and_then(|x| marks.get(&x).copied());
                yours(faces, &m, &view.who, view.tz, directed).into_any_element()
            }
            Item::Tool(e) => tool(faces, e, view.tz, view.expanded, cx).into_any_element(),
            Item::Task(t) => {
                waiting_since = None;
                task(faces, &t, view.tz, view.expanded, cx).into_any_element()
            }
        };
        column = column.child(el);
    }
    if view.typing || waiting_since.is_some() {
        column = column.child(working(faces, view, waiting_since));
    }

    div()
        .id("thread")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(32.))
        .pt(px(28.))
        .pb(px(24.))
        .child(column)
        .into_any_element()
}

/// The line under the last thing you said while nothing has answered: the
/// solver writing or thinking, for how long; or, with no daemon to hear
/// it, that your line is waiting to be delivered.
fn working(faces: &Faces, view: &View<'_>, since: Option<DateTime<Utc>>) -> Div {
    let (color, word) = if !view.connected {
        (theme::muted(), "waiting for the daemon")
    } else if view.typing {
        (theme::accent(), "writing")
    } else {
        (theme::accent(), "thinking")
    };
    let since = since.map_or_else(String::new, |w| {
        format!(" · {}", span((Utc::now() - w).num_seconds()))
    });
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(ui::dot(color).size(px(7.)))
        .child(
            ui::mono(faces, format!("{word}{since}"))
                .text_size(px(12.))
                .text_color(theme::muted()),
        )
}

/// A session with nothing in it yet: what the solver is, what it can do on
/// this machine as its deployment says, and a few things to ask for.
fn empty(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let mut column = div()
        .flex()
        .flex_col()
        .items_center()
        .w_full()
        .max_w(px(520.))
        .child(ui::icon("ghost", 40.).text_color(theme::muted()))
        .child(
            ui::serif(faces, "What needs doing?")
                .text_size(px(34.))
                .line_height(px(40.))
                .mt(px(18.)),
        )
        .child(
            ui::text(faces, view.abilities.sentence())
                .text_size(px(14.))
                .line_height(px(22.))
                .text_color(theme::muted())
                .text_center()
                .mt(px(10.)),
        );
    // The starters all need the machine; without it they would only be
    // promises.
    if view.abilities.local() {
        let mut starters = div().flex().flex_col().gap(px(8.)).w_full().mt(px(28.));
        for (i, text) in STARTERS.iter().enumerate() {
            starters = starters.child(starter(faces, i, text, cx));
        }
        column = column.child(starters);
    }
    div()
        .id("thread")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .items_center()
        .justify_center()
        .px(px(32.))
        .pb(px(40.))
        .child(column)
        .into_any_element()
}

/// One thing to ask for, which goes into the line you type into.
fn starter(faces: &Faces, i: usize, text: &'static str, cx: &mut Context<Desk>) -> Stateful<Div> {
    div()
        .id(("starter", i))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .h(px(42.))
        .px(px(16.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::border())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).border_color(theme::border2()))
        .child(ui::one_line(
            ui::text(faces, text)
                .text_size(px(13.5))
                .text_color(theme::text2()),
        ))
        .child(ui::icon("chevron-right", 14.).text_color(theme::faint()))
        .on_click(cx.listener(move |this, _, window, cx| this.prefill(text, window, cx)))
}

/// A line of small print under a message: who, when, and anything else.
fn meta(faces: &Faces, text: String) -> Div {
    ui::mono(faces, text)
        .text_size(px(11.))
        .text_color(theme::faint())
}

/// What you said: a bubble on the right, the way a chat has it.
fn yours(faces: &Faces, m: &Message, who: &str, tz: Tz, directed: Option<i64>) -> Div {
    let mut line = format!("{who} · {}", clock(m.at, tz));
    if let Some(seq) = directed {
        line = format!("{line} · directed · #{seq}");
    }
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(6.))
        .child(
            div()
                .max_w(relative(0.8))
                .px(px(16.))
                .py(px(11.))
                .rounded(px(14.))
                .bg(theme::panel2())
                .when(directed.is_some(), |d| {
                    d.border_1().border_color(theme::purple().opacity(0.4))
                })
                .child(ui::text(faces, m.text.clone()).line_height(px(22.))),
        )
        .child(meta(faces, line).mr(px(4.)))
}

/// What the solver said: plain text in the reading column.
fn hers(faces: &Faces, m: &Message, name: &str, tz: Tz, took: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(markdown::render(faces, &m.text))
        .child(meta(faces, format!("{name} · {}{took}", clock(m.at, tz))))
}

/// A quiet card: the frame tools and work are drawn in.
fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(theme::border())
}

/// Something opened or looked up while answering: one line, opened on a click.
fn tool(
    faces: &Faces,
    e: &Entry,
    tz: Tz,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Stateful<Div> {
    let key = format!("tool:{}", e.seq);
    let open = expanded.contains(&key);
    let (verb, what, body) = if e.kind.as_str() == "looked_up" {
        ("Search", s(&e.data, "query"), s(&e.data, "found"))
    } else {
        let url = s(&e.data, "url");
        let verb = if url.starts_with("http") {
            "Fetch"
        } else {
            "Read"
        };
        (verb, url, s(&e.data, "text"))
    };
    let body: String = body.chars().take(1600).collect();
    let head = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(36.))
        .px(px(14.))
        .child(ui::dot(theme::green()).size(px(6.)))
        .child(
            ui::text(faces, verb)
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .flex_none(),
        )
        .child(ui::one_line(
            ui::mono(faces, what.to_owned())
                .text_size(px(12.))
                .text_color(theme::muted()),
        ))
        .child(
            meta(
                faces,
                format!("{} {}", clock(e.at, tz), if open { "⌃" } else { "⌄" }),
            )
            .flex_none(),
        );
    let toggle = key.clone();
    card()
        .id(SharedString::from(key))
        .cursor_pointer()
        .hover(|s| s.border_color(theme::border2()))
        .child(head)
        .when(open && !body.is_empty(), |d| {
            d.child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(theme::border())
                    .bg(theme::well())
                    .rounded_b(px(10.))
                    .font_family(faces.mono.clone())
                    .text_size(px(11.5))
                    .line_height(px(17.))
                    .text_color(theme::text2())
                    .child(body),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
}

/// A piece of work: a card that says where it stands at a glance, and
/// opens to everything it said along the way and its parts.
fn task(
    faces: &Faces,
    t: &Task<'_>,
    tz: Tz,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Stateful<Div> {
    let key = format!("task:{}", t.id);
    let open = expanded.contains(&key);
    let state = t.state();
    let elapsed = (t.ended().unwrap_or_else(Utc::now) - t.started).num_seconds();
    let figures = format!("{} · {}", span(elapsed), t.figures());

    let head = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(ui::dot(state.color()).size(px(7.)))
        .child(ui::one_line(
            ui::text(faces, t.brief.clone())
                .text_size(px(13.5))
                .font_weight(gpui::FontWeight::MEDIUM),
        ))
        .child(
            ui::mono(faces, state.word())
                .text_size(px(11.))
                .text_color(state.color())
                .flex_none(),
        );

    let summary = div()
        .flex()
        .items_baseline()
        .justify_between()
        .gap(px(12.))
        .pl(px(17.))
        .child(ui::one_line(
            ui::text(faces, t.last_line())
                .text_size(px(12.5))
                .text_color(theme::muted()),
        ))
        .child(meta(faces, format!("{figures} {}", if open { "⌃" } else { "⌄" })).flex_none());

    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(14.))
        .py(px(12.))
        .child(head)
        .child(summary);
    if open {
        body = body.child(timeline(faces, t, tz));
    }

    let toggle = key.clone();
    card()
        .id(SharedString::from(key))
        .bg(theme::panel())
        .when(t.open(), |d| d.border_color(theme::border2()))
        .cursor_pointer()
        .child(body)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
}

/// Everything a piece of work said along the way, then its parts.
fn timeline(faces: &Faces, t: &Task<'_>, tz: Tz) -> Div {
    let mut timeline = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .mt(px(6.))
        .pt(px(10.))
        .pl(px(17.))
        .border_t_1()
        .border_color(theme::border());
    for e in &t.events {
        let (tag, text) = match e.kind.as_str() {
            "worked" => (
                format!(
                    "worked · {}",
                    e.data
                        .get("steps")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                ),
                s(&e.data, "summary"),
            ),
            "concluded" => (s(&e.data, "outcome").to_owned(), s(&e.data, "summary")),
            "asked" => ("asked".to_owned(), s(&e.data, "question")),
            other => (other.to_owned(), s(&e.data, "text")),
        };
        timeline = timeline.child(
            div()
                .flex()
                .gap(px(10.))
                .items_baseline()
                .child(meta(faces, clock(e.at, tz)).w(px(38.)).flex_none())
                .child(
                    ui::mono(faces, tag)
                        .text_size(px(11.))
                        .text_color(theme::kind_color(&e.kind))
                        .w(px(76.))
                        .flex_none(),
                )
                .child(
                    ui::text(faces, text.to_owned())
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(12.5))
                        .text_color(theme::text2())
                        .line_height(px(18.)),
                ),
        );
    }
    if !t.parts.is_empty() {
        let mut parts = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .mt(px(4.))
            .child(ui::eyebrow(faces, "parts").text_size(px(10.5)).pb(px(2.)));
        for p in &t.parts {
            parts = parts.child(part(faces, p));
        }
        timeline = timeline.child(parts);
    }
    timeline
}

/// One part of a piece of work, in its card.
fn part(faces: &Faces, p: &Task<'_>) -> Div {
    let ps = p.state();
    let n = p.id.rsplit('/').next().unwrap_or_default().to_owned();
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .py(px(3.))
        .child(ui::dot(ps.color()).size(px(6.)))
        .child(meta(faces, format!("#{n}")).flex_none())
        .child(ui::one_line(
            ui::text(faces, p.brief.clone()).text_size(px(12.5)),
        ))
        .child(
            ui::mono(faces, format!("{} · {}", ps.word(), p.last_line()))
                .text_size(px(11.))
                .text_color(theme::muted())
                .max_w(px(260.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .flex_none(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(seq: i64, kind: &str, data: Value) -> Entry {
        Entry {
            seq,
            at: Utc::now(),
            kind: kind.to_owned(),
            data,
        }
    }

    #[test]
    fn a_task_waits_on_you_from_its_question_until_its_answer() {
        let mut journal = vec![
            entry(1, "undertook", json!({"id": "a", "brief": "tidy"})),
            entry(2, "asked", json!({"id": "a", "question": "which folder?"})),
        ];
        assert_eq!(tasks(&journal)[0].state(), State::Waiting);
        journal.push(entry(3, "added", json!({"id": "a", "text": "Downloads"})));
        assert_eq!(tasks(&journal)[0].state(), State::Running);
        journal.push(entry(
            4,
            "concluded",
            json!({"id": "a", "outcome": "gave_up", "summary": ""}),
        ));
        assert_eq!(tasks(&journal)[0].state(), State::Blocked);
    }

    #[test]
    fn parts_are_kept_under_the_task_that_handed_them_out() {
        let journal = vec![
            entry(1, "undertook", json!({"id": "a", "brief": "tidy"})),
            entry(2, "undertook", json!({"id": "a/1", "brief": "photos"})),
            entry(
                3,
                "worked",
                json!({"id": "a/1", "summary": "sorted", "steps": 4}),
            ),
            entry(4, "undertook", json!({"id": "b", "brief": "other"})),
        ];
        let all = tasks(&journal);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].parts.len(), 1);
        assert_eq!(all[0].parts[0].steps(), 4);
    }

    /// Two sessions' worth of journal, as the daemon writes it: work taken
    /// on in `s1` with two parts, one done; work in `s2`; a file reported
    /// and one kept by `s1`'s work, one kept and let go, one kept by `s2`.
    fn two_sessions() -> Vec<Entry> {
        vec![
            entry(
                1,
                "undertook",
                json!({"id": "w1", "brief": "tidy downloads", "place": "desk:s1"}),
            ),
            entry(2, "undertook", json!({"id": "w1/1", "brief": "images"})),
            entry(3, "undertook", json!({"id": "w1/2", "brief": "archives"})),
            entry(
                4,
                "concluded",
                json!({"id": "w1/1", "outcome": "finished", "summary": "done"}),
            ),
            entry(
                5,
                "undertook",
                json!({"id": "w2", "brief": "other", "place": "desk:s2"}),
            ),
            entry(
                6,
                "reported",
                json!({"id": "w1", "text": "listed", "files": ["/home/x/list.md"]}),
            ),
            entry(
                7,
                "kept",
                json!({"file": "abc.png", "note": "a chart", "from": "w1/2"}),
            ),
            entry(
                8,
                "kept",
                json!({"file": "old.png", "note": "gone", "from": "w1"}),
            ),
            entry(9, "let_go", json!({"kept": 8})),
            entry(
                10,
                "kept",
                json!({"file": "theirs.png", "note": "", "from": "w2"}),
            ),
            entry(
                11,
                "reported",
                json!({"id": "w1", "text": "again", "files": ["/home/x/list.md"]}),
            ),
        ]
    }

    #[test]
    fn a_session_holds_only_the_work_taken_on_in_its_place() {
        let journal = two_sessions();
        let s1 = session_tasks(&journal, "s1");
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].id, "w1");
        assert_eq!(session_tasks(&journal, "s2")[0].id, "w2");
        assert!(session_tasks(&journal, "s3").is_empty());
    }

    #[test]
    fn the_sidebar_nests_every_part_of_open_work_and_none_of_finished_work() {
        let mut journal = two_sessions();
        let s1 = session_tasks(&journal, "s1");
        let parts: Vec<&str> = open_parts(&s1).iter().map(|p| p.brief.as_str()).collect();
        assert_eq!(parts, ["images", "archives"]);
        assert_eq!(open_parts(&s1)[0].state(), State::Finished);
        assert_eq!(session_state(&s1), Some(State::Running));

        journal.push(entry(
            12,
            "concluded",
            json!({"id": "w1", "outcome": "finished", "summary": ""}),
        ));
        let s1 = session_tasks(&journal, "s1");
        assert!(open_parts(&s1).is_empty());
        assert_eq!(session_state(&s1), None);
    }

    #[test]
    fn a_session_shows_waiting_before_running() {
        let journal = vec![
            entry(
                1,
                "undertook",
                json!({"id": "a", "brief": "x", "place": "desk:s"}),
            ),
            entry(
                2,
                "undertook",
                json!({"id": "b", "brief": "y", "place": "desk:s"}),
            ),
            entry(3, "asked", json!({"id": "b", "question": "which?"})),
        ];
        assert_eq!(
            session_state(&session_tasks(&journal, "s")),
            Some(State::Waiting)
        );
    }

    #[test]
    fn outputs_are_the_files_this_sessions_work_handed_over_each_once() {
        let journal = two_sessions();
        let out = outputs(&journal, "s1");
        let files: Vec<&str> = out.iter().map(|o| o.file.as_str()).collect();
        assert_eq!(files, ["/home/x/list.md", "abc.png"], "{out:?}");
        assert_eq!(out[0].name(), "list.md");
        assert_eq!(out[0].kind(), "md · handed over");
        assert_eq!(out[1].kind(), "png · kept");
        assert_eq!(outputs(&journal, "s2")[0].file, "theirs.png");
        assert!(outputs(&journal, "s3").is_empty());
    }

    #[test]
    fn a_long_list_shows_five_until_the_rest_are_asked_for() {
        assert_eq!(shown(3, 5, false), (3, 0));
        assert_eq!(shown(5, 5, false), (5, 0));
        assert_eq!(shown(8, 5, false), (5, 3));
        assert_eq!(shown(8, 5, true), (8, 0));
    }
}
