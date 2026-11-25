//! The pages that take the middle in place of a thread, as the design has
//! them: an agent's own, which gets it ready to run in three steps, and the
//! settings (`settings`), where agents are switched on and off.

mod new_project;
mod prefs;
mod settings;

pub use settings::Tab;

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::{Slot, Stage},
    registry::{Missing, Way},
    session::State,
    theme,
    ui::{self, Mark, Weight},
};

/// The width of a page's column.
const PAGE: f32 = 560.;

/// Where a step of getting an agent ready stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Todo,
    Now,
    Done,
}

fn step_number(faces: &Faces, n: u8, step: Step) -> Div {
    let (line, color) = match step {
        Step::Todo => (theme::border2(), theme::muted()),
        Step::Now => (theme::accent(), theme::accent()),
        Step::Done => (theme::ok(), theme::ok()),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded_full()
        .border_1()
        .border_color(line)
        .when(step == Step::Done, |d| d.bg(theme::ok().opacity(0.18)))
        .child(
            ui::text(faces, n.to_string())
                .text_size(px(11.))
                .weight(gpui::FontWeight::MEDIUM)
                .text_color(color),
        )
}

pub(crate) fn card() -> Div {
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
            .min_h(px(0.))
            .overflow_y_scroll();
        let Some(entry) = self.entry(id) else {
            return page
                .child(ui::text(faces, "The registry does not list this agent.").mt(px(72.)));
        };
        let enabled = self.settings.enabled.iter().any(|e| e == id);
        let stage = self.slot(id).map_or_else(
            || Slot::new(entry.clone(), &self.home, None).stage,
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
            .iter()
            .any(|s| s.agent == id && s.id.is_some());
        let signing = self
            .sessions
            .iter()
            .any(|s| s.agent == id && s.state == State::SignIn);

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
            .filter(|s| s.running().is_some())
            .and_then(Slot::caps)
            .is_some_and(|c| c.auth.logout.is_some());
        let three = match &auth {
            crate::hub::Auth::Needed(_) => Step::Now,
            crate::hub::Auth::Ready => Step::Done,
            crate::hub::Auth::Unknown if opened => Step::Done,
            crate::hub::Auth::Unknown if signing => Step::Now,
            crate::hub::Auth::Unknown => Step::Todo,
        };
        let installed = entry.installed(&self.home);
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
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded(px(2.))
                        .bg(theme::tag_line())
                        .overflow_hidden()
                        .child(div().h_full().w(gpui::relative(share)).bg(theme::accent()))
                }))
                .child(ui::fine(faces, install.what.clone()));
        } else if let Stage::Missing(m) = &stage
            && !matches!(m, Missing::Download)
        {
            download = download.child(ui::trouble(faces, m.say(&name)));
        } else {
            download = download.child(
                ui::mono(faces, Desk::shown(&entry.dir(&self.home)))
                    .text_size(px(11.5))
                    .text_color(theme::faint()),
            );
        }
        // The CLI it drives: yours, as your terminal has it.
        if let Some((_, cli)) = entry.your_cli() {
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
        let step_row = |n: u8, st: Step, body: Div| {
            div()
                .flex()
                .gap(px(14.))
                .px(px(16.))
                .py(px(14.))
                .when(n > 1, |d| d.border_t_1().border_color(theme::border()))
                .child(step_number(faces, n, st))
                // Its words wrap inside the card.
                .child(body.min_w(px(0.)))
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
        let steps = card()
            .child(step_row(1, one, download))
            .child(step_row(
                2,
                two,
                simple(
                    "Check that it speaks ACP".into(),
                    format!("Starts {command} once and reads its version and abilities."),
                    two,
                ),
            ))
            .child(step_row(
                3,
                three,
                simple(
                    format!("Sign in to {name}"),
                    match &auth {
                        crate::hub::Auth::Needed(why) => format!("{name} says: {why}"),
                        crate::hub::Auth::Ready => format!("Signed in: {name} opens sessions."),
                        crate::hub::Auth::Unknown => {
                            format!("{name}'s own sign-in. Skipped if you're already signed in.")
                        }
                    },
                    three,
                ),
            ));

        let (install_id, recheck_id, start_id, toggle_id) =
            (id.to_owned(), id.to_owned(), id.to_owned(), id.to_owned());
        let (cancel_id, update_id, uninstall_id) = (id.to_owned(), id.to_owned(), id.to_owned());
        let busy = self
            .sessions
            .iter()
            .any(|s| s.agent == id && matches!(s.state, State::Working | State::Opening));
        let mut actions = div().flex().items_center().gap(px(8.));
        match &stage {
            Stage::Installing(_) => {
                actions = actions.child(
                    ui::button(faces, "cancel-install", "Cancel", Weight::Secondary).on_click(
                        cx.listener(move |this, _, _, cx| this.cancel_install(&cancel_id, cx)),
                    ),
                );
            }
            Stage::Missing(Missing::Download) => {
                actions = actions.child(
                    ui::button(faces, "install", format!("Install {name}"), Weight::Primary)
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
                        ui::button(faces, "recheck", "Try again", Weight::Secondary).on_click(
                            cx.listener(move |this, _, _, cx| this.recheck(&recheck_id, cx)),
                        ),
                    )
                    .child(
                        ui::button(faces, "get-runner", label, Weight::Quiet)
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
                    );
            }
            Stage::Failed(_) => {
                actions = actions.child(
                    ui::button(faces, "recheck", "Try again", Weight::Secondary)
                        .on_click(cx.listener(move |this, _, _, cx| this.recheck(&recheck_id, cx))),
                );
            }
            _ if enabled => {
                actions = actions.child(
                    ui::button(faces, "start", "New session", Weight::Primary).on_click(
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
                    faces,
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
                    ui::button(faces, "page-sign-in", "Sign in", Weight::Primary)
                        .on_click(cx.listener(move |this, _, _, cx| this.sign_in_agent(&a, cx))),
                )
                .child(
                    ui::button(faces, "page-signed-in", "I've signed in", Weight::Quiet)
                        .on_click(cx.listener(move |this, _, _, cx| this.recheck_auth(&b, cx))),
                );
        } else if can_log_out && auth == crate::hub::Auth::Ready && !busy {
            let a = id.to_owned();
            actions = actions.child(
                ui::button(faces, "log-out", "Log out", Weight::Quiet)
                    .on_click(cx.listener(move |this, _, _, cx| this.log_out(&a, cx))),
            );
        }
        if installed.is_some() && !busy && !matches!(stage, Stage::Installing(_)) {
            actions = actions.child(
                ui::button(faces, "uninstall", "Uninstall", Weight::Quiet).on_click(cx.listener(
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
                faces,
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
                        .child(ui::mark(faces, &Mark::of(id, &name), 48.))
                        .child(
                            ui::text(faces, headline)
                                .text_size(px(22.))
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
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .text_color(theme::text2()),
                        ),
                )
                .child(steps)
                .children(trouble.map(|why| ui::trouble(faces, why)))
                .child(actions)
                .child(self.agent_own(faces, id, &stage, cx)),
        )
    }

    /// What you set for an agent: a command line of your own that starts
    /// it, and model ids of your own beside those it lists; and a warning
    /// where it speaks another version of ACP than Ghost.
    fn agent_own(&self, faces: &Faces, id: &str, stage: &Stage, cx: &mut Context<Self>) -> Div {
        let field = |input: gpui::Entity<crate::input::TextInput>| {
            div()
                .flex()
                .items_center()
                .flex_1()
                .h(px(32.))
                .px(px(8.))
                .rounded(px(8.))
                .bg(theme::well())
                .border_1()
                .border_color(theme::border2())
                .child(div().flex_1().child(input))
        };
        let (a, b) = (id.to_owned(), id.to_owned());
        let mut own = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(ui::label(faces, "Your own"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(self.agent_command.clone()))
                    .child(
                        ui::button(faces, "save-command", "Use this command", Weight::Secondary)
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
                    .child(field(self.agent_model.clone()))
                    .child(
                        ui::button(faces, "add-model", "Add a model", Weight::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| this.add_model(&b, cx))),
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
                    .child(ui::mono(faces, m.clone()).text_size(px(12.)))
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
        // The version of ACP it speaks, where it is not Ghost's.
        if let Stage::Running { .. } = stage
            && let Some(init) = self.slot(id).and_then(|s| s.known.as_ref())
            && init.protocol_version != agent_client_protocol::schema::ProtocolVersion::V1
        {
            own = own.child(ui::trouble(
                faces,
                format!(
                    "It speaks ACP version {:?}; Ghost speaks version 1. Some of what it says may not show.",
                    init.protocol_version
                ),
            ));
        }
        own
    }
}
