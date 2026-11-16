//! The thread: what passed between you and her on the desk, and what she
//! did about it, in one stream by time. The conversation is read from her
//! journal, so what shows here is what is on record; what was said a moment
//! ago and not yet written follows at the end.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use gpui::{AnyElement, Div, div, prelude::*, px};
use serde_json::Value;

use crate::{
    desk::{Character, Desk},
    fonts::Faces,
    link,
    model::{Entry, LinkState, Message},
    theme, ui,
};

/// A time of day in her clock.
pub fn clock(at: DateTime<Utc>, tz: Tz) -> String {
    at.with_timezone(&tz).format("%H:%M").to_string()
}

/// The message a journal entry is, if it is one said on the desk.
pub fn desk_message(e: &Entry) -> Option<Message> {
    let d = &e.data;
    if d.get("channel").and_then(Value::as_str) != Some(link::CHANNEL) {
        return None;
    }
    let hers = match e.kind.as_str() {
        "heard" => false,
        "said" => true,
        _ => return None,
    };
    Some(Message {
        hers,
        text: d.get("text")?.as_str()?.to_owned(),
        at: e.at,
    })
}

/// One thing in the stream.
enum Item<'a> {
    Said {
        m: Message,
        /// The seq it is on record as, if it is.
        seq: Option<i64>,
    },
    Did(&'a Entry),
}

/// What the journal says around the conversation: which of your messages
/// were directives, and when she read them.
struct Marks {
    /// Heard seq → the `directed` entry's seq.
    directed: HashMap<i64, i64>,
    /// Desk `read` entries: (at, upto).
    reads: Vec<(DateTime<Utc>, i64)>,
}

impl Marks {
    fn of(c: &Character) -> Self {
        let mut directed = HashMap::new();
        let mut reads = Vec::new();
        for e in &c.journal {
            match e.kind.as_str() {
                "directed" => {
                    if let Some(m) = e.data.get("message").and_then(Value::as_i64) {
                        directed.insert(m, e.seq);
                    }
                }
                "read" if e.data.get("channel").and_then(Value::as_str) == Some(link::CHANNEL) => {
                    if let Some(upto) = e.data.get("upto").and_then(Value::as_i64) {
                        reads.push((e.at, upto));
                    }
                }
                _ => {}
            }
        }
        Self { directed, reads }
    }

    fn read_at(&self, seq: i64) -> Option<DateTime<Utc>> {
        self.reads
            .iter()
            .find(|(_, upto)| *upto >= seq)
            .map(|(at, _)| *at)
    }
}

pub fn render(desk: &Desk, faces: &Faces) -> AnyElement {
    let mut thread = div()
        .id("thread")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(32.))
        .py(px(20.))
        .gap(px(18.));
    let Some(c) = desk.current() else {
        return thread
            .child(empty(
                faces,
                "Nobody here yet.",
                "Give the desk a daemon's address, bottom left. It asks what the daemon runs and follows every character.",
            ))
            .into_any_element();
    };
    let tz = c.tz();
    let name = c.name();
    let who = desk.who().to_owned();
    let marks = Marks::of(c);

    let mut items: Vec<(DateTime<Utc>, i64, Item<'_>)> = Vec::new();
    for e in &c.journal {
        if let Some(m) = desk_message(e) {
            items.push((
                e.at,
                e.seq,
                Item::Said {
                    m,
                    seq: Some(e.seq),
                },
            ));
        } else if matches!(
            e.kind.as_str(),
            "undertook" | "worked" | "reported" | "asked" | "added" | "concluded"
        ) {
            items.push((e.at, e.seq, Item::Did(e)));
        }
    }
    for m in &c.pending {
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

    if items.is_empty() {
        let (title, line) = match &c.link {
            LinkState::Connected if c.seed.local => (
                "What needs doing?",
                "Say it in one line. She takes it on, works in the background and reports here as she goes.",
            ),
            LinkState::Connected => (
                "Nothing said yet.",
                "Say something. She reads it when she gets to it, and answers if she has something to say.",
            ),
            LinkState::Connecting => ("Reaching her daemon.", "One moment."),
            LinkState::Lost(_) => (
                "Her daemon is away.",
                "Start one with `ghostd run <her directory>`; the desk keeps trying.",
            ),
        };
        thread = thread.child(empty(faces, title, line));
    }

    let mut day: Option<String> = None;
    let mut last_yours: Option<DateTime<Utc>> = None;
    for (at, _, item) in items {
        let d = at.with_timezone(&tz).format("%A %-d %B").to_string();
        if day.as_ref() != Some(&d) {
            day = Some(d.clone());
            thread = thread.child(ui::eyebrow(faces, d).text_center().py(px(6.)));
        }
        thread = thread.child(match item {
            Item::Said { m, seq } => {
                let el = if m.hers {
                    let took = last_yours
                        .filter(|y| *y <= m.at)
                        .map(|y| (m.at - y).num_minutes())
                        .filter(|mins| *mins > 0)
                        .map(|mins| format!(" · {mins} min, in her own time"))
                        .unwrap_or_default();
                    hers(faces, &m, &name, tz, &took)
                } else {
                    last_yours = Some(m.at);
                    let directed = seq.and_then(|s| marks.directed.get(&s).copied());
                    let read = seq.and_then(|s| marks.read_at(s));
                    yours(faces, &m, &who, tz, directed, read)
                };
                el.into_any_element()
            }
            Item::Did(e) => did(faces, e, &name, tz).into_any_element(),
        });
    }
    thread.into_any_element()
}

/// Where things stand right now: the line above the composer.
pub fn standing(desk: &Desk, faces: &Faces) -> Option<Div> {
    let c = desk.current()?;
    let name = c.name();
    let (color, text) = if c.typing {
        (theme::accent(), format!("{name} is typing"))
    } else if !c.desk_online && c.connected() {
        (
            theme::ghost_line(),
            "the desk is not yet a place she can be reached; what you say is held".to_owned(),
        )
    } else {
        let s = c.status.as_ref()?;
        if s.awake() {
            let doing = s.doing.as_ref().map_or_else(
                || "she is awake".to_owned(),
                |d| format!("she is {}", d.what),
            );
            (
                theme::accent(),
                format!("{doing} · a message lands as unread, and she gets to it"),
            )
        } else {
            (
                theme::ghost_line(),
                "she is asleep · what you say waits for morning".to_owned(),
            )
        }
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(32.))
            .pb(px(10.))
            .child(
                div()
                    .w(px(20.))
                    .flex()
                    .justify_center()
                    .child(ui::dot(color).size(px(7.))),
            )
            .child(ui::mono(faces, text).text_size(px(12.))),
    )
}

