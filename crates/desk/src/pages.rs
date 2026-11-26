//! The pages that take the middle in place of a thread, as the design has
//! them: an agent's own, which gets it ready to run in three steps; and
//! the settings (`settings`), a dialog over the
//! window, where agents are switched on and off.

mod devices;
mod diagnostics;
mod flows;
mod forge;
mod hosts;
mod looks;
mod prefs;
mod projects;
mod scope;
mod settings;
mod tasks;
mod updates;
mod usage;
mod voice;
mod writing;

pub(crate) use diagnostics::process_tree;
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
    hub::{Slot, Stage},
    registry::{Missing, Way},
    session::State,
    settings::Settings,
    theme,
    ui::{self, Weight},
};

/// What the pages hold as you use them: whether the settings are open,
/// their tab, what is looked for among the settings and among the
/// registry's agents, what worktree branches begin with as you write it,
/// the action whose keys are being recorded, how much each part of Ghost's
/// home takes and what the last tidying did; on an agent's page, the
/// command of yours that starts it and a model id to add.
pub struct Pages {
    pub open: bool,
    pub tab: Tab,
    /// The settings' page shown and their search: gpui-kit's.
    pub settings: Entity<SettingsState>,
    pub agent_search: Entity<InputState>,
    pub prefix: Entity<InputState>,
    /// Your snooze presets as you write them, commas between.
    pub snoozes: Entity<InputState>,
    pub recording: Option<&'static str>,
    pub sizes: Vec<(String, u64)>,
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
    pub project: Option<std::path::PathBuf>,
    /// Your instructions to the writer, by what it writes.
    pub changes: Entity<TextareaState>,
    pub titles: Entity<TextareaState>,
    pub branches: Entity<TextareaState>,
    pub transcripts: Entity<TextareaState>,
    /// Your dictation's dictionary, a word or "heard = written" to a line.
    pub dictionary: Entity<TextareaState>,
    /// The days the usage page reads over; none, all.
    pub period: Option<u64>,
    /// The project the settings a project may hold are set for, as
    /// t3code's "Applying settings for"; none, everywhere.
    pub scope: Option<std::path::PathBuf>,
    /// What is known of each forge here, and a token of yours for each,
    /// as it is typed (by `forge::Host::ALL`).
    pub forges: Vec<crate::forge::Seen>,
    pub tokens: [Entity<InputState>; 5],
    /// Themes of yours that did not read, each said.
    pub themes_refused: Vec<String>,
    /// What Ghost runs (the agents among it, by id and process id), and
    /// the warnings and errors in its log: Settings › Diagnostics.
    pub processes: (Vec<(String, u32)>, crate::diagnostics::Processes),
    /// Those processes as gpui-kit's tree.
    pub process_tree: Entity<gpui::component::tree::TreeState>,
    pub troubles: Vec<String>,
    /// How sharing this Ghost stands, as its server last said.
    pub shared: Option<crate::share::Shared>,
    /// The named tunnel's address and token as you write them.
    pub named_host: Entity<InputState>,
    pub named_token: Entity<InputState>,
    /// The servers paired with, the link pasted to pair with another, and
    /// whether that is under way.
    pub servers: crate::servers::Servers,
    pub pair_link: Entity<InputState>,
    pub pairing: bool,
}

impl Pages {
    pub fn new(settings: &Settings, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let field = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Desk>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let shown = cx.new(|cx| SettingsState::new(window, cx));
        let agent_search = field("Search the ACP registry", window, cx);
        cx.subscribe(&agent_search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let prefix = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("ghost/")
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
                .placeholder("1h, tomorrow")
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
            agent_search,
            prefix,
            snoozes,
            recording: None,
            sizes: Vec::new(),
            tidied: None,
            command: field("/path/to/agent --acp", window, cx),
            model: field("a model id", window, cx),
            var_name: field("NAME", window, cx),
            account: field("Work", window, cx),
            var_value: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("value, or op://vault/item/field")
                    .masked(true)
            }),
            var_secret: true,
            project: None,
            period: Some(30),
            scope: None,
            forges: Vec::new(),
            themes_refused: Vec::new(),
            processes: Default::default(),
            process_tree: cx.new(|cx| gpui::component::tree::TreeState::new(cx)),
            troubles: Vec::new(),

