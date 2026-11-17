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
use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, StyledText, TextRun, div,
    prelude::*, px,
};

use super::{
    State, calls, drawn, kinds,
    style::{
        BOX_RADIUS, MONO, MONO_LINE, ROW, ROW_LINE, S1, S2, S3, S4, SMALL, SMALL_LINE, WELL_RADIUS,
        meta,
    },
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
        .h(px(ROW_LINE))
        .cursor_pointer()
        .child(
            ui::text(faces, head)
                .text_size(px(ROW))
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
                14.,
            )
            .text_color(theme::faint()),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
    let mut group = div().flex().flex_col().child(header);
    if open {
        let mut list = panel();
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

/// What a group of steps opens into: its rows, a little in from the line
/// they open from, on the thread's own ground, as Claude's app opens one.
pub(super) fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(2.))
        .mt(px(S1))
        .pl(px(S4 - 10.))
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
        .h(px(30.))
        .px(px(10.))
        .child(breathing(ring))
        .child(
            ui::text(faces, word)
                .text_size(px(ROW - 1.))
                .text_color(theme::muted()),
        )
}

/// One call: its verb, what it was about, how long it took, and a chevron;
/// opened, a command in full in a well of its own, with a prompt before
/// it, its words coloured and a way to copy it, and under the well what
/// came back.
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
        meta(faces, "failed").text_color(theme::coral().opacity(0.85))
    } else {
        meta(faces, calls::took(call.ms))
    };
    // An edit or a file written says how much it changed, as a diff does.
    let changed = (!call.failed)
        .then(|| kinds::changed(&call.name, &call.args))
        .flatten();
    let counts = changed.as_deref().map(kinds::counted);
    let toggle = key.to_owned();
    let head = div()
        .id(SharedString::from(format!("{key}-row")))
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(30.))
        .px(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(
            ui::text(faces, said.verb.clone())
                .flex_none()
                .text_size(px(ROW))
                .text_color(theme::muted()),
        )
        .child(
            ui::mono(faces, calls::fit(&said.what, ROW_COLS))
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(MONO + 0.5))
                .text_color(theme::text()),
        )
        .when_some(counts, |d, (gone, added)| {
            d.when(added > 0, |d| {
                d.child(
                    meta(faces, format!("+{added}"))
                        .flex_none()
                        .text_color(theme::code_argument()),
                )
            })
            .when(gone > 0, |d| {
                d.child(
                    meta(faces, format!("−{gone}"))
                        .flex_none()
                        .text_color(theme::coral()),
                )
            })
        })
        .child(right.flex_none())
        .child(
            ui::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                11.,
            )
            .flex_none()
            .text_color(theme::faint()),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx)));
    let mut row = div()
        .id(SharedString::from(key.to_owned()))
        .flex()
        .flex_col()
        .w_full()
        .child(head);
    if open {
        // Opened, a call is a box: what it was in a word, the command in a
        // well under it, and what came back, all in hairline frames on the
        // thread's ground.
        let mut body = div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(S3))
            .mt(px(S1))
            .mb(px(S2))
            .px(px(S3))
            .pt(px(14.))
            .pb(px(S4))
            .rounded(px(BOX_RADIUS))
            .border_1()
            .border_color(theme::border())
            .child(
                ui::text(faces, label(&call.name, &said.verb))
                    .px(px(10.))
                    .text_size(px(ROW - 1.))
                    .text_color(theme::muted()),
            );
        if let Some(command) = calls::command_of(&call.name, &call.args) {
            body = body.child(command_well(faces, key, &command, cx));
        }
        // What it holds, drawn as what it is when it is one of those that
        // can be; anything else, or a call that failed, as what came back.
        let found = if call.name == "search" && !call.failed {
            kinds::hits(&call.out)
        } else {
            Vec::new()
        };
        let plan = if call.name == "plan" && !call.failed {
            kinds::plan(&call.args)
        } else {
            Vec::new()
        };
        if let Some(lines) = changed {
            row = row.child(body.child(drawn::diff(faces, &lines)));
            return row;
        }
        if !found.is_empty() {
            row = row.child(body.child(drawn::hits(faces, key, &found, cx)));
            return row;
        }
        if !plan.is_empty() {
            row = row.child(body.child(drawn::plan(faces, &plan).px(px(10.))));
            return row;
        }
        let out = calls::output(&call.out, call.cut, call.chars, call.pictures);
        let all_key = format!("{key}:all");
        let all = expanded.contains(&all_key);
        body = if out.text.is_empty() && out.more.is_none() {
            body.child(
                ui::text(faces, "Nothing came back.")
                    .px(px(10.))
                    .text_size(px(SMALL))
                    .text_color(theme::faint()),
            )
        } else if call.name == "read" && !call.failed {
            let path = call.args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            body.child(read_well(
                faces,
                &out.text,
                crate::syntax::by_path(path),
                all,
            ))
        } else {
            body.child(printed(faces, &out.text, all))
        };
        let lines = out.text.lines().count();
        if !all && lines > folded_at(&call.name) {
            let toggle = all_key.clone();
            body = body.child(
                div()
                    .id(SharedString::from(all_key))
                    .px(px(10.))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, format!("Show all {lines} lines"))
                            .text_size(px(SMALL))
                            .text_color(theme::muted())
                            .hover(|s| s.text_color(theme::text())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
            );
        }
        if let Some(more) = out.more {
            body = body.child(meta(faces, more).px(px(10.)));
        }
        row = row.child(body);
    }
    row
}

/// How many lines of what came back show before the rest is folded: a
/// file read shows its first twelve; anything else, if longer than eight,
/// its first three and its last.
fn folded_at(name: &str) -> usize {
    if name == "read" { 12 } else { 8 }
}

