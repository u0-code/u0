//! What the desk keeps between runs, in its home: which agents are switched
//! on, in the order they are listed, and the projects: the folders work is
//! done in.
//!
//! The home is `U0_HOME`, or `~/.u0`. It holds this file
//! (`settings.json`), the last registry read (`registry.json`) and the
//! agents installed from archives (`agents/`). Each agent keeps its own
//! sign-in and its own sessions where its CLI always does; the desk keeps
//! none of them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The desk's home.
///
/// A test (this crate's, or the desk's, which build it with the feature
/// `test`) has no home but the one `U0_HOME` names, and never yours:
/// one that asks for it otherwise panics, before it reads or writes there.
pub fn home() -> PathBuf {
    let yours = std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".u0");
    let named = std::env::var_os("U0_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    if cfg!(any(test, feature = "test")) {
        return named
            .filter(|d| *d != yours)
            .expect("a test reached u0's home: give it a temporary U0_HOME, or pass it a folder");
    }
    named.unwrap_or(yours)
}

/// Make u0's home `home` and its `logs/`, each readable and enterable
/// by you alone (0700) whatever it was: what is in them (a log, a
/// session's record, a token) is yours.
pub fn keep_private(home: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    for dir in [home.to_path_buf(), home.join("logs")] {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
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
    /// u0 is not in front.
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
    /// Motion in the window: as the system says, cut down, or in full.
    #[serde(default)]
    pub motion: Motion,
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
    /// The theme the dark look is drawn in, by name; none, u0's own.
    #[serde(default)]
    pub theme_dark: Option<String>,
    /// The theme the light look is drawn in, by name; none, u0's own.
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
    /// The words misspelled in the composer are underlined, the system's
    /// spell checker's guesses on their menu.
    #[serde(default = "yes")]
    pub spell_check: bool,
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
    /// The language u0 speaks, by locale (`en`, `zh-CN`); none, the
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
    /// Who writes what u0 has written for you.
    #[serde(default)]
    pub writer: Writer,
    /// What you say into the composer, heard on this Mac.
    #[serde(default)]
    pub voice: Voice,
    /// Which builds u0 updates to.
    #[serde(default)]
    pub channel: crate::update::Channel,
    /// How a pull request is merged from the column beside.
    #[serde(default)]
    pub merge: crate::git::Method,
    /// A squash takes the agents' credits out of its message.
    #[serde(default)]
    pub uncredit: bool,
    /// How many days of u0's log are kept in its home (`logs/`).
    #[serde(default = "fortnight")]
    pub log_days: u16,
    /// The ACP registry is read again each hour, for agents' newer
    /// versions.
    #[serde(default = "yes")]
    pub check_agents: bool,
    /// u0 looks for an update as it starts and every four hours.
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Where the window was, and how large, as u0 last quit: x, y,
    /// width and height in points; none, centred.
    #[serde(default)]
    pub frame: Option<[i32; 4]>,
    /// What u0 asks before it does it.
    #[serde(default)]
    pub confirm: Confirm,
    /// Where new worktrees are made; none, the home's `worktrees`.
    #[serde(default)]
    pub worktrees: Option<PathBuf>,
    /// How far a new worktree's submodules are checked out.
    #[serde(default)]
    pub submodules: Submodules,
    /// The folder new and cloned projects go in; none, ~/Documents/u0.
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
    /// The agents whose sessions are not given u0's tools, by id.
    #[serde(default)]
    pub no_tools: Vec<String>,
    /// Whether an agent outside u0 (`u0 --mcp`, or the tools over
    /// HTTP, by the key in `mcp.json`) may ask to act on a session: say
    /// something in it, stop its turn; each asked of you in that session.
    /// Off, it only reads.
    #[serde(default)]
    pub outside_acts: bool,
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
    /// How much u0 tells the people who make it (`telemetry`): a level
    /// from 0 to 3; none until you chose one, as u0 first opens.
    #[serde(default)]
    pub telemetry: Option<crate::telemetry::Level>,
    /// Whether the first-run setup screen has been completed or dismissed.
    #[serde(default)]
    pub welcomed: bool,
    /// The PRs and Issues pages' views you saved, as t3code's saved views.
    #[serde(default)]
    pub views: Vec<SavedView>,
    /// How threads are grouped in the sidebar.
    #[serde(default)]
    pub project_grouping: ProjectGrouping,
    /// How projects are ordered in the sidebar.
    #[serde(default)]
    pub project_order: ProjectOrder,
    /// Keep threads that are running or need you at the top of the sidebar.
    #[serde(default = "yes")]
    pub working_section: bool,
    /// Show in-app toast notifications for threads you are not looking at.
    #[serde(default = "yes")]
    pub in_app_notify: bool,
    /// Show the checked-out branch under each project name in the sidebar.
    #[serde(default)]
    pub two_line_projects: bool,
    /// How threads on another machine identify their machine in the title bar.
    #[serde(default)]
    pub machine_id: MachineId,
    /// Paired server names hidden from this window's sidebar without forgetting their keys.
    #[serde(default)]
    pub hidden_servers: Vec<String>,
    /// Formatting, code blocks and inline chips in the composer while you type.
    #[serde(default = "yes")]
    pub rich_composer: bool,
    /// Ask whether to queue or steer each time you send while a turn is running.
    #[serde(default)]
    pub ask_follow_up: bool,
    /// List provider and repository skills after built-in slash commands.
    #[serde(default = "yes")]
    pub slash_skills: bool,
    /// Where subagents and side chats open.
    #[serde(default)]
    pub open_subagents: SubagentPlace,
    /// What a screen reader hears in threads.
    #[serde(default)]
    pub announcements: Announcements,
    /// Put the active thread's status in the window title.
    #[serde(default)]
    pub title_status: bool,
    /// Use theme colors instead of green/red or blue/orange in diffs.
    #[serde(default)]
    pub diff_theme_colors: bool,
    /// Choose unified vs split diff layout automatically from pane width.
    #[serde(default = "yes")]
    pub diff_auto_layout: bool,
    /// Unchanged lines shown around each change in a diff.
    #[serde(default = "three")]
    pub diff_context: u32,
    /// Highlight changed words inside modified diff lines.
    #[serde(default = "yes")]
    pub highlight_words: bool,
    /// Collapse diff files over 400 changed lines automatically.
    #[serde(default = "yes")]
    pub collapse_large_diffs: bool,
    /// Collapse test files under one summary row in diffs.
    #[serde(default)]
    pub hide_test_files: bool,
    /// Open links to pull requests in u0's pull request view instead of the browser.
    #[serde(default = "yes")]
    pub open_prs_here: bool,
    /// Open changes and pull request panels proactively when files change or a PR is linked.
    #[serde(default = "yes")]
    pub proactive_panels: bool,
    /// Where a page you open shows: the Preview tab (where u0 shows pages,
    /// on macOS), or your browser.
    #[serde(default)]
    pub open_pages: PagesIn,
    /// The browser `.app` "Open in your browser" opens pages and links in;
    /// none, the system default.
    #[serde(default)]
    pub browser: Option<PathBuf>,
    /// Read by nothing yet: links open in `browser`. It stays so settings
    /// files saved with it keep reading.
    #[serde(default)]
    pub ask_browser: bool,
    /// `⌘`-clicking a URL printed in a terminal opens it where pages open
    /// (`⌥⌘`, in your browser).
    #[serde(default = "yes")]
    pub cmd_click_urls: bool,
    /// What u0 does when a terminal or agent starts a dev server.
    #[serde(default)]
    pub dev_server: DevServerOpen,
    /// First local port tried when forwarding a dev server from another Mac.
    #[serde(default = "first_forward_port")]
    pub forward_port: u16,
    /// Desktop window snapshot settings.
    #[serde(default)]
    pub snapshots: Snapshots,
    /// Ask whether a new thread works locally or in a new worktree each time.
    #[serde(default)]
    pub ask_worktree: bool,
    /// Explicit default agent for new threads; none, the first enabled agent.
    #[serde(default)]
    pub default_agent: Option<String>,
    /// Global default model id for new threads; none, the agent's default.
    #[serde(default)]
    pub default_model: Option<String>,
    /// Global default mode id per agent for new threads.
    #[serde(default)]
    pub default_modes: std::collections::BTreeMap<String, String>,
    /// Ask before `git push` runs in a thread, whatever mode the agent is in.
    #[serde(default = "yes")]
    pub ask_before_push: bool,
    /// Stream replies as they arrive.
    #[serde(default = "yes")]
    pub streaming: bool,
    /// Days after settling before a thread is automatically archived; none, never.
    #[serde(default = "thirty")]
    pub archive_after: Option<u32>,
    /// Offer snoozing until usage limits reset when a limit stops a thread.
    #[serde(default = "yes")]
    pub snooze_until_reset: bool,
    /// What a new worktree checks out from by default.
    #[serde(default)]
    pub worktree_from: WorktreeBase,
    /// Branch naming template for new worktrees (`{type}`, `{thread}`, `{user}`, `{date}`).
    #[serde(default = "branch_template")]
    pub branch_template: String,
    /// Comma-separated untracked files copied from the checkout into each new worktree.
    #[serde(default = "copied_untracked")]
    pub copy_untracked: String,
    /// When finished worktrees are cleaned up.
    #[serde(default)]
    pub worktree_cleanup: WorktreeCleanup,
    /// Skip worktrees that still have uncommitted local changes during cleanup.
    #[serde(default = "yes")]
    pub keep_dirty_worktrees: bool,
    /// Agent-listed models hidden from the model picker, by agent id.
    #[serde(default)]
    pub hidden_models: std::collections::BTreeMap<String, Vec<String>>,
    /// Standalone custom ACP agents added outside the ACP registry, by id.
    #[serde(default)]
    pub custom_agents: std::collections::BTreeMap<String, CustomAgent>,
    /// Minutes between background agent health checks.
    #[serde(default = "five_u32")]
    pub health_check_mins: u32,
    /// Compact idle threads in the background.
    #[serde(default = "yes")]
    pub compact_idle: bool,
    /// Automatically pull clean default branches on startup and fetch.
    #[serde(default)]
    pub auto_pull: bool,
    /// Sign commits made from u0 with git's configured signing key.
    #[serde(default)]
    pub sign_commits: bool,
    /// Run repository git hooks on commits and pushes made from u0.
    #[serde(default = "yes")]
    pub run_git_hooks: bool,
    /// Open new pull requests as drafts by default.
    #[serde(default)]
    pub pr_drafts: bool,
    /// Populate pull request descriptions from repository pull request templates.
    #[serde(default = "yes")]
    pub pr_templates: bool,
    /// Default commit message style when writing commits.
    #[serde(default)]
    pub commit_style: CommitStyle,
    /// Enable guided pull request review tours written by the Writer.
    #[serde(default = "yes")]
    pub review_guides: bool,
    /// Show the Simulators & devices surface in the right column.
    #[serde(default = "yes")]
    pub use_devices: bool,
    /// Whether agents may drive simulators and devices through MCP.
    #[serde(default)]
    pub agent_devices: DeviceAccess,
    /// Allow booting local simulators when a thread asks for one.
    #[serde(default = "yes")]
    pub local_simulators: bool,
    /// Maximum simulators running at once on this machine.
    #[serde(default = "two_u32")]
    pub max_simulators: u32,
    /// How aggressively background polling runs while the window is hidden.
    #[serde(default)]
    pub background_activity: BackgroundActivity,
    /// Resume working threads automatically after an update or restart.
    #[serde(default = "yes")]
    pub continue_after_restart: bool,
    /// Optional OpenTelemetry OTLP HTTP endpoint for traces and metrics.
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    /// Allow agents to file feedback about u0 itself.
    #[serde(default)]
    pub agent_feedback: bool,
    /// Enable multi-stage workflows.
    #[serde(default = "yes")]
    pub workflows_enabled: bool,
    /// Days concurrency-skipped workflow or task runs remain in history.
    #[serde(default = "seven_u32")]
    pub skipped_runs_days: u32,
    /// Use copy-on-write (`clonefile`) when seeding new worktrees where supported.
    #[serde(default = "yes")]
    pub fast_worktree: bool,
    /// Allow leasing simulators from shared Simlock device pools.
    #[serde(default)]
    pub simlock_pools: bool,
    /// Share pull request state with read-only paired Macs.
    #[serde(default)]
    pub pr_sharing: bool,
    /// Preview: confirm permission answers on a paired phone.
    #[serde(default)]
    pub phone_confirm: bool,
    /// Preview: show Live Activities on a paired phone.
    #[serde(default)]
    pub phone_live_activities: bool,
    /// Preview: open forwarded dev servers on a paired phone.
    #[serde(default)]
    pub phone_dev_server: bool,
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

/// What u0 asks before it does it, as t3code's confirmations: each
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

/// The writer: the agent that writes what u0 has written for you, and
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
    /// u0's.
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
    /// Your instructions for pull request review guides.
    #[serde(default)]
    pub guides: String,
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
    /// The filler words (`fillers`) are left out.
    #[serde(default = "yes")]
    pub no_fillers: bool,
    /// The writer cleans up what was heard before it goes in.
    #[serde(default)]
    pub clean: bool,
    /// Chosen input device name; none, the system default microphone.
    #[serde(default)]
    pub microphone: Option<String>,
    /// What is heard shows as it is heard; off, only that it listens.
    #[serde(default = "yes")]
    pub live_words: bool,
    /// Words said while thinking, not meant, commas between: left out
    /// where `no_fillers` is on.
    #[serde(default = "default_fillers")]
    pub fillers: String,
    /// Said, it takes back the sentence said before it; empty, nothing
    /// does.
    #[serde(default = "default_correction_cue")]
    pub correction_cue: String,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            key: VoiceKey::default(),
            language: None,
            dictionary: Vec::new(),
            no_fillers: true,
            clean: false,
            microphone: None,
            live_words: true,
            fillers: default_fillers(),
            correction_cue: default_correction_cue(),
        }
    }
}

