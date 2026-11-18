//! An opened call drawn as what it is: an edit as a diff, a search as its
//! results, a plan as a checklist, a recall as its memories. What each
//! holds is read in `kinds.rs`; here it is only drawn, in the thread's
//! hairline frames, on its ground.

use gpui::{Context, Div, HighlightStyle, SharedString, StyledText, div, prelude::*, px};

use super::{
    kinds::{self, Hit, Line, Memory, Status},
    style::{MONO, MONO_LINE, S1, S2, SMALL, WELL_RADIUS, meta},
    task_first::breathing,
};
use crate::{desk::Desk, fonts::Faces, theme, ui};

/// A diff in a well: a line taken away on a faint coral, one put in on a
/// faint green, the sign in the stronger colour, the words left as they
/// are so that the code still reads as code.
pub fn diff(faces: &Faces, lines: &[Line]) -> Div {
    let mut well = div()
        .flex()
        .flex_col()
        .w_full()
        .py(px(S2))
        .rounded(px(WELL_RADIUS))
        .border_1()
        .border_color(theme::border())
        .overflow_hidden()
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .text_color(theme::code_text());
    for line in lines {
        let (sign, words, ground, mark) = match line {
            Line::Same(w) => (" ", w.clone(), None, theme::quiet()),
            Line::Gone(w) => (
                "−",
                w.clone(),
                Some(theme::coral().opacity(0.08)),
                theme::coral(),
            ),
            Line::Added(w) => (
                "+",
                w.clone(),
                Some(theme::code_argument().opacity(0.07)),
                theme::code_argument(),
            ),
            Line::More(n) => (" ", format!("… {n} more lines"), None, theme::faint()),
        };
        let more = matches!(line, Line::More(_));
        well = well.child(
            div()
                .flex()
                .w_full()
                .px(px(14.))
                .when_some(ground, Styled::bg)
                .child(div().flex_none().w(px(16.)).text_color(mark).child(sign))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .when(more, |d| d.text_color(theme::faint()))
                        .child(words),
                ),
        );
    }
    well
}

/// How many of a search's results show.
const HITS_SHOWN: usize = 5;

/// A search's results: each title a link, the site under it, quiet.
pub fn hits(faces: &Faces, key: &str, found: &[Hit], cx: &mut Context<Desk>) -> Div {
    let mut list = div().flex().flex_col().w_full().gap(px(S2)).px(px(10.));
    for (i, hit) in found.iter().take(HITS_SHOWN).enumerate() {
        let url = hit.url.clone();
        list = list.child(
            div()
                .id(SharedString::from(format!("{key}-hit-{i}")))
                .flex()
                .flex_col()
                .w_full()
                .overflow_hidden()
                .gap(px(1.))
                .cursor_pointer()
                .child(
                    ui::text(faces, super::calls::fit(&hit.title, 84))
                        .w_full()
                        .truncate()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme::link())
                        .hover(Styled::underline),
                )
                .child(meta(faces, hit.host().to_owned()))
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
        );
    }
    list
}

/// What a command printed, with the words in it that say something went
/// wrong (`FAILED`, `error`) in coral, and nothing else changed: no
/// ground, no mark beside it. A failure is told calmly, by its words.
pub fn marked(text: impl Into<SharedString>) -> StyledText {
    let text: SharedString = text.into();
    let alarm = HighlightStyle {
        color: Some(theme::coral()),
        ..HighlightStyle::default()
    };
    let ranges = kinds::alarms(&text);
    StyledText::new(text).with_highlights(ranges.into_iter().map(|r| (r, alarm)))
}

/// What a recall found: each memory's number, small and quiet in mono,
/// and what it says beside it, so that a memory can be told by its number
/// in her memory and read by its words.
pub fn memories(faces: &Faces, found: &[Memory]) -> Div {
    let mut list = div().flex().flex_col().w_full().gap(px(S2)).px(px(10.));
    for memory in found {
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .w_full()
                .child(
                    ui::mono(faces, format!("#{}", memory.n))
                        .flex_none()
                        .text_size(px(12.))
                        .line_height(px(21.))
                        .text_color(theme::faint()),
                )
                .child(
                    ui::text(faces, memory.text.clone())
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(theme::text2()),
                ),
        );
    }
    list
}

/// A plan as a checklist: what is done struck through and quiet, what is
/// being done breathing, what is still to do under an empty ring.
pub fn plan(faces: &Faces, steps: &[(Status, String)]) -> Div {
    let done = steps.iter().filter(|(s, _)| *s == Status::Done).count();
    let mut list = div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(S1))
        .child(meta(faces, format!("Plan · {done} of {}", steps.len())).mb(px(6.)));
    for (status, text) in steps {
        let mark = match status {
            Status::Done => ui::icon("check", 16.)
                .text_color(theme::muted())
                .into_any_element(),
            Status::Doing => breathing(ui::dot(theme::accent()).size(px(7.))).into_any_element(),
            Status::Failed => ui::icon("x", 13.)
                .text_color(theme::coral())
                .into_any_element(),
            Status::Dropped => div()
                .w(px(9.))
                .h(px(1.5))
                .bg(theme::quiet())
                .into_any_element(),
            Status::Todo => div()
                .size(px(10.))
                .rounded_full()
                .border_1()
                .border_color(theme::quiet())
                .into_any_element(),
        };
        let quiet = matches!(status, Status::Done | Status::Dropped);
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .size(px(16.))
                        .mt(px(2.5))
                        .items_center()
                        .justify_center()
                        .child(mark),
                )
                .child(
                    ui::text(faces, text.clone())
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(SMALL + 1.))
                        .line_height(px(21.))
                        .text_color(match status {
                            Status::Doing => theme::text(),
                            _ if quiet => theme::faint(),
                            _ => theme::muted(),
                        })
                        .when(quiet, Styled::line_through),
                ),
        );
    }
    list
}