/// What a command printed, in mono, quiet: whole when it is short or
/// asked for, else its head, how much was left out, and its last line.
fn printed(faces: &Faces, text: &str, all: bool) -> Div {
    let lines: Vec<&str> = text.lines().collect();
    let mono = || {
        div()
            .w_full()
            .px(px(10.))
            .font_family(faces.mono.clone())
            .text_size(px(MONO))
            .line_height(px(MONO_LINE))
            .text_color(theme::muted())
            .whitespace_nowrap()
            .overflow_hidden()
    };
    if all || lines.len() <= folded_at("") {
        return mono().child(text.to_owned());
    }
    let head = lines[..3].join("\n");
    let gone = lines.len() - 4;
    div()
        .flex()
        .flex_col()
        .w_full()
        .child(mono().child(head))
        .child(
            mono()
                .text_color(theme::ghost_line())
                .child(format!("… {gone} lines")),
        )
        .child(mono().child(lines[lines.len() - 1].to_owned()))
}

/// A file read: its line numbers in a quiet gutter, its lines coloured by
/// the kind of file it is, the first twelve unless all are asked for.
fn read_well(faces: &Faces, text: &str, lang: &str, all: bool) -> Div {
    let mut gutter = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(40.))
        .pr(px(14.))
        .items_end()
        .text_color(theme::ghost_line());
    let mut code = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden();
    let mut tail: Option<String> = None;
    for (i, line) in text.lines().enumerate() {
        if !all && i >= folded_at("read") {
            break;
        }
        let trimmed = line.trim_start();
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && trimmed[digits..].starts_with("  ") {
            gutter = gutter.child(trimmed[..digits].to_owned());
            let words = &trimmed[digits + 2..];
            code = code.child(div().whitespace_nowrap().child(crate::syntax::styled(
                faces,
                if words.is_empty() { " " } else { words },
                lang,
            )));
        } else {
            tail = Some(line.to_owned());
        }
    }
    let well = div()
        .flex()
        .w_full()
        .py(px(S2))
        .rounded(px(WELL_RADIUS))
        .border_1()
        .border_color(theme::border())
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .child(gutter)
        .child(code);
    div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .w_full()
        .child(well)
        .when_some(tail, |d, t| d.child(meta(faces, t).px(px(10.))))
}

/// How many columns of mono a step's row has for what it was about.
const ROW_COLS: usize = 60;

/// What an opened call's box is headed with: the shell for a command, as a
/// terminal is named, and otherwise what was done.
fn label(name: &str, verb: &str) -> String {
    if name == "shell" {
        "Shell".to_owned()
    } else {
        verb.to_owned()
    }
}

/// A command in full, in a well: a prompt before it, its words coloured
/// the way a terminal colours them, wrapping rather than cut, and a button
/// at the top right to copy it.
fn command_well(faces: &Faces, key: &str, command: &str, cx: &mut Context<Desk>) -> Div {
    let copy = command.to_owned();
    div()
        .relative()
        .flex()
        .w_full()
        .gap(px(10.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(WELL_RADIUS))
        .border_1()
        .border_color(theme::border())
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .child(div().flex_none().text_color(theme::muted()).child("$"))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr(px(22.))
                .child(shell(faces, command)),
        )
        .child(
            div()
                .id(SharedString::from(format!("{key}-copy")))
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::bubble()))
                .child(ui::icon("copy", 15.).text_color(theme::text()))
                .tooltip(ui::tip("Copy"))
                .on_click(cx.listener(move |_, _, _, cx| Desk::copy_text(&copy, cx))),
        )
}

/// A command with its words coloured: the program at the head of each
/// command in blue, the joints between commands (`&&`, `|`, `;`) and a
/// comment faint, and everything else, arguments and quoted strings, in
/// green.
fn shell(faces: &Faces, command: &str) -> StyledText {
    let runs = calls::shell_words(command)
        .into_iter()
        .map(|(len, word)| {
            let color = match word {
                calls::Word::Program => theme::code_program(),
                calls::Word::Joint | calls::Word::Comment => theme::faint(),
                calls::Word::Space => theme::text2(),
                calls::Word::Argument => theme::code_argument(),
            };
            TextRun {
                len,
                font: gpui::Font {
                    family: faces.mono.clone(),
                    features: gpui::FontFeatures::default(),
                    fallbacks: Some(crate::fonts::fallbacks()),
                    weight: FontWeight::NORMAL,
                    style: gpui::FontStyle::Normal,
                },
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        })
        .collect();
    StyledText::new(SharedString::from(command.to_owned())).with_runs(runs)
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
    let mut block = div().flex().flex_col().w_full();
    let said = step.said.trim();
    if !said.is_empty() {
        // What she said at a step is written as she writes to anyone:
        // drawn the same way, small and quiet.
        let key = format!("said:{work}:{}", step.n);
        // A flex column, as her prose sits in: a table's columns are shares
        // of the width, and a plain block here leaves them nothing to share.
        block = block.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .px(px(10.))
                .pt(px(S2))
                .pb(px(S1))
                .child(crate::markdown::render_as(
                    faces,
                    &key,
                    said,
                    crate::markdown::Setting::Small,
                )),
        );
    }
    // How the work ended has a line of its own under its steps.
    for (i, call) in step
        .calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name != "done")
    {
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
    let mut list = panel();
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
            .px(px(10.))
            .h(px(30.))
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
        .px(px(10.))
        .h(px(30.))
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
        .rounded(px(10.))
        .border_1()
        .border_color(theme::border())
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
