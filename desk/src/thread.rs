//! The thread of one place, in one stream by time: what passed between
//! you and the little ghost in a session, or between a character and
//! whoever she was with; what was opened or looked up while answering; a
//! card for each piece of work taken on there, with its parts under it
//! (drawn in `thread/cards.rs`); and a quiet line where she could not think,
//! or was stopped. All of it is read from the ghost's journal, so what
//! shows here is what is on record; what was said a moment ago and not yet
//! written follows at the end.

mod cards;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::CONSOLE;
use gpui::{AnyElement, Context, Div, Hsla, SharedString, Stateful, div, prelude::*, px, relative};
use serde_json::Value;

use self::cards::{task, tool};
use crate::{
    desk::Desk,
    fonts::Faces,
    markdown,
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

/// The channel the desk spoke on before it spoke through the console: it
/// registered a presence of its own called `desk`. What was said there is
/// still in her journal, under the same place ids.
pub const OLD_CHANNEL: &str = "desk";

/// The key of the place you speak to her in from the desk, by its id: a
/// session's id with the solver, or your name with a character.
pub fn place_key(id: &str) -> String {
    format!("{CONSOLE}:{id}")
}

/// The thread a place belongs to, by its key. A place on the old desk
/// channel is one thread with the place of the same id on the console
/// channel, so that what was said in a session before the desk spoke
/// through the console is still in it; every other place is its own.
pub fn thread_key(key: &str) -> String {
    match key.split_once(':') {
        Some((OLD_CHANNEL, id)) => place_key(id),
        _ => key.to_owned(),
    }
}

/// Whether something said on `channel` was said from the desk, now or before.
fn from_desk(channel: &str) -> bool {
    channel == CONSOLE || channel == OLD_CHANNEL
}

/// The key of the place the work `work` was taken on in, if her journal's
/// tail still has it.
pub fn task_place(journal: &[Entry], work: &str) -> Option<String> {
    journal
        .iter()
        .find(|e| e.kind == "undertook" && s(&e.data, "id") == work)
        .map(|e| s(&e.data, "place").to_owned())
        .filter(|p| !p.is_empty())
}

/// Something said, as a journal entry tells it, on any channel.
pub struct Said {
    pub channel: String,
    /// Who said it, by their id and name on the channel, when it was not her.
    pub from: Option<(String, String)>,
    /// What the channel calls the place, when it names it.
    pub place_name: Option<String>,
    /// The place is a private one, with one person.
    pub direct: bool,
    /// The message, with the place's id on its channel.
    pub message: Message,
}

impl Said {
    /// The key of the place it was said in, as her memory knows it.
    pub fn key(&self) -> String {
        format!("{}:{}", self.channel, self.message.place)
    }
}

/// What a journal entry said, if it is something said: heard by her, or
/// said by her.
pub fn said(e: &Entry) -> Option<Said> {
    let d = &e.data;
    let (from, place_name, direct, place) = match e.kind.as_str() {
        "heard" => {
            let from = d.get("from")?;
            let place = d.get("place")?;
            (
                Some((s(from, "id").to_owned(), s(from, "name").to_owned())),
                place.get("name").and_then(Value::as_str).map(str::to_owned),
                s(place, "kind") == "direct",
                place.get("id")?.as_str()?,
            )
        }
        "said" => (None, None, false, d.get("place")?.as_str()?),
        _ => return None,
    };
    Some(Said {
        channel: d.get("channel")?.as_str()?.to_owned(),
        message: Message {
            place: place.to_owned(),
            hers: from.is_none(),
            text: d.get("text")?.as_str()?.to_owned(),
            at: e.at,
        },
        from,
        place_name,
        direct,
    })
}

/// The message a journal entry is, if it is one said on the console
/// channel, where what you type now goes.
pub fn console_message(e: &Entry) -> Option<Message> {
    said(e).filter(|s| s.channel == CONSOLE).map(|s| s.message)
}

/// A place in her journal where something was said, as the place picker
/// shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    /// `channel:id`.
    pub key: String,
    /// Its name, or for a private place who is on the other side; and its
    /// channel.
    pub label: String,
    /// When something was last said there.
    pub at: DateTime<Utc>,
}

