//! The cards a thread draws besides what was said: a line for something
//! opened or looked up while answering, and a card for each piece of work
//! taken on, which opens to what it did, step by step, with what it said
//! along the way between, and its parts, each a card of its own.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::LoggedStep;
use gpui::{Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px};
use serde_json::Value;

use super::{
    State, Task, clock, s, span, steps,
    style::{CARD_RADIUS, MONO, S1, S2, S3, SMALL, SMALL_LINE, meta, well},
};
use crate::{aside, desk::Desk, follow::Following, fonts::Faces, model::Entry, theme, ui};

/// A quiet card: the frame tools and work are drawn in.
fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(theme::border())
}

/// Something opened or looked up while answering: one line, opened on a click.
pub(super) fn tool(
    faces: &Faces,
    e: &Entry,
    tz: Tz,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Stateful<Div> {
    let key = format!("tool:{}", e.seq);
    let open = expanded.contains(&key);
    let (verb, what, body) = if e.kind.as_str() == "looked_up" {
        ("Searched", s(&e.data, "query"), s(&e.data, "found"))
    } else {
        let url = s(&e.data, "url");
        let verb = if url.starts_with("http") {
            "Opened"
        } else {
            "Read"
        };
        (verb, url, s(&e.data, "text"))
    };
    let body: String = body.chars().take(1600).collect();
    let head = div()
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(34.))
        .px(px(S3))
        .child(ui::dot(theme::green().opacity(0.7)).size(px(6.)))
        .child(
            ui::text(faces, verb)
                .text_size(px(SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text2())
                .flex_none(),
        )
        .child(
            ui::mono(faces, what.to_owned())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(MONO))
                .text_color(theme::muted()),
        )
        .child(meta(faces, clock(e.at, tz)).flex_none())
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
            )
            .flex_none()
            .text_color(theme::faint()),
        );
    let toggle = key.clone();
    card()
        .id(SharedString::from(key))
        .cursor_pointer()
        .hover(|s| s.border_color(theme::border2()))
        .child(head)
        .when(open && !body.is_empty(), |d| {
            d.child(div().px(px(S2)).pb(px(S2)).child(well(faces).child(body)))
        })
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
}

/// A piece of work: a card that says where it stands at a glance, and
/// opens to what it did and its parts.
pub(super) fn task(
    faces: &Faces,
    t: &Task<'_>,
    tz: Tz,
    expanded: &HashSet<String>,
    following: &Following,
    cx: &mut Context<Desk>,
) -> Div {
    let key = format!("task:{}", t.id);
    let open = expanded.contains(&key);
    let state = t.state();
    let elapsed = (t.ended().unwrap_or_else(Utc::now) - t.started).num_seconds();
    let figures = format!("{} · {}", span(elapsed), t.figures());

    let title = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(aside::glyph(state))
        .child(
            ui::text(faces, t.brief.clone())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM),
        )
        .child(
            meta(faces, state.word())
                .text_color(state.color())
                .flex_none(),
        );
    let summary = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(S3))
        .pl(px(19.))
        .child(
            ui::text(faces, t.last_line())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(SMALL))
                .text_color(theme::muted()),
        )
        .child(meta(faces, figures).flex_none())
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
            )
            .flex_none()
            .text_color(theme::faint()),
        );
    let toggle = key.clone();
    let head = div()
        .id(SharedString::from(key))
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(14.))
        .py(px(S3))
        .cursor_pointer()
        .child(title)
        .child(summary)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));

    let mut frame = card()
        .bg(theme::panel())
        .when(t.open(), |d| d.border_color(theme::border2()))
        .child(head);
    if open {
        frame = frame.child(
            div()
                .flex()
                .flex_col()
                .gap(px(S2))
                .px(px(S3))
                .pt(px(S2))
                .pb(px(S3))
                .border_t_1()
                .border_color(theme::border())
                .child(along_the_way(faces, t, tz, following, expanded, cx))
                .children(parts(faces, t, following, expanded, cx)),
        );
    }
    frame
}

