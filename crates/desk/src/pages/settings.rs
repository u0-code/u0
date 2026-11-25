//! The settings, a dialog over the window: a column of tabs, and the tab
//! chosen. Ghost's own settings (`prefs`); agents, which are switched on
//! and off and installed; sessions archived, and what Ghost keeps on this
//! machine.

use std::time::SystemTime;

use gpui::component::{
    Icon, Sizable as _,
    input::Input,
    kbd::Kbd,
    sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem},
    switch::Switch,
};
use gpui::{Context, Entity, SharedString, Window, div, prelude::*, px};

use super::card;
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page},
    fonts::Faces,
    registry::Way,
    theme,
    ui::{self, Weight},
    when,
};

/// The settings as gpui-kit's dialog layer draws them. That layer is the
/// window root's, drawn apart from the desk, so this view of the desk is
/// drawn again whenever the desk is.
pub struct Shown {
    desk: Entity<Desk>,
}

impl Shown {
    pub fn new(desk: Entity<Desk>, cx: &mut Context<Self>) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        Self { desk }
    }
}

impl Render for Shown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces = crate::fonts::faces(cx).clone();
        self.desk.update(cx, |d, cx| d.settings_dialog(&faces, cx))
    }
}

/// A tab of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Chat,
    NewSessions,
    Agents,
    Sessions,
    Keyboard,
    Usage,
    About,
}

impl Tab {
    /// Each tab, in the order the side list shows them.
    pub const ALL: [Self; 9] = [
        Self::General,
        Self::Appearance,
        Self::Chat,
        Self::Keyboard,
        Self::Agents,
        Self::NewSessions,
        Self::Sessions,
        Self::Usage,
        Self::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::NewSessions => "New sessions",
            Self::Agents => "Agents",
            Self::Sessions => "Sessions",
            Self::Keyboard => "Keyboard",
            Self::Usage => "Usage",
            Self::About => "About",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::General => "sliders",
            Self::Appearance => "sun",
            Self::Chat => "reply",
            Self::NewSessions => "plus",
            Self::Agents => "plug",
            Self::Sessions => "archive",
            Self::Keyboard => "keyboard",
            Self::Usage => "clock",
            Self::About => "ghost",
        }
    }

    /// The heading the side list shows it under; Ghost's own tabs have
    /// none.
    fn group(self) -> &'static str {
        match self {
            Self::Agents | Self::NewSessions | Self::Sessions | Self::Usage => "Agents",
            Self::About => "Ghost",
            _ => "",
        }
    }
}

