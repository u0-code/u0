//! Carbon v11 theming over `u0ui::theme` (`White`, `G10`, `G90`, `G100`),
//! with `G100` as the default dark theme and `White` as the default light
//! theme, plus custom themes from `<home>/themes/`.
//!
//! `u0ui::theme::set_theme` owns the active `CarbonTheme` and projects Carbon
//! colours, geometry, IBM Plex and motion into `gpui-kit`'s global `Theme`.
//! Theme writes (`kit`, `set_light`) run only from startup, settings updates
//! and window-appearance observers — never from `Render::render`.

use std::collections::HashMap;
use std::sync::{
    Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};

use gpui::{App, Hsla, px, rgb};
pub use u0ui::tokens::{CarbonTheme, ColorRole};

/// Whether the window is in the light mode (`Look::Light`, or `Look::System`
/// when macOS is light). Default `false` (`Look::Dark` → `CarbonTheme::G100`).
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Whether diff additions and removals use blue and orange instead of green and red.
static BLUE_ORANGE: AtomicBool = AtomicBool::new(false);

/// Whether borders and secondary text step up one Carbon contrast level.
static CONTRAST: AtomicBool = AtomicBool::new(false);

/// Whether window surfaces let the desktop show through (`sidebar-material`).
static GLASS: AtomicBool = AtomicBool::new(false);

/// Chosen theme names `(dark, light)` from `Settings`; `None` selects the
/// default Carbon theme for that mode (`G100` for dark, `White` for light).
static CHOSEN: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));

/// Role overrides when a custom user theme (`<home>/themes/*.json`) is active.
static CUSTOM: RwLock<Option<HashMap<ColorRole, Hsla>>> = RwLock::new(None);

/// Built-in Carbon themes for dark and light modes, in picker order.
pub const DARK_CARBON: [CarbonTheme; 2] = [CarbonTheme::G100, CarbonTheme::G90];
pub const LIGHT_CARBON: [CarbonTheme; 2] = [CarbonTheme::White, CarbonTheme::G10];

/// Human-readable display label of a built-in Carbon theme (`"Carbon G100"`, …).
pub const fn carbon_label(theme: CarbonTheme) -> &'static str {
    match theme {
        CarbonTheme::White => "Carbon White",
        CarbonTheme::G10 => "Carbon G10",
        CarbonTheme::G90 => "Carbon G90",
        CarbonTheme::G100 => "Carbon G100",
    }
}

/// Resolve a built-in Carbon theme name or id (`"g100"`, `"Carbon G100"`,
/// `"Gray 100"`, …). `None` returns `None`.
pub fn parse_carbon(name: &str) -> Option<CarbonTheme> {
    let trimmed = name.trim();
    CarbonTheme::from_id(trimmed).or_else(|| {
        CarbonTheme::ALL.into_iter().find(|t| {
            t.label().eq_ignore_ascii_case(trimmed)
                || carbon_label(*t).eq_ignore_ascii_case(trimmed)
        })
    })
}

/// The Carbon theme for `dark` given `chosen`: defaults to `G100` for dark
/// and `White` for light; returns `None` only when `chosen` names a custom
/// theme file rather than one of Carbon's four.
pub fn carbon_for(dark: bool, chosen: Option<&str>) -> Option<CarbonTheme> {
    match chosen {
        None => Some(if dark {
            CarbonTheme::G100
        } else {
            CarbonTheme::White
        }),
        Some(name) => parse_carbon(name),
    }
}

/// Update light/dark mode and re-project the theme if it changed. Call from
/// startup, event handlers, or window observers — never inside `render`.
pub fn set_light(on: bool, cx: &mut App) {
    if LIGHT.swap(on, Ordering::Relaxed) != on {
        kit(cx);
    }
}

/// Whether the window is drawn in light mode now.
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

pub fn set_blue_orange(on: bool) {
    BLUE_ORANGE.store(on, Ordering::Relaxed);
}

pub fn blue_orange() -> bool {
    BLUE_ORANGE.load(Ordering::Relaxed)
}

pub fn set_contrast(on: bool) {
    CONTRAST.store(on, Ordering::Relaxed);
}

pub fn contrast() -> bool {
    CONTRAST.load(Ordering::Relaxed)
}

pub fn set_glass(on: bool) {
    GLASS.store(on, Ordering::Relaxed);
}

