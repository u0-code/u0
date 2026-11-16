//! The thread drawn task-first, the way a coding agent's app shows a
//! session: what you said in a bubble on the right, what she said as
//! prose in the reading column with her mark beside it, under each a line
//! of small print with when it was and what can be done with it, and above
//! each answer what she did to get to it, folded into one line that says
//! what was done (drawn in `steps.rs`). Work taken on has a card of its
//! own (`cards.rs`). While she thinks, what she does shows as she does it.
//! Everything of hers starts after the gutter her mark sits in.

use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use gpui::{AnyElement, Context, Div, SharedString, Stateful, div, prelude::*, px, relative};

use super::{
    Item, STARTERS, View, ago,
    cards::{task, tool},
    clock, could_not, directed, items, look_started, span, steps,
    style::{
        BODY, BODY_LINE, BUBBLE_RADIUS, COLUMN, GUTTER, S1, S2, S3, S4, S6, SMALL, SMALL_LINE,
        TURN, meta,
    },
    thread_key, waiting_since,
};
use crate::{desk::Desk, fonts::Faces, markdown, model::Message, theme, ui};

/// What came before an item, for the space above it: a look's steps sit
/// close over the answer they led to; turns stand apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Before {
    Nothing,
    Look,
    Other,
}

pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let items = items(view);
    if items.is_empty() {
        return empty(view, faces, cx);
    }
    let waiting = waiting_since(&items);
    // At a console every line of yours is an operator's, so none is marked
    // as one; on a platform the marked lines are worth telling apart.
    let marks = if thread_key(view.place).starts_with("console:") {
        HashMap::new()
    } else {
        directed(view.journal)
    };
    let mut column = div().flex().flex_col().w_full().max_w(px(COLUMN)).mx_auto();

    // What you said last, and when: a failure offers to say it again.
    let mut last_yours: Option<DateTime<Utc>> = None;
    let mut last_line: Option<String> = None;
    // What was last said to her by anyone, for timing a look still going.
    let mut heard: Option<DateTime<Utc>> = None;
    let mut before = Before::Nothing;
    for (at, _, item) in items {
        let (el, now) = match item {
            Item::Said { m, .. } if m.hers => {
                // Her reply says how long it took, unless the steps above
                // it already do.
                let took = last_yours
                    .filter(|y| *y <= m.at && before != Before::Look)
                    .map(|y| span((m.at - y).num_seconds()));
                let el = hers(faces, &m, view.tz, took, cx).into_any_element();
                (el, Before::Other)
            }
            Item::Said { m, seq, from, mine } => {
                if mine {
                    last_yours = Some(m.at);
                    last_line = Some(m.text.clone());
                }
                heard = Some(m.at);
                let directed = seq.and_then(|x| marks.get(&x).copied());
                let who = from.as_deref().unwrap_or(&view.who);
                let el = yours(faces, &m, who, view.tz, directed, mine, cx).into_any_element();
                (el, Before::Other)
            }
            Item::Tool(e) => (
                after_gutter(tool(faces, e, view.tz, view.expanded, cx)),
                Before::Other,
            ),
            Item::Look { look, from, to } => {
                let el = steps::look(faces, look, from, to, view.expanded, cx);
                (after_gutter(el), Before::Look)
            }
            Item::Task(t) => {
                let el = task(faces, &t, view.tz, view.expanded, view.following, cx);
                (after_gutter(el), Before::Other)
            }
            Item::Failed(why) => {
                let el = failed(faces, &why, at, view.tz, last_line.clone(), cx);
                (after_gutter(el), Before::Other)
            }
            Item::Stopped(stopped) => {
                let word = if stopped {
                    "Stopped"
                } else {
                    "Nothing was running to stop"
                };
                let el = note(faces, format!("{word} · {}", clock(at, view.tz)));
                (after_gutter(el), Before::Other)
            }
        };
        let gap = match (before, now) {
            (Before::Nothing, _) => 0.,
            (Before::Look, _) => S3,
            _ => TURN,
        };
        column = column.child(div().mt(px(gap)).child(el));
        before = now;
    }

    // What she is doing about it now: the look going on, step by step, or
    // until she has called anything, a line saying she is thinking.
    let live = view
        .following
        .places
        .get(&thread_key(view.place))
        .and_then(crate::follow::PlaceLooks::running)
        .filter(|l| steps::look_calls(l).next().is_some());
    if let Some(look) = live {
        let from = waiting
            .or_else(|| look_started(look, heard))
            .unwrap_or_else(Utc::now);
        let el = steps::look(faces, look, from, Utc::now(), view.expanded, cx);
        column = column.child(div().mt(px(TURN)).child(after_gutter(el)));
    } else if let Some(since) = waiting {
        let el = working(faces, view, since).into_any_element();
        column = column.child(div().mt(px(TURN)).child(after_gutter(el)));
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
        .pb(px(S6))
        .child(column)
        .into_any_element()
}

