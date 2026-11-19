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

use crate::{Brain, CliAgent, CliKind, Error, Result, anthropic::Anthropic, cli, openai::OpenAi};

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
        /// The model's context window in tokens, when it is not one this
        /// crate knows. Work is paced by it.
        context_window: Option<u64>,
        /// The model takes images, and is shown the ones people send.
        #[serde(default)]
        vision: bool,
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
        /// The model's context window in tokens, when it is not one this
        /// crate knows. Work is paced by it.
        context_window: Option<u64>,
        /// The most the model writes in one answer, thinking included,
        /// when it is not one this crate knows.
        max_output: Option<u32>,
        /// The server takes `reasoning_effort` and `thinking`, so the model
        /// can be told how hard to think. Known for DeepSeek's models.
        reasoning_effort: Option<bool>,
        /// The model takes images, and is shown the ones people send. Off
        /// by default: a server sent an image it cannot read refuses the
        /// whole request.
        #[serde(default)]
        vision: bool,
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
                context_window,
                vision,
            } => std::sync::Arc::new(
                Anthropic::new(
                    key(api_key.as_ref(), api_key_env)?,
                    model.clone(),
                    base_url.clone(),
                )
                .with_window(*context_window)
                .with_vision(*vision),
            ),
            Self::Openai {
                model,
                api_key_env,
                api_key,
                base_url,
                max_tokens_param,
                json_mode,
                context_window,
                max_output,
                reasoning_effort,
                vision,
            } => std::sync::Arc::new(
                OpenAi::new(
                    key(api_key.as_ref(), api_key_env)?,
                    model.clone(),
                    base_url.clone(),
                    max_tokens_param.clone(),
                    *json_mode,
                )
                .with_window(*context_window)
                .with_max_output(*max_output)
                .with_effort(*reasoning_effort)
                .with_vision(*vision),
            ),
            Self::ClaudeCode {
                program,
                model,
                timeout_secs,
                args,
            } => std::sync::Arc::new(cli::ClaudeCode::new(
                cli_agent(CliKind::Claude, program.as_deref(), model.as_deref(), args),
                timeout_secs.map(Duration::from_secs),
            )),
            Self::Codex {
                program,
                model,
                timeout_secs,
                args,
            } => std::sync::Arc::new(cli::Codex::new(
                cli_agent(CliKind::Codex, program.as_deref(), model.as_deref(), args),
                timeout_secs.map(Duration::from_secs),
            )),
        })
    }

    /// The command-line agent this provider is, if it is one: what a piece
    /// of work can hand a part to.
    pub fn agent(&self) -> Option<CliAgent> {
        match self {
            Self::ClaudeCode {
                program,
                model,
                args,
                ..
            } => Some(cli_agent(
                CliKind::Claude,
                program.as_deref(),
                model.as_deref(),
                args,
            )),
            Self::Codex {
                program,
                model,
                args,
                ..
            } => Some(cli_agent(
                CliKind::Codex,
                program.as_deref(),
                model.as_deref(),
                args,
            )),
            Self::Anthropic { .. } | Self::Openai { .. } => None,
        }
    }
}

/// An agent of `kind` as configured, run as the program it installs as
/// unless another is named.
fn cli_agent(
    kind: CliKind,
    program: Option<&str>,
    model: Option<&str>,
    args: &[String],
) -> CliAgent {
    let installed_as = match kind {
        CliKind::Claude => "claude",
        CliKind::Codex => "codex",
    };
    CliAgent {
        kind,
        program: program.unwrap_or(installed_as).to_owned(),
        model: model.map(str::to_owned),
        args: args.to_vec(),
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
    fn only_a_command_line_provider_is_an_agent_and_it_runs_as_what_it_installs_as() {
        let text = r#"
[api]
kind = "openai"
model = "x"

[claude]
kind = "claude-code"

[codex]
kind = "codex"
program = "/opt/codex"
args = ["-c", "x=1"]
"#;
        let m: std::collections::BTreeMap<String, ProviderConfig> = toml::from_str(text).unwrap();
        assert!(m["api"].agent().is_none());
        assert_eq!(m["claude"].agent().unwrap().program, "claude");
        let codex = m["codex"].agent().unwrap();
        assert_eq!(codex.kind, CliKind::Codex);
        assert_eq!(codex.program, "/opt/codex");
        assert_eq!(codex.args, ["-c", "x=1"]);
    }

    #[test]
    fn a_provider_sees_only_when_it_says_so() {
        let text = r#"
[blind]
kind = "openai"
model = "deepseek-flash"
api_key = "x"

[eyes]
kind = "openai"
model = "deepseek-flash"
api_key = "x"
vision = true

[claude]
kind = "anthropic"
model = "claude-sonnet-5"
api_key = "x"
vision = true

[cli]
kind = "claude-code"
"#;
        let m: std::collections::BTreeMap<String, ProviderConfig> = toml::from_str(text).unwrap();
        let sees = |k: &str| m[k].build().unwrap().sees();
        assert!(!sees("blind"));
        assert!(sees("eyes"));
        assert!(sees("claude"));
        assert!(!sees("cli"));
    }

    #[test]
    fn missing_key_fails_early() {
        let cfg = ProviderConfig::Anthropic {
            model: "m".into(),
            api_key_env: "GHOST_TEST_NO_SUCH_KEY".into(),
            api_key: None,
            base_url: None,
            context_window: None,
            vision: false,
        };
        assert!(matches!(cfg.build(), Err(Error::MissingKey(_))));
    }
}
