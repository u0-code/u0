//! The settings, a dialog over the window: a column of tabs, and the tab
//! chosen. Ghost's own settings (`prefs`); agents, which are switched on
//! and off and installed; sessions archived, and what Ghost keeps on this
//! machine.

use std::time::SystemTime;

use gpui::component::{
    Sizable as _,
    input::Input,
    kbd::Kbd,
    setting::{SettingGroup, SettingItem, SettingPage, Settings},
    switch::Switch,
};
use gpui::{App, Context, Pixels, SharedString, WeakEntity, Window, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page},
    fonts::Faces,
    registry::Way,
    theme,
    ui::{self, Weight},
    when,
};

/// How tall the settings dialog stands in `window`.
pub fn height(window: &Window) -> Pixels {
    (window.viewport_size().height - px(80.)).min(px(820.))
}

/// A tab of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Chat,
    NewSessions,
    Writing,
    Agents,
    Sessions,
    Keyboard,
    Usage,
    About,
    Projects,
    SourceControl,
    Diagnostics,
    Devices,
    Tasks,
    Workflows,
    Telemetry,
}

impl Tab {
    /// Each tab, in the order the side list shows them.
    pub const ALL: [Self; 17] = [
        Self::General,
        Self::Devices,
        Self::Appearance,
        Self::Chat,
        Self::Keyboard,
        Self::Projects,
        Self::SourceControl,
        Self::Agents,
        Self::NewSessions,
        Self::Writing,
        Self::Tasks,
        Self::Workflows,
        Self::Sessions,
        Self::Usage,
        Self::Diagnostics,
        Self::Telemetry,
        Self::About,
    ];

    /// Its name, in your language.
    pub fn title(self) -> SharedString {
        crate::tr(match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::NewSessions => "New sessions",
            Self::Writing => "Writing",
            Self::Agents => "Agents",
            Self::Sessions => "Sessions",
            Self::Keyboard => "Keyboard",
            Self::Projects => "Projects",
            Self::Usage => "Usage",
            Self::About => "About",
            Self::SourceControl => "Source control",
            Self::Diagnostics => "Diagnostics",
            Self::Devices => "Devices",
            Self::Tasks => "Scheduled tasks",
            Self::Workflows => "Workflows",
            Self::Telemetry => "Telemetry",
        })
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::General => "sliders",
            Self::Appearance => "sun",
            Self::Chat => "reply",
            Self::NewSessions => "plus",
            Self::Writing => "pencil",
            Self::Agents => "plug",
            Self::Sessions => "archive",
            Self::Keyboard => "keyboard",
            Self::Projects => "folder",
            Self::Usage => "gauge",
            Self::About => "ghost",
            Self::SourceControl => "branch",
            Self::Diagnostics => "text-search",
            Self::Devices => "laptop",
            Self::Tasks => "clock",
            Self::Workflows => "list",
            Self::Telemetry => "gauge",
        }
    }

    /// The heading the side list shows it under; Ghost's own tabs have
    /// none.
    fn group(self) -> &'static str {
        match self {
            Self::Agents
            | Self::NewSessions
            | Self::Writing
            | Self::Tasks
            | Self::Workflows
            | Self::Sessions
            | Self::Usage => "Agents",
            Self::About | Self::Diagnostics | Self::Telemetry => "Ghost",
            _ => "",
        }
    }
}

impl Desk {
    /// The page of `tab`, its settings each drawn by `w`, the desk.
    fn tab_page(&self, tab: Tab, w: &WeakEntity<Desk>, cx: &App) -> SettingPage {
        let paged = |lede: &str, groups: Vec<SettingGroup>| {
            SettingPage::new(tab.title())
                .description(crate::tr(lede))
                .groups(groups)
        };
        let page = match tab {
            Tab::Agents => paged(
                "Turn on the agents you use. Each runs its own CLI over ACP, with its own sign-in.",
                self.agents(w, cx),
            ),
            Tab::Sessions => paged(
                "Sessions put away, and what Ghost keeps on this Mac.",
                self.sessions_tab(w),
            ),
            Tab::Usage => paged(
                "What sessions cost and took, and where your limits stand, as agents said.",
                self.usage(w),
            ),
            Tab::Diagnostics => paged(
                "What Ghost runs on this Mac, and what went wrong.",
                self.diagnostics(w),
            ),
            Tab::Tasks => paged(
                "A prompt said to an agent on a schedule. Ghost's server runs them, so they run while the window is closed; each run's session is in the sidebar.",
                self.tasks(w),
            ),
            Tab::Workflows => paged(
                "Work in stages, run by Ghost's server as one run that outlives a restart: each step's result is kept as it ends, and a step that fails pauses the run there.",
                self.flows(w),
            ),
            Tab::Telemetry => paged(
                "What the server these settings are of tells the people who make Ghost, as far as you let it.",
                self.telemetry_tab(w),
            ),
            Tab::Projects => self.projects_tab(w),
            Tab::Devices => self.devices_tab(w),
            _ => self
                .own_tab(tab, w, cx)
                .unwrap_or_else(|| SettingPage::new(tab.title())),
        };
        page.icon(ui::glyph(tab.icon()))
    }

