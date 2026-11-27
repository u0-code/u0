//! Shared UI helpers built over `u0ui` (Material Symbols Sharp icons, Carbon
//! tokens and square geometry) and `gpui-kit`.

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
use u0ui::icons::{CarbonIcon, Icon as CarbonGlyph};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use crate::fonts::Weighted as _;
use crate::{
    fonts::Faces,
    style::{BODY, CAPTION},
    theme,
};

/// Resolve a desk icon name to its `u0ui` Material Symbols Sharp asset path
/// (or u0's own mark for `"u0"` and `"live-dot"`).
pub fn icon_path(name: &str) -> SharedString {
    match name {
        "u0" => return "icons/u0.svg".into(),
        "live-dot" => return "icons/live-dot.svg".into(),
        _ => {}
    }
    let glyph = match name {
        "dots" => CarbonGlyph::carbon(CarbonIcon::OverflowMenuHorizontal),
        "pin" => CarbonGlyph::carbon(CarbonIcon::Pin),
        "archive" => CarbonGlyph::carbon(CarbonIcon::Archive),
        "clock" => CarbonGlyph::carbon(CarbonIcon::Time),
        "branch" => CarbonGlyph::carbon(CarbonIcon::Branch),
        "mic" => CarbonGlyph::carbon(CarbonIcon::Microphone),
        "sliders" => CarbonGlyph::named("tune"),
        "reply" => CarbonGlyph::carbon(CarbonIcon::Reply),
        "arrow-down" => CarbonGlyph::carbon(CarbonIcon::ArrowDown),
        "expand" => CarbonGlyph::carbon(CarbonIcon::Maximize),
        "camera" => CarbonGlyph::carbon(CarbonIcon::Camera),
        "sidebar" => CarbonGlyph::carbon(CarbonIcon::SidePanelOpen),
        "chevron-left" => CarbonGlyph::carbon(CarbonIcon::ChevronLeft),
        "chevron-right" => CarbonGlyph::carbon(CarbonIcon::ChevronRight),
        "chevron-down" => CarbonGlyph::carbon(CarbonIcon::ChevronDown),
        "plus" => CarbonGlyph::carbon(CarbonIcon::Add),
        "search" => CarbonGlyph::carbon(CarbonIcon::Search),
        "laptop" => CarbonGlyph::carbon(CarbonIcon::Laptop),
        "server" => CarbonGlyph::named("dns"),
        "plug" => CarbonGlyph::carbon(CarbonIcon::Plug),
        "file" => CarbonGlyph::carbon(CarbonIcon::Document),
        "folder" => CarbonGlyph::carbon(CarbonIcon::Folder),
        "alert" => CarbonGlyph::carbon(CarbonIcon::Warning),
        "minus" => CarbonGlyph::carbon(CarbonIcon::Subtract),
        "check" => CarbonGlyph::carbon(CarbonIcon::Checkmark),
        "pencil" => CarbonGlyph::carbon(CarbonIcon::Edit),
        "x" => CarbonGlyph::carbon(CarbonIcon::Close),
        "retry" => CarbonGlyph::carbon(CarbonIcon::Renew),
        "power" => CarbonGlyph::named("power_settings_new"),
        "bell" => CarbonGlyph::carbon(CarbonIcon::Notification),
        "terminal" => CarbonGlyph::carbon(CarbonIcon::Terminal),
        "book" => CarbonGlyph::carbon(CarbonIcon::Book),
        "user" => CarbonGlyph::carbon(CarbonIcon::User),
        "panel-right" => CarbonGlyph::carbon(CarbonIcon::RightPanelOpen),
        "copy" => CarbonGlyph::carbon(CarbonIcon::Copy),
        "image-off" => CarbonGlyph::named("hide_image"),
        "arrow-right" => CarbonGlyph::carbon(CarbonIcon::ArrowRight),
        "settings" => CarbonGlyph::carbon(CarbonIcon::Settings),
        "list" => CarbonGlyph::carbon(CarbonIcon::List),
        "file-plus" => CarbonGlyph::carbon(CarbonIcon::DocumentAdd),
        "trash" => CarbonGlyph::carbon(CarbonIcon::TrashCan),
        "globe" => CarbonGlyph::carbon(CarbonIcon::Globe),
        "bulb" => CarbonGlyph::carbon(CarbonIcon::Idea),
        "square" => CarbonGlyph::carbon(CarbonIcon::StopFilled),
        "arrow-up" => CarbonGlyph::carbon(CarbonIcon::ArrowUp),
        "keyboard" => CarbonGlyph::carbon(CarbonIcon::Keyboard),
        "sun" => CarbonGlyph::carbon(CarbonIcon::Sun),
        "square-pen" => CarbonGlyph::named("edit_square"),
        "square-dashed" => CarbonGlyph::carbon(CarbonIcon::CircleDash),
        "text-search" => CarbonGlyph::named("manage_search"),
        "folder-plus" => CarbonGlyph::carbon(CarbonIcon::FolderAdd),
        "undo-2" => CarbonGlyph::carbon(CarbonIcon::Undo),
        "arrow-left" => CarbonGlyph::carbon(CarbonIcon::ArrowLeft),
        "diff" => CarbonGlyph::named("difference"),
        "pull-request" => CarbonGlyph::carbon(CarbonIcon::PullRequest),
        "list-plus" => CarbonGlyph::named("playlist_add"),
        "corner-up-right" => CarbonGlyph::carbon(CarbonIcon::Redo),
        "circle-dot" => CarbonGlyph::carbon(CarbonIcon::RadioButtonChecked),
        "gauge" => CarbonGlyph::carbon(CarbonIcon::Analytics),
        "play" => CarbonGlyph::carbon(CarbonIcon::Play),
        other => CarbonGlyph::named(other),
    };
    glyph.asset_path()
}

