//! Provider configuration, as it appears in `ghostd.toml`.
//!
//! ```toml
//! [providers.main]
//! kind = "anthropic"
//! model = "claude-sonnet-5"
//!
//! [providers.local]
//! kind = "openai"
//! model = "qwen3"
//! base_url = "http://localhost:11434/v1"
//! api_key = "ollama"
//!
//! [providers.sub]
//! kind = "claude-code"
//! model = "sonnet"
//! ```

use std::time::Duration;

use serde::Deserialize;

use crate::{Brain, Error, Result, anthropic::Anthropic, cli, openai::OpenAi};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProviderConfig {
    Anthropic {
        model: String,
        /// Environment variable holding the key. Default `ANTHROPIC_API_KEY`.
        #[serde(default = "anthropic_key_env")]
        api_key_env: String,
        /// A literal key. Prefer the environment variable.
        api_key: Option<String>,
        base_url: Option<String>,
    },
    Openai {
        model: String,
        #[serde(default = "openai_key_env")]
        api_key_env: String,
        api_key: Option<String>,
        base_url: Option<String>,
        /// `max_tokens` or `max_completion_tokens`. Guessed from `base_url`
        /// when absent.
        max_tokens_param: Option<String>,
        /// Ask the server to enforce JSON output when a caller needs it.
        /// Off by default: not every compatible server accepts the field.
        #[serde(default)]
        json_mode: bool,
    },
    ClaudeCode {
        /// Path to the `claude` binary. Default: found on PATH.
        program: Option<String>,
        model: Option<String>,
        timeout_secs: Option<u64>,
        /// Extra arguments for every run.
        #[serde(default)]
        args: Vec<String>,
    },
    Codex {
        program: Option<String>,
        model: Option<String>,
        timeout_secs: Option<u64>,
        /// Extra arguments for every run, for instance the `-c` settings
        /// that point Codex at a provider other than the one it is signed
        /// in to.
        #[serde(default)]
        args: Vec<String>,
    },
}

fn anthropic_key_env() -> String {
    "ANTHROPIC_API_KEY".into()
}

fn openai_key_env() -> String {
    "OPENAI_API_KEY".into()
}

fn key(literal: Option<&String>, env: &str) -> Result<String> {
    literal
        .cloned()
        .or_else(|| std::env::var(env).ok())
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| Error::MissingKey(env.to_owned()))
}

impl ProviderConfig {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Anthropic { .. } => "anthropic",
            Self::Openai { .. } => "openai",
            Self::ClaudeCode { .. } => "claude-code",
            Self::Codex { .. } => "codex",
        }
    }

    /// Instantiate the provider. Fails early on a missing key.
    pub fn build(&self) -> Result<std::sync::Arc<dyn Brain>> {
        Ok(match self {
            Self::Anthropic {
                model,
                api_key_env,
                api_key,
                base_url,
            } => std::sync::Arc::new(Anthropic::new(
                key(api_key.as_ref(), api_key_env)?,
                model.clone(),
                base_url.clone(),
            )),
            Self::Openai {
                model,
                api_key_env,
                api_key,
                base_url,
                max_tokens_param,
                json_mode,
            } => std::sync::Arc::new(OpenAi::new(
                key(api_key.as_ref(), api_key_env)?,
                model.clone(),
                base_url.clone(),
                max_tokens_param.clone(),
                *json_mode,
            )),
            Self::ClaudeCode {
                program,
                model,
                timeout_secs,
                args,
            } => std::sync::Arc::new(
                cli::ClaudeCode::new(
                    program.clone(),
                    model.clone(),
                    timeout_secs.map(Duration::from_secs),
                )
                .with_args(args.clone()),
            ),
            Self::Codex {
                program,
                model,
                timeout_secs,
                args,
            } => std::sync::Arc::new(
                cli::Codex::new(
                    program.clone(),
                    model.clone(),
                    timeout_secs.map(Duration::from_secs),
                )
                .with_args(args.clone()),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_kind() {
        let text = r#"
[a]
kind = "anthropic"
model = "claude-sonnet-5"

[b]
kind = "openai"
model = "x"
base_url = "http://localhost:11434/v1"
api_key = "ollama"

[c]
kind = "claude-code"

[d]
kind = "codex"
model = "gpt-5-codex"
"#;
        let m: std::collections::BTreeMap<String, ProviderConfig> = toml::from_str(text).unwrap();
        assert_eq!(m["a"].kind(), "anthropic");
        assert_eq!(m["b"].kind(), "openai");
        assert_eq!(m["c"].kind(), "claude-code");
        assert_eq!(m["d"].kind(), "codex");
        assert_eq!(m["b"].build().unwrap().label(), "openai/x");
        assert_eq!(m["c"].build().unwrap().label(), "claude-code/default");
    }

    #[test]
    fn missing_key_fails_early() {
        let cfg = ProviderConfig::Anthropic {
            model: "m".into(),
            api_key_env: "GHOST_TEST_NO_SUCH_KEY".into(),
            api_key: None,
            base_url: None,
        };
        assert!(matches!(cfg.build(), Err(Error::MissingKey(_))));
    }
}
