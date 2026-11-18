//! The cards a thread draws besides what was said: a line for something
//! opened or looked up while answering, and a card for each piece of work
//! taken on, which opens to what it did, step by step, with what it said
//! along the way between, and its parts, each a card of its own.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use ghost_link::protocol::{AskKind, LoggedStep, Work};
use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px};
use serde_json::Value;

use super::{
    State, Task, calls, clock, drawn, kinds, live, s, span, steps,
    style::{
        BODY_LINE, CARD_RADIUS, MONO, PROSE, PROSE_LINE, ROW, S1, S2, S3, S4, SMALL, meta, well,
    },
    task_first::breathing,
};
use crate::{
    answer::Whom, aside, desk::Desk, follow::Following, fonts::Faces, link::Order, model::Entry,
    theme, ui,
};

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
        .gap(px(6.))
        .h(px(40.))
        .px(px(S4))
        .child(
            ui::text(faces, verb)
                .text_size(px(ROW - 1.))
                .text_color(theme::muted())
                .flex_none(),
        )
        .child(
            ui::mono(faces, calls::fit(what, 60))
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(MONO + 0.5))
                .text_color(theme::text()),
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
            d.child(div().px(px(S3)).pb(px(S3)).child(well(faces).child(body)))
        })
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)))
}

/// A piece of work, drawn as a coding agent's app draws what it did: a
/// card with a line saying what the work is, and under it a quieter line
/// saying what it has done so far and for how long, which opens to every
/// step. What it came to is in her answer after it, so it is not said
/// here again until it is opened.
pub(super) fn task(
    faces: &Faces,
    t: &Task<'_>,
    view: &super::View<'_>,
    asks: Option<(&Work, &Whom)>,
    cx: &mut Context<Desk>,
) -> Div {
    let (tz, expanded, following, column) = (view.tz, view.expanded, view.following, view.column);
    let key = format!("task:{}", t.id);
    // How many columns of words the head has room for: the column less
    // the card's margins, its mark, its chevron and the gaps between,
    // at about 7 points a column for the title and 6.6 for the line
    // under it (a Chinese character takes two).
    let room = (column - 92.).max(80.);
    let (title_cols, under_cols) = ((room / 7.0) as usize, (room / 6.6) as usize);
    let open = expanded.contains(&key);
    let state = t.state();
    let read = following.works.get(&t.id).filter(|r| r.read());
    // What it handed over: the files it reported, its parts' too.
    let files: Vec<String> = t
        .events
        .iter()
        .chain(t.parts.iter().flat_map(|p| p.events.iter()))
        .filter(|e| e.kind == "reported")
        .filter_map(|e| e.data.get("files").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let done = summary_of(t, read.map(|r| r.steps.as_slice()), files.len());

    let glyph = if state == State::Running {
        breathing(aside::glyph(state))
    } else {
        aside::glyph(state)
    };
    // The title in text, the line under it muted; a state that stands
    // still for a reason colours that line, and a dropped work fades whole.
    let dropped = state == State::Dropped;
    let under = match state {
        State::Waiting => theme::accent(),
        State::Blocked => theme::coral(),
        State::Dropped => theme::quiet(),
        _ => theme::muted(),
    };
    // Held to the card's width, so that a long title ends in an ellipsis
    // rather than running past the frame.
    let words = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .gap(px(2.))
        .child(
            ui::one_line(ui::text(faces, calls::fit(t.title(), title_cols)))
                .whitespace_nowrap()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(if dropped {
                    theme::faint()
                } else {
                    theme::text()
                }),
        )
        .child(
            ui::one_line(ui::text(faces, calls::fit(&done, under_cols)))
                .whitespace_nowrap()
                .text_size(px(SMALL))
                .line_height(px(18.))
                .text_color(under),
        );
    // While it runs, an opened card can be stopped from its head.
    let stop = (open && state == State::Running)
        .then(|| asks.map(|(w, whom)| (w.id.clone(), whom.clone())))
        .flatten()
        .map(|(work, whom)| {
            div()
                .id(SharedString::from(format!("{key}:stop")))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::border2())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bubble()))
                .child(div().size(px(8.)).rounded(px(2.)).bg(theme::muted()))
                .child(
                    ui::text(faces, "Stop")
                        .text_size(px(SMALL))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.order(&whom, Order::Stop { work: work.clone() }, cx);
                }))
        });
    let toggle = key.clone();
    let head = div()
        .id(SharedString::from(key))
        .flex()
        .items_center()
        .gap(px(S3))
        .px(px(S4))
        .py(px(10.))
        .cursor_pointer()
        .child(glyph)
        .child(words)
        .children(stop)
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                16.,
            )
            .flex_none()
            .text_color(if dropped {
                theme::quiet()
            } else {
                theme::muted()
            }),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));

    // A card, as Claude's app draws a session it started: a hairline frame,
    // no ground, the steps inside it under a rule once it is opened.
    let mut whole = card().w_full().child(head);
    // What it asks, under its head whether it is opened or not: the work
    // is waiting, and the one who can answer is looking at it.
    if let Some((w, whom)) = asks
        && let Some(question) = w.question.as_deref()
    {
        whole = whole.child(asking(faces, w, question, whom, cx));
    }
    if open {
        // Opened, under a rule: the plan as it was last set, then each
        // step and what she said between them, then the parts it handed
        // out. Rows are drawn 16 in from the card's edge, as its head is.
        let plan = read
            .and_then(|r| {
                r.steps
                    .iter()
                    .flat_map(|s| s.calls.iter())
                    .rfind(|c| c.name == "plan" && !c.failed)
            })
            .map(|c| kinds::plan(&c.args))
            .unwrap_or_default();
        whole = whole.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .px(px(S4 - 10.))
                .pt(px(14.))
                .pb(px(S4))
                .border_t_1()
                .border_color(theme::border())
                .when(!plan.is_empty(), |d| {
                    d.child(drawn::plan(faces, &plan).px(px(10.))).child(
                        div()
                            .h(px(1.))
                            .mx(px(10.))
                            .mt(px(S3))
                            .mb(px(S2))
                            .bg(theme::border()),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .gap(px(2.))
                        .children(along_the_way(faces, t, tz, following, expanded, cx)),
                )
                .children(parts(faces, t, following, expanded, cx)),
        );
    }
    // What it handed over, as small cards under it, once it is done: each
    // opens where it is on the machine.
    if state != State::Finished || files.is_empty() {
        return whole;
    }
    let mut chips = div().flex().flex_wrap().gap(px(S2));
    for (i, file) in files.into_iter().enumerate() {
        let out = super::Output {
            file: file.clone(),
            source: super::Source::Report,
        };
        chips = chips.child(
            div()
                .id(SharedString::from(format!("out:{}:{i}", t.id)))
                .flex()
                .items_center()
                .gap(px(10.))
                .pl(px(10.))
                .pr(px(14.))
                .py(px(S2))
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(theme::border())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bubble()))
                .child(ui::icon("file", 18.).text_color(theme::muted()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            ui::text(faces, calls::fit(out.name(), 40))
                                .text_size(px(13.5))
                                .text_color(theme::text()),
                        )
                        .child(
                            ui::text(faces, out.kind())
                                .text_size(px(12.))
                                .text_color(theme::muted()),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.reveal(std::path::Path::new(&file), cx);
                })),
        );
    }
    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(S3))
        .child(whole)
        .child(chips)
}

