//! The thread drawn task-first, the way a coding agent's app shows a
//! session: what you said in a bubble on the right, what she said as
//! prose in the reading column, under each a line
//! of small print with when it was and what can be done with it, and above
//! each answer what she did to get to it, folded into one line that says
//! what was done (drawn in `steps.rs`). Work taken on has a card of its
//! own (`cards.rs`). While she thinks, what she does shows as she does it.
//! Everything of hers starts at the column's edge, as in Claude's app: her
//! words are told from yours by their face and their side, not by a mark.

use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use gpui::{AnyElement, Context, Div, SharedString, Stateful, div, prelude::*, px};

use super::{
    Item, View, ago,
    cards::{task, tool},
    clock, could_not, directed, items, look_started, span, steps,
    style::{
        BODY, BODY_LINE, BUBBLE_RADIUS, COLUMN, ROW, ROW_LINE, S1, S2, S3, S4, S6, SMALL, TURN,
        meta,
    },
    thread_key, waiting_since,
};
use crate::{desk::Desk, fonts::Faces, markdown, model::Message, theme, ui};

/// What came before an item, for the space above it: her answer sits
/// close under your line and under the steps that led to it; turns stand
/// apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Before {
    Nothing,
    Yours,
    /// Someone else's line, after one of theirs: close under it.
    Theirs,
    Look,
    Other,
}

pub fn render(view: &View<'_>, faces: &Faces, cx: &mut Context<Desk>) -> AnyElement {
    let items = items(view);
    if items.is_empty() {
        return empty(view, faces);
    }
    let waiting = waiting_since(&items);
    // At a console every line of yours is an operator's, so none is marked
    // as one; on a platform the marked lines are worth telling apart.
    let marks = if thread_key(view.place).starts_with("console:") {
        HashMap::new()
    } else {
        directed(view.journal)
    };
    // The reading column: the pane's width up to 680, centred by its
    // margins, never shrunk to the window's height (it is what scrolls).
    let mut column = div()
        .flex()
        .flex_col()
        .flex_none()
        .w_full()
        .max_w(px(COLUMN))
        .mx_auto();

    // What you said last, and when: a failure offers to take it back.
    let mut last_yours: Option<DateTime<Utc>> = None;
    let mut last_line: Option<(String, i64)> = None;
    // What was last said to her by anyone, for timing a look still going.
    let mut heard: Option<DateTime<Utc>> = None;
    let mut before = Before::Nothing;
    // Who said the last line, when it was someone else's.
    let mut speaker: Option<String> = None;
    for (at, _, item) in items {
        if !matches!(&item, Item::Said { m, mine: false, .. } if !m.hers) {
            speaker = None;
        }
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
                    last_line = seq.map(|seq| (m.text.clone(), seq));
                }
                heard = Some(m.at);
                let directed = seq.and_then(|x| marks.get(&x).copied());
                let who = from.as_deref().unwrap_or(&view.who);
                let whose = Whose {
                    seq,
                    directed,
                    mine,
                };
                if mine {
                    let el =
                        yours(faces, &m, whose, who, view.tz, view.expanded, cx).into_any_element();
                    (el, Before::Yours)
                } else {
                    // Someone else's line, on a platform: under their name,
                    // unless the line before was theirs too.
                    let named = speaker.as_deref() != Some(who);
                    speaker = Some(who.to_owned());
                    let el = theirs(faces, &m, directed, who, named, view.tz);
                    let after = if named { Before::Other } else { Before::Theirs };
                    (el.into_any_element(), after)
                }
            }
            Item::Tool(e) => (
                tool(faces, e, view.tz, view.expanded, cx).into_any_element(),
                Before::Other,
            ),
            Item::Look { look, from, to } => {
                let el = steps::look(faces, look, from, to, view.expanded, cx);
                (el, Before::Look)
            }
            Item::Task(t) => {
                let asks = view
                    .work
                    .iter()
                    .find(|w| w.id == t.id)
                    .map(|w| (w, &view.whom));
                let el = task(faces, &t, view.tz, view.expanded, view.following, asks, cx);
                (el.into_any_element(), Before::Other)
            }
            Item::Failed(why) => {
                let el = failed(faces, &why, at, view.tz, last_line.clone(), cx);
                (el.into_any_element(), Before::Other)
            }
            Item::Stopped(stopped) => {
                let el = note(faces, stopped, clock(at, view.tz));
                (el.into_any_element(), Before::Other)
            }
        };
        let gap = match before {
            Before::Nothing => 0.,
            Before::Yours => S4,
            Before::Theirs => S1,
            Before::Look => S6,
            Before::Other => TURN,
        };
        column = column.child(slot(gap).child(el));
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
        column = column.child(slot(gap_after(before)).child(el));
    } else if let Some(since) = waiting {
        let el = working(faces, view, since).into_any_element();
        column = column.child(slot(gap_after(before)).child(el));
    }

    div()
        .id("thread")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .when_some(view.scroll, StatefulInteractiveElement::track_scroll)
        .px(px(32.))
        .pt(px(28.))
        .pb(px(S6))
        // A flex column, so the reading column has a width of its own to
        // lay what is in it against; the column itself does not shrink,
        // or the thread would end where the window does. Not centred by
        // the scroller: an item centred there is measured first with no
        // width, its text as one long line, and the thread comes out
        // shorter than it is, so its end cannot be scrolled to.
        .flex()
        .flex_col()
        .child(column)
        .into_any_element()
}

