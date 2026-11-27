//! The pages that take the middle in place of a thread, as the design has
//! them: an agent's own, which gets it ready to run in three steps; and
//! the settings (`settings`), a dialog over the
//! window, where agents are switched on and off.

mod devices;
mod diagnostics;
mod flows;
pub(crate) mod forge;
mod hosts;
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
pub(crate) use prefs::whole;
pub use settings::{Tab, height as settings_height};

use gpui::component::{
    IconName, Sizable as _,
    input::{Input, InputEvent, InputState, TextareaState},
    progress::Progress,
    scroll::ScrollableElement as _,
    setting::SettingsState,
    stepper::{Stepper, StepperItem},
};
use gpui::{Context, Div, Entity, SharedString, Window, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::{Host, Slot, Stage},
    registry::{Missing, Way},
    session::State,
    settings::Settings,
    theme,
    ui::{self, Weight},
};

/// What the pages hold as you use them: whether the settings are open,
/// their tab, what is looked for among the settings and among the
/// registry's agents, what worktree branches begin with as you write it,
/// the action whose keys are being recorded, how much each part of u0's
/// home takes and what the last tidying did; on an agent's page, the
/// command of yours that starts it and a model id to add.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool tracks an independent UI state: dialog open, secret mask, pairing in flight, expanded theme grid"
)]
pub struct Pages {
    pub open: bool,
    pub tab: Tab,
    /// The settings' page shown and their search: gpui-kit's.
    pub settings: Entity<SettingsState>,
    /// The Carbon settings sidebar's search field, synced into `settings`.
    pub settings_search: Entity<InputState>,
    pub agent_search: Entity<InputState>,
    pub prefix: Entity<InputState>,
    /// Your snooze presets as you write them, commas between.
    pub snoozes: Entity<InputState>,
    pub recording: Option<&'static str>,
    /// The server the pages of a server's own are of: its agents,
    /// projects, tasks, sharing and what it keeps.
    pub on: crate::hub::Host,
    pub sizes: crate::tidy::StorageReport,
    pub tidied: Option<String>,
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
    /// Your dictation's dictionary, a word or "heard = written" to a line.
    pub dictionary: Entity<TextareaState>,
    /// The days the usage page reads over; none, all.
    pub period: Option<u64>,
    /// Project filter on the Usage workspace page (`None` = all projects).
    pub usage_project: Option<std::path::PathBuf>,
    /// Machine filter on the Usage workspace page (`None` = all machines).
    pub usage_host: Option<crate::hub::Host>,
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
    /// The telemetry level on its slider, and the batch it would send,
    /// as the server last showed it.
    pub level: Entity<gpui::component::slider::SliderState>,
    pub sent: Option<String>,
}

impl Pages {
    pub fn new(settings: &Settings, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let field = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Desk>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr(placeholder)))
        };
        let shown = cx.new(|cx| SettingsState::new(window, cx));
        let settings_search = field("Search settings", window, cx);
        cx.subscribe(&settings_search, |_, _, _: &InputEvent, cx| cx.notify())
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
        Self {
            open: false,
            tab: Tab::General,
            settings: shown,
            settings_search,
            agent_search,
            prefix,
            snoozes,
            recording: None,
            sizes: crate::tidy::StorageReport::default(),
            tidied: None,
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
            sent: None,
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
            dictionary: dictionary_field(&settings.voice.dictionary, window, cx),
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

/// The dictionary's field: a line each, kept as you type.
fn dictionary_field(
    now: &[String],
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> Entity<TextareaState> {
    let input = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(2, 8)
            .placeholder(crate::tr("u0\ngpui kit = gpui-kit"))
            .default_value(now.join("\n"))
    });
    cx.subscribe(&input, |this, input, e: &InputEvent, cx| {
        if let InputEvent::Change = e {
            this.settings.voice.dictionary = (input.read(cx).value().lines())
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
            this.save_settings();
        }
    })
    .detach();
    input
}

/// The width of a page's column.
const PAGE: f32 = 560.;

/// Where a step of getting an agent ready stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Todo,
    Now,
    Done,
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .border_1()
        .border_color(theme::border())
}

