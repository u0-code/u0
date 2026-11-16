//! What the desk remembers between runs: which daemons to reach, which of
//! their characters to follow, and who you are to them. A small JSON file
//! in your home directory. No token is ever written here: it comes from
//! `GHOSTD_TOKEN` in the environment or the `.env` file, as everywhere in
//! Ghost.

use std::path::PathBuf;

use serde_json::{Value, json};

/// A daemon the desk reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSeed {
    pub url: String,
}

/// A character to follow on a daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSeed {
    pub url: String,
    /// Her directory's name on the daemon; `None` when it runs only one.
    pub ghost: Option<String>,
    /// The one that solves things on this machine rather than living online.
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub who: String,
    pub daemons: Vec<DaemonSeed>,
    pub characters: Vec<CharacterSeed>,
}

impl Default for Settings {
    fn default() -> Self {
        // A first run follows the daemon GHOSTD_URL names, else the one on
        // this machine at the usual port.
        let url = std::env::var("GHOSTD_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(ghost_link::default_url);
        Self {
            who: whoami(),
            daemons: vec![DaemonSeed { url: url.clone() }],
            characters: vec![CharacterSeed {
                url,
                ghost: None,
                local: false,
            }],
        }
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "you".to_owned())
}

pub fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("ghost-desk")
            .join("desk.json"),
    )
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return Self::default();
        };
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let daemons = v
            .get("daemons")
            .and_then(Value::as_array)
            .map(|ds| {
                ds.iter()
                    .filter_map(|d| s(d, "url").map(|url| DaemonSeed { url }))
                    .collect()
            })
            .unwrap_or_default();
        let characters = v
            .get("characters")
            .and_then(Value::as_array)
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| {
                        Some(CharacterSeed {
                            url: s(c, "url")?,
                            ghost: s(c, "ghost"),
                            local: c.get("local").and_then(Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            who: s(&v, "who").unwrap_or_else(whoami),
            daemons,
            characters,
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let v = json!({
            "who": self.who,
            "daemons": self.daemons.iter().map(|d| json!({"url": d.url})).collect::<Vec<_>>(),
            "characters": self.characters.iter().map(|c| json!({
                "url": c.url, "ghost": c.ghost, "local": c.local,
            })).collect::<Vec<_>>(),
        });
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(path, text);
        }
    }
}