impl Desk {
    /// Every setting and key in which `q` is said, under its tab's title.
    fn found(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> gpui::Div {
        let pages: Vec<_> = Tab::ALL
            .into_iter()
            .filter_map(|tab| self.own_tab(faces, tab, q, cx))
            .collect();
        if pages.is_empty() {
            return div()
                .px(px(40.))
                .py(px(28.))
                .child(ui::fine(faces, format!("No setting says “{q}”.")));
        }
        div().flex().flex_col().children(pages)
    }

    /// The settings, over the window: the tabs down the left under their
    /// headings, the search above them; the tab chosen, or what the search
    /// found, under a header that names it.
    pub(crate) fn settings_dialog(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let q = self.pages.search.read(cx).value().trim().to_lowercase();
        // The tabs under their headings, a heading where the group changes.
        let mut groups: Vec<(&str, Vec<SidebarMenuItem>)> = Vec::new();
        for tab in Tab::ALL {
            let on = q.is_empty() && self.pages.tab == tab;
            let item = SidebarMenuItem::new(tab.title())
                .icon(Icon::empty().path(format!("icons/{}.svg", tab.icon())))
                .active(on)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_tab(tab, window, cx);
                }));
            match groups.last_mut() {
                Some((heading, items)) if *heading == tab.group() => items.push(item),
                _ => groups.push((tab.group(), vec![item])),
            }
        }
        let nav = Sidebar::new("settings-nav")
            .collapsible(false)
            .w(px(232.))
            .header(
                Input::new(&self.pages.search)
                    .small()
                    .prefix(ui::icon("search", 13.).text_color(theme::faint())),
            )
            .children(groups.into_iter().map(|(heading, items)| {
                SidebarGroup::new(heading).child(SidebarMenu::new().children(items))
            }));
        let shown = if !q.is_empty() {
            self.found(faces, &q, cx)
        } else if let Some(own) = self.own_tab(faces, self.pages.tab, "", cx) {
            own
        } else if self.pages.tab == Tab::Sessions {
            self.sessions_tab(faces, cx)
        } else {
            self.agents_tab(faces, cx)
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(48.))
            .pl(px(24.))
            .pr(px(12.))
            .border_b_1()
            .border_color(theme::border())
            .child(ui::text(faces, "Settings").text_color(theme::muted()))
            .child(ui::text(faces, "/").text_color(theme::faint()))
            .child(
                ui::text(
                    faces,
                    if q.is_empty() {
                        self.pages.tab.title()
                    } else {
                        "Search"
                    },
                )
                .flex_1()
                .weight(gpui::FontWeight::MEDIUM),
            )
            .child(
                ui::icon_button("close-settings", "x", "Close · Esc")
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            );
        div()
            .flex()
            .size_full()
            .bg(theme::bg())
            .child(nav)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(header)
                    .child(
                        div()
                            .id("settings-tab")
                            .flex()
                            .flex_col()
                            .items_center()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .child(shown.w_full()),
                    ),
            )
            .into_any_element()
    }

    /// The agents of the registry: switched on and off, installed.
    fn agents_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Div {
        let mut list = card().overflow_hidden();
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
            .filter(|e| self.settings.enabled.contains(&e.id) || !matches!(e.way(), Way::None))
            .filter(|e| {
                query.is_empty()
                    || e.name.to_lowercase().contains(&query)
                    || e.id.to_lowercase().contains(&query)
            })
            .collect();
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
            let here = self.here.get(&e.id).cloned().unwrap_or_default();
            let installed = here.launches;
            let name = self.name_of(&e.id);
            let how = match e.way() {
                Way::Archive => "binary",
                Way::Runner { runner, .. } => runner,
                Way::None => "unavailable",
            };
            // Installed, which version, signed in: as far as each is known.
            let version = here.installed.unwrap_or_else(|| e.version.clone());
            let signed = match self.slot(&e.id).map(|s| &s.auth) {
                Some(crate::hub::Auth::Ready) => " · signed in",
                Some(crate::hub::Auth::Needed(_)) => " · needs sign-in",
                _ => "",
            };
            let line = match (installed, on, e.way()) {
                (true, true, _) => format!("{version} · {how}{signed}"),
                (true, false, Way::Archive) => format!("{version} installed · off"),
                (true, false, _) => format!("runs with {how} · off"),
                (false, _, _) => "not installed".to_owned(),
            };
            let (id, page_id) = (e.id.clone(), e.id.clone());
            let control = if installed || on {
                Switch::new(SharedString::from(format!("toggle-{id}")))
                    .small()
                    .checked(on)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(&id, cx)))
                    .into_any_element()
            } else {
                ui::button(
                    SharedString::from(format!("get-{id}")),
                    "Install",
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_page(Page::Agent(id.clone()), window, cx);
                }))
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
                        &self.mark_of(&e.id),
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
                            .child(ui::fine(faces, line))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_page(Page::Agent(page_id.clone()), window, cx);
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
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .px(px(40.))
            .py(px(28.))
            .max_w(px(760.))
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
                    // Said, not chosen here: the first agent on is it.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::mark(faces, &self.mark_of(&id), 14.))
                            .child(
                                ui::text(faces, name)
                                    .text_size(px(12.5))
                                    .text_color(theme::text2()),
                            ),
                    )
                    .children(crate::keys::keys_of(cx, "new_session").into_iter().map(Kbd::new))
            }))
            .children(
                self.settings_refused
                    .clone()
                    .map(ui::trouble),
            )
            .children(
                self.registry_trouble
                    .clone()
                    .map(ui::trouble),
            )
            .when(
                self.registry.agents.is_empty() && self.registry_trouble.is_none(),
                |d| d.child(ui::fine(faces, "Reading the registry…")),
            )
            .child(
                Input::new(&self.pages.agent_search)
                    .small()
                    .prefix(ui::icon("search", 14.).text_color(theme::faint())),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        ui::button("refresh", "Read the registry again", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx))),
                    )
                    .child(div().flex_1())
                    .child(ui::fine(faces, read)),
            )
            .child(ui::fine(
                faces,
                format!(
                    "Ghost keeps its settings and the agents it installs in {}.",
                    Desk::shown(&self.home)
                ),
            ))
    }

    /// What Ghost keeps on this machine, how much, and tidying it.
    fn storage(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Div {
        use crate::tidy::Tidy;
        let mut rows = card().overflow_hidden();
        for (n, (label, bytes)) in self.pages.sizes.iter().enumerate() {
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .py(px(8.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::text(faces, label.clone()).flex_1().text_size(px(13.)))
                    .child(ui::fine(faces, crate::tidy::shown(*bytes))),
            );
        }
        let button = |id: &'static str, label: &'static str, what: Tidy| {
            ui::button(id, label, Weight::Secondary)
                .on_click(cx.listener(move |this, _, _, cx| this.tidy(what, cx)))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                ui::text(faces, "On this Mac")
                    .text_size(px(16.))
                    .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(rows)
            // What no session keeps is known only from the index: while it
            // does not read, every record, worktree and checkpoint looks so.
            .when(self.store_refused.is_none(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(button("tidy-records", "Remove records no session keeps", Tidy::Records))
                        .child(button("tidy-worktrees", "Remove worktrees no session works in", Tidy::Worktrees))
                        .child(button("tidy-checkpoints", "Remove checkpoints of earlier runs", Tidy::Checkpoints)),
                )
            })
            .children(self.pages.tidied.clone().map(|said| ui::fine(faces, said)))
            .child(ui::fine(
                faces,
                format!(
                    "In {}. A worktree with changes not committed is left as it is; undo reaches only the turns of this run.",
                    Desk::shown(&self.home)
                ),
            ))
    }

    /// Projects archived: out of the list until restored here.
    fn archived_projects(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let archived: Vec<_> = self
            .settings
            .prefs
            .iter()
            .filter(|(_, p)| p.archived)
            .map(|(f, _)| f.clone())
            .collect();
        if archived.is_empty() {
            return None;
        }
        let mut list = card().overflow_hidden();
        for (n, folder) in archived.into_iter().enumerate() {
            let name = self.project_name(&folder);
            let f = folder.clone();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::icon("folder", 16.).text_color(theme::faint()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::text(faces, name).text_size(px(13.5)))
                            .child(ui::fine(faces, Desk::shown(&folder))),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("restore-p-{n}")),
                            "Restore",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.restore_project(&f, cx))),
                    ),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    ui::text(faces, "Archived projects")
                        .text_size(px(16.))
                        .weight(gpui::FontWeight::SEMIBOLD),
                )
                .child(list),
        )
    }

    /// Sessions put away: archived ones, restored or deleted here; and what
    /// Ghost keeps of them on this machine.
    fn sessions_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Div {
        let mut list = card().overflow_hidden();
        let archived: Vec<_> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.flags.archived && !self.deleting(s.key))
            .collect();
        for (n, s) in archived.iter().enumerate() {
            let key = s.key;
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
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
                                ui::text(faces, s.meta.title.clone()).text_size(px(13.5)),
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
                            "Restore",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("delete-{key}")),
                            "Delete",
                            Weight::Quiet,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_later(key, cx))),
                    ),
            );
        }
        if archived.is_empty() {
            list = list.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .child(ui::fine(faces, "Nothing archived. A session archived from its menu waits here, read only, until it is restored.")),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .px(px(40.))
            .py(px(28.))
            .max_w(px(760.))
            .child(
                ui::text(faces, "Archived sessions")
                    .text_size(px(20.))
                    .line_height(px(28.))
                    .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(list)
            .children(self.archived_projects(faces, cx))
            .child(self.storage(faces, cx))
            .children(self.store_refused.clone().map(ui::trouble))
    }
}
