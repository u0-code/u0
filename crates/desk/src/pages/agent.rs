//! An agent's page in the settings (`SetAgent` board): its head (its mark,
//! its name and how it stands, where it comes from, what can be done to
//! it), what wants you, then its instances, its variables, u0's tools, a
//! command of yours and models of yours, each a Carbon `SettingsSection`
//! in the settings' 720px column.

use gpui::component::{Sizable as _, input::Input, progress::Progress};
use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};
use u0ui::{
    components::{
        alert::{Alert, AlertAction, AlertVariant},
        badge::{Badge, BadgeSize, BadgeVariant},
        button::{Button, ButtonSize, ButtonVariant},
        checkbox::Checkbox,
    },
    icons::CarbonIcon,
    t3::workspace::settings::SettingsSection,
    tokens::ColorRole,
};

use super::srow::field;
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::{Auth, Host, Slot, Stage},
    registry::{Missing, Way},
    session::State,
    theme, ui,
};

/// A Carbon `sm` button of `variant`.
fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
) -> Button {
    Button::new(id.into(), label)
        .variant(variant)
        .size(ButtonSize::Sm)
}

/// A row of a section drawn here (`.ag-row`): what it is on the left, its
/// buttons on the right. `SRow` holds one control; these hold several.
fn row(title: Div, about: Option<Div>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(56.))
        .py(px(8.))
        .px(px(20.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(title)
                .children(about),
        )
}

/// A section's foot (`.ag-foot`): a line to add one more.
fn foot() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(12.))
        .px(px(20.))
}

/// "Managed by" its organisation, at a section's head, where a policy locks `key`.
fn managed(faces: &Faces, key: &str) -> Option<Div> {
    crate::policy::locked(key).map(|by| {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                u0ui::icons::Icon::carbon(CarbonIcon::Locked)
                    .size_length(px(14.))
                    .color(theme::role(ColorRole::IconSecondary)),
            )
            .child(ui::fine(
                faces,
                crate::tr_with("Managed by %{by}", &[("by", &by)]),
            ))
    })
}

impl Desk {
    /// Whether what the settings key `key` holds can't be changed here: a
    /// policy locks it, the settings file did not read, or the server they
    /// are of is out of reach.
    fn held(&self, key: &str) -> bool {
        crate::policy::locked(key).is_some() || self.edited_unreadable() || self.edited_offline()
    }

    /// The agent `id` on `host`, as Settings › Agents › its name shows it.
    pub(super) fn agent_tab(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let column = div()
            .w_full()
            .max_w(px(720.))
            .mx_auto()
            .flex()
            .flex_col()
            .gap(px(28.));
        let column = match self.entry(id) {
            None => column.child(ui::text(
                faces,
                crate::tr("The registry does not list this agent."),
            )),
            Some(_) => column
                .child(self.agent_head(faces, host, id, cx))
                .children(self.agent_alerts(host, id, cx))
                .child(self.agent_instances(faces, host, id, cx))
                .child(self.agent_vars(faces, host, id, cx))
                .child(self.agent_tools(host, id, cx))
                .child(self.agent_command(faces, host, id, cx))
                .child(self.agent_models(faces, host, id, cx)),
        };
        div()
            .id("agent-page")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pt(px(24.))
            .px(px(32.))
            .pb(px(48.))
            .child(column)
            .into_any_element()
    }

    /// How the agent `id` on `host` stands, its slot's or, switched off,
    /// as it would.
    fn stage_of(&self, host: Host, id: &str) -> Option<Stage> {
        let entry = self.entry(id)?;
        Some(self.slot(host, id).map_or_else(
            || Slot::new(host, entry.clone(), self.here_on(host, id), None).stage,
            |s| s.stage.clone(),
        ))
    }