/// One thing in a work's card: a step it took, or something its journal
/// says it said or did.
pub enum Along<'a> {
    Step(&'a LoggedStep),
    Event(&'a Entry),
}

/// A work's steps and its journal's entries about it, in the order they
/// happened. A step that does not say when it was taken goes with the one
/// before it, or at the start.
pub fn along<'a>(
    started: DateTime<Utc>,
    events: &[&'a Entry],
    steps: &'a [LoggedStep],
) -> Vec<Along<'a>> {
    let mut at = started;
    let mut all: Vec<(DateTime<Utc>, Along<'a>)> = Vec::new();
    for step in steps {
        if let Some(t) = step.at.as_deref().and_then(|a| a.parse().ok()) {
            at = t;
        }
        all.push((at, Along::Step(step)));
    }
    all.extend(events.iter().map(|e| (e.at, Along::Event(e))));
    all.sort_by_key(|(at, _)| *at);
    all.into_iter().map(|(_, a)| a).collect()
}

/// What a piece of work did and said, interleaved: its steps in rails,
/// broken by what it told, asked and concluded, each on a line of its own.
fn along_the_way(
    faces: &Faces,
    t: &Task<'_>,
    tz: Tz,
    following: &Following,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut column = div().flex().flex_col().gap(px(S1));
    let read = following.works.get(&t.id).filter(|r| r.read());
    let held: &[LoggedStep] = read.map_or(&[], |r| r.steps.as_slice());
    let (shown, earlier) = steps::earlier(faces, &t.id, held.len(), expanded, cx);
    if let Some(earlier) = earlier {
        column = column.child(steps::rail().child(earlier));
    }
    let mut rail: Option<Div> = None;
    for item in along(t.started, &t.events, &held[held.len() - shown..]) {
        match item {
            Along::Step(step) => {
                let r = rail.take().unwrap_or_else(steps::rail);
                rail = Some(r.child(steps::step(faces, &t.id, step, expanded, cx)));
            }
            Along::Event(e) => {
                column = column.children(rail.take());
                column = column.child(milestone(faces, e, tz));
            }
        }
    }
    let running = t.state() == State::Running;
    if read.is_none() {
        let r = rail.take().unwrap_or_else(steps::rail);
        rail = Some(r.child(steps::quiet(faces, "Reading its steps…")));
    } else if running {
        let r = rail.take().unwrap_or_else(steps::rail);
        rail = Some(r.child(steps::live_row(faces, "Working")));
    }
    column.children(rail)
}

/// Something a piece of work's journal says of it: a sitting's summary,
/// what it told or asked, how it ended.
fn milestone(faces: &Faces, e: &Entry, tz: Tz) -> Div {
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
        "concluded" => (
            s(&e.data, "outcome").replace('_', " "),
            s(&e.data, "summary"),
        ),
        "asked" => ("asked".to_owned(), s(&e.data, "question")),
        other => (other.to_owned(), s(&e.data, "text")),
    };
    div()
        .flex()
        .gap(px(10.))
        .items_baseline()
        .py(px(S1))
        .px(px(6.))
        .child(meta(faces, clock(e.at, tz)).w(px(38.)).flex_none())
        .child(
            meta(faces, tag)
                .text_color(theme::kind_color(&e.kind))
                .w(px(76.))
                .flex_none(),
        )
        .child(
            ui::text(faces, text.to_owned())
                .flex_1()
                .min_w(px(0.))
                .text_size(px(SMALL))
                .line_height(px(SMALL_LINE))
                .text_color(theme::text2()),
        )
}

/// The parts a piece of work handed out, each a card of its own: as its
/// steps list them, where they have been read, or else as the journal
/// has them.
fn parts(
    faces: &Faces,
    t: &Task<'_>,
    following: &Following,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Option<Div> {
    let listed = following
        .works
        .get(&t.id)
        .map(|w| w.parts.as_slice())
        .unwrap_or_default();
    let known: Vec<(String, String, State)> = if listed.is_empty() {
        t.parts
            .iter()
            .map(|p| (p.id.clone(), p.brief.clone(), p.state()))
            .collect()
    } else {
        listed
            .iter()
            .map(|p| (p.id.clone(), p.brief.clone(), aside::state_of(p.state)))
            .collect()
    };
    if known.is_empty() {
        return None;
    }
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .pt(px(S1))
        .child(ui::eyebrow(faces, "parts").text_size(px(10.5)).px(px(6.)));
    for (id, brief, state) in known {
        let last = t.parts.iter().find(|p| p.id == id).map(Task::last_line);
        column = column.child(steps::part(
            faces, &id, &brief, state, last, following, expanded, cx,
        ));
    }
    Some(column)
}
