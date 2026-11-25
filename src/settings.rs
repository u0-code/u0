//! What the desk keeps between runs, in its home: which agents are switched
//! on, in the order they are listed, and the projects: the folders work is
//! done in.
//!
//! The home is `GHOST_HOME`, or `~/.ghost`. It holds this file
//! (`settings.json`), the last registry read (`registry.json`) and the
//! agents installed from archives (`agents/`). Each agent keeps its own
//! sign-in and its own sessions where its CLI always does; the desk keeps
//! none of them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The agents switched on when the desk is first opened.
pub const FIRST: [&str; 3] = ["claude-acp", "codex-acp", "antigravity-acp"];

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
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a choice of its own, not a state among several"
)]
pub struct Settings {
    /// The agents shown down the left, by registry id, in order.
    pub enabled: Vec<String>,
    /// The projects down the left, each a folder, the latest added first.
    pub folders: Vec<PathBuf>,
    /// Whether a new session in a git repository works in a worktree of
    /// its own: the choice last made.
    #[serde(default)]
    pub worktree: bool,
    /// Whether the sidebar lists sessions by where they stand (needing
    /// you, working, pinned, recent, done) rather than by project.
    #[serde(default)]
    pub by_status: bool,
    /// The one project the sidebar shows by status, if it shows one.
    #[serde(default)]
    pub only: Option<PathBuf>,
    /// What you set for each project: its name, whether it is pinned or
    /// archived, the agent and the way of working its new sessions take.
    #[serde(default)]
    pub prefs: std::collections::BTreeMap<PathBuf, Prefs>,
    /// A command line of yours that starts an agent, by its id, instead of
    /// the registry's way: your own build, a version manager's.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
    /// Model ids of your own, by agent, offered beside those it lists.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, Vec<String>>,
    /// What macOS says when a session ends a turn or needs you, while
    /// Ghost is not in front.
    #[serde(default)]
    pub notify: Notify,
    /// Keep the Mac from sleeping while an agent works.
    #[serde(default = "yes")]
    pub awake: bool,
    /// The days a session is left before it is done; none, never.
    #[serde(default = "week")]
    pub settle_days: Option<u32>,
    /// Times on a twelve-hour clock.
    #[serde(default)]
    pub twelve: bool,
    /// Lines added and taken away in blue and orange, not green and red.
    #[serde(default)]
    pub blue_orange: bool,
    /// Text and edges that matter less drawn stronger.
    #[serde(default)]
    pub contrast: bool,
    /// The face code and numbers are set in.
    #[serde(default)]
    pub mono: CodeFont,
    /// Dark, light, or as the system is.
    #[serde(default)]
    pub look: Look,
    /// How wide the thread and the composer read.
    #[serde(default)]
    pub width: Width,
    /// A file's diff opens side by side, the column beside widened.
    #[serde(default)]
    pub side_by_side: bool,
    /// What you write while a turn goes on steers it, rather than waiting
    /// for it to end.
    #[serde(default)]
    pub steer: bool,
    /// The editor files open in, by its command; none, the first found.
    #[serde(default)]
    pub editor: Option<String>,
    /// A diff's long lines wrap, rather than being cut at the edge.
    #[serde(default = "yes")]
    pub diff_wrap: bool,
    /// A change only in spacing is left out of diffs.
    #[serde(default)]
    pub no_spacing: bool,
    /// How wide the sidebar is, in points, as you dragged it.
    #[serde(default = "left")]
    pub left: u32,
    /// ⌘Return sends what you write, and Return breaks the line.
    #[serde(default)]
    pub cmd_sends: bool,
    /// How the work an agent did in a turn (its calls, its thinking) shows
    /// in the thread.
    #[serde(default)]
    pub calls: Calls,
    /// What a worktree's branch begins with, before the session's title.
    #[serde(default = "prefix")]
    pub branch_prefix: String,
}

/// What macOS says when a session ends a turn or needs you.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    Off,
    #[default]
    Banner,
    BannerAndSound,
}

/// How an agent's work shows in the thread: every call; a finished
/// turn's work folded into one line; or none, only what was said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Calls {
    Shown,
    #[default]
    Folded,
    Hidden,
}

/// The face code is set in: the one shipped, or one macOS has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeFont {
    #[default]
    JetBrains,
    SfMono,
    Menlo,
}

