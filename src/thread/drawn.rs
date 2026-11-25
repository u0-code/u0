//! A diff and a plan, drawn as the design has them, for the thread and for
//! the side column alike.

use gpui::{Div, div, prelude::*, px};

use super::kinds::Status;
use crate::{fonts::Faces, theme, ui};

/// A line's code: wrapped (`wrap`, as the settings say), or kept to one
/// line and cut at the edge.
fn code(cell: Div, wrap: bool) -> Div {
    if wrap {
        cell
    } else {
        cell.whitespace_nowrap().overflow_hidden()
    }
}

/// How a diff is set: in a frame in the thread, or bare and smaller in the
/// side column.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Thread,
    Column,
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

/// A file's changes, from git or an edit: each hunk under its head, every line
/// numbered on the side it is on.
pub fn hunks(
    faces: &Faces,
    file: &crate::git::FileDiff,
    wrap: bool,
    fit: Fit,
    on_line: Option<&OnLine>,
) -> Div {
    use crate::git::Line as G;
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
    if file.binary {
        return well.child(
            div()
                .px(px(12.))
                .py(px(6.))
                .text_color(theme::faint())
                .child("A binary file: no lines to show."),
        );
    }
    for (h, hunk) in file.hunks.iter().enumerate() {
        well = well.child(
            div()
                .pl(px(num + 16.))
                .py(px(2.))
                .text_size(px(12.))
                .text_color(theme::quiet())
                .whitespace_nowrap()
                .overflow_hidden()
                .child(hunk.head.clone()),
        );
        for (i, l) in hunk.lines.iter().enumerate() {
            let (n, sign, text, ground, mark) = match l {
                G::Same { new, text, .. } => (*new, " ", text, None, theme::quiet()),
                G::Gone { old, text } => (
                    *old,
                    "−",
                    text,
                    Some(theme::gone().opacity(0.09)),
                    theme::gone(),
                ),
                G::Added { new, text } => (
                    *new,
                    "+",
                    text,
                    Some(theme::added().opacity(0.09)),
                    theme::added(),
                ),
            };
            let row = div()
                .flex()
                .w_full()
                .when_some(ground, Styled::bg)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .justify_end()
                        .w(px(num))
                        .pr(px(10.))
                        .text_color(theme::quiet())
                        .child(n.to_string()),
                )
                .child(div().flex_none().w(px(16.)).text_color(mark).child(sign))
                .child(code(div().flex_1().min_w(px(0.)), wrap).child(text.clone()));
            // A line can be clicked, to say something of it.
            well = match on_line {
                Some(on) => {
                    let on = on.clone();
                    well.child(
                        row.id(gpui::SharedString::from(format!("{}-{h}-{i}", file.path)))
                            .cursor_pointer()
                            .hover(|st| st.bg(theme::hover()))
                            .on_click(move |_, window, cx| on(n, window, cx)),
                    )
                }
                None => well.child(row),
            };
        }
    }
    well
}

/// What to do when a line of a diff is clicked: its number.
pub type OnLine = std::rc::Rc<dyn Fn(usize, &mut gpui::Window, &mut gpui::App)>;

/// A file's changes side by side: as it was on the left, as it is on the
/// right, the lines taken away beside those put in their place.
pub fn hunks_split(faces: &Faces, file: &crate::git::FileDiff, wrap: bool) -> Div {
    use crate::git::Line as G;
    let mut well = div()
        .flex()
        .flex_col()
        .w_full()
        .overflow_hidden()
        .font_family(faces.mono.clone())
        .text_size(px(11.5))
        .line_height(px(18.))
        .text_color(theme::code_text());
    let cell = |n: Option<usize>, text: Option<&String>, ground: Option<gpui::Hsla>| {
        div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .when_some(ground, Styled::bg)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .w(px(36.))
                    .pr(px(8.))
                    .text_color(theme::quiet())
                    .child(n.map(|n| n.to_string()).unwrap_or_default()),
            )
            .child(
                code(div().flex_1().min_w(px(0.)), wrap).child(text.cloned().unwrap_or_default()),
            )
    };
    let gone_bg = theme::gone().opacity(0.09);
    let added_bg = theme::added().opacity(0.09);
    for hunk in &file.hunks {
        well = well.child(
            div()
                .pl(px(44.))
                .py(px(2.))
                .text_size(px(12.))
                .text_color(theme::quiet())
                .whitespace_nowrap()
                .overflow_hidden()
                .child(hunk.head.clone()),
        );
        // Runs of lines taken away and put in, paired row by row.
        let mut gone: Vec<(usize, &String)> = Vec::new();
        let mut added: Vec<(usize, &String)> = Vec::new();
        let flush =
            |well: Div, gone: &mut Vec<(usize, &String)>, added: &mut Vec<(usize, &String)>| {
                let mut well = well;
                for k in 0..gone.len().max(added.len()) {
                    let l = gone.get(k);
                    let r = added.get(k);
                    well = well.child(
                        div()
                            .flex()
                            .w_full()
                            .gap(px(1.))
                            .child(cell(l.map(|x| x.0), l.map(|x| x.1), l.map(|_| gone_bg)))
                            .child(cell(r.map(|x| x.0), r.map(|x| x.1), r.map(|_| added_bg))),
                    );
                }
                gone.clear();
                added.clear();
                well
            };
        for l in &hunk.lines {
            match l {
                G::Gone { old, text } => gone.push((*old, text)),
                G::Added { new, text } => added.push((*new, text)),
                G::Same { old, new, text } => {
                    well = flush(well, &mut gone, &mut added);
                    well = well.child(
                        div()
                            .flex()
                            .w_full()
                            .gap(px(1.))
                            .child(cell(Some(*old), Some(text), None))
                            .child(cell(Some(*new), Some(text), None)),
                    );
                }
            }
        }
        well = flush(well, &mut gone, &mut added);
    }
    well
}
