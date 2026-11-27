//! The settings dialog (`SettingsSidebar` board + Carbon dialog shell): a
//! `1080×820` modal over the window with the `232px` `layer-01` page list down
//! the left (`This app`, the `layer-02` per-machine box headed by the inline
//! `Machine` picker, and `u0`) and the `720px`-centered Carbon page on the
//! right.

use std::time::SystemTime;

use gpui::component::{
    Sizable as _,
    input::Input,
    kbd::Kbd,
    setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings},
    switch::Switch,
};
use gpui::{
    AnyElement, App, Context, Div, Pixels, SharedString, WeakEntity, Window, div, prelude::*, px,
    rgb,
};
use u0ui::{
    components::{
        badge::{Badge, BadgeSize},
        button::{Button as CarbonButton, ButtonSize, ButtonVariant},
        form::FieldSize,
        list_box::ListItem,
        select::{Select, SelectType},
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::workspace::settings::SettingsBadgeTone,
    theme::LayerLevel,
    tokens::ColorRole,
};

use super::prefs::{group, item, whole};
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page},
    fonts::Faces,
    hub::{Auth, Host, Stage, Up},
    registry::Way,
    theme,
    ui::{self, Weight},
    when,
};

/// How tall the settings dialog stands in `window`.
pub fn height(window: &Window) -> Pixels {
    (window.viewport_size().height - px(80.)).min(px(crate::style::SETTINGS_DIALOG_MAX_HEIGHT))
}

