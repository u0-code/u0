//! What the desk remembers between runs: which of your sessions with the
//! solver was on screen, the other daemons you connect to, and which of
//! what waits on you you have seen. A small JSON file in your home
//! directory. The sessions themselves are the solver's core's to keep.
//!
//! Who you are and where the solver's daemon is are read from the
//! environment each run, `USER` and `GHOSTD_URL`, and never kept: a file
//! that held them would hold whatever they were the first time.
//!
//! No token is ever written here. The solver's comes from `GHOSTD_TOKEN`,
//! and a connection names the variable its token is in, read when the desk
//! connects, in the environment or the `.env` file, as everywhere in Ghost.
//!
//! What you have seen in Attention is kept here and not on any daemon:
//! a daemon lists what waits on its operators, and which of it one person
//! has looked at is theirs to remember.
//!
//! It is written whole (see `whole.rs`). A file that cannot be read as
//! these settings is not written over: it is moved aside, with the time in
//! its name, and said so, and the desk starts as if it had none.

use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// What the desk calls the daemon it runs itself, the first group down the
/// left. No connection may take the name.
pub const THIS_MACHINE: &str = "This machine";

/// A daemon somewhere else that the desk connects to. It only connects:
/// whoever runs that daemon starts and stops it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// What the desk calls it. No two share one.
    pub name: String,
    /// `ws://` or `wss://`.
    pub url: String,
    /// The environment variable its token is in, if it needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// What the desk is set to. What the file leaves out is as it is by
/// default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Who sits at the desk: `USER`, read each run.
    #[serde(skip)]
    pub who: String,
    /// The solver's daemon: `GHOSTD_URL`, or else the core the desk runs
    /// itself. Read each run.
    #[serde(skip)]
    pub solver_url: String,
    /// The session on screen when the desk last showed one, by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// The daemons it connects to besides its own, in the order added.
    pub connections: Vec<Connection>,
    /// What you have marked seen in Attention, by its key there.
    pub seen: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        Self {
            who: var("USER").unwrap_or_else(|| "you".to_owned()),
            solver_url: var("GHOSTD_URL").unwrap_or_else(crate::solver::url),
            open: None,
            connections: Vec::new(),
            seen: BTreeSet::new(),
        }
    }
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

    /// The settings in the file at `path`; a file that is not these
    /// settings is moved aside first.
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
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(e) => {
                let aside =
                    path.with_extension(format!("json.unreadable-{}", Utc::now().timestamp()));
                match std::fs::rename(path, &aside) {
                    Ok(()) => tracing::warn!(
                        "{} cannot be read ({e}); kept as {} and started without it",
                        path.display(),
                        aside.display()
                    ),
                    Err(moved) => tracing::warn!(
                        "{} cannot be read ({e}) and could not be moved aside ({moved}); \
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
        if let Ok(text) = serde_json::to_string_pretty(self)
            && let Err(e) = crate::whole::write(path, text.as_bytes())
        {
            tracing::warn!("{} could not be written: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// What is written to the file.
    fn written(settings: &Settings) -> Value {
        serde_json::to_value(settings).unwrap()
    }

    /// The settings a file holds.
    fn read(v: Value) -> Settings {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_file_that_cannot_be_read_is_kept_aside_and_never_written_over() {
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
        next.open = Some("s1".to_owned());
        next.save_to(&path);
        assert_eq!(Settings::load_from(&path).open.as_deref(), Some("s1"));
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), torn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_keeps_which_session_was_open_and_none_of_the_sessions() {
        let settings = Settings {
            open: Some("s1".to_owned()),
            ..read(json!({}))
        };
        let written = written(&settings);
        assert!(written.get("sessions").is_none());
        assert_eq!(written["open"], "s1");
        assert_eq!(read(written), settings);
    }

    #[test]
    fn who_you_are_and_where_the_solver_is_are_never_kept() {
        let settings = Settings {
            who: "fox".to_owned(),
            solver_url: "ws://elsewhere:7438".to_owned(),
            ..Settings::default()
        };
        let written = written(&settings);
        assert!(written.get("who").is_none() && written.get("solver_url").is_none());
        // A file that names them does not change them.
        let named = read(json!({"who": "fox", "solver_url": "ws://elsewhere:7438"}));
        assert_eq!(named, Settings::default());
    }

    #[test]
    fn what_was_seen_in_attention_survives_a_round_trip() {
        let settings = Settings {
            open: None,
            connections: Vec::new(),
            seen: [
                "group:machine/solver/asked-12",
                "group:remote:hk/weiwei/report-9",
            ]
            .map(str::to_owned)
            .into(),
            ..Settings::default()
        };
        let written = written(&settings);
        assert_eq!(read(written.clone()), settings);
        assert_eq!(written["seen"][0], "group:machine/solver/asked-12");
    }

    #[test]
    fn connections_survive_a_round_trip_and_no_token_is_written() {
        let settings = Settings {
            open: None,
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
            ..Settings::default()
        };

        let written = written(&settings);
        let back = read(written.clone());
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
