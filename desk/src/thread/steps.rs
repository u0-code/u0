//! What she did, step by step, drawn the way a coding agent's app draws
//! it: a row for each call, its verb in words and what it was about in
//! mono, how long it took at the right, and on a click what came back.
//!
//! A look at a conversation is one group, folded into "Worked for 12s · 3
//! steps" once it is over and open while it goes on. A piece of work's
//! card shows its steps the same way, with what the model said at a step
//! as a quiet line before that step's calls, and each part it handed out
//! as a card of its own inside it, as a coding agent shows its sub-agents.
//!
//! What is going on now breathes, slowly: the look or the work still
//! running, at the foot of its rows. A call that failed is marked, calmly,
//! in the colour of work that was blocked; nothing here is alarm red.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ghost_link::protocol::{LoggedStep, Look, ToolCall};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px};

use super::{
    State, calls,
    style::{BODY, MONO, S1, S2, S3, SMALL, SMALL_LINE, meta, well},
    task_first::{breathing, secondary},
};
use crate::{desk::Desk, follow, fonts::Faces, theme, ui};

/// How many of a work's steps show before "Show earlier steps": the
/// latest are what someone following it wants.
pub const STEPS_SHOWN: usize = 40;

/// The calls of a look that get a row, in the order she made them.
pub fn look_calls(look: &Look) -> impl Iterator<Item = &ToolCall> {
    look.rounds
        .iter()
        .flat_map(|r| r.calls.iter())
        .filter(|c| calls::shown(&c.name))
}

/// Whether the group of rows a look would have is open: one going on is
/// open unless it was folded; one that is over is folded unless opened.
/// The two are kept under different keys, so that what was done to a look
/// while it ran says nothing about it once it is over.
fn look_open(look: &Look, expanded: &HashSet<String>) -> (String, bool) {
    if look.running {
        let key = format!("look-live:{}", look.look);
        let open = !expanded.contains(&key);
        (key, open)
    } else {
        let key = format!("look:{}", look.look);
        let open = expanded.contains(&key);
        (key, open)
    }
}

/// One look's calls: a line saying how long and how many, which opens to
/// a row for each, and while it goes on, a last row breathing.
pub(super) fn look(
    faces: &Faces,
    look: &Look,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> AnyElement {
    let rows: Vec<&ToolCall> = look_calls(look).collect();
    let (key, open) = look_open(look, expanded);
    let secs = (to - from).num_seconds();
    let head = if look.running {
        calls::worked(secs, rows.len(), true)
    } else {
        let said: Vec<calls::Said> = rows.iter().map(|c| calls::say(&c.name, &c.args)).collect();
        calls::summary(&said, secs)
    };
    let toggle = key.clone();
    let header = div()
        .id(SharedString::from(format!("{key}-head")))
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .cursor_pointer()
        .child(
            ui::text(faces, head)
                .text_size(px(BODY))
                .text_color(theme::muted())
                .hover(|s| s.text_color(theme::text2())),
        )
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
            )
            .text_color(theme::faint()),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
    let mut group = div().flex().flex_col().child(header);
    if open {
        let mut list = rail();
        for (i, call) in rows.into_iter().enumerate() {
            list = list.child(call_row(faces, &format!("{key}:{i}"), call, expanded, cx));
        }
        if look.running {
            list = list.child(live_row(faces, "Thinking"));
        }
        group = group.child(list);
    }
    group.into_any_element()
}

/// The column rows hang in: a hairline down the left, under the chevron.
pub(super) fn rail() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .ml(px(2.))
        .pl(px(S3))
        .mt(px(4.))
        .border_l_1()
        .border_color(theme::border())
}

/// Where a call stands, as a small mark: done, or failed.
fn call_glyph(failed: bool) -> Div {
    if failed {
        div()
            .flex_none()
            .size(px(7.))
            .rounded_full()
            .border_1()
            .border_color(theme::coral())
    } else {
        ui::dot(theme::green().opacity(0.7)).size(px(6.)).m(px(0.5))
    }
}

