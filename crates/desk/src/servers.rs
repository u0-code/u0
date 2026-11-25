//! The Ghosts on other machines this desk has paired with, in
//! `servers.json`: each by its name, at the address it was last reached
//! at; and the one the window opens on, if not this Mac's own. The token
//! each gave is kept secret (`secrets`), never in a file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{link, proto, whole};

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
}

/// Where the token the server `name` gave is kept.
fn account(name: &str) -> String {
    format!("server/{name}")
}

impl Servers {
    /// The servers paired with, as `home` keeps them, or why they do not
    /// read.
    pub fn load(home: &Path) -> Result<Self, String> {
        whole::read_json(&home.join("servers.json"))
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

    /// Keep `server`, paired with and given `token`: in place of one of
    /// its name.
    pub fn paired(&mut self, server: Server, token: &str) -> Result<(), String> {
        crate::secrets::set(&account(&server.name), Some(token))?;
        self.servers.retain(|s| s.name != server.name);
        self.servers.push(server);
        Ok(())
    }

    /// Forget the server `name`, and its token.
    pub fn forget(&mut self, name: &str) {
        let _ = crate::secrets::set(&account(name), None);
        self.servers.retain(|s| s.name != name);
        if self.open.as_deref() == Some(name) {
            self.open = None;
        }
    }
}

impl Server {
    /// How to reach it, and come in.
    pub fn reach(&self) -> Result<(String, proto::Auth), String> {
        let token = crate::secrets::get(&account(&self.name))?
            .ok_or("Its token is not in the Keychain: pair again")?;
        Ok((link::url_of(&self.host), proto::Auth::Token(token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opens_on_a_server_paired_with_and_a_file_in_another_form_is_refused() {
        let mut servers: Servers = serde_json::from_str(
            r#"{"servers": [{"name": "box", "host": "a.trycloudflare.com"}], "open": "box"}"#,
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
}