/// A tab of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Chat,
    NewThreads,
    Writing,
    Agents,
    Archive,
    Keybindings,
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
    /// Each tab, in canonical index order.
    pub const ALL: [Self; 17] = [
        Self::General,
        Self::Devices,
        Self::Appearance,
        Self::Chat,
        Self::Keybindings,
        Self::Projects,
        Self::SourceControl,
        Self::Agents,
        Self::NewThreads,
        Self::Writing,
        Self::Tasks,
        Self::Workflows,
        Self::Archive,
        Self::Usage,
        Self::Diagnostics,
        Self::Telemetry,
        Self::About,
    ];

    const APP_TABS: [Self; 4] = [
        Self::General,
        Self::Appearance,
        Self::Chat,
        Self::Keybindings,
    ];

    const MACHINE_GROUPS: [(&'static str, &'static [Self]); 4] = [
        (
            "Agents",
            &[
                Self::Agents,
                Self::NewThreads,
                Self::Writing,
                Self::Tasks,
                Self::Workflows,
                Self::Archive,
                Self::Usage,
            ],
        ),
        ("Code", &[Self::Projects, Self::SourceControl]),
        ("Access", &[Self::Devices]),
        ("Upkeep", &[Self::Diagnostics]),
    ];

    const U0_TABS: [Self; 2] = [Self::Telemetry, Self::About];

    /// Its page heading, in your language.
    pub fn title(self) -> SharedString {
        crate::tr(match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::NewThreads => "New threads",
            Self::Writing => "Writing",
            Self::Agents => "Agents",
            Self::Archive => "Archive",
            Self::Keybindings => "Keybindings",
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

    /// Its label in the settings sidebar (`SettingsSidebar.dc.html`).
    fn nav_label(self) -> SharedString {
        match self {
            Self::SourceControl => crate::tr("Source control & issues"),
            Self::Diagnostics => crate::tr("Maintenance & diagnostics"),
            _ => self.title(),
        }
    }

    /// Material Symbols Sharp (`CarbonIcon`) glyph for the sidebar list.
    pub fn carbon_icon(self) -> CarbonIcon {
        match self {
            Self::General => CarbonIcon::Settings,
            Self::Appearance => CarbonIcon::ColorPalette,
            Self::Chat => CarbonIcon::Chat,
            Self::Keybindings => CarbonIcon::Keyboard,
            Self::Agents => CarbonIcon::Bot,
            Self::NewThreads => CarbonIcon::Add,
            Self::Writing => CarbonIcon::Edit,
            Self::Tasks => CarbonIcon::Calendar,
            Self::Workflows => CarbonIcon::Flow,
            Self::Archive => CarbonIcon::Archive,
            Self::Usage => CarbonIcon::ChartBar,
            Self::Projects => CarbonIcon::Folder,
            Self::SourceControl => CarbonIcon::Branch,
            Self::Devices => CarbonIcon::Laptop,
            Self::Diagnostics => CarbonIcon::Activity,
            Self::Telemetry => CarbonIcon::View,
            Self::About => CarbonIcon::Information,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::General => "sliders",
            Self::Appearance => "sun",
            Self::Chat => "reply",
            Self::NewThreads => "plus",
            Self::Writing => "pencil",
            Self::Agents => "plug",
            Self::Archive => "archive",
            Self::Keybindings => "keyboard",
            Self::Projects => "folder",
            Self::Usage => "gauge",
            Self::About => "u0",
            Self::SourceControl => "branch",
            Self::Diagnostics => "text-search",
            Self::Devices => "laptop",
            Self::Tasks => "clock",
            Self::Workflows => "list",
            Self::Telemetry => "gauge",
        }
    }

    /// Whether this page sits inside the per-machine box (`u0-setbox`).
    pub fn is_per_machine(self) -> bool {
        !matches!(
            self,
            Self::General
                | Self::Appearance
                | Self::Chat
                | Self::Keybindings
                | Self::Telemetry
                | Self::About
        )
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }
}

impl Desk {
    /// Legacy `SettingPage` builder for tabs not yet migrated to `carbon_tab`.
    fn tab_page(&self, tab: Tab, w: &WeakEntity<Desk>, cx: &App) -> SettingPage {
        let banners = self.settings_banner_group(tab, w);
        let paged = |lede: &str, mut groups: Vec<SettingGroup>| {
            if let Some(b) = banners.clone() {
                groups.insert(0, b);
            }
            SettingPage::new(tab.title())
                .description(crate::tr(lede))
                .groups(groups)
        };
        let page = match tab {
            Tab::Agents => paged(
                "Turn on the agents you use. Each runs its own CLI over ACP, with its own sign-in.",
                self.agents(w, cx),
            ),
            Tab::Archive => paged(
                "Threads put away, and what u0 keeps on this Mac.",
                self.sessions_tab(w),
            ),
            Tab::Usage | Tab::Workflows => SettingPage::new(tab.title()),
            Tab::Diagnostics => paged(
                "What u0 runs on this Mac, and what went wrong.",
                self.diagnostics(w),
            ),
            Tab::Tasks => paged(
                "A prompt said to an agent on a schedule. u0's server runs them, so they run while the window is closed; each run's thread is in the sidebar.",
                self.tasks(w),
            ),
            Tab::Telemetry => paged(
                "What the machine these settings are of tells the people who make u0, as far as you let it.",
                self.telemetry_tab(w),
            ),
            Tab::Projects => self.projects_tab(w, banners),
            Tab::Devices => self.devices_tab(w, banners),
            _ => self
                .own_tab(tab, w, banners, cx)
                .unwrap_or_else(|| SettingPage::new(tab.title())),
        };
        page.icon(ui::glyph(tab.icon()))
    }

    /// Select `tab` inside the open settings dialog.
    pub(crate) fn pick_settings_tab(
        &mut self,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pages.tab != tab {
            self.pages.tab = tab;
            let page_ix = tab.index();
            self.pages.settings.update(cx, |s, cx| {
                s.set_selected_index(
                    SelectIndex {
                        page_ix,
                        group_ix: None,
                    },
                    cx,
                );
            });
            self.tab_shown(tab, window, cx);
            cx.notify();
        }
    }

    /// Severity-only badge for `tab` (`SettingsSidebar.dc.html`: yellow = needs
    /// sign-in / setup, red = failed task or security / turned away; plain
    /// counts are never shown).
    fn tab_badge(&self, tab: Tab) -> Option<(usize, SettingsBadgeTone)> {
        match tab {
            Tab::Agents => {
                let n = self
                    .slots_on(self.pages.on)
                    .filter(|s| {
                        matches!(s.auth, Auth::Needed(_))
                            || matches!(s.stage, Stage::Missing(_) | Stage::Failed(_))
                    })
                    .count();
                (n > 0).then_some((n, SettingsBadgeTone::Warning))
            }
            Tab::Tasks => {
                let n = match self.tasks_on(self.pages.on) {
                    Some(Ok(list)) => list
                        .iter()
                        .filter(|t| t.last.as_ref().is_some_and(|r| r.error.is_some()))
                        .count(),
                    Some(Err(_)) => 1,
                    None => 0,
                };
                (n > 0).then_some((n, SettingsBadgeTone::Error))
            }
            Tab::Devices => {
                let refused = self
                    .machines
                    .iter()
                    .skip(1)
                    .filter(|m| matches!(m.up, Up::Refused(_)))
                    .count();
                let waiting = self
                    .pages
                    .shared
                    .as_ref()
                    .and_then(|s| s.devices.as_ref().ok())
                    .map_or(0, |devs| devs.iter().filter(|d| d.waiting).count());
                let n = refused + waiting;
                (n > 0).then_some((n, SettingsBadgeTone::Error))
            }
            _ => None,
        }
    }

    /// Carbon `1080×820` settings dialog shell (`SettingsSidebar` + right pane).
    pub(crate) fn settings_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self
            .pages
            .settings_search
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let state_ix = self.pages.tab.index();
        self.pages.settings.update(cx, |s, cx| {
            if s.selected_index().page_ix != state_ix {
                s.set_selected_index(
                    SelectIndex {
                        page_ix: state_ix,
                        group_ix: None,
                    },
                    cx,
                );
            }
            s.set_query(query.clone(), window, cx);
        });

        let faces = crate::fonts::faces(cx).clone();
        let nav = self.settings_sidebar(&faces, &query, cx);
        let scope_hint = if self.pages.tab.is_per_machine() {
            let mname = self
                .machine(self.pages.on)
                .map_or_else(|| crate::tr("This Mac").to_string(), |m| m.name.clone());
            crate::tr_with(
                "Machine: %{name} · ⌘, opens settings, Esc closes them",
                &[("name", &mname)],
            )
        } else {
            crate::tr("This app · ⌘, opens settings, Esc closes them")
        };
        let desk_close = cx.entity();
        let top_bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(48.))
            .pl(px(32.))
            .pr(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .bg(theme::bg())
            .child(
                ui::text(&faces, scope_hint)
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(16.))
                    .text_color(theme::text2()),
            )
            .child(
                CarbonButton::new("close-settings", crate::tr("Close settings (Esc)"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Close)
                    .icon_only(true)
                    .on_click(move |_, window, cx| {
                        desk_close.update(cx, |this, cx| this.close_settings(window, cx));
                    }),
            );

        let body: AnyElement =
            if let Some(carbon) = self.carbon_tab(self.pages.tab, &query, &faces, cx) {
                carbon
            } else {
                let w = cx.entity().downgrade();
                let mut settings = Settings::new("settings")
                    .state(&self.pages.settings)
                    .sidebar_width(px(100.))
                    .sidebar_size_range(px(100.)..px(100.));
                for tab in Tab::ALL {
                    settings = settings.page(self.tab_page(tab, &w, cx));
                }
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(div().h_full().ml(px(-101.)).child(settings))
                    .into_any_element()
            };

        div()
            .relative()
            .flex()
            .size_full()
            .bg(theme::bg())
            .overflow_hidden()
            .child(nav)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .child(top_bar)
                    .child(body),
            )
            .children(self.theme_editor_drawer(cx))
            .into_any_element()
    }

    /// `232px` Carbon settings sidebar (`SettingsSidebar.dc.html`).
    fn settings_sidebar(&self, faces: &Faces, query: &str, cx: &mut Context<Self>) -> Div {
        let q = query.to_lowercase();
        let tab_matches = |tab: Tab| {
            q.is_empty() || tab.nav_label().to_lowercase().contains(&q) || tab == self.pages.tab
        };

        let group_head = |label: SharedString| {
            ui::text(faces, label)
                .px(px(16.))
                .pb(px(8.))
                .text_size(px(crate::style::CAPTION))
                .line_height(px(16.))
                .weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::text2())
        };

        let app_items: Vec<AnyElement> = Tab::APP_TABS
            .into_iter()
            .filter(|&t| tab_matches(t))
            .map(|t| {
                self.settings_nav_row(faces, t, false, cx)
                    .into_any_element()
            })
            .collect();

        let machine_items: Vec<ListItem> = self
            .machines
            .iter()
            .map(|m| ListItem::labeled(m.host.0.to_string(), m.name.clone()))
            .collect();
        let hosts: Vec<Host> = self.machines.iter().map(|m| m.host).collect();
        let desk_machine = cx.entity();
        let machine_select = Select::new("settings-machine")
            .label(crate::tr("Machine"))
            .kind(SelectType::Inline)
            .size(FieldSize::Sm)
            .layer(LayerLevel::Two)
            .readonly(self.machines.len() < 2)
            .items(machine_items)
            .value(Some(self.pages.on.0.to_string()))
            .on_value_change(move |val, window, cx| {
                if let Some(raw) = val
                    && let Ok(h) = raw.parse::<u16>()
                    && hosts.contains(&Host(h))
                {
                    desk_machine.update(cx, |this, cx| {
                        this.pages.on = Host(h);
                        this.pages.project = None;
                        this.pages.scope = None;
                        this.tab_shown(this.pages.tab, window, cx);
                        cx.notify();
                    });
                }
            });

        let mut machine_groups: Vec<AnyElement> = Vec::new();
        for (g_ix, (heading, tabs)) in Tab::MACHINE_GROUPS.into_iter().enumerate() {
            let mut rows: Vec<AnyElement> = Vec::new();
            for &tab in tabs {
                if tab_matches(tab) {
                    rows.push(
                        self.settings_nav_row(faces, tab, true, cx)
                            .into_any_element(),
                    );
                }
                if tab == Tab::Agents {
                    for slot in self.slots_on(self.pages.on) {
                        let id = slot.entry.id.clone();
                        let name = self.name_of(&id);
                        if !q.is_empty() && !name.to_lowercase().contains(&q) {
                            continue;
                        }
                        rows.push(self.settings_agent_nav_row(faces, self.pages.on, id, name, cx));
                    }
                }
            }
            if !rows.is_empty() {
                machine_groups.push(
                    div()
                        .flex()
                        .flex_col()
                        .when(g_ix > 0, |d| d.mt(px(20.)))
                        .child(group_head(crate::tr(heading)))
                        .children(rows)
                        .into_any_element(),
                );
            }
        }

        let u0_items: Vec<AnyElement> = Tab::U0_TABS
            .into_iter()
            .filter(|&t| tab_matches(t))
            .map(|t| {
                self.settings_nav_row(faces, t, false, cx)
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(crate::style::SETTINGS_NAV_WIDTH))
            .h_full()
            .bg(theme::role(ColorRole::Layer01))
            .border_r_1()
            .border_color(theme::border())
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(48.))
                    .px(px(16.))
                    .child(
                        ui::text(faces, crate::tr("Settings"))
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::text()),
                    ),
            )
            .child(
                div().flex_none().px(px(8.)).pb(px(16.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(32.))
                        .px(px(8.))
                        .bg(theme::role(ColorRole::Field02))
                        .border_b_1()
                        .border_color(theme::role(ColorRole::BorderStrong01))
                        .child(
                            CarbonGlyph::carbon(CarbonIcon::Search)
                                .size_length(px(16.))
                                .color(theme::role(ColorRole::IconSecondary))
                                .flex_none(),
                        )
                        .child(
                            div().flex_1().min_w(px(0.)).child(
                                Input::new(&self.pages.settings_search)
                                    .appearance(false)
                                    .cleanable(true),
                            ),
                        ),
                ),
            )
            .child(
                div()
                    .id("settings-nav-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .pt(px(4.))
                    .pb(px(24.))
                    .overflow_y_scroll()
                    .when(!app_items.is_empty(), |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(group_head(crate::tr("This app")))
                                .children(app_items),
                        )
                    })
                    .when(!machine_groups.is_empty(), |d| {
                        d.child(
                            div()
                                .my(px(20.))
                                .pt(px(12.))
                                .pb(px(16.))
                                .bg(theme::role(ColorRole::Layer02))
                                .border_t_1()
                                .border_b_1()
                                .border_color(theme::border())
                                .child(div().px(px(16.)).pb(px(4.)).child(machine_select))
                                .child(
                                    ui::text(
                                        faces,
                                        crate::tr("Each page here is set per machine."),
                                    )
                                    .px(px(16.))
                                    .pb(px(16.))
                                    .text_size(px(crate::style::CAPTION))
                                    .line_height(px(16.))
                                    .text_color(theme::text2()),
                                )
                                .children(machine_groups),
                        )
                    })
                    .when(!u0_items.is_empty(), |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(group_head(crate::tr("u0")))
                                .children(u0_items),
                        )
                    }),
            )
    }

    fn settings_nav_row(
        &self,
        faces: &Faces,
        tab: Tab,
        on_layer2: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let active = self.pages.tab == tab;
        let selected_bg = theme::role(if on_layer2 {
            ColorRole::LayerSelected02
        } else {
            ColorRole::LayerSelected01
        });
        let hover_bg = theme::role(if on_layer2 {
            ColorRole::LayerHover02
        } else {
            ColorRole::LayerHover01
        });
        let badge = self.tab_badge(tab).map(|(n, tone)| {
            let (bg, fg) = match tone {
                SettingsBadgeTone::Warning => (
                    theme::role(ColorRole::SupportWarning),
                    rgb(0x0016_1616).into(),
                ),
                _ => (
                    theme::role(ColorRole::SupportError),
                    if theme::is_light() {
                        gpui::white()
                    } else {
                        rgb(0x0016_1616).into()
                    },
                ),
            };
            Badge::new(n.to_string()).size(BadgeSize::Sm).colors(bg, fg)
        });
        let label = tab.nav_label();
        div()
            .id(SharedString::from(format!("snav-{tab:?}")))
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .h(px(32.))
            .px(px(16.))
            .cursor_pointer()
            .when(active, |d| {
                d.bg(selected_bg).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(theme::role(ColorRole::BorderInteractive)),
                )
            })
            .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
            .child(
                CarbonGlyph::carbon(tab.carbon_icon())
                    .size_length(px(16.))
                    .color(if active {
                        theme::text()
                    } else {
                        theme::text2()
                    })
                    .flex_none(),
            )
            .child(
                ui::text(faces, label)
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(crate::style::BODY))
                    .weight(if active {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if active {
                        theme::text()
                    } else {
                        theme::text2()
                    }),
            )
            .children(badge)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.pick_settings_tab(tab, window, cx);
            }))
    }

    fn settings_agent_nav_row(
        &self,
        faces: &Faces,
        host: Host,
        id: String,
        name: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = self;
        let hover_bg = theme::role(ColorRole::LayerHover02);
        div()
            .id(SharedString::from(format!("snav-agent-{}-{id}", host.0)))
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .h(px(32.))
            .px(px(16.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .child(
                CarbonGlyph::carbon(CarbonIcon::Chip)
                    .size_length(px(16.))
                    .color(theme::text2())
                    .flex_none(),
            )
            .child(
                ui::text(faces, name)
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(crate::style::BODY))
                    .text_color(theme::text2()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_page(Page::Agent(host, id.clone()), window, cx);
            }))
            .into_any_element()
    }

    /// The agents of the registry: switched on and off, installed.
    fn agents(&self, w: &WeakEntity<Desk>, cx: &App) -> Vec<SettingGroup> {
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
            head.push(item(
                w,
                crate::tr("Default for new threads"),
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
        if let Some(trouble) = self.registry_trouble.clone() {
            let empty_reg = self.registry.agents.is_empty();
            head.push(whole(w, &[], move |_, _, cx| {
                use u0ui::components::alert::{AlertAction, AlertVariant};
                use u0ui::components::empty::{
                    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
                };
                use u0ui::prelude::*;
                if empty_reg {
                    Empty::new()
                        .child(
                            EmptyHeader::new()
                                .child(EmptyTitle::new(crate::tr("Registry not read yet")))
                                .child(EmptyDescription::new(trouble.clone())),
                        )
                        .child(
                            EmptyContent::new().child(
                                Button::new(
                                    "empty-refresh-registry",
                                    crate::tr("Read the registry again"),
                                )
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::Renew)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx))),
                            ),
                        )
                        .into_any_element()
                } else {
                    let desk = cx.entity();
                    let again = AlertAction::new(crate::tr("Read the registry again"))
                        .on_click(move |_, cx| desk.update(cx, Self::refresh_registry));
                    crate::desk::banners::banner(
                        "registry-trouble",
                        AlertVariant::Error,
                        crate::tr("Registry not read yet"),
                        trouble.clone(),
                        None,
                        Some(again),
                    )
                    .into_any_element()
                }
            }));
        }
        if self.registry.agents.is_empty() && self.registry_trouble.is_none() {
            head.push(whole(w, &[], |_, _, _| {
                use u0ui::components::spinner::{Spinner, SpinnerSize};
                Spinner::new("registry-reading")
                    .size(SpinnerSize::Inline)
                    .description(crate::tr("Reading the registry…"))
                    .into_any_element()
            }));
        }
        let host = self.pages.on;
        for slot in self.slots_on(host) {
            if let crate::hub::Stage::Missing(m @ crate::registry::Missing::Runner(_)) = &slot.stage
            {
                let (agent_id, name) = (slot.entry.id.clone(), self.name_of(&slot.entry.id));
                let body = m.say(&name);
                head.push(whole(w, &[], move |_, _, cx| {
                    use u0ui::components::alert::{AlertAction, AlertVariant};
                    let desk = cx.entity();
                    let id = agent_id.clone();
                    let again = AlertAction::new(crate::tr("Try again")).on_click(move |_, cx| {
                        let id = id.clone();
                        desk.update(cx, |this, cx| this.recheck(host, &id, cx));
                    });
                    crate::desk::banners::banner(
                        SharedString::from(format!("missing-runner-{agent_id}")),
                        AlertVariant::Error,
                        crate::tr_with("%{name} cannot start yet", &[("name", &name)]),
                        body.clone(),
                        None,
                        Some(again),
                    )
                    .into_any_element()
                }));
            }
        }
        let acts = self.settings_on(host).outside_acts;
        head.push(item(
            w,
            crate::tr("Agents outside u0 may act"),
            crate::tr("An agent outside u0 reaches u0's tools by the key in mcp.json (u0 --mcp, or over HTTP on this Mac). Off, it only reads; on, it may say something in a thread or stop its turn, each asked of you there."),
            move |_, _, cx| {
                Switch::new("outside-acts")
                    .accessibility_label(crate::tr("Agents outside u0 may act"))
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
        head.push(item(
            w,
            crate::tr("Key for agents outside u0"),
            crate::tr("An app set up with u0's tools (mcp.json's address and key) keeps working across restarts: the key stays until you make a new one, and the old one then lets no one in."),
            move |_, _, cx| {
                gpui::component::button::Button::new("new-tools-key")
                    .small()
                    .label(crate::tr("New key"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.call(host, crate::proto::Call::NewToolsKey);
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
                "u0 keeps its settings and the agents it installs in %{home}.",
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

    fn agent_row(&self, faces: &Faces, id: &str, cx: &mut Context<Self>) -> Div {
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
        let version = here.installed.unwrap_or_else(|| e.version.clone());
        let line = match (
            installed,
            on,
            e.way(),
            self.slot(host, &e.id).map(|s| &s.auth),
        ) {
            (true, true, _, Some(Auth::Ready)) => crate::tr_with(
                "%{version} · %{how} · signed in",
                &[("version", &version), ("how", &how)],
            ),
            (true, true, _, Some(Auth::Needed(_))) => crate::tr_with(
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

    fn storage(&self, w: &WeakEntity<Desk>) -> SettingGroup {
        use crate::tidy::Tidy;
        let free = self.pages.sizes.free_bytes;
        let mut items: Vec<SettingItem> = (self.pages.sizes.buckets.iter())
            .map(|b| {
                let size = match b.cleanable {
                    Some(c) if c > 0 => {
                        format!(
                            "{} ({})",
                            crate::tidy::shown(b.bytes),
                            crate::tidy::shown(c)
                        )
                    }
                    _ => crate::tidy::shown(b.bytes),
                };
                let detail = if free > 0 && b.label == "Worktrees" {
                    format!("{} · {}", b.detail, crate::tidy::shown(free))
                } else {
                    b.detail.clone()
                };
                item(w, b.label.clone(), detail, move |_, faces, _| {
                    ui::fine(faces, size.clone()).into_any_element()
                })
            })
            .collect();
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
                            "Remove records no thread keeps",
                            Tidy::Records,
                        ))
                        .child(button(
                            "tidy-worktrees",
                            "Remove worktrees no thread works in",
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
        group(crate::tr("What u0 keeps"), items).description(crate::tr_with(
            "In %{home}. A worktree with changes not committed is left as it is; undo reaches only the turns of this run.",
            &[("home", &Desk::shown(self.home_on(self.pages.on)))],
        ))
    }

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
                    "A thread archived from its menu waits here, read only, until it is restored.",
                ),
                |_, _, _| div().into_any_element(),
            ));
        }
        let mut groups = vec![group(crate::tr("Archived threads"), list)];
        groups.extend(self.archived_projects(w));
        groups.push(self.storage(w));
        groups
    }

    fn archived_row(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.sessions.at(key) else {
            return div();
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
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
