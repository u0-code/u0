//! The PRs and Issues pages drawn: the choices and the list on the left,
//! the one read on the right, as t3code's pages set them.

use gpui::component::{
    Sizable as _,
    input::{Input, Textarea},
    list::List,
    menu::ContextMenuExt as _,
    scroll::ScrollableElement as _,
    tab::{Tab as KitTab, TabBar},
    tag::Tag,
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

mod menus;

use crate::{
    desk::{
        Desk, Popup,
        forge::{PARTS, Tab},
    },
    fonts::Faces,
    github::{Act, Detail, Item, Kind},
    markdown, theme,
    ui::{self, Weight},
    when,
};

/// A state's colour: open green, merged violet-blue, closed coral, a
/// draft muted.
fn tint(item: &Item) -> gpui::Hsla {
    match (item.state.as_str(), item.draft) {
        (_, true) => theme::muted(),
        ("open", _) => theme::ok(),
        ("merged", _) => theme::link(),
        _ => theme::coral(),
    }
}

fn when_of(text: &str) -> String {
    when::parse(text).map_or_else(String::new, |t| when::ago(t, std::time::SystemTime::now()))
}

/// A state as the filter says it, in sentence case as its neighbours.
pub fn state_word(state: &str) -> &'static str {
    match state {
        "open" => "Open",
        "closed" => "Closed",
        "merged" => "Merged",
        _ => "All",
    }
}