fn default_fillers() -> String {
    "um, umm, uh, uhm, er, erm, ah, hmm, mm".into()
}

fn default_correction_cue() -> String {
    "scratch that".into()
}

/// How threads are grouped in the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectGrouping {
    #[default]
    ByProject,
    ByMachine,
    Flat,
}

/// How projects are ordered in the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectOrder {
    #[default]
    Manual,
    Activity,
    Name,
}

/// How another machine is marked in the title bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineId {
    #[default]
    ColorAndName,
    NameOnly,
    Off,
}

/// Where subagents and side chats open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentPlace {
    #[default]
    RightPanel,
    Split,
    NewWindow,
}

/// What a screen reader announces in threads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Announcements {
    #[default]
    RepliesAndAlerts,
    AlertsOnly,
    Off,
}

/// Where a page you open shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagesIn {
    /// The Preview tab of the thread on screen.
    #[default]
    Preview,
    /// Your browser.
    Browser,
}

/// What u0 does when a dev server starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevServerOpen {
    #[default]
    Show,
    Open,
    Ignore,
}

/// Desktop window snapshot capture settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each toggles one independent capture or feedback behavior"
)]
pub struct Snapshots {
    #[serde(default = "yes")]
    pub shortcut: bool,
    #[serde(default)]
    pub include_text: bool,
    #[serde(default)]
    pub never_capture: Vec<String>,
    #[serde(default)]
    pub sound: bool,
    #[serde(default = "yes")]
    pub flash: bool,
    #[serde(default = "yes")]
    pub animations: bool,
}

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            shortcut: true,
            include_text: false,
            never_capture: Vec::new(),
            sound: false,
            flash: true,
            animations: true,
        }
    }
}