pub fn glass() -> bool {
    GLASS.load(Ordering::Relaxed)
}

pub fn set_themes(dark: Option<String>, light: Option<String>) {
    *CHOSEN.lock().expect("the themes") = (dark, light);
}

/// Read a Carbon `ColorRole` in the active theme, with any custom user-theme
/// override applied on top.
pub fn role(r: ColorRole) -> Hsla {
    if let Some(over) = CUSTOM.read().expect("custom theme").as_ref()
        && let Some(&c) = over.get(&r)
    {
        return c;
    }
    u0ui::theme::color(r)
}

fn glassy(colour: Hsla) -> Hsla {
    if glass() {
        colour.opacity(0.82)
    } else {
        colour
    }
}

// ── Semantic role helpers used across u0-desk ───────────────────────────

/// Page ground (`background`).
pub fn bg() -> Hsla {
    glassy(role(ColorRole::Background))
}

/// Left sidebar surface (`sidebar-material` when glass is on, else `layer-01`).
pub fn rail() -> Hsla {
    if glass() {
        role(ColorRole::SidebarMaterial)
    } else {
        role(ColorRole::Layer01)
    }
}

/// Elevated panel / menu / card on `background` (`layer-01`).
pub fn panel() -> Hsla {
    role(ColorRole::Layer01)
}

/// User message bubble (`chat-bubble-user`).
pub fn bubble() -> Hsla {
    role(ColorRole::ChatBubbleUser)
}

/// Nested surface on `layer-01` (`layer-02`).
pub fn panel2() -> Hsla {
    role(ColorRole::Layer02)
}

/// Input / field surface on `background` (`field-01`).
pub fn well() -> Hsla {
    role(ColorRole::Field01)
}

/// Hairline structural divider (`border-subtle-01`, or `border-strong-01` with high contrast).
pub fn border() -> Hsla {
    role(if contrast() {
        ColorRole::BorderStrong01
    } else {
        ColorRole::BorderSubtle01
    })
}

/// Chip / tile border (`border-subtle-02`).
pub fn tag_line() -> Hsla {
    role(ColorRole::BorderSubtle02)
}

/// Control boundary / field bottom rule (`border-strong-01`, or `border-inverse` with high contrast).
pub fn border2() -> Hsla {
    role(if contrast() {
        ColorRole::BorderInverse
    } else {
        ColorRole::BorderStrong01
    })
}

/// Hover wash on `background` (`background-hover`).
pub fn hover() -> Hsla {
    role(ColorRole::BackgroundHover)
}

/// Hover wash on a sidebar or list row (`layer-hover-01`).
pub fn row_hover() -> Hsla {
    role(ColorRole::LayerHover01)
}

/// Primary text (`text-primary`).
pub fn text() -> Hsla {
    role(ColorRole::TextPrimary)
}

/// Secondary text (`text-secondary`, or `text-primary` with high contrast).
pub fn text2() -> Hsla {
    role(if contrast() {
        ColorRole::TextPrimary
    } else {
        ColorRole::TextSecondary
    })
}

/// Labels and descriptions (`text-secondary`, or `text-primary` with high contrast).
pub fn muted() -> Hsla {
    text2()
}

/// Helper text, timestamps and metadata (`text-helper`, or `text-secondary` with high contrast).
pub fn faint() -> Hsla {
    role(if contrast() {
        ColorRole::TextSecondary
    } else {
        ColorRole::TextHelper
    })
}

/// Placeholder and de-emphasized marks (`text-placeholder`).
pub fn quiet() -> Hsla {
    role(ColorRole::TextPlaceholder)
}

/// Disabled / off track (`toggle-off`).
pub fn ghost_line() -> Hsla {
    role(ColorRole::ToggleOff)
}

/// Success status (`support-success`).
pub fn ok() -> Hsla {
    role(ColorRole::SupportSuccess)
}

/// Active / warning / attention status (`support-warning`).
pub fn accent() -> Hsla {
    role(ColorRole::SupportWarning)
}

/// Error / destructive status (`support-error`).
pub fn coral() -> Hsla {
    role(ColorRole::SupportError)
}

/// Diff addition (`support-info` when blue/orange, else `support-success`).
pub fn added() -> Hsla {
    role(if blue_orange() {
        ColorRole::SupportInfo
    } else {
        ColorRole::SupportSuccess
    })
}

