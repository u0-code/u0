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
    /// What you set for each project: its name, whether it is pinned or
    /// archived, the agent and the way of working its new sessions take.
    #[serde(default)]
    pub prefs: std::collections::BTreeMap<PathBuf, Prefs>,
    /// A command line of yours that starts an agent, by its id, instead of
    /// the registry's way: your own build, a version manager's.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
    /// The variables each agent starts with, by its id and their names:
    /// a value written here, or none where it is kept secret, in the
    /// Keychain (`secrets`).
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Option<String>>>,
    /// Model ids of your own, by agent, offered beside those it lists.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, Vec<String>>,
    /// The models you starred in the model picker, each with its agent,
    /// listed first there.
    #[serde(default)]
    pub favourites: Vec<Favourite>,
    /// What macOS says when a session ends a turn or needs you, while
    /// Ghost is not in front.
    #[serde(default)]
    pub notify: Notify,
    /// Which of what a session does is told.
    #[serde(default)]
    pub notify_on: NotifyOn,
    /// Keep the Mac from sleeping while an agent works.
    #[serde(default = "yes")]
    pub awake: bool,
    /// The days a session is left before it is done; none, never.
    #[serde(default = "week")]
    pub settle_days: Option<u32>,
    /// A session whose pull request merged, or closed, is done.
    #[serde(default = "yes")]
    pub settle_merged: bool,
    /// A session with an open pull request is not done however long it
    /// is left.
    #[serde(default)]
    pub keep_with_pr: bool,
    /// The minutes between fetches from each project's remote; none,
    /// never.
    #[serde(default = "five")]
    pub fetch_every: Option<u32>,
    /// Times on a twelve-hour clock.
    #[serde(default)]
    pub twelve: bool,
    /// Lines added and taken away in blue and orange, not green and red.
    #[serde(default)]
    pub blue_orange: bool,
    /// Text and edges that matter less drawn stronger.
    #[serde(default)]
    pub contrast: bool,
    /// The window's grounds let the desktop show through, blurred.
    #[serde(default)]
    pub glass: bool,
    /// How large the whole window is drawn, in percent of its natural size:
    /// one of `SCALES`.
    #[serde(default = "natural")]
    pub scale: u16,
    /// The face code and numbers are set in.
    #[serde(default)]
    pub mono: CodeFont,
    /// Dark, light, or as the system is.
    #[serde(default)]
    pub look: Look,
    /// The theme the dark look is drawn in, by name; none, Ghost's own.
    #[serde(default)]
    pub theme_dark: Option<String>,
    /// The theme the light look is drawn in, by name; none, Ghost's own.
    #[serde(default)]
    pub theme_light: Option<String>,
    /// A picture drawn faintly under the thread.
    #[serde(default)]
    pub backdrop: Option<PathBuf>,
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
    /// A diff's files show folded, each opened as wanted.
    #[serde(default)]
    pub diff_folded: bool,
    /// The shell a terminal of yours runs.
    #[serde(default)]
    pub shell: Shell,
    /// How large a terminal's text is set.
    #[serde(default)]
    pub terminal_size: TermSize,
    /// How wide the sidebar is, in points, as you dragged it.
    #[serde(default = "left")]
    pub left: u32,
    /// How wide the right column is, in points, as you dragged it: one
    /// width whatever its tab shows.
    #[serde(default = "right")]
    pub right: u32,
    /// What sends what you write.
    #[serde(default)]
    pub send: SendKey,
    /// How an agent's work shows in the thread: Claude's transcript views.
    #[serde(default)]
    pub view: View,
    /// An answer streams in a paragraph at a time, not word by word.
    #[serde(default)]
    pub paragraphs: bool,
    /// TeX in what an agent says (`$…$`, `$$…$$`) is set as formulas.
    #[serde(default)]
    pub math: bool,
    /// The language Ghost speaks, by locale (`en`, `zh-CN`); none, the
    /// system's.
    #[serde(default)]
    pub language: Option<String>,
    /// What a worktree's branch begins with, before the name the writer
    /// gives it.
    #[serde(default = "prefix")]
    pub branch_prefix: String,
    /// The sections of a session's details you hid from the column.
    #[serde(default)]
    pub hidden_details: Vec<Detail>,
    /// A worktree's branch is led by the kind of work the writer sees in
    /// it (`feat/`, `fix/`) instead of the prefix.
    #[serde(default)]
    pub branch_kind: bool,
    /// Who writes what Ghost has written for you.
    #[serde(default)]
    pub writer: Writer,
    /// What you say into the composer, heard on this Mac.
    #[serde(default)]
    pub voice: Voice,
    /// Which builds Ghost updates to.
    #[serde(default)]
    pub channel: crate::update::Channel,
    /// How a pull request is merged from the column beside.
    #[serde(default)]
    pub merge: crate::git::Method,
    /// A squash takes the agents' credits out of its message.
    #[serde(default)]
    pub uncredit: bool,
    /// How many days of Ghost's log are kept in its home (`logs/`).
    #[serde(default = "fortnight")]
    pub log_days: u16,
    /// The ACP registry is read again each hour, for agents' newer
    /// versions.
    #[serde(default = "yes")]
    pub check_agents: bool,
    /// Ghost looks for an update as it starts and every four hours.
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Where the window was, and how large, as Ghost last quit: x, y,
    /// width and height in points; none, centred.
    #[serde(default)]
    pub frame: Option<[i32; 4]>,
    /// What Ghost asks before it does it.
    #[serde(default)]
    pub confirm: Confirm,
    /// Where new worktrees are made; none, the home's `worktrees`.
    #[serde(default)]
    pub worktrees: Option<PathBuf>,
    /// How far a new worktree's submodules are checked out.
    #[serde(default)]
    pub submodules: Submodules,
    /// The folder new and cloned projects go in; none, ~/Documents/Ghost.
    #[serde(default)]
    pub projects_in: Option<PathBuf>,
    /// A clone goes in `<host>/<owner>/<repo>` under that folder, as ghq
    /// lays them out.
    #[serde(default)]
    pub host_folders: bool,
    /// The ways to snooze offered first, each as typed in Custom… ("1h",
    /// "5pm", "tomorrow"), as t3code's snooze presets.
    #[serde(default = "snoozes")]
    pub snoozes: Vec<String>,
    /// The agents whose sessions are not given Ghost's tools, by id.
    #[serde(default)]
    pub no_tools: Vec<String>,
    /// A turn an agent's limit stopped goes on by itself as the limit
    /// resets, where the agent says when.
    #[serde(default = "yes")]
    pub resume_at_reset: bool,
    /// The days an archived session's worktree is left before it is taken
    /// away, as t3code's worktree cleanup; none, kept. One with changes not
    /// committed stays, and its branch always does: opened again, the
    /// session checks it out again.
    #[serde(default)]
    pub tidy_after: Option<u32>,
    /// How the sidebar orders a project's sessions, and the projects.
    #[serde(default)]
    pub sort: Sort,
    /// How much Ghost tells the people who make it (`telemetry`): a level
    /// from 0 to 3; none until you chose one, as Ghost first opens.
    #[serde(default)]
    pub telemetry: Option<crate::telemetry::Level>,
    /// The PRs and Issues pages' views you saved, as t3code's saved views.
    #[serde(default)]
    pub views: Vec<SavedView>,
}

