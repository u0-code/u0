//! An opened call drawn as what it is: an edit as a diff, a search as its
//! results, a plan as a checklist, a recall as its memories. What each
//! holds is read in `kinds.rs`; here it is only drawn, in the thread's
//! hairline frames, on its ground.

use gpui::{Div, div, prelude::*, px};

use super::kinds::{Line, Status};
use crate::{
    fonts::Faces,
    style::{BODY, MONO, MONO_LINE, S1, S2, WELL_RADIUS, meta},
    theme, ui,
};

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
            Status::Doing => {
                ui::breathing(ui::dot(theme::accent()).size(px(7.))).into_any_element()
            }
            Status::Todo => div()
                .size(px(10.))
                .rounded_full()
                .border_1()
                .border_color(theme::quiet())
                .into_any_element(),
        };
        let quiet = *status == Status::Done;
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
                        .text_size(px(BODY))
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