/// Diff removal (`status-orange` when blue/orange, else `support-error`).
pub fn gone() -> Hsla {
    role(if blue_orange() {
        ColorRole::StatusOrange
    } else {
        ColorRole::SupportError
    })
}

/// Command program and code keywords (`syntax-keyword`).
pub fn code_program() -> Hsla {
    role(ColorRole::SyntaxKeyword)
}

/// Command arguments and code strings (`syntax-string`).
pub fn code_argument() -> Hsla {
    role(ColorRole::SyntaxString)
}

/// Numbers in code (`syntax-number`).
pub fn code_number() -> Hsla {
    role(ColorRole::SyntaxNumber)
}

/// Inline links (`link-primary`).
pub fn link() -> Hsla {
    role(ColorRole::LinkPrimary)
}

/// Brand colour of agent `id` from `registry::KNOWN`, falling back to `text-primary`.
pub fn brand(id: &str) -> Hsla {
    crate::registry::known(id)
        .and_then(|k| k.3)
        .map_or_else(text, |(dark, light)| {
            rgb(if is_light() { light } else { dark }).into()
        })
}

/// Apply the active Carbon theme (and any custom user theme over it) to
/// `u0ui` and `gpui-kit`.
pub fn kit(cx: &mut App) {
    use gpui::component::theme::{Theme, ThemeTokens};

    let light = is_light();
    let dark = !light;
    let chosen = {
        let names = CHOSEN.lock().expect("the themes");
        if light {
            names.1.clone()
        } else {
            names.0.clone()
        }
    };

    let custom = chosen
        .as_deref()
        .filter(|name| parse_carbon(name).is_none())
        .and_then(|name| crate::themes::find(cx, name));

    let base_carbon = carbon_for(dark, chosen.as_deref()).unwrap_or(if dark {
        CarbonTheme::G100
    } else {
        CarbonTheme::White
    });

    // Project the base Carbon theme into u0ui and gpui-kit first.
    u0ui::theme::set_theme(cx, base_carbon);

    let overrides = custom.as_deref().map(crate::themes::roles_of);
    *CUSTOM.write().expect("custom theme") = overrides;

    if cx.try_global::<Theme>().is_some() {
        let mono = crate::fonts::faces(cx).mono.clone();
        Theme::update(cx, |t| {
            t.mono_font_family = mono;
            t.notification.placement = gpui::Anchor::TopRight;
            t.notification.margins.top = px(50.);
            t.notification.margins.right = px(16.);
            if custom.is_some() {
                let c = &mut t.colors;
                c.background = bg();
                c.foreground = text();
                c.border = border();
                c.input = border2();
                c.ring = role(ColorRole::Focus);
                c.caret = text();
                c.popover = panel();
                c.popover_foreground = text();
                c.secondary = panel();
                c.secondary_hover = row_hover();
                c.secondary_active = panel2();
                c.secondary_foreground = text();
                c.muted = panel();
                c.muted_foreground = text2();
                c.sidebar = rail();
                c.sidebar_foreground = text2();
                c.sidebar_border = border();
                c.primary = role(ColorRole::ButtonPrimary);
                c.button_primary = role(ColorRole::ButtonPrimary);
                c.link = link();
                c.danger = gone();
                c.success = added();
                c.warning = accent();
                t.tokens = ThemeTokens::from(t.colors);
            } else if blue_orange() || contrast() || glass() {
                t.colors.background = bg();
                t.colors.sidebar = rail();
                t.colors.border = border();
                t.colors.input = border2();
                t.colors.muted_foreground = text2();
                t.colors.danger = gone();
                t.colors.success = added();
                t.tokens = ThemeTokens::from(t.colors);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dark_is_g100_and_default_light_is_white() {
        assert_eq!(carbon_for(true, None), Some(CarbonTheme::G100));
        assert_eq!(carbon_for(false, None), Some(CarbonTheme::White));
        assert_eq!(carbon_for(true, Some("g90")), Some(CarbonTheme::G90));
        assert_eq!(
            carbon_for(true, Some("Carbon G100")),
            Some(CarbonTheme::G100)
        );
        assert_eq!(
            carbon_for(false, Some("Carbon G10")),
            Some(CarbonTheme::G10)
        );
        assert_eq!(carbon_for(true, Some("Midnight")), None);
    }
}
