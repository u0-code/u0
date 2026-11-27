//! What is kept secret: an agent's variable marked secret, a token for a
//! forge. Each is a password in the macOS Keychain, never in a file of
//! u0's, under a service named for u0's home, so that a home made for
//! a test keeps its own. A value, kept there or written in the settings,
//! may instead be a 1Password reference (`op://vault/item/field`), which the
//! `op` CLI reads each time it is used. A test keeps its secrets in
//! memory (`memory`), so that no test touches your Keychain.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Where a named secret reference reads its value from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    #[default]
    Keychain,
    OnePassword,
    Plain,
}

/// Metadata for a project-scoped secret reference (`secrets.json`).
///
/// Typed values live only in the macOS Keychain under `ref/<name>`, never in
/// `secrets.json`; `reference` holds only an `op://vault/item/field` pointer
/// when `source` is [`SecretSource::OnePassword`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub name: String,
    #[serde(default)]
    pub source: SecretSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Allowed project folders; empty means all projects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretRefs {
    #[serde(default)]
    refs: Vec<SecretRef>,
}

/// Status of a secret backend on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretProvider {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub connected: bool,
}

/// Keychain account name for a named secret reference `name`.
pub fn ref_account(name: &str) -> String {
    format!("ref/{name}")
}

fn refs_path(home: &Path) -> PathBuf {
    home.join("secrets.json")
}

/// Named secret references in `home`.
pub fn load_refs(home: &Path) -> Result<Vec<SecretRef>, String> {
    crate::whole::read_json::<SecretRefs>(&refs_path(home))
        .map(|r| r.refs)
        .map_err(|e| format!("{e}. Move secrets.json away to start afresh."))
}

/// Save or update `entry` in `home`, storing `value` in the Keychain when
/// `entry.source` is [`SecretSource::Keychain`].
pub fn set_ref(home: &Path, mut entry: SecretRef, value: Option<&str>) -> Result<(), String> {
    let name = entry.name.trim().to_owned();
    if name.is_empty() {
        return Err("Give the secret reference a name".into());
    }
    entry.name.clone_from(&name);
    match entry.source {
        SecretSource::Keychain | SecretSource::Plain => {
            entry.reference = None;
            if let Some(v) = value {
                set(&ref_account(&name), Some(v))?;
            }
        }
        SecretSource::OnePassword => {
            let op_ref = entry
                .reference
                .as_deref()
                .or(value)
                .unwrap_or_default()
                .trim()
                .to_owned();
            if !op_ref.starts_with("op://") {
                return Err("A 1Password reference begins with op://".into());
            }
            entry.reference = Some(op_ref);
            let _ = set(&ref_account(&name), None);
        }
    }
    let mut all = crate::whole::read_json::<SecretRefs>(&refs_path(home))?;
    match all.refs.iter_mut().find(|r| r.name == name) {
        Some(existing) => *existing = entry,
        None => all.refs.push(entry),
    }
    all.refs.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = serde_json::to_vec_pretty(&all).expect("secret refs serialize");
    crate::whole::write(&refs_path(home), &bytes).map_err(|e| e.to_string())
}

/// Remove the secret reference `name` from `home` and delete any Keychain item for it.
pub fn drop_ref(home: &Path, name: &str) -> Result<(), String> {
    let _ = set(&ref_account(name), None);
    let mut all = crate::whole::read_json::<SecretRefs>(&refs_path(home))?;
    all.refs.retain(|r| r.name != name);
    let bytes = serde_json::to_vec_pretty(&all).expect("secret refs serialize");
    crate::whole::write(&refs_path(home), &bytes).map_err(|e| e.to_string())
}