impl Desk {
    pub(crate) fn forge_page(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let states: &[&'static str] = match f.kind {
            Kind::Pr => &["open", "closed", "merged", "all"],
            Kind::Issue => &["open", "closed", "all"],
        };
        let state_picks: Vec<(SharedString, bool, ui::Pick)> = states
            .iter()
            .map(|st| {
                let desk = cx.entity();
                let pick: ui::Pick = Box::new(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.forge.state = st;
                        d.list_forge(cx);
                    });
                });
                (crate::tr(state_word(st)), f.state == *st, pick)
            })
            .collect();
        let part_picks: Vec<(SharedString, bool, ui::Pick)> = PARTS
            .iter()
            .enumerate()
            .filter(|(n, _)| f.kind == Kind::Pr || *n != 2)
            .map(|(n, (name, _))| {
                let desk = cx.entity();
                let pick: ui::Pick = Box::new(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.forge.part = n;
                        d.list_forge(cx);
                    });
                });
                (crate::tr(name), f.part == n, pick)
            })
            .collect();
        let project = match &f.folder {
            _ if f.every.is_some() => crate::tr("All projects").to_string(),
            Some(p) => self.forge_name(p),
            None => crate::tr("No project").to_string(),
        };
        let project_label = project.clone();
        let chip = ui::opener("forge-project")
            .h(px(26.))
            .px(px(8.))
            .child(ui::text(faces, project).text_size(px(crate::style::LABEL)))
            .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                self.menu(
                    Popup::ForgeProject,
                    chip,
                    |d, menu, desk, _| d.forge_projects(menu, desk),
                    cx,
                )
                .anchor(gpui::Anchor::TopLeft),
            )
            .child(ui::segmented("forge-state", state_picks))
            .child(ui::segmented("forge-part", part_picks))
            .child(
                self.menu(
                    Popup::ForgeViews,
                    ui::opener("forge-views")
                        .h(px(26.))
                        .px(px(8.))
                        .child(
                            ui::text(faces, crate::tr("Views")).text_size(px(crate::style::LABEL)),
                        )
                        .child(ui::icon("chevron-down", 10.).text_color(theme::faint())),
                    |d, menu, desk, _| d.forge_views(menu, desk),
                    cx,
                )
                .anchor(gpui::Anchor::TopLeft),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .child(Input::new(&f.search).small()),
            )
            .when(f.kind == Kind::Issue && f.every.is_none(), |d| {
                d.child(div().w(px(110.)).child(Input::new(&f.team).small()))
                    .child(
                        ui::button("new-issue", crate::tr("New issue"), Weight::Secondary)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.edit_forge(true, window, cx);
                            })),
                    )
            })
            .child(
                ui::icon_button("forge-refresh", "retry", crate::tr("Read again"))
                    .size(px(26.))
                    .on_click(cx.listener(|this, _, _, cx| this.list_forge(cx))),
            );
        // A row's menu: what can be done with the one right-clicked.
        let (desk, rows) = (cx.entity(), f.rows.clone());
        let list = div()
            .id("forge-rows")
            .size_full()
            .child(List::new(&f.rows).p(px(6.)))
            .context_menu(move |menu, _, cx| {
                let rows = rows.read(cx);
                let Some(row) = rows.right_clicked_index().map(|ix| ix.row) else {
                    return menu;
                };
                let rows = rows.delegate();
                let Some(item) = rows.items().get(row).cloned() else {
                    return menu;
                };
                let d = desk.read(cx);
                let folder = (rows.kin(row).0.map(|(p, _)| p.clone()))
                    .or_else(|| d.forge.folder.clone())
                    .unwrap_or_default();
                d.forge_menu(&item, folder, menu, &desk)
            });
        let empty_or_err = match &f.rows.read(cx).delegate().list {
            Some(Err(why)) => Some(Err(why.clone())),
            Some(Ok(items)) if items.is_empty() => Some(Ok(())),
            _ => None,
        };
        if !f.creating
            && let Some(state) = empty_or_err
            && let Some(full) = self.forge_state(&project_label, state, cx)
        {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .child(bar)
                .child(full);
        }
        let side = if f.creating {
            self.forge_form(faces, &crate::tr("New issue"), crate::tr("Create"), cx)
        } else {
            self.forge_detail(faces, cx)
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .w(px(400.))
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(theme::border())
                            .child(list),
                    )
                    .child(side),
            )
    }

    /// Full-body empty or error state on the PRs / Issues page (`EmptyStates`).
    fn forge_state(
        &self,
        project: &str,
        list: Result<(), String>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use u0ui::components::alert::{Alert, AlertVariant};
        use u0ui::components::empty::{
            Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
        };
        use u0ui::prelude::*;
        let f = &self.forge;
        let wrap = |center: bool| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .p(px(24.))
                .gap(px(16.))
                .when(center, |d| d.items_start().justify_center())
        };
        match list {
            Err(why) if why == "no remote" => Some(
                wrap(true).child(
                    Empty::new()
                        .child(
                            EmptyHeader::new()
                                .child(EmptyTitle::new(crate::tr_with(
                                    "%{project} has no host",
                                    &[("project", &project)],
                                )))
                                .child(EmptyDescription::new(crate::tr(
                                    "Add a remote with git remote add origin, then read again.",
                                ))),
                        )
                        .child(
                            EmptyContent::new()
                                .child(
                                    Button::new("forge-no-host-again", crate::tr("Read again"))
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::Renew)
                                        .on_click(cx.listener(|this, _, _, cx| this.list_forge(cx))),
                                )
                                .child(
                                    Button::new("forge-no-host-all", crate::tr("All projects"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.list_projects(true, None, window, cx);
                                        })),
                                ),
                        ),
                ),
            ),
            Err(why) if why.starts_with("no token:") => {
                let host_name = why
                    .strip_prefix("no token:")
                    .and_then(|s| s.split(':').next())
                    .map_or("host", str::trim);
                Some(
                    wrap(false)
                        .child(
                            Alert::new("forge-no-token")
                                .variant(AlertVariant::Info)
                                .title(crate::tr("Only what is public"))
                                .description(crate::tr_with(
                                    "%{project} is private; add a %{host} token (kept in the Keychain).",
                                    &[("host", &host_name), ("project", &project)],
                                )),
                        )
                        .child(
                            Button::new(
                                "forge-give-token",
                                crate::tr("Give a token in Source control › Hosts"),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_tab(crate::pages::Tab::SourceControl, window, cx);
                            })),
                        ),
                )
            }
            Err(why) if why.contains("Linear team") => {
                let place = f.folder.clone();
                Some(
                    wrap(true).child(
                        Empty::new()
                            .child(
                                EmptyHeader::new()
                                    .child(EmptyTitle::new(crate::tr_with(
                                        "%{project}’s issues come through Linear",
                                        &[("project", &project)],
                                    )))
                                    .child(EmptyDescription::new(crate::tr_with(
                                        "Bind %{project} to a Linear team key, such as ENG, in its project settings.",
                                        &[("project", &project)],
                                    ))),
                            )
                            .child(
                                EmptyContent::new().child(
                                    Button::new(
                                        "forge-choose-linear",
                                        crate::tr("Choose a Linear team"),
                                    )
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.pages.project.clone_from(&place);
                                        this.open_tab(crate::pages::Tab::Projects, window, cx);
                                    })),
                                ),
                            ),
                    ),
                )
            }
            Err(why)
                if !self.on_path.contains(&"gh")
                    || (why.contains("gh") && why.contains("No such file")) =>
            {
                Some(
                    wrap(false)
                        .child(
                            Alert::new("forge-no-gh")
                                .variant(AlertVariant::Warning)
                                .title(crate::tr(
                                    "GitHub is reached with gh, and gh is not on this Mac",
                                ))
                                .description(crate::tr(
                                    "brew install gh, then gh auth login, or add a token in Source control › Hosts.",
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .child(
                                    Button::new(
                                        "forge-copy-brew-gh",
                                        crate::tr("Copy brew install gh"),
                                    )
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .icon(CarbonIcon::Copy)
                                    .on_click(|_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            "brew install gh".into(),
                                        ));
                                    }),
                                )
                                .child(
                                    Button::new("forge-no-gh-again", crate::tr("Read again"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::Renew)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.list_forge(cx)),
                                        ),
                                ),
                        ),
                )
            }
            Ok(()) if f.every.is_none() => {
                let filtered = f.part != 0
                    || f.state != "open"
                    || !f.search.read(cx).value().trim().is_empty();
                let desc = if f.kind == Kind::Issue && f.part == 1 && f.state == "open" {
                    crate::tr_with(
                        "No open issue in %{project} is assigned to you or was opened by you.",
                        &[("project", &project)],
                    )
                } else {
                    crate::tr("Nothing matched these filters.")
                };
                let mut actions = EmptyContent::new();
                if filtered {
                    actions = actions.child(
                        Button::new("forge-show-all", crate::tr("Show All"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.forge.part = 0;
                                this.forge.state = "open";
                                this.forge
                                    .search
                                    .update(cx, |i, cx| i.set_value("", window, cx));
                                this.list_forge(cx);
                            })),
                    );
                }
                if f.kind == Kind::Issue {
                    actions = actions.child(
                        Button::new("forge-empty-new-issue", crate::tr("New issue"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Add)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.edit_forge(true, window, cx);
                            })),
                    );
                }
                Some(
                    wrap(true).child(
                        Empty::new()
                            .child(
                                EmptyHeader::new()
                                    .child(EmptyTitle::new(crate::tr("None here.")))
                                    .child(EmptyDescription::new(desc)),
                            )
                            .when(filtered || f.kind == Kind::Issue, |e| e.child(actions)),
                    ),
                )
            }
            _ => None,
        }
    }
}

