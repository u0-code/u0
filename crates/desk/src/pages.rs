//! The settings (`settings`), a dialog over the window, and its pages:
//! among them each agent's own (`agent`), where it is installed, signed in
//! to and set up.

mod agent;
mod browser;
mod devices;
mod diagnostics;
mod flows;
pub(crate) mod forge;
mod hosts;
pub(crate) mod keybindings;
mod looks;
pub(crate) mod pairing;
pub(crate) mod pr;
mod prefs;
mod projects;
mod scope;
mod settings;
mod srow;
mod tasks;
mod telemetry;
mod theme_drawer;
mod theme_library;
mod updates;
mod usage;
mod voice;
mod writing;

pub(crate) use diagnostics::process_tree;
pub use settings::{Folder, Tab, height as settings_height};

use gpui::component::input::{InputEvent, InputState, TextareaState};
use gpui::{Context, Entity, SharedString, Window, prelude::*};

use crate::{desk::Desk, settings::Settings};

/// What the pages hold as you use them: whether the settings are open,
/// their tab, what is looked for among the settings and among the
/// registry's agents, what worktree branches begin with as you write it,
/// the action whose keys are being changed, how much each part of u0's
/// home takes and what the last tidying did; on an agent's page, the
/// command of yours that starts it and a model id to add.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool tracks an independent UI state: dialog open, secret mask, pairing in flight, expanded theme grid"
)]
pub struct Pages {
    pub open: bool,
    pub tab: Tab,
    /// The settings sidebar's search field: what the page shown finds.
    pub settings_search: Entity<InputState>,
    pub agent_search: Entity<InputState>,
    pub prefix: Entity<InputState>,
    /// Your snooze presets as you write them, commas between.
    pub snoozes: Entity<InputState>,
    /// Settings › Keybindings: what is looked for among the actions, which
    /// of them show, the action whose keys are being changed with the keys
    /// recorded for it, and where they are to hold as it is typed.
    pub keys_search: Entity<InputState>,
    pub keys_show: crate::pages::keybindings::Show,
    pub editing_keys: Option<(&'static str, SharedString)>,
    pub keys_when: Entity<InputState>,
    /// The server the pages of a server's own are of: its agents,
    /// projects, tasks, sharing and what it keeps.
    pub on: crate::hub::Host,
    pub sizes: crate::tidy::StorageReport,
    pub tidied: Option<String>,
    /// The agent whose page Settings › Agents shows, if one; none, the list.
    pub agent: Option<String>,
    pub command: Entity<InputState>,
    pub model: Entity<InputState>,
    /// A variable for an agent: its name, its value (hidden as it is
    /// typed), and whether it is kept secret.
    pub var_name: Entity<InputState>,
    pub var_value: Entity<InputState>,
    pub var_secret: bool,
    /// The name of another account of an agent, as it is typed.
    pub account: Entity<InputState>,
    /// The project whose page Settings → Projects shows, if one.
    pub project: Option<crate::desk::Place>,
    /// Your instructions to the writer, by what it writes.
    pub changes: Entity<TextareaState>,
    pub titles: Entity<TextareaState>,
    pub branches: Entity<TextareaState>,
    pub transcripts: Entity<TextareaState>,
    /// Dictation's filler words and correction cue as you write them.
    pub fillers: Entity<InputState>,
    pub cue: Entity<InputState>,
    /// The days the usage page reads over; none, all.
    pub period: Option<u64>,
    /// Project filter on the Usage workspace page (`None` = all projects).
    pub usage_project: Option<std::path::PathBuf>,
    /// Machine filter on the Usage workspace page (`None` = all machines).
    pub usage_host: Option<crate::hub::Host>,
    /// The settings sidebar's Agents folder.
    pub agents: Folder,
    /// The project the settings a project may hold are set for, as
    /// t3code's "Applying settings for"; none, everywhere.
    pub scope: Option<std::path::PathBuf>,
    /// What is known of each forge here, and a token of yours for each,
    /// as it is typed (by `forge::Host::ALL`).
    pub forges: Vec<crate::forge::Seen>,
    pub tokens: [Entity<InputState>; 5],
    /// Themes of yours that did not read, each said.
    pub themes_refused: Vec<String>,
    /// Search query in `Settings › Appearance › Themes`: your themes, and
    /// Open VSX's.
    pub theme_search: Entity<InputState>,
    /// What Open VSX found for the search, by the query it was asked.
    pub vsx: Option<(String, Result<Vec<crate::themes::Listed>, String>)>,
    /// The extension being brought from Open VSX, by its title.
    pub vsx_adding: Option<String>,
    /// What you ask the writer for a theme with, while it is open; and
    /// whether a theme is being written.
    pub theme_ask: Option<Entity<InputState>>,
    pub theme_asked: bool,
    /// Whether `Settings › Appearance › Themes` expands beyond the first 6 cards.
    pub show_all_themes: bool,
    /// Custom theme currently open in the `600px` `Customize <Theme>` drawer.
    pub editing_theme: Option<String>,
    /// What u0 runs (the agents among it, by id and process id), and
    /// the warnings and errors in its log: Settings › Diagnostics.
    pub processes: (Vec<(String, u32)>, crate::diagnostics::Processes),
    /// Those processes as gpui-kit's tree.
    pub process_tree: Entity<gpui::component::tree::TreeState>,
    pub troubles: Vec<crate::diagnostics::LogTrouble>,
    /// Random install id read from `telemetry.json` for About and Telemetry.
    pub install_id: Option<String>,
    /// How sharing this u0 stands, as its server last said.
    pub shared: Option<crate::share::Shared>,
    /// The named tunnel's address and token as you write them.
    pub named_host: Entity<InputState>,
    pub named_token: Entity<InputState>,
    /// The servers paired with, why `servers.json` did not read if so, the
    /// link pasted to pair with another, and whether that is under way.
    pub servers: crate::servers::Servers,
    pub servers_refused: Option<String>,
    pub pair_link: Entity<InputState>,
    pub pairing: bool,
    /// What the next pairing link made lets in.
    pub grant: crate::access::Grant,
    /// The pairing link this desk last made, when, and what it lets in:
    /// told to this desk alone.
    pub offered: Option<(String, u64, crate::access::Grant)>,
    /// The way the next pairing link names; none, the best there is.
    pub route: Option<crate::reach::Route>,
    /// Active Carbon pairing dialog (`Show Pairing Link…` / `Pair a Device…`).
    pub pairing_modal: Option<pairing::PairingModal>,
    /// Recently approved device `(label, public_key, standing)` for the green `Alert`.
    pub recently_approved: Option<(String, String, String)>,
    /// The telemetry level on its slider, and the batch each level would
    /// send, as the server last showed it.
    pub level: Entity<gpui::component::slider::SliderState>,
    pub sent: [Option<String>; 4],
}

impl Pages {
    pub fn new(settings: &Settings, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let field = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Desk>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr(placeholder)))
        };
        let settings_search = field("Search settings", window, cx);
        // What is looked for is found among the pages, not behind an agent's.
        cx.subscribe(&settings_search, |this, input, _: &InputEvent, cx| {
            if !input.read(cx).value().is_empty() {
                this.pages.agent = None;
            }
            cx.notify();
        })
        .detach();
        let agent_search = field("Search the ACP registry", window, cx);
        cx.subscribe(&agent_search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let theme_search = field("Search themes and Open VSX", window, cx);
        cx.subscribe(&theme_search, |this, _, _: &InputEvent, cx| {
            this.search_vsx(cx);
            cx.notify();
        })
        .detach();
        let prefix = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("u0/")
                .default_value(settings.branch_prefix.clone())
        });
        cx.subscribe(&prefix, |this, input, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                input
                    .read(cx)
                    .value()
                    .trim()
                    .clone_into(&mut this.settings.branch_prefix);
                this.save_settings();
            }
        })
        .detach();
        let snoozes = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr("1h, tomorrow"))
                .default_value(settings.snoozes.join(", "))
        });
        cx.subscribe(&snoozes, |this, input, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                this.settings.snoozes = (input.read(cx).value().split(','))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                this.save_settings();
            }
        })
        .detach();
        let keys_search = field("Search commands, or type keys like ⌘J", window, cx);
        cx.subscribe(&keys_search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let keys_when = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe(&keys_when, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        Self {
            open: false,
            tab: Tab::General,
            settings_search,
            agent_search,
            prefix,
            snoozes,
            keys_search,
            keys_show: crate::pages::keybindings::Show::All,
            editing_keys: None,
            keys_when,
            sizes: crate::tidy::StorageReport::default(),
            tidied: None,
            agent: None,
            command: field("/path/to/agent --acp", window, cx),
            model: field("a model id", window, cx),
            var_name: field("NAME", window, cx),
            account: field("Work", window, cx),
            var_value: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::tr("value, or op://vault/item/field"))
                    .masked(true)
            }),
            var_secret: true,
            project: None,
            period: Some(7),
            usage_project: None,
            usage_host: None,
            agents: Folder::default(),
            scope: None,
            forges: Vec::new(),
            themes_refused: Vec::new(),
            theme_search,
            vsx: None,
            vsx_adding: None,
            theme_ask: None,
            theme_asked: false,
            show_all_themes: false,
            editing_theme: None,
            processes: Default::default(),
            process_tree: cx.new(|cx| gpui::component::tree::TreeState::new(cx)),
            troubles: Vec::new(),
            install_id: None,

            tokens: crate::forge::Host::ALL.map(|_| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(crate::tr("a token"))
                        .masked(true)
                })
            }),
            on: crate::hub::Host::HERE,
            shared: None,
            named_host: field("u0.example.com", window, cx),
            named_token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::tr("Tunnel token"))
                    .masked(true)
            }),
            servers: crate::servers::Servers::default(),
            servers_refused: None,
            level: telemetry::slider(
                settings.telemetry.unwrap_or(crate::telemetry::Level::Usage),
                window,
                cx,
            ),
            sent: Default::default(),
            pair_link: field("u0://pair?host=…", window, cx),
            pairing: false,
            grant: crate::access::Grant::default(),
            offered: None,
            route: None,
            pairing_modal: None,
            recently_approved: None,
            changes: instructions_field(
                &settings.writer.changes,
                "As the repository's recent commits do",
                |w| &mut w.changes,
                window,
                cx,
            ),
            titles: instructions_field(
                &settings.writer.titles,
                "What it is about, in 3 to 8 words",
                |w| &mut w.titles,
                window,
                cx,
            ),
            branches: instructions_field(
                &settings.writer.branches,
                "2 to 6 words, lower case, joined by hyphens",
                |w| &mut w.branches,
                window,
                cx,
            ),
            transcripts: instructions_field(
                &settings.writer.transcripts,
                "Correct what was misheard, take out fillers, punctuate",
                |w| &mut w.transcripts,
                window,
                cx,
            ),
            fillers: voice_field(&settings.voice.fillers, |v| &mut v.fillers, window, cx),
            cue: voice_field(
                &settings.voice.correction_cue,
                |v| &mut v.correction_cue,
                window,
                cx,
            ),
        }
    }
}

/// A field for your instructions to the writer, written with `set` as
/// you type.
fn instructions_field(
    now: &str,
    placeholder: &'static str,
    set: fn(&mut crate::settings::Writer) -> &mut String,
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> Entity<TextareaState> {
    let input = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(2, 8)
            .placeholder(crate::tr(placeholder))
            .default_value(now.to_owned())
    });
    cx.subscribe(&input, move |this, input, e: &InputEvent, cx| {
        if let InputEvent::Change = e {
            input
                .read(cx)
                .value()
                .trim()
                .clone_into(set(&mut this.settings.writer));
            this.save_settings();
        }
    })
    .detach();
    input
}

/// A field for one of dictation's settings, written with `set` as you
/// type.
fn voice_field(
    now: &str,
    set: fn(&mut crate::settings::Voice) -> &mut String,
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(now.to_owned()));
    cx.subscribe(&input, move |this, input, e: &InputEvent, cx| {
        if let InputEvent::Change = e {
            input
                .read(cx)
                .value()
                .trim()
                .clone_into(set(&mut this.settings.voice));
            this.save_settings();
        }
    })
    .detach();
    input
}