/// A view of the PRs or Issues page you saved: its name, which page, the
/// state and your part in them (as GitHub's search says it, `author:@me`),
/// and the words searched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedView {
    pub name: String,
    pub kind: crate::github::Kind,
    pub state: String,
    pub part: String,
    pub search: String,
}

/// How the sidebar orders, as t3code's sort: by your last message (and
/// a project by its latest), by anything happening, by when it began, or
/// by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Used,
    Updated,
    Begun,
    Name,
}

fn snoozes() -> Vec<String> {
    vec!["1h".into(), "tomorrow".into(), "next week".into()]
}

/// How far a new worktree's submodules are checked out, as t3code's: all
/// the way down, those the repository names itself, or none (for a setup
/// script to do).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Submodules {
    #[default]
    Recursive,
    Top,
    Skip,
}

/// What Ghost asks before it does it, as t3code's confirmations: each
/// action that can be taken back asks only if you want it to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a question of its own, asked or not"
)]
pub struct Confirm {
    /// Before a session is unpinned.
    #[serde(default)]
    pub unpin: bool,
    /// Before a session is archived.
    #[serde(default)]
    pub archive: bool,
    /// Before a session and what was said in it are deleted.
    #[serde(default = "yes")]
    pub delete: bool,
    /// Before a terminal is closed with something running in it.
    #[serde(default = "yes")]
    pub terminal: bool,
    /// What ⌘Q takes to quit.
    #[serde(default)]
    pub quit: Quit,
}