/// A sharp icon by name at `size` px, in `icon-secondary` (`theme::muted()`).
pub fn icon(name: &str, size: f32) -> Svg {
    svg()
        .path(icon_path(name))
        .size(px(size))
        .text_color(theme::muted())
}

/// A sharp icon by name, as `gpui-kit`'s components take one.
pub fn glyph(name: &str) -> Icon {
    Icon::empty().path(icon_path(name))
}

/// An icon of a menu item (16px).
pub fn menu_glyph(name: &str) -> Icon {
    glyph(name).with_size(gpui::component::Size::Medium)
}

/// Mono text: times, counts, hashes, keys.
pub fn mono(faces: &Faces, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(faces.mono.clone())
        .text_size(px(CAPTION))
        .text_color(theme::muted())
        .child(text.into())
}

/// Body text in the interface face (`IBM Plex Sans`).
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
/// on a Carbon 4px-radius neutral tile.
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
            .rounded(px(u0ui::t3::shell::TILE_RADIUS))
            .bg(theme::panel2())
            .font_family(faces.mono.clone())
            .text_size(px(size * 0.62))
            .line_height(px(size))
            .text_color(theme::text2())
            .child(SharedString::from(letter.to_string()))
            .into_any_element(),
    }
}

/// A floating Carbon layer (`layer-01`, 1px `border-subtle-01`, square
/// corners, shadow), for autocomplete offers and terminal cell actions.
pub fn menu(id: impl Into<gpui::ElementId>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .rounded_none()
        .shadow_md()
        .overflow_hidden()
}

/// How much a button asks to be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Primary,
    Secondary,
    Quiet,
}

/// A compact button, weighted as Carbon primary / secondary / ghost.
pub fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
) -> Button {
    led_button(id, None, label, weight)
}

/// A button with a leading mark or icon.
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

/// Section heading (`label-01` / `heading-compact-01`, sentence case, muted).
pub fn label(faces: &Faces, text: &str) -> Div {
    self::text(faces, text.to_owned())
        .text_size(px(CAPTION))
        .line_height(px(16.))
        .weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme::muted())
}

/// Inline error notification.
pub fn trouble(words: impl Into<SharedString>) -> Alert {
    Alert::error("trouble", words.into())
}

/// What a choice of a [`segmented`] control does when picked.
pub type Pick = Box<dyn Fn(&Vec<usize>, &mut Window, &mut App)>;

