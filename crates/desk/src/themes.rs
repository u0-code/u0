//! Custom themes in `<home>/themes/*.json`, mapped onto Carbon `ColorRole`s
//! and `gpui-kit`'s `ThemeSet`.
//!
//! Built-in themes are Carbon's four (`White`, `G10`, `G90`, `G100` in
//! `u0ui::tokens::CarbonTheme`). User themes in `<home>/themes/` are created
//! by duplicating a built-in or active theme, editing tokens in the Appearance
//! drawer, or importing a VS Code or `gpui-kit` colour theme.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::component::theme::{ThemeConfig, ThemeMode, ThemeSet};
use gpui::{App, Global, Hsla};
use serde_json::{Map, Value, json};
use u0ui::tokens::{CarbonTheme, ColorRole};

/// User themes loaded from `<home>/themes/`, each with its source file.
#[derive(Default)]
pub struct Themes(pub Vec<(Rc<ThemeConfig>, Option<PathBuf>)>);

impl Global for Themes {}

/// The 12 Carbon roles edited in `Settings › Appearance › Customize`, paired
/// with their canonical `ThemeConfig.colors` key.
pub const EDITOR_ROLES: [(ColorRole, &str); 12] = [
    (ColorRole::Background, "background"),
    (ColorRole::Layer01, "sidebar.background"),
    (ColorRole::Layer02, "popover.background"),
    (ColorRole::BorderSubtle01, "border"),
    (ColorRole::TextPrimary, "foreground"),
    (ColorRole::TextSecondary, "muted.foreground"),
    (ColorRole::LinkPrimary, "link"),
    (ColorRole::Interactive, "primary.background"),
    (ColorRole::SupportError, "danger.background"),
    (ColorRole::SupportSuccess, "success.background"),
    (ColorRole::SupportWarning, "warning.background"),
    (ColorRole::SupportInfo, "base.blue"),
];

/// The 6 primary tokens highlighted above the live preview card in the drawer.
pub const PRIMARY_ROLES: [ColorRole; 6] = [
    ColorRole::Background,
    ColorRole::Layer01,
    ColorRole::TextPrimary,
    ColorRole::TextSecondary,
    ColorRole::Interactive,
    ColorRole::SupportError,
];

const PREVIEW_ROLES: [ColorRole; 5] = [
    ColorRole::Background,
    ColorRole::Layer01,
    ColorRole::TextPrimary,
    ColorRole::TextSecondary,
    ColorRole::Interactive,
];

/// Where your custom themes are kept.
pub fn folder(home: &Path) -> PathBuf {
    home.join("themes")
}

/// Read custom themes from `<home>/themes/*.json`; returns a message for each
/// file that could not be parsed.
pub fn load(home: &Path, cx: &mut App) -> Vec<String> {
    let mut all = Vec::new();
    let mut refused = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder(home))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for path in files {
        let read = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        match read.and_then(|t| serde_json::from_str::<ThemeSet>(&t).map_err(|e| e.to_string())) {
            Ok(set) => all.extend(
                set.themes
                    .into_iter()
                    .map(|t| (Rc::new(t), Some(path.clone()))),
            ),
            Err(e) => refused.push(format!("{}: {e}", path.display())),
        }
    }
    cx.set_global(Themes(all));
    refused
}

/// Look up a custom theme by `name`.
pub fn find(cx: &App, name: &str) -> Option<Rc<ThemeConfig>> {
    let themes = cx.try_global::<Themes>()?;
    themes
        .0
        .iter()
        .find(|(t, _)| t.name == name)
        .map(|(t, _)| t.clone())
}

/// Look up the source file path of custom theme `name`.
pub fn path_of(cx: &App, name: &str) -> Option<PathBuf> {
    let themes = cx.try_global::<Themes>()?;
    themes
        .0
        .iter()
        .find(|(t, _)| t.name == name)
        .and_then(|(_, path)| path.clone())
}

/// Parse a `#rrggbb` or `#rrggbbaa` hex colour into `Hsla`.
pub fn parse_hex(hex: &str) -> Option<Hsla> {
    let hex = hex.trim().strip_prefix('#')?;
    match hex.len() {
        6 => {
            let n = u32::from_str_radix(hex, 16).ok()?;
            Some(gpui::rgb(n).into())
        }
        8 => {
            let n = u32::from_str_radix(hex, 16).ok()?;
            Some(gpui::rgba(n).into())
        }
        _ => None,
    }
}

