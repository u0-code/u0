//! The desk's type and its rhythm, in one place, so that every line of
//! it, in the thread and in the columns beside it, is set from the same
//! few sizes.
//!
//! Three sizes of text do nearly everything in the thread: the body an agent's
//! words, yours and each step are read in, a smaller one for what is said
//! about them (a card's summary), and mono for what was run and what it
//! printed. The columns beside it set their names a half point under the
//! body and their small print smaller still. Space comes in steps of four,
//! and the reading column is about as wide as a line of comfortable prose.

use gpui::{Div, prelude::*, px};

use crate::{fonts::Faces, theme, ui};

/// The reading column, and the composer under it.
pub const COLUMN: f32 = 680.;

// The sizes are measured from 2x captures of Claude's desktop app, the
// standard the desk is held to. Its prose sets a Chinese character 28 px
// wide on lines 42 px apart: 14 on 21, a line and a half, which is what
// makes it read open. At 15 on the same 21 the desk read cramped beside
// it. A line of code takes 38 px there, "3 minutes ago" 24.
//
// Every size text is set at, smallest first. Nothing in the desk sets text
// at a size of its own.

/// Mono text, a menu's head, a tip; a line in a form.
pub const CAPTION: f32 = 12.;
/// Small print in the thread: when, how long; a button's word.
pub const META: f32 = 12.5;
/// Secondary text: a card's summary, a thought, a
/// quiet row's words; a menu's choices.
pub const SMALL: f32 = 13.;
/// Commands, paths and what they printed.
pub const MONO: f32 = 13.;
/// Body text: what you said, the agent's words (in the serif), a step ("Ran
/// python3…"), whose verb is grey and thing is white, which is enough to
/// set it apart.
pub const BODY: f32 = 14.;
/// An agent's prose, in the serif.
pub const PROSE: f32 = 15.;
pub const PROSE_LINE: f32 = 23.;
pub const MONO_LINE: f32 = 19.;
/// Headings in what an agent wrote, by level.
pub const H1: (f32, f32) = (20., 28.);
pub const H2: (f32, f32) = (17., 24.);
pub const H3: (f32, f32) = (15., 22.);

pub const S2: f32 = 8.;
pub const S4: f32 = 16.;

pub const BUBBLE_RADIUS: f32 = 18.;
pub const WELL_RADIUS: f32 = 12.;

/// A line of small print under a message or beside a step: who, when,
/// how long.
pub fn meta(faces: &Faces, text: impl Into<gpui::SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(META))
        .line_height(px(18.))
        .text_color(theme::faint())
}
