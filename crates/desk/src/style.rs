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

// The sizes are measured from 2x captures of Claude's desktop app, the
// standard the desk is held to. Its prose sets a Chinese character 28 px
// wide on lines 42 px apart: 14 on 21, a line and a half, which is what
// makes it read open. At 15 on the same 21 the desk read cramped beside
// it. A line of code takes 38 px there, "3 minutes ago" 24.
//
// Every size text is set at, smallest first: 12, 13, 14 and 15, and the
// headings. Nothing in the desk sets text at a size of its own.

/// Small print: when, how long, a count, a key, a menu's head, a tip.
pub const CAPTION: f32 = 12.;
/// A control's or a row's words: a menu's line, a chip, a name in a list
/// beside the thread, a setting.
pub const LABEL: f32 = 13.;
/// Commands, paths and what they printed.
pub const MONO: f32 = 13.;
/// Body text: what you said, a step ("Ran python3…"), whose verb is grey
/// and thing is white, which is enough to set it apart.
pub const BODY: f32 = 14.;
/// An agent's prose, in the sans, as Claude now sets it.
pub const PROSE: f32 = 15.;
pub const PROSE_LINE: f32 = 23.;
pub const MONO_LINE: f32 = 19.;
/// The size of headings in what an agent wrote, by level.
pub const H1: f32 = 20.;
pub const H2: f32 = 17.;
pub const H3: f32 = 15.;

pub const S2: f32 = 8.;

pub const BUBBLE_RADIUS: f32 = 18.;

/// A line of small print under a message or beside a step: who, when,
/// how long.
pub fn meta(faces: &Faces, text: impl Into<gpui::SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(CAPTION))
        .line_height(px(18.))
        .text_color(theme::faint())
}