/// Lowercase `#rrggbb` representation of `color`.
pub fn to_hex(color: Hsla) -> String {
    u0ui::u0::settings::to_hex(color)
}

/// Carbon token name ↔ `ThemeConfig.colors` fallback key mapping.
pub const CARBON_TOKEN_MAP: &[(ColorRole, &str)] = &[
    (ColorRole::Background, "background"),
    (ColorRole::Layer01, "sidebar.background"),
    (ColorRole::Layer02, "popover.background"),
    (ColorRole::Field01, "secondary.background"),
    (ColorRole::Field02, "secondary.background"),
    (ColorRole::ChatBubbleUser, "muted.background"),
    (ColorRole::StateSelectedBackground, "muted.background"),
    (ColorRole::StateCurrentBackground, "muted.background"),
    (ColorRole::BackgroundHover, "secondary.hover.background"),
    (ColorRole::LayerHover01, "secondary.hover.background"),
    (ColorRole::BorderSubtle01, "border"),
    (ColorRole::BorderSubtle02, "border"),
    (ColorRole::BorderStrong01, "input.border"),
    (ColorRole::TextPrimary, "foreground"),
    (ColorRole::TextSecondary, "muted.foreground"),
    (ColorRole::TextHelper, "muted.foreground"),
    (ColorRole::LinkPrimary, "link"),
    (ColorRole::Interactive, "primary.background"),
    (ColorRole::ButtonPrimary, "primary.background"),
    (ColorRole::Focus, "primary.background"),
    (ColorRole::StateCurrentIndicator, "primary.background"),
    (ColorRole::SupportSuccess, "success.background"),
    (ColorRole::SupportError, "danger.background"),
    (ColorRole::SupportWarning, "warning.background"),
    (ColorRole::SupportInfo, "base.blue"),
    (ColorRole::SyntaxKeyword, "base.blue"),
    (ColorRole::SyntaxString, "base.green"),
    (ColorRole::SyntaxNumber, "base.yellow"),
];

/// Extract Carbon `ColorRole` overrides from a `ThemeConfig`.
pub fn roles_of(config: &ThemeConfig) -> HashMap<ColorRole, Hsla> {
    let Ok(Value::Object(raw)) = serde_json::to_value(&config.colors) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    // First apply gpui-kit ThemeConfig fallback keys.
    for &(role, kit_key) in CARBON_TOKEN_MAP {
        if let Some(hex) = raw.get(kit_key).and_then(Value::as_str)
            && let Some(c) = parse_hex(hex)
        {
            out.insert(role, c);
        }
    }
    // Explicit Carbon token names (when present in extra/direct keys) win.
    for role in ColorRole::ALL {
        if let Some(hex) = raw.get(role.name()).and_then(Value::as_str)
            && let Some(c) = parse_hex(hex)
        {
            out.insert(role, c);
        }
    }
    out
}

/// Five `ThemeCard` wireframe preview colours for a built-in `CarbonTheme`.
pub fn preview_colors_of_carbon(theme: CarbonTheme) -> [Hsla; 5] {
    PREVIEW_ROLES.map(|role| u0ui::theme::color_in(theme, role))
}

/// Five `ThemeCard` wireframe preview colours for a custom `ThemeConfig`.
pub fn preview_colors_of_custom(config: &ThemeConfig) -> [Hsla; 5] {
    let base = default_carbon(config.mode.is_dark());
    let over = roles_of(config);
    PREVIEW_ROLES.map(|role| {
        over.get(&role)
            .copied()
            .unwrap_or_else(|| u0ui::theme::color_in(base, role))
    })
}

/// Resolve all 12 [`EDITOR_ROLES`] for `config`, falling back to Carbon's base
/// palette for its mode where a key is unset.
pub fn role_values(config: &ThemeConfig) -> [(ColorRole, Hsla); 12] {
    let base = default_carbon(config.mode.is_dark());
    let over = roles_of(config);
    EDITOR_ROLES.map(|(role, _)| {
        let color = over
            .get(&role)
            .copied()
            .unwrap_or_else(|| u0ui::theme::color_in(base, role));
        (role, color)
    })
}