impl Desk {
    pub(crate) fn agent_page(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let page = div()
            .id("page")
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_h(px(0.));
        let Some(entry) = self.entry(id) else {
            return page
                .child(
                    ui::text(faces, crate::tr("The registry does not list this agent."))
                        .mt(px(72.)),
                )
                .overflow_y_scrollbar();
        };
        let enabled = self.settings_on(host).enabled.iter().any(|e| e == id);
        let stage = self.slot(host, id).map_or_else(
            || Slot::new(host, entry.clone(), self.here_on(host, id), None).stage,
            |s| s.stage.clone(),
        );
        let name = self.name_of(id);
        let command = match entry.way() {
            Way::Archive => {
                let b = &entry.distribution.binary[&crate::registry::platform()];
                std::iter::once(b.cmd.trim_start_matches("./").to_owned())
                    .chain(b.args.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            Way::Runner { runner, package } => format!("{runner} -y {package}"),
            Way::None => String::new(),
        };
        let opened = self
            .sessions
            .all
            .iter()
            .any(|s| s.of(host, id) && s.meta.id.is_some());
        let signing = self
            .sessions
            .all
            .iter()
            .any(|s| s.of(host, id) && s.state == State::SignIn);

        let named = [("name", &name as &dyn std::fmt::Display)];
        let (headline, ready) = match &stage {
            Stage::Missing(_) => (
                crate::tr_with("%{name} isn't on this Mac yet", &named),
                false,
            ),
            Stage::Installing(_) => (crate::tr_with("Installing %{name}", &named), false),
            Stage::Failed(_) => (crate::tr_with("%{name} stopped", &named), true),
            _ => (crate::tr_with("%{name} is ready", &named), true),
        };
        let one = match &stage {
            Stage::Missing(_) => Step::Todo,
            Stage::Installing(_) => Step::Now,
            _ => Step::Done,
        };
        let two = match &stage {
            Stage::Running { .. } => Step::Done,
            Stage::Starting => Step::Now,
            // Let go while idle, it was heard from all the same.
            Stage::Ready if self.slot(host, id).is_some_and(|s| s.known.is_some()) => Step::Done,
            _ => Step::Todo,
        };
        let auth = self
            .slot(host, id)
            .map_or(crate::hub::Auth::Unknown, |s| s.auth.clone());
        let can_log_out = self
            .slot(host, id)
            .filter(|s| s.running())
            .and_then(Slot::caps)
            .is_some_and(|c| c.auth.logout.is_some());
        let three = match &auth {
            crate::hub::Auth::Needed(_) => Step::Now,
            crate::hub::Auth::Ready => Step::Done,
            crate::hub::Auth::Unknown if opened => Step::Done,
            crate::hub::Auth::Unknown if signing => Step::Now,
            crate::hub::Auth::Unknown => Step::Todo,
        };
        let installed = self
            .here_on(host, &entry.id)
            .and_then(|h| h.installed.clone());
        let update = installed.as_ref().filter(|v| **v != entry.version);
        let available = |v: &String| {
            crate::tr_with(
                "%{name} %{v} installed · %{latest} available",
                &[("name", &name), ("v", v), ("latest", &entry.version)],
            )
        };
        let first_step = match entry.way() {
            Way::Archive => match update {
                Some(v) => available(v),
                None => crate::tr_with(
                    "Download %{name} %{latest}",
                    &[("name", &name), ("latest", &entry.version)],
                ),
            },
            Way::Runner { runner, .. } => match &installed {
                Some(v) if *v == entry.version => {
                    crate::tr_with("%{name} %{v} installed in u0", &[("name", &name), ("v", v)])
                }
                Some(v) => available(v),
                None => crate::tr_with(
                    "Install %{name} %{latest} with %{runner}",
                    &[
                        ("name", &name),
                        ("latest", &entry.version),
                        ("runner", &if runner == "npx" { "npm" } else { "uv" }),
                    ],
                ),
            },
            Way::None => crate::tr("No build for this Mac"),
        };
        let mut download = div().flex().flex_col().flex_1().gap(px(8.)).child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .child(
                    ui::text(faces, first_step)
                        .weight(gpui::FontWeight::MEDIUM)
                        .text_color(if one == Step::Todo {
                            theme::text()
                        } else {
                            theme::text2()
                        }),
                )
                .child(ui::fine(
                    faces,
                    crate::tr_with(
                        "%{platform} · from the ACP registry",
                        &[("platform", &crate::registry::platform())],
                    ),
                )),
        );
        if let Stage::Installing(install) = &stage {
            // How far it is, when the size is known; never a guess.
            download = download
                .children(install.share.map(|share| {
                    Progress::new("install-share")
                        .xsmall()
                        .value(share * 100.)
                        .color(theme::accent())
                }))
                .child(ui::fine(faces, install.what.clone()));
        } else if let Stage::Missing(m) = &stage
            && !matches!(m, Missing::Download)
        {
            download = download.child(ui::trouble(m.say(&name)));
        } else {
            download = download.child(
                ui::mono(faces, Desk::shown(&entry.dir(self.home_on(host))))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::faint()),
            );
        }
        // The CLI it drives: yours, as your terminal has it.
        if let Some(cli) = self.here_on(host, &entry.id).and_then(|h| h.cli.clone()) {
            let version = (self.machine(host))
                .and_then(|m| m.cli_versions.get(&cli))
                .cloned()
                .unwrap_or_default();
            download = download.child(ui::fine(
                faces,
                crate::tr_with(
                    "Runs your %{cli} %{version} · %{path}",
                    &[
                        (
                            "cli",
                            &cli.file_name()
                                .map(|n| n.to_string_lossy())
                                .unwrap_or_default(),
                        ),
                        ("version", &version),
                        ("path", &Desk::shown(&cli)),
                    ],
                ),
            ));
        }
        // A step done is checked; the first not done is where you are.
        let step = |st: Step, body: Div| {
            StepperItem::new()
                .when(st == Step::Done, |i| i.icon(IconName::Check))
                .child(body.min_w(px(0.)).pb(px(14.)))
        };
        let simple = |title: SharedString, text: SharedString, st: Step| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(4.))
                .child(
                    ui::text(faces, title)
                        .weight(gpui::FontWeight::MEDIUM)
                        .text_color(if st == Step::Todo {
                            theme::text2()
                        } else {
                            theme::text()
                        }),
                )
                .child(ui::fine(faces, text))
        };
        let now = [one, two, three]
            .iter()
            .position(|st| *st != Step::Done)
            .unwrap_or(2);
        let steps = card().px(px(16.)).pt(px(14.)).child(
            Stepper::new("agent-steps")
                .vertical()
                .selected_index(now)
                .item(step(one, download))
                .item(step(
                    two,
                    simple(
                        crate::tr("Check that it speaks ACP"),
                        crate::tr_with(
                            "Starts %{command} once and reads its version and abilities.",
                            &[("command", &command)],
                        ),
                        two,
                    ),
                ))
                .item(step(
                    three,
                    simple(
                        crate::tr_with("Sign in to %{name}", &named),
                        match &auth {
                            crate::hub::Auth::Needed(why) => crate::tr_with(
                                "%{name} says: %{why}",
                                &[("name", &name), ("why", why)],
                            ),
                            crate::hub::Auth::Ready => {
                                crate::tr_with("Signed in: %{name} opens threads.", &named)
                            }
                            crate::hub::Auth::Unknown => crate::tr_with(
                                "%{name}'s own sign-in. Skipped if you're already signed in.",
                                &named,
                            ),
                        },
                        three,
                    ),
                )),
        );

        let (install_id, recheck_id, start_id, toggle_id) =
            (id.to_owned(), id.to_owned(), id.to_owned(), id.to_owned());
        let (cancel_id, update_id, uninstall_id) = (id.to_owned(), id.to_owned(), id.to_owned());
        let busy = self
            .sessions
            .all
            .iter()
            .any(|s| s.meta.agent == id && matches!(s.state, State::Working | State::Opening));
        let mut actions = div().flex().items_center().gap(px(8.));
        match &stage {
            Stage::Installing(_) => {
                actions = actions.child(
                    ui::button("cancel-install", crate::tr("Cancel"), Weight::Secondary).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.cancel_install(host, &cancel_id, cx);
                        }),
                    ),
                );
            }
            Stage::Missing(Missing::Download) => {
                actions = actions.child(
                    ui::button(
                        "install",
                        crate::tr_with("Install %{name}", &named),
                        Weight::Primary,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.install(host, &install_id, cx)),
                    ),
                );
            }
            Stage::Missing(Missing::Runner(runner)) => {
                let url = if *runner == "npx" {
                    "https://nodejs.org/"
                } else {
                    "https://docs.astral.sh/uv/"
                };
                let label = url.trim_start_matches("https://").trim_end_matches('/');
                actions = actions
                    .child(
                        ui::button("recheck", crate::tr("Try again"), Weight::Secondary).on_click(
                            cx.listener(move |this, _, _, cx| this.recheck(host, &recheck_id, cx)),
                        ),
                    )
                    .child(
                        ui::button("get-runner", label, Weight::Quiet)
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
                    );
            }
            Stage::Failed(why) => {
                use u0ui::prelude::*;
                actions = actions.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .child(
                                    div().mt(px(2.)).child(
                                        Icon::carbon(CarbonIcon::ErrorFilled)
                                            .size(IconSize::S16)
                                            .color(color(ColorRole::SupportError)),
                                    ),
                                )
                                .child(
                                    div()
                                        .type_style(TypeStyle::Body01)
                                        .text_color(color(ColorRole::TextSecondary))
                                        .child(why.clone()),
                                ),
                        )
                        .child(
                            div().flex().child(
                                Button::new("recheck", crate::tr("Check that it speaks ACP"))
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.recheck(host, &recheck_id, cx);
                                    })),
                            ),
                        ),
                );
            }
            _ if enabled => {
                actions = actions.child(
                    ui::button("start", crate::tr("New thread"), Weight::Primary).on_click(
                        cx.listener(move |this, _, window, cx| {
                            if let Some(project) = this.project() {
                                this.new_session(&start_id, project, window, cx);
                            }
                        }),
                    ),
                );
            }
            _ => {}
        }
        if update.is_some() && enabled && !busy && !matches!(stage, Stage::Installing(_)) {
            actions = actions.child(
                ui::button(
                    "update",
                    crate::tr_with("Update to %{version}", &[("version", &entry.version)]),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.install(host, &update_id, cx))),
            );
        }
        if matches!(auth, crate::hub::Auth::Needed(_)) {
            let (a, b) = (id.to_owned(), id.to_owned());
            actions = actions
                .child(
                    ui::button("page-sign-in", crate::tr("Sign in"), Weight::Primary).on_click(
                        cx.listener(move |this, _, _, cx| this.sign_in_agent(host, &a, cx)),
                    ),
                )
                .child(
                    ui::button("page-signed-in", crate::tr("I've signed in"), Weight::Quiet)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.recheck_auth(host, &b, cx)),
                        ),
                );
        } else if can_log_out && auth == crate::hub::Auth::Ready && !busy {
            let a = id.to_owned();
            actions = actions.child(
                ui::button("log-out", crate::tr("Log out"), Weight::Quiet)
                    .on_click(cx.listener(move |this, _, _, cx| this.log_out(host, &a, cx))),
            );
        }
        if installed.is_some() && !busy && !matches!(stage, Stage::Installing(_)) {
            actions = actions.child(
                ui::button("uninstall", crate::tr("Uninstall"), Weight::Quiet).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.uninstall(host, &uninstall_id, window, cx);
                    }),
                ),
            );
        }
        let toggle_weight = if enabled || !ready {
            Weight::Quiet
        } else {
            Weight::Primary
        };
        actions = actions.child(div().flex_1()).child(
            ui::button(
                "toggle",
                crate::tr(if enabled { "Turn off" } else { "Turn on" }),
                toggle_weight,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(host, &toggle_id, cx))),
        );

        let trouble = match &stage {
            Stage::Failed(why) => Some(why.clone()),
            _ => None,
        };
        page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .w(px(PAGE))
                .pt(px(72.))
                .pb(px(40.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(ui::mark(faces, &self.mark_of(id), 48.))
                        .child(
                            ui::text(faces, headline)
                                .text_size(px(crate::style::H1))
                                .line_height(px(30.))
                                .weight(gpui::FontWeight::SEMIBOLD),
                        )
                        .child(
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "u0 runs %{name}'s own agent and shows its work here. Its account, models and billing stay with %{name}; u0 never sees its sign-in.",
                                    &named,
                                ),
                            )
                            .text_size(px(crate::style::BODY))
                            .line_height(px(21.))
                            .text_color(theme::text2()),
                        ),
                )
                .child(steps)
                .children(trouble.map(ui::trouble))
                .child(actions)
                .child(self.agent_own(faces, host, id, &stage, cx)),
        )
        .overflow_y_scrollbar()
    }

    /// What you set for an agent: a command line of your own that starts
    /// it, and model ids of your own beside those it lists; and a warning
    /// where it speaks another version of ACP than u0.
    fn agent_own(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        stage: &Stage,
        cx: &mut Context<Self>,
    ) -> Div {
        let field = |input: &Entity<InputState>| div().flex_1().child(Input::new(input).small());
        let (a, b, c) = (id.to_owned(), id.to_owned(), id.to_owned());
        let tools = !self.settings_on(host).no_tools.iter().any(|t| t == id);
        let mut own = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(ui::label(faces, &crate::tr("u0's tools")).flex_1())
                    .child(
                        gpui::component::switch::Switch::new("agent-tools")
                            .accessibility_label(crate::tr("u0's tools"))
                            .small()
                            .checked(tools)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let off = &mut this.settings_on_mut(host).no_tools;
                                match off.iter().position(|t| *t == c) {
                                    Some(at) => drop(off.remove(at)),
                                    None => off.push(c.clone()),
                                }
                                this.save_settings_on(host);
                                cx.notify();
                            })),
                    ),
            )
            .child(ui::fine(
                faces,
                crate::tr("Each new thread of it is given u0's tools, as an MCP server: the threads u0 keeps, a diff, a task handed to another thread, a pull request, a task scheduled, a question put to you. What acts asks you here first."),
            ))
            .child(ui::label(faces, &crate::tr("Your own")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&self.pages.command))
                    .child(
                        ui::button("save-command", crate::tr("Use this command"), Weight::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| this.save_command(host, &a, cx))),
                    ),
            )
            .child(ui::fine(
                faces,
                match self.settings_on(host).commands.get(id) {
                    Some(c) => crate::tr_with("Started with `%{command}`; clear it to go back to the registry's way.", &[("command", c)]),
                    None => crate::tr("A command of yours starts it instead of the registry's way (your own build, a version manager's): it must speak ACP on its standard input and output."),
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&self.pages.model))
                    .child(
                        ui::button("add-model", crate::tr("Add a model"), Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_model(host, &b, window, cx))),
                    ),
            );
        let mut models = div().flex().flex_wrap().gap(px(6.));
        for (n, m) in self
            .settings
            .models
            .get(id)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let (agent, model) = (id.to_owned(), m.clone());
            models = models.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(2.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(theme::tag_line())
                    .child(ui::mono(faces, m.clone()).text_size(px(crate::style::CAPTION)))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("drop-model-{n}")),
                            "x",
                            crate::tr("Take it out"),
                        )
                        .size(px(18.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.drop_model(host, &agent, &model, cx);
                        })),
                    ),
            );
        }
        own = own.child(models).child(ui::fine(
            faces,
            crate::tr("Model ids of yours are offered in the composer beside those the agent lists; the agent may refuse one it does not know."),
        ));
        own = own.child(self.agent_vars(faces, host, id, cx));
        own = own.child(self.agent_accounts(faces, host, id, cx));
        // The version of ACP it speaks, where it is not u0's.
        if let Stage::Running { .. } = stage
            && let Some(init) = self.slot(host, id).and_then(|s| s.known.as_ref())
            && init.protocol_version != agent_client_protocol::schema::ProtocolVersion::V1
        {
            own = own.child(ui::trouble(crate::tr_with(
                "It speaks ACP version %{version}; u0 speaks version 1. Some of what it says may not show.",
                &[("version", &format!("{:?}", init.protocol_version))],
            )));
        }
        own
    }

    /// The accounts of the agent `id`: each signs in apart, its CLI's
    /// settings in a folder of its own; a line to add one. None where
    /// u0 does not know where its CLI keeps them.
    fn agent_accounts(&self, faces: &Faces, host: Host, id: &str, cx: &mut Context<Self>) -> Div {
        let agent = crate::registry::instance(id).0.to_owned();
        if crate::registry::known(&agent).and_then(|k| k.4).is_none() {
            return div();
        }
        let mut list = card().overflow_hidden();
        let accounts: Vec<&String> = (self.settings_on(host).enabled.iter())
            .filter(|e| crate::registry::instance(e).0 == agent && **e != agent)
            .collect();
        for (n, account) in accounts.iter().enumerate() {
            let (open, drop) = ((*account).clone(), (*account).clone());
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(6.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::mark(faces, &self.mark_of(account), 16.))
                    .child(
                        div()
                            .id(SharedString::from(format!("account-{n}")))
                            .flex_1()
                            .cursor_pointer()
                            .child(
                                ui::text(faces, self.name_of(account))
                                    .text_size(px(crate::style::LABEL)),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_page(
                                    crate::desk::Page::Agent(host, open.clone()),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(ui::fine(
                        faces,
                        match self.slot(host, account).map(|s| &s.auth) {
                            Some(crate::hub::Auth::Ready) => crate::tr("signed in"),
                            Some(crate::hub::Auth::Needed(_)) => crate::tr("needs sign-in"),
                            _ => SharedString::default(),
                        },
                    ))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("drop-account-{n}")),
                            "x",
                            crate::tr("Take it away"),
                        )
                        .size(px(22.))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.drop_account(host, &drop, window, cx);
                            },
                        )),
                    ),
            );
        }
        let base = agent.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(8.))
            .child(ui::label(faces, &crate::tr("Instances")))
            .when(!accounts.is_empty(), |d| d.child(list))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(180.)).child(Input::new(&self.pages.account).small()))
                    .child(
                        ui::button("add-account", crate::tr("Add an instance"), Weight::Secondary).on_click(
                            cx.listener(move |this, _, window, cx| this.add_account(host, &base, window, cx)),
                        ),
                    ),
            )
            .child(ui::fine(
                faces,
                crate::tr("Another instance is the agent run again with its own sign-in and settings, side by side with this one: a thread begun in it stays with it."),
            ))
    }

    /// The variables the agent `id` starts with, a secret one's value
    /// hidden (it is in the Keychain, not here); and a line to add one.
    fn agent_vars(&self, faces: &Faces, host: Host, id: &str, cx: &mut Context<Self>) -> Div {
        let mut list = card().overflow_hidden();
        for (n, (name, value)) in self
            .settings_on(host)
            .env
            .get(id)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let (agent, var) = (id.to_owned(), name.clone());
            list =
                list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(6.))
                        .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                        .child(ui::mono(faces, name.clone()).text_size(px(crate::style::CAPTION)))
                        .child(
                            ui::one_line(ui::mono(
                                faces,
                                value.clone().unwrap_or_else(|| "••••••••".into()),
                            ))
                            .flex_1()
                            .text_color(theme::muted()),
                        )
                        .child(ui::fine(
                            faces,
                            crate::tr(if value.is_some() {
                                "Plain text"
                            } else {
                                "In the Keychain"
                            }),
                        ))
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("drop-var-{n}")),
                                "x",
                                crate::tr("Take it out"),
                            )
                            .size(px(22.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.drop_var(host, &agent, &var, cx),
                            )),
                        ),
                );
        }
        let agent = id.to_owned();
        let secret = self.pages.var_secret;
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(8.))
            .child(ui::label(faces, &crate::tr("Variables")))
            .when(self.settings_on(host).env.contains_key(id), |d| d.child(list))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(180.)).child(Input::new(&self.pages.var_name).small()))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.pages.var_value).small().mask_toggle()),
                    )
                    .child(
                        gpui::component::switch::Switch::new("var-secret")
                            .accessibility_label(crate::tr("Secret"))
                            .small()
                            .checked(secret)
                            .label(crate::tr("Secret"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pages.var_secret = !this.pages.var_secret;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::button("add-var", crate::tr("Add"), Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_var(host, &agent, window, cx))),
                    ),
            )
            .child(ui::fine(
                faces,
                crate::tr("Set in the agent's environment as it starts. A secret one is kept in the macOS Keychain, never in u0's files; a value may be a 1Password reference, read by op as it starts."),
            ))
    }
}