impl Default for Confirm {
    fn default() -> Self {
        Self {
            unpin: false,
            archive: false,
            delete: true,
            terminal: true,
            quit: Quit::default(),
        }
    }
}

/// What ⌘Q takes to quit. Quit in the menu always quits at once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quit {
    /// Held a moment, or pressed twice quickly.
    #[default]
    Hold,
    /// Pressed once.
    Direct,
    /// Pressed twice quickly.
    Double,
}

/// The writer: the agent that writes what Ghost has written for you, and
/// the model it writes with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Writer {
    /// The agent, by id; none, the session's own.
    #[serde(default)]
    pub agent: Option<String>,
    /// Its value in that agent's model option; none, the agent's default.
    #[serde(default)]
    pub model: Option<String>,
    /// Your instructions for commit messages and pull requests, over
    /// Ghost's.
    #[serde(default)]
    pub changes: String,
    /// Your instructions for sessions' titles.
    #[serde(default)]
    pub titles: String,
    /// Your instructions for worktrees' branch names.
    #[serde(default)]
    pub branches: String,
    /// After each turn, what you are likely to say next is written, to be
    /// taken with Tab.
    #[serde(default)]
    pub suggest: bool,
    /// Your instructions for cleaning up what you dictated.
    #[serde(default)]
    pub transcripts: String,
}

/// Dictation: what you say heard by Apple's speech recognition on this
/// Mac, and put in the composer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    /// How the dictation key listens.
    #[serde(default)]
    pub key: VoiceKey,
    /// The language listened for, as a locale ("en-US"); none, the
    /// system's.
    #[serde(default)]
    pub language: Option<String>,
    /// Words to listen for (a name, a term), one to a line; a line
    /// "heard = written" writes what is heard otherwise.
    #[serde(default)]
    pub dictionary: Vec<String>,
    /// "Um", "uh" and the like are left out.
    #[serde(default = "yes")]
    pub no_fillers: bool,
    /// The writer cleans up what was heard before it goes in.
    #[serde(default)]
    pub clean: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            key: VoiceKey::default(),
            language: None,
            dictionary: Vec::new(),
            no_fillers: true,
            clean: false,
        }
    }
}

/// How the dictation key (⇧⌘D) listens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceKey {
    /// A press begins, a press ends; held down, it listens till let go.
    #[default]
    Auto,
    /// A press begins, a press ends.
    Toggle,
    /// It listens while held.
    Hold,
}

/// The shell a terminal runs: your login shell ($SHELL), or one named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    #[default]
    Login,
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    /// Its program; none for your login shell.
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::Login => None,
            Self::Zsh => Some("zsh"),
            Self::Bash => Some("bash"),
            Self::Fish => Some("fish"),
        }
    }
}

/// How large a terminal's text is: its points, and its line's height.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TermSize {
    pub fn points(self) -> (f32, f32) {
        match self {
            Self::Small => (11.5, 17.),
            Self::Medium => (12.5, 19.),
            Self::Large => (14., 21.),
        }
    }
}

/// A section of a session's details in the column beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    Context,
    Plan,
    Background,
    Servers,
    Terminals,
    Lineage,
    Linked,
}

impl Detail {
    pub const ALL: [Self; 7] = [
        Self::Context,
        Self::Plan,
        Self::Background,
        Self::Servers,
        Self::Terminals,
        Self::Lineage,
        Self::Linked,
    ];

    /// The head its section goes under.
    pub fn head(self) -> &'static str {
        match self {
            Self::Context => "Context",
            Self::Plan => "Plan",
            Self::Background => "Background tasks",
            Self::Servers => "Servers",
            Self::Terminals => "Terminals",
            Self::Lineage => "Lineage",
            Self::Linked => "Linked",
        }
    }
}

/// A model you starred: the agent's id, and the model's value in the
/// agent's model option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Favourite {
    pub agent: String,
    pub model: String,
}

/// Which of what a session does is told: a turn finished, one that
/// failed, a question asked before it goes on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyOn {
    #[serde(default = "yes")]
    pub finished: bool,
    #[serde(default = "yes")]
    pub failed: bool,
    #[serde(default = "yes")]
    pub asks: bool,
}

