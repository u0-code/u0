//! An organisation's policy for this machine, as t3code's managed policy,
//! read once as u0 starts. It names the organisation, and sets
//! settings: a default where you have chosen nothing, or, locked, a value
//! you cannot change.
//!
//! An administrator sets it where only an administrator can: on a Mac,
//! with a configuration profile (`profile`; `policy/u0.mobileconfig` is
//! the example), elsewhere in `/etc/u0/policy.json`. Both have one
//! shape:
//!
//! ```json
//! {
//!   "organization": "Example Corp",
//!   "support_url": "https://it.example.com/u0",
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
//! Each key of `settings` is a top-level key of `settings.json`, or a field
//! of one of its nested objects (`confirm.delete`, `confirm.quit`,
//! `writer.suggest`, `voice.clean`). A policy
//! that does not read, or names a key there is not, or a value of the
//! wrong kind, keeps the settings from loading at all: nothing runs half
//! governed. `limits` are what no setting undoes, each enforced by the
//! server where the thing is done (`allows`); one left out limits nothing.

#[cfg(target_os = "macos")]
mod profile;

use std::{collections::BTreeMap, sync::OnceLock};

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

/// What the policy keeps u0 from doing, as t3code's `limits`.
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

/// The policy of this machine, read once: none where there is none; why
/// it does not read, where it does not.
pub fn current() -> &'static Result<Option<Policy>, String> {
    static POLICY: OnceLock<Result<Option<Policy>, String>> = OnceLock::new();
    POLICY.get_or_init(read)
}

#[cfg(target_os = "macos")]
use profile::read;

/// The policy in `/etc/u0/policy.json`.
#[cfg(not(target_os = "macos"))]
fn read() -> Result<Option<Policy>, String> {
    read_file(std::path::Path::new("/etc/u0/policy.json"))
}

/// The organisation that locks the setting `key`, if one does.
pub fn locked(key: &str) -> Option<&'static str> {
    let policy = current().as_ref().ok()?.as_ref()?;
    policy.locks(key).then_some(policy.organization.as_str())
}

#[cfg(not(target_os = "macos"))]
fn read_file(path: &std::path::Path) -> Result<Option<Policy>, String> {
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

    fn locks(&self, key: &str) -> bool {
        if self.settings.get(key).is_some_and(|m| m.locked) {
            return true;
        }
        key.split_once('.')
            .and_then(|(outer, _)| self.settings.get(outer))
            .is_some_and(|m| m.locked)
    }

    /// `settings` (settings.json as read) as the policy has them: its
    /// defaults where they hold nothing, its locked values whatever they
    /// hold. A key settings.json has no place for is refused.
    pub fn apply(
        &self,
        settings: &mut serde_json::Map<String, serde_json::Value>,
        known: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        let mut touched_nested = std::collections::BTreeSet::new();
        for (key, managed) in &self.settings {
            if let Some((outer, inner)) = key.split_once('.') {
                let Some(serde_json::Value::Object(known_inner)) = known.get(outer) else {
                    return Err(format!(
                        "{}'s policy sets \"{key}\", which is not a setting",
                        self.organization
                    ));
                };
                if !known_inner.contains_key(inner) {
                    return Err(format!(
                        "{}'s policy sets \"{key}\", which is not a setting",
                        self.organization
                    ));
                }
                let entry = settings
                    .entry(outer.to_owned())
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                let Some(map) = entry.as_object_mut() else {
                    return Err(format!(
                        "{}'s policy sets \"{key}\", while \"{outer}\" is not an object",
                        self.organization
                    ));
                };
                if managed.locked || !map.contains_key(inner) {
                    map.insert(inner.to_owned(), managed.value.clone());
                }
                touched_nested.insert(outer.to_owned());
                continue;
            }
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
        for outer in touched_nested {
            if let (
                Some(serde_json::Value::Object(known_inner)),
                Some(serde_json::Value::Object(map)),
            ) = (known.get(&outer), settings.get_mut(&outer))
            {
                for (k, v) in known_inner {
                    map.entry(k.clone()).or_insert_with(|| v.clone());
                }
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
        let nested_bad =
            policy(r#"{"organization": "Corp", "settings": {"confirm.nope": {"value": true}}}"#);
        let known = map(r#"{"confirm": {"delete": true, "quit": "hold"}}"#);
        assert!(nested_bad.apply(&mut map("{}"), &known).is_err());
    }

    #[test]
    fn a_nested_setting_can_be_locked_on_its_own_without_locking_its_siblings() {
        let p = policy(
            r#"{"organization": "Corp", "settings": {
                "confirm.delete": {"value": true, "locked": true},
                "confirm.quit": {"value": "hold"}}}"#,
        );
        let known = map(r#"{"confirm": {"unpin": false, "delete": true, "quit": "direct"}}"#);
        let mut mine = map(r#"{"confirm": {"unpin": true, "delete": false, "quit": "double"}}"#);
        p.apply(&mut mine, &known).unwrap();
        assert_eq!(mine["confirm"]["delete"], true);
        assert_eq!(mine["confirm"]["unpin"], true);
        assert_eq!(mine["confirm"]["quit"], "double");
        let mut empty = map("{}");
        p.apply(&mut empty, &known).unwrap();
        assert_eq!(empty["confirm"]["quit"], "hold");
        assert!(p.locks("confirm.delete") && !p.locks("confirm.unpin") && !p.locks("confirm.quit"));
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
        assert!(unknown.is_err(), "a limit u0 cannot enforce is refused");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn no_file_is_no_policy_and_a_bad_one_says_why() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            read_file(&dir.path().join("policy.json"))
                .unwrap()
                .is_none()
        );
        std::fs::write(dir.path().join("policy.json"), r#"{"org": 1}"#).unwrap();
        assert!(read_file(&dir.path().join("policy.json")).is_err());
    }
}
