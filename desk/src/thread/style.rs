//! The thread's type and its rhythm, in one place, so that every line of
//! it is set from the same few sizes.
//!
//! Three sizes of text do nearly everything: the body her words and yours
//! are read in, a smaller one for what is said about them (a step, a
//! card's summary), and mono for what was run and what it printed. Space
//! comes in steps of four, and the reading column is about as wide as a
//! line of comfortable prose.

use gpui::{Div, div, prelude::*, px};

use crate::{fonts::Faces, theme, ui};

/// The reading column, and the composer under it.
pub const COLUMN: f32 = 720.;

/// Body text: what you said, and what is said about things.
pub const BODY: f32 = 15.;
pub const BODY_LINE: f32 = 24.;
/// Her words, set in the serif: a little larger, with more lead.
pub const PROSE: f32 = 16.;
pub const PROSE_LINE: f32 = 26.;
/// The gutter her avatar sits in, which everything of hers starts after.
pub const GUTTER: f32 = 34.;
/// Secondary text: steps, summaries, notes.
pub const SMALL: f32 = 12.5;
pub const SMALL_LINE: f32 = 19.;
/// Commands, paths and what they printed.
pub const MONO: f32 = 12.;
pub const MONO_LINE: f32 = 18.;
/// Small print: who, when, how long.
pub const META: f32 = 11.;
/// Headings in what she wrote, by level.
pub const H1: (f32, f32) = (18., 26.);
pub const H2: (f32, f32) = (16., 24.);
pub const H3: (f32, f32) = (14.5, 22.);

/// The steps of space: within a line, between lines that belong together,
/// between blocks, between turns.
pub const S1: f32 = 4.;
pub const S2: f32 = 8.;
pub const S3: f32 = 12.;
pub const S4: f32 = 16.;
pub const S6: f32 = 24.;
/// Between turns.
pub const TURN: f32 = 32.;

/// How round a card, a bubble, a well is.
pub const CARD_RADIUS: f32 = 10.;
pub const BUBBLE_RADIUS: f32 = 16.;
pub const WELL_RADIUS: f32 = 8.;

/// A line of small print under a message or beside a step: who, when,
/// how long.
pub fn meta(faces: &Faces, text: impl Into<gpui::SharedString>) -> Div {
    ui::mono(faces, text)
        .text_size(px(META))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// A well for what was run or printed: mono on the deepest ground.
pub fn well(faces: &Faces) -> Div {
    div()
        .px(px(S3))
        .py(px(10.))
        .rounded(px(WELL_RADIUS))
        .bg(theme::well())
        .border_1()
        .border_color(theme::border())
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .text_color(theme::text2())
}