const fn default_carbon(dark: bool) -> CarbonTheme {
    if dark {
        CarbonTheme::G100
    } else {
        CarbonTheme::White
    }
}

fn carbon_colors_map(base: CarbonTheme) -> Map<String, Value> {
    let pick = |role: ColorRole| json!(to_hex(u0ui::theme::color_in(base, role)));
    [
        ("background", pick(ColorRole::Background)),
        ("foreground", pick(ColorRole::TextPrimary)),
        ("border", pick(ColorRole::BorderSubtle01)),
        ("sidebar.background", pick(ColorRole::Layer01)),
        ("popover.background", pick(ColorRole::Layer02)),
        ("muted.background", pick(ColorRole::ChatBubbleUser)),
        ("muted.foreground", pick(ColorRole::TextSecondary)),
        ("secondary.background", pick(ColorRole::Field01)),
        ("secondary.hover.background", pick(ColorRole::LayerHover01)),
        ("input.border", pick(ColorRole::BorderStrong01)),
        ("primary.background", pick(ColorRole::Interactive)),
        ("link", pick(ColorRole::LinkPrimary)),
        ("success.background", pick(ColorRole::SupportSuccess)),
        ("danger.background", pick(ColorRole::SupportError)),
        ("warning.background", pick(ColorRole::SupportWarning)),
        ("base.blue", pick(ColorRole::SupportInfo)),
        ("base.green", pick(ColorRole::SyntaxString)),
        ("base.yellow", pick(ColorRole::SyntaxNumber)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// Duplicate `config` as a file in `<home>/themes/`, under `name`.
pub fn duplicate(home: &Path, config: &ThemeConfig, name: &str) -> (PathBuf, Vec<u8>) {
    let mut theme = serde_json::to_value(config).expect("a theme serializes");
    theme["name"] = json!(name);
    theme["is_default"] = json!(false);
    file(home, name, &theme)
}

/// Export built-in `base` into a custom theme file under `name`.
pub fn own_carbon(home: &Path, name: &str, base: CarbonTheme) -> (PathBuf, Vec<u8>) {
    let mode = if base.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let mode_str = if mode.is_dark() { "dark" } else { "light" };
    file(
        home,
        name,
        &json!({
            "name": name,
            "mode": mode_str,
            "colors": carbon_colors_map(base),
        }),
    )
}

/// Reset `config` to the Carbon defaults for its mode, keeping its name.
pub fn reset_to_carbon(home: &Path, config: &ThemeConfig) -> (PathBuf, Vec<u8>) {
    own_carbon(home, &config.name, default_carbon(config.mode.is_dark()))
}

/// Update `role` in `config` to `color` and serialize `<home>/themes/<slug>.json`.
pub fn set_role_color(
    home: &Path,
    config: &ThemeConfig,
    role: ColorRole,
    color: Hsla,
) -> (PathBuf, Vec<u8>) {
    let mut theme = serde_json::to_value(config).expect("a theme serializes");
    if !theme["colors"].is_object() {
        theme["colors"] = json!({});
    }
    let hex = json!(to_hex(color));
    if let Some((_, kit_key)) = EDITOR_ROLES.iter().find(|(r, _)| *r == role) {
        theme["colors"][*kit_key] = hex.clone();
        if role == ColorRole::Layer02 {
            theme["colors"]["secondary.background"] = hex;
        }
    }
    file(home, &config.name, &theme)
}

fn file(home: &Path, name: &str, theme: &Value) -> (PathBuf, Vec<u8>) {
    let stem = crate::git::slug(name);
    let set = json!({ "name": name, "themes": [theme] });
    let text = serde_json::to_vec_pretty(&set).expect("a theme serializes");
    (folder(home).join(format!("{stem}.json")), text)
}

/// A theme read from a file of another app's, to be made one of yours
/// under a name not taken (`Desk::import`).
pub struct Imported {
    pub name: String,
    pub dark: bool,
    pub theme: Value,
}

impl Imported {
    /// Its file in `<home>/themes/`, under `name`.
    pub fn file(mut self, home: &Path, name: &str) -> (PathBuf, Vec<u8>) {
        self.theme["name"] = json!(name);
        file(home, name, &self.theme)
    }
}

/// Who has a theme's name.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    Free,
    /// One of the themes u0 brings: never replaced.
    Ours,
    /// The theme of yours, by name, a theme under it would replace: one
    /// called so, or kept in the file it would go in (names that slug
    /// alike share a file).
    Yours(String),
}

/// Who has `name` among the themes.
pub fn taken(cx: &App, home: &Path, name: &str) -> Taken {
    if crate::theme::parse_carbon(name).is_some() {
        return Taken::Ours;
    }
    let path = folder(home).join(format!("{}.json", crate::git::slug(name)));
    cx.try_global::<Themes>()
        .and_then(|t| {
            (t.0.iter()).find(|(t, at)| t.name == name || at.as_deref() == Some(path.as_path()))
        })
        .map_or(Taken::Free, |(t, _)| Taken::Yours(t.name.to_string()))
}

/// `base`, or the first of `base 2`, `base 3`, … that is not `taken`.
pub fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    let mut n = 2;
    loop {
        let name = format!("{base} {n}");
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

/// Read a `gpui-kit` `ThemeSet` JSON file: its first theme.
pub fn from_kit_json(text: &str) -> Result<Imported, String> {
    let set: ThemeSet = serde_json::from_str(text).map_err(|e| {
        crate::tr_with("it is not a gpui-kit theme file: %{e}", &[("e", &e)]).to_string()
    })?;
    let first = set
        .themes
        .into_iter()
        .next()
        .ok_or_else(|| crate::tr("it lists no themes under \"themes\"").to_string())?;
    let name = if first.name.trim().is_empty() {
        "Imported".to_owned()
    } else {
        first.name.to_string()
    };
    let dark = first.mode.is_dark();
    let mut theme = serde_json::to_value(&first).expect("a theme serializes");
    theme["is_default"] = json!(false);
    Ok(Imported { name, dark, theme })
}

// VS Code theme colour keys mapped onto our Carbon-bridged ThemeSet keys.
const VSCODE_MAP: [(&str, &[&str]); 19] = [
    ("background", &["editor.background"]),
    ("foreground", &["editor.foreground", "foreground"]),
    (
        "border",
        &[
            "panel.border",
            "editorGroup.border",
            "sideBar.border",
            "contrastBorder",
        ],
    ),
    (
        "sidebar.background",
        &["sideBar.background", "editorWidget.background"],
    ),
    (
        "popover.background",
        &[
            "editorWidget.background",
            "dropdown.background",
            "menu.background",
        ],
    ),
    (
        "muted.background",
        &[
            "list.activeSelectionBackground",
            "list.inactiveSelectionBackground",
        ],
    ),
    (
        "muted.foreground",
        &["descriptionForeground", "editorLineNumber.foreground"],
    ),
    (
        "secondary.background",
        &["input.background", "button.secondaryBackground"],
    ),
    (
        "secondary.hover.background",
        &["list.hoverBackground", "toolbar.hoverBackground"],
    ),
    (
        "input.border",
        &["input.border", "dropdown.border", "focusBorder"],
    ),
    (
        "primary.background",
        &["button.background", "focusBorder", "textLink.foreground"],
    ),
    ("link", &["textLink.foreground", "button.background"]),
    (
        "success.background",
        &[
            "terminal.ansiGreen",
            "gitDecoration.addedResourceForeground",
        ],
    ),
    (
        "danger.background",
        &["errorForeground", "terminal.ansiRed"],
    ),
    (
        "warning.background",
        &["editorWarning.foreground", "terminal.ansiYellow"],
    ),
    ("base.blue", &["terminal.ansiBlue"]),
    ("base.green", &["terminal.ansiGreen"]),
    ("base.yellow", &["terminal.ansiYellow"]),
    ("base.red", &["terminal.ansiRed"]),
];

/// Read a VS Code JSONC colour theme, its colours as gpui-kit names them.
pub fn from_vscode(text: &str) -> Result<Imported, String> {
    let theme: Value =
        jsonc_parser::parse_to_serde_value(text, &jsonc_parser::ParseOptions::default()).map_err(
            |e| crate::tr_with("it is not a theme VS Code reads: %{e}", &[("e", &e)]).to_string(),
        )?;
    let colors = theme["colors"].as_object().ok_or_else(|| {
        crate::tr("it has no colours: a VS Code colour theme names them under \"colors\"")
            .to_string()
    })?;
    let name = theme["name"].as_str().unwrap_or("Imported").to_owned();
    let dark = !matches!(theme["type"].as_str(), Some("light" | "hcLight"));
    let mut ours = Map::new();
    for (key, theirs) in VSCODE_MAP {
        if let Some(c) = theirs.iter().find_map(|k| colors.get(*k)?.as_str()) {
            ours.insert(key.to_owned(), json!(c));
        }
    }
    if !ours.contains_key("background") {
        return Err(
            crate::tr("it does not say editor.background, the colour everything is on").into(),
        );
    }
    let theme =
        json!({ "name": name, "mode": if dark { "dark" } else { "light" }, "colors": ours });
    Ok(Imported { name, dark, theme })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vscode_theme_with_comments_maps_onto_carbon_roles() {
        let text = r##"{
            // Night, as VS Code writes it
            "name": "Night // Owl", "type": "dark",
            "colors": {
                "editor.background": "#011627", /* the ground */
                "sideBar.background": "#0b253a",
                "editor.foreground": "#d6deeb",
                "button.background": "#7e57c2",
                "terminal.ansiGreen": "#22da6e",
            },
            "tokenColors": [],
        }"##;
        let imported = from_vscode(text).unwrap();
        let (name, dark) = (imported.name.clone(), imported.dark);
        let (path, written) = imported.file(Path::new("/h"), &name);
        assert_eq!(name, "Night // Owl");
        assert!(dark);
        assert_eq!(path, Path::new("/h/themes/night-owl.json"));
        let set: ThemeSet = serde_json::from_slice(&written).unwrap();
        let theme = &set.themes[0];
        assert!(theme.mode.is_dark());
        let roles = roles_of(theme);
        assert_eq!(
            roles.get(&ColorRole::Background),
            parse_hex("#011627").as_ref()
        );
        assert_eq!(
            roles.get(&ColorRole::Layer01),
            parse_hex("#0b253a").as_ref()
        );
        assert_eq!(
            roles.get(&ColorRole::Interactive),
            parse_hex("#7e57c2").as_ref()
        );
        assert_eq!(
            roles.get(&ColorRole::SupportSuccess),
            parse_hex("#22da6e").as_ref()
        );
        assert!(from_vscode(r#"{"colors": {}}"#).is_err());
    }

    #[test]
    fn editing_tokens_and_importing_kit_json_round_trip() {
        let home = Path::new("/h");
        let (_, base_bytes) = own_carbon(home, "Midnight", CarbonTheme::G100);
        let set: ThemeSet = serde_json::from_slice(&base_bytes).unwrap();
        let mut current = set.themes[0].clone();
        for (idx, (role, _)) in EDITOR_ROLES.into_iter().enumerate() {
            let hex = format!("#{:02x}{:02x}80", idx * 10 + 5, idx * 8 + 12);
            let picked = parse_hex(&hex).unwrap();
            let (_, updated) = set_role_color(home, &current, role, picked);
            let parsed: ThemeSet = serde_json::from_slice(&updated).unwrap();
            current = parsed.themes[0].clone();
            let resolved = role_values(&current);
            assert_eq!(to_hex(resolved[idx].1), hex, "{role:?}");
        }
        let kit_raw = String::from_utf8(
            serde_json::to_vec(&json!({
                "name": "Midnight",
                "themes": [current],
            }))
            .unwrap(),
        )
        .unwrap();
        let imported = from_kit_json(&kit_raw).unwrap();
        let (name, dark) = (imported.name.clone(), imported.dark);
        let (path, _) = imported.file(home, &name);
        assert_eq!(path, Path::new("/h/themes/midnight.json"));
        assert_eq!(name, "Midnight");
        assert!(dark);
        assert!(from_kit_json("not json").is_err());
        assert!(from_kit_json(r#"{"name":"Empty","themes":[]}"#).is_err());
    }

    #[test]
    fn an_import_under_a_name_taken_is_offered_the_next_one_free() {
        let taken = ["Night Owl", "Night Owl 2"];
        let is_taken = |n: &str| taken.contains(&n);
        assert_eq!(free_name("Night Owl", is_taken), "Night Owl 3");
        assert_eq!(free_name("Paper", is_taken), "Paper");
    }
}