/// What a new worktree checks out from by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeBase {
    #[default]
    OriginDefault,
    LocalMain,
    CurrentBranch,
}

/// When finished worktrees are removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCleanup {
    #[default]
    WhenArchived,
    WhenMerged,
    Never,
}

/// A standalone ACP agent configured by command line outside the ACP registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomAgent {
    pub name: String,
    pub command: String,
}

/// Commit message convention used when writing commits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitStyle {
    #[default]
    MatchHistory,
    Conventional,
    Plain,
}

/// Whether agents may drive simulators and devices over MCP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAccess {
    #[default]
    Ask,
    Allowed,
    Off,
}

/// How hard background tasks work while the window is hidden.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundActivity {
    BatterySaver,
    #[default]
    Balanced,
    FullSpeed,
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
    Checkpoints,
    Linked,
    Plan,
    Background,
    Servers,
    Terminals,
    Lineage,
}

impl Detail {
    pub const ALL: [Self; 8] = [
        Self::Context,
        Self::Checkpoints,
        Self::Linked,
        Self::Plan,
        Self::Background,
        Self::Servers,
        Self::Terminals,
        Self::Lineage,
    ];

    /// The head its section goes under.
    pub fn head(self) -> &'static str {
        match self {
            Self::Context => "Context",
            Self::Checkpoints => "Checkpoints",
            Self::Linked => "Linked",
            Self::Plan => "Plan",
            Self::Background => "Background tasks",
            Self::Servers => "Servers",
            Self::Terminals => "Terminals",
            Self::Lineage => "Lineage",
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

/// The face code is set in: IBM Plex Mono (bundled by u0ui), or one installed on this Mac.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeFont {
    #[default]
    PlexMono,
    JetBrains,
    SfMono,
    Menlo,
}

impl CodeFont {
    /// Its family's name.
    pub fn family(self) -> &'static str {
        match self {
            Self::PlexMono => "IBM Plex Mono",
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

/// Whether the window moves: as the system's Reduce motion says, always
/// cut down (panels and the thread fade rather than slide), or in full.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    #[default]
    System,
    Reduce,
    Full,
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
    /// The reading column in a pane `pane` wide (`chat-max-width` 720px,
    /// `chat-max-width-wide` 880px, `thread-gutter` 24px per side).
    pub fn column(self, pane: f32) -> f32 {
        let most = match self {
            Self::Comfortable => 720.,
            Self::Wide => 880.,
            Self::Full => f32::MAX,
        };
        (pane - 48.).clamp(0., most)
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

#[allow(
    clippy::unnecessary_wraps,
    reason = "serde's default for an Option field"
)]
fn thirty() -> Option<u32> {
    Some(30)
}

fn two_u32() -> u32 {
    2
}

fn three() -> u32 {
    3
}

fn five_u32() -> u32 {
    5
}

fn seven_u32() -> u32 {
    7
}

fn first_forward_port() -> u16 {
    41873
}

fn branch_template() -> String {
    "{type}/{thread}".into()
}

fn copied_untracked() -> String {
    ".env, .env.local".into()
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
    "u0/".into()
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
    /// Its default branch is pulled as u0 starts and each fetch, when
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
    /// Project-specific dictation words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dictionary: Vec<String>,
    /// External folders linked into this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_folders: Vec<PathBuf>,
    /// Submodule paths excluded when creating worktrees for this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_submodules: Vec<PathBuf>,
    /// Environment variables and `secret:<NAME>` references given to agents and actions in this project.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
    /// What you closed of what u0 offers here, until it says something new.
    #[serde(default, skip_serializing_if = "Closed::is_none")]
    pub closed: Closed,
}

/// What you closed of what u0 offers in a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Closed {
    /// The mark (`git::mark`) of the changes whose bar above the composer
    /// you closed, by the folder they are in (the project's, or a
    /// worktree's): once they change, it shows again.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub changes: std::collections::BTreeMap<PathBuf, String>,
    /// The offer to publish it, while it has no remote: once it has one,
    /// it is offered again should it lose it.
    #[serde(default)]
    pub publish: bool,
}

impl Closed {
    fn is_none(&self) -> bool {
        *self == Self::default()
    }
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
            motion: Motion::System,
            scale: natural(),
            mono: CodeFont::default(),
            side_by_side: false,
            steer: false,
            spell_check: yes(),
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
            outside_acts: false,
            resume_at_reset: true,
            tidy_after: None,
            sort: Sort::Used,
            telemetry: None,
            welcomed: false,
            views: Vec::new(),
            project_grouping: ProjectGrouping::default(),
            project_order: ProjectOrder::default(),
            working_section: true,
            in_app_notify: true,
            two_line_projects: false,
            machine_id: MachineId::default(),
            hidden_servers: Vec::new(),
            rich_composer: true,
            ask_follow_up: false,
            slash_skills: true,
            open_subagents: SubagentPlace::default(),
            announcements: Announcements::default(),
            title_status: false,
            diff_theme_colors: false,
            diff_auto_layout: true,
            diff_context: three(),
            highlight_words: true,
            collapse_large_diffs: true,
            hide_test_files: false,
            open_prs_here: true,
            proactive_panels: true,
            open_pages: PagesIn::default(),
            browser: None,
            ask_browser: false,
            cmd_click_urls: true,
            dev_server: DevServerOpen::default(),
            forward_port: first_forward_port(),
            snapshots: Snapshots::default(),
            ask_worktree: false,
            default_agent: None,
            default_model: None,
            default_modes: std::collections::BTreeMap::new(),
            ask_before_push: true,
            streaming: true,
            archive_after: thirty(),
            snooze_until_reset: true,
            worktree_from: WorktreeBase::default(),
            branch_template: branch_template(),
            copy_untracked: copied_untracked(),
            worktree_cleanup: WorktreeCleanup::default(),
            keep_dirty_worktrees: true,
            hidden_models: std::collections::BTreeMap::new(),
            custom_agents: std::collections::BTreeMap::new(),
            health_check_mins: five_u32(),
            compact_idle: true,
            auto_pull: false,
            sign_commits: false,
            run_git_hooks: true,
            pr_drafts: false,
            pr_templates: true,
            commit_style: CommitStyle::default(),
            review_guides: true,
            use_devices: true,
            agent_devices: DeviceAccess::default(),
            local_simulators: true,
            max_simulators: two_u32(),
            background_activity: BackgroundActivity::default(),
            continue_after_restart: true,
            otlp_endpoint: None,
            agent_feedback: false,
            workflows_enabled: true,
            skipped_runs_days: seven_u32(),
            fast_worktree: true,
            simlock_pools: false,
            pr_sharing: false,
            phone_confirm: false,
            phone_live_activities: false,
            phone_dev_server: false,
        }
    }
}