    /// Its mark, its name and how it stands, where it comes from and the
    /// CLI it drives, and what can be done to it (`.ag-head`).
    fn agent_head(&self, faces: &Faces, host: Host, id: &str, cx: &mut Context<Self>) -> Div {
        let entry = self.entry(id).expect("an agent the registry lists");
        let stage = self.stage_of(host, id).unwrap_or(Stage::Ready);
        let name = self.name_of(id);
        let named = [("name", &name as &dyn std::fmt::Display)];
        let enabled = self.settings_on(host).enabled.iter().any(|e| e == id);
        let auth = self
            .slot(host, id)
            .map_or(Auth::Unknown, |s| s.auth.clone());
        let installed = (self.here_on(host, id)).and_then(|h| h.installed.clone());
        let update = installed.as_ref().filter(|v| **v != entry.version);
        let busy = self
            .sessions
            .all
            .iter()
            .any(|s| s.meta.agent == id && matches!(s.state, State::Working | State::Opening));

        let (word, tone) = match &stage {
            Stage::Missing(_) => ("Not installed", BadgeVariant::WarmGray),
            Stage::Installing(_) => ("Installing", BadgeVariant::Blue),
            Stage::Failed(_) => ("Stopped", BadgeVariant::Red),
            _ if !enabled => ("Off", BadgeVariant::Gray),
            _ if matches!(auth, Auth::Needed(_)) => ("Needs sign-in", BadgeVariant::WarmGray),
            Stage::Running { .. } => ("Running", BadgeVariant::Green),
            _ => ("Ready", BadgeVariant::Green),
        };

        // What it is: the adapter the registry gives, and your CLI it drives.
        let what = match entry.way() {
            Way::Archive => entry.distribution.binary[&crate::registry::platform()]
                .cmd
                .trim_start_matches("./")
                .to_owned(),
            Way::Runner { package, .. } => package.to_owned(),
            Way::None => entry.id.clone(),
        };
        let mut sub = crate::tr_with(
            "%{what} %{version}, from the ACP registry",
            &[
                ("what", &what),
                ("version", installed.as_ref().unwrap_or(&entry.version)),
            ],
        )
        .to_string();
        if let Some(cli) = self.here_on(host, id).and_then(|h| h.cli.clone()) {
            let version = (self.machine(host))
                .and_then(|m| m.cli_versions.get(&cli))
                // `2.1.326 (Claude Code)`: the number, its name is said already.
                .and_then(|v| v.split_whitespace().next())
                .unwrap_or_default()
                .to_owned();
            sub = format!(
                "{sub} · {}",
                crate::tr_with(
                    "drives your %{cli} %{version} (%{path})",
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
                )
            );
        }

        let mut acts = div().flex().flex_wrap().gap(px(8.)).mt(px(12.));
        let own = |s: &str| s.to_owned();
        match &stage {
            Stage::Installing(_) => {
                let a = own(id);
                acts = acts.child(
                    button("cancel-install", crate::tr("Cancel"), ButtonVariant::Ghost).on_click(
                        cx.listener(move |this, _, _, cx| this.cancel_install(host, &a, cx)),
                    ),
                );
            }
            Stage::Missing(Missing::Download) => {
                let a = own(id);
                acts = acts.child(
                    button(
                        "install",
                        crate::tr_with("Install %{name}", &named),
                        ButtonVariant::Tertiary,
                    )
                    .icon(CarbonIcon::Download)
                    .on_click(cx.listener(move |this, _, _, cx| this.install(host, &a, cx))),
                );
            }
            Stage::Missing(Missing::Runner(runner)) => {
                let url = if *runner == "npx" {
                    "https://nodejs.org/"
                } else {
                    "https://docs.astral.sh/uv/"
                };
                let label = url.trim_start_matches("https://").trim_end_matches('/');
                let a = own(id);
                acts = acts
                    .child(
                        button("recheck", crate::tr("Try again"), ButtonVariant::Tertiary)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.recheck(host, &a, cx)),
                            ),
                    )
                    .child(
                        button("get-runner", label, ButtonVariant::Ghost)
                            .on_click(cx.listener(move |this, _, _, cx| this.browse(url, cx))),
                    );
            }
            Stage::Failed(_) => {
                let a = own(id);
                acts = acts.child(
                    button(
                        "recheck",
                        crate::tr("Check that it speaks ACP"),
                        ButtonVariant::Tertiary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.recheck(host, &a, cx))),
                );
            }
            _ if enabled => {
                let a = own(id);
                acts = acts.child(
                    button("start", crate::tr("New thread"), ButtonVariant::Tertiary)
                        .icon(CarbonIcon::Add)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // A project on the agent's own server; with none, a
                            // scratch folder there.
                            let place = (this.chosen_project())
                                .filter(|p| p.host == host)
                                .or_else(|| {
                                    (this.projects().into_iter())
                                        .find(|p| p.host == host && !this.is_scratch(p))
                                })
                                .or_else(|| this.scratch(host));
                            if let Some(place) = place {
                                this.close_settings(window, cx);
                                this.new_session(&a, place, window, cx);
                            } else {
                                this.trouble(
                                    crate::tr("There is no project to begin the thread in"),
                                    None,
                                    None,
                                );
                                cx.notify();
                            }
                        })),
                );
            }
            _ => {}
        }
        if !enabled {
            let a = own(id);
            acts = acts.child(
                button("toggle", crate::tr("Turn on"), ButtonVariant::Tertiary)
                    .disabled(self.held("enabled"))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(host, &a, cx))),
            );
        }
        if update.is_some() && enabled && !busy && !matches!(stage, Stage::Installing(_)) {
            let a = own(id);
            acts = acts.child(
                button(
                    "update",
                    crate::tr_with("Update to %{version}", &[("version", &entry.version)]),
                    ButtonVariant::Tertiary,
                )
                .icon(CarbonIcon::Download)
                .on_click(cx.listener(move |this, _, _, cx| this.install(host, &a, cx))),
            );
        }
        if matches!(auth, Auth::Needed(_)) {
            let a = own(id);
            acts = acts.child(
                button(
                    "page-sign-in",
                    crate::tr("Sign in"),
                    ButtonVariant::Tertiary,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.sign_in_agent(host, &a, cx))),
            );
        } else if auth == Auth::Ready
            && !busy
            && self
                .slot(host, id)
                .filter(|s| s.running())
                .and_then(Slot::caps)
                .is_some_and(|c| c.auth.logout.is_some())
        {
            let a = own(id);
            acts = acts.child(
                button("log-out", crate::tr("Log out"), ButtonVariant::Ghost)
                    .on_click(cx.listener(move |this, _, _, cx| this.log_out(host, &a, cx))),
            );
        }
        if enabled {
            let a = own(id);
            acts = acts.child(
                button("toggle", crate::tr("Turn off"), ButtonVariant::Ghost)
                    .disabled(self.held("enabled"))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(host, &a, cx))),
            );
        }
        if installed.is_some() && !busy && !matches!(stage, Stage::Installing(_)) {
            let a = own(id);
            acts = acts.child(
                button(
                    "uninstall",
                    crate::tr("Uninstall"),
                    ButtonVariant::DangerGhost,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.uninstall(host, &a, window, cx)),
                ),
            );
        }

        // How far an install is, when its size is known; never a guess.
        let progress = match &stage {
            Stage::Installing(install) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .mt(px(8.))
                    .children(install.share.map(|share| {
                        Progress::new("install-share")
                            .xsmall()
                            .value(share * 100.)
                            .color(theme::accent())
                    }))
                    .child(ui::fine(faces, install.what.clone())),
            ),
            _ => None,
        };

        div()
            .flex()
            .items_start()
            .gap(px(16.))
            .child(ui::mark(faces, &self.mark_of(id), 40.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                ui::text(faces, name.clone())
                                    .text_size(px(28.))
                                    .line_height(px(36.))
                                    .text_color(theme::text()),
                            )
                            .child(
                                Badge::new(crate::tr(word))
                                    .variant(tone)
                                    .size(BadgeSize::Sm),
                            ),
                    )
                    .child(
                        ui::text(faces, sub)
                            .mt(px(4.))
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text2()),
                    )
                    .children(progress)
                    .child(acts),
            )
    }

    /// What wants you before it runs: why it stopped, why it is not here,
    /// its sign-in, and an ACP other than u0's.
    fn agent_alerts(&self, host: Host, id: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let name = self.name_of(id);
        let named = [("name", &name as &dyn std::fmt::Display)];
        let mut alerts = Vec::new();
        match self.stage_of(host, id) {
            Some(Stage::Failed(why)) => alerts.push(
                Alert::new("agent-failed")
                    .full_width(true)
                    .variant(AlertVariant::Error)
                    .title(crate::tr_with("%{name} stopped", &named))
                    .description(why)
                    .into_any_element(),
            ),
            Some(Stage::Missing(m)) if !matches!(m, Missing::Download) => {
                alerts.push(ui::trouble(m.say(&name)).into_any_element());
            }
            _ => {}
        }
        let slot = self.slot(host, id);
        if let Some(Auth::Needed(why)) = slot.map(|s| &s.auth) {
            let desk = cx.entity();
            let a = id.to_owned();
            alerts.push(
                Alert::new("agent-sign-in")
                    .full_width(true)
                    .variant(AlertVariant::Info)
                    .title(crate::tr_with("Sign in to %{name}", &named))
                    .description(crate::tr_with(
                        "%{name} says: %{why}",
                        &[("name", &name), ("why", why)],
                    ))
                    .action(
                        AlertAction::new(crate::tr("I've signed in")).on_click(move |_, cx| {
                            desk.update(cx, |this, cx| this.recheck_auth(host, &a, cx));
                        }),
                    )
                    .into_any_element(),
            );
        }
        if let Some(Stage::Running { .. }) = slot.map(|s| &s.stage)
            && let Some(init) = slot.and_then(|s| s.known.as_ref())
            && init.protocol_version != agent_client_protocol::schema::ProtocolVersion::V1
        {
            alerts.push(
                Alert::new("agent-acp")
                    .full_width(true)
                    .variant(AlertVariant::Warning)
                    .title(crate::tr_with(
                        "It speaks ACP version %{version}; u0 speaks version 1. Some of what it says may not show.",
                        &[("version", &format!("{:?}", init.protocol_version))],
                    ))
                    .into_any_element(),
            );
        }
        alerts
    }

    /// The other instances of the agent `id`: each signs in apart, its
    /// CLI's settings in a folder of its own; a line to add one. Nothing
    /// where u0 does not know where its CLI keeps them.
    fn agent_instances(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let agent = crate::registry::instance(id).0.to_owned();
        let Some(var) = crate::registry::known(&agent).and_then(|k| k.4) else {
            return div().into_any_element();
        };
        let settings = self.settings_on(host);
        let held = self.held("enabled");
        let mut section = SettingsSection::new(crate::tr("Instances")).description(crate::tr(
            "Another instance is the agent run again with its own sign-in and settings, side by side with this one: a thread begun in it stays with it.",
        ));
        if let Some(by) = managed(faces, "enabled") {
            section = section.action(by);
        }
        for (n, account) in (settings.enabled.iter())
            .filter(|e| crate::registry::instance(e).0 == agent && **e != agent)
            .enumerate()
        {
            let auth = self.slot(host, account).map(|s| &s.auth);
            let state = match auth {
                Some(Auth::Ready) => Some(("Signed in", BadgeVariant::Green)),
                Some(Auth::Needed(_)) => Some(("Needs sign-in", BadgeVariant::WarmGray)),
                _ => None,
            };
            let folder = (settings.env.get(account))
                .and_then(|env| env.get(var))
                .cloned()
                .flatten()
                .unwrap_or_default();
            let (sign, drop) = (account.clone(), account.clone());
            section = section.child(
                row(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::mark(faces, &self.mark_of(account), 16.))
                        .child(
                            ui::text(faces, self.name_of(account))
                                .text_size(px(crate::style::BODY)),
                        )
                        .children(state.map(|(word, tone)| {
                            Badge::new(crate::tr(word))
                                .variant(tone)
                                .size(BadgeSize::Sm)
                        })),
                    Some(
                        ui::one_line(ui::mono(
                            faces,
                            format!("{var} = {}", Desk::shown(std::path::Path::new(&folder))),
                        ))
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(16.))
                        .text_color(theme::text2()),
                    ),
                )
                .when(matches!(auth, Some(Auth::Needed(_))), |r| {
                    r.child(
                        button(
                            format!("sign-in-{n}"),
                            crate::tr("Sign in"),
                            ButtonVariant::Tertiary,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.sign_in_agent(host, &sign, cx)),
                        ),
                    )
                })
                .child(
                    button(
                        format!("drop-account-{n}"),
                        crate::tr("Take it away"),
                        ButtonVariant::Ghost,
                    )
                    .disabled(held)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.drop_account(host, &drop, window, cx);
                    })),
                ),
            );
        }
        section
            .child(
                foot()
                    .child(field(Input::new(&self.pages.account), faces, false, held).w(px(300.)))
                    .child(
                        button(
                            "add-account",
                            crate::tr("Add an instance"),
                            ButtonVariant::Ghost,
                        )
                        .icon(CarbonIcon::Add)
                        .disabled(held)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.add_account(host, &agent, window, cx);
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    /// The variables the agent `id` starts with, a secret one's value
    /// hidden (it is in the Keychain, not here); and a line to add one.
    fn agent_vars(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> SettingsSection {
        let grid = || {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(8.))
                .pl(px(20.))
                .pr(px(12.))
        };
        let held = self.held("env");
        let mut section = SettingsSection::new(crate::tr("Variables")).description(crate::tr(
            "Set in the agent's environment as it starts. A secret one is kept in the macOS Keychain, never in u0's files; a value may be a 1Password reference, read by op as it starts.",
        ));
        if let Some(by) = managed(faces, "env") {
            section = section.action(by);
        }
        for (n, (name, value)) in self
            .settings_on(host)
            .env
            .get(id)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let (agent, var) = (id.to_owned(), name.clone());
            section =
                section.child(
                    grid()
                        .child(
                            ui::one_line(ui::mono(faces, name.clone()))
                                .w(px(200.))
                                .flex_none()
                                .text_size(px(crate::style::BODY))
                                .line_height(px(20.)),
                        )
                        .child(
                            ui::one_line(ui::mono(
                                faces,
                                value.clone().unwrap_or_else(|| "••••••••".into()),
                            ))
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(crate::style::BODY))
                            .text_color(theme::text2()),
                        )
                        // Where it comes from, badged as the board's SecretField
                        // does: the Keychain, or a 1Password reference.
                        .children(
                            match value.as_deref() {
                                None => Some("Keychain"),
                                Some(v) if v.starts_with("op://") => Some("1Password"),
                                Some(_) => None,
                            }
                            .map(|word| {
                                Badge::new(crate::tr(word))
                                    .variant(BadgeVariant::Purple)
                                    .size(BadgeSize::Sm)
                            }),
                        )
                        .child(
                            button(
                                format!("drop-var-{n}"),
                                crate::tr_with("Take out %{name}", &[("name", &name)]),
                                ButtonVariant::Ghost,
                            )
                            .icon(CarbonIcon::Close)
                            .icon_only(true)
                            .disabled(held)
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.drop_var(host, &agent, &var, cx),
                            )),
                        ),
                );
        }
        let (agent, desk) = (id.to_owned(), cx.entity());
        section.child(
            grid()
                .child(
                    field(Input::new(&self.pages.var_name), faces, true, held)
                        .w(px(200.))
                        .flex_none(),
                )
                .child(
                    field(
                        Input::new(&self.pages.var_value).mask_toggle(),
                        faces,
                        true,
                        held,
                    )
                    .flex_1()
                    .min_w(px(0.)),
                )
                .child(
                    Checkbox::new("var-secret")
                        .label(crate::tr("Secret"))
                        .checked(self.pages.var_secret)
                        .disabled(held)
                        .on_checked_change(move |on, _, cx| {
                            desk.update(cx, |this, cx| {
                                this.pages.var_secret = on;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    button(
                        "add-var",
                        crate::tr("Add the variable"),
                        ButtonVariant::Ghost,
                    )
                    .icon(CarbonIcon::Add)
                    .icon_only(true)
                    .disabled(held)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.add_var(host, &agent, window, cx);
                    })),
                ),
        )
    }

    /// Whether the agent's new threads are given u0's tools.
    fn agent_tools(&self, host: Host, id: &str, cx: &mut Context<Self>) -> SettingsSection {
        let on = !self.settings_on(host).no_tools.iter().any(|t| t == id);
        let (agent, desk) = (id.to_owned(), cx.entity());
        SettingsSection::new(crate::tr("u0's tools")).child(
            self.srow(
                "no_tools",
                crate::tr("Give its threads u0's tools"),
                crate::tr("Each new thread of it is given u0's tools, as an MCP server: the threads u0 keeps, a diff, a task handed to another thread, a pull request, a task scheduled, a question put to you. What acts asks you here first."),
            )
                .switch(on, move |on, _, cx| {
                    desk.update(cx, |this, cx| {
                        let off = &mut this.settings_on_mut(host).no_tools;
                        off.retain(|t| *t != agent);
                        if !on {
                            off.push(agent.clone());
                        }
                        this.save_settings_on(host);
                        cx.notify();
                    });
                }),
        )
    }

    /// A command line of yours that starts the agent, checked as it is
    /// used.
    fn agent_command(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> SettingsSection {
        let registry = match self.entry(id).map(crate::registry::Entry::way) {
            Some(Way::Archive) => self.entry(id).map_or_else(String::new, |e| {
                let b = &e.distribution.binary[&crate::registry::platform()];
                std::iter::once(b.cmd.trim_start_matches("./").to_owned())
                    .chain(b.args.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
            Some(Way::Runner { runner, package }) => format!("{runner} -y {package}"),
            _ => String::new(),
        };
        let about = match self.settings_on(host).commands.get(id) {
            Some(c) => crate::tr_with(
                "Started with `%{command}`; clear it to go back to the registry's way.",
                &[("command", c)],
            ),
            None => crate::tr_with(
                "Empty: the registry's way (%{command}).",
                &[("command", &registry)],
            ),
        };
        let a = id.to_owned();
        let command = self.srow("commands", crate::tr("Command"), about);
        let held = command.is_locked();
        SettingsSection::new(crate::tr("Use this command"))
            .description(crate::tr("A command of yours starts it instead of the registry's way (your own build, a version manager's): it must speak ACP on its standard input and output."))
            .child(
                command.control(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(field(Input::new(&self.pages.command), faces, true, held).w(px(240.)))
                        .child(
                            button("save-command", crate::tr("Check that it speaks ACP"), ButtonVariant::Tertiary)
                                .disabled(held)
                                .on_click(cx.listener(move |this, _, _, cx| this.save_command(host, &a, cx))),
                        ),
                ),
            )
    }

    /// Model ids of yours, offered beside those the agent lists.
    fn agent_models(
        &self,
        faces: &Faces,
        host: Host,
        id: &str,
        cx: &mut Context<Self>,
    ) -> SettingsSection {
        let mut chips = div().flex().flex_wrap().gap(px(8.)).py(px(16.)).px(px(20.));
        let held = self.held("models");
        let models = self.settings_on(host).models.get(id).into_iter().flatten();
        let mut any = false;
        for (n, m) in models.enumerate() {
            any = true;
            let (agent, model, desk) = (id.to_owned(), m.clone(), cx.entity());
            chips = chips.child(
                Badge::new(m.clone())
                    .id(SharedString::from(format!("model-{n}")))
                    .variant(BadgeVariant::Blue)
                    .disabled(held)
                    .dismiss_label(crate::tr("Take it out"))
                    .on_dismiss(move |_, _, cx| {
                        desk.update(cx, |this, cx| this.drop_model(host, &agent, &model, cx));
                    }),
            );
        }
        let a = id.to_owned();
        SettingsSection::new(crate::tr("Your own models"))
            .description(crate::tr("Model ids of yours are offered in the composer beside those the agent lists; the agent may refuse one it does not know."))
            .when_some(managed(faces, "models"), SettingsSection::action)
            .when(any, |s| s.child(chips))
            .child(
                foot()
                    .when(any, |d| d.border_t_1().border_color(theme::role(ColorRole::BorderSubtle01)))
                    .child(field(Input::new(&self.pages.model), faces, true, held).w(px(300.)))
                    .child(
                        button("add-model", crate::tr("Add a model"), ButtonVariant::Ghost)
                            .icon(CarbonIcon::Add)
                            .disabled(held)
                            .on_click(cx.listener(move |this, _, window, cx| this.add_model(host, &a, window, cx))),
                    ),
            )
    }
}
