//! `ghostd.toml`: which brains she runs on, where the daemon listens, and how
//! she paces herself. This is body configuration and lives outside the ghost
//! directory, because none of it is her.
//!
//! Search order: `--config`, `$GHOSTD_CONFIG`, `./ghostd.toml`,
//! `~/.config/ghost/ghostd.toml`.
//!
//! One file serves every character a daemon runs. Most of it is the
//! deployment's alone: the brains, where it listens, the network, the
//! prompts. How each character lives may differ: `[ghosts.<id>]`, where
//! `<id>` is the name of her directory, holds any of `mind`, `operators`,
//! `places`, `tools`, `mcp` and `skills`, laid over the top level for her
//! alone. Tables are merged a key at a time, so what she leaves out she
//! inherits; any other value, a list included, replaces the deployment's.
//! That is the whole of the rule, and [`Config::hers`] is all of it.

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

/// The file. What a character may have her own of is kept here as it was
/// written, and read only through [`Config::hers`], so that there is one
/// way to read it whether or not anything is laid over it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub brain: Tiers,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub listen: Listen,
    /// How requests leave this machine. `GHOST_PROXY` and `GHOST_DIRECT`
    /// in the environment win over it, a part at a time: see `ghost_net`.
    #[serde(default)]
    pub network: ghost_net::Setting,
    #[serde(default)]
    mind: toml::Table,
    #[serde(default)]
    operators: toml::Table,
    #[serde(default)]
    places: toml::Table,
    #[serde(default)]
    tools: toml::Table,
    #[serde(default)]
    mcp: toml::value::Array,
    #[serde(default)]
    skills: toml::Table,
    /// What differs for one character, by the name of her directory.
    #[serde(default)]
    ghosts: BTreeMap<String, toml::Table>,
}

/// How one character lives: the deployment's settings with hers laid over
/// them. Nothing in it is shared with another character once she runs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hers {
    #[serde(default)]
    pub mind: MindConfig,
    /// The accounts that run her, per channel.
    #[serde(default)]
    pub operators: Operators,
    /// What they say about particular places, by place key.
    #[serde(default)]
    pub places: BTreeMap<String, PlaceConfig>,
    /// How she finds and runs servers of tools.
    #[serde(default)]
    pub tools: ghost_work::servers::Config,
    /// Servers of tools whoever runs her gives her, one `[[mcp]]` each.
    #[serde(default)]
    pub mcp: Vec<ghost_work::servers::Configured>,
    /// How she finds and keeps agent skills.
    #[serde(default)]
    pub skills: ghost_work::skills::Config,
}

