//! The settings page: a column of tabs, and the tab chosen. Ghost's own
//! settings (`prefs`); agents, which are switched on and off and installed;
//! sessions archived, and what Ghost keeps on this machine.

use std::time::SystemTime;

use gpui::{Context, SharedString, div, prelude::*, px};

use super::{card, fine};
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page},
    fonts::Faces,
    registry::Way,
    sidebar::kbd,
    theme,
    ui::{self, Mark, Weight},
    when,
};

/// A tab of the settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Chat,
    NewSessions,
    Agents,
    Sessions,
    Keyboard,
    About,
}

impl Desk {
    pub(crate) fn settings_page(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut nav = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(220.))
            .gap(px(2.))
            .p(px(8.))
            .border_r_1()
            .border_color(theme::border());
        for (tab, label) in [
            (Tab::General, "General"),
            (Tab::Appearance, "Appearance"),
            (Tab::Chat, "Chat"),
            (Tab::NewSessions, "New sessions"),
            (Tab::Agents, "Agents"),
            (Tab::Sessions, "Sessions"),
            (Tab::Keyboard, "Keyboard"),
            (Tab::About, "About"),
        ] {
            let on = self.settings_tab == tab;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("tab-{label}")))
                    .flex()
                    .items_center()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .when(!on, |d| d.hover(|s| s.bg(theme::hover())))
                    .child(ui::text(faces, label).text_size(px(13.5)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_tab = tab;
                        if tab == Tab::Sessions {
                            this.measure();
                        }
                        cx.notify();
                    })),
            );
        }
        let own = self.own_tab(faces, self.settings_tab, cx);
        if self.settings_tab == Tab::Sessions || own.is_some() {
            let tab = match own {
                Some(page) => page,
                None => self.sessions_tab(faces, cx),
            };
            return div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .child(nav)
                .child(
                    div()
                        .id("settings")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_y_scroll()
                        .child(tab),
                )
                .into_any_element();
        }

        let mut list = card().overflow_hidden();
        // What runs here, and what answers the search, from the registry.
        let query = self.agent_search.read(cx).text().trim().to_lowercase();
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
            let installed = e.launch(&self.home).is_ok();
            let name = self.name_of(&e.id);
            let how = match e.way() {
                Way::Archive => "binary",
                Way::Runner { runner, .. } => runner,
                Way::None => "unavailable",
            };
            // Installed, which version, signed in: as far as each is known.
            let version = e.installed(&self.home).unwrap_or_else(|| e.version.clone());
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
                    .map(|why| ui::trouble(faces, why)),
            )
            .children(
                self.registry_trouble
                    .clone()
                    .map(|why| ui::trouble(faces, why)),
            )
            .when(
                self.registry.agents.is_empty() && self.registry_trouble.is_none(),
                |d| d.child(fine(faces, "Reading the registry…")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(32.))
                    .px(px(8.))
                    .gap(px(6.))
                    .rounded(px(8.))
                    .bg(theme::well())
                    .border_1()
                    .border_color(theme::border2())
                    .child(ui::icon("search", 14.).text_color(theme::faint()))
                    .child(div().flex_1().child(self.agent_search.clone())),
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
        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .child(nav)
            .child(
                div()
                    .id("settings")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }

    /// What Ghost keeps on this machine, how much, and tidying it.
    fn storage(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Div {
        use crate::tidy::Tidy;
        let mut rows = card().overflow_hidden();
        for (n, (label, bytes)) in self.sizes.iter().enumerate() {
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .py(px(8.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::text(faces, *label).flex_1().text_size(px(13.)))
                    .child(fine(faces, crate::tidy::shown(*bytes))),
            );
        }
        let button = |id: &'static str, label: &'static str, what: Tidy| {
            ui::button(faces, id, label, Weight::Secondary)
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
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(button("tidy-records", "Remove records no session keeps", Tidy::Records))
                    .child(button("tidy-worktrees", "Remove worktrees no session works in", Tidy::Worktrees))
                    .child(button("tidy-checkpoints", "Remove checkpoints of earlier runs", Tidy::Checkpoints)),
            )
            .children(self.tidied.clone().map(|said| fine(faces, said)))
            .child(fine(
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
                            .child(fine(faces, Desk::shown(&folder))),
                    )
                    .child(
                        ui::button(
                            faces,
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
            .iter()
            .filter(|s| s.flags.archived && !self.deleting(s.key))
            .collect();
        for (n, s) in archived.iter().enumerate() {
            let key = s.key;
            let name = self.name_of(&s.agent);
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::mark(faces, &Mark::of(&s.agent, &name), 16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::one_line(
                                ui::text(faces, s.title.clone()).text_size(px(13.5)),
                            ))
                            .child(fine(
                                faces,
                                format!(
                                    "{} · {}",
                                    Desk::shown(&s.project),
                                    when::ago(s.updated, SystemTime::now())
                                ),
                            )),
                    )
                    .child(
                        ui::button(
                            faces,
                            SharedString::from(format!("restore-{key}")),
                            "Restore",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
                    )
                    .child(
                        ui::button(
                            faces,
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
                    .child(fine(faces, "Nothing archived. A session archived from its menu waits here, read only, until it is restored.")),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .px(px(40.))
            .py(px(28.))
            .max_w(px(720.))
            .child(
                ui::text(faces, "Archived sessions")
                    .text_size(px(20.))
                    .line_height(px(28.))
                    .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(list)
            .children(self.archived_projects(faces, cx))
            .child(self.storage(faces, cx))
            .children(
                self.store_refused
                    .clone()
                    .map(|why| ui::trouble(faces, why)),
            )
    }
}
