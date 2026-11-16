//! The cards a thread draws besides what was said: a line for something
//! opened or looked up while answering, and a card for each piece of work
//! taken on, which opens to everything it said along the way and its parts.

use std::collections::HashSet;

use chrono::Utc;
use chrono_tz::Tz;
use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px};
use serde_json::Value;

use super::{Task, clock, meta, s, span};
use crate::{desk::Desk, fonts::Faces, model::Entry, theme, ui};

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
pub(super) fn task(
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
