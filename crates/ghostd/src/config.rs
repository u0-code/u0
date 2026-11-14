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
use ghost_mind::{MindConfig, Operators, PlaceConfig};
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
    /// The accounts that run her, per channel, and the prefix of a
    /// directive from one of them.
    #[serde(default)]
    pub operators: Operators,
    /// What they say about particular places, by place key.
    #[serde(default)]
    pub places: BTreeMap<String, PlaceConfig>,
    /// How requests leave this machine. `GHOST_PROXY` and `GHOST_DIRECT`
    /// in the environment win over it, a part at a time: see `ghost_net`.
    #[serde(default)]
    pub network: ghost_net::Setting,
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

    /// How she lives, with who runs her and what they say about places.
    pub fn mind(&self) -> MindConfig {
        MindConfig {
            operators: self.operators.clone(),
            places: self.places.clone(),
            ..self.mind.clone()
        }
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

#[cfg(test)]
mod operator_tests {
    use super::*;

    const BRAIN: &str = "[brain]\nthink = \"a\"\ntick = \"a\"\n";

    fn parse(operators: &str) -> Config {
        toml::from_str(&format!("{BRAIN}{operators}")).unwrap()
    }

    #[test]
    fn operators_are_read_per_channel_with_or_without_a_prefix() {
        let none = parse("");
        assert_eq!(none.mind().operators.prefix, "fox:");
        assert!(none.mind().operators.accounts.is_empty());

        let qq = parse("[operators]\nqq = [\"0123456789ABCDEF0123456789ABCDEF\"]\n");
        let ops = qq.mind().operators;
        assert_eq!(ops.prefix, "fox:");
        assert!(ops.includes("qq", "0123456789ABCDEF0123456789ABCDEF"));
        assert!(!ops.includes("telegram", "0123456789ABCDEF0123456789ABCDEF"));

        let several = parse(
            "[operators]\nprefix = \"op:\"\nqq = [\"A\", \"B\"]\ntelegram = [\"42\"]\nterm = [\"fox\"]\n",
        );
        let ops = several.mind().operators;
        assert_eq!(ops.prefix, "op:");
        assert_eq!(ops.accounts.len(), 3);
        assert!(
            ops.includes("qq", "B")
                && ops.includes("telegram", "42")
                && ops.includes("term", "fox")
        );
        assert_eq!(
            ops.directive("telegram", "42", "OP： restart"),
            Some("restart")
        );
        assert_eq!(ops.directive("telegram", "42", "fox: restart"), None);
    }

    #[test]
    fn she_reports_at_nine_in_the_evening_unless_told_another_time_or_none() {
        let at = |h, m| chrono::NaiveTime::from_hms_opt(h, m, 0);
        assert_eq!(parse("").mind().operators.report, at(21, 0));
        let ops = parse("[operators]\nreport = \"07:30\"\nqq = [\"A\"]\n")
            .mind()
            .operators;
        assert_eq!(ops.report, at(7, 30));
        // The time is not taken for a channel.
        assert_eq!(ops.accounts.len(), 1);
        assert_eq!(
            parse("[operators]\nreport = \"\"\n")
                .mind()
                .operators
                .report,
            None
        );
        let wrong: Result<Config, _> =
            toml::from_str(&format!("{BRAIN}[operators]\nreport = \"9pm\"\n"));
        assert!(wrong.is_err());
    }

    #[test]
    fn a_place_can_be_set_to_addressed_only_and_given_a_note() {
        let cfg = parse(
            "[places.\"qq:group:5F2A\"]\naddressed_only = true\nnote = \"keep it light here\"\n\n\
             [places.\"qq:group:77\"]\nnote = \"old friends\"\n",
        );
        let places = cfg.mind().places;
        assert!(places["qq:group:5F2A"].addressed_only);
        assert_eq!(
            places["qq:group:5F2A"].note.as_deref(),
            Some("keep it light here")
        );
        assert!(!places["qq:group:77"].addressed_only);
        assert!(parse("").mind().places.is_empty());
        let wrong: Result<Config, _> =
            toml::from_str(&format!("{BRAIN}[places.\"x:y\"]\nadressed_only = true\n"));
        assert!(wrong.is_err());
    }

    #[test]
    fn the_environment_wins_over_the_network_in_ghostd_toml() {
        let cfg =
            parse("[network]\nproxy = \"http://127.0.0.1:7890\"\ndirect = [\"*.deepseek.com\"]\n");
        let with = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        let rule = |pairs| {
            ghost_net::Network::new(&cfg.network, with(pairs))
                .unwrap()
                .to_string()
        };
        assert_eq!(
            rule(&[]),
            "proxy http://127.0.0.1:7890; direct to *.deepseek.com"
        );
        assert_eq!(rule(&[("GHOST_PROXY", "none")]), "no proxy");
        assert_eq!(
            rule(&[("GHOST_DIRECT", "api.example")]),
            "proxy http://127.0.0.1:7890; direct to api.example"
        );
        assert_eq!(
            rule(&[("GHOST_PROXY", "http://10.0.0.2:3128")]),
            "proxy http://10.0.0.2:3128; direct to *.deepseek.com"
        );
        // Without a [network], the environment alone decides.
        let bare = parse("");
        assert_eq!(bare.network, ghost_net::Setting::default());
        let wrong: Result<Config, _> =
            toml::from_str(&format!("{BRAIN}[network]\nproxies = \"none\"\n"));
        assert!(wrong.is_err());
    }

    #[test]
    fn operators_are_not_read_from_under_mind() {
        let wrong: Result<Config, _> =
            toml::from_str(&format!("{BRAIN}[mind.operators]\nqq = [\"A\"]\n"));
        assert!(wrong.is_err());
        // Nor is anything but a list of accounts taken for a channel.
        let wrong: Result<Config, _> =
            toml::from_str(&format!("{BRAIN}[operators]\nprefx = \"op:\"\n"));
        assert!(wrong.is_err());
    }
}