/// What a piece of work asks, under its card's head: the question in her
/// own face, since she is the one asking, and the options it gave as
/// buttons, one click an answer. A question to answer in words is answered
/// in the right column, where its box is.
fn asking(faces: &Faces, w: &Work, question: &str, whom: &Whom, cx: &mut Context<Desk>) -> Div {
    let mut block = div()
        .flex()
        .flex_col()
        .gap(px(S3))
        .px(px(S4))
        .pt(px(S1))
        .pb(px(14.))
        .child(
            ui::serif(faces, question.to_owned())
                .text_size(px(PROSE))
                .line_height(px(PROSE_LINE)),
        );
    let mut answers: Vec<String> = Vec::new();
    if w.kind == Some(AskKind::Choose) {
        answers.extend(w.options.iter().cloned());
    } else if w.kind == Some(AskKind::Act) {
        answers.push("Done.".to_owned());
    }
    if answers.is_empty() {
        return block.child(meta(
            faces,
            "Answer it under Background tasks, on the right.",
        ));
    }
    let mut row = div().flex().flex_wrap().gap(px(S2));
    for (i, text) in answers.into_iter().enumerate() {
        let (whom, work) = (whom.clone(), w.id.clone());
        row = row.child(
            div()
                .id(SharedString::from(format!("ask:{}:{i}", w.id)))
                .flex()
                .items_center()
                .h(px(30.))
                .px(px(S3))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::border2())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bubble()))
                .child(
                    ui::text(faces, text.clone())
                        .text_size(px(SMALL))
                        .text_color(theme::text()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let order = Order::Answer {
                        work: work.clone(),
                        text: text.clone(),
                    };
                    this.order(&whom, order, cx);
                })),
        );
    }
    block = block.child(row);
    block
}

