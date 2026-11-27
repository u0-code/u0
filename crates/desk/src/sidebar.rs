//! Carbon workspace sidebar (`AppSidebar` and `SidebarStates` boards): the
//! 38px unified brand/navigation header clearing the 76px traffic-light inset,
//! the 5-section bar (`Threads`, `Pull requests`, `Issues`, `Workflows`,
//! `Usage`), searchable `Projects` and `Tags` filter menus (`u0ui`), the
//! thread and project list (`list`), multi-select toolbar, and the 40px
//! `Settings` / `Update` footer.

pub(crate) mod list;
mod menus;
mod search;

pub(crate) use list::side_tree;

use std::rc::Rc;

use gpui::component::{
    InteractiveElementExt as _, Selectable as _, Sizable as _,
    button::{Button as KitButton, ButtonVariants as _},
    input::Input,
    resizable::{HandleEdge, resize_handle},
};
use gpui::{Context, Div, Focusable as _, SharedString, div, prelude::*, px};
use u0ui::{
    components::{
        badge::{Badge, BadgeSize, BadgeVariant},
        button::{Button as CarbonButton, ButtonSize, ButtonVariant},
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    tokens::ColorRole,
    u0::shell::{FilterItem, FilterMenu, FilterMenuTexts},
};

use crate::{
    desk::{Desk, Page, Place, Popup, TOPBAR, draggable, lists, updates::Updating},
    fonts::{Faces, Weighted as _},
    hub::{Host, Stage},
    pages::Tab,
    settings::Sort,
    theme, ui,
};

impl Desk {
    pub(crate) fn sidebar(
        &self,
        faces: &Faces,
        lights: f32,
        width: f32,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let typed = self.search.input.read(cx).value().trim().to_owned();
        let searching = !typed.is_empty();
        let search_shown = self.search_open || searching;

        let head = self.sidebar_head(faces, lights, cx);
        let search_bar = search_shown.then(|| self.sidebar_search_bar());
        let nav = self.sidebar_nav(cx);
        let projects = self.projects();
        let filter_bar =
            (!projects.is_empty() && !searching).then(|| self.sidebar_filter_bar(&projects, cx));
        let tag_bar = (!self.tag_filter.is_empty() && !searching).then(|| self.sidebar_tag_bar(cx));
        let store_banner = self.sidebar_banner(faces);

        let body = if projects.is_empty() {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .px(px(12.))
                .py(px(16.))
                .border_t_1()
                .border_color(theme::border())
                .child(
                    ui::text(faces, crate::tr("No projects yet"))
                        .text_size(px(crate::style::BODY))
                        .line_height(px(20.))
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::text()),
                )
                .child(
                    ui::text(
                        faces,
                        crate::tr("Projects you add show up here, with their threads."),
                    )
                    .mt(px(4.))
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(16.))
                    .text_color(theme::text2()),
                )
                .into_any_element()
        } else {
            self.side_list(faces, window, cx).into_any_element()
        };

        let sel_bar = (self.chosen.len() > 1).then(|| self.sidebar_selection_bar(faces, cx));
        let foot = self.sidebar_foot(faces, cx);

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            .overflow_hidden()
            // Folders dropped from Finder become projects on this Mac.
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                for dir in paths.paths().iter().filter(|p| p.is_dir()) {
                    this.adopt(&Place::new(Host::HERE, dir.clone()));
                }
                cx.notify();
            }))
            .child(head)
            .children(search_bar)
            .child(nav)
            .children(filter_bar)
            .children(tag_bar)
            .children(store_banner)
            .child(body)
            .children(sel_bar)
            .child(foot)
            .child(
                resize_handle::<LeftEdge, LeftEdge>("left-edge", gpui::Axis::Horizontal)
                    .inside(HandleEdge::Trailing)
                    .on_drag(LeftEdge, |edge, _, _, cx| cx.new(|_| edge.clone()))
                    .on_double_click(cx.listener(|this, _, _, cx| {
                        this.settings.left = crate::settings::LEFT;
                        this.save_settings();
                        cx.notify();
                    })),
            )
    }

    /// Unified 38px sidebar brand header (`sb-top`): clears the 76px macOS
    /// traffic lights, shows `"u0"` without a square mark, and hosts `Back`,
    /// `Ahead`, `Search`, and `New thread`.
    fn sidebar_head(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
        let desk_back = cx.entity();
        let desk_fwd = cx.entity();
        let desk_search = cx.entity();
        let desk_new = cx.entity();
        draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                ui::text(faces, "u0")
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(crate::style::BODY))
                    .weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme::text()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .child(
                        CarbonButton::new("sb-back", crate::keys::hint(cx, "Back", "back"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::ArrowLeft)
                            .icon_only(true)
                            .disabled(!self.trail.can_back())
                            .on_click(move |_, window, cx| {
                                desk_back.update(cx, |d, cx| d.step_trail(false, window, cx));
                            }),
                    )
                    .child(
                        CarbonButton::new("sb-ahead", crate::keys::hint(cx, "Ahead", "ahead"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::ArrowRight)
                            .icon_only(true)
                            .disabled(!self.trail.can_forward())
                            .on_click(move |_, window, cx| {
                                desk_fwd.update(cx, |d, cx| d.step_trail(true, window, cx));
                            }),
                    )
                    .child(
                        CarbonButton::new(
                            "sb-search",
                            crate::keys::hint(cx, "Search", "search_words"),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Search)
                        .icon_only(true)
                        .on_click(move |_, window, cx| {
                            desk_search.update(cx, |d, cx| {
                                if d.search_open && d.search.input.read(cx).value().is_empty() {
                                    d.search_open = false;
                                } else {
                                    d.search_open = true;
                                    let fh = d.search.input.focus_handle(cx);
                                    window.focus(&fh, cx);
                                }
                                cx.notify();
                            });
                        }),
                    )
                    .child(
                        CarbonButton::new(
                            "sb-new",
                            crate::keys::hint(cx, "New thread", "new_session"),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Edit)
                        .icon_only(true)
                        .on_click(move |e, window, cx| {
                            desk_new.update(cx, |d, cx| {
                                if e.modifiers().shift {
                                    d.new_default(window, cx);
                                } else {
                                    d.new_anywhere(window, cx);
                                }
                            });
                        }),
                    ),
            )
    }

    /// Carbon `field-01` inline search input (`ss-search` in `SidebarStates`).
    fn sidebar_search_bar(&self) -> Div {
        div().flex_none().px(px(12.)).pt(px(8.)).pb(px(4.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(32.))
                .px(px(10.))
                .bg(theme::role(ColorRole::Field01))
                .border_b_1()
                .border_color(theme::role(ColorRole::BorderStrong01))
                .child(
                    CarbonGlyph::carbon(CarbonIcon::Search)
                        .size_length(px(16.))
                        .color(theme::muted()),
                )
                .child(
                    div().flex_1().min_w(px(0.)).child(
                        Input::new(&self.search.input)
                            .appearance(false)
                            .cleanable(true),
                    ),
                ),
        )
    }

    /// Five-item section navigation bar (`sb-nav`): `Threads`, `Pull requests`,
    /// `Issues`, `Workflows`, `Usage`.
    fn sidebar_nav(&self, cx: &mut Context<Self>) -> Div {
        let usage = !self.pages.open && self.page == Page::Usage;
        let flows = !self.pages.open && self.page == Page::Workflows;
        let prs = !self.pages.open && self.page == Page::Forge(crate::github::Kind::Pr);
        let issues = !self.pages.open && self.page == Page::Forge(crate::github::Kind::Issue);
        let threads = !self.pages.open && self.page == Page::Thread;

        let tab =
            |id: &'static str,
             icon: CarbonIcon,
             tip: SharedString,
             current: bool,
             on_click: fn(&mut Self, &mut gpui::Window, &mut Context<Self>)| {
                div()
                    .id(id)
                    .relative()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .h(px(32.))
                    .cursor_pointer()
                    .when(current, |d| {
                        d.bg(theme::role(ColorRole::StateCurrentBackground)).child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(2.))
                                .bg(theme::role(ColorRole::BorderInteractive)),
                        )
                    })
                    .when(!current, |d| {
                        d.hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                    })
                    .tooltip(ui::tip(tip))
                    .child(
                        CarbonGlyph::carbon(icon)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconPrimary)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            };

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(12.))
            .py(px(4.))
            .child(tab(
                "nav-threads",
                CarbonIcon::Chat,
                crate::tr("Threads"),
                threads,
                |this, window, cx| {
                    this.close_settings(window, cx);
                    this.page = Page::Thread;
                    cx.notify();
                },
            ))
            .child(tab(
                "nav-prs",
                CarbonIcon::PullRequest,
                crate::keys::hint(cx, "Pull requests", "pull_requests").into(),
                prs,
                |this, window, cx| {
                    this.close_settings(window, cx);
                    this.open_forge(crate::github::Kind::Pr, window, cx);
                },
            ))
            .child(tab(
                "nav-issues",
                CarbonIcon::RadioButtonChecked,
                crate::keys::hint(cx, "Issues", "issues").into(),
                issues,
                |this, window, cx| {
                    this.close_settings(window, cx);
                    this.open_forge(crate::github::Kind::Issue, window, cx);
                },
            ))
            .child(tab(
                "nav-workflows",
                CarbonIcon::Flow,
                crate::tr("Workflows"),
                flows,
                |this, window, cx| {
                    this.open_page(Page::Workflows, window, cx);
                },
            ))
            .child(tab(
                "nav-usage",
                CarbonIcon::ChartBar,
                crate::keys::hint(cx, "Usage", "usage").into(),
                usage,
                |this, window, cx| {
                    this.open_page(Page::Usage, window, cx);
                },
            ))
    }

    /// 36px `sb-filter` bar: `Projects` and `Tags` `FilterMenu`s (`u0ui`), plus
    /// `Sort and group` and `Add project`.
    fn sidebar_filter_bar(&self, projects: &[Place], cx: &mut Context<Self>) -> Div {
        let mut folders: Vec<Place> = Vec::new();
        for p in projects {
            let p = self.listed_under(p);
            if !folders.contains(&p) {
                folders.push(p);
            }
        }
        let project_items: Vec<FilterItem> = folders
            .iter()
            .map(|p| {
                let key = place_key(p);
                let count = self
                    .sessions
                    .all
                    .iter()
                    .filter(|s| {
                        self.listed_under(&Place::project_of(s)) == *p
                            && self.listed_now(s)
                            && s.meta.flags.settled.is_none()
                    })
                    .count();
                FilterItem::new(key, self.project_name(p)).count(count)
            })
            .collect();
        let selected_projects: Vec<SharedString> =
            self.filter.iter().map(|p| place_key(p).into()).collect();
        let desk_proj = cx.entity();
        let folders_for_pick = folders;

        let tag_items: Vec<FilterItem> = self
            .all_tags()
            .into_iter()
            .map(|(tag, count)| FilterItem::new(tag.clone(), format!("#{tag}")).count(count))
            .collect();
        let selected_tags: Vec<SharedString> =
            self.tag_filter.iter().cloned().map(Into::into).collect();
        let desk_tags = cx.entity();
        let desk_add = cx.entity();

        let sort_trigger = KitButton::new("sb-sort")
            .ghost()
            .small()
            .child(ui::icon("filter_list", 16.))
            .accessibility_label(crate::tr("Sort and group"))
            .tooltip(crate::tr("Sort and group"))
            .selected(self.popup == Popup::SidebarSort);

        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(36.))
            .pl(px(8.))
            .pr(px(4.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                FilterMenu::new("sb-filter-projects", crate::tr("Projects"), project_items)
                    .value(selected_projects)
                    .search_placeholder(crate::tr("Filter projects"))
                    .texts(FilterMenuTexts {
                        dialog: Rc::new(|_| crate::tr("Filter projects")),
                        empty: crate::tr("No matching projects"),
                        select_all: crate::tr("Select all"),
                        clear: crate::tr("Clear"),
                    })
                    .on_value_change(move |vals, _, cx| {
                        let chosen: Vec<Place> = folders_for_pick
                            .iter()
                            .filter(|p| {
                                let k = place_key(p);
                                vals.iter().any(|v| v.as_ref() == k)
                            })
                            .cloned()
                            .collect();
                        desk_proj.update(cx, |d, cx| {
                            d.filter = chosen;
                            cx.notify();
                        });
                    }),
            )
            .child(
                FilterMenu::new("sb-filter-tags", crate::tr("Tags"), tag_items)
                    .value(selected_tags)
                    .search_placeholder(crate::tr("Filter tags"))
                    .texts(FilterMenuTexts {
                        dialog: Rc::new(|_| crate::tr("Filter tags")),
                        empty: crate::tr("No matching tags"),
                        select_all: crate::tr("Select all"),
                        clear: crate::tr("Clear"),
                    })
                    .on_value_change(move |vals, _, cx| {
                        let tags = vals.into_iter().map(|v| v.to_string()).collect();
                        desk_tags.update(cx, |d, cx| d.set_tag_filter(tags, cx));
                    }),
            )
            .child(div().flex_1())
            .child(
                self.menu(
                    Popup::SidebarSort,
                    sort_trigger,
                    |d, mut menu, desk, _| {
                        menu = menu.check_side(gpui::component::Side::Right);
                        for (label, sort) in [
                            ("Your message", Sort::Used),
                            ("Activity", Sort::Updated),
                            ("Begun", Sort::Begun),
                            ("Name", Sort::Name),
                        ] {
                            let on = d.settings.sort == sort;
                            menu = menu.item(
                                Self::menu_item(desk, crate::tr(label), move |this, _, cx| {
                                    this.settings.sort = sort;
                                    this.save_settings();
                                    cx.notify();
                                })
                                .checked(on),
                            );
                        }
                        menu
                    },
                    cx,
                )
                .anchor(gpui::Anchor::TopRight),
            )
            .child(
                CarbonButton::new("sb-add-project", crate::tr("Add project"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::FolderAdd)
                    .icon_only(true)
                    .on_click(move |_, window, cx| {
                        desk_add.update(cx, |d, cx| d.add_project_page(window, cx));
                    }),
            )
    }

    /// Dismissible purple `#tag` badges (`ss-tagbar` in `SidebarStates`).
    fn sidebar_tag_bar(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap(px(4.))
            .px(px(12.))
            .pt(px(4.))
            .pb(px(8.));
        for tag in &self.tag_filter {
            let desk = cx.entity();
            let t = tag.clone();
            bar = bar.child(
                Badge::new(format!("#{tag}"))
                    .id(SharedString::from(format!("sb-tag-{tag}")))
                    .variant(BadgeVariant::Purple)
                    .size(BadgeSize::Sm)
                    .dismiss_label(crate::tr("Remove tag filter"))
                    .on_dismiss(move |_, _, cx| {
                        desk.update(cx, |d, cx| d.toggle_tag_filter(&t, cx));
                    }),
            );
        }
        bar
    }

    /// 40px multi-select toolbar (`sb-selbar`): count, Settle, Archive, Clear.
    fn sidebar_selection_bar(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let keys: Vec<u64> = self.chosen.iter().copied().collect();
        let (desk_settle, desk_archive, desk_clear) = (cx.entity(), cx.entity(), cx.entity());
        let settle_keys = keys.clone();
        let archive_keys = keys;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(40.))
            .pl(px(12.))
            .pr(px(8.))
            .border_t_1()
            .border_color(theme::border())
            .bg(theme::panel2())
            .child(
                ui::text(
                    faces,
                    crate::tr_with("%{n} selected", &[("n", &self.chosen.len())]),
                )
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(crate::style::BODY))
                .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(
                CarbonButton::new("sb-sel-settle", crate::tr("Settle selected"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Checkmark)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        desk_settle.update(cx, |d, cx| {
                            d.undoable(lists::Undone::Settle, &settle_keys, cx);
                        });
                    }),
            )
            .child(
                CarbonButton::new("sb-sel-archive", crate::tr("Archive selected"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Archive)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        desk_archive.update(cx, |d, cx| {
                            d.undoable(lists::Undone::Archive, &archive_keys, cx);
                        });
                    }),
            )
            .child(
                CarbonButton::new("sb-sel-clear", crate::tr("Clear selection"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Close)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        desk_clear.update(cx, |d, cx| {
                            d.chosen.clear();
                            cx.notify();
                        });
                    }),
            )
    }

    /// 40px sidebar footer (`sb-foot`): `Settings`, compact `Update to …`
    /// prompt when an update is available, and one pill for the agents that
    /// need something: an agent's own, or their marks stacked when several
    /// do (opening Settings › Agents), so `Settings` keeps its room.
    fn sidebar_foot(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let needing: Vec<_> = self
            .slots
            .iter()
            .filter_map(|slot| {
                let word = match &slot.stage {
                    Stage::Ready | Stage::Running { .. }
                        if matches!(slot.auth, crate::hub::Auth::Needed(_)) =>
                    {
                        "Sign in"
                    }
                    Stage::Missing(_) => "Install",
                    Stage::Installing(_) => "Installing",
                    Stage::Failed(_) => "Stopped",
                    _ => return None,
                };
                Some((slot, word))
            })
            .collect();
        let pill = needing.first().map(|&(first, word)| {
            let (host, id) = (first.host, first.entry.id.clone());
            let one = needing.len() == 1;
            let shown = if needing.iter().all(|(_, w)| *w == word) {
                crate::tr(word)
            } else {
                crate::tr_with("%{n} agents", &[("n", &needing.len())])
            };
            let told = needing
                .iter()
                .map(|(slot, word)| {
                    let agent = match self.server_chip(slot.host) {
                        Some(on) => crate::tr_with(
                            "%{agent} on %{server}",
                            &[("agent", &slot.entry.name), ("server", &on)],
                        ),
                        None => slot.entry.name.clone().into(),
                    };
                    crate::tr_with(
                        "%{word}: %{agent}",
                        &[("word", &crate::tr(word)), ("agent", &agent)],
                    )
                    .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let marks = needing.iter().enumerate().fold(
                div().flex().items_center(),
                |marks, (ix, (slot, _))| {
                    marks.child(div().when(ix > 0, |d| d.ml(px(-4.))).child(ui::mark(
                        faces,
                        &self.mark_of(&slot.entry.id),
                        12.,
                    )))
                },
            );
            KitButton::new("sb-agents")
                .outline()
                .xsmall()
                .child(marks)
                .child(shown)
                .accessibility_label(told.clone())
                .tooltip(told)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if one {
                        this.open_agent(host, &id, window, cx);
                    } else {
                        this.open_tab(Tab::Agents, window, cx);
                    }
                }))
        });

        let update_btn = match &self.updates.now {
            Updating::Found(found) => Some((
                CarbonIcon::Upgrade,
                crate::tr_with("Update to %{version}", &[("version", &found.name)]),
                false,
            )),
            Updating::Installing(what) => Some((CarbonIcon::Upgrade, what.clone().into(), false)),
            Updating::Ready => Some((CarbonIcon::Restart, crate::tr("Restart to update"), true)),
            Updating::Failed(_) => Some((
                CarbonIcon::WarningFilled,
                crate::tr("Could not download update"),
                false,
            )),
            Updating::Idle | Updating::Checking | Updating::Newest => None,
        }
        .map(|(icon, label, ready)| {
            div()
                .id("sb-update")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(28.))
                .px(px(10.))
                .cursor_pointer()
                .when(ready, |d| {
                    d.bg(theme::role(ColorRole::ButtonPrimary))
                        .hover(|s| s.bg(theme::role(ColorRole::ButtonPrimaryHover)))
                        .text_color(theme::role(ColorRole::TextOnColor))
                })
                .when(!ready, |d| {
                    d.hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                        .text_color(theme::text2())
                })
                .child(
                    CarbonGlyph::carbon(icon)
                        .size_length(px(14.))
                        .color(if ready {
                            theme::role(ColorRole::IconOnColor)
                        } else {
                            theme::muted()
                        }),
                )
                .child(
                    ui::text(faces, label)
                        .text_size(px(crate::style::CAPTION))
                        .text_color(if ready {
                            theme::role(ColorRole::TextOnColor)
                        } else {
                            theme::text2()
                        }),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| match &this.updates.now {
                        Updating::Found(_) => this.install_update(cx),
                        Updating::Ready => this.reopen(window, cx),
                        _ => this.open_tab(Tab::About, window, cx),
                    }),
                )
        });

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(40.))
            .pl(px(8.))
            .pr(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .id("sb-settings")
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
                    .items_center()
                    .gap(px(12.))
                    .h(px(32.))
                    .px(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                    .tooltip(ui::tip(crate::keys::hint(cx, "Settings", "settings")))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Settings)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconPrimary)),
                    )
                    .child(
                        ui::text(faces, crate::tr("Settings"))
                            .truncate()
                            .text_size(px(crate::style::BODY))
                            .text_color(theme::text()),
                    )
                    .on_click(|_, window, cx| {
                        if let Some(action) = crate::keys::action("settings") {
                            window.dispatch_action(action, cx);
                        }
                    }),
            )
            .children(update_btn)
            .children(pill)
    }
}

