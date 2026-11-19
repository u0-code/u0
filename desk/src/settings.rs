//! What the desk remembers between runs: where the solver's daemon is, who
//! you are to it, which of your sessions with it was on screen, the other
//! daemons you connect to, and which of what waits on you you have seen. A
//! small JSON file in your home directory. The sessions themselves are the
//! solver's core's to keep; a file from before it kept them still has them
//! here, until they are handed over to it (see `sessions.rs`).
//!
//! No token is ever written here. The solver's comes from `GHOSTD_TOKEN`,
//! and a connection names the variable its token is in, read when the desk
//! connects, in the environment or the `.env` file, as everywhere in Ghost.
//! A file written before there were connections reads as having none.
//!
//! What you have seen in Attention is kept here and not on any daemon:
//! a daemon lists what waits on its operators, and which of it one person
//! has looked at is theirs to remember.
//!
//! It is written whole (see `whole.rs`). A file that cannot be read as
//! JSON is not written over: it is moved aside, with the time in its name,
//! and said so, and the desk starts as if it had none.

use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::sessions::Session;

/// The port the solver's deployment listens on, apart from characters'.
pub const SOLVER_URL: &str = "ws://127.0.0.1:7438";

/// What the desk calls the daemon it runs itself, the first group down the
/// left. No connection may take the name.
pub const THIS_MACHINE: &str = "This machine";

/// A daemon somewhere else that the desk connects to. It only connects:
/// whoever runs that daemon starts and stops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// What the desk calls it. No two share one.
    pub name: String,
    /// `ws://` or `wss://`.
    pub url: String,
    /// The environment variable its token is in, if it needs one.
    pub token_env: Option<String>,
}

impl Connection {
    /// Its token, read now from the variable it names: `None` when it
    /// names none, and an error saying so when the variable is not set.
    pub fn token(&self) -> Result<Option<String>, String> {
        let Some(var) = &self.token_env else {
            return Ok(None);
        };
        match std::env::var(var) {
            Ok(token) if !token.is_empty() => Ok(Some(token)),
            _ => Err(format!("{var} is not set in the environment or .env")),
        }
    }

    /// A connection as typed into the form, checked; or one line saying
    /// what is wrong with it. `taken` are the connections there are.
    ///
    /// A plain `ws://` link to another machine carries the token and
    /// everything she says in the clear, and anyone who can reach the port
    /// can read her journal unless the daemon asks for a token. So it is
    /// taken only with a token named; `wss://` or an ssh tunnel to a
    /// loopback port is better.
    pub fn check(
        name: &str,
        url: &str,
        token_env: &str,
        taken: &[Connection],
    ) -> Result<Self, String> {
        let (name, url, token_env) = (name.trim(), url.trim(), token_env.trim());
        if name.is_empty() {
            return Err("Give the connection a name.".to_owned());
        }
        if name.eq_ignore_ascii_case(THIS_MACHINE) || taken.iter().any(|c| c.name == name) {
            return Err(format!("There is already a connection called {name}."));
        }
        let (plain, rest) = if let Some(rest) = url.strip_prefix("ws://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("wss://") {
            (false, rest)
        } else {
            return Err("The URL begins with ws:// or wss://.".to_owned());
        };
        let Some(host) = host(rest) else {
            return Err("The URL has no host.".to_owned());
        };
        if !token_env.is_empty() && !is_variable(token_env) {
            return Err(
                "Name the variable the token is in, such as STUDIO_TOKEN, not the token."
                    .to_owned(),
            );
        }
        if plain && !loopback(host) && token_env.is_empty() {
            return Err(
                "ws:// to another machine is unencrypted: name a token variable, or tunnel over ssh."
                    .to_owned(),
            );
        }
        Ok(Self {
            name: name.to_owned(),
            url: url.to_owned(),
            token_env: (!token_env.is_empty()).then(|| token_env.to_owned()),
        })
    }
}

/// The host of what follows `ws://`: before any path, without the port,
/// and an IPv6 address without its brackets.
fn host(rest: &str) -> Option<&str> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split_once(']').map(|(h, _)| h)?
    } else {
        authority.rsplit_once(':').map_or(authority, |(h, _)| h)
    };
    (!host.is_empty()).then_some(host)
}