impl CodeFont {
    /// Its family's name.
    pub fn family(self) -> &'static str {
        match self {
            Self::JetBrains => "JetBrains Mono",
            Self::SfMono => "SF Mono",
            Self::Menlo => "Menlo",
        }
    }
}

/// The look: dark, light, or as the system is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    #[default]
    Dark,
    Light,
    System,
}

/// How wide the thread reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    #[default]
    Comfortable,
    Wide,
    Full,
}

impl Width {
    /// The reading column in a pane `pane` wide.
    pub fn column(self, pane: f32) -> f32 {
        let most = match self {
            Self::Comfortable => 680.,
            Self::Wide => 880.,
            Self::Full => f32::MAX,
        };
        (pane - 64.).clamp(240., most)
    }
}

fn yes() -> bool {
    true
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "serde's default for an Option field"
)]
fn week() -> Option<u32> {
    Some(7)
}

fn left() -> u32 {
    264
}

fn prefix() -> String {
    "ghost/".into()
}

/// What you set for one project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefs {
    /// The name it shows by, if not its folder's.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    /// The agent its new sessions begin with.
    #[serde(default)]
    pub agent: Option<String>,
    /// Whether its new sessions work in a worktree of their own, when it
    /// is a repository; unset, the last choice made anywhere.
    #[serde(default)]
    pub worktree: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: FIRST.iter().map(|s| (*s).to_owned()).collect(),
            folders: Vec::new(),
            worktree: false,
            by_status: false,
            only: None,
            prefs: std::collections::BTreeMap::new(),
            commands: std::collections::BTreeMap::new(),
            models: std::collections::BTreeMap::new(),
            notify: Notify::default(),
            awake: yes(),
            settle_days: week(),
            width: Width::default(),
            look: Look::default(),
            twelve: false,
            blue_orange: false,
            contrast: false,
            mono: CodeFont::default(),
            side_by_side: false,
            steer: false,
            branch_prefix: prefix(),
            calls: Calls::default(),
            cmd_sends: false,
            left: left(),
            diff_wrap: yes(),
            editor: None,
            no_spacing: false,
        }
    }
}

impl Settings {
    /// The settings in `home`; the defaults when there are none. A file
    /// that cannot be read is not overwritten: the reason comes back with
    /// the defaults, and nothing is saved until it is moved away.
    pub fn load(home: &Path) -> (Self, Option<String>) {
        let path = home.join("settings.json");
        let text = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            text => text.map_err(|e| e.to_string()),
        };
        match text.and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string())) {
            Ok(settings) => (settings, None),
            Err(e) => (
                Self::default(),
                Some(format!(
                    "{} is not in a form this desk reads ({e}). Move it away to start afresh; until then nothing is saved.",
                    path.display()
                )),
            ),
        }
    }

    /// Hand the settings to the writer, to be written in `home`.
    pub fn save(&self, home: &Path) {
        let text = serde_json::to_vec_pretty(self).expect("settings serialize");
        crate::store::queue(crate::store::Job::Write(home.join("settings.json"), text));
    }

    /// Switch the agent `id` on or off.
    pub fn toggle(&mut self, id: &str) {
        if let Some(at) = self.enabled.iter().position(|e| e == id) {
            self.enabled.remove(at);
        } else {
            self.enabled.push(id.to_owned());
        }
    }

    /// Make `folder` a project, first in the list; one already a project
    /// keeps its place, so the list does not move under you.
    pub fn used(&mut self, folder: &Path) {
        if !self.folders.iter().any(|f| f == folder) {
            self.folders.insert(0, folder.to_path_buf());
        }
    }

    /// Take `folder` off the projects.
    pub fn forget(&mut self, folder: &Path) {
        self.folders.retain(|f| f != folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_comes_first_and_one_used_again_keeps_its_place() {
        let mut s = Settings::default();
        for i in 0..10 {
            s.used(Path::new(&format!("/p{i}")));
        }
        s.used(Path::new("/p3"));
        assert_eq!(s.folders.len(), 10);
        assert_eq!(s.folders[0], Path::new("/p9"));
        assert_eq!(s.folders[6], Path::new("/p3"));
        s.forget(Path::new("/p3"));
        assert_eq!(s.folders.len(), 9);
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
