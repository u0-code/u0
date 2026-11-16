//! The look: a dark ground, warm text, and one amber accent that is used
//! only where she is alive. Everything else is told apart by lightness.
//!
//! The journal's kinds each have a fixed colour, the same on every page, so
//! that a glance at a column says what sort of day it was.

use gpui::{Hsla, rgb};

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// The window's ground.
pub fn bg() -> Hsla {
    c(0x1111_10)
}
/// The rail down the left.
pub fn rail() -> Hsla {
    c(0x0D0D_0C)
}
/// A panel on the ground.
pub fn panel() -> Hsla {
    c(0x1515_14)
}
/// A panel's deeper inset, or a selected row.
pub fn panel2() -> Hsla {
    c(0x1D1D_1B)
}
/// Lines between things.
pub fn border() -> Hsla {
    c(0x2222_20)
}
/// A stronger line: a control's edge.
pub fn border2() -> Hsla {
    c(0x2A2A_28)
}
/// A hover.
pub fn hover() -> Hsla {
    c(0x1A1A_19)
}
/// Text.
pub fn text() -> Hsla {
    c(0xE9E5_DC)
}
/// Text that matters a little less.
pub fn text2() -> Hsla {
    c(0xB8B3_A8)
}
/// Labels, times, hashes.
pub fn muted() -> Hsla {
    c(0x8F8B_81)
}
/// The faintest readable grey: footnotes and hashes.
pub fn faint() -> Hsla {
    c(0x5F5B_52)
}
/// A disabled or empty mark.
pub fn ghost_line() -> Hsla {
    c(0x3A3A_37)
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
/// Something she kept.
pub fn amber2() -> Hsla {
    c(0xC7A8_6E)
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
