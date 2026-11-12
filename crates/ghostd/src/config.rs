//! `ghostd.toml`: which brains she runs on, where the daemon listens, and how
//! she paces herself. This is body configuration and lives outside the ghost
//! directory, because none of it is her.
//!
//! Search order: `--config`, `$GHOSTD_CONFIG`, `./ghostd.toml`,
//! `~/.config/ghost/ghostd.toml`.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, bail};
use ghost_brain::{Brain, config::ProviderConfig};
use ghost_mind::MindConfig;
use serde::Deserialize;

pub const FILE: &str = "ghostd.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub brain: Tiers,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub listen: Listen,
    #[serde(default)]
    pub mind: MindConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tiers {
    /// Provider name for conversation and reflection.
    pub think: String,
    /// Provider name for the routine ticks of her day.
    pub tick: String,
}

/// Where presences and consoles connect.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Listen {
    pub addr: String,
    /// Environment variable holding the token clients must present.
    pub token_env: String,
    /// Web origins allowed to connect besides pages served from this
    /// machine, e.g. `https://console.example`.
    pub origins: Vec<String>,
}

impl Default for Listen {
    fn default() -> Self {
        Self {
            addr: ghost_protocol::DEFAULT_ADDR.to_owned(),
            token_env: "GHOSTD_TOKEN".to_owned(),
            origins: Vec::new(),
        }
    }
}

impl Listen {
    /// The token clients must present, if one is set. Listening beyond this
    /// machine without one is refused.
    pub fn token(&self) -> anyhow::Result<Option<String>> {
        let token = std::env::var(&self.token_env)
            .ok()
            .filter(|t| !t.trim().is_empty());
        let local = self
            .addr
            .parse::<SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback());
        if token.is_none() && !local {
            bail!(
                "listening on {} without a token; set {} or listen on 127.0.0.1",
                self.addr,
                self.token_env
            );
        }
        Ok(token)
    }
}

impl Config {
    /// Whether there is a configuration file to load: one named explicitly,
    /// or one in the usual places.
    pub fn exists(explicit: Option<&Path>) -> bool {
        explicit.is_some() || candidates().iter().any(|p| p.exists())
    }

    /// A configuration for a machine with no configuration file: every
    /// default, and the first thing found here that she can think with.
    /// Returns the configuration and what it settled on.
    pub fn from_found(found: &[ghost_brain::discover::Found]) -> anyhow::Result<(Self, String)> {
        // A suggestion with a blank to fill in cannot be used as it stands.
        let usable = found
            .iter()
            .find(|f| !f.config.iter().any(|line| line.starts_with("model = \"\"")))
            .context(
                "no ghostd.toml, and nothing here to think with: set DEEPSEEK_API_KEY or \
                 ANTHROPIC_API_KEY, install the claude or codex CLI, or start Ollama",
            )?;
        let text = format!(
            "[brain]\nthink = \"{0}\"\ntick = \"{0}\"\n\n{1}",
            usable.name,
            usable.toml()
        );
        let cfg: Self =
            toml::from_str(&text).context("build a configuration from what was found")?;
        Ok((cfg, usable.what.clone()))
    }

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

    pub fn brain(&self, name: &str) -> anyhow::Result<Arc<dyn Brain>> {
        let cfg = self
            .providers
            .get(name)
            .with_context(|| format!("no provider named {name:?}"))?;
        cfg.build().with_context(|| format!("provider {name:?}"))
    }

    pub fn think(&self) -> anyhow::Result<Arc<dyn Brain>> {
        self.brain(&self.brain.think)
    }

    pub fn tick(&self) -> anyhow::Result<Arc<dyn Brain>> {
        self.brain(&self.brain.tick)
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

#[cfg(test)]
mod found_tests {
    use ghost_brain::discover::Found;

    use super::*;

    fn found(name: &str, config: &[&str]) -> Found {
        Found {
            name: name.into(),
            what: format!("{name} is here"),
            config: config.iter().map(|&l| l.to_owned()).collect(),
            works: true,
        }
    }

    #[test]
    fn with_no_file_she_thinks_with_the_first_thing_that_is_ready() {
        let here = [
            found(
                "openai",
                &[r#"kind = "openai""#, r#"model = "" # the model to use"#],
            ),
            found("claude-code", &[r#"kind = "claude-code""#]),
        ];
        let (cfg, what) = Config::from_found(&here).unwrap();
        assert_eq!(cfg.brain.think, "claude-code");
        assert_eq!(cfg.brain.tick, "claude-code");
        assert_eq!(what, "claude-code is here");
        assert!(Config::from_found(&[]).is_err());
    }
}