/// Inspect secret providers available on this machine without reading secret values.
pub fn providers() -> Vec<SecretProvider> {
    let op_cli = which::which("op").ok().and_then(|path| {
        std::process::Command::new(path)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    });
    let kept_accounts = kept();
    let sa_connected = std::env::var_os("OP_SERVICE_ACCOUNT_TOKEN").is_some()
        || kept_accounts.iter().any(|a| a == "op/service_account");
    vec![
        SecretProvider {
            id: "1password-cli".into(),
            name: "1Password CLI".into(),
            detail: match &op_cli {
                Some(v) => format!("op {v}"),
                None => "op CLI is not installed".into(),
            },
            connected: op_cli.is_some(),
        },
        SecretProvider {
            id: "1password-sa".into(),
            name: "1Password service account".into(),
            detail: if sa_connected {
                "Service account token configured".into()
            } else {
                "No service account token set".into()
            },
            connected: sa_connected,
        },
        SecretProvider {
            id: "keychain".into(),
            name: "macOS Keychain".into(),
            detail: "Built in · This Mac only".into(),
            connected: cfg!(target_os = "macos") || cfg!(any(test, feature = "test")),
        },
    ]
}

/// The Keychain's service for the home u0 runs in. A test has no home
/// (`settings::home` refuses it one) and keeps its secrets in memory, under
/// this one name.
fn service() -> String {
    if cfg!(any(test, feature = "test")) {
        return "u0 test".into();
    }
    format!("u0 {}", crate::settings::home().display())
}

/// Where an agent's variable `name` is kept.
pub fn env_account(agent: &str, name: &str) -> String {
    format!("env/{agent}/{name}")
}

/// The secret kept as `account`; none where nothing is.
pub fn get(account: &str) -> Result<Option<String>, String> {
    store::get(&service(), account)
}

/// The accounts something is kept as.
pub fn kept() -> Vec<String> {
    store::accounts(&service())
}

/// Keep `value` as `account`, or, with none, keep nothing there.
pub fn set(account: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(v) => store::set(&service(), account, v),
        None => store::remove(&service(), account),
    }
}

/// `value` as it is used: a 1Password reference read, anything else as it
/// is.
pub fn resolve(value: String) -> Result<String, String> {
    if !value.starts_with("op://") {
        return Ok(value);
    }
    let op = which::which("op").map_err(|_| "1Password's op CLI is not installed")?;
    crate::git::yours(
        std::path::Path::new("/"),
        &op.to_string_lossy(),
        &["read", &value],
    )
}

/// The variables an agent starts with, by name: those written in the
/// settings as they are (`Some`), those kept secret (`None`) read from the
/// Keychain.
pub fn env(
    agent: &str,
    vars: &BTreeMap<String, Option<String>>,
) -> Result<Vec<(String, String)>, String> {
    vars.iter()
        .map(|(name, value)| {
            let value = match value {
                Some(v) => v.clone(),
                None => get(&env_account(agent, name))?
                    .ok_or_else(|| format!("{name} is not in the Keychain: set it again"))?,
            };
            Ok((name.clone(), resolve(value)?))
        })
        .collect()
}

// A test, this crate's or the desk's (the feature `test`), never reaches
// the Keychain: macOS would ask you to let a test binary read u0's.
#[cfg(not(any(test, feature = "test")))]
use keychain as store;
#[cfg(any(test, feature = "test"))]
use memory as store;

/// What a test keeps in place of the Keychain.
#[cfg(any(test, feature = "test"))]
mod memory {
    use std::{collections::BTreeMap, sync::Mutex};

    type Kept = BTreeMap<(String, String), String>;
    static KEPT: Mutex<Kept> = Mutex::new(BTreeMap::new());

    fn kept() -> std::sync::MutexGuard<'static, Kept> {
        KEPT.lock().expect("secrets")
    }

    #[expect(clippy::unnecessary_wraps, reason = "as the Keychain's answers")]
    pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
        Ok(kept().get(&(service.into(), account.into())).cloned())
    }

    #[expect(clippy::unnecessary_wraps, reason = "as the Keychain's answers")]
    pub fn set(service: &str, account: &str, value: &str) -> Result<(), String> {
        kept().insert((service.into(), account.into()), value.into());
        Ok(())
    }

    #[expect(clippy::unnecessary_wraps, reason = "as the Keychain's answers")]
    pub fn remove(service: &str, account: &str) -> Result<(), String> {
        kept().remove(&(service.into(), account.into()));
        Ok(())
    }

    pub fn accounts(service: &str) -> Vec<String> {
        (kept().keys())
            .filter(|(s, _)| s == service)
            .map(|(_, a)| a.clone())
            .collect()
    }
}

