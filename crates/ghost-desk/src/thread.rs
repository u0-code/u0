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
    model::{Entry, Message},
    theme, ui,
};

/// How wide the thread and the composer may grow.
pub const COLUMN: f32 = 760.;

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
    let thread = div()
        .id("thread")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(32.))
        .py(px(24.))
        .gap(px(18.));
    let Some(c) = desk.current() else {
        return thread.into_any_element();
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

    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(18.))
        .w_full()
        .max_w(px(COLUMN))
        .mx_auto();

    if items.is_empty() {
        column = column.child(
            ui::serif(
                faces,
                if c.seed.local {
                    "What needs doing?"
                } else {
                    "Say something."
                },
            )
            .text_size(px(34.))
            .line_height(px(40.))
            .pt(px(48.)),
        );
    }

    let mut day: Option<String> = None;
    let mut last_yours: Option<DateTime<Utc>> = None;
    for (at, _, item) in items {
        let d = at.with_timezone(&tz).format("%A %-d %B").to_string();
        if day.as_ref() != Some(&d) {
            day = Some(d.clone());
            column = column.child(ui::eyebrow(faces, d).text_center().py(px(6.)));
        }
        column = column.child(match item {
            Item::Said { m, seq } => {
                let el = if m.hers {
                    let took = last_yours
                        .filter(|y| *y <= m.at)
                        .map(|y| (m.at - y).num_minutes())
                        .filter(|mins| *mins > 0)
                        .map(|mins| format!(" · {mins} min"))
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
            Item::Did(e) => did(faces, e, tz).into_any_element(),
        });
    }
    if c.typing {
        column = column.child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .w(px(20.))
                        .flex()
                        .justify_center()
                        .child(ui::dot(theme::accent()).size(px(6.))),
                )
                .child(ui::mono(faces, format!("{name} is typing")).text_size(px(11.5))),
        );
    }
    thread.child(column).into_any_element()
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
    let mut meta = format!("{who} · {when}");
    if let Some(seq) = directed {
        meta = format!("{meta} · directed · #{seq}");
    } else if let Some(at) = read {
        meta = format!("{meta} · read {}", clock(at, tz));
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
                .child(ui::text(faces, m.text.clone()).line_height(px(23.))),
        )
        .child(
            ui::mono(faces, format!("{name} · {when}{took}"))
                .text_size(px(11.))
                .text_color(theme::faint())
                .ml(px(34.)),
        )
}

/// A step of her work: taken on, worked, reported, asked, concluded.
fn did(faces: &Faces, e: &Entry, tz: Tz) -> Div {
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
    let tag = match e.kind.as_str() {
        "undertook" => "took on".to_owned(),
        "worked" => format!(
            "{} steps",
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
        .items_baseline()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(
            ui::mono(faces, tag)
                .text_size(px(11.))
                .text_color(theme::kind_color(&e.kind))
                .w(px(72.))
                .flex_none(),
        )
        .child(
            ui::text(faces, text)
                .flex_1()
                .min_w(px(0.))
                .text_size(px(13.5))
                .text_color(theme::text2())
                .line_height(px(20.)),
        )
        .child(
            ui::mono(faces, when)
                .text_size(px(10.5))
                .text_color(theme::faint())
                .flex_none(),
        )
}