            tokens: crate::forge::Host::ALL.map(|_| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("a token")
                        .masked(true)
                })
            }),
            shared: None,
            named_host: field("ghost.example.com", window, cx),
            named_token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Tunnel token")
                    .masked(true)
            }),
            servers: crate::servers::Servers::default(),
            pair_link: field("ghost://pair?host=…", window, cx),
            pairing: false,
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
            .placeholder(placeholder)
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
            .placeholder("Ghost\ngpui kit = gpui-kit")
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
                .child(ui::text(faces, "The registry does not list this agent.").mt(px(72.)))
                .overflow_y_scrollbar();
        };
        let enabled = self.settings.enabled.iter().any(|e| e == id);
        let stage = self.slot(id).map_or_else(
            || Slot::new(entry.clone(), self.here.get(id), None).stage,
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
            .any(|s| s.meta.agent == id && s.meta.id.is_some());
        let signing = self
            .sessions
            .all
            .iter()
            .any(|s| s.meta.agent == id && s.state == State::SignIn);

        let (headline, ready) = match &stage {
            Stage::Missing(_) => (format!("{name} isn't on this Mac yet"), false),
            Stage::Installing(_) => (format!("Installing {name}"), false),
            Stage::Failed(_) => (format!("{name} stopped"), true),
            _ => (format!("{name} is ready"), true),
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
            Stage::Ready if self.slot(id).is_some_and(|s| s.known.is_some()) => Step::Done,
            _ => Step::Todo,
        };
        let auth = self
            .slot(id)
            .map_or(crate::hub::Auth::Unknown, |s| s.auth.clone());
        let can_log_out = self
            .slot(id)
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
        let installed = self.here.get(&entry.id).and_then(|h| h.installed.clone());
        let update = installed.as_ref().filter(|v| **v != entry.version);
        let first_step = match entry.way() {
            Way::Archive => match update {
                Some(v) => format!("{name} {v} installed · {} available", entry.version),
                None => format!("Download {name} {}", entry.version),
            },
            Way::Runner { runner, .. } => match &installed {
                Some(v) if *v == entry.version => format!("{name} {v} installed in Ghost"),
                Some(v) => format!("{name} {v} installed · {} available", entry.version),
                None => format!(
                    "Install {name} {} with {}",
                    entry.version,
                    if runner == "npx" { "npm" } else { "uv" }
                ),
            },
            Way::None => "No build for this Mac".into(),
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
                    format!("{} · from the ACP registry", crate::registry::platform()),
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
                ui::mono(faces, Desk::shown(&entry.dir(&self.home)))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::faint()),
            );
        }
        // The CLI it drives: yours, as your terminal has it.
        if let Some(cli) = self.here.get(&entry.id).and_then(|h| h.cli.clone()) {
            let version = self.cli_versions.get(&cli).cloned().unwrap_or_default();
            download = download.child(ui::fine(
                faces,
                format!(
                    "Runs your {} {version} · {}",
                    cli.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default(),
                    Desk::shown(&cli)
                ),
            ));
        }
        // A step done is checked; the first not done is where you are.
        let step = |st: Step, body: Div| {
            StepperItem::new()
                .when(st == Step::Done, |i| i.icon(IconName::Check))
                .child(body.min_w(px(0.)).pb(px(14.)))
        };
        let simple = |title: String, text: String, st: Step| {
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
                        "Check that it speaks ACP".into(),
                        format!("Starts {command} once and reads its version and abilities."),
                        two,
                    ),
                ))
                .item(step(
                    three,
                    simple(
                        format!("Sign in to {name}"),
                        match &auth {
                            crate::hub::Auth::Needed(why) => format!("{name} says: {why}"),
                            crate::hub::Auth::Ready => format!("Signed in: {name} opens sessions."),
                            crate::hub::Auth::Unknown => {
                                format!(
                                    "{name}'s own sign-in. Skipped if you're already signed in."
                                )
                            }
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
                    ui::button("cancel-install", "Cancel", Weight::Secondary).on_click(
                        cx.listener(move |this, _, _, cx| this.cancel_install(&cancel_id, cx)),
                    ),
                );
            }
            Stage::Missing(Missing::Download) => {
                actions = actions.child(
                    ui::button("install", format!("Install {name}"), Weight::Primary)
                        .on_click(cx.listener(move |this, _, _, cx| this.install(&install_id, cx))),
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
                        ui::button("recheck", "Try again", Weight::Secondary).on_click(
                            cx.listener(move |this, _, _, cx| this.recheck(&recheck_id, cx)),
                        ),
                    )
                    .child(
                        ui::button("get-runner", label, Weight::Quiet)
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
                    );
            }
            Stage::Failed(_) => {
                actions = actions.child(
                    ui::button("recheck", "Try again", Weight::Secondary)
                        .on_click(cx.listener(move |this, _, _, cx| this.recheck(&recheck_id, cx))),
                );
            }
            _ if enabled => {
                actions = actions.child(
                    ui::button("start", "New session", Weight::Primary).on_click(cx.listener(
                        move |this, _, window, cx| {
                            if let Some(project) = this.project() {
                                this.new_session(&start_id, project, window, cx);
                            }
                        },
                    )),
                );
            }
            _ => {}
        }
        if update.is_some() && enabled && !busy && !matches!(stage, Stage::Installing(_)) {
            actions = actions.child(
                ui::button(
                    "update",
                    format!("Update to {}", entry.version),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.install(&update_id, cx))),
            );
        }
        if matches!(auth, crate::hub::Auth::Needed(_)) {
            let (a, b) = (id.to_owned(), id.to_owned());
            actions = actions
                .child(
                    ui::button("page-sign-in", "Sign in", Weight::Primary)
                        .on_click(cx.listener(move |this, _, _, cx| this.sign_in_agent(&a, cx))),
                )
                .child(
                    ui::button("page-signed-in", "I've signed in", Weight::Quiet)
                        .on_click(cx.listener(move |this, _, _, cx| this.recheck_auth(&b, cx))),
                );
        } else if can_log_out && auth == crate::hub::Auth::Ready && !busy {
            let a = id.to_owned();
            actions = actions.child(
                ui::button("log-out", "Log out", Weight::Quiet)
                    .on_click(cx.listener(move |this, _, _, cx| this.log_out(&a, cx))),
            );
        }
        if installed.is_some() && !busy && !matches!(stage, Stage::Installing(_)) {
            actions = actions.child(
                ui::button("uninstall", "Uninstall", Weight::Quiet).on_click(cx.listener(
                    move |this, _, window, cx| this.uninstall(&uninstall_id, window, cx),
                )),
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
                if enabled { "Turn off" } else { "Turn on" },
                toggle_weight,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(&toggle_id, cx))),
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
                                format!(
                                    "Ghost runs {name}'s own agent and shows its work here. Its account, models and billing stay with {name}; Ghost never sees its sign-in."
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
                .child(self.agent_own(faces, id, &stage, cx)),
        )
        .overflow_y_scrollbar()
    }

    /// What you set for an agent: a command line of your own that starts
    /// it, and model ids of your own beside those it lists; and a warning
    /// where it speaks another version of ACP than Ghost.
    fn agent_own(&self, faces: &Faces, id: &str, stage: &Stage, cx: &mut Context<Self>) -> Div {
        let field = |input: &Entity<InputState>| div().flex_1().child(Input::new(input).small());
        let (a, b, c) = (id.to_owned(), id.to_owned(), id.to_owned());
        let tools = !self.settings.no_tools.iter().any(|t| t == id);
        let mut own = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(ui::label(faces, "Ghost's tools").flex_1())
                    .child(
                        gpui::component::switch::Switch::new("agent-tools")
                            .small()
                            .checked(tools)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let off = &mut this.settings.no_tools;
                                match off.iter().position(|t| *t == c) {
                                    Some(at) => drop(off.remove(at)),
                                    None => off.push(c.clone()),
                                }
                                this.save_settings();
                                cx.notify();
                            })),
                    ),
            )
            .child(ui::fine(
                faces,
                "Each new session of it is given Ghost's tools, as an MCP server: the sessions Ghost keeps, a diff, a task handed to another session, a pull request, a task scheduled, a question put to you. What acts asks you here first.",
            ))
            .child(ui::label(faces, "Your own"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&self.pages.command))
                    .child(
                        ui::button("save-command", "Use this command", Weight::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| this.save_command(&a, cx))),
                    ),
            )
            .child(ui::fine(
                faces,
                match self.settings.commands.get(id) {
                    Some(c) => format!("Started with `{c}`; clear it to go back to the registry's way."),
                    None => "A command of yours starts it instead of the registry's way (your own build, a version manager's): it must speak ACP on its standard input and output.".to_owned(),
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&self.pages.model))
                    .child(
                        ui::button("add-model", "Add a model", Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_model(&b, window, cx))),
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
                            "Take it out",
                        )
                        .size(px(18.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.drop_model(&agent, &model, cx)),
                        ),
                    ),
            );
        }
        own = own.child(models).child(ui::fine(
            faces,
            "Model ids of yours are offered in the composer beside those the agent lists; the agent may refuse one it does not know.",
        ));
        own = own.child(self.agent_vars(faces, id, cx));
        own = own.child(self.agent_accounts(faces, id, cx));
        // The version of ACP it speaks, where it is not Ghost's.
        if let Stage::Running { .. } = stage
            && let Some(init) = self.slot(id).and_then(|s| s.known.as_ref())
            && init.protocol_version != agent_client_protocol::schema::ProtocolVersion::V1
        {
            own = own.child(ui::trouble(format!(
                    "It speaks ACP version {:?}; Ghost speaks version 1. Some of what it says may not show.",
                    init.protocol_version
                ),
            ));
        }
        own
    }

    /// The accounts of the agent `id`: each signs in apart, its CLI's
    /// settings in a folder of its own; a line to add one. None where
    /// Ghost does not know where its CLI keeps them.
    fn agent_accounts(&self, faces: &Faces, id: &str, cx: &mut Context<Self>) -> Div {
        let agent = crate::registry::instance(id).0.to_owned();
        if crate::registry::known(&agent).and_then(|k| k.4).is_none() {
            return div();
        }
        let mut list = card().overflow_hidden();
        let accounts: Vec<&String> = (self.settings.enabled.iter())
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
                                this.open_page(crate::desk::Page::Agent(open.clone()), window, cx);
                            })),
                    )
                    .child(ui::fine(
                        faces,
                        match self.slot(account).map(|s| &s.auth) {
                            Some(crate::hub::Auth::Ready) => "signed in",
                            Some(crate::hub::Auth::Needed(_)) => "needs sign-in",
                            _ => "",
                        },
                    ))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("drop-account-{n}")),
                            "x",
                            "Take it away",
                        )
                        .size(px(22.))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.drop_account(&drop, window, cx);
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
            .child(ui::label(faces, "Accounts"))
            .when(!accounts.is_empty(), |d| d.child(list))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(180.)).child(Input::new(&self.pages.account).small()))
                    .child(
                        ui::button("add-account", "Add an account", Weight::Secondary).on_click(
                            cx.listener(move |this, _, window, cx| this.add_account(&base, window, cx)),
                        ),
                    ),
            )
            .child(ui::fine(
                faces,
                "Another account is the agent run again with its own sign-in and settings, side by side with this one: a session begun in it stays with it.",
            ))
    }

    /// The variables the agent `id` starts with, a secret one's value
    /// hidden (it is in the Keychain, not here); and a line to add one.
    fn agent_vars(&self, faces: &Faces, id: &str, cx: &mut Context<Self>) -> Div {
        let mut list = card().overflow_hidden();
        for (n, (name, value)) in self.settings.env.get(id).into_iter().flatten().enumerate() {
            let (agent, var) = (id.to_owned(), name.clone());
            list = list.child(
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
                        if value.is_some() {
                            "Plain text"
                        } else {
                            "In the Keychain"
                        },
                    ))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("drop-var-{n}")),
                            "x",
                            "Take it out",
                        )
                        .size(px(22.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.drop_var(&agent, &var, cx)),
                        ),
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
            .child(ui::label(faces, "Variables"))
            .when(self.settings.env.contains_key(id), |d| d.child(list))
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
                            .small()
                            .checked(secret)
                            .label("Secret")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pages.var_secret = !this.pages.var_secret;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::button("add-var", "Add", Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_var(&agent, window, cx))),
                    ),
            )
            .child(ui::fine(
                faces,
                "Set in the agent's environment as it starts. A secret one is kept in the macOS Keychain, never in Ghost's files; a value may be a 1Password reference, read by op as it starts.",
            ))
    }
}
