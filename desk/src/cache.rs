//! What the desk showed of a daemon last time, kept on this machine so
//! that the first screen is drawn the moment the window opens, before the
//! daemon has answered: its journal's tail and its work. The daemon's own
//! answer replaces it within a few seconds of connecting, and anything
//! written since is read then; nothing here is the record, only a copy of
//! what the desk had seen of it.
//!
//! One file per daemon and character, under `~/.config/ghost-desk/cache/`,
//! written at most once a second while new entries arrive. The images her
//! core hands out are kept beside them, under `media/`: see `media.rs`.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use ghost_link::protocol::Work;
use serde::{Deserialize, Serialize};

use crate::model::Entry;

/// What is kept of a daemon between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cached {
    #[serde(default)]
    pub journal: Vec<Entry>,
    #[serde(default)]
    pub work: Vec<Work>,
    /// Her clock's zone, so that times read right before her daemon has
    /// said it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Her core's id, so that the images kept from it show before her
    /// daemon has said it again, or while it cannot be reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<String>,
}

/// How long new entries may wait before the file is written again.
const SETTLE: Duration = Duration::from_secs(1);

/// Where the desk keeps its copies: `~/.config/ghost-desk/cache/`.
pub fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("ghost-desk")
            .join("cache"),
    )
}

/// The file one daemon's copy is kept in: by its address and, when it
/// runs several, the character.
fn path(url: &str, ghost: Option<&str>) -> Option<PathBuf> {
    let key: String = format!("{url}#{}", ghost.unwrap_or_default())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(dir()?.join(format!("{key}.json")))
}

/// What was kept of the daemon at `url`, if anything. A copy that cannot
/// be read is said so and taken as none: it is only a copy, and the next
/// one written replaces it.
pub fn load(url: &str, ghost: Option<&str>) -> Cached {
    let Some(path) = path(url, ghost) else {
        return Cached::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Cached::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!("{} is not a copy this desk can read: {e}", path.display());
        Cached::default()
    })
}

/// Keeps a daemon's copy written, a little after it changes.
pub struct Keeper {
    url: String,
    ghost: Option<String>,
    changed: Option<Instant>,
}

impl Keeper {
    pub fn new(url: &str, ghost: Option<&str>) -> Self {
        Self {
            url: url.to_owned(),
            ghost: ghost.map(str::to_owned),
            changed: None,
        }
    }

    /// Something worth keeping changed.
    pub fn changed(&mut self) {
        self.changed.get_or_insert_with(Instant::now);
    }

    /// Write the copy if it changed a moment ago. `cached` is asked for
    /// only then, so that nothing is serialised for nothing. The file is
    /// written whole on a thread of its own, since syncing it takes longer
    /// than a frame; written a second apart at the least, one write is done
    /// long before the next begins, and if not, each is still whole.
    pub fn settle(&mut self, cached: impl FnOnce() -> Cached) {
        let Some(since) = self.changed else { return };
        if since.elapsed() < SETTLE {
            return;
        }
        self.changed = None;
        let Some(path) = path(&self.url, self.ghost.as_deref()) else {
            return;
        };
        if let Ok(text) = serde_json::to_string(&cached()) {
            std::thread::spawn(move || {
                if let Err(e) = crate::whole::write(&path, text.as_bytes()) {
                    tracing::warn!("{} could not be written: {e}", path.display());
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemons_file_is_named_by_its_address_and_character() {
        let p = path("ws://127.0.0.1:7438", None).unwrap();
        assert!(
            p.ends_with("cache/ws---127-0-0-1-7438-.json"),
            "{}",
            p.display()
        );
        let q = path("wss://studio.example/ghost", Some("weiwei")).unwrap();
        assert!(q.ends_with("cache/wss---studio-example-ghost-weiwei.json"));
    }
}
