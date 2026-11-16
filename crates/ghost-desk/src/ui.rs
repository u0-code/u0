//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about her.

use gpui::{Div, Hsla, SharedString, Stateful, Svg, div, prelude::*, px, svg};

use crate::{fonts::Faces, theme};

/// A line icon by name, in the current text colour.
pub fn icon(name: &str, size: f32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .text_color(theme::muted())
}

/// Mono text: times, counts, hashes, keys.
pub fn mono(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.mono.clone())
        .text_size(px(12.))
        .text_color(theme::muted())
        .child(text.into())
}

/// A small mono label in capitals, spaced out: the head of a section.
pub fn eyebrow(faces: &Faces, text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div()
        .font_family(faces.mono.clone())
        .text_size(px(11.))
        .text_color(theme::faint())
        .child(SharedString::from(text.to_uppercase()))
}

/// Body text in the interface face.
pub fn text(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.ui.clone())
        .text_size(px(14.))
        .text_color(theme::text())
        .child(text.into())
}

/// Her words, and the one headline on a page.
pub fn serif(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.serif.clone())
        .text_color(theme::text())
        .child(text.into())
}

/// A six-pixel dot: a state.
pub fn dot(color: Hsla) -> Div {
    div().size(px(6.)).rounded_full().bg(color).flex_none()
}

/// A hollow dot: a state that is off.
pub fn hollow() -> Div {
    div()
        .size(px(6.))
        .rounded_full()
        .border_1()
        .border_color(theme::ghost_line())
        .flex_none()
}

/// A quiet button: a border, no fill.
pub fn button(
    id: impl Into<gpui::ElementId>,
    faces: &Faces,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .min_h(px(32.))
        .px(px(12.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme::border2())
        .font_family(faces.ui.clone())
        .text_size(px(12.5))
        .text_color(theme::text2())
        .cursor_pointer()
        .hover(|s| {
            s.border_color(theme::ghost_line())
                .text_color(theme::text())
        })
        .child(label.into())
}

/// Her initial in a circle: the avatar until she has a picture.
pub fn initial(faces: &Faces, name: &str, size: f32) -> Div {
    let letter: String = name
        .chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_default();
    div()
        .size(px(size))
        .rounded_full()
        .bg(theme::border2())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .font_family(faces.serif.clone())
        .text_size(px(size * 0.62))
        .text_color(theme::text())
        .child(letter)
}

/// A piece of text that stays on one line and trails off.
pub fn one_line(d: Div) -> Div {
    d.flex_1().min_w(px(0.)).overflow_hidden().text_ellipsis()
}
