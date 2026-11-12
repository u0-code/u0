//! Finding what this machine can think with, without being told.
//!
//! Three kinds of thing are looked for: API keys in the environment, the
//! `claude` and `codex` command-line tools, and a local Ollama. Nothing is
//! read from any tool's stored credentials, and nothing is sent to a model:
//! this only says what is there.

use std::time::Duration;

use tokio::process::Command;

/// Something found, and the configuration that would use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// A name for it in `[providers]`.
    pub name: String,
    /// What was found, in a few words.
    pub what: String,
    /// The lines to put under `[providers.<name>]`.
    pub config: Vec<String>,
    /// Whether it can run tools (see [`Brain::step`](crate::Brain::step)),
    /// or only answer.
    pub works: bool,
}

impl Found {
    /// A block ready to paste into `ghostd.toml`.
    pub fn toml(&self) -> String {
        format!("[providers.{}]\n{}\n", self.name, self.config.join("\n"))
    }
}

/// Keys this looks for, and what each one reaches.
const KEYS: &[(&str, &str, &[&str])] = &[
    (
        "DEEPSEEK_API_KEY",
        "deepseek",
        &[
            r#"kind = "openai""#,
            r#"model = "deepseek-flash""#,
            r#"base_url = "https://api.deepseek.com""#,
            r#"api_key_env = "DEEPSEEK_API_KEY""#,
            "json_mode = true",
        ],
    ),
    (
        "ANTHROPIC_API_KEY",
        "anthropic",
        &[r#"kind = "anthropic""#, r#"model = "claude-sonnet-5-5""#],
    ),
    (
        "OPENAI_API_KEY",
        "openai",
        &[
            r#"kind = "openai""#,
            r#"model = "" # the model to use"#,
            "json_mode = true",
        ],
    ),
    (
        "OPENROUTER_API_KEY",
        "openrouter",
        &[
            r#"kind = "openai""#,
            r#"model = "" # for example deepseek/deepseek-flash"#,
            r#"base_url = "https://openrouter.ai/api/v1""#,
            r#"api_key_env = "OPENROUTER_API_KEY""#,
        ],
    ),
];

async fn version(program: &str) -> Option<String> {
    let run = Command::new(program).arg("--version").output();
    let out = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .ok()?
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    })
}

async fn ollama() -> Vec<String> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
    else {
        return Vec::new();
    };
    let Ok(resp) = client.get("http://127.0.0.1:11434/api/tags").send().await else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return Vec::new();
    };
    v["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Look around. Cheap enough to run at every start.
pub async fn discover() -> Vec<Found> {
    let mut found = Vec::new();
    for (var, name, config) in KEYS {
        if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
            found.push(Found {
                name: (*name).to_owned(),
                what: format!("${var} is set"),
                config: config.iter().map(|&l| l.to_owned()).collect(),
                works: true,
            });
        }
    }
    let (claude, codex, local) = tokio::join!(version("claude"), version("codex"), ollama());
    if let Some(v) = claude {
        found.push(Found {
            name: "claude-code".into(),
            what: format!(
                "the claude CLI is installed ({v}); it answers as whoever it is signed in as"
            ),
            config: vec![r#"kind = "claude-code""#.into()],
            works: false,
        });
    }
    if let Some(v) = codex {
        found.push(Found {
            name: "codex".into(),
            what: format!(
                "the codex CLI is installed ({v}); it answers as whoever it is signed in as"
            ),
            config: vec![r#"kind = "codex""#.into()],
            works: false,
        });
    }
    if let Some(model) = local.first() {
        found.push(Found {
            name: "ollama".into(),
            what: format!("Ollama is running here with {}", local.join(", ")),
            config: vec![
                r#"kind = "openai""#.into(),
                format!(r#"model = "{model}""#),
                r#"base_url = "http://127.0.0.1:11434/v1""#.into(),
                r#"api_key = "ollama""#.into(),
            ],
            works: true,
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    #[test]
    fn every_suggested_block_is_valid_configuration() {
        for (var, name, config) in KEYS {
            let found = Found {
                name: (*name).to_owned(),
                what: (*var).to_owned(),
                config: config.iter().map(|&l| l.to_owned()).collect(),
                works: true,
            };
            let parsed: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, ProviderConfig>,
            > = toml::from_str(&found.toml()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(parsed["providers"].contains_key(*name));
        }
    }
}
