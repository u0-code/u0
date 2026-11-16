//! The models a brain can think with, as the picker offers them.
//!
//! A command-line brain is asked nothing over the network: Claude Code
//! takes the names of its model families (`opus`, `sonnet`, `haiku`) and
//! keeps them current itself, and Codex lists the models your account may
//! use in its own cache, `~/.codex/models_cache.json`, which is read here.
//! Either can also be left to its own default. An API's models are what
//! its `/models` lists, asked for when the picker shows it (`brains.rs`).

use std::path::Path;

use serde_json::Value;

/// A model to choose. An empty `id` is the CLI's own default: no model is
/// named to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

impl Choice {
    fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
        }
    }
}

/// The models a CLI of `kind` offers, its default first; `None` for an API.
pub fn of_cli(kind: &str, home: Option<&Path>) -> Option<Vec<Choice>> {
    match kind {
        "claude-code" => Some(vec![
            Choice::new("", "Default"),
            Choice::new("opus", "Opus"),
            Choice::new("sonnet", "Sonnet"),
            Choice::new("haiku", "Haiku"),
        ]),
        "codex" => {
            let dir = home.map(|h| h.join(".codex"));
            let read = |name: &str| {
                dir.as_ref()
                    .and_then(|d| std::fs::read_to_string(d.join(name)).ok())
            };
            let default = read("config.toml").and_then(|t| codex_default(&t));
            let label = default
                .as_deref()
                .map_or_else(|| "Default".to_owned(), |m| format!("Default · {m}"));
            let mut all = vec![Choice {
                id: String::new(),
                label,
            }];
            all.extend(read("models_cache.json").map_or_else(Vec::new, |j| codex_listed(&j)));
            Some(all)
        }
        _ => None,
    }
}

/// The models in Codex's cache it shows in its own picker, in its order.
pub fn codex_listed(json: &str) -> Vec<Choice> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut listed: Vec<(i64, Choice)> = v
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let id = m.get("slug").and_then(Value::as_str)?;
            let label = m.get("display_name").and_then(Value::as_str).unwrap_or(id);
            let order = m
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((order, Choice::new(id, label)))
        })
        .collect();
    listed.sort_by_key(|(order, _)| *order);
    listed.into_iter().map(|(_, c)| c).collect()
}

/// The model Codex uses when none is named: `model` in its config.
fn codex_default(toml: &str) -> Option<String> {
    let doc = toml.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("model")?.as_str().map(str::to_owned)
}

/// An API's models as choices, the one it is set to first, so that it is
/// there even when the list leaves it out.
pub fn of_api(current: &str, ids: Vec<String>) -> Vec<Choice> {
    let mut all = Vec::with_capacity(ids.len() + 1);
    if !current.is_empty() {
        all.push(Choice::new(current, current));
    }
    all.extend(
        ids.into_iter()
            .filter(|id| id != current)
            .map(|id| Choice::new(&id, &id)),
    );
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_offers_what_it_lists_in_its_own_order_and_hides_the_rest() {
        let json = r#"{"models":[
            {"slug":"b","display_name":"B","visibility":"list","priority":2},
            {"slug":"hidden","visibility":"hide","priority":0},
            {"slug":"a","display_name":"A","visibility":"list","priority":1}
        ]}"#;
        let ids: Vec<_> = codex_listed(json).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(codex_listed("not json").is_empty());
    }

    #[test]
    fn codex_default_is_named_from_its_config() {
        assert_eq!(
            codex_default("model = \"gpt-x\"\n[tui]\n").as_deref(),
            Some("gpt-x")
        );
        assert_eq!(codex_default("[tui]\n"), None);
    }

    #[test]
    fn claude_code_offers_its_families_after_its_default() {
        let all = of_cli("claude-code", None).unwrap();
        assert_eq!(all[0].id, "");
        assert!(all.iter().any(|c| c.id == "opus"));
        assert!(of_cli("openai", None).is_none());
    }

    #[test]
    fn an_api_keeps_the_model_it_is_set_to_even_when_unlisted() {
        let all = of_api("mine", vec!["x".into(), "mine".into()]);
        let ids: Vec<_> = all.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["mine", "x"]);
    }
}
