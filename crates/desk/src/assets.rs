//! The icons. The desk's own are built into the binary so that it runs
//! from anywhere (`assets/icons/CREDITS.txt` says where each came from).
//! An agent's logo is the one the ACP registry publishes for it, read with
//! the registry into the home's `icons/`, and asked for as
//! `agent/<id>.svg`.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../../../assets/icons/", $name, ".svg")) as &[u8])),*]
    };
}

const FILES: &[(&str, &[u8])] = icons![
    "ghost",
    "dots",
    "pin",
    "archive",
    "clock",
    "branch",
    "mic",
    "sliders",
    "reply",
    "arrow-down",
    "expand",
    "camera",
    "sidebar",
    "chevron-left",
    "chevron-right",
    "chevron-down",
    "plus",
    "search",
    "laptop",
    "server",
    "plug",
    "file",
    "folder",
    "alert",
    "minus",
    "check",
    "pencil",
    "x",
    "retry",
    "power",
    "bell",
    "terminal",
    "book",
    "user",
    "panel-right",
    "copy",
    "image-off",
    "arrow-right",
    "settings",
    "list",
    "file-plus",
    "trash",
    "globe",
    "bulb",
    "square",
    "arrow-up",
    "keyboard",
    "sun",
    "square-pen",
    "square-dashed",
    "text-search",
    "folder-plus",
    "undo-2",
    "arrow-left",
    "diff",
    "pull-request",
    "list-plus",
    "corner-up-right",
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(id) = path
            .strip_prefix("agent/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            let file = crate::registry::icon_path(&crate::settings::home(), id);
            return Ok(std::fs::read(file).ok().map(Cow::Owned));
        }
        // gpui-kit's components ask for icons the desk does not draw (a
        // close mark, a pair of chevrons): those are gpui-kit's own.
        match FILES.iter().find(|(name, _)| *name == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(*bytes))),
            None => gpui::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