/// Something of hers, set after the gutter her mark sits in, so that it
/// lines up with her words.
fn after_gutter(el: impl IntoElement) -> AnyElement {
    div().ml(px(GUTTER)).child(el).into_any_element()
}

/// A small round button under a message: an icon that does one thing.
fn small_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(ui::icon(icon, 13.).text_color(theme::muted()))
}

/// Her mark, beside her words: the ghost on a small round tile.
fn avatar() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded_full()
        .bg(theme::panel2())
        .child(ui::icon("ghost", 13.).text_color(theme::text2()))
}

/// How long one breath of something going on takes.
const BREATH: Duration = Duration::from_millis(1800);

/// How visible something going on is, `t` into its breathing: from full
/// down to a third and back, smoothly, once every [`BREATH`].
pub fn breath(t: Duration) -> f32 {
    let phase = (t.as_millis() % BREATH.as_millis()) as f32 / BREATH.as_millis() as f32;
    let wave = f32::midpoint(1., (phase * std::f32::consts::TAU).cos());
    0.35 + 0.65 * wave
}

/// A mark that breathes, slowly: something going on now. It is drawn at
/// the point of its breath the clock is at; the desk draws again a few
/// times a second while something on screen is going on (`Desk::pump`),
/// which is enough for a breath this slow, where an animation would draw
/// the whole window every frame for as long as she works.
pub(super) fn breathing(dot: Div) -> Div {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    dot.opacity(breath(EPOCH.get_or_init(std::time::Instant::now).elapsed()))
}

/// A small, quiet line in the thread: something that happened rather than
/// something said.
fn note(faces: &Faces, text: String) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(ui::dot(theme::faint()).size(px(6.)))
        .child(
            meta(faces, text)
                .text_size(px(11.5))
                .text_color(theme::muted()),
        )
}

/// Where her answer would be, when she could not think: why, calmly, and
/// a way to say your last line again.
fn failed(
    faces: &Faces,
    why: &str,
    at: DateTime<Utc>,
    tz: chrono_tz::Tz,
    again: Option<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut line = div()
        .flex()
        .items_start()
        .gap(px(10.))
        .child(div().pt(px(7.)).child(ui::dot(theme::faint()).size(px(6.))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(S1))
                .flex_1()
                .min_w(px(0.))
                .child(
                    ui::text(faces, could_not(why))
                        .text_size(px(13.))
                        .line_height(px(20.))
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
                .px(px(S2))
                .rounded(px(6.))
                .border_1()
                .border_color(theme::border2())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon("retry", 12.).text_color(theme::muted()))
                .child(
                    ui::text(faces, "Retry")
                        .text_size(px(SMALL))
                        .text_color(theme::text2()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.say_again(text.clone(), cx))),
        );
    }
    line
}

