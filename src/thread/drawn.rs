//! A diff and a plan, drawn as the design has them, for the thread and for
//! the side column alike. What a diff holds is read in `kinds.rs`.

use gpui::{Div, div, prelude::*, px};

use super::kinds::{Line, Status};
use crate::{fonts::Faces, theme, ui};

/// How a diff is set: in a frame in the thread, or bare and smaller in the
/// side column.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Thread,
    Column,
}

/// A diff: each line with its number in the file where it is known, a sign,
/// and a faint ground of green or coral for what came and went.
pub fn diff(faces: &Faces, lines: &[Line], start: Option<usize>, fit: Fit) -> Div {
    let (size, line, num) = match fit {
        Fit::Thread => (12.5, 19., 44.),
        Fit::Column => (11.5, 18., 36.),
    };
    let mut well = div()
        .flex()
        .flex_col()
        .w_full()
        .overflow_hidden()
        .font_family(faces.mono.clone())
        .text_size(px(size))
        .line_height(px(line))
        .text_color(theme::code_text())
        .when(fit == Fit::Thread, |d| {
            d.rounded(px(12.)).border_1().border_color(theme::border())
        });
    // The new side counts from the line the edit starts on; the line of
    // context before it is the one above; the old side counts as the new
    // does, for an edit replaces one stretch in place.
    let (mut old_n, mut new_n) = match (start, lines.first()) {
        (Some(s), Some(Line::Same(_))) => (s.saturating_sub(1), s.saturating_sub(1)),
        (Some(s), _) => (s, s),
        (None, _) => (0, 0),
    };
    for l in lines {
        let (sign, words, ground, mark, n) = match l {
            Line::Same(w) => {
                let n = new_n;
                old_n += 1;
                new_n += 1;
                (" ", w.clone(), None, theme::quiet(), n)
            }
            Line::Gone(w) => {
                let n = old_n;
                old_n += 1;
                (
                    "−",
                    w.clone(),
                    Some(theme::coral().opacity(0.09)),
                    theme::coral(),
                    n,
                )
            }
            Line::Added(w) => {
                let n = new_n;
                new_n += 1;
                (
                    "+",
                    w.clone(),
                    Some(theme::ok().opacity(0.09)),
                    theme::ok(),
                    n,
                )
            }
            Line::More(k) => {
                well = well.child(
                    div()
                        .pl(px(if start.is_some() { num + 26. } else { 38. }))
                        .py(px(2.))
                        .text_size(px(12.))
                        .text_color(theme::quiet())
                        .child(format!("… {k} more lines")),
                );
                continue;
            }
        };
        // Without a place in the file there are no numbers, and no gutter
        // for them: the sign starts the line.
        well = well.child(
            div()
                .flex()
                .w_full()
                .when_some(ground, Styled::bg)
                .child(
                    div()
                        .flex_none()
                        .w(px(if start.is_some() { num } else { 12. }))
                        .pr(px(10.))
                        .flex()
                        .justify_end()
                        .text_color(theme::quiet())
                        .child(if start.is_some() {
                            n.to_string()
                        } else {
                            String::new()
                        }),
                )
                .child(div().flex_none().w(px(16.)).text_color(mark).child(sign))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(words),
                ),
        );
    }
    well
}

/// A plan as the design draws it: a small square for each step, green and
/// struck through when done, amber while it is being done, empty before.
pub fn plan(faces: &Faces, steps: &[(Status, String)]) -> Div {
    let mut list = div().flex().flex_col().gap(px(2.));
    for (status, text) in steps {
        let mark = div()
            .flex_none()
            .size(px(14.))
            .mt(px(4.))
            .rounded(px(4.))
            .border(px(1.5))
            .border_color(match status {
                Status::Done => theme::ok(),
                Status::Doing => theme::accent(),
                Status::Todo => theme::quiet(),
            })
            .when(*status == Status::Done, |d| d.bg(theme::ok().opacity(0.25)));
        list = list.child(
            div().flex().items_start().gap(px(10.)).child(mark).child(
                ui::text(faces, text.clone())
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(13.5))
                    .line_height(px(22.))
                    .text_color(match status {
                        Status::Doing => theme::text(),
                        Status::Done => theme::faint(),
                        Status::Todo => theme::text2(),
                    })
                    .when(*status == Status::Done, Styled::line_through),
            ),
        );
    }
    list
}
