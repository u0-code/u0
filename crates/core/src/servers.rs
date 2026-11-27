//! The u0s on other machines this Mac has paired with, in
//! `servers.json`: each by its name, at the address it was last reached
//! at, with the public half of the key this Mac made for it (`key`); and
//! whether this Mac controls others at all: while it does, the window
//! holds each of them beside this Mac's own. The private half is kept
//! secret (`secrets`), never in a file.
//!
//! A desk pairs as you ask it to; a server pairs back with a desk that
//! asked it to as it paired (`link`). Both write here, each through
//! [`Servers::change`].

use std::{path::Path, sync::Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    key::{self, Key},
    link,
    reach::Way,
    whole,
};

/// Which of the servers a window holds something is on: a number the
/// window gives each as it holds it, never given again while it runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Host(pub u16);

impl Host {
    /// This Mac's own.
    pub const HERE: Self = Self(0);
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Servers {
    pub servers: Vec<Server>,
    /// Whether this Mac controls others: until it does, the window holds
    /// this Mac's own u0 only.
    #[serde(default)]
    pub control: bool,
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
    /// The SHA-256 of its certificate, where it is reached over a network
    /// and speaks TLS itself (`reach`): that one alone is trusted.
    #[serde(default)]
    pub pin: Option<String>,
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

    /// The servers the window holds beside this Mac's own: none until this
    /// Mac controls others. This Mac's service (`cli`), at `127.0.0.1` on
    /// this home's own `port`, is this Mac's own, not another's.
    pub fn held(&self, port: u16) -> impl Iterator<Item = &Server> {
        let service = format!("127.0.0.1:{port}");
        (self.servers.iter()).filter(move |s| self.control && s.host != service)
    }

    /// This Mac's service, where it runs one: the window reaches this
    /// Mac's own u0 there rather than running a second on its home.
    pub fn service(&self, port: u16) -> Option<&Server> {
        let service = format!("127.0.0.1:{port}");
        self.servers.iter().find(|s| s.host == service)
    }

    /// Keep `server`, paired with by `key`: in place of one of its name at
    /// its address (paired with again); one of its name elsewhere is kept,
    /// and this refused.
    pub(crate) fn paired(&mut self, server: Server, key: &Key) -> Result<(), String> {
        if (self.servers.iter()).any(|s| s.name == server.name && s.host != server.host) {
            return Err(format!(
                "This Mac is paired with another server called {} already: forget it first",
                server.name
            ));
        }
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
    }
}

impl Server {
    /// How to reach it, and the key to come in with.
    pub fn reach(&self) -> Result<(Way, Key), String> {
        let kept = crate::secrets::get(&account(&self.key))?
            .ok_or("This Mac's key for it is not in the Keychain: pair again")?;
        let way = Way {
            host: self.host.clone(),
            pin: self.pin.clone(),
        };
        Ok((way, Key::read(&kept)?))
    }
}

/// Pair with the server `way` by its one-time `code`, coming in with
/// `key`, and keep it in `home`; `back`, a pairing link of this Mac's for
/// it to pair back with. What came of it.
pub async fn pair(
    home: &Path,
    way: Way,
    code: String,
    key: Key,
    back: Option<String>,
) -> Result<link::Paired, String> {
    let paired = link::pair(&way, code, &key, back).await?;
    let server = Server {
        name: paired.name.clone(),
        host: way.host,
        key: key.public(),
        pin: way.pin,
    };
    Servers::change(home, |s| s.paired(server, &key))?;
    Ok(paired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_holds_the_servers_paired_with_only_while_this_mac_controls_others() {
        let mut servers: Servers = serde_json::from_str(
            r#"{"servers": [{"name": "box", "host": "a.trycloudflare.com", "key": "ssh-ed25519 AAAA"}, {"name": "This Mac's service", "host": "127.0.0.1:4317", "key": "ssh-ed25519 BBBB"}], "control": true}"#,
        )
        .unwrap();
        let held = |s: &Servers| s.held(4317).map(|s| s.name.clone()).collect::<Vec<_>>();
        assert_eq!(held(&servers), ["box"], "the service is this Mac's own");
        assert_eq!(
            servers.service(4317).map(|s| s.key.as_str()),
            Some("ssh-ed25519 BBBB")
        );
        assert_eq!(servers.service(4318), None);
        servers.control = false;
        assert!(held(&servers).is_empty());
        assert!(servers.service(4317).is_some(), "the service is this Mac's");
        assert!(serde_json::from_str::<Servers>(r#"{"hosts": []}"#).is_err());
    }

    #[test]
    fn a_server_of_a_name_paired_with_already_elsewhere_is_refused_not_replaced() {
        let mine = Server {
            name: "studio".into(),
            host: "studio.example:4317".into(),
            key: "ssh-ed25519 MINE".into(),
            pin: None,
        };
        let mut servers = Servers {
            servers: vec![mine.clone()],
            ..Servers::default()
        };
        let planted = Server {
            host: "evil.example".into(),
            key: "ssh-ed25519 EVIL".into(),
            ..mine.clone()
        };
        let why = servers.paired(planted, &Key::generate()).unwrap_err();
        assert!(why.contains("forget it first"), "{why}");
        assert_eq!(servers.servers, [mine]);
        assert!(!servers.control);
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
            pin: None,
        };
        let kept = server.clone();
        Servers::change(home.path(), |s| s.paired(kept, &key)).unwrap();
        let (way, again) = server.reach().unwrap();
        assert_eq!(
            (way.url().as_str(), again.public()),
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