/// The line under the last thing you said while nothing has answered and
/// she has called nothing yet: thinking, for how long; or, with no daemon
/// to hear it, that your line is waiting to be delivered.
fn working(faces: &Faces, view: &View<'_>, since: DateTime<Utc>) -> Div {
    let (color, word) = if view.connected {
        (theme::accent(), "Thinking")
    } else {
        (theme::muted(), "Waiting for the daemon")
    };
    let since = span((Utc::now() - since).num_seconds());
    let dot = ui::dot(color).size(px(7.));
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(24.))
        .child(if view.connected { breathing(dot) } else { dot })
        .child(
            ui::text(faces, word)
                .text_size(px(SMALL))
                .text_color(theme::text2()),
        )
        .child(meta(faces, since))
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
                .text_size(px(BODY))
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
                .text_size(px(BODY))
                .line_height(px(BODY_LINE))
                .text_color(theme::muted())
                .text_center()
                .mt(px(10.)),
        );
    // The starters all need the machine; without it they would only be
    // promises.
    if abilities.local() {
        let mut starters = div().flex().flex_col().gap(px(S2)).w_full().mt(px(28.));
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
        .gap(px(S3))
        .h(px(42.))
        .px(px(S4))
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

/// What you said: a bubble on the right, the way a chat has it, and under
/// it when it was, with a way to say it again and to copy it.
fn yours(
    faces: &Faces,
    m: &Message,
    who: &str,
    tz: chrono_tz::Tz,
    directed: Option<i64>,
    mine: bool,
    cx: &mut Context<Desk>,
) -> Div {
    let stamp = m.at.timestamp_millis();
    let mut line = ago(m.at, tz);
    if !mine {
        line = format!("{who} · {line}");
    }
    if let Some(seq) = directed {
        line = format!("{line} · directed · #{seq}");
    }
    let (again, copy) = (m.text.clone(), m.text.clone());
    let mut foot = div()
        .flex()
        .items_center()
        .gap(px(2.))
        .mr(px(S1))
        .child(meta(faces, line).mr(px(6.)));
    if mine {
        foot = foot.child(
            small_button(SharedString::from(format!("again-{stamp}")), "retry")
                .tooltip(ui::tip("Say it again"))
                .on_click(cx.listener(move |this, _, _, cx| this.say_again(again.clone(), cx))),
        );
    }
    foot = foot.child(
        small_button(SharedString::from(format!("copy-{stamp}")), "copy")
            .tooltip(ui::tip("Copy"))
            .on_click(cx.listener(move |this, _, _, cx| this.copy_text(&copy, cx))),
    );
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(S1))
        .child(
            div()
                .max_w(relative(0.8))
                .px(px(S4))
                .py(px(10.))
                .rounded(px(BUBBLE_RADIUS))
                .bg(theme::panel2())
                .when(directed.is_some(), |d| {
                    d.border_1().border_color(theme::purple().opacity(0.4))
                })
                .child(
                    ui::text(faces, m.text.clone())
                        .text_size(px(BODY))
                        .line_height(px(BODY_LINE)),
                ),
        )
        .child(foot)
}

/// What she said: her mark in the gutter, her words as prose beside it,
/// and under them when it was, how long it took, and a way to copy them.
fn hers(
    faces: &Faces,
    m: &Message,
    tz: chrono_tz::Tz,
    took: Option<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let stamp = m.at.timestamp_millis();
    let key = format!("said-{stamp}");
    let mut line = ago(m.at, tz);
    if let Some(took) = took {
        line = format!("{line} · {took}");
    }
    let copy = m.text.clone();
    div()
        .flex()
        .items_start()
        .gap(px(GUTTER - 24.))
        .child(avatar().mt(px(1.)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(S2))
                .child(markdown::render(faces, &key, &m.text))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .ml(px(-5.))
                        .child(
                            small_button(SharedString::from(format!("copy-{stamp}")), "copy")
                                .tooltip(ui::tip("Copy"))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.copy_text(&copy, cx)),
                                ),
                        )
                        .child(meta(faces, line).ml(px(4.))),
                ),
        )
}

/// A line of secondary text that may wrap: a step's words, a summary.
pub(super) fn secondary(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(SMALL))
        .line_height(px(SMALL_LINE))
        .text_color(theme::muted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_going_on_breathes_from_full_to_a_third_and_back() {
        let at = |ms| breath(Duration::from_millis(ms));
        assert!((at(0) - 1.).abs() < 1e-4);
        assert!((at(900) - 0.35).abs() < 1e-4, "at half a breath");
        assert!((at(1800) - 1.).abs() < 1e-4, "and again");
        assert!(at(450) > 0.35 && at(450) < 1.);
    }
}
