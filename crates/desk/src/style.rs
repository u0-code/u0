//! Carbon type scale, spacing and desktop layout constants.
//!
//! Type sizes, spacing and corner radii come from `u0ui::tokens` (Carbon v11:
//! 12 / 14 / 16 / 20 / 28 / 32 / 42 / 54 px, square corners by default).
//! Window and panel dimensions come from the design's `tokens.json` `"layout"`
//! group.

use gpui::{Div, prelude::*, px};
use u0ui::tokens::{self, TypeStyle};

use crate::{fonts::Faces, theme, ui};

// ── Carbon productive type scale (px at 100% zoom, 1rem = 16px) ─────────

/// `label-01` / `helper-text-01`: 12px / 16px.
pub const CAPTION: f32 = TypeStyle::Label01.spec().font_size_rem * tokens::REM_PX;
/// `body-compact-01`: 14px / 18px (buttons, rows, menu items, settings).
pub const LABEL: f32 = TypeStyle::BodyCompact01.spec().font_size_rem * tokens::REM_PX;
/// `code-01`: 12px / 16px; inline code in 14px prose uses `u0 Inline Mono` (13px).
pub const MONO: f32 = TypeStyle::Code01.spec().font_size_rem * tokens::REM_PX;
/// `body-compact-01`: 14px / 18px.
pub const BODY: f32 = TypeStyle::BodyCompact01.spec().font_size_rem * tokens::REM_PX;
/// `body-01`: 14px / 20px (paragraphs in threads and replies).
pub const PROSE: f32 = TypeStyle::Body01.spec().font_size_rem * tokens::REM_PX;
/// Line height of `body-01` at 100% zoom (20px).
pub const PROSE_LINE: f32 = 20.;
/// Line height of compact code rows (18px).
pub const MONO_LINE: f32 = 18.;
/// `heading-03`: 20px / 28px.
pub const H1: f32 = TypeStyle::Heading03.spec().font_size_rem * tokens::REM_PX;
/// `heading-02`: 16px / 24px.
pub const H2: f32 = TypeStyle::Heading02.spec().font_size_rem * tokens::REM_PX;
/// `heading-01`: 14px / 20px, semibold.
pub const H3: f32 = TypeStyle::Heading01.spec().font_size_rem * tokens::REM_PX;

/// `spacing-03`: 8px.
pub const S2: f32 = tokens::SPACING_03.px_value();

/// Carbon chat bubbles and cards are square (`border-radius-00` = 0px).
pub const BUBBLE_RADIUS: f32 = tokens::BORDER_RADIUS_00.px_value();

// ── Layout constants (`ds/carbon-ui/tokens.json` → `"layout"`) ──────────
// Sidebar default (`settings::LEFT = 256`), right column default
// (`settings::RIGHT = 400`), and thread column max widths (`settings::Width`:
// 720 / 880 with 24px gutter) live in `u0_core::settings` as the single source
// of truth.

/// `titlebar-height` (`2.375rem` = 38px): macOS unified title bar across all columns.
pub const TITLEBAR_HEIGHT: f32 = tokens::TITLEBAR_HEIGHT.px_value();
/// `traffic-light-inset` (`4.75rem` = 76px): leading inset clearing macOS traffic lights.
pub const TRAFFIC_LIGHT_INSET: f32 = tokens::TRAFFIC_LIGHT_INSET.px_value();
/// `traffic-light-offset-y` (`1.1875rem` = 19px): vertical centre of traffic lights in the 38px bar.
pub const TRAFFIC_LIGHT_OFFSET_Y: f32 = 19.;
/// `traffic-light-start` (`0.875rem` = 14px): left edge of the first traffic light (12px discs, 8px apart).
pub const TRAFFIC_LIGHT_START: f32 = 14.;
/// Minimum draggable width of the left sidebar (208px).
pub const SIDEBAR_MIN_WIDTH: f32 = 208.;
/// Minimum draggable width of the right column (280px).
pub const RIGHT_COLUMN_MIN_WIDTH: f32 = 280.;
/// `thread-min-width` (`30rem` = 480px): minimum thread column width before side columns overlay.
pub const THREAD_MIN_WIDTH: f32 = tokens::THREAD_MIN_WIDTH.px_value();
/// `settings-dialog-width` (`67.5rem` = 1080px): Settings modal width.
pub const SETTINGS_DIALOG_WIDTH: f32 = 1080.;
/// `settings-dialog-max-height` (`51.25rem` = 820px): Settings modal maximum height.
pub const SETTINGS_DIALOG_MAX_HEIGHT: f32 = 820.;
/// `settings-nav-width` (`14.5rem` = 232px): page list inside the Settings modal.
pub const SETTINGS_NAV_WIDTH: f32 = 232.;
/// `window-min-width` (800px): minimum window width.
pub const WINDOW_MIN_WIDTH: f32 = tokens::WINDOW_MIN_WIDTH.px_value();
/// `window-min-height` (520px): minimum window height.
pub const WINDOW_MIN_HEIGHT: f32 = tokens::WINDOW_MIN_HEIGHT.px_value();

/// A line of small print (`label-01`, 12/16) under a message or beside a step.
pub fn meta(faces: &Faces, text: impl Into<gpui::SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(CAPTION))
        .line_height(px(16.))
        .text_color(theme::faint())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{LEFT, RIGHT, Width};

    fn px_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < f32::EPSILON
    }

    #[test]
    fn layout_and_type_tokens_match_the_design_spec() {
        for (actual, expected) in [
            (CAPTION, 12.),
            (LABEL, 14.),
            (MONO, 12.),
            (BODY, 14.),
            (PROSE, 14.),
            (H1, 20.),
            (H2, 16.),
            (H3, 14.),
            (BUBBLE_RADIUS, 0.),
            (TITLEBAR_HEIGHT, 38.),
            (TRAFFIC_LIGHT_INSET, 76.),
            (TRAFFIC_LIGHT_OFFSET_Y, 19.),
            (TRAFFIC_LIGHT_START, 14.),
            (THREAD_MIN_WIDTH, 480.),
            (Width::Comfortable.column(2000.), 720.),
            (Width::Wide.column(2000.), 880.),
            (Width::Comfortable.column(528.), 480.),
            (SETTINGS_DIALOG_WIDTH, 1080.),
            (SETTINGS_DIALOG_MAX_HEIGHT, 820.),
            (SETTINGS_NAV_WIDTH, 232.),
            (WINDOW_MIN_WIDTH, 800.),
            (WINDOW_MIN_HEIGHT, 520.),
        ] {
            assert!(px_eq(actual, expected), "{actual} != {expected}");
        }
        assert_eq!((LEFT, RIGHT), (256, 400));
    }
}
