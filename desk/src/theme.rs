//! The look: dark neutral grounds, four greys of text, and one amber
//! accent that is used only where she is alive. Everything else is told
//! apart by lightness. The values are measured from Claude's desktop app,
//! which the desk is held against pane by pane.
//! None of the grounds is black: the sidebar is a shade lighter than the
//! thread and the right column, the way a coding agent's app sets its
//! panes apart, and all of them are easy on the eyes for a day's use.
//!
//! The journal's kinds each have a fixed colour, the same on every page, so
//! that a glance at a column says what sort of day it was.

use gpui::{Hsla, rgb};

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// The window's ground: the thread and the right column.
pub fn bg() -> Hsla {
    c(0x1919_19)
}
/// The rail down the left, a shade lighter than the ground.
pub fn rail() -> Hsla {
    c(0x1F1F_1F)
}
/// A panel on the ground: the composer, a menu, a sheet.
pub fn panel() -> Hsla {
    c(0x2626_26)
}
/// What you said, in its bubble, and a name in code in her prose: a
/// shade over the ground, as light as Claude's app keeps it (#252525 on
/// #191919), not a panel's grey.
pub fn bubble() -> Hsla {
    c(0x2525_25)
}
/// The rules of a table, under its head and between its rows.
pub fn rule() -> Hsla {
    c(0x3030_30)
}
/// A panel's deeper inset, or a selected row.
pub fn panel2() -> Hsla {
    c(0x3434_34)
}
/// An inset a little under the window's ground, in the side columns and
/// sheets. The thread draws no grounds of its own: its code sits in frames.
pub fn well() -> Hsla {
    c(0x1414_14)
}
/// Lines between things, and the frame of a card or a box in the thread:
/// barely there, as in Claude's app.
pub fn border() -> Hsla {
    c(0x2222_22)
}
/// A stronger line: a control's edge.
pub fn border2() -> Hsla {
    c(0x3838_38)
}
/// A hover, on the ground or on the rail.
pub fn hover() -> Hsla {
    c(0x2A2A_2A)
}
/// Text.
pub fn text() -> Hsla {
    c(0xEFEF_EF)
}
/// Text that matters a little less.
pub fn text2() -> Hsla {
    c(0xB4B4_B4)
}
/// Labels, a step's verb, what a command printed.
pub fn muted() -> Hsla {
    c(0x9C9C_9C)
}
/// The faintest readable grey: times, durations, footnotes.
pub fn faint() -> Hsla {
    c(0x7575_75)
}
/// A disabled or empty mark.
pub fn ghost_line() -> Hsla {
    c(0x4A4A_4A)
}
/// The one accent: her being awake, the prompt's chevron.
pub fn accent() -> Hsla {
    c(0xE0A3_5A)
}
/// Connected, verified, woke.
pub fn green() -> Hsla {
    c(0x8FBF_8A)
}
/// Work: taken on, worked, folded, reported.
pub fn blue() -> Hsla {
    c(0x8AB4_D8)
}
/// A directive from an operator.
pub fn purple() -> Hsla {
    c(0xB49A_D6)
}
/// Her persona being revised; a removal in a diff.
pub fn coral() -> Hsla {
    c(0xD98C_7A)
}
/// The program at the head of a command, and a keyword in code.
pub fn code_program() -> Hsla {
    c(0x81B6_F9)
}
/// Everything after the program: arguments, quoted strings.
pub fn code_argument() -> Hsla {
    c(0xACE7_75)
}

/// Something she kept.
pub fn amber2() -> Hsla {
    c(0xC7A8_6E)
}

/// The colour each brain's mark is drawn in: GPUI draws an icon in one
/// colour, so each gets its brand's own. A brand whose mark is black is
/// drawn in the text colour, as it would be on a dark ground.
pub fn brand(icon: &str) -> Hsla {
    match icon {
        "claude" => c(0xD977_57),
        "deepseek" => c(0x4D6B_FE),
        "gemini" => c(0x3186_FF),
        "openrouter" => c(0x7B7C_F4),
        "qwen" => c(0x7D79_F0),
        "zhipu" => c(0x5B78_FF),
        "mistral" => c(0xFA52_0F),
        "groq" => c(0xF550_36),
        "plug" => muted(),
        _ => text(),
    }
}

/// The colour a journal kind is always shown in.
pub fn kind_color(kind: &str) -> Hsla {
    match kind {
        "said" | "posted" | "reacted" => text(),
        "kept" | "viewed" => amber2(),
        "undertook" | "worked" | "reported" | "concluded" | "asked" | "added" => blue(),
        "directed" => purple(),
        "woke" | "slept" | "born" => green(),
        "described" | "canon" => coral(),
        _ => muted(),
    }
}
