//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about her.

use std::path::Path;

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

/// A path as a person writes it: your home as `~`, and when it is long,
/// only its first and last two directories, so that the end, which says
/// what it is, survives a narrow column.
pub fn short_path(path: &Path, home: Option<&Path>) -> String {
    let (lead, rest) = match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => ("~".to_owned(), rest.to_path_buf()),
        None => (String::new(), path.to_path_buf()),
    };
    let parts: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let shown = if parts.len() > 3 {
        vec![
            parts[0].clone(),
            "…".to_owned(),
            parts[parts.len() - 2].clone(),
            parts[parts.len() - 1].clone(),
        ]
    } else {
        parts
    };
    format!("{lead}/{}", shown.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_path_keeps_its_home_its_head_and_its_end() {
        let home = Path::new("/Users/fox");
        let short = |p: &str| short_path(Path::new(p), Some(home));
        assert_eq!(
            short("/Users/fox/code/ghosts/solver/home"),
            "~/code/…/solver/home"
        );
        assert_eq!(short("/Users/fox/ghosts/solver"), "~/ghosts/solver");
        assert_eq!(
            short_path(Path::new("/srv/a/b/c/d"), Some(home)),
            "/srv/…/c/d"
        );
        assert_eq!(short_path(Path::new("/srv/a"), None), "/srv/a");
    }
}