/// The widest your bubble grows, and how many lines of it show before the
/// rest is folded.
const BUBBLE_WIDTH: f32 = 520.;
const BUBBLE_LINES: usize = 8;

/// Where one thing in the thread sits: the column's whole width, never
/// more, `gap` below what came before. A flex column, so what is in it is
/// stretched to that width: your bubble then has an edge to sit against
/// at the right, and a card whose title does not wrap is held to the
/// column rather than pushing past it. A plain block here takes the width
/// of what it holds, and in a wide window that shows.
fn slot(gap: f32) -> Div {
    div().flex().flex_col().w_full().min_w(px(0.)).mt(px(gap))
}

/// The space above what she is doing now, after what came last.
fn gap_after(before: Before) -> f32 {
    if before == Before::Yours { S4 } else { TURN }
}

/// A small round button under a message: an icon that does one thing.
fn small_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::bubble()))
        .child(ui::icon(icon, 15.).text_color(theme::muted()))
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

/// A stop asked for from the desk, as a quiet line: a square, filled when
/// something was stopped and empty when nothing was running, and when.
fn note(faces: &Faces, stopped: bool, at: String) -> Div {
    let square = div().size(px(8.)).rounded(px(1.5));
    let square = if stopped {
        square.bg(theme::faint())
    } else {
        square.border_1().border_color(theme::faint())
    };
    div()
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(28.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(14.))
                .child(square),
        )
        .child(
            ui::text(
                faces,
                if stopped {
                    "Stopped."
                } else {
                    "Nothing was running."
                },
            )
            .text_size(px(ROW - 1.))
            .text_color(theme::muted()),
        )
        .child(meta(faces, at))
}

/// Where her answer would be, when she could not think: why, calmly, and
/// a way to take your last line back and put it another way.
fn failed(
    faces: &Faces,
    why: &str,
    at: DateTime<Utc>,
    tz: chrono_tz::Tz,
    again: Option<(String, i64)>,
    cx: &mut Context<Desk>,
) -> Div {
    let button = |id: String, text: &'static str| {
        div()
            .id(SharedString::from(id))
            .flex()
            .flex_none()
            .items_center()
            .h(px(28.))
            .px(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme::border2())
            .cursor_pointer()
            .hover(|s| s.bg(theme::bubble()))
            .child(
                ui::text(faces, text)
                    .text_size(px(SMALL))
                    .text_color(theme::text()),
            )
    };
    let stamp = at.timestamp_millis();
    let mut buttons = div().flex().gap(px(S2));
    if let Some((text, seq)) = again {
        buttons = buttons.child(button(format!("again-{stamp}"), "Try again").on_click(
            cx.listener(move |this, _, window, cx| {
                this.take_back(&text, seq, window, cx);
            }),
        ));
    }
    buttons = buttons.child(button(format!("brain-{stamp}"), "Switch brain").on_click(
        cx.listener(|this, _, window, cx| {
            this.open_menu(
                crate::desk::Menu::Brains,
                crate::desk::At::Composer,
                window,
                cx,
            );
        }),
    ));
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .child(div().pt(px(8.)).child(ui::dot(theme::coral()).size(px(6.))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(S2))
                .flex_1()
                .min_w(px(0.))
                .child(
                    ui::text(faces, could_not(why))
                        .text_size(px(ROW - 1.))
                        .line_height(px(BODY_LINE))
                        .text_color(theme::muted()),
                )
                .child(buttons)
                .child(meta(faces, clock(at, tz))),
        )
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
        .h(px(ROW_LINE))
        .child(if view.connected { breathing(dot) } else { dot })
        .child(
            ui::text(faces, word)
                .text_size(px(ROW - 1.))
                .text_color(theme::muted()),
        )
        .child(meta(faces, since))
}

/// A session with nothing in it yet: one line that says what she is. Nothing to click: what she does here is whatever you
/// say, and the line you type into is right below.
fn empty(view: &View<'_>, faces: &Faces) -> AnyElement {
    let line = if view.abilities.is_some() {
        format!("{} lives on this machine.", view.name)
    } else {
        format!("Nothing is said here in {}'s journal yet.", view.name)
    };
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
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .w_full()
                .max_w(px(520.))
                .child(
                    ui::serif(faces, line)
                        .text_size(px(22.))
                        .line_height(px(30.))
                        .text_center(),
                ),
        )
        .into_any_element()
}

/// What you said: a bubble on the right, the way a chat has it, and under
/// it when it was, with a way to take it back and to copy it.
/// Whose a line in the thread is, and where it stands in her journal.
#[derive(Clone, Copy)]
struct Whose {
    /// Its `heard` entry, once her journal has it.
    seq: Option<i64>,
    /// The entry that marked it a directive, on a platform.
    directed: Option<i64>,
    /// It came from you at the desk.
    mine: bool,
}