impl Hers {
    /// How she lives, with who runs her and what they say about places.
    pub fn mind(&self) -> MindConfig {
        MindConfig {
            operators: self.operators.clone(),
            places: self.places.clone(),
            ..self.mind.clone()
        }
    }
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
        let cfg = Self::parse(&text).context("build a configuration from what was found")?;
        Ok((cfg, usable.what.clone()))
    }

    /// Read a configuration from the text of a file, and check what it says
    /// for each character.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let cfg: Self = toml::from_str(text)?;
        let prefixed = cfg.operators.contains_key("prefix")
            || cfg.ghosts.iter().any(|(_, own)| {
                own.get("operators")
                    .and_then(toml::Value::as_table)
                    .is_some_and(|ops| ops.contains_key("prefix"))
            });
        if prefixed {
            bail!(
                "[operators] prefix is gone: nothing anyone writes gives her an order any more. \
                 Remove the line. The accounts listed there are still whoever runs her, and \
                 a console is where she answers them at once"
            );
        }
        cfg.deployment()?;
        for id in cfg.ghosts.keys() {
            cfg.hers(id)?;
        }
        Ok(cfg)
    }

    /// The deployment's directory of prompts, shared by every character.
    pub fn prompts(&self) -> anyhow::Result<Option<PathBuf>> {
        Ok(self.deployment()?.mind.prompts)
    }

    /// The settings as the top level of the file has them, for a character
    /// with nothing of her own.
    fn deployment(&self) -> anyhow::Result<Hers> {
        Ok(toml::Value::Table(self.top()).try_into()?)
    }

    /// What the top level of the file says about how a character lives.
    fn top(&self) -> toml::Table {
        let mut top = toml::Table::new();
        for (key, table) in [
            ("mind", &self.mind),
            ("operators", &self.operators),
            ("places", &self.places),
            ("tools", &self.tools),
            ("skills", &self.skills),
        ] {
            top.insert(key.to_owned(), toml::Value::Table(table.clone()));
        }
        top.insert("mcp".to_owned(), toml::Value::Array(self.mcp.clone()));
        top
    }

    /// The settings of the character whose directory is called `id`: the
    /// deployment's, with `[ghosts.<id>]` laid over them.
    pub fn hers(&self, id: &str) -> anyhow::Result<Hers> {
        let mut merged = self.top();
        if let Some(own) = self.ghosts.get(id) {
            if own
                .get("mind")
                .and_then(toml::Value::as_table)
                .is_some_and(|mind| mind.contains_key("prompts"))
            {
                bail!(
                    "[ghosts.{id}.mind] sets prompts: the prompts are shared by every character, \
                     so they are set under [mind] alone"
                );
            }
            lay_over(&mut merged, own);
        }
        toml::Value::Table(merged)
            .try_into()
            .with_context(|| format!("[ghosts.{id}]"))
    }

    /// Refuse a `[ghosts.<id>]` for a character this daemon is not running:
    /// most likely a misspelled directory, whose settings would otherwise
    /// silently not apply.
    pub fn check_ids(&self, running: &[&str]) -> anyhow::Result<()> {
        if let Some(stray) = self
            .ghosts
            .keys()
            .find(|id| !running.contains(&id.as_str()))
        {
            bail!(
                "ghostd.toml has [ghosts.{stray}], and no character called {stray:?} is running; \
                 the ids are the names of the ghost directories: {}",
                running.join(", ")
            );
        }
        Ok(())
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
        let cfg = Self::parse(&text).with_context(|| format!("parse {}", path.display()))?;
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

/// Lay `over` onto `base`: a table into a table a key at a time, anything
/// else in place of what was there.
fn lay_over(base: &mut toml::Table, over: &toml::Table) {
    for (key, value) in over {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(below)), toml::Value::Table(above)) => lay_over(below, above),
            _ => {
                base.insert(key.clone(), value.clone());
            }
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
        Config::parse(&format!("{BRAIN}{operators}")).unwrap()
    }

    impl Config {
        /// How a character with nothing of her own lives.
        fn mind(&self) -> MindConfig {
            self.deployment().unwrap().mind()
        }
    }

    #[test]
    fn a_characters_own_settings_apply_to_her_and_what_she_leaves_out_she_inherits() {
        let cfg = parse(
            "[mind]\ntempo = 0.5\ncalls_per_hour = 60\nweather = false\n\
             [mind.work]\nenabled = true\nper_day = 4\n\
             [operators]\nreport = \"20:00\"\nqq = [\"A\"]\n\
             [places.\"qq:group:1\"]\nnote = \"everyone's\"\n\
             [ghosts.lin.mind]\ncalls_per_hour = 10\n\
             [ghosts.lin.mind.work]\nper_day = 1\n\
             [ghosts.lin.operators]\nqq = [\"B\"]\ntelegram = [\"7\"]\n\
             [ghosts.lin.places.\"qq:group:2\"]\naddressed_only = true\n\
             [[ghosts.lin.mcp]]\nname = \"notes\"\ncommand = \"notes-mcp\"\n",
        );
        let lin = cfg.hers("lin").unwrap().mind();
        // Hers.
        assert_eq!(lin.calls_per_hour, 10);
        assert_eq!(lin.work.per_day, 1);
        assert!(lin.operators.includes("qq", "B") && !lin.operators.includes("qq", "A"));
        assert!(lin.operators.includes("telegram", "7"));
        assert!(lin.places["qq:group:2"].addressed_only);
        assert_eq!(cfg.hers("lin").unwrap().mcp.len(), 1);
        // Inherited, beside hers in the same tables.
        assert!((lin.tempo - 0.5).abs() < f32::EPSILON);
        assert!(!lin.weather && lin.work.enabled);
        assert_eq!(
            lin.operators.report,
            chrono::NaiveTime::from_hms_opt(20, 0, 0)
        );
        assert_eq!(lin.places["qq:group:1"].note.as_deref(), Some("everyone's"));
        // Someone with nothing of her own has the deployment's, untouched.
        let mei = cfg.hers("mei").unwrap().mind();
        assert_eq!((mei.calls_per_hour, mei.work.per_day), (60, 4));
        assert!(mei.operators.includes("qq", "A") && !mei.places.contains_key("qq:group:2"));
        assert!(cfg.hers("mei").unwrap().mcp.is_empty());
    }

    #[test]
    fn a_characters_settings_are_checked_and_must_name_a_character_that_runs() {
        // What is not hers to set, or not a setting, is refused at load.
        for wrong in [
            "[ghosts.lin.brain]\nthink = \"b\"\n",
            "[ghosts.lin.mind]\ntempoo = 1.0\n",
            "[ghosts.lin.mind]\nprompts = \"elsewhere\"\n",
            "[ghosts.lin.listen]\naddr = \"127.0.0.1:1\"\n",
        ] {
            let said = Config::parse(&format!("{BRAIN}{wrong}")).unwrap_err();
            assert!(format!("{said:#}").contains("lin"), "{wrong}: {said:#}");
        }
        let cfg = parse("[ghosts.lin.mind]\ntempo = 0.1\n");
        assert!(cfg.check_ids(&["lin", "mei"]).is_ok());
        let said = cfg.check_ids(&["mei"]).unwrap_err().to_string();
        assert!(
            said.contains("[ghosts.lin]") && said.contains("mei"),
            "{said}"
        );
        // A file with no [ghosts] runs whoever it is given.
        assert!(parse("").check_ids(&["anyone"]).is_ok());
    }

    #[test]
    fn operators_are_read_per_channel_and_a_prefix_is_refused_with_why() {
        let none = parse("");
        assert!(none.mind().operators.accounts.is_empty());

        let several =
            parse("[operators]\nqq = [\"A\", \"B\"]\ntelegram = [\"42\"]\nterm = [\"fox\"]\n");
        let ops = several.mind().operators;
        assert_eq!(ops.accounts.len(), 3);
        assert!(
            ops.includes("qq", "B")
                && ops.includes("telegram", "42")
                && ops.includes("term", "fox")
        );
        assert!(!ops.includes("telegram", "B"));

        for old in [
            "[operators]\nprefix = \"fox:\"\nqq = [\"A\"]\n",
            "[ghosts.lin.operators]\nprefix = \"op:\"\n",
        ] {
            let e = Config::parse(&format!("{BRAIN}{old}"))
                .unwrap_err()
                .to_string();
            assert!(e.contains("prefix is gone"), "{e}");
        }
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
            Config::parse(&format!("{BRAIN}[operators]\nreport = \"9pm\"\n"));
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
            Config::parse(&format!("{BRAIN}[places.\"x:y\"]\nadressed_only = true\n"));
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
            Config::parse(&format!("{BRAIN}[network]\nproxies = \"none\"\n"));
        assert!(wrong.is_err());
    }

    #[test]
    fn operators_are_not_read_from_under_mind() {
        let wrong: Result<Config, _> =
            Config::parse(&format!("{BRAIN}[mind.operators]\nqq = [\"A\"]\n"));
        assert!(wrong.is_err());
        // Nor is anything but a list of accounts taken for a channel.
        let wrong: Result<Config, _> =
            Config::parse(&format!("{BRAIN}[operators]\nprefx = \"op:\"\n"));
        assert!(wrong.is_err());
    }
}

