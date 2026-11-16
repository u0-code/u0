//! The thread of one session, in one stream by time: what passed between
//! you and the solver there, what it opened or looked up while answering,
//! and a card for each piece of work it took on, with its parts under it.
//! All of it is read from the solver's journal, so what shows here is what
//! is on record; what was said a moment ago and not yet written follows at
//! the end.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use gpui::{AnyElement, Context, Div, Hsla, SharedString, Stateful, div, prelude::*, px};
use serde_json::Value;

use crate::{
    desk::Desk,
    fonts::Faces,
    link, markdown,
    model::{Entry, Message},
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
    fn color(self) -> Hsla {
        match self {
            State::Running => theme::accent(),
            State::Waiting => theme::purple(),
            State::Finished => theme::green(),
            State::Blocked => theme::coral(),
            State::Dropped => theme::muted(),
        }
    }

    fn word(self) -> &'static str {
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

/// Which session a piece of work belongs to: the place its `undertook`
/// names, when it names one on the desk.
fn task_place(journal: &[Entry], id: &str) -> Option<String> {
    journal
        .iter()
        .find(|e| e.kind == "undertook" && s(&e.data, "id") == id)
        .and_then(|e| e.data.get("place").and_then(Value::as_str))
        .map(str::to_owned)
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

/// Everything the thread needs to draw one session.
pub struct View<'a> {
    pub journal: &'a [Entry],
    pub pending: &'a [Message],
    pub session: &'a str,
    pub name: String,
    pub who: String,
    pub tz: Tz,
    pub typing: bool,
    pub expanded: &'a HashSet<String>,
}

pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let place = place_key(view.session);
    let marks = directed(view.journal);
    let mut items: Vec<(DateTime<Utc>, i64, Item<'_>)> = Vec::new();
    for e in view.journal {
        if let Some(m) = desk_message(e) {
            if m.place == view.session {
                items.push((
                    e.at,
                    e.seq,
                    Item::Said {
                        m,
                        seq: Some(e.seq),
                    },
                ));
            }
        } else if matches!(e.kind.as_str(), "opened" | "looked_up")
            && e.data.get("place").and_then(Value::as_str) == Some(place.as_str())
        {
            items.push((e.at, e.seq, Item::Tool(e)));
        }
    }
    for t in tasks(view.journal) {
        if task_place(view.journal, &t.id).as_deref() == Some(place.as_str()) {
            items.push((t.started, 0, Item::Task(t)));
        }
    }
    for m in view.pending.iter().filter(|m| m.place == view.session) {
        items.push((
            m.at,
            i64::MAX,
            Item::Said {
                m: m.clone(),
                seq: None,
            },
        ));
    }
    items.sort_by_key(|(at, seq, _)| (*at, *seq));

    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(20.))
        .w_full()
        .max_w(px(COLUMN))
        .mx_auto();
    if items.is_empty() {
        column = column.child(
            ui::serif(faces, "What needs doing?")
                .text_size(px(34.))
                .line_height(px(40.))
                .pt(px(48.)),
        );
    }

    // When you spoke last and nothing has answered since, the solver is
    // working on it.
    let mut waiting_since: Option<DateTime<Utc>> = None;
    let mut last_yours: Option<DateTime<Utc>> = None;
    let mut day: Option<String> = None;
    for (at, _, item) in items {
        let d = at.with_timezone(&view.tz).format("%A %-d %B").to_string();
        if day.as_ref() != Some(&d) {
            day = Some(d.clone());
            column = column.child(ui::eyebrow(faces, d).text_center().py(px(4.)));
        }
        let el = match item {
            Item::Said { m, seq } => {
                if m.hers {
                    let took = last_yours
                        .filter(|y| *y <= m.at)
                        .map(|y| span((m.at - y).num_seconds()))
                        .map(|t| format!(" · {t}"))
                        .unwrap_or_default();
                    waiting_since = None;
                    hers(faces, &m, &view.name, view.tz, &took).into_any_element()
                } else {
                    last_yours = Some(m.at);
                    waiting_since = Some(m.at);
                    let directed = seq.and_then(|x| marks.get(&x).copied());
                    yours(faces, &m, &view.who, view.tz, directed).into_any_element()
                }
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
        let since = waiting_since.map_or_else(String::new, |w| {
            format!(" · {}", span((Utc::now() - w).num_seconds()))
        });
        column = column.child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(
                    div()
                        .w(px(20.))
                        .flex()
                        .justify_center()
                        .child(ui::dot(theme::accent()).size(px(7.))),
                )
                .child(
                    ui::mono(
                        faces,
                        format!(
                            "{}{since}",
                            if view.typing { "writing" } else { "thinking" }
                        ),
                    )
                    .text_size(px(12.))
                    .text_color(theme::muted()),
                ),
        );
    }

    div()
        .id("thread")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(32.))
        .py(px(24.))
        .child(column)
        .into_any_element()
}

