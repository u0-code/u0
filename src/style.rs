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

use gpui::{Div, div, prelude::*, px};

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

/// Small print in the columns beside the thread: a value at a row's
/// right, a time, a line under a name.
pub const FINE: f32 = 11.5;
/// Mono text, a menu's head, a tip; a line in a form.
pub const CAPTION: f32 = 12.;
/// Small print in the thread: when, how long; a button's word.
pub const META: f32 = 12.5;
/// Secondary text: a card's summary, a thought, a
/// quiet row's words; a menu's choices.
pub const SMALL: f32 = 13.;
/// Commands, paths and what they printed.
pub const MONO: f32 = 13.;
/// A name in a list: a session, an agent, a card's title in the right
/// column; and what a step was about, when it is in mono, which reads
/// smaller than the interface's face at the same size.
pub const NAME: f32 = 13.5;
/// Body text: what you said, the agent's words (in the serif), a step ("Ran
/// python3…"), whose verb is grey and thing is white, which is enough to
/// set it apart.
pub const BODY: f32 = 14.;
pub const BODY_LINE: f32 = 21.;
pub const MONO_LINE: f32 = 19.;
/// Headings in what an agent wrote, by level.
pub const H1: (f32, f32) = (20., 28.);
pub const H2: (f32, f32) = (17., 24.);
pub const H3: (f32, f32) = (15., 21.);
/// The one line on an empty thread, in the serif.
pub const DISPLAY: f32 = 22.;

/// The steps of space: within a line, between lines that belong together,
/// between blocks, between turns.
pub const S1: f32 = 4.;
pub const S2: f32 = 8.;
pub const S3: f32 = 12.;
pub const S4: f32 = 16.;
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
    ui::text(faces, text)
        .text_size(px(META))
        .line_height(px(18.))
        .text_color(theme::faint())
}

/// A row of the thread, the size and inset of a step's: 30 high, its
/// words 10 in from the column, so that every row lines up with the
/// steps above and below it.
pub fn row() -> Div {
    div().flex().items_center().h(px(30.)).px(px(10.))
}

/// The box an opened step is drawn in, and a fenced block, and what an agent
/// said or posted out there: a hairline frame on the thread's ground.
pub fn boxed() -> Div {
    div()
        .flex()
        .flex_col()
        .px(px(S3))
        .pt(px(14.))
        .pb(px(S4))
        .rounded(px(BOX_RADIUS))
        .border_1()
        .border_color(theme::border())
}