fn empty(faces: &Faces, title: &str, line: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .max_w(px(620.))
        .pt(px(40.))
        .child(
            ui::serif(faces, title.to_owned())
                .text_size(px(34.))
                .line_height(px(40.)),
        )
        .child(
            ui::text(faces, line.to_owned())
                .text_size(px(14.))
                .text_color(theme::text2())
                .line_height(px(21.)),
        )
}

fn yours(
    faces: &Faces,
    m: &Message,
    who: &str,
    tz: Tz,
    directed: Option<i64>,
    read: Option<DateTime<Utc>>,
) -> Div {
    let when = clock(m.at, tz);
    let mut meta = div().flex().items_baseline().gap(px(6.)).ml(px(34.)).child(
        ui::mono(faces, format!("{who} · {when}"))
            .text_size(px(11.))
            .text_color(theme::faint()),
    );
    if let Some(seq) = directed {
        meta = meta
            .child(
                ui::mono(faces, "· directed")
                    .text_size(px(11.))
                    .text_color(theme::purple()),
            )
            .child(
                ui::mono(faces, format!("· on record as event {seq}"))
                    .text_size(px(11.))
                    .text_color(theme::faint()),
            );
    } else if let Some(at) = read {
        meta = meta.child(
            ui::mono(faces, format!("· she read it at {}", clock(at, tz)))
                .text_size(px(11.))
                .text_color(theme::faint()),
        );
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
                .child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .text_center()
                        .font_family(faces.mono.clone())
                        .text_size(px(15.))
                        .text_color(if directed.is_some() {
                            theme::purple()
                        } else {
                            theme::muted()
                        })
                        .child(if directed.is_some() { "!" } else { "›" }),
                )
                .child(
                    ui::text(faces, m.text.clone())
                        .text_color(theme::text2())
                        .line_height(px(23.))
                        .max_w(px(680.)),
                ),
        )
        .child(meta)
}

fn hers(faces: &Faces, m: &Message, name: &str, tz: Tz, took: &str) -> Div {
    let when = clock(m.at, tz);
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .gap(px(14.))
                .items_start()
                .child(ui::initial(faces, name, 20.))
                .child(
                    ui::text(faces, m.text.clone())
                        .line_height(px(23.))
                        .max_w(px(680.)),
                ),
        )
        .child(
            ui::mono(faces, format!("{name} · {when}{took}"))
                .text_size(px(11.))
                .text_color(theme::faint())
                .ml(px(34.)),
        )
}

/// A step of her work: taken on, worked, reported, asked, concluded.
fn did(faces: &Faces, e: &Entry, name: &str, tz: Tz) -> Div {
    let when = clock(e.at, tz);
    let d = &e.data;
    let text = d
        .get("text")
        .or_else(|| d.get("brief"))
        .or_else(|| d.get("summary"))
        .or_else(|| d.get("question"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let id = d.get("id").and_then(Value::as_str).unwrap_or_default();
    let tag = match e.kind.as_str() {
        "undertook" => "took on".to_owned(),
        "worked" => format!(
            "worked · {} steps",
            d.get("steps").and_then(Value::as_i64).unwrap_or_default()
        ),
        "concluded" => d
            .get("outcome")
            .and_then(Value::as_str)
            .unwrap_or("concluded")
            .to_owned(),
        other => other.to_owned(),
    };
    div()
        .ml(px(34.))
        .flex()
        .gap(px(12.))
        .items_start()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .max_w(px(680.))
        .child(
            ui::mono(faces, tag)
                .text_size(px(11.))
                .text_color(theme::kind_color(&e.kind))
                .w(px(110.))
                .flex_none(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w(px(0.))
                .child(
                    ui::text(faces, text)
                        .text_size(px(13.5))
                        .text_color(theme::text2())
                        .line_height(px(20.)),
                )
                .child(
                    ui::mono(faces, format!("{name} · {when} · [{id}]"))
                        .text_size(px(10.5))
                        .text_color(theme::faint()),
                ),
        )
}