/// The last row of something still going on: a ring breathing, and a word.
pub(super) fn live_row(faces: &Faces, word: &'static str) -> Div {
    let ring = div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .border_1()
        .border_color(theme::accent());
    div()
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(26.))
        .px(px(6.))
        .child(breathing(ring))
        .child(
            ui::text(faces, word)
                .text_size(px(SMALL))
                .text_color(theme::muted()),
        )
}

/// One call: its mark, its verb, what it was about, how long it took; and
/// opened, the head of what came back.
fn call_row(
    faces: &Faces,
    key: &str,
    call: &ToolCall,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Stateful<Div> {
    let said = calls::say(&call.name, &call.args);
    let open = expanded.contains(key);
    let right = if call.failed {
        meta(faces, "failed").text_color(theme::coral().opacity(0.8))
    } else {
        meta(faces, calls::took(call.ms))
    };
    let toggle = key.to_owned();
    let head = div()
        .id(SharedString::from(format!("{key}-row")))
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(26.))
        .px(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .when(open, |d| d.bg(theme::hover()))
        .child(call_glyph(call.failed))
        .child(
            ui::text(faces, said.verb)
                .flex_none()
                .text_size(px(SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text2()),
        )
        .child(
            ui::mono(faces, said.what)
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(MONO))
                .text_color(theme::muted()),
        )
        .child(right.flex_none())
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
    let mut row = div()
        .id(SharedString::from(key.to_owned()))
        .flex()
        .flex_col()
        .child(head);
    if open {
        let out = calls::output(&call.out, call.cut, call.chars, call.pictures);
        let mut body = well(faces).ml(px(20.)).mt(px(2.)).mb(px(S1));
        body = if out.text.is_empty() && out.more.is_none() {
            body.child(
                ui::text(faces, "Nothing came back.")
                    .text_size(px(SMALL))
                    .text_color(theme::faint()),
            )
        } else {
            body.child(out.text)
        };
        if let Some(more) = out.more {
            body = body.child(meta(faces, more).mt(px(6.)));
        }
        row = row.child(body);
    }
    row
}

/// One step of a piece of work: what the model said then, if anything,
/// and a row for each call.
pub(super) fn step(
    faces: &Faces,
    work: &str,
    step: &LoggedStep,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut block = div().flex().flex_col();
    let said = step.said.trim();
    if !said.is_empty() {
        block = block.child(
            secondary(faces, said.to_owned())
                .px(px(6.))
                .pt(px(S1))
                .pb(px(2.)),
        );
    }
    for (i, call) in step.calls.iter().enumerate() {
        let key = format!("step:{work}:{}:{i}", step.n);
        block = block.child(call_row(faces, &key, call, expanded, cx));
    }
    block
}

/// The steps of a work or a part, as far as they have been read: the
/// latest of them, a line to show the earlier, and while it runs, a last
/// row breathing.
pub(super) fn work_steps(
    faces: &Faces,
    work: &str,
    read: Option<&follow::WorkSteps>,
    running: bool,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut list = rail();
    let Some(read) = read.filter(|r| r.read()) else {
        return list.child(quiet(faces, "Reading its steps…"));
    };
    let (shown, earlier) = earlier(faces, work, read.steps.len(), expanded, cx);
    if let Some(earlier) = earlier {
        list = list.child(earlier);
    }
    for s in &read.steps[read.steps.len() - shown..] {
        list = list.child(step(faces, work, s, expanded, cx));
    }
    if read.steps.is_empty() && !running {
        list = list.child(quiet(faces, "No steps on record."));
    }
    if running {
        list = list.child(live_row(faces, "Working"));
    }
    list
}

/// How many of a work's `total` steps to show, the latest; and while
/// some are held back, a line that shows them.
pub(super) fn earlier(
    faces: &Faces,
    work: &str,
    total: usize,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> (usize, Option<Stateful<Div>>) {
    let all_key = format!("steps-all:{work}");
    let (shown, hidden) = super::shown(total, STEPS_SHOWN, expanded.contains(&all_key));
    let line = (hidden > 0).then(|| {
        div()
            .id(SharedString::from(all_key.clone()))
            .px(px(6.))
            .h(px(24.))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                ui::text(faces, format!("Show {hidden} earlier steps"))
                    .text_size(px(SMALL))
                    .text_color(theme::muted())
                    .hover(|s| s.text_color(theme::text2())),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&all_key, cx)))
    });
    (shown, line)
}

