//! The Ghosts on other machines this desk has paired with, in
//! `servers.json`: each by its name, at the address it was last reached
//! at, with the public half of the key this Mac made for it (`key`); and
//! the one the window opens on, if not this Mac's own. The private half is
//! kept secret (`secrets`), never in a file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{key::Key, link, whole};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Servers {
    pub servers: Vec<Server>,
    /// The server the window opens on, by name; none, this Mac's own.
    #[serde(default)]
    pub open: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    /// What it calls itself: its machine's name.
    pub name: String,
    /// Where it was last reached: `127.0.0.1:4317` through an `ssh -L`, a
    /// tunnel's host name.
    pub host: String,
    /// The public half of this Mac's key for it, `ssh-ed25519 AAAA…`.
    pub key: String,
}

/// Where the private half of this Mac's key for the server `name` is kept.
fn account(name: &str) -> String {
    format!("server/{name}")
}

impl Servers {
    /// The servers paired with, as `home` keeps them, or why they do not
    /// read.
    pub fn load(home: &Path) -> Result<Self, String> {
        whole::read_json(&home.join("servers.json"))
            .map_err(|e| format!("{e}. Move servers.json away and pair with each server again."))
    }

    pub fn save(&self, home: &Path) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(self).expect("servers serialize");
        whole::write(&home.join("servers.json"), &text).map_err(|e| e.to_string())
    }

    /// The server the window opens on, if not this Mac's own.
    pub fn opened(&self) -> Option<&Server> {
        let name = self.open.as_ref()?;
        self.servers.iter().find(|s| &s.name == name)
    }

    /// Keep `server`, paired with by the key kept as `kept`: in place of
    /// one of its name.
    pub fn paired(&mut self, server: Server, kept: &str) -> Result<(), String> {
        crate::secrets::set(&account(&server.name), Some(kept))?;
        self.servers.retain(|s| s.name != server.name);
        self.servers.push(server);
        Ok(())
    }

    /// Forget the server `name`, and this Mac's key for it.
    pub fn forget(&mut self, name: &str) {
        let _ = crate::secrets::set(&account(name), None);
        self.servers.retain(|s| s.name != name);
        if self.open.as_deref() == Some(name) {
            self.open = None;
        }
    }
}

impl Server {
    /// How to reach it, and the key to come in with.
    pub fn reach(&self) -> Result<(String, Key), String> {
        let kept = crate::secrets::get(&account(&self.name))?
            .ok_or("This Mac's key for it is not in the Keychain: pair again")?;
        Ok((link::url_of(&self.host), Key::read(&kept)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opens_on_a_server_paired_with_and_a_file_in_another_form_is_refused() {
        let mut servers: Servers = serde_json::from_str(
            r#"{"servers": [{"name": "box", "host": "a.trycloudflare.com", "key": "ssh-ed25519 AAAA"}], "open": "box"}"#,
        )
        .unwrap();
        assert_eq!(
            servers.opened().map(|s| s.host.as_str()),
            Some("a.trycloudflare.com")
        );
        servers.open = Some("gone".into());
        assert_eq!(servers.opened(), None);
        assert!(serde_json::from_str::<Servers>(r#"{"hosts": []}"#).is_err());
    }

    #[test]
    fn a_servers_file_from_before_keys_is_refused_with_what_to_do() {
        let home = tempfile::tempdir().unwrap();
        let old = r#"{"servers": [{"name": "box", "host": "a.trycloudflare.com"}]}"#;
        std::fs::write(home.path().join("servers.json"), old).unwrap();
        let why = Servers::load(home.path()).unwrap_err();
        assert!(why.contains("pair with each server again"), "{why}");
    }

    /// Run by hand (`cargo test -p ghost-desk -- --ignored keychain`): a
    /// key kept in the login Keychain, under a home (and so a service) of
    /// its own, reads back as the same key, and is gone once forgotten.
    #[test]
    #[ignore = "touches the login Keychain"]
    fn a_key_kept_in_the_keychain_reads_back_and_is_gone_once_forgotten() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: this test sets it before it reads the home, and no other
        // test reads it while it runs alone, as it is run.
        unsafe { std::env::set_var("GHOST_HOME", home.path()) };
        let key = Key::generate();
        let server = Server {
            name: format!("test-{}", std::process::id()),
            host: "127.0.0.1:1".into(),
            key: key.public(),
        };
        let mut servers = Servers::default();
        servers.paired(server.clone(), key.kept()).unwrap();
        let (url, again) = server.reach().unwrap();
        assert_eq!(
            (url.as_str(), again.public()),
            ("ws://127.0.0.1:1", key.public())
        );
        servers.forget(&server.name);
        assert!(matches!(server.reach(), Err(why) if why.contains("pair again")));
    }
}
