//! The providers Ghost knows how to think with, each as a person would
//! otherwise look it up in the provider's documentation: the kind of API,
//! its base URL, the variable its key is conventionally kept in, and a
//! model to start from. Every API here but Anthropic's speaks the
//! `OpenAI`-compatible protocol, which is what `kind = "openai"` means.
//!
//! This is the one list of them. What the machine already has is found
//! from it ([`crate::discover`]), a new home's configuration is written
//! from it, and a console offers it to add a brain from.

/// One provider, and what a brain on it is configured with.
#[derive(Debug, PartialEq, Eq)]
pub struct Preset {
    /// What a brain from it is saved as unless named otherwise, and how a
    /// brain already configured is said to be this one.
    pub id: &'static str,
    /// The provider's name, as shown.
    pub name: &'static str,
    /// `kind` in `[providers.<id>]`.
    pub kind: &'static str,
    pub base_url: Option<&'static str>,
    /// The variable its key is conventionally kept in; `None` for one that
    /// needs no key, or is signed in to.
    pub key_env: Option<&'static str>,
    /// A model to start from; `None` for a command-line agent's own default,
    /// or where there is none to suggest.
    pub model: Option<&'static str>,
    /// What else a brain on it is written with, as TOML lines.
    extra: &'static [&'static str],
    /// Hosts its base URL is on, by which a brain already configured is
    /// known for this one.
    hosts: &'static [&'static str],
}

const fn api(
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    key_env: &'static str,
    model: &'static str,
    hosts: &'static [&'static str],
) -> Preset {
    Preset {
        id,
        name,
        kind: "openai",
        base_url: Some(base_url),
        key_env: Some(key_env),
        model: Some(model),
        extra: &[],
        hosts,
    }
}

/// How long a command-line agent is given to answer once.
const CLI_TIMEOUT: &str = "timeout_secs = 120";

/// Every preset, in the order they are offered. Each base URL, key variable
/// and model is the one the provider's own documentation gives.
pub const PRESETS: &[Preset] = &[
    // DeepSeek takes `https://api.deepseek.com` with or without `/v1`; its
    // own examples leave it out. It enforces JSON when asked.
    Preset {
        extra: &["json_mode = true"],
        ..api(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com",
            "DEEPSEEK_API_KEY",
            "deepseek-flash",
            &["deepseek.com"],
        )
    },
    api(
        "openai",
        "OpenAI",
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        "gpt-6-astra",
        &["api.openai.com"],
    ),
    Preset {
        kind: "anthropic",
        ..api(
            "anthropic",
            "Anthropic",
            "https://api.anthropic.com",
            "ANTHROPIC_API_KEY",
            "claude-opus-5-5",
            &["anthropic.com"],
        )
    },
    api(
        "gemini",
        "Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "GEMINI_API_KEY",
        "gemini-3.8-flash",
        &["generativelanguage.googleapis.com"],
    ),
    api(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
        "openrouter/auto",
        &["openrouter.ai"],
    ),
    api(
        "kimi",
        "Kimi",
        "https://api.moonshot.ai/v1",
        "MOONSHOT_API_KEY",
        "kimi-k3.1-pro",
        &["moonshot.ai", "moonshot.cn", "kimi.ai"],
    ),
    api(
        "qwen",
        "Qwen",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        "DASHSCOPE_API_KEY",
        "qwen4-max",
        &["aliyuncs.com"],
    ),
    api(
        "glm",
        "GLM",
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
        "glm-5.3",
        &["z.ai", "bigmodel.cn"],
    ),
    api(
        "grok",
        "Grok",
        "https://api.x.ai/v1",
        "XAI_API_KEY",
        "grok-4.7",
        &["x.ai"],
    ),
    api(
        "mistral",
        "Mistral",
        "https://api.mistral.ai/v1",
        "MISTRAL_API_KEY",
        "mistral-large-latest",
        &["mistral.ai"],
    ),
    api(
        "groq",
        "Groq",
        "https://api.groq.com/openai/v1",
        "GROQ_API_KEY",
        "openai/gpt-oss-120b",
        &["groq.com"],
    ),
    // Ollama checks no key, but the client sends one: this stand-in, which
    // is not a secret.
    Preset {
        key_env: None,
        extra: &[r#"api_key = "ollama""#],
        ..api(
            "ollama",
            "Ollama",
            "http://localhost:11434/v1",
            "",
            "gpt-oss:20b",
            &["localhost:11434", "127.0.0.1:11434", "ollama.com"],
        )
    },
    Preset {
        id: "claude-code",
        name: "Claude Code",
        kind: "claude-code",
        base_url: None,
        key_env: None,
        model: Some("sonnet"),
        extra: &[CLI_TIMEOUT],
        hosts: &[],
    },
    Preset {
        id: "codex",
        name: "Codex",
        kind: "codex",
        base_url: None,
        key_env: None,
        model: None,
        extra: &[CLI_TIMEOUT],
        hosts: &[],
    },
    // Any other `OpenAI`-compatible server: everything but its kind is
    // whoever adds it's to say.
    Preset {
        id: "custom",
        name: "Custom",
        kind: "openai",
        base_url: None,
        key_env: None,
        model: None,
        extra: &[],
        hosts: &[],
    },
];

impl Preset {
    pub fn by_id(id: &str) -> Option<&'static Self> {
        PRESETS.iter().find(|p| p.id == id)
    }

    /// The preset a brain configured with `kind`, its API at `base_url`,
    /// is; `None` for one there is no preset for.
    pub fn of(kind: &str, base_url: Option<&str>) -> Option<&'static Self> {
        match kind {
            "claude-code" | "codex" | "anthropic" => return Self::by_id(kind),
            _ => {}
        }
        let Some(base) = base_url.filter(|b| !b.is_empty()) else {
            // The `OpenAI`-compatible client's own default is `OpenAI`.
            return Self::by_id("openai");
        };
        let (authority, host) = authority(base);
        PRESETS.iter().filter(|p| p.kind == kind).find(|p| {
            p.hosts
                .iter()
                .any(|h| authority == *h || host == *h || host.ends_with(&format!(".{h}")))
        })
    }

    /// The lines of its `[providers.<id>]`, on `model` in place of its own
    /// if one is given.
    pub fn lines(&self, model: Option<&str>) -> Vec<String> {
        let quoted = |v: &str| toml::Value::from(v).to_string();
        let mut lines = vec![format!("kind = {}", quoted(self.kind))];
        if let Some(model) = model.or(self.model) {
            lines.push(format!("model = {}", quoted(model)));
        }
        if let Some(base) = self.base_url {
            lines.push(format!("base_url = {}", quoted(base)));
        }
        if let Some(var) = self.key_env {
            lines.push(format!("api_key_env = {}", quoted(var)));
        }
        lines.extend(self.extra.iter().map(|&l| l.to_owned()));
        lines
    }

    /// Its `[providers.<id>]` as a table.
    pub fn table(&self) -> toml::Table {
        toml::from_str(&self.lines(None).join("\n")).unwrap_or_default()
    }
}