#[cfg(test)]
mod example_tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../ghostd.example.toml");
    const README: &str = include_str!("../README.md");

    /// The example with its commented-out settings put back: a line of
    /// comment that is a table's header or a key's assignment.
    fn uncommented(text: &str) -> String {
        text.lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest)
                    if rest.starts_with('[')
                        || rest.split_once(" = ").is_some_and(|(key, _)| {
                            key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                        }) =>
                {
                    rest
                }
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_published_example_runs_as_it_is_and_with_every_commented_line_put_back() {
        for text in [EXAMPLE.to_owned(), uncommented(EXAMPLE)] {
            let cfg = Config::parse(&text).unwrap_or_else(|e| panic!("{e:#}\n{text}"));
            for tier in [&cfg.brain.think, &cfg.brain.tick] {
                assert!(cfg.providers.contains_key(tier), "{tier}");
            }
        }
        let all = Config::parse(&uncommented(EXAMPLE)).unwrap();
        assert!(all.network.proxy.is_some(), "the commented lines were read");
        assert!(!all.hers("her").unwrap().operators.accounts.is_empty());
    }

    #[test]
    fn every_example_in_the_configuration_reference_is_read_by_ghostd() {
        let reference = README
            .split_once("\n## Configuration\n")
            .expect("crates/ghostd/README.md has a Configuration section")
            .1;
        let blocks: Vec<&str> = reference
            .split("```toml\n")
            .skip(1)
            .map(|b| b.split_once("```").unwrap().0)
            .collect();
        assert!(blocks.len() >= 5, "{}", blocks.len());
        for block in blocks {
            let text = format!("[brain]\nthink = \"a\"\ntick = \"a\"\n\n{block}");
            Config::parse(&text).unwrap_or_else(|e| panic!("{e:#}\n{block}"));
        }
    }
}
