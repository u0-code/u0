//! What the desk keeps between runs, in its home: which agents are switched
//! on, in the order they are listed, and the folders work was started in.
//!
//! The home is `GHOST_HOME`, or `~/.ghost`. It holds this file
//! (`settings.json`), the last registry read (`registry.json`) and the
//! agents installed from archives (`agents/`). Each agent keeps its own
//! sign-in and its own sessions where its CLI always does; the desk keeps
//! none of them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::whole;

/// The agents switched on when the desk is first opened.
pub const FIRST: [&str; 3] = ["claude-acp", "codex-acp", "antigravity-acp"];

/// How many folders are remembered for starting a session in.
const FOLDERS: usize = 8;

/// The desk's home.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("GHOST_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".ghost")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The agents shown down the left, by registry id, in order.
    pub enabled: Vec<String>,
    /// Folders sessions were started in, the latest first.
    pub folders: Vec<PathBuf>,
    /// Whether a new session in a git repository works in a worktree of
    /// its own: the choice last made.
    #[serde(default)]
    pub worktree: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: FIRST.iter().map(|s| (*s).to_owned()).collect(),
            folders: Vec::new(),
            worktree: false,
        }
    }
}

impl Settings {
    /// The settings in `home`; the defaults when there are none. A file
    /// that cannot be read is not overwritten: the reason comes back with
    /// the defaults, and nothing is saved until it is moved away.
    pub fn load(home: &Path) -> (Self, Option<String>) {
        let path = home.join("settings.json");
        match std::fs::read_to_string(&path) {
            Err(_) => (Self::default(), None),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => (settings, None),
                Err(e) => (
                    Self::default(),
                    Some(format!(
                        "{} is not in a form this desk reads ({e}). Move it away to start afresh; until then nothing is saved.",
                        path.display()
                    )),
                ),
            },
        }
    }

    pub fn save(&self, home: &Path) -> std::io::Result<()> {
        let text = serde_json::to_vec_pretty(self).expect("settings serialize");
        whole::write(&home.join("settings.json"), &text)
    }

    /// Switch the agent `id` on or off.
    pub fn toggle(&mut self, id: &str) {
        if let Some(at) = self.enabled.iter().position(|e| e == id) {
            self.enabled.remove(at);
        } else {
            self.enabled.push(id.to_owned());
        }
    }

    /// Put `folder` first among those remembered.
    pub fn used(&mut self, folder: &Path) {
        self.folders.retain(|f| f != folder);
        self.folders.insert(0, folder.to_path_buf());
        self.folders.truncate(FOLDERS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_used_again_moves_first_and_the_list_stays_short() {
        let mut s = Settings::default();
        for i in 0..10 {
            s.used(Path::new(&format!("/p{i}")));
        }
        s.used(Path::new("/p3"));
        assert_eq!(s.folders.len(), FOLDERS);
        assert_eq!(s.folders[0], Path::new("/p3"));
    }

    #[test]
    fn a_settings_file_in_another_form_is_refused_not_replaced() {
        let dir = std::env::temp_dir().join(format!("ghost-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"who":"me"}"#).unwrap();
        let (s, why) = Settings::load(&dir);
        assert_eq!(s, Settings::default());
        assert!(why.unwrap().contains("Move it away"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