/// Every place something was said in `journal`, the most recent first. A
/// place on the old desk channel is listed as one with its place on the
/// console channel.
pub fn places(journal: &[Entry]) -> Vec<Spot> {
    let mut spots: Vec<Spot> = Vec::new();
    for said in journal.iter().filter_map(said) {
        let key = thread_key(&said.key());
        let channel = if from_desk(&said.channel) {
            CONSOLE
        } else {
            said.channel.as_str()
        };
        // A place is called by its own name, or a private one by who it is
        // with.
        let name = said.place_name.clone().or_else(|| {
            said.from
                .as_ref()
                .filter(|_| said.direct)
                .map(|(_, name)| name.clone())
        });
        let at = said.message.at;
        match spots.iter_mut().find(|s| s.key == key) {
            Some(spot) => {
                spot.at = spot.at.max(at);
                if let Some(name) = name {
                    spot.label = format!("{name} · {channel}");
                }
            }
            None => spots.push(Spot {
                label: format!(
                    "{} · {channel}",
                    name.unwrap_or_else(|| said.message.place.clone()),
                ),
                key,
                at,
            }),
        }
    }
    spots.sort_by(|a, b| b.at.cmp(&a.at));
    spots
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
    place_tasks(journal, &place_key(session))
}

/// The work taken on in the place `place`, a place key, or in the place
/// on the old desk channel that is one thread with it; oldest first, parts
/// under each.
pub fn place_tasks<'a>(journal: &'a [Entry], place: &str) -> Vec<Task<'a>> {
    let thread = thread_key(place);
    let ours: HashSet<&str> = journal
        .iter()
        .filter(|e| e.kind == "undertook" && thread_key(s(&e.data, "place")) == thread)
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
    /// Something said: by her, or by `from`, which is you when `mine`.
    Said {
        m: Message,
        seq: Option<i64>,
        from: Option<String>,
        mine: bool,
    },
    Tool(&'a Entry),
    Task(Task<'a>),
    /// She looked and could not think, and so said nothing: why, as her
    /// journal's `decided` entry has it.
    Failed(String),
    /// A stop was asked for from the desk; whether anything was stopped.
    Stopped(bool),
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

/// Everything the thread needs to draw one place: a session with the
/// solver, or any place a character is in.
pub struct View<'a> {
    pub journal: &'a [Entry],
    /// What you said that the journal has not shown yet.
    pub pending: &'a [Message],
    /// The place, by its key: `console:<session>` for a session. What was
    /// said in its place on the old desk channel is in its thread too.
    pub place: &'a str,
    /// The stops asked for from the desk, in whatever place.
    pub stops: &'a [crate::model::Stop],
    pub name: String,
    pub who: String,
    pub tz: Tz,
    /// Whether the daemon is there to hear what you send.
    pub connected: bool,
    /// What the solver can do, for its empty page. A character's empty
    /// place has none to offer.
    pub abilities: Option<&'a Abilities>,
    pub expanded: &'a HashSet<String>,
}

/// Everything in one place, in order of time: what was said, what was
/// opened or looked up there, the work taken on there, and what you sent
/// that the journal has not shown yet.
fn items<'a>(view: &View<'a>) -> Vec<(DateTime<Utc>, i64, Item<'a>)> {
    let you = view.who.to_lowercase();
    let thread = thread_key(view.place);
    let mut items: Vec<(DateTime<Utc>, i64, Item<'a>)> = Vec::new();
    for e in view.journal {
        if let Some(said) = said(e) {
            if thread_key(&said.key()) == thread {
                // Yours if it came from you at the desk: anyone else's
                // words are drawn the same way, under their own name.
                let mine = from_desk(&said.channel)
                    && said.from.as_ref().is_some_and(|(id, _)| *id == you);
                let item = Item::Said {
                    m: said.message,
                    seq: Some(e.seq),
                    from: said.from.map(|(_, name)| name),
                    mine,
                };
                items.push((e.at, e.seq, item));
            }
        } else if matches!(e.kind.as_str(), "opened" | "looked_up")
            && e.data
                .get("place")
                .and_then(Value::as_str)
                .is_some_and(|p| thread_key(p) == thread)
        {
            items.push((e.at, e.seq, Item::Tool(e)));
        } else if let Some(why) = could_not_think(e)
            && thread_key(s(&e.data, "about")) == thread
        {
            items.push((e.at, e.seq, Item::Failed(why.to_owned())));
        }
    }
    for stop in view.stops.iter().filter(|x| place_key(&x.place) == thread) {
        items.push((stop.at, i64::MAX, Item::Stopped(stop.stopped)));
    }
    for t in place_tasks(view.journal, view.place) {
        items.push((t.started, 0, Item::Task(t)));
    }
    for m in view
        .pending
        .iter()
        .filter(|m| place_key(&m.place) == thread)
    {
        let said = Item::Said {
            m: m.clone(),
            seq: None,
            from: None,
            mine: true,
        };
        items.push((m.at, i64::MAX, said));
    }
    items.sort_by_key(|(at, seq, _)| (*at, *seq));
    items
}

/// Why she could not think, if `e` is the entry that says so: when a look
/// fails for good she decides on nothing, and the reason says why.
fn could_not_think(e: &Entry) -> Option<&str> {
    (e.kind == "decided" && s(&e.data, "decision") == "nothing")
        .then(|| s(&e.data, "reason").strip_prefix("could not think:"))
        .flatten()
        .map(str::trim)
}

/// Why she could not think, as a person reads it: a brain that is not
/// signed in says how to sign it in; anything else is the first line of
/// what the brain said.
pub fn could_not(reason: &str) -> String {
    let low = reason.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| low.contains(w));
    if has(&["please run /login", "/login"]) || (low.contains("claude") && has(&["oauth", "401"])) {
        return "Couldn't think: Claude Code is not logged in — run `claude` in a terminal and /login".to_owned();
    }
    if has(&[
        "401",
        "403",
        "unauthorized",
        "authentication",
        "not logged in",
        "invalid api key",
        "invalid_api_key",
    ]) {
        return "Couldn't think: the brain did not accept its key or its login".to_owned();
    }
    let first = reason.lines().next().unwrap_or_default().trim();
    let mut line: String = first.chars().take(160).collect();
    if first.chars().count() > 160 {
        line.push('…');
    }
    format!("Couldn't think: {line}")
}

