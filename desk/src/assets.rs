//! The icons, built into the binary so that the desk runs from anywhere:
//! Ghost's own mark, and the marks of the brains it can run on. Whose each
//! is, and where it came from, is in `assets/icons/CREDITS.txt`.

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
    "openai",
    "claude",
    "deepseek",
    "gemini",
    "openrouter",
    "kimi",
    "qwen",
    "zhipu",
    "grok",
    "mistral",
    "groq",
    "ollama",
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
    "check",
    "pencil",
    "x",
    "retry",
    "power",
];

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