/// The authority (`host:port`) and the host of a URL, lower case.
pub fn authority(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(authority.as_str(), |(h, _)| h)
        .to_owned();
    (authority, host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    #[test]
    fn every_preset_is_configuration_that_runs_and_none_carries_a_secret() {
        let mut ids = std::collections::HashSet::new();
        for p in PRESETS {
            assert!(ids.insert(p.id), "{} twice", p.id);
            let table = p.table();
            assert_eq!(
                table.len(),
                p.lines(None).len(),
                "{}: every line read",
                p.id
            );
            // An API with no model to suggest is given one when it is added.
            let mut added = table.clone();
            if p.model.is_none() && p.kind == "openai" {
                added.insert("model".into(), "m".into());
            }
            let cfg: ProviderConfig = toml::Value::Table(added)
                .try_into()
                .unwrap_or_else(|e| panic!("{}: {e}", p.id));
            assert_eq!(cfg.kind(), p.kind);
            if let Some(key) = table.get("api_key") {
                assert_eq!(key.as_str(), Some("ollama"), "only Ollama's stand-in");
            }
        }
    }

    #[test]
    fn deepseek_is_its_current_model_at_the_address_its_documentation_gives() {
        let deepseek = Preset::by_id("deepseek").unwrap();
        assert_eq!(
            deepseek.lines(None),
            [
                r#"kind = "openai""#,
                r#"model = "deepseek-flash""#,
                r#"base_url = "https://api.deepseek.com""#,
                r#"api_key_env = "DEEPSEEK_API_KEY""#,
                "json_mode = true",
            ]
        );
        // The model is one this crate knows the window of.
        assert!(crate::known_window(deepseek.model.unwrap()).is_some());
    }

    #[test]
    fn a_configured_brain_is_known_by_where_its_api_is() {
        let id = |kind: &str, base: Option<&str>| Preset::of(kind, base).map(|p| p.id);
        assert_eq!(
            id("openai", Some("https://api.deepseek.com/v1")),
            Some("deepseek")
        );
        assert_eq!(
            id(
                "openai",
                Some("https://generativelanguage.googleapis.com/v1beta/openai/")
            ),
            Some("gemini")
        );
        assert_eq!(
            id("openai", Some("https://open.bigmodel.cn/api/paas/v4")),
            Some("glm")
        );
        assert_eq!(
            id("openai", Some("http://localhost:11434/v1")),
            Some("ollama")
        );
        assert_eq!(id("openai", Some("https://api.x.ai/v1")), Some("grok"));
        assert_eq!(
            id("openai", Some("https://max.ai/v1")),
            None,
            "x.ai is a host, not a suffix"
        );
        assert_eq!(id("openai", None), Some("openai"));
        assert_eq!(id("openai", Some("http://localhost:8000/v1")), None);
        assert_eq!(id("claude-code", None), Some("claude-code"));
        assert_eq!(
            id("anthropic", Some("https://proxy.example")),
            Some("anthropic")
        );
    }

    #[test]
    fn an_authority_and_host_are_read_from_a_url() {
        assert_eq!(
            authority("http://localhost:11434/v1"),
            ("localhost:11434".to_owned(), "localhost".to_owned())
        );
        assert_eq!(
            authority("https://API.x.ai/v1"),
            ("api.x.ai".to_owned(), "api.x.ai".to_owned())
        );
    }
}
