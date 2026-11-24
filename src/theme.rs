//! The look: dark neutral grounds, four greys of text, and one amber
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
/// What you said, in its bubble, and a name in code in an agent's prose: a
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
/// An inset under the ground: a field, a search box.
pub fn well() -> Hsla {
    c(0x1414_14)
}
/// Lines between things, and the frame of a card or a box in the thread:
/// barely there, as in Claude's app.
pub fn border() -> Hsla {
    c(0x2222_22)
}
/// The edge of a chip or a small framed control.
pub fn tag_line() -> Hsla {
    c(0x2E2E_2E)
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
/// Below faint, for what is there only to be skipped: line numbers, the
/// "… n lines" of a fold, a plan's struck and empty marks, work dropped.
pub fn quiet() -> Hsla {
    c(0x5E5E_5E)
}
/// A disabled or empty mark.
pub fn ghost_line() -> Hsla {
    c(0x4A4A_4A)
}
/// Done, passed, a line added.
pub fn ok() -> Hsla {
    c(0x8FBF_8A)
}
/// The one accent: something going on now, a question waiting on you.
pub fn accent() -> Hsla {
    c(0xE0A3_5A)
}
/// Trouble, calmly; a removal in a diff.
pub fn coral() -> Hsla {
    c(0xD98C_7A)
}
/// The words of code in a diff, a shade under text so the signs and
/// grounds around them carry the change.
pub fn code_text() -> Hsla {
    c(0xE6E6_E6)
}
/// The program at the head of a command, and a keyword in code.
pub fn code_program() -> Hsla {
    c(0x81B6_F9)
}
/// Everything after the program: arguments, quoted strings.
pub fn code_argument() -> Hsla {
    c(0xACE7_75)
}

/// A number in code.
pub fn code_number() -> Hsla {
    c(0xE5B7_7A)
}
/// A function's name in code.
pub fn code_function() -> Hsla {
    c(0xC9A6_F2)
}

/// A link, as Claude's app draws one.
pub fn link() -> Hsla {
    c(0x79A9_EB)
}

/// The line under a link: a shade darker than its words.
pub fn link_line() -> Hsla {
    c(0x3F53_6D)
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
