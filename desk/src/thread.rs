//! The thread of one place, in one stream by time: what passed between
//! you and Ghost in a session, or between a character and
//! whoever she was with; what was opened or looked up while answering;
//! what she ran, read and searched before she answered, as `look.steps`
//! gives it; a card for each piece of work taken on there, with its steps
//! and its parts under it; and a quiet line where she could not think, or
//! was stopped. What was said and done is read from the ghost's journal,
//! so what shows here is what is on record; what was said a moment ago
//! and not yet written follows at the end.
//!
//! This file finds what is in the stream. How it is drawn is a
//! [`Rendering`]: task-first, the way a coding agent's app shows its work
//! (`thread/task_first.rs`, with the cards in `thread/cards.rs` and the
//! steps in `thread/steps.rs`), is the one there is.

mod calls;
mod cards;
mod drawn;
mod kinds;
mod live;
mod output;
mod outputs;
mod platform;
mod steps;
pub mod style;
mod task_first;
#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::{CONSOLE, Look};
use gpui::{AnyElement, Context, Hsla};
use serde_json::Value;

pub use self::{
    outputs::{Output, Source, outputs},
    style::COLUMN,
};
use crate::{
    desk::Desk,
    follow::{Following, PlaceLooks},
    fonts::Faces,
    model::{Entry, Message},
    theme,
};

/// How a thread is drawn. Task-first shows what she did as a coding
/// agent's app does: each command and file and search, under a line
/// saying how long it took. A thread read as a story would be drawn
/// another way from the same stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Rendering {
    #[default]
    Task,
}

/// A time of day in the solver's clock.
pub fn clock(at: DateTime<Utc>, tz: Tz) -> String {
    at.with_timezone(&tz).format("%H:%M").to_string()
}

/// When something was, as a person says it under a message: just now, so
/// many minutes or hours ago, or the day and time in her clock.
pub fn ago(at: DateTime<Utc>, tz: Tz) -> String {
    let secs = (Utc::now() - at).num_seconds();
    match secs {
        s if s < 45 => "just now".to_owned(),
        s if s < 3600 => {
            let m = (s / 60).max(1);
            format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
        }
        s if s < 86_400 => {
            let h = s / 3600;
            format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
        }
        _ => at.with_timezone(&tz).format("%-d %b · %H:%M").to_string(),
    }
}

