//! Finding what this machine can think with, without being told.
//!
//! Three kinds of thing are looked for: API keys in the environment, the
//! `claude` and `codex` command-line tools, and a local Ollama. Nothing is
//! read from any tool's stored credentials, and nothing is sent to a model:
//! this only says what is there.

use std::time::Duration;

use tokio::process::Command;

use crate::presets::{PRESETS, Preset};

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
    // This machine's own addresses never go through a proxy.
    let Ok(client) = ghost_net::builder().timeout(Duration::from_secs(2)).build() else {
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

/// Look around. Cheap enough to run at every start. What is found is
/// configured as its preset is ([`crate::presets`]).
pub async fn discover() -> Vec<Found> {
    let found_as = |id: &str, what: String, model: Option<&str>, works: bool| {
        Preset::by_id(id).map(|p| Found {
            name: p.id.to_owned(),
            what,
            config: p.lines(model),
            works,
        })
    };
    let mut found: Vec<Found> = PRESETS
        .iter()
        .filter_map(|p| {
            let var = p.key_env?;
            std::env::var(var)
                .is_ok_and(|v| !v.trim().is_empty())
                .then(|| found_as(p.id, format!("${var} is set"), None, true))?
        })
        .collect();
    let (claude, codex, local) = tokio::join!(version("claude"), version("codex"), ollama());
    for (id, program, v) in [("claude-code", "claude", claude), ("codex", "codex", codex)] {
        let Some(v) = v else { continue };
        let what = format!(
            "the {program} CLI is installed ({v}); it answers as whoever it is signed in as"
        );
        found.extend(found_as(id, what, None, false));
    }
    if let Some(model) = local.first() {
        let what = format!("Ollama is running here with {}", local.join(", "));
        found.extend(found_as("ollama", what, Some(model), true));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    #[test]
    fn what_is_found_is_written_as_configuration_that_runs() {
        let ollama = Preset::by_id("ollama").unwrap();
        let found = Found {
            name: ollama.id.to_owned(),
            what: String::new(),
            config: ollama.lines(Some("qwen3:8b")),
            works: true,
        };
        let parsed: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, ProviderConfig>,
        > = toml::from_str(&found.toml()).unwrap();
        assert!(found.toml().contains("model = \"qwen3:8b\""));
        assert_eq!(parsed["providers"]["ollama"].kind(), "openai");
    }
}
