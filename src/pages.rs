//! The pages that take the middle in place of a thread, as the design has
//! them: an agent's own, which gets it ready to run in three steps, and the
//! settings, where agents are switched on and off.

use std::time::SystemTime;

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page},
    fonts::Faces,
    hub::{Slot, Stage},
    registry::{Missing, Way},
    session::State,
    sidebar::kbd,
    theme,
    ui::{self, Mark, Weight},
    when,
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

fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
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
            .min_h(px(0.))
            .overflow_y_scroll();
        let Some(entry) = self.entry(id) else {
            return page
                .child(ui::text(faces, "The registry does not list this agent.").mt(px(72.)));
        };
        let enabled = self.settings.enabled.iter().any(|e| e == id);
        let stage = self.slot(id).map_or_else(
            || Slot::new(entry.clone(), &self.home).stage,
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
            _ => Step::Todo,
        };
        let three = if opened {
            Step::Done
        } else if signing {
            Step::Now
        } else {
            Step::Todo
        };
        let installed = entry.installed(&self.home);
        let update = installed.as_ref().filter(|v| **v != entry.version);
        let first_step = match entry.way() {
            Way::Archive => match update {
                Some(v) => format!("{name} {v} installed · {} available", entry.version),
                None => format!("Download {name} {}", entry.version),
            },
            Way::Runner { runner, .. } => format!("Fetched by {runner} on first start"),
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
                .child(fine(
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
                .child(fine(faces, install.what.clone()));
        } else if let Stage::Missing(m) = &stage
            && !matches!(m, Missing::Download)
        {
            download = download.child(fine(faces, m.say(&name)).text_color(theme::coral()));
        } else {
            download = download.child(
                ui::mono(faces, Desk::shown(&entry.dir(&self.home)))
                    .text_size(px(11.5))
                    .text_color(theme::faint()),
            );
        }
        let step_row = |n: u8, st: Step, body: Div| {
            div()
                .flex()
                .gap(px(14.))
                .px(px(16.))
                .py(px(14.))
                .when(n > 1, |d| d.border_t_1().border_color(theme::border()))
                .child(step_number(faces, n, st))
                .child(body)
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
                .child(fine(faces, text))
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
                    format!("{name}'s own sign-in. Skipped if you're already signed in."),
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
                            this.new_session(&start_id, this.project(), window, cx);
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
                .children(trouble.map(|why| {
                    crate::thread::well(faces)
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(px(12.))
                        .text_color(theme::coral())
                        .child(why)
                }))
                .child(actions),
        )
    }

    pub(crate) fn settings_page(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(220.))
            .gap(px(2.))
            .p(px(8.))
            .border_r_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .bg(theme::panel2())
                    .child(ui::text(faces, "Agents").text_size(px(13.5))),
            );

        let mut list = card().overflow_hidden();
        let mut agents: Vec<_> = self.registry.agents.iter().collect();
        agents.sort_by_key(|e| {
            (
                self.settings
                    .enabled
                    .iter()
                    .position(|id| id == &e.id)
                    .unwrap_or(usize::MAX),
                e.name.to_lowercase(),
            )
        });
        for (n, e) in agents.into_iter().enumerate() {
            let on = self.settings.enabled.contains(&e.id);
            let installed = e.launch(&self.home).is_ok();
            let name = self.name_of(&e.id);
            let how = match e.way() {
                Way::Archive => "binary",
                Way::Runner { runner, .. } => runner,
                Way::None => "unavailable",
            };
            let line = match (installed, on, e.way()) {
                (true, true, _) => format!("{} {} · {how}", e.id, e.version),
                (true, false, Way::Archive) => "installed · off".to_owned(),
                (true, false, _) => format!("runs with {how} · off"),
                (false, _, _) => "not installed".to_owned(),
            };
            let (id, page_id) = (e.id.clone(), e.id.clone());
            let control = if installed || on {
                ui::switch(SharedString::from(format!("toggle-{id}")), on)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(&id, cx)))
                    .into_any_element()
            } else {
                ui::button(
                    faces,
                    SharedString::from(format!("get-{id}")),
                    "Install",
                    Weight::Secondary,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.open_page(Page::Agent(id.clone()), cx)),
                )
                .into_any_element()
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(12.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(div().when(!on, |d| d.opacity(0.45)).child(ui::mark(
                        faces,
                        &Mark::of(&e.id, &name),
                        24.,
                    )))
                    .child(
                        div()
                            .id(SharedString::from(format!("aname-{n}")))
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .cursor_pointer()
                            .child(ui::text(faces, name).weight(gpui::FontWeight::MEDIUM))
                            .child(fine(faces, line))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_page(Page::Agent(page_id.clone()), cx);
                            })),
                    )
                    .child(control),
            );
        }
        let default = self
            .slots
            .first()
            .map(|s| (s.entry.id.clone(), self.name_of(&s.entry.id)));
        let read = self.registry_read.map_or_else(
            || "Registry not read yet".to_owned(),
            |t| match when::ago(t, SystemTime::now()).as_str() {
                "now" => "Registry checked just now".to_owned(),
                ago => format!("Registry checked {ago} ago"),
            },
        );
        let content = div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .px(px(40.))
            .py(px(28.))
            .max_w(px(720.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        ui::text(faces, "Agents")
                            .text_size(px(20.))
                            .line_height(px(28.))
                            .weight(gpui::FontWeight::SEMIBOLD),
                    )
                    .child(
                        ui::text(
                            faces,
                            "Turn on the agents you use. Each runs its own CLI over ACP, with its own sign-in.",
                        )
                        .text_size(px(13.5))
                        .text_color(theme::text2()),
                    ),
            )
            .children(default.map(|(id, name)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        ui::text(faces, "Default for new sessions")
                            .flex_1()
                            .text_size(px(13.5)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(26.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(theme::tag_line())
                            .child(ui::mark(faces, &Mark::of(&id, &name), 14.))
                            .child(
                                ui::text(faces, name)
                                    .text_size(px(12.5))
                                    .text_color(theme::text2()),
                            ),
                    )
                    .child(kbd(faces, "⌘N"))
            }))
            .children(
                self.settings_refused
                    .clone()
                    .map(|why| fine(faces, why).text_color(theme::coral())),
            )
            .children(
                self.registry_trouble
                    .clone()
                    .map(|why| fine(faces, why).text_color(theme::coral())),
            )
            .when(
                self.registry.agents.is_empty() && self.registry_trouble.is_none(),
                |d| d.child(fine(faces, "Reading the registry…")),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        ui::button(faces, "refresh", "Read the registry again", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx))),
                    )
                    .child(div().flex_1())
                    .child(fine(faces, read)),
            )
            .child(fine(
                faces,
                format!(
                    "Ghost keeps its settings and the agents it installs in {}.",
                    Desk::shown(&self.home)
                ),
            ));
        div().flex().flex_1().min_h(px(0.)).child(nav).child(
            div()
                .id("settings")
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .overflow_y_scroll()
                .child(content),
        )
    }
}