/// Whether `host` is this machine.
fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether `name` is the name of an environment variable, as a person
/// writes one, rather than something pasted in its place.
fn is_variable(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub who: String,
    pub solver_url: String,
    /// The session on screen when the desk last showed one, by id.
    pub open: Option<String>,
    /// The sessions a file from before the core kept them still has,
    /// newest first, until they are handed over to it.
    pub kept: Option<Vec<Session>>,
    /// When their handover began: a session the core began after it is
    /// not one deleted here.
    pub since: Option<DateTime<Utc>>,
    /// The daemons it connects to besides its own, in the order added.
    pub connections: Vec<Connection>,
    /// What you have marked seen in Attention, by its key there.
    pub seen: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            who: whoami(),
            solver_url: std::env::var("GHOSTD_URL")
                .ok()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| SOLVER_URL.to_owned()),
            open: None,
            kept: None,
            since: None,
            connections: Vec::new(),
            seen: BTreeSet::new(),
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
        path().map_or_else(Self::default, |path| Self::load_from(&path))
    }

    /// The settings in the file at `path`; a file that is not JSON is
    /// moved aside first.
    fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("{} could not be read: {e}", path.display());
                }
                return Self::default();
            }
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Self::read(&v),
            Err(e) => {
                let aside =
                    path.with_extension(format!("json.unreadable-{}", Utc::now().timestamp()));
                match std::fs::rename(path, &aside) {
                    Ok(()) => tracing::warn!(
                        "{} is not JSON ({e}); kept as {} and started without it",
                        path.display(),
                        aside.display()
                    ),
                    Err(moved) => tracing::warn!(
                        "{} is not JSON ({e}) and could not be moved aside ({moved}); \
                         it is written over when the desk next saves",
                        path.display()
                    ),
                }
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        self.save_to(&path);
    }

    fn save_to(&self, path: &Path) {
        if let Ok(text) = serde_json::to_string_pretty(&self.written())
            && let Err(e) = crate::whole::write(path, text.as_bytes())
        {
            tracing::warn!("{} could not be written: {e}", path.display());
        }
    }

    /// The settings a file holds. Whatever it leaves out is as it is by
    /// default; an entry that cannot be read is passed over.
    fn read(v: &Value) -> Self {
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let list = |k: &str| {
            v.get(k)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let default = Self::default();
        // An empty list is nothing to hand over: it is not taken as every
        // session the core has having been deleted here.
        let kept: Vec<Session> = list("sessions")
            .iter()
            .filter_map(|x| {
                Some(Session {
                    id: s(x, "id")?,
                    title: s(x, "title").unwrap_or_default(),
                    at: s(x, "at")?.parse().ok()?,
                })
            })
            .collect();
        let connections = list("connections")
            .iter()
            .filter_map(|x| {
                Some(Connection {
                    name: s(x, "name")?,
                    url: s(x, "url")?,
                    token_env: s(x, "token_env").filter(|t| !t.is_empty()),
                })
            })
            .collect();
        let seen = list("seen")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Self {
            who: s(v, "who").unwrap_or(default.who),
            solver_url: s(v, "solver_url").unwrap_or(default.solver_url),
            open: s(v, "open").filter(|o| !o.is_empty()),
            since: s(v, "sessions_since").and_then(|t| t.parse().ok()),
            kept: (!kept.is_empty()).then_some(kept),
            connections,
            seen,
        }
    }

    /// What is written to the file: everything but a token, which is never
    /// the desk's to keep.
    fn written(&self) -> Value {
        let mut v = json!({
            "who": self.who,
            "solver_url": self.solver_url,
            "connections": self.connections.iter().map(|c| {
                let mut v = json!({"name": c.name, "url": c.url});
                if let Some(var) = &c.token_env {
                    v["token_env"] = json!(var);
                }
                v
            }).collect::<Vec<_>>(),
            "seen": self.seen,
        });
        if let Some(open) = &self.open {
            v["open"] = json!(open);
        }
        if let Some(kept) = &self.kept {
            v["sessions"] = json!(
                kept.iter()
                    .map(|x| json!({
                        "id": x.id, "title": x.title, "at": x.at.to_rfc3339(),
                    }))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(since) = &self.since {
            v["sessions_since"] = json!(since.to_rfc3339());
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_json_is_kept_aside_and_never_written_over() {
        let dir = std::env::temp_dir().join(format!("ghost-desk-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("desk.json");
        let torn = "{\"who\": \"me\", \"connections\": [{\"name\": \"stu";
        std::fs::write(&path, torn).unwrap();

        let settings = Settings::load_from(&path);
        assert_eq!(
            settings.connections.len(),
            0,
            "started as if there were none"
        );
        assert!(!path.exists(), "moved, not left to be written over");
        let aside: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().contains("desk.json.unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), torn);

        // What is saved next is a file of its own, read back whole.
        let mut next = settings;
        next.who = "me".to_owned();
        next.save_to(&path);
        assert_eq!(Settings::load_from(&path).who, "me");
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), torn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn named(title: &str) -> Session {
        Session {
            title: title.to_owned(),
            ..Session::new()
        }
    }

    #[test]
    fn a_file_from_before_connections_reads_with_none_and_keeps_its_sessions_to_hand_over() {
        let old = json!({
            "who": "fox",
            "solver_url": "ws://127.0.0.1:7438",
            "sessions": [{"id": "s1", "title": "Free some disk", "at": "2026-11-15T10:00:00Z"}],
        });
        let read = Settings::read(&old);
        assert_eq!(read.who, "fox");
        let kept = read.kept.as_deref().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "Free some disk");
        assert_eq!(read.since, None, "the handover has not begun");
        assert!(read.connections.is_empty());
        assert!(read.seen.is_empty(), "nothing was seen before Attention");
        // Kept until they are handed over, with when that began.
        let mut handing = read.clone();
        handing.since = Some("2026-11-19T09:00:00Z".parse().unwrap());
        assert_eq!(Settings::read(&handing.written()), handing);
    }

    #[test]
    fn once_the_core_keeps_the_sessions_the_file_keeps_only_which_was_open() {
        let settings = Settings {
            open: Some("s1".to_owned()),
            ..Settings::read(&json!({"who": "fox"}))
        };
        let written = settings.written();
        assert!(written.get("sessions").is_none() && written.get("sessions_since").is_none());
        assert_eq!(written["open"], "s1");
        assert_eq!(Settings::read(&written), settings);
        // An empty list is nothing to hand over.
        let empty = Settings::read(&json!({"who": "fox", "sessions": []}));
        assert_eq!(empty.kept, None);
    }

    #[test]
    fn what_was_seen_in_attention_survives_a_round_trip() {
        let settings = Settings {
            who: "fox".to_owned(),
            solver_url: SOLVER_URL.to_owned(),
            open: None,
            kept: None,
            since: None,
            connections: Vec::new(),
            seen: [
                "group:machine/solver/asked-12",
                "group:remote:hk/weiwei/report-9",
            ]
            .map(str::to_owned)
            .into(),
        };
        let written = settings.written();
        assert_eq!(Settings::read(&written), settings);
        assert_eq!(written["seen"][0], "group:machine/solver/asked-12");
    }

    #[test]
    fn connections_survive_a_round_trip_and_no_token_is_written() {
        let settings = Settings {
            who: "fox".to_owned(),
            solver_url: SOLVER_URL.to_owned(),
            open: None,
            kept: Some(vec![named("one")]),
            since: None,
            connections: vec![
                Connection {
                    name: "studio".to_owned(),
                    url: "wss://studio.example/ghost".to_owned(),
                    token_env: Some("STUDIO_TOKEN".to_owned()),
                },
                Connection {
                    name: "tunnel".to_owned(),
                    url: "ws://127.0.0.1:7537".to_owned(),
                    token_env: None,
                },
            ],
            seen: BTreeSet::new(),
        };

        let written = settings.written();
        let back = Settings::read(&written);
        assert_eq!(back, settings);
        assert!(
            !written["connections"][1]
                .as_object()
                .unwrap()
                .contains_key("token_env")
        );
        // The file names the variable; it never holds what is in it.
        let text = written.to_string();
        assert!(text.contains("STUDIO_TOKEN") && !text.contains("\"token\""));
    }

    #[test]
    fn a_connection_needs_a_name_of_its_own_and_a_websocket_url() {
        let taken = [Connection {
            name: "studio".to_owned(),
            url: "wss://studio.example".to_owned(),
            token_env: None,
        }];
        let check = |name: &str, url: &str| Connection::check(name, url, "", &taken);
        assert!(check("", "wss://a.example").is_err());
        assert!(check("studio", "wss://a.example").is_err());
        assert!(check("this machine", "wss://a.example").is_err());
        assert!(check("lab", "https://a.example").is_err());
        assert!(check("lab", "a.example:7437").is_err());
        assert!(check("lab", "wss://").is_err());
        let lab = check("  lab ", " wss://a.example:443/ghost ").unwrap();
        assert_eq!(
            (lab.name.as_str(), lab.url.as_str()),
            ("lab", "wss://a.example:443/ghost")
        );
        assert_eq!(lab.token_env, None);
    }

    #[test]
    fn plain_ws_to_another_machine_is_refused_unless_a_token_is_named() {
        let check = |url: &str, var: &str| Connection::check("lab", url, var, &[]);
        for here in [
            "ws://127.0.0.1:7537",
            "ws://localhost:7437",
            "ws://[::1]:7437",
            "ws://127.0.0.2",
        ] {
            assert!(check(here, "").is_ok(), "{here}");
        }
        let refused = check("ws://192.168.1.20:7437", "").unwrap_err();
        assert!(refused.contains("ssh"), "{refused}");
        assert!(check("ws://localhost.example:7437", "").is_err());
        assert!(check("ws://192.168.1.20:7437", "LAB_TOKEN").is_ok());
        assert!(check("wss://lab.example", "").is_ok());
    }

    #[test]
    fn a_token_is_named_by_its_variable_and_never_pasted_in() {
        let check = |var: &str| Connection::check("lab", "wss://lab.example", var, &[]);
        assert_eq!(
            check("LAB_TOKEN").unwrap().token_env.as_deref(),
            Some("LAB_TOKEN")
        );
        assert!(check("_lab2").is_ok());
        for pasted in ["pasted-in!", "2abc", "LAB TOKEN", "a-b"] {
            assert!(check(pasted).is_err(), "{pasted}");
        }
        // A variable that is not set says so when the desk connects.
        let unset = Connection {
            name: "lab".to_owned(),
            url: "wss://lab.example".to_owned(),
            token_env: Some("GHOST_DESK_TEST_NOTHING_HERE".to_owned()),
        };
        assert!(
            unset
                .token()
                .unwrap_err()
                .contains("GHOST_DESK_TEST_NOTHING_HERE")
        );
        assert_eq!(
            Connection {
                token_env: None,
                ..unset
            }
            .token(),
            Ok(None)
        );
    }
}