/// Since when what you said last in this place has waited on her: nothing
/// she said, no work she took on, no failure and no stop has come after it.
fn waiting_since(items: &[(DateTime<Utc>, i64, Item<'_>)]) -> Option<DateTime<Utc>> {
    let mut since = None;
    for (_, _, item) in items {
        match item {
            // Only what you said waits on her: in a character's places
            // others speak, and she need not answer them.
            Item::Said { m, mine: true, .. } if !m.hers => since = Some(m.at),
            Item::Said { m, .. } if m.hers => since = None,
            Item::Task(_) | Item::Failed(_) | Item::Stopped(_) => since = None,
            _ => {}
        }
    }
    since
}

/// Whether she is busy with what you said in the place `view` shows:
/// thinking about your last line, or at work taken on there that is still
/// open. While she is, the composer offers to stop her.
pub fn busy(view: &View<'_>) -> bool {
    waiting_since(&items(view)).is_some()
        || place_tasks(view.journal, view.place).iter().any(Task::open)
}

pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let items = items(view);
    if items.is_empty() {
        return empty(view, faces, cx);
    }
    let waiting = waiting_since(&items);
    let marks = directed(view.journal);
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(TURN))
        .w_full()
        .max_w(px(COLUMN))
        .mx_auto();

    // What you said last, and when: a failure offers to say it again.
    let mut last_yours: Option<DateTime<Utc>> = None;
    let mut last_line: Option<String> = None;
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
                hers(faces, &m, &view.name, view.tz, &took).into_any_element()
            }
            Item::Said { m, seq, from, mine } => {
                if mine {
                    last_yours = Some(m.at);
                    last_line = Some(m.text.clone());
                }
                let directed = seq.and_then(|x| marks.get(&x).copied());
                let who = from.as_deref().unwrap_or(&view.who);
                yours(faces, &m, who, view.tz, directed).into_any_element()
            }
            Item::Tool(e) => tool(faces, e, view.tz, view.expanded, cx).into_any_element(),
            Item::Task(t) => task(faces, &t, view.tz, view.expanded, cx).into_any_element(),
            Item::Failed(why) => {
                failed(faces, &why, at, view.tz, last_line.clone(), cx).into_any_element()
            }
            Item::Stopped(stopped) => {
                let word = if stopped {
                    "Stopped"
                } else {
                    "Nothing was running to stop"
                };
                note(faces, format!("{word} · {}", clock(at, view.tz))).into_any_element()
            }
        };
        column = column.child(el);
    }
    if let Some(since) = waiting {
        column = column.child(working(faces, view, since));
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

/// A small, quiet line in the thread: something that happened rather than
/// something said.
fn note(faces: &Faces, text: String) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(ui::dot(theme::faint()).size(px(7.)))
        .child(
            ui::mono(faces, text)
                .text_size(px(12.))
                .text_color(theme::muted()),
        )
}