#[cfg(all(target_os = "macos", any(test, not(feature = "test"))))]
mod keychain {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework::passwords;

    /// errSecItemNotFound: nothing is kept there.
    const NOT_FOUND: i32 = -25300;

    pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
        match passwords::get_generic_password(service, account) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(service: &str, account: &str, value: &str) -> Result<(), String> {
        passwords::set_generic_password(service, account, value.as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn remove(service: &str, account: &str) -> Result<(), String> {
        match passwords::delete_generic_password(service, account) {
            Err(e) if e.code() != NOT_FOUND => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// The accounts under `service`, read without their secrets.
    pub fn accounts(service: &str) -> Vec<String> {
        let found = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
            .load_attributes(true)
            .limit(Limit::All)
            .search()
            .unwrap_or_default();
        found
            .iter()
            .filter_map(|r| r.simplify_dict()?.remove("acct"))
            .collect()
    }
}

#[cfg(all(not(target_os = "macos"), any(test, not(feature = "test"))))]
mod keychain {
    const NONE: &str = "secrets are kept in the macOS Keychain, which this system does not have";

    pub fn get(_: &str, _: &str) -> Result<Option<String>, String> {
        Err(NONE.into())
    }

    pub fn set(_: &str, _: &str, _: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn remove(_: &str, _: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn accounts(_: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_variable_is_used_as_written_and_a_secret_one_must_be_kept() {
        let mut vars = BTreeMap::new();
        vars.insert("PLAIN".to_owned(), Some("yes".to_owned()));
        assert_eq!(
            env("a", &vars).unwrap(),
            [("PLAIN".to_owned(), "yes".to_owned())]
        );
        assert_eq!(env_account("claude-acp", "KEY"), "env/claude-acp/KEY");
        assert_eq!(resolve("op".into()).unwrap(), "op");
    }

    #[test]
    fn a_secret_reference_keeps_only_metadata_on_disk_and_values_in_the_store() {
        let home = tempfile::tempdir().unwrap();
        let entry = SecretRef {
            name: "STRIPE_TEST_KEY".into(),
            source: SecretSource::Keychain,
            reference: None,
            projects: vec!["/p/api-server".into()],
        };
        set_ref(home.path(), entry.clone(), Some("sk_test_12345")).unwrap();
        let on_disk = std::fs::read_to_string(home.path().join("secrets.json")).unwrap();
        assert!(
            !on_disk.contains("sk_test_12345"),
            "secret leaked to disk: {on_disk}"
        );
        assert_eq!(load_refs(home.path()).unwrap(), [entry]);
        assert_eq!(
            get(&ref_account("STRIPE_TEST_KEY")).unwrap().as_deref(),
            Some("sk_test_12345")
        );
        drop_ref(home.path(), "STRIPE_TEST_KEY").unwrap();
        assert!(load_refs(home.path()).unwrap().is_empty());
        assert_eq!(get(&ref_account("STRIPE_TEST_KEY")).unwrap(), None);
    }

    /// Run by hand (`cargo test -p u0-core -- --ignored keychain`): it
    /// writes, reads and removes one item in the login Keychain, under a
    /// service of its own.
    #[test]
    #[ignore = "touches the login Keychain"]
    fn a_secret_kept_reads_back_and_is_gone_once_removed() {
        let service = format!("u0 test {}", std::process::id());
        keychain::set(&service, "env/a/KEY", "s3cret").unwrap();
        assert_eq!(
            keychain::get(&service, "env/a/KEY").unwrap().as_deref(),
            Some("s3cret")
        );
        assert_eq!(keychain::accounts(&service), ["env/a/KEY"]);

        keychain::remove(&service, "env/a/KEY").unwrap();
        assert_eq!(keychain::get(&service, "env/a/KEY").unwrap(), None);
    }
}
