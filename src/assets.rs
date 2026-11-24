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
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8])),*]
    };
}

const FILES: &[(&str, &[u8])] = icons![
    "ghost",
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
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