/// Where her answer would be, when she could not think: why, calmly, and
/// a way to say your last line again.
fn failed(
    faces: &Faces,
    why: &str,
    at: DateTime<Utc>,
    tz: Tz,
    again: Option<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut line = div()
        .flex()
        .items_start()
        .gap(px(10.))
        .child(div().pt(px(6.)).child(ui::dot(theme::faint()).size(px(7.))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .flex_1()
                .min_w(px(0.))
                .child(
                    ui::text(faces, could_not(why))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(theme::text2()),
                )
                .child(meta(faces, clock(at, tz))),
        );
    if let Some(text) = again {
        line = line.child(
            div()
                .id(SharedString::from(format!(
                    "again-{}",
                    at.timestamp_millis()
                )))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(theme::border2())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon("retry", 12.).text_color(theme::muted()))
                .child(
                    ui::text(faces, "Retry")
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.say_again(text.clone(), cx))),
        );
    }
    line
}

/// The line under the last thing you said while nothing has answered: the
/// solver thinking, for how long; or, with no daemon to hear it, that your
/// line is waiting to be delivered.
fn working(faces: &Faces, view: &View<'_>, since: DateTime<Utc>) -> Div {
    let (color, word) = if view.connected {
        (theme::accent(), "thinking")
    } else {
        (theme::muted(), "waiting for the daemon")
    };
    let since = format!(" · {}", span((Utc::now() - since).num_seconds()));

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
    let Some(abilities) = view.abilities else {
        return div()
            .id("thread")
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(
                ui::text(
                    faces,
                    format!("Nothing is said here in {}'s journal yet.", view.name),
                )
                .text_size(px(14.))
                .text_color(theme::muted()),
            )
            .into_any_element();
    };
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
            ui::text(faces, abilities.sentence())
                .text_size(px(14.))
                .line_height(px(22.))
                .text_color(theme::muted())
                .text_center()
                .mt(px(10.)),
        );
    // The starters all need the machine; without it they would only be
    // promises.
    if abilities.local() {
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