/// A compact segmented control.
pub fn segmented(id: &str, choices: Vec<(impl Into<SharedString>, bool, Pick)>) -> ButtonGroup {
    let (buttons, picks): (Vec<Button>, Vec<Pick>) = choices
        .into_iter()
        .enumerate()
        .map(|(n, (label, on, pick))| {
            let choice = Button::new(SharedString::from(format!("{id}-{n}")))
                .label(label)
                .selected(on);
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

/// Small print (`label-01`, 12/16): a count, a time, a word beside a name.
pub fn fine(faces: &Faces, words: impl Into<SharedString>) -> Div {
    small(text(faces, words))
}

/// Small print in mono (`code-01`, 12/16): a hash, a count.
pub fn fine_mono(faces: &Faces, words: impl Into<SharedString>) -> Div {
    small(mono(faces, words))
}

fn small(d: Div) -> Div {
    d.text_size(px(CAPTION))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// Lines added and lines gone, `+3 −1`, in their Carbon diff colours.
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

/// Menu item with a description underneath.
pub fn described(
    faces: &Faces,
    label: impl Into<SharedString>,
    about: Option<impl Into<SharedString>>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .py(px(2.))
        .child(text(faces, label).text_size(px(crate::style::LABEL)))
        .children(about.map(|a| {
            text(faces, a)
                .text_size(px(CAPTION))
                .text_color(theme::faint())
        }))
}

/// Quiet menu trigger button.
pub fn opener(id: impl Into<gpui::ElementId>) -> Button {
    Button::new(id).ghost().small()
}

/// Ghost icon button with accessible label and tooltip.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    icon_name: &str,
    tip_text: impl Into<SharedString>,
) -> Button {
    let tip_text = tip_text.into();
    Button::new(id)
        .ghost()
        .icon(glyph(icon_name))
        .accessibility_label(tip_text.clone())
        .tooltip(tip_text)
}

/// Hover tooltip capped at 320px wide.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |window, cx| Tooltip::new(text.clone()).max_w(px(320.)).build(window, cx)
}

/// Six-pixel status disc.
pub fn dot(color: Hsla) -> Div {
    div().size(px(6.)).rounded_full().bg(color).flex_none()
}

/// Breathing status indicator for an active turn.
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

/// Single-line truncated text.
pub fn one_line(d: Div) -> Div {
    d.w_full().truncate()
}

/// Fit `text` on one line in at most `cols` display columns (CJK = 2), ending
/// in `…` when truncated.
pub fn fit(text: &str, cols: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.width() <= cols {
        return flat;
    }
    let mut out = String::new();
    let mut used = 1;
    for g in flat.graphemes(true) {
        if used + g.width() > cols {
            break;
        }
        used += g.width();
        out.push_str(g);
    }
    let mut out = out.trim_end().to_owned();
    out.push('…');
    out
}

/// Abbreviate a path with `~` and middle `…` when deeper than 3 components.
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

const BREATH: std::time::Duration = std::time::Duration::from_millis(2400);

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource as _;

    #[test]
    fn every_desk_icon_resolves_through_u0ui_assets() {
        let assets = u0ui::Assets::with(crate::assets::Assets);
        let names = [
            "u0",
            "live-dot",
            "dots",
            "pin",
            "archive",
            "clock",
            "branch",
            "mic",
            "sliders",
            "reply",
            "arrow-down",
            "expand",
            "camera",
            "sidebar",
            "chevron-left",
            "chevron-right",
            "chevron-down",
            "plus",
            "search",
            "laptop",
            "server",
            "plug",
            "file",
            "folder",
            "alert",
            "minus",
            "check",
            "pencil",
            "x",
            "retry",
            "power",
            "bell",
            "terminal",
            "book",
            "user",
            "panel-right",
            "copy",
            "image-off",
            "arrow-right",
            "settings",
            "list",
            "file-plus",
            "trash",
            "globe",
            "bulb",
            "square",
            "arrow-up",
            "keyboard",
            "sun",
            "square-pen",
            "square-dashed",
            "text-search",
            "folder-plus",
            "undo-2",
            "arrow-left",
            "diff",
            "pull-request",
            "list-plus",
            "corner-up-right",
            "circle-dot",
            "gauge",
            "play",
        ];
        for name in names {
            let path = icon_path(name);
            let bytes = assets
                .load(&path)
                .unwrap()
                .unwrap_or_else(|| panic!("missing SVG for {name} ({path})"));
            assert!(bytes.starts_with(b"<svg"), "{name} ({path}) is not SVG");
        }
        assert!(
            CarbonGlyph::carbon(CarbonIcon::Add)
                .asset_path()
                .contains("add")
        );
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
            for cols in 0..=line.width() + 2 {
                let cut = fit(line, cols);
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
