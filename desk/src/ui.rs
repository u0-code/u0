//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about her.

use std::path::Path;

use gpui::{
    AnyElement, AnyView, App, Context, Div, Hsla, Render, SharedString, Svg, Window, div,
    prelude::*, px, svg,
};

use crate::{
    brains::Mark,
    fonts::{self, Faces},
    style::{BODY, CAPTION, META, TINY},
    theme,
};

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
        .text_size(px(CAPTION))
        .text_color(theme::muted())
        .child(text.into())
}

/// Body text in the interface face.
pub fn text(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.ui.clone())
        .text_size(px(BODY))
        .text_color(theme::text())
        .child(text.into())
}

/// Her words, and the one headline on a page.
pub fn serif(faces: &Faces, text: impl Into<SharedString>) -> Div {
    fonts::with_serif_fallbacks(div())
        .font_family(faces.serif.clone())
        .text_color(theme::text())
        .child(text.into())
}

/// A brain's mark at `size`: its brand's icon in its brand's colour, or its
/// letter on a neutral tile.
pub fn mark(faces: &Faces, mark: Mark, size: f32) -> AnyElement {
    match mark {
        Mark::Icon(name) => icon(name, size)
            .text_color(theme::brand(name))
            .flex_none()
            .into_any_element(),
        Mark::Letter(letter) => div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(size))
            .rounded(px(size / 4.))
            .bg(theme::border2())
            .font_family(faces.mono.clone())
            .text_size(px(size * 0.62))
            .line_height(px(size))
            .text_color(theme::text2())
            .child(SharedString::from(letter.to_string()))
            .into_any_element(),
    }
}

/// A menu that opens over the page: a raised panel with a soft edge.
/// Every menu of the desk is drawn on it, so that they all look alike.
pub fn menu(id: impl Into<gpui::ElementId>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border2())
        .rounded(px(12.))
        .shadow_lg()
        .overflow_hidden()
}

/// The quiet title over a menu's choices: "Mode", "Brain".
pub fn menu_head(faces: &Faces, text: impl Into<SharedString>) -> Div {
    self::text(faces, text)
        .text_size(px(CAPTION))
        .text_color(theme::muted())
        .px(px(10.))
        .pt(px(8.))
        .pb(px(4.))
}

/// The key that picks a choice in a menu, at its right: "1", "Esc".
pub fn key(faces: &Faces, text: impl Into<SharedString>) -> Div {
    mono(faces, text)
        .flex_none()
        .text_size(px(TINY))
        .text_color(theme::faint())
}

/// How much a button asks to be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// The one that finishes what is on screen: a raised grey, its word
    /// in full text and medium weight. The accent is kept for small
    /// things (focus, the choice in use), never for a block of colour.
    Primary,
    /// Beside it: an outline.
    Secondary,
    /// A way out, such as Cancel: only its word.
    Quiet,
}

/// A button, the same everywhere in the desk: small corners, one height,
/// and its weight said by its edge and its type rather than a fill.
pub fn button(
    faces: &Faces,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> gpui::Stateful<Div> {
    let word = text(faces, label).text_size(px(META));
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.));
    match weight {
        Weight::Primary => base
            .cursor_pointer()
            .bg(theme::border2())
            .border_1()
            .border_color(theme::ghost_line())
            .hover(|s| s.bg(theme::ghost_line()))
            .child(
                word.font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::text()),
            ),
        Weight::Secondary => base
            .cursor_pointer()
            .border_1()
            .border_color(theme::border2())
            .hover(|s| s.bg(theme::hover()))
            .child(word.text_color(theme::text2())),
        Weight::Quiet => base
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(word.text_color(theme::muted())),
    }
}

/// A short note shown on hover, for `.tooltip`.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tip(text)).into()
    }
}

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(theme::panel2())
            .border_1()
            .border_color(theme::border2())
            .font_family(crate::fonts::faces(cx).ui.clone())
            .text_size(px(CAPTION))
            .text_color(theme::text2())
            .child(self.0.clone())
    }
}

/// A six-pixel dot: a state.
pub fn dot(color: Hsla) -> Div {
    div().size(px(6.)).rounded_full().bg(color).flex_none()
}

/// A piece of text that stays on one line and trails off. It is as wide
/// as it may be: in a row, what its siblings leave; in a column, the
/// column. Hiding the overflow is what lets it shrink below its text; a
/// minimum width of zero, or a flex basis of zero, would do that too, but
/// in a column either has the text measured without a width and cut to a
/// few characters.
pub fn one_line(d: Div) -> Div {
    d.w_full().overflow_hidden().text_ellipsis()
}

/// A box that scrolls sideways, for what is wider than the column it sits
/// in and must keep its lines whole: a table, a long line of output. It
/// takes only a sideways wheel: left to itself GPUI turns a wheel turned
/// down into a scroll sideways as well, and the thread under the pointer
/// would move both ways at once.
pub fn sideways(d: gpui::Stateful<Div>) -> gpui::Stateful<Div> {
    let mut d = d.overflow_x_scroll();
    d.style().restrict_scroll_to_axis = Some(true);
    d
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
