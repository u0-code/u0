//! The thread's type and its rhythm, in one place, so that every line of
//! it is set from the same few sizes.
//!
//! Three sizes of text do nearly everything: the body her words, yours and
//! each step are read in, a smaller one for what is said about them (a
//! card's summary), and mono for what was run and what it printed. Space
//! comes in steps of four, and the reading column is about as wide as a
//! line of comfortable prose.

use gpui::{Div, div, prelude::*, px};

use crate::{fonts::Faces, theme, ui};

/// The reading column, and the composer under it.
pub const COLUMN: f32 = 680.;

// The sizes are measured from a 2x capture of Claude's desktop app, the
// standard the desk is held to: her prose's glyphs stand 30 px there, a
// line of code takes 38, "3 minutes ago" 24.

/// Body text: what you said, and what is said about things.
pub const BODY: f32 = 15.;
pub const BODY_LINE: f32 = 21.;
/// Her words, set in the serif, the same size as yours.
pub const PROSE: f32 = 15.;
pub const PROSE_LINE: f32 = 21.;
/// A step: "Ran python3…", and the line a look folds into. As large as
/// the body: the verb is grey and the thing is white, and that is enough
/// to set it apart.
pub const ROW: f32 = 15.;
pub const ROW_LINE: f32 = 22.;
/// Secondary text: a card's summary, what she said between steps.
pub const SMALL: f32 = 13.;
pub const SMALL_LINE: f32 = 19.;
/// Commands, paths and what they printed.
pub const MONO: f32 = 13.;
pub const MONO_LINE: f32 = 19.;
/// Small print: when, how long.
pub const META: f32 = 12.5;
/// Headings in what she wrote, by level.
pub const H1: (f32, f32) = (20., 28.);
pub const H2: (f32, f32) = (17., 24.);
pub const H3: (f32, f32) = (15., 21.);

/// The steps of space: within a line, between lines that belong together,
/// between blocks, between turns.
pub const S1: f32 = 4.;
pub const S2: f32 = 8.;
pub const S3: f32 = 12.;
pub const S4: f32 = 16.;
pub const S6: f32 = 24.;
/// Between turns.
pub const TURN: f32 = 32.;

/// How round things are. Nothing in the thread has a ground of its own but
/// your bubble: a card, a box and the well in it are told by a hairline.
pub const CARD_RADIUS: f32 = 12.;
pub const BUBBLE_RADIUS: f32 = 18.;
/// The box an opened step is drawn in, around its well.
pub const BOX_RADIUS: f32 = 16.;
pub const WELL_RADIUS: f32 = 12.;

/// A line of small print under a message or beside a step: who, when,
/// how long.
pub fn meta(faces: &Faces, text: impl Into<gpui::SharedString>) -> Div {
    ui::mono(faces, text)
        .text_size(px(META))
        .line_height(px(18.))
        .text_color(theme::faint())
}

/// A well for what was run or printed: mono in a hairline frame, on the
/// thread's own ground.
pub fn well(faces: &Faces) -> Div {
    div()
        .px(px(14.))
        .py(px(10.))
        .rounded(px(WELL_RADIUS))
        .border_1()
        .border_color(theme::border())
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .text_color(theme::text2())
}
