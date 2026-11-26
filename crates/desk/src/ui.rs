//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about agents.

use std::path::Path;

use gpui::component::{
    Icon, Selectable as _, Sizable as _,
    alert::Alert,
    button::{Button, ButtonGroup, ButtonVariants as _},
    tooltip::Tooltip,
};
use gpui::{
    AnyElement, AnyView, App, Div, Hsla, SharedString, Svg, Window, div, prelude::*, px, svg,
};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use crate::fonts::Weighted as _;
use crate::{
    fonts::Faces,
    style::{BODY, CAPTION},
    theme,
};

/// A line icon by name, in the current text colour.
pub fn icon(name: &str, size: f32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .text_color(theme::muted())
}

/// A line icon by name, as gpui-kit's components take one (a menu's item).
pub fn glyph(name: &str) -> Icon {
    Icon::empty().path(format!("icons/{name}.svg"))
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

/// An agent's mark: its logo, or its initial where there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The logo the registry publishes for the agent with this id.
    Logo(SharedString),
    Letter(char),
}

/// An agent's mark at `size`: its logo in its brand's colour, or its letter
/// on a neutral tile.
pub fn mark(faces: &Faces, mark: &Mark, size: f32) -> AnyElement {
    match mark {
        Mark::Logo(id) => svg()
            .path(format!("agent/{id}.svg"))
            .size(px(size))
            .text_color(theme::brand(id))
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

/// A raised panel with a soft edge, over the page, for what floats but
/// opens from no trigger: the composer's offers, what a terminal's chosen
/// cells offer. A menu that opens from a
/// trigger is a `PopupMenu` (`Desk::menu`).
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

/// How much a button asks to be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// The one that does what the page is for: light on the dark ground,
    /// its word in the ground's colour. One to a card at most.
    Primary,
    /// Beside it: an outline.
    Secondary,
    /// A way out, such as Cancel: only its word.
    Quiet,
}

/// A button, the same everywhere in the desk: gpui-kit's, small, its
/// weight said by its variant.
pub fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> Button {
    led_button(id, None, label, weight)
}

/// A button with something before its word: an agent's mark, an icon.
pub fn led_button(
    id: impl Into<gpui::ElementId>,
    lead: Option<AnyElement>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> Button {
    let label = label.into();
    let button = Button::new(id).small();
    let button = match lead {
        Some(lead) => button
            .accessibility_label(label.clone())
            .child(lead)
            .child(label),
        None => button.label(label),
    };
    match weight {
        Weight::Primary => button.primary(),
        Weight::Secondary => button,
        Weight::Quiet => button.ghost(),
    }
}

/// The head of a section: small capitals, faint, as the side columns and
/// pages set them. GPUI has no letter spacing, so the capitals carry it.
pub fn label(faces: &Faces, text: &str) -> Div {
    self::text(faces, text.to_uppercase())
        .text_size(px(11.))
        .line_height(px(16.))
        .weight(gpui::FontWeight::MEDIUM)
        .text_color(theme::faint())
}

/// Something gone wrong, said where it happened (in a thread, on a page):
/// gpui-kit's error alert, tinted for trouble.
pub fn trouble(words: impl Into<SharedString>) -> Alert {
    Alert::error("trouble", words.into())
}

/// What a choice of a [`segmented`] control does when picked.
pub type Pick = Box<dyn Fn(&Vec<usize>, &mut Window, &mut App)>;

/// A few choices side by side in one frame, each a word, the one chosen
/// lit: gpui-kit's button group, a button to each choice, the chosen one
/// primary so it stands out from the ground in both looks.
pub fn segmented(id: &str, choices: Vec<(impl Into<SharedString>, bool, Pick)>) -> ButtonGroup {
    let (buttons, picks): (Vec<Button>, Vec<Pick>) = choices
        .into_iter()
        .enumerate()
        .map(|(n, (label, on, pick))| {
            let choice = Button::new(n).label(label).selected(on);
            (if on { choice.primary() } else { choice }, pick)
        })
        .unzip();
    ButtonGroup::new(SharedString::from(id.to_owned()))
        .small()
        .children(buttons)
        .on_click(move |chosen, window, cx| {
            for &n in chosen {
                picks[n](chosen, window, cx);
            }
        })
}

/// Small print: a count, a time, a word beside a name.
pub fn fine(faces: &Faces, words: impl Into<SharedString>) -> Div {
    small(text(faces, words))
}

/// Small print in mono: a hash, a count.
pub fn fine_mono(faces: &Faces, words: impl Into<SharedString>) -> Div {
    small(mono(faces, words))
}

fn small(d: Div) -> Div {
    d.text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// Lines added and lines gone, `+3 −1`, in their colours, at `size`.
pub fn counts(faces: &Faces, added: usize, gone: usize, size: f32) -> Div {
    let count =
        |words: String, color: Hsla| mono(faces, words).text_size(px(size)).text_color(color);
    div()
        .flex()
        .flex_none()
        .gap(px(4.))
        .when(added > 0 || gone == 0, |d| {
            d.child(count(format!("+{added}"), theme::added()))
        })
        .when(gone > 0, |d| {
            d.child(count(format!("−{gone}"), theme::gone()))
        })
}

/// What a menu's item says when one line is not enough: its name, and
/// under it, fainter, what it does.
pub fn described(
    faces: &Faces,
    label: impl Into<SharedString>,
    about: Option<impl Into<SharedString>>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .py(px(2.))
        .child(text(faces, label).text_size(px(13.)))
        .children(about.map(|a| text(faces, a).text_size(px(12.)).text_color(theme::faint())))
}

/// What opens a menu where it is not an icon (a chip, a line): gpui-kit's
/// quiet button, small, holding what it says; lit while its menu is open.
pub fn opener(id: impl Into<gpui::ElementId>) -> Button {
    Button::new(id).ghost().small()
}

/// An icon button: gpui-kit's, quiet until hovered, its tip said on
/// hover.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    icon_name: &str,
    tip_text: impl Into<SharedString>,
) -> Button {
    Button::new(id)
        .ghost()
        .icon(glyph(icon_name))
        .tooltip(tip_text)
}

/// A short note shown on hover, for `.tooltip`: gpui-kit's tooltip, which
/// wraps a column wide rather than across the window.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |window, cx| Tooltip::new(text.clone()).max_w(px(320.)).build(window, cx)
}

/// A six-pixel dot: a state.
pub fn dot(color: Hsla) -> Div {
    div().size(px(6.)).rounded_full().bg(color).flex_none()
}

/// Something going on now: the amber dot in a faint ring of its own colour,
/// breathing slowly, from full down to a third and back. The breath is the
/// GPU's, moving the drawn dot's alpha: nothing is laid out again for it.
pub fn live_dot() -> Svg {
    use gpui::{Easing, TimedTransformation, Transformation};
    svg()
        .path("icons/live-dot.svg")
        .size(px(12.))
        .flex_none()
        .text_color(theme::accent())
        .with_timed_transformation(
            TimedTransformation::new(BREATH)
                .from(Transformation::alpha(1.))
                .to(Transformation::alpha(0.35))
                .alternate()
                .with_easing(Easing::EaseInOut)
                .with_max_fps(30.),
        )
}

/// A piece of text that stays on one line. It is as wide as it may be: in
/// a row, what its siblings leave; in a column, the column. What is wider
/// trails off in an ellipsis where GPUI measured it would not fit, so the
/// cut falls between characters, Chinese or Latin, never through one.
pub fn one_line(d: Div) -> Div {
    d.w_full().truncate()
}

/// `text` on one line in at most `cols` columns, a wide character
/// (Chinese, Japanese, Korean, an emoji) taking two: its runs of space and
/// its line breaks each one space, and ending in "…" when it is cut.
///
/// For text that is kept or said somewhere else (a title made from what
/// you wrote, a notification), or a label that nothing clips held to a
/// length (a chip, a tab, a menu's line); never to make text fit a box:
/// what a box clips trails off in [`one_line`], measured.
pub fn fit(text: &str, cols: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.width() <= cols {
        return flat;
    }
    let mut out = String::new();
    // The ellipsis takes a column of its own.
    let mut used = 1;
    for g in flat.graphemes(true) {
        if used + g.width() > cols {
            break;
        }
        used += g.width();
        out.push_str(g);
    }
    // Not a space and then the ellipsis.
    let mut out = out.trim_end().to_owned();
    out.push('…');
    out
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

/// How long one breath of something going on takes.
const BREATH: std::time::Duration = std::time::Duration::from_millis(2400);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_too_long_is_cut_with_an_ellipsis_counting_chinese_as_two() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("abcdefghijkl", 6), "abcde…");
        assert_eq!(fit("一二三四五六", 7), "一二三…");
        assert_eq!(fit("一二三", 6), "一二三");
        assert_eq!(fit("ab cd", 4), "ab…");
        assert_eq!(fit("短 的\n句子", 10), "短 的 句子");
    }

    #[test]
    fn chinese_and_mixed_text_is_never_cut_inside_a_character_and_fits() {
        let lines = [
            "修复 bug：把 GPUI 的省略号换成我们自己的",
            "a修b复c",
            "整理一下 ~/Downloads 里的 PDF 和截图 🙂👍🏽 然后发给我",
            "café crème brûlée 和 naïve 的 résumé",
            "e\u{301}e\u{301}e\u{301}一二",
        ];
        for line in lines {
            for cols in 0..=line.width() + 2 {
                let cut = fit(line, cols);
                // Every grapheme of what is kept is one of the line's:
                // nothing is split, an accent stays on its letter and a
                // skin tone on its hand.
                let kept = cut.strip_suffix('…').unwrap_or(&cut);
                let n = kept.graphemes(true).count();
                assert!(
                    kept.graphemes(true).eq(line.graphemes(true).take(n)),
                    "{line:?} at {cols}: {cut:?}"
                );
                assert!(
                    cut.width() <= cols.max(1),
                    "{line:?} at {cols}: {cut:?} is {} wide",
                    cut.width()
                );
            }
        }
    }

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