impl Settings {
    /// Whether u0 should open on the first-run setup screen (`Welcome`):
    /// before setup is completed, while there are no projects. A level of
    /// telemetry set already (by a policy) does not skip it: only the sheet
    /// that asks for one.
    #[must_use]
    pub fn needs_welcome(&self) -> bool {
        !self.welcomed && self.folders.is_empty()
    }
    /// These settings as a device that may only read is told them, as
    /// `Event::for_reader` tells it what happens: no value of an agent's
    /// variables (each as if kept secret), nor any command line of yours
    /// that starts one, either of which may hold a key that spends as you.
    #[must_use]
    pub fn for_reader(&self) -> Self {
        let mut told = self.clone();
        (told.env.values_mut().flat_map(|vars| vars.values_mut())).for_each(|v| *v = None);
        told.commands.clear();
        (told.custom_agents.values_mut()).for_each(|a| a.command.clear());
        (told.prefs.values_mut()).for_each(|p| p.env.clear());
        told
    }

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
    pub const PROJECT: [&'static str; 26] = [
        "merge",
        "uncredit",
        "auto_pull",
        "sign_commits",
        "run_git_hooks",
        "pr_drafts",
        "pr_templates",
        "commit_style",
        "review_guides",
        "submodules",
        "branch_prefix",
        "branch_kind",
        "branch_template",
        "worktree_from",
        "copy_untracked",
        "worktree_cleanup",
        "keep_dirty_worktrees",
        "tidy_after",
        "worktree",
        "ask_worktree",
        "default_agent",
        "default_model",
        "ask_before_push",
        "streaming",
        "settle_days",
        "resume_at_reset",
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

    /// The folders u0's worktrees are in, in the home `home`: where new
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
    fn u0_s_home_and_its_logs_are_yours_alone_whatever_they_were() {
        use std::os::unix::fs::PermissionsExt as _;
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join(".u0");
        let mode = |d: &std::path::Path| std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
        keep_private(&home).unwrap();
        assert_eq!((mode(&home), mode(&home.join("logs"))), (0o700, 0o700));
        for d in [&home, &home.join("logs")] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        keep_private(&home).unwrap();
        assert_eq!((mode(&home), mode(&home.join("logs"))), (0o700, 0o700));
    }

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
    fn reduce_motion_is_kept_in_the_file_and_follows_the_system_when_left_out() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(home.path()).0.motion, Motion::System);
        let s = Settings {
            motion: Motion::Reduce,
            ..Settings::default()
        };
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::to_vec(&s).unwrap(),
        )
        .unwrap();
        assert_eq!(Settings::load(home.path()).0.motion, Motion::Reduce);
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

        let nested: crate::policy::Policy = serde_json::from_str(
            r#"{"organization": "Corp", "settings": {
                "confirm.delete": {"value": false, "locked": true},
                "confirm.quit": {"value": "double"}}}"#,
        )
        .unwrap();
        let s = Settings::read(&path, Some(&nested)).unwrap();
        assert!(!s.confirm.delete);
        assert_eq!(s.confirm.quit, Quit::Double);
    }

    #[test]
    fn every_project_overridable_key_exists_on_settings_and_reader_scrubs_project_secrets() {
        let map = serde_json::to_value(Settings::default()).unwrap();
        for key in Settings::PROJECT {
            assert!(map.get(key).is_some(), "missing Settings key: {key}");
        }
        let mut s = Settings::default();
        s.custom_agents.insert(
            "my-acp".into(),
            CustomAgent {
                name: "My ACP".into(),
                command: "my-acp --token sk-secret".into(),
            },
        );
        s.prefs
            .entry("/p".into())
            .or_default()
            .env
            .insert("FIGMA_TOKEN".into(), "figd_secret".into());
        let reader = s.for_reader();
        assert!(reader.custom_agents["my-acp"].command.is_empty());
        assert!(reader.prefs[Path::new("/p")].env.is_empty());
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
    fn what_a_project_closed_is_kept_by_its_folder_and_nothing_else_is_read() {
        let read: Prefs =
            serde_json::from_str(r#"{"closed": {"changes": {"/p/wt": "00ff"}, "publish": true}}"#)
                .unwrap();
        assert_eq!(read.closed.changes[Path::new("/p/wt")], "00ff");
        assert!(read.closed.publish);
        let none = serde_json::to_string(&Prefs::default()).unwrap();
        assert!(!none.contains("closed"));
        assert!(serde_json::from_str::<Prefs>(r#"{"closed": {"bar": true}}"#).is_err());
    }

    #[test]
    fn a_fresh_home_needs_welcome_until_completed_or_configured() {
        let mut s = Settings::default();
        assert!(s.needs_welcome());
        s.welcomed = true;
        assert!(!s.needs_welcome());
        let mut with_folder = Settings::default();
        with_folder.used(Path::new("/project"));
        assert!(!with_folder.needs_welcome());
    }

    #[test]
    fn a_policy_setting_telemetry_still_shows_a_fresh_home_its_setup() {
        // A fresh home: no settings.json.
        let none = std::env::temp_dir().join(format!("u0-welcome-{}.json", std::process::id()));
        for locked in [true, false] {
            let policy: crate::policy::Policy = serde_json::from_value(serde_json::json!({
                "organization": "Example Corp",
                "settings": {"telemetry": {"value": 0, "locked": locked}},
            }))
            .unwrap();
            let s = Settings::read(&none, Some(&policy)).unwrap();
            assert!(s.telemetry.is_some());
            assert!(s.needs_welcome());
        }
    }

    #[test]
    fn a_settings_file_in_another_form_is_refused_not_replaced() {
        let dir = std::env::temp_dir().join(format!("u0-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"who":"me"}"#).unwrap();
        let (s, why) = Settings::load(&dir);
        assert_eq!(s, Settings::default());
        assert!(why.unwrap().contains("Move it away"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