/// A span of time in a word or two.
pub fn span(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
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
    /// Still going: running, or waiting on an answer.
    pub fn open(self) -> bool {
        matches!(self, State::Running | State::Waiting)
    }

    pub fn color(self) -> Hsla {
        match self {
            State::Running => theme::accent(),
            State::Waiting => theme::accent(),
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
    /// The folder it is done in, when it has one.
    pub dir: Option<String>,
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

    /// Whether it is still going: running, or waiting on you.
    pub fn open(&self) -> bool {
        matches!(self.state(), State::Running | State::Waiting)
    }

    /// When it last changed: its latest entry, or its start.
    pub fn changed(&self) -> DateTime<Utc> {
        self.events.last().map_or(self.started, |e| e.at)
    }

    /// The brief as she wrote it for the work: without the words of
    /// whoever asked for it, which the work is handed after it and which
    /// are in the thread above it already.
    pub fn brief_shown(&self) -> &str {
        let brief = self.brief.trim();
        brief
            .find(" wrote, word for word:")
            .and_then(|i| brief[..i].rfind("\n\n"))
            .map_or(brief, |cut| brief[..cut].trim_end())
    }

    /// What the work is, in a line: the first of its brief.
    pub fn title(&self) -> &str {
        self.brief_shown()
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default()
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
                    dir: e.data.get("dir").and_then(Value::as_str).map(str::to_owned),
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
    /// What she called in one look before she answered, from `from` (what
    /// she was answering) to `to` (the entry it ended with).
    Look {
        look: &'a Look,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
    Task(Task<'a>),
    /// She looked and could not think, and so said nothing: why, as her
    /// journal's `decided` entry has it.
    Failed(String),
    /// A stop was asked for from the desk; whether anything was stopped.
    Stopped(bool),
    /// Something she did: out there, shown in a console; or here, an
    /// image she sent, a quiet row for something she did inside, a send
    /// the platform refused.
    Did(platform::Did),
    /// She folded the earlier part of the conversation here into a note,
    /// which is what she sees of it from then on: the note, and the seq
    /// of the entry it is in.
    Folded {
        seq: i64,
        text: String,
    },
    /// Her body started her on another brain: the one she thinks with
    /// from here on, and the one before, as each names itself.
    Brain {
        think: String,
        was: String,
    },
}

/// The thread of `view.place` as text: who said what, with the time,
/// one blank line between turns; and how many lines of it there are.
pub fn transcript(view: &View<'_>) -> (String, usize) {
    let mut out = String::new();
    let mut lines = 0;
    for (at, _, item) in items(view) {
        // An image she sent here is a line of hers, by its name.
        if let Item::Did(platform::Did {
            here: true,
            what: platform::What::Image { file, .. },
        }) = &item
        {
            let _ = writeln!(
                out,
                "{} · {}\n[image: {file}]\n",
                view.name,
                clock(at, view.tz)
            );
            lines += 1;
        }
        let Item::Said { m, from, mine, .. } = item else {
            continue;
        };
        let who = if m.hers {
            view.name.clone()
        } else if mine {
            view.who.clone()
        } else {
            from.unwrap_or_else(|| view.who.clone())
        };
        let _ = writeln!(out, "{who} · {}\n{}\n", clock(at, view.tz), m.text.trim());
        lines += 1;
    }
    (out.trim_end().to_owned(), lines)
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
    pub expanded: &'a HashSet<String>,
    /// What has been read of her steps on this ghost.
    pub following: &'a Following,
    /// Her work as her daemon keeps it: what a piece asks, and the options
    /// it gave to choose from, which her journal does not hold.
    pub work: &'a [ghost_link::protocol::Work],
    /// Whose thread this is, for answering what her work asks.
    pub whom: crate::answer::Whom,
    /// Where the thread is scrolled to, when it is drawn on screen.
    pub scroll: Option<&'a gpui::ScrollHandle>,
    pub rendering: Rendering,
    /// How wide the reading column is drawn, in points: 680 where the
    /// pane has room, less in a narrow window. What must end in an
    /// ellipsis on one line is cut to it, since GPUI's own ellipsis is not
    /// to be trusted with Chinese.
    pub column: f32,
    /// Her `media/` directory, when it is on this machine: the solver's.
    /// A character's is where her daemon runs, and her images are not
    /// shown, only named.
    pub media: Option<std::path::PathBuf>,
    /// What her daemon last said of her: for an empty thread, whether her
    /// day has her awake and what it has her doing.
    pub status: Option<&'a crate::model::Status>,
}

/// Everything in one place, in order of time: what was said, what was
/// opened or looked up there, the work taken on there, and what you sent
/// that the journal has not shown yet.
fn items<'a>(view: &View<'a>) -> Vec<(DateTime<Utc>, i64, Item<'a>)> {
    let you = view.who.to_lowercase();
    let thread = thread_key(view.place);
    let looks = view.following.places.get(&thread);
    // What was last said to her here: a look is timed from it.
    let mut heard: Option<DateTime<Utc>> = None;
    let mut items: Vec<(DateTime<Utc>, i64, Item<'a>)> = Vec::new();
    let gone = taken_back(view.journal, &thread);
    let known = platform::Known::of(view.journal);
    for e in view.journal {
        // A change of brain is hers, not the thread's: what was taken
        // back does not take it with it.
        if let Some(item) = brain_change(e) {
            items.push((e.at, e.seq, item));
            continue;
        }
        // What was taken back is out of the thread, as it is out of the
        // conversation in front of her.
        if gone.iter().any(|(from, to)| (*from..*to).contains(&e.seq)) {
            continue;
        }
        if let Some(did) = platform::did(e, &thread, &known) {
            items.push((e.at, e.seq, Item::Did(did)));
        }
        if e.kind == "digested" && thread_key(s(&e.data, "place")) == thread {
            let text = s(&e.data, "text").trim().to_owned();
            items.push((e.at, e.seq, Item::Folded { seq: e.seq, text }));
            continue;
        }
        if let Some(mut said) = said(e) {
            if thread_key(&said.key()) == thread {
                if !said.message.hers {
                    heard = Some(e.at);
                }
                // Yours if it came from you at the desk: anyone else's
                // words are drawn the same way, under their own name.
                let mine = from_desk(&said.channel)
                    && said.from.as_ref().is_some_and(|(id, _)| *id == you);
                // A line that carried an image of hers has the image drawn
                // above it; what is left is her words, if any.
                if e.data.get("file").is_some() {
                    let words = platform::words(&said.message.text).to_owned();
                    said.message.text = words;
                    if said.message.text.is_empty() {
                        continue;
                    }
                }
                let item = Item::Said {
                    m: said.message,
                    seq: Some(e.seq),
                    from: said.from.map(|(_, name)| name),
                    mine,
                };
                items.push((e.at, e.seq, item));
            }
            continue;
        }
        // What she called in a look goes just before what it ended with,
        // and so above what she said after it.
        if look_end(e).is_some_and(|p| thread_key(p) == thread)
            && let Some(look) = looks.and_then(|l| l.ended_with(e.seq))
            && steps::look_calls(look).next().is_some()
        {
            let from = look_started(look, heard).unwrap_or(e.at);
            let item = Item::Look {
                look,
                from,
                to: e.at,
            };
            items.push((e.at, e.seq, item));
        }
        if matches!(e.kind.as_str(), "opened" | "looked_up")
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
    // What she opened or looked up in a look shown step by step is one of
    // its rows already, and is not shown again on a card of its own.
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = items
        .iter()
        .filter_map(|(_, _, item)| match item {
            Item::Look { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
        .collect();
    if let Some(from) = looks
        .and_then(PlaceLooks::running)
        .and_then(|l| look_started(l, heard))
    {
        spans.push((from, Utc::now()));
    }
    items.retain(|(at, _, item)| {
        !matches!(item, Item::Tool(_)) || !spans.iter().any(|(from, to)| from <= at && at <= to)
    });
    out_there_while_here(&mut items, looks.and_then(PlaceLooks::running).is_some());
    items.sort_by_key(|(at, seq, _)| (*at, *seq));
    between_turns(items)
}

/// Keep what she did out there, on other channels, only to the stretch of
/// time the thread itself covers: from its first line or card to its
/// last, and on to now while something here is still going. A console
/// session is a conversation, not her whole life: what she said in a
/// group days before it began, or her sleeping and waking, is not part of
/// it.
fn out_there_while_here(items: &mut Vec<(DateTime<Utc>, i64, Item<'_>)>, going: bool) {
    let own = |item: &Item<'_>| match item {
        Item::Did(did) => did.here,
        Item::Brain { .. } => false,
        _ => true,
    };
    let times = items
        .iter()
        .filter(|(_, _, i)| own(i))
        .map(|(at, _, _)| *at);
    let (Some(first), Some(last)) = (times.clone().min(), times.max()) else {
        items.retain(|(_, _, i)| !matches!(i, Item::Did(d) if !d.here));
        return;
    };
    let open = items
        .iter()
        .any(|(_, _, i)| matches!(i, Item::Task(t) if t.open()));
    let last = if going || open { Utc::now() } else { last };
    items.retain(|(at, _, i)| match i {
        Item::Did(d) if !d.here => first <= *at && *at <= last,
        _ => true,
    });
}

/// A change of brain, if `e` is one with a brain before it. The first
/// such entry in a journal names only the brain she was started on, and is
/// no change.
fn brain_change(e: &Entry) -> Option<Item<'static>> {
    if e.kind != "brain_changed" {
        return None;
    }
    let was = e.data.get("was")?.as_str()?.to_owned();
    let think = s(&e.data, "think").to_owned();
    (!think.is_empty() && think != was).then_some(Item::Brain { think, was })
}

/// The changes of brain in a thread, each where it falls between turns:
/// one before anything was said here is no turn's, and one straight after
/// another is a single change, from the first brain to the last.
fn between_turns<'a>(
    items: Vec<(DateTime<Utc>, i64, Item<'a>)>,
) -> Vec<(DateTime<Utc>, i64, Item<'a>)> {
    let mut out: Vec<(DateTime<Utc>, i64, Item<'a>)> = Vec::with_capacity(items.len());
    let mut said = false;
    for (at, seq, item) in items {
        if let Item::Brain { think, was } = item {
            if !said {
                continue;
            }
            match out.last_mut() {
                Some((
                    last_at,
                    last_seq,
                    Item::Brain {
                        think: last,
                        was: first,
                    },
                )) => {
                    *last = think;
                    (*last_at, *last_seq) = (at, seq);
                    if last == first {
                        out.pop();
                    }
                }
                _ => out.push((at, seq, Item::Brain { think, was })),
            }
            continue;
        }
        said |= matches!(item, Item::Said { .. });
        out.push((at, seq, item));
    }
    out
}

/// Which of your lines, by their place in `items`, were sent while she was
/// still at what came before and have not been taken up yet: sent after a
/// line of yours she has not yet answered, or after the look `looking`
/// still going on began. She takes them up when what she is at ends, at
/// her next look; from then on they are lines like any other.
fn sent_while_working(
    items: &[(DateTime<Utc>, i64, Item<'_>)],
    looking: Option<DateTime<Utc>>,
) -> Vec<usize> {
    let mut busy = false;
    let mut waiting = Vec::new();
    for (i, (at, _, item)) in items.iter().enumerate() {
        match item {
            Item::Said { m, mine: true, .. } if !m.hers => {
                if busy || looking.is_some_and(|since| since < *at) {
                    waiting.push(i);
                }
                busy = true;
            }
            // Whatever ends her wait on your last line, as
            // `waiting_since` reckons it, ends what she was at.
            Item::Said { m, .. } if m.hers => {
                busy = false;
                waiting.clear();
            }
            Item::Did(did) if did.answers() => {
                busy = false;
                waiting.clear();
            }
            Item::Task(_) | Item::Look { .. } | Item::Failed(_) | Item::Stopped(_) => {
                busy = false;
                waiting.clear();
            }
            _ => {}
        }
    }
    waiting
}

/// The place a journal entry ends a look in, if it can end one: what she
/// decided there, or her being stopped there, or a line taken back there.
fn look_end(e: &Entry) -> Option<&str> {
    match e.kind.as_str() {
        "decided" => Some(s(&e.data, "about")),
        "stopped" | "rewound" => Some(s(&e.data, "place")),
        _ => None,
    }
}

/// The stretches of the journal taken back in the thread `thread`, each
/// as `from..to`: from the line taken back up to, not including, the entry
/// that took it back.
fn taken_back(journal: &[Entry], thread: &str) -> Vec<(i64, i64)> {
    journal
        .iter()
        .filter(|e| e.kind == "rewound" && thread_key(s(&e.data, "place")) == thread)
        .filter_map(|e| Some((e.data.get("to")?.as_i64()?, e.seq)))
        .collect()
}

/// The newest entry in the thread of `place` a look can end with.
pub fn newest_look_end(journal: &[Entry], place: &str) -> Option<i64> {
    let thread = thread_key(place);
    journal
        .iter()
        .rev()
        .find(|e| look_end(e).is_some_and(|p| thread_key(p) == thread))
        .map(|e| e.seq)
}

/// When a look began, as near as the record says: when what she was
/// answering was said, if that was before her first call; otherwise her
/// first call. `None` for a look with no round that says when.
pub fn look_started(look: &Look, heard: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    let first: DateTime<Utc> = look.rounds.first()?.at.parse().ok()?;
    Some(heard.filter(|h| *h <= first).unwrap_or(first))
}

/// What the thread on screen would show of her steps: enough for the
/// desk to decide what to ask her daemon (see `follow.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The place, by the key her looks are kept under.
    pub place: String,
    /// She is thinking about what you said there.
    pub thinking: bool,
    /// The newest entry there a look can end with.
    pub newest_end: Option<i64>,
    /// The work and the parts whose cards are open, each with whether it
    /// is still going.
    pub works: Vec<(String, bool)>,
}

/// What the thread `view` shows of her steps.
pub fn wanted(view: &View<'_>) -> Wanted {
    let mut works = Vec::new();
    for t in place_tasks(view.journal, view.place) {
        // Every work's steps are read, folded or not: its line says what
        // it did in them. Its parts only once it is opened.
        works.push((t.id.clone(), t.open()));
        if !view.expanded.contains(&format!("task:{}", t.id)) {
            continue;
        }
        // Its parts as its steps list them, or else as the journal does.
        let listed = view.following.works.get(&t.id).map(|w| &w.parts);
        let parts: Vec<(String, bool)> = match listed {
            Some(parts) if !parts.is_empty() => parts
                .iter()
                .map(|p| (p.id.clone(), !p.state.ended()))
                .collect(),
            _ => t.parts.iter().map(|p| (p.id.clone(), p.open())).collect(),
        };
        works.extend(
            parts
                .into_iter()
                .filter(|(id, _)| view.expanded.contains(&format!("part:{id}"))),
        );
    }
    Wanted {
        place: thread_key(view.place),
        thinking: waiting_since(&items(view)).is_some(),
        newest_end: newest_look_end(view.journal, view.place),
        works,
    }
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
            Item::Did(did) if did.answers() => since = None,
            // A look that ended is her done with it, whether or not she
            // said anything.
            Item::Task(_) | Item::Look { .. } | Item::Failed(_) | Item::Stopped(_) => {
                since = None;
            }
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

/// Draw the thread `view` shows, the way it is to be read.
pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    match view.rendering {
        Rendering::Task => task_first::render(view, faces, cx),
    }
}
