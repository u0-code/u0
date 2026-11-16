//! What the desk remembers between runs: where the solver's daemon is, who
//! you are to it, and the sessions you have opened with it. A small JSON
//! file in your home directory. No token is ever written here: it comes
//! from `GHOSTD_TOKEN` in the environment or the `.env` file, as everywhere
//! in Ghost.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// The port the solver's deployment listens on, apart from characters'.
pub const SOLVER_URL: &str = "ws://127.0.0.1:7438";

/// One conversation with the solver: a place of its own on the `desk`
/// channel, so that its thread is its own in her journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub at: DateTime<Utc>,
}

impl Session {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: format!("s{}", now.timestamp_millis()),
            title: String::new(),
            at: now,
        }
    }

    /// What the list calls it: its first line, or nothing yet.
    pub fn label(&self) -> String {
        if self.title.is_empty() {
            "New task".to_owned()
        } else {
            self.title.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub who: String,
    pub solver_url: String,
    /// Newest first.
    pub sessions: Vec<Session>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            who: whoami(),
            solver_url: std::env::var("GHOSTD_URL")
                .ok()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| SOLVER_URL.to_owned()),
            sessions: Vec::new(),
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
        let default = Self::default();
        let sessions = v
            .get("sessions")
            .and_then(Value::as_array)
            .map(|ss| {
                ss.iter()
                    .filter_map(|x| {
                        Some(Session {
                            id: s(x, "id")?,
                            title: s(x, "title").unwrap_or_default(),
                            at: s(x, "at")?.parse().ok()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            who: s(&v, "who").unwrap_or(default.who),
            solver_url: s(&v, "solver_url").unwrap_or(default.solver_url),
            sessions,
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let v = json!({
            "who": self.who,
            "solver_url": self.solver_url,
            "sessions": self.sessions.iter().map(|x| json!({
                "id": x.id, "title": x.title, "at": x.at.to_rfc3339(),
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