/// How much of what a milestone says shows: a few lines' worth.
const SAID_SHORT: usize = 240;

/// What a piece of work has done, in a line: `Ran 7 commands · 42s` once
/// it is over, `Working for 13s · 6 steps` while it goes on, and before its
/// steps are read, where it stands and for how long.
fn summary_of(t: &Task<'_>, steps: Option<&[LoggedStep]>, outputs: usize) -> String {
    let state = t.state();
    let time = span((t.ended().unwrap_or_else(Utc::now) - t.started).num_seconds());
    let calls: Vec<&ghost_link::protocol::ToolCall> = steps
        .unwrap_or_default()
        .iter()
        .flat_map(|s| s.calls.iter())
        .filter(|c| calls::shown(&c.name) && c.name != "done")
        .collect();
    let parts = (!t.parts.is_empty()).then(|| calls::plural(t.parts.len(), "part"));
    let line = |words: Vec<String>| {
        words
            .into_iter()
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    match state {
        // While it runs: what it is doing now, its parts, how long so far.
        State::Running => {
            let now = calls.last().map_or_else(
                || "Starting".to_owned(),
                |c| {
                    let said = calls::say(&c.name, &c.args);
                    format!("{} {}", said.verb, said.what).trim().to_owned()
                },
            );
            line(vec![now, parts.unwrap_or_default(), time])
        }
        State::Waiting => line(vec!["Waiting on you".to_owned(), t.last_line()]),
        // Done: what it changed, what it handed over, how long it took.
        State::Finished => {
            let said: Vec<calls::Said> =
                calls.iter().map(|c| calls::say(&c.name, &c.args)).collect();
            let failed = calls.iter().filter(|c| c.failed).count();
            let mut did = calls::tally(&said, failed);
            if let Some(c) = did.get_mut(0..1) {
                c.make_ascii_lowercase();
            }
            line(vec![
                "Done".to_owned(),
                did,
                parts.unwrap_or_default(),
                if outputs > 0 {
                    calls::plural(outputs, "output")
                } else {
                    String::new()
                },
                time,
            ])
        }
        State::Blocked => line(vec!["Blocked".to_owned(), t.last_line()]),
        State::Dropped => line(vec!["Dropped".to_owned(), t.last_line()]),
    }
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

/// What a piece of work did and said, interleaved, as the rows of its
/// panel: its steps, broken by what it told, asked and concluded.
fn along_the_way(
    faces: &Faces,
    t: &Task<'_>,
    tz: Tz,
    following: &Following,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Vec<Div> {
    let mut rows = Vec::new();
    let read = following.works.get(&t.id).filter(|r| r.read());
    let held: &[LoggedStep] = read.map_or(&[], |r| r.steps.as_slice());
    let (shown, earlier) = steps::earlier(faces, &t.id, held.len(), expanded, cx);
    if let Some(earlier) = earlier {
        rows.push(div().child(earlier));
    }
    // How it ended is in its head and said again in her answer after the
    // card; inside, only what happened on the way.
    let on_the_way: Vec<&Entry> = t
        .events
        .iter()
        .copied()
        .filter(|e| e.kind != "concluded")
        .collect();
    for item in along(t.started, &on_the_way, &held[held.len() - shown..]) {
        rows.push(match item {
            Along::Step(step) => steps::step(faces, &t.id, step, expanded, cx),
            Along::Event(e) => milestone(faces, e, tz),
        });
    }
    if read.is_none() {
        rows.push(steps::quiet(faces, "Reading its steps…"));
    } else if t.state() == State::Running {
        let since = live::work_since(held).unwrap_or(t.started);
        rows.push(live::row(faces, "Working", Some(since)));
    }
    rows
}

/// Something a piece of work's journal says of it on the way: a sitting's
/// summary, what it told or asked. A line saying which and when, and under
/// it what was said, quietly, a few lines at most.
fn milestone(faces: &Faces, e: &Entry, tz: Tz) -> Div {
    let (tag, text) = match e.kind.as_str() {
        "worked" => (
            format!(
                "Sat down · {} steps",
                e.data
                    .get("steps")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            ),
            s(&e.data, "summary"),
        ),
        "concluded" => {
            let mut word = s(&e.data, "outcome").replace('_', " ");
            if let Some(c) = word.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            (word, s(&e.data, "summary"))
        }
        "asked" => ("Asked".to_owned(), s(&e.data, "question")),
        "reported" => ("Told".to_owned(), s(&e.data, "text")),
        "added" => ("Added".to_owned(), s(&e.data, "text")),
        other => (other.to_owned(), s(&e.data, "text")),
    };
    // Drawn as what she says between steps is: quiet words, with what
    // happened and when in small print over them.
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .py(px(S1))
        .child(meta(faces, format!("{tag} · {}", clock(e.at, tz))))
        .when(!text.trim().is_empty(), |d| {
            d.child(
                // Cut by characters here: GPUI's own clamp with an
                // ellipsis cuts Chinese inside a character and panics.
                ui::text(faces, calls::one_line(text, SAID_SHORT))
                    .text_size(px(SMALL + 1.))
                    .line_height(px(BODY_LINE))
                    .text_color(theme::text2()),
            )
        })
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
    // What runs a part is known only from its daemon's list, not from her
    // journal.
    let known: Vec<(String, String, State, Option<&str>)> = if listed.is_empty() {
        t.parts
            .iter()
            .map(|p| (p.id.clone(), p.brief.clone(), p.state(), None))
            .collect()
    } else {
        listed
            .iter()
            .map(|p| {
                let state = aside::state_of(p.state);
                (p.id.clone(), p.brief.clone(), state, p.brain.as_deref())
            })
            .collect()
    };
    if known.is_empty() {
        return None;
    }
    // Under the steps, 16 in from their words, each part a smaller card.
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .my(px(6.))
        .pl(px(10. + S4))
        .pr(px(10.));
    for (id, brief, state, brain) in known {
        let last = t.parts.iter().find(|p| p.id == id).map(Task::last_line);
        column = column.child(steps::part(
            faces, &id, &brief, state, brain, last, following, expanded, cx,
        ));
    }
    Some(column)
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use ghost_link::protocol::ToolCall;
    use serde_json::json;

    use super::*;
    use crate::thread::tasks;

    fn entry(seq: i64, at: DateTime<Utc>, kind: &str, data: Value) -> Entry {
        Entry {
            seq,
            at,
            kind: kind.to_owned(),
            data,
        }
    }

    fn step(n: u64, calls: &[&str]) -> LoggedStep {
        LoggedStep {
            n,
            at: None,
            said: String::new(),
            ms: None,
            calls: calls
                .iter()
                .map(|name| ToolCall {
                    name: (*name).to_owned(),
                    args: json!({"command": "ls"}),
                    out: String::new(),
                    cut: false,
                    chars: 0,
                    ms: 0,
                    failed: false,
                    pictures: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn a_works_line_says_what_it_is_doing_or_what_it_did_and_how_long() {
        let t0 = Utc::now() - Duration::minutes(5);
        let open = vec![entry(1, t0, "undertook", json!({"id": "w", "brief": "b"}))];
        let mut done = open.clone();
        done.push(entry(
            2,
            t0 + Duration::seconds(42),
            "concluded",
            json!({"id": "w", "outcome": "finished", "summary": "s"}),
        ));
        let steps = [step(1, &["shell", "shell"]), step(2, &["shell", "done"])];

        let running = &tasks(&open)[0];
        assert!(
            summary_of(running, Some(&steps), 0).starts_with("Ran ls · "),
            "what it is doing now"
        );
        let finished = &tasks(&done)[0];
        assert_eq!(
            summary_of(finished, Some(&steps), 2),
            "Done · ran 3 commands · 2 outputs · 42s",
            "the call that ends the work is not counted as something it did"
        );
        assert_eq!(summary_of(finished, None, 0), "Done · 42s");
    }
}
