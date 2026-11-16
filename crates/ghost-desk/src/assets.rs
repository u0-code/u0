//! The ghost glyph, built into the binary so that the desk runs from anywhere.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Assets;

const FILES: &[(&str, &[u8])] = &[(
    "icons/ghost.svg",
    include_bytes!("../assets/icons/ghost.svg"),
)];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
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
