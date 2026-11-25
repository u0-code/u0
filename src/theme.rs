//! The look: neutral grounds (dark, or light where you choose it or the
//! system is), four greys of text, and one amber
//! accent that is used only where something is going on now. Everything else is told
//! apart by lightness. The values are measured from Claude's desktop app,
//! which the desk is held against pane by pane.
//! None of the grounds is black: the sidebar is a shade lighter than the
//! thread and the right column, the way a coding agent's app sets its
//! panes apart, and all of them are easy on the eyes for a day's use.

use gpui::{Hsla, rgb};

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Whether the light look is on: as the settings and the system say, set
/// by `set_light` as the window is drawn.
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_light(on: bool) {
    LIGHT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether lines added and taken away are blue and orange (told apart
/// without red and green), as the settings say.
static BLUE_ORANGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_blue_orange(on: bool) {
    BLUE_ORANGE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The colour of what a change added: green, or blue.
pub fn added() -> Hsla {
    if BLUE_ORANGE.load(std::sync::atomic::Ordering::Relaxed) {
        pick(0x6FA8_F0, 0x1F6F_C8)
    } else {
        ok()
    }
}

/// The colour of what a change took away: coral, or orange.
pub fn gone() -> Hsla {
    if BLUE_ORANGE.load(std::sync::atomic::Ordering::Relaxed) {
        pick(0xE8A0_4C, 0xB363_0F)
    } else {
        coral()
    }
}

/// The colour for the dark look, or its like for the light one.
fn pick(dark: u32, light: u32) -> Hsla {
    c(if LIGHT.load(std::sync::atomic::Ordering::Relaxed) {
        light
    } else {
        dark
    })
}

/// The window's ground: the thread and the right column.
pub fn bg() -> Hsla {
    pick(0x1919_19, 0xFAFA_F9)
}
/// The rail down the left, a shade lighter than the ground.
pub fn rail() -> Hsla {
    pick(0x1F1F_1F, 0xF2F2_F0)
}
/// A panel on the ground: the composer, a menu, a sheet.
pub fn panel() -> Hsla {
    pick(0x2626_26, 0xFFFF_FF)
}
/// What you said, in its bubble, and a name in code in an agent's prose: a
/// shade over the ground, as light as Claude's app keeps it (#252525 on
/// #191919), not a panel's grey.
pub fn bubble() -> Hsla {
    pick(0x2525_25, 0xEFEF_ED)
}
/// The rules of a table, under its head and between its rows.
pub fn rule() -> Hsla {
    pick(0x3030_30, 0xE0E0_DE)
}
/// A panel's deeper inset, or a selected row.
pub fn panel2() -> Hsla {
    pick(0x3434_34, 0xE6E6_E4)
}
/// An inset under the ground: a field, a search box.
pub fn well() -> Hsla {
    pick(0x1414_14, 0xF6F6_F5)
}
/// Lines between things, and the frame of a card or a box in the thread:
/// barely there, as in Claude's app.
pub fn border() -> Hsla {
    pick(0x2222_22, 0xE5E5_E3)
}
/// The edge of a chip or a small framed control.
pub fn tag_line() -> Hsla {
    pick(0x2E2E_2E, 0xDADA_D8)
}
/// A stronger line: a control's edge.
pub fn border2() -> Hsla {
    pick(0x3838_38, 0xCFCF_CC)
}
/// A hover, on the ground or on the rail.
pub fn hover() -> Hsla {
    pick(0x2A2A_2A, 0xEDED_EB)
}
/// Text.
pub fn text() -> Hsla {
    pick(0xEFEF_EF, 0x1C1C_1C)
}
/// Text that matters a little less.
pub fn text2() -> Hsla {
    pick(0xB4B4_B4, 0x4747_47)
}
/// Labels, a step's verb, what a command printed.
pub fn muted() -> Hsla {
    pick(0x9C9C_9C, 0x6666_66)
}
/// The faintest readable grey: times, durations, footnotes.
pub fn faint() -> Hsla {
    pick(0x7575_75, 0x8686_86)
}
/// Below faint, for what is there only to be skipped: line numbers, the
/// "… n lines" of a fold, a plan's struck and empty marks, work dropped.
pub fn quiet() -> Hsla {
    pick(0x5E5E_5E, 0xA6A6_A6)
}
/// A disabled or empty mark.
pub fn ghost_line() -> Hsla {
    pick(0x4A4A_4A, 0xBDBD_BA)
}
/// Done, passed, a line added.
pub fn ok() -> Hsla {
    pick(0x8FBF_8A, 0x3D8A_3A)
}
/// The one accent: something going on now, a question waiting on you.
pub fn accent() -> Hsla {
    pick(0xE0A3_5A, 0xB06E_1B)
}
/// Trouble, calmly; a removal in a diff.
pub fn coral() -> Hsla {
    pick(0xD98C_7A, 0xBF51_3A)
}
/// The words of code in a diff, a shade under text so the signs and
/// grounds around them carry the change.
pub fn code_text() -> Hsla {
    pick(0xE6E6_E6, 0x2626_26)
}
/// The program at the head of a command, and a keyword in code.
pub fn code_program() -> Hsla {
    pick(0x81B6_F9, 0x1D6A_C9)
}
/// Everything after the program: arguments, quoted strings.
pub fn code_argument() -> Hsla {
    pick(0xACE7_75, 0x4B84_19)
}

/// A number in code.
pub fn code_number() -> Hsla {
    pick(0xE5B7_7A, 0xA35C_17)
}
/// A function's name in code.
pub fn code_function() -> Hsla {
    pick(0xC9A6_F2, 0x7A4B_BE)
}

/// A link, as Claude's app draws one.
pub fn link() -> Hsla {
    pick(0x79A9_EB, 0x2C6B_CC)
}

/// The line under a link: a shade darker than its words.
pub fn link_line() -> Hsla {
    pick(0x3F53_6D, 0xB5C9_E8)
}

/// The colour an agent's logo is drawn in: GPUI draws an icon in one
/// colour, so each gets its brand's own, by registry id. A brand whose
/// mark is black is drawn in the text colour, as on a dark ground.
pub fn brand(id: &str) -> Hsla {
    match id {
        "claude-acp" => c(0xD977_57),
        "antigravity-acp" => c(0x8FBD_FF),
        "gemini" => c(0xA99B_FA),
        "opencode" => c(0xB4B4_B4),
        "github-copilot-cli" => c(0xC4C4_C4),
        _ => text(),
    }
}

/// The name an agent goes by in the desk: the product's own short name
/// where the registry's is longer ("Claude Agent" is Claude Code), else the
/// registry's.
pub fn agent_name<'a>(id: &str, registry: &'a str) -> std::borrow::Cow<'a, str> {
    match id {
        "claude-acp" => "Claude Code".into(),
        "antigravity-acp" => "Antigravity".into(),
        "github-copilot-cli" => "Copilot CLI".into(),
        _ => registry.into(),
    }
}