fn place_key(p: &Place) -> String {
    format!("{}:{}", p.host.0, p.folder.display())
}

/// The sidebar's edge, dragged.
#[derive(Clone)]
pub struct LeftEdge;

impl gpui::Render for LeftEdge {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A session's row being dragged, as it shows under the pointer.
#[derive(Clone)]
pub(crate) struct Dragged {
    pub key: u64,
    title: SharedString,
}

impl gpui::Render for Dragged {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(5.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .shadow_md()
            .text_size(px(crate::style::LABEL))
            .text_color(theme::text())
            .child(self.title.clone())
    }
}

/// What an agent said, as a line of plain words for a hover tooltip: prose
/// only (no code blocks, drawn trees or tables).
pub(crate) fn gist(said: &str) -> String {
    let mut words = String::new();
    let mut code = false;
    for line in said.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        let drawn = line.contains(['│', '├', '└', '┌', '─']) || line.starts_with('|');
        if code || drawn || line.is_empty() {
            continue;
        }
        let line = line
            .trim_start_matches(['#', '>', '-', '*', ' '])
            .replace(['`', '*'], "");
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(line.trim());
        if words.chars().count() > 120 {
            break;
        }
    }
    ui::fit(&words, 110)
}

#[cfg(test)]
mod tests {
    use super::gist;

    #[test]
    fn a_cards_words_are_the_prose_without_code_trees_or_marks() {
        let said = "这次调研生成的文件都在 `/Users/x/jepa` 下：\n```\n/Users/x/jepa/\n├── README.md\n└── reports/\n```\n## 结论\n- **核心思想**：JEPA 在表征空间里预测。";
        assert_eq!(
            gist(said),
            "这次调研生成的文件都在 /Users/x/jepa 下： 结论 核心思想：JEPA 在表征空间里预测。"
        );
    }
}
