//! An organisation's policy for this machine, as t3code's managed policy:
//! a file an administrator puts where only an administrator can write
//! (`/Library/Application Support/Ghost/policy.json` on a Mac,
//! `/etc/ghost/policy.json` elsewhere), read once as Ghost starts. It
//! names the organisation, and sets settings: a default where you have
//! chosen nothing, or, locked, a value you cannot change.
//!
//! ```json
//! {
//!   "organization": "Example Corp",
//!   "support_url": "https://it.example.com/ghost",
//!   "settings": {
//!     "telemetry": { "value": 0, "locked": true },
//!     "check_updates": { "value": false, "locked": true },
//!     "branch_prefix": { "value": "corp/", "locked": false }
//!   },
//!   "limits": {
//!     "agents": ["claude-acp", "codex-acp"],
//!     "remote": true,
//!     "tunnel": false,
//!     "webhooks": false
//!   }
//! }
//! ```
//!
//! Each key of `settings` is a top-level key of `settings.json`. A policy
//! that does not read, or names a key there is not, or a value of the
//! wrong kind, keeps the settings from loading at all: nothing runs half
//! governed. `limits` are what no setting undoes, each enforced by the
//! server where the thing is done (`allows`); one left out limits nothing.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Who sets it, said beside each setting it locks.
    pub organization: String,
    /// Where to ask about it.
    #[serde(default)]
    pub support_url: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, Managed>,
    #[serde(default)]
    pub limits: Limits,
}

/// What the policy keeps Ghost from doing, as t3code's `limits`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The agents that may run, by their registry id; none, any.
    pub agents: Option<Vec<String>>,
    /// False: no other device links to this machine (it does not listen).
    pub remote: Option<bool>,
    /// False: this machine is reached through no cloudflared tunnel.
    pub tunnel: Option<bool>,
    /// False: no webhook task runs.
    pub webhooks: Option<bool>,
}

/// What a policy may forbid.
#[derive(Debug, Clone, Copy)]
pub enum Limit<'a> {
    /// The agent of this registry id running.
    Agent(&'a str),
    Remote,
    Tunnel,
    Webhooks,
}

/// Whether this machine's policy allows `what`; why not, where it does
/// not.
pub fn allows(what: Limit) -> Result<(), String> {
    match current() {
        Ok(Some(policy)) => policy.allows(what),
        _ => Ok(()),
    }
}

/// A setting the policy sets.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Managed {
    pub value: serde_json::Value,
    /// Set for good, not only as the default.
    #[serde(default)]
    pub locked: bool,
}

/// Where the policy is put.
fn path() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/Ghost/policy.json")
    } else {
        PathBuf::from("/etc/ghost/policy.json")
    }
}

/// The policy of this machine, read once: none where there is none; why
/// it does not read, where it does not.
pub fn current() -> &'static Result<Option<Policy>, String> {
    static POLICY: OnceLock<Result<Option<Policy>, String>> = OnceLock::new();
    POLICY.get_or_init(|| read(&path()))
}

/// The organisation that locks the setting `key`, if one does.
pub fn locked(key: &str) -> Option<&'static str> {
    let policy = current().as_ref().ok()?.as_ref()?;
    (policy.settings.get(key)?.locked).then_some(policy.organization.as_str())
}

fn read(path: &Path) -> Result<Option<Policy>, String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "The policy at {} cannot be read: {e}",
            path.display()
        )),
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("The policy at {} does not read: {e}", path.display())),
    }
}

impl Policy {
    fn allows(&self, what: Limit) -> Result<(), String> {
        let l = &self.limits;
        let (allowed, thing) = match what {
            Limit::Agent(id) => (
                (l.agents.as_ref()).is_none_or(|a| a.iter().any(|x| x == id)),
                format!("the agent {id}"),
            ),
            Limit::Remote => (l.remote != Some(false), "other devices to link here".into()),
            Limit::Tunnel => (l.tunnel != Some(false), "a tunnel to this machine".into()),
            Limit::Webhooks => (l.webhooks != Some(false), "webhook tasks".into()),
        };
        if allowed {
            Ok(())
        } else {
            Err(format!(
                "{}'s policy does not allow {thing}.",
                self.organization
            ))
        }
    }

    /// `settings` (settings.json as read) as the policy has them: its
    /// defaults where they hold nothing, its locked values whatever they
    /// hold. A key settings.json has no place for is refused.
    pub fn apply(
        &self,
        settings: &mut serde_json::Map<String, serde_json::Value>,
        known: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        for (key, managed) in &self.settings {
            if !known.contains_key(key) {
                return Err(format!(
                    "{}'s policy sets \"{key}\", which is not a setting",
                    self.organization
                ));
            }
            if managed.locked || !settings.contains_key(key) {
                settings.insert(key.clone(), managed.value.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(json: &str) -> Policy {
        serde_json::from_str(json).unwrap()
    }

    fn map(json: &str) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_locked_value_wins_and_a_default_fills_only_what_is_not_chosen() {
        let p = policy(
            r#"{"organization": "Corp", "settings": {
                "twelve": {"value": true, "locked": true},
                "glass": {"value": true},
                "contrast": {"value": true}}}"#,
        );
        let mut mine = map(r#"{"twelve": false, "glass": false}"#);
        let known = map(r#"{"twelve": 0, "glass": 0, "contrast": 0}"#);
        p.apply(&mut mine, &known).unwrap();
        assert_eq!(
            mine,
            map(r#"{"twelve": true, "glass": false, "contrast": true}"#)
        );
    }

    #[test]
    fn a_policy_naming_no_setting_is_refused() {
        let p = policy(r#"{"organization": "Corp", "settings": {"nope": {"value": 1}}}"#);
        let why = p.apply(&mut map("{}"), &map("{}")).unwrap_err();
        assert!(why.contains("\"nope\""));
    }

    #[test]
    fn a_limit_forbids_only_what_it_names_and_says_whose_it_is() {
        let p = policy(
            r#"{"organization": "Corp", "limits": {
                "agents": ["claude-acp"], "tunnel": false, "remote": true}}"#,
        );
        assert!(p.allows(Limit::Agent("claude-acp")).is_ok());
        let why = p.allows(Limit::Agent("codex-acp")).unwrap_err();
        assert_eq!(why, "Corp's policy does not allow the agent codex-acp.");
        assert!(p.allows(Limit::Tunnel).is_err());
        assert!(p.allows(Limit::Remote).is_ok() && p.allows(Limit::Webhooks).is_ok());
        let none = policy(r#"{"organization": "Corp"}"#);
        assert!(none.allows(Limit::Agent("any")).is_ok() && none.allows(Limit::Tunnel).is_ok());
        let unknown =
            serde_json::from_str::<Policy>(r#"{"organization": "Corp", "limits": {"x": false}}"#);
        assert!(unknown.is_err(), "a limit Ghost cannot enforce is refused");
    }

    #[test]
    fn no_file_is_no_policy_and_a_bad_one_says_why() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("policy.json")).unwrap().is_none());
        std::fs::write(dir.path().join("policy.json"), r#"{"org": 1}"#).unwrap();
        assert!(read(&dir.path().join("policy.json")).is_err());
    }
}
