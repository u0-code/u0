//! First-run onboarding (`Welcome` board) and empty-workspace project setup
//! (`NoProjects` board), built with `u0ui` Carbon components.

use std::{collections::HashSet, path::PathBuf};

use gpui::component::{
    input::{InputEvent, InputState},
    scroll::ScrollableElement as _,
};
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, SharedString, Window, div, prelude::*, px,
};
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, Checkbox, Tabs, TabsTrigger, form::FieldSize,
        list_box::ListItem, select::Select,
    },
    icons::{CarbonIcon, Icon, IconSize},
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::{
        fields,
        primitives::Dropzone,
        settings::{AgentRow, AgentState},
    },
};

use crate::{
    desk::{Desk, Place, TOPBAR, draggable, projects::Source},
    fonts::Faces,
    hub::{Auth, Host, Stage},
    pages::Tab,
    theme,
};

/// State for `Welcome` and `NoProjects`.
pub struct Onboarding {
    pub tab: SharedString,
    pub host: Host,
    pub folder: Entity<InputState>,
    pub repo_url: Entity<InputState>,
    pub clone_into: Entity<InputState>,
    pub new_name: Entity<InputState>,
    pub publish: bool,
    pub expanded_agents: HashSet<String>,
    pub picked_folders: HashSet<PathBuf>,
    pub show_all_found: bool,
}

impl Onboarding {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let notify_change = |input: &Entity<InputState>, cx: &mut Context<Desk>| {
            cx.subscribe(input, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        };
        let folder = cx.new(|cx| InputState::new(window, cx));
        let repo_url = cx.new(|cx| InputState::new(window, cx));
        let clone_into = cx.new(|cx| InputState::new(window, cx));
        let new_name = cx.new(|cx| InputState::new(window, cx));
        for inp in [&folder, &repo_url, &clone_into, &new_name] {
            notify_change(inp, cx);
        }
        let mut expanded_agents = HashSet::new();
        if let Some((first, ..)) = crate::registry::KNOWN.first() {
            expanded_agents.insert((*first).to_owned());
        }
        Self {
            tab: "open".into(),
            host: Host::HERE,
            folder,
            repo_url,
            clone_into,
            new_name,
            publish: false,
            expanded_agents,
            picked_folders: HashSet::new(),
            show_all_found: false,
        }
    }
}