    /// The settings, over the window: gpui-kit's Settings, a page for each
    /// tab down the left with the search above them; the way to close them
    /// at the top. A page chosen there is the tab shown.
    pub(crate) fn settings_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let chosen = self.pages.settings.read(cx).selected_index().page_ix;
        if let Some(&tab) = Tab::ALL.get(chosen)
            && tab != self.pages.tab
        {
            self.pages.tab = tab;
            self.tab_shown(tab, window, cx);
        }
        let w = cx.entity().downgrade();
        // The tabs under their headings, a heading where the group changes.
        let mut settings = Settings::new("settings")
            .state(&self.pages.settings)
            .sidebar_width(px(232.));
        let mut heading = "";
        for tab in Tab::ALL {
            if tab.group() != heading {
                heading = tab.group();
                settings = settings.section(crate::tr(heading));
            }
            settings = settings.page(self.tab_page(tab, &w, cx));
        }
        div()
            .relative()
            .size_full()
            .bg(theme::bg())
            .child(settings)
            .child(
                div()
                    .absolute()
                    .top(px(10.))
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(self.server_pick(cx))
                    .child(
                        ui::icon_button("close-settings", "x", crate::tr("Close · Esc")).on_click(
                            cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// Where the window holds more than one server, the one whose own
    /// pages (its agents, projects, tasks, sharing, what it keeps) the
    /// settings show: each server's are that server's, saved there. The
    /// window's own (its look, its keys) are this Mac's whichever it is.
    fn server_pick(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.machines.len() < 2 {
            return None;
        }
        let choices = (self.machines.iter())
            .map(|m| {
                let (host, desk) = (m.host, cx.entity());
                let pick: ui::Pick = Box::new(move |_, window, cx| {
                    desk.update(cx, |this, cx| {
                        this.pages.on = host;
                        this.pages.project = None;
                        this.pages.scope = None;
                        this.tab_shown(this.pages.tab, window, cx);
                        cx.notify();
                    });
                });
                (m.name.clone(), m.host == self.pages.on, pick)
            })
            .collect();
        Some(ui::segmented("settings-server", choices))
    }

    /// The agents of the registry: switched on and off, installed.
    fn agents(&self, w: &WeakEntity<Desk>, cx: &App) -> Vec<SettingGroup> {
        // What runs here, and what answers the search, from the registry.
        let query = self
            .pages
            .agent_search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let mut agents: Vec<_> = self
            .registry
            .agents
            .iter()
            .filter(|e| self.edited().enabled.contains(&e.id) || !matches!(e.way(), Way::None))
            .filter(|e| {
                query.is_empty()
                    || e.name.to_lowercase().contains(&query)
                    || e.id.to_lowercase().contains(&query)
            })
            .collect();
        agents.sort_by_key(|e| {
            (
                self.edited()
                    .enabled
                    .iter()
                    .position(|id| id == &e.id)
                    .unwrap_or(usize::MAX),
                e.name.to_lowercase(),
            )
        });
        let mut head: Vec<SettingItem> = Vec::new();
        if let Some(first) = self.slots_on(self.pages.on).next() {
            let id = first.entry.id.clone();
            // Said, not chosen here: the first agent on is it.
            head.push(item(
                w,
                crate::tr("Default for new sessions"),
                crate::tr("The first agent switched on."),
                move |d, faces, cx| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(ui::mark(faces, &d.mark_of(&id), 14.))
                        .child(
                            ui::text(faces, d.name_of(&id))
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::text2()),
                        )
                        .children(
                            crate::keys::keys_of(cx, "new_session")
                                .into_iter()
                                .map(Kbd::new),
                        )
                        .into_any_element()
                },
            ));
        }
        let refused = match self.pages.on {
            crate::hub::Host::HERE => self.settings_refused.clone(),
            on => self.machine(on).and_then(|m| m.refused.clone()),
        };
        for trouble in [refused, self.registry_trouble.clone()]
            .into_iter()
            .flatten()
        {
            head.push(whole(w, &[], move |_, _, _| {
                ui::trouble(trouble.clone()).into_any_element()
            }));
        }
        if self.registry.agents.is_empty() && self.registry_trouble.is_none() {
            head.push(item(
                w,
                crate::tr("Reading the registry…"),
                "",
                |_, _, _| div().into_any_element(),
            ));
        }
        let host = self.pages.on;
        let acts = self.settings_on(host).outside_acts;
        head.push(item(
            w,
            "Agents outside Ghost may act",
            "An agent outside Ghost reaches Ghost's tools by the key in mcp.json (ghost --mcp, or over HTTP on this Mac). Off, it only reads; on, it may say something in a session or stop its turn, each asked of you there.",
            move |_, _, cx| {
                gpui::component::switch::Switch::new("outside-acts")
                    .accessibility_label("Agents outside Ghost may act")
                    .small()
                    .checked(acts)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_on_mut(host).outside_acts = !acts;
                        this.save_settings_on(host);
                        cx.notify();
                    }))
                    .into_any_element()
            },
        ));
        head.push(whole(w, &["search"], |d, _, _| {
            Input::new(&d.pages.agent_search)
                .small()
                .prefix(ui::icon("search", 14.).text_color(theme::faint()))
                .into_any_element()
        }));
        let rows = agents
            .into_iter()
            .map(|e| {
                let id = e.id.clone();
                whole(w, &[], move |d, faces, cx| {
                    d.agent_row(faces, &id, cx).into_any_element()
                })
                .keywords([e.name.clone(), e.id.clone()])
            })
            .collect();
        let read = self.registry_read.map_or_else(
            || crate::tr("Registry not read yet"),
            |t| match when::ago(t, SystemTime::now()).as_str() {
                "now" => crate::tr("Registry checked just now"),
                ago => crate::tr_with("Registry checked %{ago} ago", &[("ago", &ago)]),
            },
        );
        vec![
            SettingGroup::new().items(head),
            group(crate::tr("Agents"), rows).description(crate::tr_with(
                "Ghost keeps its settings and the agents it installs in %{home}.",
                &[("home", &Desk::shown(self.home_on(self.pages.on)))],
            )),
            SettingGroup::new().item(item(w, read, "", |_, _, cx| {
                ui::button(
                    "refresh",
                    crate::tr("Read the registry again"),
                    Weight::Secondary,
                )
                .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx)))
                .into_any_element()
            })),
        ]
    }

    /// An agent of the registry, `id`: its logo and name (a click opens its
    /// page), how it stands here, and its switch or the way to install it.
    fn agent_row(&self, faces: &Faces, id: &str, cx: &mut Context<Self>) -> gpui::Div {
        let Some(e) = self.registry.agents.iter().find(|e| e.id == id) else {
            return div();
        };
        let host = self.pages.on;
        let on = self.edited().enabled.contains(&e.id);
        let here = self.here_on(host, &e.id).cloned().unwrap_or_default();
        let installed = self.here_on(host, &e.id).is_some() && here.missing.is_none();
        let name = self.name_of(&e.id);
        let how = match e.way() {
            Way::Archive => crate::tr("binary"),
            Way::Runner { runner, .. } => runner.into(),
            Way::None => crate::tr("unavailable"),
        };
        // Installed, which version, signed in: as far as each is known.
        let version = here.installed.unwrap_or_else(|| e.version.clone());
        let line = match (
            installed,
            on,
            e.way(),
            self.slot(host, &e.id).map(|s| &s.auth),
        ) {
            (true, true, _, Some(crate::hub::Auth::Ready)) => crate::tr_with(
                "%{version} · %{how} · signed in",
                &[("version", &version), ("how", &how)],
            ),
            (true, true, _, Some(crate::hub::Auth::Needed(_))) => crate::tr_with(
                "%{version} · %{how} · needs sign-in",
                &[("version", &version), ("how", &how)],
            ),
            (true, true, _, _) => format!("{version} · {how}").into(),
            (true, false, Way::Archive, _) => {
                crate::tr_with("%{version} installed · off", &[("version", &version)])
            }
            (true, false, _, _) => crate::tr_with("runs with %{how} · off", &[("how", &how)]),
            (false, _, _, _) => crate::tr("not installed"),
        };
        let (id, page_id) = (e.id.clone(), e.id.clone());
        let control = if installed || on {
            Switch::new(SharedString::from(format!("toggle-{id}")))
                .accessibility_label(e.name.clone())
                .small()
                .checked(on)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(host, &id, cx)))
                .into_any_element()
        } else {
            ui::button(
                SharedString::from(format!("get-{id}")),
                crate::tr("Install"),
                Weight::Secondary,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_page(Page::Agent(host, id.clone()), window, cx);
            }))
            .into_any_element()
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .child(div().when(!on, |d| d.opacity(0.45)).child(ui::mark(
                faces,
                &self.mark_of(&e.id),
                24.,
            )))
            .child(
                div()
                    .id(SharedString::from(format!("aname-{page_id}")))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .cursor_pointer()
                    .child(ui::text(faces, name).weight(gpui::FontWeight::MEDIUM))
                    .child(ui::fine(faces, line))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_page(Page::Agent(host, page_id.clone()), window, cx);
                    })),
            )
            .child(control)
    }

    /// What Ghost keeps on this machine, how much, and tidying it.
    fn storage(&self, w: &WeakEntity<Desk>) -> SettingGroup {
        use crate::tidy::Tidy;
        let mut items: Vec<SettingItem> = (self.pages.sizes.iter())
            .map(|(label, bytes)| {
                let size = crate::tidy::shown(*bytes);
                item(w, label.clone(), "", move |_, faces, _| {
                    ui::fine(faces, size.clone()).into_any_element()
                })
            })
            .collect();
        // What no session keeps is known only from the index: while it
        // does not read, every record, worktree and checkpoint looks so.
        if self.store_refused(self.pages.on).is_none() {
            items.push(item(
                w,
                crate::tr("Tidy"),
                self.pages.tidied.clone().unwrap_or_default(),
                |_, _, cx| {
                    let button = |id: &'static str, label: &'static str, what: Tidy| {
                        ui::button(id, crate::tr(label), Weight::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| this.tidy(what, cx)))
                    };
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(px(8.))
                        .child(button(
                            "tidy-records",
                            "Remove records no session keeps",
                            Tidy::Records,
                        ))
                        .child(button(
                            "tidy-worktrees",
                            "Remove worktrees no session works in",
                            Tidy::Worktrees,
                        ))
                        .child(button(
                            "tidy-checkpoints",
                            "Remove checkpoints of earlier runs",
                            Tidy::Checkpoints,
                        ))
                        .into_any_element()
                },
            ));
        }
        if let Some(refused) = self.store_refused(self.pages.on).cloned() {
            items.push(whole(w, &[], move |_, _, _| {
                ui::trouble(refused.clone()).into_any_element()
            }));
        }
        group(crate::tr("What Ghost keeps"), items).description(crate::tr_with(
            "In %{home}. A worktree with changes not committed is left as it is; undo reaches only the turns of this run.",
            &[("home", &Desk::shown(self.home_on(self.pages.on)))],
        ))
    }

    /// Projects archived: out of the list until restored here.
    fn archived_projects(&self, w: &WeakEntity<Desk>) -> Option<SettingGroup> {
        let items: Vec<SettingItem> = (self.edited().prefs.iter())
            .filter(|(_, p)| p.archived)
            .enumerate()
            .map(|(n, (folder, _))| {
                let f = crate::desk::Place::new(self.pages.on, folder.clone());
                item(
                    w,
                    self.project_name(&f),
                    Desk::shown(folder),
                    move |_, _, cx| {
                        let f = f.clone();
                        ui::button(
                            SharedString::from(format!("restore-p-{n}")),
                            crate::tr("Restore"),
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.restore_project(&f, cx)))
                        .into_any_element()
                    },
                )
            })
            .collect();
        (!items.is_empty()).then(|| group(crate::tr("Archived projects"), items))
    }

    /// Sessions put away: archived ones, restored or deleted here; and what
    /// Ghost keeps of them on this machine.
    fn sessions_tab(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let mut list: Vec<SettingItem> = (self.sessions.all.iter())
            .filter(|s| s.meta.flags.archived && !self.deleting(s.key))
            .map(|s| {
                let key = s.key;
                whole(w, &[], move |d, faces, cx| {
                    d.archived_row(faces, key, cx).into_any_element()
                })
                .keywords([s.meta.title.clone()])
            })
            .collect();
        if list.is_empty() {
            list.push(item(
                w,
                crate::tr("Nothing archived"),
                crate::tr(
                    "A session archived from its menu waits here, read only, until it is restored.",
                ),
                |_, _, _| div().into_any_element(),
            ));
        }
        let mut groups = vec![group(crate::tr("Archived sessions"), list)];
        groups.extend(self.archived_projects(w));
        groups.push(self.storage(w));
        groups
    }

    /// A session archived, `key`: its agent, title, project and when (a
    /// click reads it, saying it is archived), Restore and Delete.
    fn archived_row(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> gpui::Div {
        let Some(s) = self.sessions.at(key) else {
            return div();
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
            // Opened to read, it says it is archived, with Restore.
            .child(
                div()
                    .id(SharedString::from(format!("read-{key}")))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show(key, window, cx);
                    }))
                    .child(ui::one_line(
                        ui::text(faces, s.meta.title.clone()).text_size(px(crate::style::LABEL)),
                    ))
                    .child(ui::fine(
                        faces,
                        format!(
                            "{} · {}",
                            Desk::shown(&s.meta.project),
                            when::ago(s.meta.updated, SystemTime::now())
                        ),
                    )),
            )
            .child(
                ui::button(
                    SharedString::from(format!("restore-{key}")),
                    crate::tr("Restore"),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
            )
            .child(
                ui::button(
                    SharedString::from(format!("delete-{key}")),
                    crate::tr("Delete"),
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ask_delete(vec![key], window, cx);
                })),
            )
    }
}