/// Someone else's line in a place on a platform: their name and when, and
/// their words in the interface's face, on the left with no bubble; a line
/// marked as a directive says so in the operator's colour.
fn theirs(
    faces: &Faces,
    m: &Message,
    directed: Option<i64>,
    who: &str,
    named: bool,
    tz: chrono_tz::Tz,
) -> Div {
    let head = div()
        .flex()
        .items_center()
        .gap(px(S2))
        .child(
            ui::text(faces, who.to_owned())
                .text_size(px(SMALL))
                .text_color(theme::muted()),
        )
        .when(directed.is_some(), |d| {
            d.child(
                ui::text(faces, "operator")
                    .px(px(6.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(theme::purple().opacity(0.35))
                    .text_size(px(11.5))
                    .text_color(theme::purple()),
            )
        })
        .child(meta(faces, ago(m.at, tz)));
    // Their mark: the first letter of their name on a small round tile,
    // only beside the first of their lines in a row.
    let initial: String = who.chars().take(1).collect();
    let mark = div()
        .flex()
        .flex_none()
        .size(px(24.))
        .items_center()
        .justify_center()
        .when(named, |d| {
            d.rounded_full().bg(theme::border2()).child(
                ui::text(faces, initial)
                    .text_size(px(11.))
                    .text_color(theme::text2()),
            )
        });
    div().flex().items_start().gap(px(10.)).child(mark).child(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(2.))
            .when(named, |d| d.child(head))
            .child(
                ui::text(faces, m.text.clone())
                    .text_size(px(BODY))
                    .line_height(px(BODY_LINE))
                    .text_color(theme::text()),
            ),
    )
}

fn yours(
    faces: &Faces,
    m: &Message,
    whose: Whose,
    who: &str,
    tz: chrono_tz::Tz,
    expanded: &std::collections::HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let Whose {
        seq,
        directed,
        mine,
    } = whose;
    let stamp = m.at.timestamp_millis();
    let mut line = ago(m.at, tz);
    if !mine {
        line = format!("{who} · {line}");
    }
    if let Some(seq) = directed {
        line = format!("{line} · directed · #{seq}");
    }
    let (again, copy) = (m.text.clone(), m.text.clone());
    // A long line of yours shows its first eight lines until it is opened.
    let fold_key = format!("bubble-{stamp}");
    let all = expanded.contains(&fold_key);
    let lines = m.text.lines().count();
    let shown_text = if lines > BUBBLE_LINES && !all {
        let head: Vec<&str> = m.text.lines().take(BUBBLE_LINES).collect();
        format!("{}…", head.join("\n"))
    } else {
        m.text.clone()
    };
    let mut foot = div()
        .flex()
        .items_center()
        .gap(px(2.))
        .mr(px(2.))
        .child(meta(faces, line).mr(px(6.)));
    // A line of yours her journal has can be taken back, to be put
    // another way; one still on its way to her cannot yet.
    if mine && let Some(seq) = seq {
        foot = foot.child(
            small_button(SharedString::from(format!("again-{stamp}")), "retry")
                .tooltip(ui::tip(
                    "Say it again: back in the box, and what followed taken back",
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.take_back(&again, seq, window, cx);
                })),
        );
    }
    foot = foot.child(
        small_button(SharedString::from(format!("copy-{stamp}")), "copy")
            .tooltip(ui::tip("Copy"))
            .on_click(cx.listener(move |_, _, _, cx| Desk::copy_text(&copy, cx))),
    );
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(S2))
        .child(
            div()
                .max_w(px(BUBBLE_WIDTH))
                .px(px(S4))
                .py(px(S3))
                .rounded(px(BUBBLE_RADIUS))
                .bg(theme::bubble())
                .when(directed.is_some(), |d| {
                    d.border_1().border_color(theme::purple().opacity(0.4))
                })
                .child(
                    ui::text(faces, shown_text)
                        .text_size(px(BODY))
                        .line_height(px(BODY_LINE)),
                ),
        )
        .when(lines > BUBBLE_LINES && !all, |d| {
            let toggle = fold_key.clone();
            d.child(
                div()
                    .id(SharedString::from(fold_key.clone()))
                    .mr(px(S1))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, format!("Show all {lines} lines"))
                            .text_size(px(SMALL))
                            .text_color(theme::muted())
                            .hover(|s| s.text_color(theme::text())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
            )
        })
        .child(foot)
}

/// What she said: her words as prose at the column's edge, and under them when it was, how long it took, and a way to copy them.
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
        .flex_col()
        .w_full()
        .gap(px(S3))
        .child(markdown::render(faces, &key, &m.text))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .ml(px(-4.))
                .child(
                    small_button(SharedString::from(format!("copy-{stamp}")), "copy")
                        .tooltip(ui::tip("Copy"))
                        .on_click(cx.listener(move |_, _, _, cx| Desk::copy_text(&copy, cx))),
                )
                .child(meta(faces, line).ml(px(4.))),
        )
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
