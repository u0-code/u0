//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about her.

use gpui::{Div, Hsla, SharedString, Svg, div, prelude::*, px, svg};

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

/// A piece of text that stays on one line and trails off.
pub fn one_line(d: Div) -> Div {
    d.flex_1().min_w(px(0.)).overflow_hidden().text_ellipsis()
}
