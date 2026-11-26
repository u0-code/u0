//! Themes, in gpui-kit's theme format (a file of one or more, each light or
//! dark, its colours named as gpui-kit's components read them): those
//! Ghost brings (`assets/themes/`), and yours in the home's `themes/`,
//! written by hand, duplicated from one here, or imported from a VS Code
//! theme. The look takes the one chosen for light and the one for dark
//! (`theme::kit`); with none, Ghost's own.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::component::theme::{ThemeConfig, ThemeSet};
use gpui::{App, Global};
use serde_json::{Map, Value, json};

/// The themes Ghost brings.
const BUNDLED: [&str; 6] = [
    include_str!("../../../assets/themes/ayu.json"),
    include_str!("../../../assets/themes/catppuccin.json"),
    include_str!("../../../assets/themes/flexoki.json"),
    include_str!("../../../assets/themes/gruvbox.json"),
    include_str!("../../../assets/themes/solarized.json"),
    include_str!("../../../assets/themes/tokyonight.json"),
];

/// The themes there are, each with the file of yours it is in (none, one
/// Ghost brings).
#[derive(Default)]
pub struct Themes(pub Vec<(Rc<ThemeConfig>, Option<PathBuf>)>);

impl Global for Themes {}

/// Where your themes are kept.
pub fn folder(home: &Path) -> PathBuf {
    home.join("themes")
}

/// Read the themes again: Ghost's, then yours; what of yours did not read,
/// each said.
pub fn load(home: &Path, cx: &mut App) -> Vec<String> {
    let mut all = Vec::new();
    for text in BUNDLED {
        let set: ThemeSet = serde_json::from_str(text).expect("a theme Ghost brings reads");
        all.extend(set.themes.into_iter().map(|t| (Rc::new(t), None)));
    }
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

/// The theme called `name`.
pub fn find(cx: &App, name: &str) -> Option<Rc<ThemeConfig>> {
    let themes = cx.try_global::<Themes>()?;
    themes
        .0
        .iter()
        .find(|(t, _)| t.name == name)
        .map(|(t, _)| t.clone())
}

/// `config` as a file of yours, under `name`, to be changed as you like;
/// its path, and the file's text.
pub fn duplicate(home: &Path, config: &ThemeConfig, name: &str) -> (PathBuf, Vec<u8>) {
    let mut theme = serde_json::to_value(config).expect("a theme serializes");
    theme["name"] = json!(name);
    theme["is_default"] = json!(false);
    file(home, name, &theme)
}

/// Ghost's own colours for the look drawn now (`dark` or light) as a file
/// of yours, under `name`, written in the colours Ghost reads from a theme.
pub fn own(home: &Path, name: &str, dark: bool) -> (PathBuf, Vec<u8>) {
    use crate::theme as t;
    use gpui::component::Colorize as _;
    let colors: Map<String, Value> = [
        ("background", t::bg()),
        ("foreground", t::text()),
        ("border", t::border()),
        ("sidebar.background", t::rail()),
        ("popover.background", t::panel()),
        ("muted.background", t::panel2()),
        ("muted.foreground", t::muted()),
        ("secondary.background", t::bubble()),
        ("secondary.hover.background", t::hover()),
        ("input.border", t::border2()),
        ("link", t::link()),
        ("success.background", t::ok()),
        ("danger.background", t::coral()),
        ("warning.background", t::accent()),
        ("base.blue", t::code_program()),
        ("base.green", t::code_argument()),
        ("base.yellow", t::code_number()),
    ]
    .into_iter()
    .map(|(k, c)| (k.to_owned(), json!(c.to_hex())))
    .collect();
    let mode = if dark { "dark" } else { "light" };
    file(
        home,
        name,
        &json!({ "name": name, "mode": mode, "colors": colors }),
    )
}

/// A file of yours holding the one `theme` called `name`.
fn file(home: &Path, name: &str, theme: &Value) -> (PathBuf, Vec<u8>) {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let set = json!({ "name": name, "themes": [theme] });
    let text = serde_json::to_vec_pretty(&set).expect("a theme serializes");
    (folder(home).join(format!("{stem}.json")), text)
}

// Each of gpui-kit's colours, from the first of VS Code's that names it.
const MAP: [(&str, &[&str]); 18] = [
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
    ("sidebar.background", &["sideBar.background"]),
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
    ("link", &["textLink.foreground"]),
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

/// A VS Code theme (`text`, its JSON with comments as VS Code writes it)
/// as a theme of yours: its colours put where gpui-kit and Ghost read
/// theirs. Its path, its text, and its name.
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
    for (key, theirs) in MAP {
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
    fn every_theme_ghost_brings_reads() {
        for text in BUNDLED {
            let set: ThemeSet = serde_json::from_str(text).unwrap();
            assert!(!set.themes.is_empty());
        }
    }

    #[test]
    fn a_vscode_theme_with_comments_becomes_a_theme_of_yours() {
        let text = r##"{
            // Night, as VS Code writes it
            "name": "Night // Owl", "type": "dark",
            "colors": {
                "editor.background": "#011627", /* the ground */
                "editor.foreground": "#d6deeb",
                "terminal.ansiGreen": "#22da6e",
            },
            "tokenColors": [],
        }"##;
        let (path, written, name) = from_vscode(Path::new("/h"), text).unwrap();
        assert_eq!(name, "Night // Owl");
        assert_eq!(path, Path::new("/h/themes/night----owl.json"));
        let set: ThemeSet = serde_json::from_slice(&written).unwrap();
        let theme = &set.themes[0];
        assert!(theme.mode.is_dark());
        let raw: Value = serde_json::from_slice(&written).unwrap();
        assert_eq!(raw["themes"][0]["colors"]["background"], "#011627");
        assert_eq!(raw["themes"][0]["colors"]["success.background"], "#22da6e");
        assert!(from_vscode(Path::new("/h"), r#"{"colors": {}}"#).is_err());
    }
}
