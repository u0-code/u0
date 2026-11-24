//! The small pieces every page is built from: mono labels, dots, panels,
//! buttons, section heads. None of them knows about agents.

use std::path::Path;

use gpui::{
    AnyElement, AnyView, App, Context, Div, Hsla, Render, SharedString, Svg, Window, div,
    prelude::*, px, svg,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::fonts::Weighted as _;
use crate::{
    fonts::Faces,
    style::{BODY, CAPTION, META},
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

/// An agent's words, and the one headline on a page.
pub fn serif(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.serif.clone())
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

impl Mark {
    /// The mark of the agent `id` called `name`: its logo when the home
    /// holds one.
    pub fn of(id: &str, name: &str) -> Self {
        // An agent on another machine shows its own logo.
        let id = crate::remote::split(id).0;
        if crate::registry::icon_path(&crate::settings::home(), id).is_file() {
            Self::Logo(SharedString::from(id.to_owned()))
        } else {
            Self::Letter(
                name.chars()
                    .find(|c| c.is_alphanumeric())
                    .map_or('?', |c| c.to_ascii_uppercase()),
            )
        }
    }
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

/// A button, the same everywhere in the desk: 28 high, corners of 8, its
/// weight said by its fill, its edge or neither.
pub fn button(
    faces: &Faces,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> gpui::Stateful<Div> {
    led_button(faces, id, None, label, weight)
}

/// A button with something before its word: an agent's mark, an icon.
pub fn led_button(
    faces: &Faces,
    id: impl Into<gpui::ElementId>,
    lead: Option<AnyElement>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> gpui::Stateful<Div> {
    let word = text(faces, label)
        .text_size(px(META))
        .weight(gpui::FontWeight::MEDIUM);
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(11.))
        .rounded(px(8.))
        .cursor_pointer()
        .children(lead);
    match weight {
        Weight::Primary => base
            .bg(theme::text())
            .hover(|s| s.bg(theme::text2()))
            .child(word.text_color(theme::bg())),
        Weight::Secondary => base
            .border_1()
            .border_color(theme::border2())
            .hover(|s| s.bg(theme::hover()))
            .child(word.text_color(theme::text())),
        Weight::Quiet => base
            .hover(|s| s.bg(theme::hover()))
            .child(word.text_color(theme::text2())),
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

/// An on/off switch.
pub fn switch(id: impl Into<gpui::ElementId>, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .w(px(30.))
        .h(px(18.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on {
            theme::text()
        } else {
            theme::ghost_line()
        })
        .when(on, Styled::justify_end)
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(if on { theme::bg() } else { theme::muted() }),
        )
}

/// An icon button: a 28-point square that lights on hover.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    icon_name: &str,
    tip_text: &'static str,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(7.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(icon(icon_name, 16.))
        .tooltip(tip(tip_text))
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

/// Something going on now: the amber dot in a faint ring of its own colour,
/// breathing.
pub fn live_dot() -> Div {
    breathing(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(12.))
            .rounded_full()
            .bg(theme::accent().opacity(0.16))
            .child(dot(theme::accent())),
    )
}

/// A piece of text that stays on one line. It is as wide as it may be: in
/// a row, what its siblings leave; in a column, the column. Hiding the
/// overflow is what lets it shrink below its text; a minimum width of
/// zero, or a flex basis of zero, would do that too, but in a column
/// either has the text measured without a width and cut to a few
/// characters. What is wider than that is clipped at its edge: text that
/// can be is cut to its room with [`fit`] first, so that it trails off.
pub fn one_line(d: Div) -> Div {
    d.w_full().overflow_hidden().whitespace_nowrap()
}

/// About how many points a column of one-line text takes at the sizes it
/// is set in (11.5 to 14): a Latin letter is a column, a Chinese
/// character two.
const POINTS_PER_COL: f32 = 7.;

/// How many columns of text `points` of room hold.
pub fn cols(points: f32) -> usize {
    (points.max(0.) / POINTS_PER_COL) as usize
}

/// `text` on one line in at most `cols` columns, a wide character
/// (Chinese, Japanese, Korean, an emoji) taking two: its runs of space and
/// its line breaks each one space, and ending in "…" when it is cut.
///
/// Every line the desk lets trail off is cut here, between graphemes, so
/// that no character is ever split. GPUI's own ellipsis is not used: it
/// measures a line before it knows its width, and measured again it can
/// cut Chinese inside a character and panic.
pub fn fit(text: &str, cols: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if width(&flat) <= cols {
        return flat;
    }
    let mut out = String::new();
    // The ellipsis takes a column of its own.
    let mut used = 1;
    for g in flat.graphemes(true) {
        if used + width(g) > cols {
            break;
        }
        used += width(g);
        out.push_str(g);
    }
    // Not a space and then the ellipsis.
    let mut out = out.trim_end().to_owned();
    out.push('…');
    out
}

/// How many columns `text` takes on one line, a wide character two.
pub fn width(text: &str) -> usize {
    text.graphemes(true)
        .map(|g| if g.chars().any(wide) { 2 } else { 1 })
        .sum()
}

/// Whether `c` takes two columns: the East Asian scripts and their
/// full-width forms, and the emoji.
fn wide(c: char) -> bool {
    matches!(
        c,
        '\u{1100}'..='\u{115F}'
            | '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE30}'..='\u{FE4F}'
            | '\u{FF00}'..='\u{FF60}'
            | '\u{FFE0}'..='\u{FFE6}'
            | '\u{1F300}'..='\u{1FAFF}'
            | '\u{20000}'..='\u{3FFFD}'
    )
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

/// How long one breath of something going on takes.
const BREATH: std::time::Duration = std::time::Duration::from_millis(2400);

/// A mark that breathes, slowly: something going on now, from full down to
/// a third and back. It is drawn at the point of its breath the clock is
/// at; the desk draws again a few times a second while something is going
/// on, which is enough for a breath this slow.
pub fn breathing(d: Div) -> Div {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let t = EPOCH.get_or_init(std::time::Instant::now).elapsed();
    let phase = (t.as_millis() % BREATH.as_millis()) as f32 / BREATH.as_millis() as f32;
    let wave = f32::midpoint(1., (phase * std::f32::consts::TAU).cos());
    d.opacity(0.35 + 0.65 * wave)
}

/// How much of something is used, as a ring: a faint track, and the share
/// used drawn over it from the top, clockwise.
pub fn ring(share: f32, size: f32) -> impl IntoElement {
    let share = share.clamp(0., 1.);
    gpui::canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let width = 2.5;
            let r = (size - width) / 2.;
            let c = bounds.center();
            let at = |turn: f32| {
                let a = turn * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                gpui::point(c.x + px(r * a.cos()), c.y + px(r * a.sin()))
            };
            let arc = |from: f32, to: f32| {
                let mut p = gpui::PathBuilder::stroke(px(width));
                p.move_to(at(from));
                p.arc_to(
                    gpui::point(px(r), px(r)),
                    px(0.),
                    to - from > 0.5,
                    true,
                    at(to),
                );
                p.build().ok()
            };
            for path in [arc(0., 0.5), arc(0.5, 0.999)].into_iter().flatten() {
                window.paint_path(path, theme::ghost_line());
            }
            if share > 0.005
                && let Some(path) = arc(0., share.min(0.999))
            {
                window.paint_path(path, theme::muted());
            }
        },
    )
    .size(px(size))
    .flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many columns `s` takes, as `fit` counts them.
    fn width(s: &str) -> usize {
        s.graphemes(true)
            .map(|g| if g.chars().any(wide) { 2 } else { 1 })
            .sum()
    }

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
            for cols in 0..=width(line) + 2 {
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
                    width(&cut) <= cols.max(1),
                    "{line:?} at {cols}: {cut:?} is {} wide",
                    width(&cut)
                );
            }
        }
    }

    #[test]
    fn a_room_in_points_holds_a_column_every_seven() {
        assert_eq!(cols(70.), 10);
        assert_eq!(cols(-5.), 0);
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