/// One in the list: its state, number and title; its project in the list
/// of every project, who, its branch (and the pull request it is stacked
/// on, `under`), when; its labels, checks, review and comments.
pub fn row(faces: &Faces, item: &Item, under: Option<u64>, project: Option<&str>) -> Div {
    let n = item.number;
    let (passed, failed, going) = item.checks;
    let mut meta = vec![format!("#{n}"), item.author.clone(), when_of(&item.updated)];
    if !item.branch.is_empty() {
        meta.insert(2, item.branch.clone());
    }
    if let Some(under) = under {
        meta.insert(
            3,
            crate::tr_with("on #%{under}", &[("under", &under)]).to_string(),
        );
    }
    if let Some(project) = project {
        meta.insert(0, project.to_owned());
    }
    div()
        .flex()
        .gap(px(10.))
        .w_full()
        .child(div().mt(px(6.)).child(ui::dot(tint(item))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .flex_1()
                .min_w(px(0.))
                .child(ui::one_line(
                    ui::text(faces, item.title.clone()).text_size(px(crate::style::LABEL)),
                ))
                .child(ui::one_line(ui::fine(faces, meta.join(" · "))))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .children(
                            (item.labels.iter()).map(|l| Tag::secondary().small().child(l.clone())),
                        )
                        .when(passed + failed + going > 0, |d| {
                            d.child(ui::fine(faces, format!("✓{passed}")).text_color(theme::ok()))
                                .when(failed > 0, |d| {
                                    d.child(
                                        ui::fine(faces, format!("✗{failed}"))
                                            .text_color(theme::coral()),
                                    )
                                })
                                .when(going > 0, |d| {
                                    d.child(
                                        ui::fine(faces, format!("•{going}"))
                                            .text_color(theme::accent()),
                                    )
                                })
                        })
                        .when(!item.review.is_empty(), |d| {
                            d.child(ui::fine(faces, item.review.clone()))
                        })
                        .when(item.comments > 0, |d| {
                            d.child(ui::fine(
                                faces,
                                crate::tr_with("%{n} said", &[("n", &item.comments)]),
                            ))
                        }),
                ),
        )
}