impl Default for NotifyOn {
    fn default() -> Self {
        Self {
            finished: true,
            failed: true,
            asks: true,
        }
    }
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

/// How an agent's work shows in the thread, as Claude's transcript views
/// name them, and two quieter: your messages and its answers only (zen);
/// its calls that went wrong, not those that went well (quiet); each run
/// of its calls folded into a line, its answers in full (normal); that,
/// and its thinking; every call and step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Zen,
    Quiet,
    #[default]
    Normal,
    Thinking,
    Verbose,
}

impl View {
    /// The view after this one, as ⌃O goes round them.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Zen => Self::Quiet,
            Self::Quiet => Self::Normal,
            Self::Normal => Self::Thinking,
            Self::Thinking => Self::Verbose,
            Self::Verbose => Self::Zen,
        }
    }
}

/// What sends what you write: Return; Return while it is one line and
/// ⌘Return once it is more; ⌘Return always. Where Return does not send,
/// it breaks the line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendKey {
    #[default]
    Return,
    Multiline,
    Command,
}

impl SendKey {
    /// Whether Return sends `text`, as it stands.
    pub fn return_sends(self, text: &str) -> bool {
        match self {
            Self::Return => true,
            Self::Multiline => !text.contains('\n'),
            Self::Command => false,
        }
    }
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
        (pane - 64.).clamp(0., most)
    }
}

fn yes() -> bool {
    true
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "serde's default for an Option field"
)]
fn five() -> Option<u32> {
    Some(5)
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "serde's default for an Option field"
)]
fn week() -> Option<u32> {
    Some(7)
}

/// How wide the sidebar stands until you drag it: t3code's.
pub const LEFT: u32 = 256;

fn left() -> u32 {
    LEFT
}

/// How wide the right column stands until you drag it.
pub const RIGHT: u32 = 400;

fn right() -> u32 {
    RIGHT
}

/// The sizes the window can be drawn at, in percent, smallest first; ⌘+
/// and ⌘− step through them.
pub const SCALES: [u16; 8] = [80, 90, 100, 110, 125, 150, 175, 200];

fn fortnight() -> u16 {
    14
}

fn natural() -> u16 {
    100
}

/// The size after `now` among `SCALES`, larger or smaller; `now` at either
/// end.
pub fn next_scale(now: u16, larger: bool) -> u16 {
    if larger {
        SCALES.into_iter().find(|&s| s > now)
    } else {
        SCALES.into_iter().rev().find(|&s| s < now)
    }
    .unwrap_or(now)
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
    /// Its default branch is pulled as Ghost starts and each fetch, when
    /// it is checked out with nothing changed.
    #[serde(default)]
    pub pull: bool,
    /// Its icon as you chose it; none, its own icon file where it has one.
    #[serde(default)]
    pub icon: Option<Icon>,
    /// The Linear team (its key) its issues are read from, rather than its
    /// repository's host.
    #[serde(default)]
    pub linear: Option<String>,
    /// The settings it holds otherwise than everywhere else, as t3code's
    /// project overrides: a sparse `settings.json` of its own, of the keys
    /// `Settings::PROJECT` names. A key not here follows everywhere's.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub over: serde_json::Map<String, serde_json::Value>,
    /// What its new sessions of each agent begin with, by the agent's id:
    /// what was last chosen in one.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub last: std::collections::BTreeMap<String, Chosen>,
}

/// The options and mode last chosen in a session of one agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chosen {
    /// Each option (model, effort, ...) by its id, and its value.
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// A project's icon as you chose it, as t3code's: a picture, or a mark
/// (one or two letters, or an emoji) on a colour, by its place among the
/// desk's marks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Icon {
    Picture(PathBuf),
    Mark { text: String, colour: u8 },
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: crate::registry::KNOWN
                .iter()
                .filter(|k| k.2)
                .map(|k| k.0.to_owned())
                .collect(),
            folders: Vec::new(),
            worktree: false,
            prefs: std::collections::BTreeMap::new(),
            commands: std::collections::BTreeMap::new(),
            env: std::collections::BTreeMap::new(),
            models: std::collections::BTreeMap::new(),
            favourites: Vec::new(),
            notify: Notify::default(),
            notify_on: NotifyOn::default(),
            awake: yes(),
            settle_days: week(),
            settle_merged: yes(),
            keep_with_pr: false,
            fetch_every: five(),
            width: Width::default(),
            look: Look::default(),
            theme_dark: None,
            theme_light: None,
            backdrop: None,

            twelve: false,
            blue_orange: false,
            contrast: false,
            glass: false,
            scale: natural(),
            mono: CodeFont::default(),
            side_by_side: false,
            steer: false,
            branch_prefix: prefix(),
            branch_kind: false,
            hidden_details: Vec::new(),
            diff_folded: false,
            shell: Shell::default(),
            terminal_size: TermSize::default(),
            view: View::default(),
            send: SendKey::default(),
            paragraphs: false,
            math: false,
            language: None,
            left: left(),
            right: right(),
            diff_wrap: yes(),
            editor: None,
            no_spacing: false,
            writer: Writer::default(),
            confirm: Confirm::default(),
            voice: Voice::default(),
            channel: crate::update::Channel::default(),
            merge: crate::git::Method::default(),
            uncredit: false,

            log_days: fortnight(),
            check_updates: yes(),
            check_agents: yes(),

            frame: None,
            worktrees: None,
            submodules: Submodules::default(),
            projects_in: None,
            host_folders: false,
            snoozes: snoozes(),
            no_tools: Vec::new(),
            resume_at_reset: true,
            tidy_after: None,
            sort: Sort::Used,
            telemetry: None,
            views: Vec::new(),
        }
    }
}

