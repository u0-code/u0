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
    State, Task, calls, clock, s, span, steps,
    style::{BODY, BODY_LINE, CARD_RADIUS, MONO, S1, S2, S3, SMALL, SMALL_LINE, meta, well},
    task_first::breathing,
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

/// A piece of work, drawn as a coding agent's app draws what it did: no
/// frame, a line saying what the work is, and under it a quieter line
/// saying what it has done so far and for how long, which opens to every
/// step. What it came to is in her answer after it, so it is not said
/// here again until it is opened.
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
    let read = following.works.get(&t.id).filter(|r| r.read());
    let done = summary_of(t, read.map(|r| r.steps.as_slice()));

    let glyph = div()
        .flex()
        .flex_none()
        .w(px(MARK))
        .h(px(BODY_LINE))
        .items_center()
        .child(if state == State::Running {
            breathing(aside::glyph(state))
        } else {
            aside::glyph(state)
        });
    let title = div()
        .flex()
        .items_start()
        .child(glyph)
        .child(
            ui::text(faces, t.title().to_owned())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(BODY))
                .line_height(px(BODY_LINE))
                .text_color(theme::text()),
        )
        .when(!matches!(state, State::Finished | State::Running), |d| {
            d.child(
                meta(faces, state.word())
                    .flex_none()
                    .pl(px(S3))
                    .line_height(px(BODY_LINE))
                    .text_color(state.color()),
            )
        });
    let toggle = key.clone();
    let line = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(MARK))
        .h(px(24.))
        .child(
            ui::text(faces, done)
                .text_size(px(BODY))
                .text_color(theme::muted())
                .group_hover("work-head", |s| s.text_color(theme::text2())),
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
        );
    let head = div()
        .id(SharedString::from(key))
        .group("work-head")
        .flex()
        .flex_col()
        .cursor_pointer()
        .child(title)
        .child(line)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));

    let mut whole = div().flex().flex_col().w_full().child(head);
    if open {
        // Opened, everything it did is in one panel, the way a coding
        // agent's app opens a group of steps: what it was asked first, then
        // each step, then how it ended.
        let brief =
            div()
                .w_full()
                .px(px(10.))
                .pt(px(S2))
                .pb(px(S2))
                .child(crate::markdown::render_as(
                    faces,
                    &format!("brief:{}", t.id),
                    t.brief_shown(),
                    crate::markdown::Setting::Small,
                ));
        whole = whole.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .gap(px(S2))
                .pl(px(MARK))
                .child(
                    steps::panel()
                        .child(brief)
                        .children(along_the_way(faces, t, tz, following, expanded, cx)),
                )
                .children(parts(faces, t, following, expanded, cx)),
        );
    }
    whole
}

/// How much of what a milestone says shows: a few lines' worth.
const SAID_SHORT: usize = 240;

/// The room a work's mark takes before its words.
const MARK: f32 = 20.;

/// What a piece of work has done, in a line: `Ran 7 commands · 42s` once
/// it is over, `Working for 13s · 6 steps` while it goes on, and before its
/// steps are read, where it stands and for how long.
fn summary_of(t: &Task<'_>, steps: Option<&[LoggedStep]>) -> String {
    let state = t.state();
    let secs = (t.ended().unwrap_or_else(Utc::now) - t.started).num_seconds();
    let said: Vec<calls::Said> = steps
        .unwrap_or_default()
        .iter()
        .flat_map(|s| s.calls.iter())
        .filter(|c| calls::shown(&c.name) && c.name != "done")
        .map(|c| calls::say(&c.name, &c.args))
        .collect();
    match state {
        State::Running => calls::worked(secs, steps.map_or(0, <[LoggedStep]>::len), true),
        State::Waiting => format!("Waiting on you · {}", span(secs)),
        _ if !said.is_empty() => calls::summary(&said, secs),
        _ => {
            let word = state.word();
            let mut w = word.to_owned();
            if let Some(c) = w.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            format!("{w} in {}", span(secs))
        }
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
    for item in along(t.started, &t.events, &held[held.len() - shown..]) {
        rows.push(match item {
            Along::Step(step) => steps::step(faces, &t.id, step, expanded, cx),
            Along::Event(e) => milestone(faces, e, tz),
        });
    }
    if read.is_none() {
        rows.push(steps::quiet(faces, "Reading its steps…"));
    } else if t.state() == State::Running {
        rows.push(steps::live_row(faces, "Working"));
    }
    rows
}

/// Something a piece of work's journal says of it: a sitting's summary,
/// what it told or asked, how it ended. A line saying which, in the colour
/// of its kind, with when at the right; under it what was said, quietly,
/// a few lines at most, since how it ended is said again in her answer.
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
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .py(px(S1 + 2.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    ui::text(faces, tag)
                        .text_size(px(SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::kind_color(&e.kind)),
                )
                .child(meta(faces, clock(e.at, tz))),
        )
        .when(!text.trim().is_empty(), |d| {
            d.child(
                // Cut by characters here: GPUI's own clamp with an
                // ellipsis cuts Chinese inside a character and panics.
                ui::text(faces, calls::one_line(text, SAID_SHORT))
                    .text_size(px(SMALL))
                    .line_height(px(SMALL_LINE))
                    .text_color(theme::muted()),
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
    fn a_works_line_says_what_it_did_and_how_long_or_where_it_stands_before_its_steps_are_read() {
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
        assert!(summary_of(running, Some(&steps)).starts_with("Working for "));
        assert!(summary_of(running, Some(&steps)).ends_with("· 2 steps"));
        let finished = &tasks(&done)[0];
        assert_eq!(
            summary_of(finished, Some(&steps)),
            "Ran 3 commands · 42s",
            "the call that ends the work is not counted as something it did"
        );
        assert_eq!(summary_of(finished, None), "Finished in 42s");
    }
}