impl Desk {
    /// Complete first-run setup: adopt any ticked folders, persist
    /// `settings.welcomed = true`, and ask telemetry consent if unset.
    pub fn finish_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked: Vec<PathBuf> = self.onboarding.picked_folders.drain().collect();
        for folder in picked {
            self.adopt(&Place::new(Host::HERE, folder));
        }
        self.settings.welcomed = true;
        self.save_settings();
        if self.settings.telemetry.is_none() {
            self.ask_telemetry(window, cx);
        }
        cx.notify();
    }

    /// Full-window 3-card setup (`Welcome` board).
    pub(crate) fn welcome_view(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
        let top = draggable()
            .flex()
            .flex_none()
            .items_center()
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(16.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(
                div()
                    .type_style(TypeStyle::HeadingCompact01)
                    .child(crate::tr("Set up u0")),
            )
            .child(
                div()
                    .ml(px(12.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextHelper))
                    .child(crate::tr("Every step is optional")),
            );

        let card_shell = |heading: SharedString, sub: SharedString| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(260.))
                .gap(px(16.))
                .p(px(20.))
                .bg(theme::role(ColorRole::Layer01))
                .child(
                    div()
                        .type_style(TypeStyle::Heading03)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(heading),
                )
                .child(
                    div()
                        .mt(px(-8.))
                        .type_style(TypeStyle::Body01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(sub),
                )
        };

        // Card 1: Agents
        let mut agent_list = div().flex().flex_col();
        let mut listed_ids: Vec<String> = crate::registry::KNOWN
            .iter()
            .map(|(id, ..)| (*id).to_owned())
            .collect();
        for id in &self.settings.enabled {
            if !listed_ids.contains(id) {
                listed_ids.push(id.clone());
            }
        }
        for (idx, id) in listed_ids.into_iter().take(5).enumerate() {
            let name = self.name_of(&id);
            let enabled = self.settings.enabled.contains(&id);
            let slot = self.slot(Host::HERE, &id);
            let here = self.here_on(Host::HERE, &id);
            let version = here
                .and_then(|h| h.installed.clone())
                .or_else(|| self.entry(&id).map(|e| e.version.clone()));
            let (state, progress, installing) = match slot.map(|s| &s.stage) {
                Some(Stage::Installing(inst)) => (
                    AgentState::Installing,
                    inst.share.unwrap_or(0.35) * 100.,
                    true,
                ),
                Some(Stage::Missing(_)) => (AgentState::Missing, 0., false),
                None if here.is_some_and(|h| h.missing.is_some()) => {
                    (AgentState::Missing, 0., false)
                }
                _ => (AgentState::Installed, 0., false),
            };
            let signed_in = slot.is_none_or(|s| !matches!(s.auth, Auth::Needed(_)));
            let desk_install = cx.entity();
            let desk_sign = cx.entity();
            let desk_toggle = cx.entity();
            let (id_inst, id_sign, id_tog, id_cancel) =
                (id.clone(), id.clone(), id.clone(), id.clone());
            let mut row = AgentRow::new(SharedString::from(format!("wl-agent-{id}")), name)
                .state(state)
                .progress(progress)
                .enabled(enabled)
                .signed_in(signed_in)
                .accent(u8::try_from((idx % 5) + 1).unwrap_or(1))
                .on_install(move |_, cx| {
                    desk_install.update(cx, |d, cx| {
                        if !d.settings.enabled.contains(&id_inst) {
                            d.toggle_agent(Host::HERE, &id_inst, cx);
                        }
                        d.install(Host::HERE, &id_inst, cx);
                    });
                })
                .on_sign_in(move |_, cx| {
                    desk_sign.update(cx, |d, cx| d.sign_in_agent(Host::HERE, &id_sign, cx));
                })
                .on_toggle(move |_, _, cx| {
                    desk_toggle.update(cx, |d, cx| d.toggle_agent(Host::HERE, &id_tog, cx));
                });
            if let Some(v) = version {
                row = row.version(v);
            }
            agent_list = agent_list.child(div().relative().child(row).when(installing, |d| {
                d.child(
                    div().absolute().right(px(0.)).top(px(12.)).child(
                        Button::new(
                            SharedString::from(format!("wl-cancel-{id_cancel}")),
                            crate::tr("Cancel"),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_install(Host::HERE, &id_cancel, cx);
                        })),
                    ),
                )
            }));
        }
        let desk_reg = cx.entity();
        let agents_card = card_shell(
            crate::tr("Agents"),
            crate::tr("Found on this Mac. Turn on the ones u0 may start; sign in to the rest first."),
        )
        .child(agent_list)
        .child(
            div().ml(px(-16.)).mt(px(-8.)).child(
                Button::new("wl-browse-registry", crate::tr("Browse the ACP registry"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Launch)
                    .on_click(move |_, window, cx| {
                        desk_reg.update(cx, |d, cx| d.open_tab(Tab::Agents, window, cx));
                    }),
            ),
        )
        .child(
            div()
                .mt_auto()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "Installs use each agent's own installer, and each agent signs in its own way. An agent starts in its own default mode, listed by the agent, and asks before it acts as that mode says.",
                )),
        );

        // Card 2: Your other devices
        let step_row = |num: &'static str, text: SharedString| {
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme::role(ColorRole::LayerAccent01))
                        .type_style(TypeStyle::Label01)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(num),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .type_style(TypeStyle::Body01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(text),
                )
        };
        let desk_pair = cx.entity();
        let devices_card = card_shell(
            crate::tr("Your other devices"),
            crate::tr("Pair another Mac, or the u0 phone app, to follow threads and answer approvals."),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(step_row(
                            "1",
                            crate::tr(
                                "Show a pairing link here: u0://pair and its QR code. It works once, for 10 minutes.",
                            ),
                        ))
                        .child(step_row(
                            "2",
                            crate::tr(
                                "Open the link on the other Mac, or scan the QR with the phone. It makes a key of its own for this Mac.",
                            ),
                        ))
                        .child(step_row(
                            "3",
                            crate::tr(
                                "Check that both show the same SHA256: fingerprint, and choose what it may do: Everything or Read only.",
                            ),
                        )),
                )
                .child(
                    Button::new("wl-show-pair", crate::tr("Show pairing link"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::QrCode)
                        .on_click(move |_, window, cx| {
                            desk_pair.update(cx, |d, cx| {
                                d.share(crate::share::Share::Serve(true));
                                d.new_link();
                                d.open_tab(Tab::Devices, window, cx);
                            });
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .gap(px(4.))
                        .pt(px(16.))
                        .border_t_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr("Reached by")),
                        )
                        .child(
                            self.route_select("wl-reach", cx)
                                .size(FieldSize::Sm)
                                .layer(LayerLevel::Two),
                        )
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(crate::tr(
                                    "This Mac alone at first. From elsewhere, through Tailscale, every network, or a cloudflared tunnel you start.",
                                )),
                        ),
                ),
        )
        .child(
            div()
                .mt_auto()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "A device comes in by its key, never by a password or an account. Nothing goes through a server of ours.",
                )),
        );

        // Card 3: Projects and history
        let desk_drop = cx.entity();
        let dropzone = Dropzone::new("wl-dropzone")
            .folder(true)
            .label(crate::tr("Drop a project folder"))
            .hint(crate::tr("or click to choose a repository or any folder"))
            .on_files(move |paths, _, cx| {
                desk_drop.update(cx, |d, cx| {
                    for p in paths {
                        d.onboarding.picked_folders.insert(p.clone());
                        d.adopt(&Place::new(Host::HERE, p));
                    }
                    cx.notify();
                });
            });

        let mut agent_history = div().flex().flex_col();
        if let Some(m) = self.machine(Host::HERE) {
            for (agent_id, folders) in &m.elsewhere {
                if folders.is_empty() {
                    continue;
                }
                let total_threads: usize = folders.iter().map(|(_, n)| *n).sum();
                let open = self.onboarding.expanded_agents.contains(agent_id);
                let agent_toggle = agent_id.clone();
                let header_label = crate::tr_with(
                    "%{name} · %{n} threads",
                    &[("name", &self.name_of(agent_id)), ("n", &total_threads)],
                );
                let mut group = div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .child(
                        div()
                            .id(SharedString::from(format!("wl-hist-{agent_id}")))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.onboarding.expanded_agents.remove(&agent_toggle) {
                                    this.onboarding.expanded_agents.insert(agent_toggle.clone());
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .type_style(TypeStyle::Body01)
                                    .child(header_label),
                            )
                            .child(
                                Icon::carbon(if open {
                                    CarbonIcon::ChevronUp
                                } else {
                                    CarbonIcon::ChevronDown
                                })
                                .size(IconSize::S16)
                                .color(theme::role(ColorRole::IconSecondary)),
                            ),
                    );
                if open {
                    let limit = if self.onboarding.show_all_found {
                        folders.len()
                    } else {
                        2
                    };
                    for (folder, count) in folders.iter().take(limit) {
                        let checked = self.onboarding.picked_folders.contains(folder)
                            || self.settings.folders.contains(folder);
                        let path_clone = folder.clone();
                        let desk_chk = cx.entity();
                        group =
                            group.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .pl(px(12.))
                                    .child(
                                        Checkbox::new(SharedString::from(format!(
                                            "wl-fold-{}-{}",
                                            agent_id,
                                            folder.display()
                                        )))
                                        .label(Desk::shown(folder))
                                        .checked(checked)
                                        .on_checked_change(move |v, _, cx| {
                                            desk_chk.update(cx, |d, cx| {
                                                if v {
                                                    d.onboarding
                                                        .picked_folders
                                                        .insert(path_clone.clone());
                                                } else {
                                                    d.onboarding.picked_folders.remove(&path_clone);
                                                }
                                                cx.notify();
                                            });
                                        }),
                                    )
                                    .child(
                                        div()
                                            .font_family(faces.mono.clone())
                                            .type_style(TypeStyle::Code01)
                                            .text_color(theme::role(ColorRole::TextSecondary))
                                            .child(count.to_string()),
                                    ),
                            );
                    }
                    if !self.onboarding.show_all_found && folders.len() > 2 {
                        let rem_count = folders.len() - 2;
                        let rem_threads: usize = folders[2..].iter().map(|(_, n)| *n).sum();
                        group = group.child(
                            div()
                                .id(SharedString::from(format!("wl-more-{agent_id}")))
                                .pl(px(28.))
                                .cursor_pointer()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::LinkPrimary))
                                .child(crate::tr_with(
                                    "Show %{n} more folders · %{threads} threads",
                                    &[("n", &rem_count), ("threads", &rem_threads)],
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.onboarding.show_all_found = true;
                                    cx.notify();
                                })),
                        );
                    }
                }
                agent_history = agent_history.child(group);
            }
        }

        let projects_card = card_shell(
            crate::tr("Projects and history"),
            crate::tr(
                "Add the folders you work in. Your agents worked in these; tick one to add it as a project.",
            ),
        )
        .child(dropzone)
        .child(agent_history)
        .child(
            div()
                .mt_auto()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "Nothing is copied. Each agent lists its own threads, by project; a thread is loaded from its agent when you open it, and goes on from there.",
                )),
        );

        let desk_done = cx.entity();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::role(ColorRole::Background))
            .text_color(theme::role(ColorRole::TextPrimary))
            .child(top)
            .child(
                div()
                    .id("welcome-scroll")
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(40.))
                    .py(px(32.))
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .max_w(px(1120.))
                            .gap(px(28.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .type_style(TypeStyle::Body01)
                                            .text_color(theme::role(ColorRole::TextSecondary))
                                            .child(crate::tr("Welcome to u0")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(32.))
                                            .line_height(px(40.))
                                            .font_weight(FontWeight::LIGHT)
                                            .child(crate::tr("Bring the agents you already use")),
                                    )
                                    .child(
                                        div()
                                            .type_style(TypeStyle::Heading02)
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(theme::role(ColorRole::TextSecondary))
                                            .child(crate::tr(
                                                "u0 has no agent of its own. It runs yours, on your machine, with your own sign-ins.",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(24.))
                                    .items_stretch()
                                    .child(agents_card)
                                    .child(devices_card)
                                    .child(projects_card),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .type_style(TypeStyle::Body01)
                                            .text_color(theme::role(ColorRole::TextSecondary))
                                            .child(crate::tr(
                                                "You can change all of this later in Settings (⌘,).",
                                            )),
                                    )
                                    .child(
                                        Button::new("welcome-continue", crate::tr("Continue"))
                                            .size(ButtonSize::Sm)
                                            .on_click(move |_, window, cx| {
                                                desk_done.update(cx, |d, cx| {
                                                    d.finish_welcome(window, cx);
                                                });
                                            }),
                                    ),
                            ),
                    ),
            )
    }

    /// Empty-workspace project setup (`NoProjects` board) rendered inside the
    /// main pane when no projects exist yet.
    pub(crate) fn no_projects_view(
        &self,
        faces: &Faces,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host = self.onboarding.host;
        let folder_val = self.onboarding.folder.read(cx).value().trim().to_owned();
        let url_val = self.onboarding.repo_url.read(cx).value().trim().to_owned();
        let clone_dir_val = self
            .onboarding
            .clone_into
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let new_name_val = self.onboarding.new_name.read(cx).value().trim().to_owned();
        let tab = self.onboarding.tab.as_ref();
        let can_submit = match tab {
            "open" => !folder_val.is_empty(),
            "clone" => Source::read(&url_val).is_some(),
            "new" => !new_name_val.is_empty(),
            _ => false,
        };

        let machine_row = || {
            let desk = cx.entity();
            let hosts: Vec<Host> = self.machines.iter().map(|m| m.host).collect();
            let items: Vec<ListItem> = (self.machines.iter())
                .map(|m| ListItem::labeled(m.host.0.to_string(), m.name.clone()))
                .collect();
            div().w(px(256.)).child(
                Select::new("np-machine")
                    .label(crate::tr("Machine"))
                    .size(FieldSize::Sm)
                    .layer(LayerLevel::Two)
                    .items(items)
                    .value(Some(SharedString::from(host.0.to_string())))
                    .on_value_change(move |val, _, cx| {
                        let picked = val.and_then(|v| {
                            hosts
                                .iter()
                                .copied()
                                .find(|h| h.0.to_string() == v.as_ref())
                        });
                        if let Some(h) = picked {
                            desk.update(cx, |this, cx| {
                                this.onboarding.host = h;
                                cx.notify();
                            });
                        }
                    }),
            )
        };

        let open_panel = {
            let desk_browse = cx.entity();
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .child(machine_row())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr("Folder")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap(px(1.))
                                .child(
                                    div().flex_1().min_w(px(0.)).child(fields::line_input(
                                        &self.onboarding.folder,
                                        false,
                                        fields::field_fill(true),
                                        Some(crate::tr(
                                            "Choose a folder, or drop one on this window",
                                        )),
                                        window,
                                        cx,
                                    )),
                                )
                                .child(
                                    Button::new("np-browse", crate::tr("Browse"))
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::FolderOpen)
                                        .on_click(move |_, window, cx| {
                                            desk_browse.update(cx, |_, cx| {
                                                Desk::add_project(window, cx);
                                            });
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(crate::tr(
                                    "Any folder works. If it is a Git repository, u0 also finds its branches and pull requests.",
                                )),
                        ),
                )
        };

        let clone_panel = {
            let desk_parent = cx.entity();
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .child(machine_row())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr("Repository")),
                        )
                        .child(fields::line_input(
                            &self.onboarding.repo_url,
                            false,
                            fields::field_fill(true),
                            Some(crate::tr("owner/name or URL")),
                            window,
                            cx,
                        ))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(crate::tr(
                                    "From GitHub, GitLab, Bitbucket or Azure DevOps with the token in Settings › Source control · or paste any Git URL",
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr("Clone into")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap(px(1.))
                                .child(
                                    div().flex_1().min_w(px(0.)).child(fields::line_input(
                                        &self.onboarding.clone_into,
                                        false,
                                        fields::field_fill(true),
                                        Some(SharedString::from(format!(
                                            "{}/<name>",
                                            Desk::shown(&self.projects_in(host))
                                        ))),
                                        window,
                                        cx,
                                    )),
                                )
                                .child(
                                    Button::new("np-clone-browse", crate::tr("Browse"))
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::FolderOpen)
                                        .on_click(move |_, _, cx| {
                                            desk_parent.update(cx, |_, cx| Desk::choose_parent(cx));
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(crate::tr("Default folder is set in Settings › Projects")),
                        ),
                )
        };

        let new_panel = {
            let desk_pub = cx.entity();
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .child(machine_row())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr("Project name")),
                        )
                        .child(fields::line_input(
                            &self.onboarding.new_name,
                            false,
                            fields::field_fill(true),
                            Some(crate::tr("my-project")),
                            window,
                            cx,
                        )),
                )
                .child(
                    Checkbox::new("np-publish")
                        .label(crate::tr("Publish it as a new repository"))
                        .checked(self.onboarding.publish)
                        .on_checked_change(move |v, _, cx| {
                            desk_pub.update(cx, |d, cx| {
                                d.onboarding.publish = v;
                                cx.notify();
                            });
                        }),
                )
        };

        let desk_tabs = cx.entity();
        let tabs = Tabs::new("np-tabs")
            .value(self.onboarding.tab.clone())
            .trigger(TabsTrigger::new("open", crate::tr("Open a folder")))
            .trigger(TabsTrigger::new("clone", crate::tr("Clone a repository")))
            .trigger(TabsTrigger::new("new", crate::tr("New empty project")))
            .content("open", open_panel)
            .content("clone", clone_panel)
            .content("new", new_panel)
            .on_value_change(move |v, _, cx| {
                desk_tabs.update(cx, |d, cx| {
                    d.onboarding.tab = v;
                    cx.notify();
                });
            });

        let desk_scratch = cx.entity();
        let desk_add = cx.entity();
        let scratch_path = Desk::shown(&self.scratch_dir(host));
        let card = div()
            .mt(px(28.))
            .pt(px(8.))
            .px(px(24.))
            .pb(px(24.))
            .bg(theme::role(ColorRole::Layer01))
            .flex()
            .flex_col()
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    if let Some(first) = paths.paths().first() {
                        let dir = if first.is_dir() {
                            first.clone()
                        } else {
                            first
                                .parent()
                                .map_or_else(|| first.clone(), std::path::Path::to_path_buf)
                        };
                        this.add_and_begin(&Place::new(this.onboarding.host, dir), window, cx);
                    }
                }),
            )
            .child(tabs)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mt(px(24.))
                    .ml(px(-16.))
                    .child(
                        Button::new("np-scratch", crate::tr("Start without a project (⌥⌘N)"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, window, cx| {
                                desk_scratch.update(cx, |d, cx| d.new_scratch(window, cx));
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(crate::tr_with(
                                "Runs in %{path}",
                                &[("path", &scratch_path)],
                            )),
                    )
                    .child(
                        Button::new("np-submit", crate::tr("Add project"))
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Add)
                            .disabled(!can_submit)
                            .on_click(move |_, window, cx| {
                                desk_add.update(cx, |d, cx| {
                                    let h = d.onboarding.host;
                                    match d.onboarding.tab.as_ref() {
                                        "open" => {
                                            let path = expand_home(&folder_val);
                                            d.add_and_begin(&Place::new(h, path), window, cx);
                                        }
                                        "clone" => {
                                            if !clone_dir_val.is_empty()
                                                && let Some(src) = Source::read(&url_val)
                                            {
                                                let to = Place::new(h, expand_home(&clone_dir_val));
                                                d.clone_into(to, src.url, cx);
                                            } else {
                                                d.start_clone(h, &url_val, cx);
                                            }
                                        }
                                        "new" => {
                                            d.create_project(
                                                h,
                                                &new_name_val,
                                                d.onboarding.publish,
                                                cx,
                                            );
                                        }
                                        _ => {}
                                    }
                                });
                            }),
                    ),
            );

        let found = self.found_elsewhere(host);
        let found_section = (!found.is_empty()).then(|| {
            let shown_limit = if self.onboarding.show_all_found {
                found.len()
            } else {
                2
            };
            let mut sec = div().flex().flex_col().mt(px(36.)).child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .mb(px(8.))
                    .child(
                        div()
                            .type_style(TypeStyle::HeadingCompact01)
                            .child(crate::tr("Found on This Mac")),
                    )
                    .child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(crate::tr("Folders your agents already worked in")),
                    ),
            );
            for (ix, (folder, count, agents)) in found.iter().take(shown_limit).enumerate() {
                let agent_names: Vec<String> = agents.iter().map(|a| self.name_of(a)).collect();
                let meta = crate::tr_with(
                    "%{count} threads · %{agents}",
                    &[("count", count), ("agents", &agent_names.join(", "))],
                );
                let place = Place::new(host, folder.clone());
                let desk_adopt = cx.entity();
                sec = sec.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .min_h(px(44.))
                        .py(px(8.))
                        .border_t_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            Icon::carbon(CarbonIcon::Folder)
                                .size(IconSize::S16)
                                .color(theme::role(ColorRole::IconSecondary)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .gap(px(2.))
                                .child(
                                    div()
                                        .font_family(faces.mono.clone())
                                        .type_style(TypeStyle::Code01)
                                        .child(Desk::shown(folder)),
                                )
                                .child(
                                    div()
                                        .type_style(TypeStyle::Label01)
                                        .text_color(theme::role(ColorRole::TextSecondary))
                                        .child(meta),
                                ),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("np-adopt-{ix}")),
                                crate::tr("Add"),
                            )
                            .variant(ButtonVariant::Tertiary)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Add)
                            .on_click(move |_, window, cx| {
                                desk_adopt.update(cx, |d, cx| {
                                    d.add_and_begin(&place, window, cx);
                                });
                            }),
                        ),
                );
            }
            if !self.onboarding.show_all_found && found.len() > 2 {
                let more = found.len() - 2;
                let desk_more = cx.entity();
                sec = sec.child(
                    div()
                        .flex()
                        .items_center()
                        .min_h(px(40.))
                        .border_t_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .ml(px(-16.))
                        .child(
                            Button::new(
                                "np-show-more",
                                crate::tr_with("Show %{n} more folders", &[("n", &more)]),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::ChevronDown)
                            .on_click(move |_, _, cx| {
                                desk_more.update(cx, |d, cx| {
                                    d.onboarding.show_all_found = true;
                                    cx.notify();
                                });
                            }),
                        ),
                );
            }
            sec
        });

        div()
            .id("no-projects-scroll")
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_h(px(0.))
            .px(px(32.))
            .py(px(40.))
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(px(800.))
                    .child(
                        div()
                            .text_size(px(28.))
                            .line_height(px(36.))
                            .font_weight(FontWeight::LIGHT)
                            .child(crate::tr("What should we work on?")),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .type_style(TypeStyle::Body01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(crate::tr(
                                "A project is a folder on one of your machines. Its threads, worktrees and pull requests live under it.",
                            )),
                    )
                    .child(card)
                    .children(found_section),
            )
            .into_any_element()
    }
}

fn expand_home(typed: &str) -> PathBuf {
    match (typed.strip_prefix('~'), Desk::home_path()) {
        (Some(rest), Some(home)) => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(typed),
    }
}
