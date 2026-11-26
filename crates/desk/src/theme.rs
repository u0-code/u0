//! The look: neutral grounds (dark, or light where you choose it or the
//! system is), four greys of text, and one amber
//! accent that is used only where something is going on now. Everything else is told
//! apart by lightness. The values are measured from Claude's desktop app,
//! which the desk is held against pane by pane.
//! None of the grounds is black: the sidebar is a shade lighter than the
//! thread and the right column, the way a coding agent's app sets its
//! panes apart, and all of them are easy on the eyes for a day's use.

use gpui::{App, Hsla, px, rgb};

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Whether the light look is on: as the settings and the system say, set
/// by `set_light` as the window is drawn.
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the look, and with it gpui-kit's, when it changed.
pub fn set_light(on: bool, cx: &mut App) {
    if LIGHT.swap(on, std::sync::atomic::Ordering::Relaxed) != on {
        kit(cx);
    }
}

/// Whether the window is drawn in the light look now.
pub fn is_light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether lines added and taken away are blue and orange (told apart
/// without red and green), as the settings say.
static BLUE_ORANGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_blue_orange(on: bool) {
    BLUE_ORANGE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether what matters less is drawn stronger: set from the settings.
static CONTRAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_contrast(on: bool) {
    CONTRAST.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the grounds let the desktop show through: set from the settings.
static GLASS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_glass(on: bool) {
    GLASS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the grounds let the desktop show through.
pub fn glass() -> bool {
    GLASS.load(std::sync::atomic::Ordering::Relaxed)
}

/// A ground, see-through where glass is on.
fn ground(dark: u32, light: u32) -> Hsla {
    glassy(pick(dark, light))
}

/// `colour`, see-through where glass is on.
fn glassy(colour: Hsla) -> Hsla {
    if glass() {
        colour.opacity(0.82)
    } else {
        colour
    }
}

/// The colours of a theme of yours, for the look now drawn, as the
/// functions below give Ghost's own: each read from what gpui-kit made of
/// the theme.
#[derive(Clone, Copy)]
struct Palette {
    bg: Hsla,
    rail: Hsla,
    panel: Hsla,
    bubble: Hsla,
    panel2: Hsla,
    well: Hsla,
    border: Hsla,
    border2: Hsla,
    hover: Hsla,
    text: Hsla,
    text2: Hsla,
    muted: Hsla,
    faint: Hsla,
    quiet: Hsla,
    ok: Hsla,
    accent: Hsla,
    coral: Hsla,
    program: Hsla,
    argument: Hsla,
    number: Hsla,
    link: Hsla,
}

impl Palette {
    fn of(c: &gpui::component::theme::ThemeColor) -> Self {
        Self {
            bg: c.background,
            rail: c.sidebar,
            panel: c.popover,
            bubble: c.secondary,
            panel2: c.muted,
            well: c.background.blend(c.foreground.opacity(0.03)),
            border: c.border,
            border2: c.input,
            hover: c.secondary_hover,
            text: c.foreground,
            text2: c.foreground.blend(c.muted_foreground.opacity(0.5)),
            muted: c.muted_foreground,
            faint: c.muted_foreground.opacity(0.85),
            quiet: c.muted_foreground.opacity(0.6),
            ok: c.success,
            accent: c.warning,
            coral: c.danger,
            program: c.blue,
            argument: c.green,
            number: c.yellow,
            link: c.link,
        }
    }
}

/// The colours of the theme of yours drawn now; none, Ghost's own.
static THEMED: std::sync::RwLock<Option<Palette>> = std::sync::RwLock::new(None);

/// A colour of the theme of yours drawn now, where there is one.
fn themed(of: fn(&Palette) -> Hsla) -> Option<Hsla> {
    THEMED.read().expect("the theme").as_ref().map(of)
}

/// The themes chosen for the dark look and the light, by name; none,
/// Ghost's own.
static CHOSEN: std::sync::Mutex<(Option<String>, Option<String>)> =
    std::sync::Mutex::new((None, None));

/// Choose the themes for the dark look and the light, as the settings
/// say; the look takes them as gpui-kit's is set (`kit`).
pub fn set_themes(dark: Option<String>, light: Option<String>) {
    *CHOSEN.lock().expect("the themes") = (dark, light);
}

/// `normal`, or `strong` with more contrast; each dark then light.
fn step(normal: (u32, u32), strong: (u32, u32)) -> Hsla {
    let (dark, light) = if CONTRAST.load(std::sync::atomic::Ordering::Relaxed) {
        strong
    } else {
        normal
    };
    pick(dark, light)
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
    c(if is_light() { light } else { dark })
}

/// The window's ground: the thread and the right column.
pub fn bg() -> Hsla {
    themed(|p| p.bg).map_or_else(|| ground(0x1919_19, 0xFAFA_F9), glassy)
}
/// The rail down the left, a shade lighter than the ground.
pub fn rail() -> Hsla {
    themed(|p| p.rail).map_or_else(|| ground(0x1F1F_1F, 0xF2F2_F0), glassy)
}
/// A panel on the ground: the composer, a menu, a sheet.
pub fn panel() -> Hsla {
    themed(|p| p.panel).unwrap_or_else(|| pick(0x2626_26, 0xFFFF_FF))
}
/// What you said, in its bubble, and a name in code in an agent's prose: a
/// shade over the ground, as light as Claude's app keeps it (#252525 on
/// #191919), not a panel's grey.
pub fn bubble() -> Hsla {
    themed(|p| p.bubble).unwrap_or_else(|| pick(0x2525_25, 0xEFEF_ED))
}
/// A panel's deeper inset, or a selected row.
pub fn panel2() -> Hsla {
    themed(|p| p.panel2).unwrap_or_else(|| pick(0x3434_34, 0xE6E6_E4))
}
/// An inset under the ground: a field, a search box.
pub fn well() -> Hsla {
    themed(|p| p.well).unwrap_or_else(|| pick(0x1414_14, 0xF6F6_F5))
}
/// Lines between things, and the frame of a card or a box in the thread:
/// barely there, as in Claude's app.
pub fn border() -> Hsla {
    themed(|p| p.border).unwrap_or_else(|| step((0x2222_22, 0xE5E5_E3), (0x3838_38, 0xCFCF_CC)))
}
/// The edge of a chip or a small framed control.
pub fn tag_line() -> Hsla {
    themed(|p| p.border).unwrap_or_else(|| pick(0x2E2E_2E, 0xDADA_D8))
}
/// A stronger line: a control's edge.
pub fn border2() -> Hsla {
    themed(|p| p.border2).unwrap_or_else(|| step((0x3838_38, 0xCFCF_CC), (0x5050_50, 0xA8A8_A4)))
}
/// A hover, on the ground or on the rail.
pub fn hover() -> Hsla {
    themed(|p| p.hover).unwrap_or_else(|| pick(0x2A2A_2A, 0xEDED_EB))
}
/// Text.
/// A row in the sidebar under the pointer, open, and chosen among others:
/// the text's colour over the ground, as t3code mixes them.
pub fn row_hover() -> Hsla {
    text().opacity(0.08)
}

pub fn row_open() -> Hsla {
    text().opacity(0.11)
}

pub fn row_chosen() -> Hsla {
    text().opacity(0.07)
}

pub fn text() -> Hsla {
    themed(|p| p.text).unwrap_or_else(|| pick(0xEFEF_EF, 0x1C1C_1C))
}
/// Text that matters a little less.
pub fn text2() -> Hsla {
    themed(|p| p.text2).unwrap_or_else(|| step((0xB4B4_B4, 0x4747_47), (0xD6D6_D6, 0x2E2E_2E)))
}
/// Labels, a step's verb, what a command printed.
pub fn muted() -> Hsla {
    themed(|p| p.muted).unwrap_or_else(|| step((0x9C9C_9C, 0x5A5A_5A), (0xBDBD_BD, 0x4A4A_4A)))
}
/// The faintest readable grey: times, durations, footnotes. Still 4.5:1
/// (WCAG AA) on a panel and on the rail, for the small text it is.
pub fn faint() -> Hsla {
    themed(|p| p.faint).unwrap_or_else(|| step((0x8E8E_8E, 0x6E6E_6E), (0xA8A8_A8, 0x5757_57)))
}
/// Below faint, for what is there only to be skipped: line numbers, the
/// "… n lines" of a fold, a plan's struck and empty marks, work dropped.
pub fn quiet() -> Hsla {
    themed(|p| p.quiet).unwrap_or_else(|| pick(0x5E5E_5E, 0xA6A6_A6))
}
/// A disabled or empty mark.
pub fn ghost_line() -> Hsla {
    themed(|p| p.border).unwrap_or_else(|| pick(0x4A4A_4A, 0xBDBD_BA))
}
/// Done, passed, a line added.
pub fn ok() -> Hsla {
    themed(|p| p.ok).unwrap_or_else(|| pick(0x8FBF_8A, 0x3D8A_3A))
}
/// The one accent: something going on now, a question waiting on you.
pub fn accent() -> Hsla {
    themed(|p| p.accent).unwrap_or_else(|| pick(0xE0A3_5A, 0xB06E_1B))
}
/// Trouble, calmly; a removal in a diff.
pub fn coral() -> Hsla {
    themed(|p| p.coral).unwrap_or_else(|| pick(0xD98C_7A, 0xBF51_3A))
}
/// The program at the head of a command, and a keyword in code.
pub fn code_program() -> Hsla {
    themed(|p| p.program).unwrap_or_else(|| pick(0x81B6_F9, 0x1D6A_C9))
}
/// Everything after the program: arguments, quoted strings.
pub fn code_argument() -> Hsla {
    themed(|p| p.argument).unwrap_or_else(|| pick(0xACE7_75, 0x4B84_19))
}

/// A number in code.
pub fn code_number() -> Hsla {
    themed(|p| p.number).unwrap_or_else(|| pick(0xE5B7_7A, 0xA35C_17))
}

/// A link, as Claude's app draws one.
pub fn link() -> Hsla {
    themed(|p| p.link).unwrap_or_else(|| pick(0x79A9_EB, 0x2C6B_CC))
}

/// The colour the logo of the agent with this id is drawn in
/// (`registry::KNOWN`).
pub fn brand(id: &str) -> Hsla {
    crate::registry::known(id)
        .and_then(|k| k.3)
        .map_or_else(text, |(dark, light)| pick(dark, light))
}

/// gpui-kit's theme, in the desk's look: its components take their
/// colours, faces and corners from here, as Ghost's own draws do from the
/// functions above. Set as Ghost starts and whenever the look changes.
pub fn kit(cx: &mut App) {
    use gpui::component::theme::{Theme, ThemeMode};
    let faces = crate::fonts::faces(cx).clone();
    // A change of mode loads gpui-kit's own colours for it: first that,
    // then the desk's over them.
    // The theme chosen for this look, else gpui-kit's own for it, which
    // the desk's colours then go over.
    let chosen = {
        let names = CHOSEN.lock().expect("the themes");
        if is_light() {
            names.1.clone()
        } else {
            names.0.clone()
        }
    };
    let config = chosen.and_then(|name| crate::themes::find(cx, &name));
    let registry = gpui::component::theme::ThemeRegistry::global(cx);
    let own = if is_light() {
        registry.default_light_theme().clone()
    } else {
        registry.default_dark_theme().clone()
    };
    let light = is_light();
    let mode_theme = config.clone().unwrap_or(own);
    Theme::update(cx, |t| {
        if light {
            t.light_theme = mode_theme;
        } else {
            t.dark_theme = mode_theme;
        }
    });
    Theme::change(
        if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        None,
        cx,
    );
    *THEMED.write().expect("the theme") = config.map(|_| Palette::of(&Theme::global(cx).colors));
    Theme::update(cx, |t| {
        t.font_family = faces.ui;
        t.mono_font_family = faces.mono;
        t.font_size = px(crate::style::BODY);
        t.radius = px(8.);
        t.radius_lg = px(12.);
        // Notices sit at the foot, clear of the composer, not over the
        // controls at the top of a column.
        t.notification.placement = gpui::Anchor::BottomRight;
        t.notification.margins.bottom = px(96.);
        let c = &mut t.colors;
        c.background = bg();
        c.foreground = text();
        c.border = border2();
        c.input = border2();
        c.ring = border2();
        c.caret = text();
        c.selection = link().opacity(0.3);
        c.popover = panel();
        c.popover_foreground = text();
        c.muted = panel2();
        c.muted_foreground = muted();
        c.accent = hover();
        c.accent_foreground = text();
        c.primary = text();
        c.primary_hover = text2();
        c.primary_active = text2();
        c.primary_foreground = bg();
        c.secondary = hover();
        c.secondary_hover = hover();
        c.secondary_active = panel2();
        c.secondary_foreground = text2();
        c.button = gpui::transparent_black();
        c.button_hover = hover();
        c.button_active = panel2();
        c.button_foreground = text();
        c.button_primary = text();
        c.button_primary_hover = text2();
        c.button_primary_active = text2();
        c.button_primary_foreground = bg();
        c.sidebar = rail();
        c.sidebar_foreground = text();
        c.sidebar_border = border();
        c.sidebar_accent = panel2();
        c.sidebar_accent_foreground = text();
        c.tab_bar_segmented = panel2();
        c.tab_foreground = text2();
        c.tab_active_foreground = text();
        c.switch = ghost_line();
        c.switch_thumb = bg();
        // What a diff adds and takes away, in the colours chosen for it.
        c.danger = gone();
        c.success = added();
        c.warning = accent();
        c.link = link();
        c.link_hover = link();
        c.link_active = link();
        c.list_hover = row_hover();
        c.list_active = row_open();
        c.overlay = bg().opacity(0.64);
    });
}