impl Settings {
    /// The settings in `home`, as this machine's policy has them
    /// (`policy`); the defaults where there are none. A file that cannot be
    /// read is not overwritten: the reason comes back with the defaults,
    /// and nothing is saved until it is moved away. A policy that does not
    /// read keeps them from loading the same way.
    pub fn load(home: &Path) -> (Self, Option<String>) {
        let read = match crate::policy::current() {
            Err(why) => Err(format!("{why}. Until it is put right")),
            Ok(policy) => Self::read(&home.join("settings.json"), policy.as_ref())
                .map_err(|e| format!("{e}. Move it away to start afresh; until then")),
        };
        match read {
            Ok(settings) => (settings, None),
            Err(e) => (Self::default(), Some(format!("{e} nothing is saved."))),
        }
    }

    /// The settings at `path`, with `policy`'s defaults and locks.
    fn read(path: &Path, policy: Option<&crate::policy::Policy>) -> Result<Self, String> {
        type Map = serde_json::Map<String, serde_json::Value>;
        let mut mine: Map = crate::whole::read_json(path)?;
        let serde_json::Value::Object(mut all) =
            serde_json::to_value(Self::default()).expect("settings serialize")
        else {
            unreachable!("settings are an object")
        };
        if let Some(policy) = policy {
            policy.apply(&mut mine, &all)?;
        }
        all.extend(mine);
        let settings: Self = serde_json::from_value(serde_json::Value::Object(all))
            .map_err(|e| format!("{} does not read ({e})", path.display()))?;
        settings.overs_read()?;
        Ok(settings)
    }

    /// The settings a project may hold otherwise than everywhere else.
    pub const PROJECT: [&'static str; 6] = [
        "merge",
        "uncredit",
        "submodules",
        "branch_kind",
        "resume_at_reset",
        "tidy_after",
    ];

    /// These settings as they hold in `project`: its own where it has them,
    /// everywhere's for the rest.
    #[must_use]
    pub fn of(&self, project: &Path) -> Self {
        self.overlaid(project).unwrap_or_else(|_| self.clone())
    }

    fn overlaid(&self, project: &Path) -> Result<Self, String> {
        let Some(over) = (self.prefs.get(project))
            .map(|p| &p.over)
            .filter(|o| !o.is_empty())
        else {
            return Ok(self.clone());
        };
        let mut all = serde_json::to_value(self).expect("settings serialize");
        for (k, v) in over {
            all[k] = v.clone();
        }
        serde_json::from_value(all).map_err(|e| e.to_string())
    }

    /// Whether each project's own settings are ones a project may hold, and
    /// read; why not, naming the project.
    fn overs_read(&self) -> Result<(), String> {
        for (project, p) in &self.prefs {
            let shown = project.display();
            if let Some(k) = (p.over.keys()).find(|k| !Self::PROJECT.contains(&k.as_str())) {
                return Err(format!("{shown} holds \"{k}\", which a project cannot"));
            }
            self.overlaid(project)
                .map_err(|e| format!("{shown}'s own settings do not read: {e}"))?;
        }
        Ok(())
    }