/// A line where there is nothing to show yet.
pub(super) fn quiet(faces: &Faces, text: &'static str) -> Div {
    ui::text(faces, text)
        .px(px(6.))
        .h(px(24.))
        .flex()
        .items_center()
        .text_size(px(SMALL))
        .text_color(theme::faint())
}

/// One part a piece of work handed out: a card inside its card, with its
/// state and its brief, which opens to its own steps.
#[allow(
    clippy::too_many_arguments,
    reason = "one part, drawn from what is known of it"
)]
pub(super) fn part(
    faces: &Faces,
    id: &str,
    brief: &str,
    state: State,
    last: Option<String>,
    following: &follow::Following,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let key = format!("part:{id}");
    let open = expanded.contains(&key);
    let n = id.rsplit('/').next().unwrap_or_default().to_owned();
    let toggle = key.clone();
    let head = div()
        .id(SharedString::from(format!("{key}-head")))
        .flex()
        .items_center()
        .gap(px(S2))
        .min_h(px(28.))
        .px(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(crate::aside::glyph(state))
        .child(meta(faces, format!("#{n}")).flex_none())
        .child(
            ui::text(faces, brief.to_owned())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(SMALL))
                .text_color(theme::text()),
        )
        .child(
            meta(faces, state.word())
                .flex_none()
                .text_color(state.color()),
        )
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
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
    let mut card = div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border())
        .bg(theme::bg())
        .p(px(2.))
        .child(head);
    if !open && let Some(last) = last.filter(|l| !l.is_empty()) {
        card = card.child(
            secondary(faces, last)
                .truncate()
                .pl(px(S3 + 13.))
                .pr(px(6.))
                .pb(px(S1))
                .line_height(px(SMALL_LINE)),
        );
    }
    if open {
        let running = state == State::Running;
        card = card.child(div().px(px(S1)).pb(px(S1)).child(work_steps(
            faces,
            id,
            following.works.get(id),
            running,
            expanded,
            cx,
        )));
    }
    card
}

#[cfg(test)]
mod tests {
    use ghost_link::protocol::LookRound;
    use serde_json::json;

    use super::*;

    fn call(name: &str) -> ToolCall {
        ToolCall {
            name: name.into(),
            args: json!({}),
            out: String::new(),
            cut: false,
            chars: 0,
            ms: 0,
            failed: false,
            pictures: 0,
        }
    }

    fn a_look(running: bool) -> Look {
        Look {
            look: 9,
            seq: (!running).then_some(30),
            running,
            rounds: vec![
                LookRound {
                    at: "2026-11-16T12:00:00Z".into(),
                    calls: vec![call("shell"), call("say")],
                    usage: None,
                    context: None,
                },
                LookRound {
                    at: "2026-11-16T12:00:04Z".into(),
                    calls: vec![call("read")],
                    usage: None,
                    context: None,
                },
            ],
        }
    }

    #[test]
    fn a_look_has_a_row_for_each_call_but_what_she_said() {
        let look = a_look(false);
        let names: Vec<&str> = look_calls(&look).map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["shell", "read"]);
    }

    #[test]
    fn a_look_going_on_is_open_and_one_that_is_over_is_folded_until_asked() {
        let mut expanded = HashSet::new();
        assert!(look_open(&a_look(true), &expanded).1);
        assert!(!look_open(&a_look(false), &expanded).1);
        expanded.insert("look-live:9".to_owned());
        assert!(
            !look_open(&a_look(true), &expanded).1,
            "folded while it ran"
        );
        assert!(
            !look_open(&a_look(false), &expanded).1,
            "and still folded after"
        );
        expanded.insert("look:9".to_owned());
        assert!(look_open(&a_look(false), &expanded).1);
    }
}