impl Desk {
    /// The one read: its head and what can be done with it, its tabs, and
    /// the line to say something.
    fn forge_detail(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let shell = div().flex().flex_col().flex_1().min_w(px(0.)).h_full();
        let d = match &f.detail {
            None if f.chosen.is_none() => {
                return shell.items_center().justify_center().child(ui::fine(
                    faces,
                    crate::tr(match f.kind {
                        Kind::Pr => "Pick a pull request to read it.",
                        Kind::Issue => "Pick an issue to read it.",
                    }),
                ));
            }
            None => {
                return shell
                    .p(px(16.))
                    .child(ui::fine(faces, crate::tr("Reading…")));
            }
            Some(Err(e)) => {
                return shell.p(px(16.)).child(ui::fine(
                    faces,
                    crate::tr_with("Couldn't read it: %{e}", &[("e", e)]),
                ));
            }
            Some(Ok(d)) => d,
        };
        if f.editing {
            return self.forge_form(faces, &crate::tr("Edit"), crate::tr("Save"), cx);
        }
        let item = &d.item;
        let url = item.url.clone();
        let head = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(20.))
            .pt(px(16.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        ui::text(faces, format!("{} #{}", item.title, item.number))
                            .flex_1()
                            .text_size(px(crate::style::H2))
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::button("forge-open", crate::tr("Open on its host"), Weight::Quiet)
                            .h(px(26.))
                            .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx))),
                    )
                    .child(self.forge_actions(&d.item, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::fine(
                            faces,
                            if item.draft {
                                crate::tr("draft").to_string()
                            } else {
                                item.state.clone()
                            },
                        )
                        .text_color(tint(item)),
                    )
                    .child(ui::fine(
                        faces,
                        crate::tr_with("by %{author}", &[("author", &item.author)]),
                    ))
                    .when(!item.branch.is_empty(), |d| {
                        d.child(ui::fine_mono(
                            faces,
                            format!("{} → {}", item.branch, item.base),
                        ))
                    })
                    .when(d.conflicts, |d| {
                        d.child(
                            ui::fine(faces, crate::tr("conflicts with its base"))
                                .text_color(theme::coral()),
                        )
                    })
                    .child(ui::fine(faces, when_of(&item.updated))),
            );
        let tabs: &[(&str, Tab)] = match f.kind {
            Kind::Pr => &[
                ("Summary", Tab::Summary),
                ("Timeline", Tab::Timeline),
                ("Code", Tab::Code),
                ("Conversations", Tab::Threads),
                ("Guide", Tab::Guide),
            ],
            Kind::Issue => &[("Summary", Tab::Summary), ("Timeline", Tab::Timeline)],
        };
        let chosen = tabs.iter().position(|(_, t)| *t == f.tab).unwrap_or(0);
        let picks: Vec<Tab> = tabs.iter().map(|(_, t)| *t).collect();
        let strip = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(20.))
            .pb(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                TabBar::new("forge-tabs")
                    .segmented()
                    .small()
                    .selected_index(chosen)
                    .children(
                        tabs.iter()
                            .map(|(name, _)| KitTab::new().label(crate::tr(name))),
                    )
                    .on_click(cx.listener(move |this, n: &usize, _, cx| {
                        this.forge.tab = picks[*n];
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(f.tab == Tab::Code && !f.pr.files.is_empty(), |el| {
                let seen = (f.pr.files.iter()).filter(|file| self.viewed(file)).count();
                el.child(ui::fine(
                    faces,
                    crate::tr_with(
                        "%{seen} of %{total} viewed",
                        &[("seen", &seen), ("total", &f.pr.files.len())],
                    ),
                ))
            })
            .when(f.tab == Tab::Timeline, |d| {
                d.child(
                    ui::button(
                        "forge-order",
                        crate::tr(if f.newest {
                            "Newest first"
                        } else {
                            "Oldest first"
                        }),
                        Weight::Quiet,
                    )
                    .h(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forge.newest = !this.forge.newest;
                        cx.notify();
                    })),
                )
            });
        let body: gpui::AnyElement = match f.tab {
            Tab::Code if f.kind == Kind::Pr => div()
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_code(d, cx))
                .into_any_element(),
            Tab::Threads if f.kind == Kind::Pr => div()
                .id("forge-threads")
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_threads(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
            Tab::Guide if f.kind == Kind::Pr => div()
                .id("forge-guide")
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_guide(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
            Tab::Timeline => {
                let mut said: Vec<_> = d.said.iter().collect();
                if f.newest {
                    said.reverse();
                }
                let mut list = div().flex().flex_col().gap(px(14.)).px(px(20.)).py(px(14.));
                if said.is_empty() {
                    list = list.child(ui::fine(faces, crate::tr("Nothing said yet.")));
                }
                for (n, s) in said.into_iter().enumerate() {
                    list = list.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(ui::fine(
                                faces,
                                format!("{} {} · {}", s.who, s.how, when_of(&s.when)),
                            ))
                            .when(!s.body.trim().is_empty(), |d| {
                                d.child(markdown::render(
                                    faces,
                                    &format!("forge-said-{n}"),
                                    &s.body,
                                    cx,
                                ))
                            }),
                    );
                }
                div()
                    .id("forge-timeline")
                    .flex_1()
                    .min_h(px(0.))
                    .child(list)
                    .overflow_y_scrollbar()
                    .into_any_element()
            }
            _ => div()
                .id("forge-summary")
                .flex_1()
                .min_h(px(0.))
                .child(self.forge_summary(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
        };
        shell
            .child(head)
            .child(strip)
            .child(body)
            .child(self.forge_say(faces, cx))
    }

    /// What it is: its labels and people, its checks, its words.
    fn forge_summary(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let row = |label: &str, value: String| {
            div()
                .flex()
                .gap(px(10.))
                .child(ui::fine(faces, crate::tr(label)).w(px(80.)).flex_none())
                .child(
                    ui::text(faces, value)
                        .text_size(px(crate::style::LABEL))
                        .text_color(theme::text2()),
                )
        };
        let none = |v: &[String]| {
            if v.is_empty() {
                crate::tr("none").to_string()
            } else {
                v.join(", ")
            }
        };
        let kind = self.forge.kind;
        let mut checks = div().flex().flex_col().gap(px(4.));
        for (n, c) in d.checks.iter().enumerate() {
            let url = c.url.clone();
            let failed = c.failed();
            let run = c.run().map(str::to_owned).filter(|_| failed);
            checks = checks.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::dot(match c.state.as_str() {
                        "success" | "neutral" | "skipped" => theme::ok(),
                        _ if failed => theme::coral(),
                        _ => theme::accent(),
                    }))
                    .child(
                        ui::one_line(
                            ui::text(faces, c.name.clone()).text_size(px(crate::style::LABEL)),
                        )
                        .flex_1(),
                    )
                    .child(ui::fine(faces, c.state.clone()))
                    .children(run.clone().map(|run| {
                        ui::button(
                            SharedString::from(format!("log-{n}")),
                            crate::tr("Log"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| this.read_log(run.clone(), cx)))
                    }))
                    .children(run.map(|run| {
                        ui::button(
                            SharedString::from(format!("rerun-{n}")),
                            crate::tr("Run again"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act_forge(Act::Rerun(run.clone()), cx);
                        }))
                    }))
                    .when(!url.is_empty(), |d| {
                        d.child(
                            ui::button(
                                SharedString::from(format!("check-{n}")),
                                crate::tr("Open"),
                                Weight::Quiet,
                            )
                            .h(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx))),
                        )
                    }),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(20.))
            .py(px(14.))
            .child(row("Labels", none(&d.item.labels)))
            .child(row("Assignees", none(&d.assignees)))
            .when(kind == Kind::Pr, |el| {
                let listed = (self.forge.rows.read(cx).delegate()).of(self.forge.folder.as_ref());
                let (under, over) = super::pr::stack(&d.item, listed);
                let numbers = |ns: Vec<u64>| {
                    ns.iter()
                        .map(|n| format!("#{n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                el.child(row("Reviewers", none(&d.reviewers)))
                    .when(under.is_some() || !over.is_empty(), |el| {
                        let mut said = Vec::new();
                        if let Some(n) = under {
                            said.push(crate::tr_with("on #%{under}", &[("under", &n)]).to_string());
                        }
                        if !over.is_empty() {
                            said.push(
                                crate::tr_with("%{numbers} on it", &[("numbers", &numbers(over))])
                                    .to_string(),
                            );
                        }
                        el.child(row("Stack", said.join("; ")))
                    })
                    .when(!self.forge.pr.files.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(ui::fine(faces, crate::tr("Changes")).w(px(80.)).flex_none())
                                .child(super::pr::breakdown(&self.forge.pr.files)),
                        )
                    })
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div().w(px(240.)).child(
                            Input::new(&self.forge.who)
                                .list(&self.forge.offered)
                                .small(),
                        ),
                    )
                    .child(
                        ui::button("add-label", crate::tr("Add label"), Weight::Quiet)
                            .h(px(24.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_person_or_label(0, window, cx);
                            })),
                    )
                    .child(
                        ui::button(
                            "add-person",
                            crate::tr(if kind == Kind::Pr {
                                "Ask for review"
                            } else {
                                "Assign"
                            }),
                            Weight::Quiet,
                        )
                        .h(px(24.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_person_or_label(1, window, cx);
                        })),
                    ),
            )
            .when(!d.checks.is_empty(), |el| {
                el.child(ui::label(faces, &crate::tr("Checks")))
                    .child(checks)
            })
            .children(self.forge.log.as_ref().map(|(run, log)| {
                let (text, bad) = match log {
                    Ok(t) => (t.clone(), false),
                    Err(e) => (e.clone(), true),
                };
                div()
                    .id("forge-log")
                    .max_h(px(320.))
                    .p(px(10.))
                    .rounded(px(8.))
                    .bg(theme::well())
                    .child(ui::fine(
                        faces,
                        crate::tr_with("Run %{run}, where it failed", &[("run", run)]),
                    ))
                    .child(ui::fine_mono(faces, text).when(bad, |d| d.text_color(theme::coral())))
                    .overflow_y_scrollbar()
            }))
            .child(ui::label(faces, &crate::tr("Description")))
            .child(if d.body.trim().is_empty() {
                ui::fine(faces, crate::tr("No description.")).into_any_element()
            } else {
                markdown::render(faces, "forge-body", &d.body, cx).into_any_element()
            })
    }

    /// Add what is on the line as a label (`0`), or as a reviewer or
    /// assignee.
    pub(crate) fn add_person_or_label(
        &mut self,
        what: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let named = self
            .forge
            .who
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('@')
            .to_owned();
        if named.is_empty() {
            return;
        }
        self.forge
            .who
            .update(cx, |i, cx| i.set_value("", window, cx));
        let act = match (what, self.forge.kind) {
            (0, _) => Act::Label(named),
            (_, Kind::Pr) => Act::Reviewer(named),
            _ => Act::Assignee(named),
        };
        self.act_forge(act, cx);
    }

    /// The line to say something on the one read, and its buttons.
    fn forge_say(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let pr = self.forge.kind == Kind::Pr;
        let lines = self.forge.pr.lines.len();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .p(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(Textarea::new(&self.forge.line).small())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(self.forge.doing.as_ref().map(|d| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::live_dot())
                            .child(ui::fine(faces, format!("{d}…")))
                    }))
                    .child(div().flex_1())
                    .when(pr, |d| {
                        d.child(
                            ui::button(
                                "review-changes",
                                crate::tr("Request changes"),
                                Weight::Quiet,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.say_forge(Some("request-changes"), window, cx);
                                },
                            )),
                        )
                        .child(
                            ui::button("review-approve", crate::tr("Approve"), Weight::Secondary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.say_forge(Some("approve"), window, cx);
                                })),
                        )
                    })
                    .child(
                        ui::button(
                            "say-comment",
                            if lines > 0 {
                                crate::tr_with(
                                    "Comment with %{lines} on lines",
                                    &[("lines", &lines)],
                                )
                            } else {
                                crate::tr("Comment")
                            },
                            Weight::Primary,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.say_forge(None, window, cx)),
                        ),
                    ),
            )
    }

    /// A title and words: editing the one read, or a new issue.
    fn forge_form(
        &self,
        faces: &Faces,
        head: &str,
        done: SharedString,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(10.))
            .p(px(20.))
            .child(ui::label(faces, head))
            .when(
                self.forge.creating && !self.forge.templates.is_empty(),
                |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::fine(faces, crate::tr("Begin from")))
                            .children(self.forge.templates.iter().enumerate().map(
                                |(n, (name, _))| {
                                    ui::button(
                                        SharedString::from(format!("template-{n}")),
                                        name.clone(),
                                        Weight::Secondary,
                                    )
                                    .h(px(24.))
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| this.use_template(n, window, cx),
                                    ))
                                },
                            )),
                    )
                },
            )
            .child(Input::new(&self.forge.title))
            .child(Textarea::new(&self.forge.body))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::button("form-done", done, Weight::Primary)
                            .on_click(cx.listener(|this, _, _, cx| this.save_forge(cx))),
                    )
                    .child(
                        ui::button("form-cancel", crate::tr("Cancel"), Weight::Quiet).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.forge.editing = false;
                                this.forge.creating = false;
                                cx.notify();
                            }),
                        ),
                    ),
            )
    }
}