    /// What `set` makes of these settings, held by `project` alone: each
    /// key it changes that a project may hold becomes the project's own,
    /// and one back at everywhere's value follows everywhere's again.
    pub fn hold(&mut self, project: &Path, set: impl FnOnce(&mut Self)) {
        let mut theirs = self.of(project);
        set(&mut theirs);
        let (all, mine) = (
            serde_json::to_value(&*self).expect("settings serialize"),
            serde_json::to_value(&theirs).expect("settings serialize"),
        );
        let over = &mut self.prefs.entry(project.to_path_buf()).or_default().over;
        for k in Self::PROJECT {
            if mine[k] == all[k] {
                over.remove(k);
            } else {
                over.insert(k.to_owned(), mine[k].clone());
            }
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

    /// The folders Ghost's worktrees are in, in the home `home`: where new
    /// ones are made first, then the home's own, where those made before
    /// another was chosen stay.
    pub fn worktree_folders(&self, home: &Path) -> Vec<PathBuf> {
        let own = home.join("worktrees");
        let mut all: Vec<PathBuf> = self.worktrees.iter().cloned().collect();
        if !all.contains(&own) {
            all.push(own);
        }
        all
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
    fn a_project_holds_its_own_choice_and_follows_everywhere_s_for_the_rest() {
        let (mut s, p) = (Settings::default(), Path::new("/p"));
        s.hold(p, |s| s.merge = crate::git::Method::Rebase);
        assert_eq!(s.of(p).merge, crate::git::Method::Rebase);
        assert_eq!(
            s.merge,
            crate::git::Method::Squash,
            "everywhere's is untouched"
        );
        s.uncredit = true;
        assert!(
            s.of(p).uncredit,
            "what it does not hold follows everywhere's"
        );
        // Set back to everywhere's, it is the project's own no more.
        s.hold(p, |s| s.merge = crate::git::Method::Squash);
        assert!(s.prefs[p].over.is_empty());
        // What a project cannot hold is not held.
        s.hold(p, |s| s.twelve = true);
        assert!(s.prefs[p].over.is_empty() && !s.twelve);
    }

    #[test]
    fn a_project_holding_what_it_cannot_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.prefs
            .entry("/p".into())
            .or_default()
            .over
            .insert("twelve".into(), true.into());
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::to_vec(&s).unwrap(),
        )
        .unwrap();
        let why = Settings::load(home.path()).1.unwrap();
        assert!(why.contains("\"twelve\", which a project cannot"), "{why}");
    }

    #[test]
    fn a_policy_locks_a_setting_whatever_the_file_says_and_fills_one_left_out() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("settings.json");
        std::fs::write(&path, r#"{"enabled": [], "folders": [], "twelve": false}"#).unwrap();
        let policy: crate::policy::Policy = serde_json::from_str(
            r#"{"organization": "Corp", "settings": {
                "twelve": {"value": true, "locked": true},
                "branch_prefix": {"value": "corp/"}}}"#,
        )
        .unwrap();
        let s = Settings::read(&path, Some(&policy)).unwrap();
        assert!(s.twelve);
        assert_eq!(s.branch_prefix, "corp/");
        let wrong: crate::policy::Policy = serde_json::from_str(
            r#"{"organization": "Corp", "settings": {"twelve": {"value": "yes", "locked": true}}}"#,
        )
        .unwrap();
        assert!(Settings::read(&path, Some(&wrong)).is_err());
    }

    #[test]
    fn zooming_steps_through_the_sizes_and_stops_at_either_end() {
        assert_eq!(next_scale(100, true), 110);
        assert_eq!(next_scale(100, false), 90);
        assert_eq!(next_scale(200, true), 200);
        assert_eq!(next_scale(80, false), 80);
        // A size written by hand steps to the nearest listed beyond it.
        assert_eq!(next_scale(133, true), 150);
        assert_eq!(next_scale(133, false), 125);
    }

    #[test]
    fn return_sends_a_multiline_draft_only_where_it_always_sends() {
        let two = "one\ntwo";
        assert!(SendKey::Return.return_sends(two));
        assert!(SendKey::Multiline.return_sends("one"));
        assert!(!SendKey::Multiline.return_sends(two));
        assert!(!SendKey::Command.return_sends("one"));
    }

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
    fn unsaid_confirmations_ask_before_deleting_and_closing_and_quit_on_a_hold() {
        let c: Confirm = serde_json::from_str("{}").unwrap();
        assert_eq!(c, Confirm::default());
        assert!(c.delete && c.terminal && !c.unpin && !c.archive);
        assert_eq!(c.quit, Quit::Hold);
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
