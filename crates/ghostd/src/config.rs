//! `ghostd.toml`: which brains she runs on. This is body configuration and
//! lives outside the ghost directory, because the model is not her.
//!
//! Search order: `--config`, `$GHOSTD_CONFIG`, `./ghostd.toml`,
//! `~/.config/ghost/ghostd.toml`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use ghost_brain::{Brain, config::ProviderConfig};
use serde::Deserialize;

pub const FILE: &str = "ghostd.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub brain: Tiers,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tiers {
    /// Provider name for conversation and reflection.
    pub think: String,
    /// Provider name for the life engine's routine ticks.
    pub tick: String,
    /// Longest reply she may write, in tokens.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Override the shipped `self.md` template with this file.
    pub self_template: Option<PathBuf>,
}

fn default_max_tokens() -> u32 {
    400
}

impl Config {
    pub fn load(explicit: Option<&Path>) -> anyhow::Result<(Self, PathBuf)> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => candidates()
                .into_iter()
                .find(|p| p.exists())
                .with_context(|| {
                    format!(
                        "no {FILE} found; pass --config, set GHOSTD_CONFIG, or copy ghostd.example.toml"
                    )
                })?,
        };
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        for tier in [&cfg.brain.think, &cfg.brain.tick] {
            if !cfg.providers.contains_key(tier) {
                bail!("brain tier refers to unknown provider {tier:?}");
            }
        }
        Ok((cfg, path))
    }

    pub fn brain(&self, name: &str) -> anyhow::Result<Box<dyn Brain>> {
        let cfg = self
            .providers
            .get(name)
            .with_context(|| format!("no provider named {name:?}"))?;
        cfg.build().with_context(|| format!("provider {name:?}"))
    }

    pub fn think(&self) -> anyhow::Result<Box<dyn Brain>> {
        self.brain(&self.brain.think)
    }

    pub fn self_template(&self) -> anyhow::Result<String> {
        match &self.brain.self_template {
            Some(p) => std::fs::read_to_string(p).with_context(|| format!("read {}", p.display())),
            None => Ok(ghost_core::narrate::DEFAULT_SELF_TEMPLATE.to_owned()),
        }
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("GHOSTD_CONFIG") {
        v.push(PathBuf::from(p));
    }
    v.push(PathBuf::from(FILE));
    if let Ok(home) = std::env::var("HOME") {
        v.push(Path::new(&home).join(".config/ghost").join(FILE));
    }
    v
}