/// The gutter every line in the thread starts with: a glyph, centred.
fn gutter(child: impl IntoElement) -> Div {
    div()
        .w(px(20.))
        .h(px(23.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(child)
}

fn yours(faces: &Faces, m: &Message, who: &str, tz: Tz, directed: Option<i64>) -> Div {
    let mut meta = format!("{who} · {}", clock(m.at, tz));
    if let Some(seq) = directed {
        meta = format!("{meta} · directed · #{seq}");
    }
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .gap(px(14.))
                .items_start()
                .child(gutter(
                    ui::mono(faces, if directed.is_some() { "!" } else { "›" })
                        .text_size(px(15.))
                        .text_color(if directed.is_some() {
                            theme::purple()
                        } else {
                            theme::muted()
                        }),
                ))
                .child(
                    ui::text(faces, m.text.clone())
                        .text_color(theme::text2())
                        .line_height(px(23.)),
                ),
        )
        .child(
            ui::mono(faces, meta)
                .text_size(px(11.))
                .text_color(theme::faint())
                .ml(px(34.)),
        )
}

fn hers(faces: &Faces, m: &Message, name: &str, tz: Tz, took: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .gap(px(14.))
                .items_start()
                .child(gutter(ui::icon("ghost", 14.).text_color(theme::text2())))
                .child(markdown::render(faces, &m.text).flex_1()),
        )
        .child(
            ui::mono(faces, format!("{name} · {}{took}", clock(m.at, tz)))
                .text_size(px(11.))
                .text_color(theme::faint())
                .ml(px(34.)),
        )
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
    let toggle = key.clone();
    div()
        .id(SharedString::from(key))
        .flex()
        .flex_col()
        .gap(px(6.))
        .cursor_pointer()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(gutter(ui::dot(theme::green()).size(px(6.))))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            ui::text(faces, verb)
                                .text_size(px(13.))
                                .font_weight(gpui::FontWeight::MEDIUM),
                        )
                        .child(ui::one_line(
                            ui::mono(faces, what.to_owned())
                                .text_size(px(12.))
                                .text_color(theme::muted()),
                        )),
                )
                .child(
                    ui::mono(
                        faces,
                        format!("{} {}", clock(e.at, tz), if open { "⌃" } else { "⌄" }),
                    )
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .flex_none(),
                ),
        )
        .when(open && !body.is_empty(), |d| {
            d.child(
                div()
                    .ml(px(34.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(theme::well())
                    .border_1()
                    .border_color(theme::border())
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
    let running_parts = t
        .parts
        .iter()
        .filter(|p| matches!(p.state(), State::Running))
        .count();
    let mut meta = format!("{} · {} steps", span(elapsed), t.steps());
    if !t.parts.is_empty() {
        meta = format!("{meta} · {} parts", t.parts.len());
        if running_parts > 0 {
            meta = format!("{meta}, {running_parts} running");
        }
    }

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
        .child(
            ui::mono(faces, format!("{meta} {}", if open { "⌃" } else { "⌄" }))
                .text_size(px(11.))
                .text_color(theme::faint())
                .flex_none(),
        );

    let mut card = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(10.))
        .bg(theme::panel())
        .border_1()
        .border_color(if matches!(state, State::Running | State::Waiting) {
            theme::border2()
        } else {
            theme::border()
        })
        .child(head)
        .child(summary);

    if open {
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
                    .child(
                        ui::mono(faces, clock(e.at, tz))
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .w(px(38.))
                            .flex_none(),
                    )
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
                let ps = p.state();
                let n = p.id.rsplit('/').next().unwrap_or_default().to_owned();
                parts = parts.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .py(px(3.))
                        .child(
                            ui::mono(faces, "└")
                                .text_size(px(11.))
                                .text_color(theme::ghost_line()),
                        )
                        .child(ui::dot(ps.color()).size(px(6.)))
                        .child(
                            ui::mono(faces, format!("#{n}"))
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .flex_none(),
                        )
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
                        ),
                );
            }
            timeline = timeline.child(parts);
        }
        card = card.child(timeline);
    }

    let toggle = key.clone();
    div()
        .id(SharedString::from(key))
        .flex()
        .gap(px(14.))
        .items_start()
        .cursor_pointer()
        .child(gutter(
            ui::mono(faces, "⚙")
                .text_size(px(12.))
                .text_color(theme::muted()),
        ))
        .child(card.flex_1().min_w(px(0.)))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
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
}
