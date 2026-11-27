//! Custom themes in `<home>/themes/*.json`, mapped onto Carbon `ColorRole`s
//! and `gpui-kit`'s `ThemeSet`.
//!
//! Built-in themes are Carbon's four (`White`, `G10`, `G90`, `G100` in
//! `u0ui::tokens::CarbonTheme`). User themes in `<home>/themes/` are created
//! by duplicating an active theme, editing tokens, or importing a VS Code
//! colour theme.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::component::Colorize as _;
use gpui::component::theme::{ThemeConfig, ThemeMode, ThemeSet};
use gpui::{App, Global, Hsla};
use serde_json::{Map, Value, json};
use u0ui::tokens::ColorRole;

/// User themes loaded from `<home>/themes/`, each with its source file.
#[derive(Default)]
pub struct Themes(pub Vec<(Rc<ThemeConfig>, Option<PathBuf>)>);

impl Global for Themes {}

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

/// Duplicate `config` as a file in `<home>/themes/`, under `name`.
pub fn duplicate(home: &Path, config: &ThemeConfig, name: &str) -> (PathBuf, Vec<u8>) {
    let mut theme = serde_json::to_value(config).expect("a theme serializes");
    theme["name"] = json!(name);
    theme["is_default"] = json!(false);
    file(home, name, &theme)
}

/// Export the active Carbon theme for `dark` (or light) into a custom theme
/// file under `name`.
pub fn own(home: &Path, name: &str, dark: bool) -> (PathBuf, Vec<u8>) {
    use crate::theme as t;
    let colors: Map<String, Value> = [
        ("background", t::role(ColorRole::Background)),
        ("foreground", t::role(ColorRole::TextPrimary)),
        ("border", t::role(ColorRole::BorderSubtle01)),
        ("sidebar.background", t::role(ColorRole::Layer01)),
        ("popover.background", t::role(ColorRole::Layer02)),
        ("muted.background", t::role(ColorRole::ChatBubbleUser)),
        ("muted.foreground", t::role(ColorRole::TextSecondary)),
        ("secondary.background", t::role(ColorRole::Field01)),
        (
            "secondary.hover.background",
            t::role(ColorRole::LayerHover01),
        ),
        ("input.border", t::role(ColorRole::BorderStrong01)),
        ("primary.background", t::role(ColorRole::Interactive)),
        ("link", t::role(ColorRole::LinkPrimary)),
        ("success.background", t::role(ColorRole::SupportSuccess)),
        ("danger.background", t::role(ColorRole::SupportError)),
        ("warning.background", t::role(ColorRole::SupportWarning)),
        ("base.blue", t::role(ColorRole::SyntaxKeyword)),
        ("base.green", t::role(ColorRole::SyntaxString)),
        ("base.yellow", t::role(ColorRole::SyntaxNumber)),
    ]
    .into_iter()
    .map(|(k, c)| (k.to_owned(), json!(c.to_hex())))
    .collect();
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let mode_str = if mode.is_dark() { "dark" } else { "light" };
    file(
        home,
        name,
        &json!({ "name": name, "mode": mode_str, "colors": colors }),
    )
}

fn file(home: &Path, name: &str, theme: &Value) -> (PathBuf, Vec<u8>) {
    let stem = crate::git::slug(name);
    let set = json!({ "name": name, "themes": [theme] });
    let text = serde_json::to_vec_pretty(&set).expect("a theme serializes");
    (folder(home).join(format!("{stem}.json")), text)
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

/// Import a VS Code JSONC colour theme into `<home>/themes/<slug>.json`.
pub fn from_vscode(home: &Path, text: &str) -> Result<(PathBuf, Vec<u8>, String), String> {
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
    let (path, text) = file(home, &name, &theme);
    Ok((path, text, name))
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
        let (path, written, name) = from_vscode(Path::new("/h"), text).unwrap();
        assert_eq!(name, "Night // Owl");
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
        assert!(from_vscode(Path::new("/h"), r#"{"colors": {}}"#).is_err());
    }
}
