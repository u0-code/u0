//! A plan, drawn as the design has it, for the thread and for the side
//! column alike.

use gpui::{Div, div, prelude::*, px};

use super::kinds::Status;
use crate::{fonts::Faces, theme, ui};

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
