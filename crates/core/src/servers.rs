//! The Ghosts on other machines this Mac has paired with, in
//! `servers.json`: each by its name, at the address it was last reached
//! at, with the public half of the key this Mac made for it (`key`);
//! whether this Mac controls others at all, and the one the window opens
//! on, if not this Mac's own. The private half is kept secret (`secrets`),
//! never in a file.
//!
//! A desk pairs as you ask it to; a server pairs back with a desk that
//! asked it to as it paired (`link`). Both write here, each through
//! [`Servers::change`].

use std::{path::Path, sync::Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    key::{self, Key},
    link, whole,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Servers {
    pub servers: Vec<Server>,
    /// Whether this Mac controls others: until it does, the window opens
    /// on this Mac's own Ghost only.
    #[serde(default)]
    pub control: bool,
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

/// Where the private half of the key whose public half is `public` is
/// kept: one key, one place, whatever its server is called.
fn account(public: &str) -> String {
    format!("server/{}", key::fingerprint(public))
}

impl Servers {
    /// The servers paired with, as `home` keeps them, or why they do not
    /// read.
    pub fn load(home: &Path) -> Result<Self, String> {
        whole::read_json(&home.join("servers.json"))
            .map_err(|e| format!("{e}. Move servers.json away and pair with each server again."))
    }

    /// Change what `home` keeps by `change`, and write it: read afresh
    /// and written whole, one change at a time.
    pub fn change(
        home: &Path,
        change: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
        let _one = ONE_AT_A_TIME.lock().expect("servers");
        let mut servers = Self::load(home)?;
        change(&mut servers)?;
        let text = serde_json::to_vec_pretty(&servers).expect("servers serialize");
        whole::write(&home.join("servers.json"), &text).map_err(|e| e.to_string())
    }

    /// The server the window opens on, if not this Mac's own.
    pub fn opened(&self) -> Option<&Server> {
        let name = self.open.as_ref().filter(|_| self.control)?;
        self.servers.iter().find(|s| &s.name == name)
    }

    /// Keep `server`, paired with by `key`: in place of one of its name.
    fn paired(&mut self, server: Server, key: &Key) -> Result<(), String> {
        crate::secrets::set(&account(&server.key), Some(key.kept()))?;
        self.forget(&server.name);
        self.servers.push(server);
        self.control = true;
        Ok(())
    }

    /// Forget the server `name`, and this Mac's key for it.
    pub fn forget(&mut self, name: &str) {
        for s in self.servers.iter().filter(|s| s.name == name) {
            let _ = crate::secrets::set(&account(&s.key), None);
        }
        self.servers.retain(|s| s.name != name);
        if self.open.as_deref() == Some(name) {
            self.open = None;
        }
    }
}

impl Server {
    /// How to reach it, and the key to come in with.
    pub fn reach(&self) -> Result<(String, Key), String> {
        let kept = crate::secrets::get(&account(&self.key))?
            .ok_or("This Mac's key for it is not in the Keychain: pair again")?;
        Ok((link::url_of(&self.host), Key::read(&kept)?))
    }
}

/// Pair with the server at `host` by its one-time `code`, coming in with
/// `key`, and keep it in `home`; `back`, a pairing link of this Mac's for
/// it to pair back with. Its name, and how pairing back went.
pub async fn pair(
    home: &Path,
    host: String,
    code: String,
    key: Key,
    back: Option<String>,
) -> Result<(String, Option<Result<String, String>>), String> {
    let welcome = link::pair(&host, code, &key, back).await?;
    let server = Server {
        name: welcome.name.clone(),
        host,
        key: key.public(),
    };
    Servers::change(home, |s| s.paired(server, &key))?;
    Ok((welcome.name, welcome.back))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opens_on_a_server_paired_with_only_while_this_mac_controls_others() {
        let mut servers: Servers = serde_json::from_str(
            r#"{"servers": [{"name": "box", "host": "a.trycloudflare.com", "key": "ssh-ed25519 AAAA"}], "control": true, "open": "box"}"#,
        )
        .unwrap();
        assert_eq!(
            servers.opened().map(|s| s.host.as_str()),
            Some("a.trycloudflare.com")
        );
        servers.control = false;
        assert_eq!(servers.opened(), None);
        servers.control = true;
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
        let refused = Servers::change(home.path(), |_| Ok(()));
        assert!(refused.is_err());
        let left = std::fs::read_to_string(home.path().join("servers.json")).unwrap();
        assert_eq!(left, old);
    }

    #[test]
    fn a_key_kept_reads_back_and_is_gone_once_its_server_is_forgotten() {
        let home = tempfile::tempdir().unwrap();
        let key = Key::generate();
        let server = Server {
            name: "box".into(),
            host: "127.0.0.1:1".into(),
            key: key.public(),
        };
        let kept = server.clone();
        Servers::change(home.path(), |s| s.paired(kept, &key)).unwrap();
        let (url, again) = server.reach().unwrap();
        assert_eq!(
            (url.as_str(), again.public()),
            ("ws://127.0.0.1:1", key.public())
        );
        let servers = Servers::load(home.path()).unwrap();
        assert!(servers.control, "pairing is controlling another");
        Servers::change(home.path(), |s| {
            s.forget("box");
            Ok(())
        })
        .unwrap();
        assert!(Servers::load(home.path()).unwrap().servers.is_empty());
        assert!(matches!(server.reach(), Err(why) if why.contains("pair again")));
    }
}
