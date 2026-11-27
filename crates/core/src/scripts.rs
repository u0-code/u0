//! A repository's own commands, named in `u0.json` at its root:
//!
//! ```json
//! { "scripts": [ { "name": "dev", "command": "npm run dev" },
//!                { "name": "setup", "command": "npm install", "worktree": true } ] }
//! ```
//!
//! Each shows as a button in the drawer and runs in a terminal of its own;
//! one marked `worktree` runs in each new worktree made for a session, as
//! it is made, and one marked `wait` as well holds the session's first
//! message until it has run; one marked `settle` runs in a session's
//! worktree as the session settles, one marked `stop` is stopped then,
//! and `preview` is the address its preview opens. A key runs one, as
//! `"action": "script:dev"` in keybindings.json. The drawer adds, changes
//! and takes away
//! scripts, written back to the file.
//!
//! It may also name, as `"sparse": ".sparse"`, a file of git's
//! sparse-checkout patterns, which new worktrees check out alone.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A repository's own commands, from `u0.json` at its root.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub scripts: Vec<Script>,
    /// A file of git sparse-checkout patterns (non-cone), by its path in
    /// the repository: a new worktree checks out only what they match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sparse: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a moment it runs or stops at, a choice of its own"
)]
pub struct Script {
    pub name: String,
    pub command: String,
    /// Run in each new worktree, as it is made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktree: bool,
    /// Run so in a new worktree, the session's first message waits until
    /// it has run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
    /// Run in its session's own worktree each time the session settles
    /// (to take away what a build left); one with no worktree skips it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub settle: bool,
    /// Stopped, its terminal closed, as its session settles.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop: bool,
    /// The address its preview opens, rather than the last one it prints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Put `script` at `at` in `u0.json` of the repository at `dir` (a new
/// one at the end), or take the one at `at` away when `script` is none.
pub fn write_script(dir: &Path, at: Option<usize>, script: Option<Script>) -> Result<(), String> {
    let root = crate::git::root(dir).ok_or("the folder is not in a git repository")?;
    let path = root.join("u0.json");
    let mut config: Config = crate::whole::read_json(&path)?;
    match (at, script) {
        (Some(at), Some(script)) if at < config.scripts.len() => config.scripts[at] = script,
        (Some(at), None) if at < config.scripts.len() => {
            config.scripts.remove(at);
        }
        (None, Some(script)) => config.scripts.push(script),
        _ => return Err("the script is no longer in u0.json".into()),
    }
    let mut text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    text.push('\n');
    crate::whole::write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// The commands `u0.json` names in the repository whose root is `root`;
/// none where there is no such file, and none (said once in the log) where
/// it does not read.
pub fn config(root: &Path) -> Config {
    crate::whole::read_json(&root.join("u0.json")).unwrap_or_else(|e| {
        tracing::warn!("{e}");
        Config::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_written_is_kept_in_u0_json_and_one_that_does_not_read_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git");
        };
        git(&["init", "-q"]);
        let script = |name: &str| Script {
            name: name.into(),
            command: "npm i".into(),
            worktree: true,
            wait: true,
            ..Script::default()
        };
        write_script(dir, None, Some(script("setup"))).expect("written");
        write_script(dir, None, Some(script("dev"))).expect("written");
        write_script(dir, Some(0), None).expect("taken away");
        let names: Vec<_> = config(dir).scripts.into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["dev"]);
        let text = std::fs::read_to_string(dir.join("u0.json")).expect("file");
        assert!(text.contains("\"wait\": true"), "{text}");
        std::fs::write(dir.join("u0.json"), "{ not json").expect("broken");
        assert!(write_script(dir, None, Some(script("x"))).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("u0.json")).expect("kept"),
            "{ not json"
        );
    }
}
