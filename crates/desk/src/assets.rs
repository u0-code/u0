//! Application assets beyond `u0ui::Assets` (which serves Material Symbols
//! Sharp, Carbon custom glyphs, and vendored Simple Icons 16.37.0 brand marks
//! under `u0ui/brands/<slug>.svg`): u0's own mark (`icons/u0.svg`), the
//! breathing activity indicator (`icons/live-dot.svg`), and an agent's logo
//! read from the ACP registry into the home's `icons/` (`agent/<id>.svg`).

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Assets;

const FILES: &[(&str, &[u8])] = &[
    (
        "icons/u0.svg",
        include_bytes!("../../../assets/icons/u0.svg"),
    ),
    (
        "icons/live-dot.svg",
        include_bytes!("../../../assets/icons/live-dot.svg"),
    ),
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
        if let Some((_, bytes)) = FILES.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        gpui::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
