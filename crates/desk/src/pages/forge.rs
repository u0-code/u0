//! The PRs and Issues pages drawn: the choices and the list on the left,
//! the one read on the right, as t3code's pages set them.

use gpui::component::{
    Sizable as _,
    diff::Diff,
    input::{Input, Textarea},
    scroll::ScrollableElement as _,
    tab::{Tab as KitTab, TabBar},
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

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

/// What a line of the "⋯" menu does.
type Act3 = Box<dyn Fn(&mut Desk, &mut gpui::Window, &mut Context<Desk>)>;

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

impl Desk {
    pub(crate) fn forge_page(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let states: &[&'static str] = match f.kind {
            Kind::Pr => &["open", "closed", "merged", "all"],
            Kind::Issue => &["open", "closed", "all"],
        };
        let state_picks: Vec<(&'static str, bool, ui::Pick)> = states
            .iter()
            .map(|st| {
                let desk = cx.entity();
                let pick: ui::Pick = Box::new(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.forge.state = st;
                        d.list_forge(cx);
                    });
                });
                (*st, f.state == *st, pick)
            })
            .collect();
        let part_picks: Vec<(&'static str, bool, ui::Pick)> = PARTS
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
                (*name, f.part == n, pick)
            })
            .collect();
        let project = f.folder.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "No project".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let faces2 = faces.clone();
        let chip = ui::opener("forge-project")
            .h(px(26.))
            .px(px(8.))
            .child(ui::text(faces, project).text_size(px(13.)))
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
                    move |d, _, cx| d.forge_projects(&faces2, cx),
                    cx,
                )
                .anchor(gpui::Anchor::TopLeft),
            )
            .child(ui::segmented("forge-state", state_picks))
            .child(ui::segmented("forge-part", part_picks))
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .child(Input::new(&f.search).small()),
            )
            .when(f.kind == Kind::Issue, |d| {
                d.child(
                    ui::button("new-issue", "New issue", Weight::Secondary).on_click(
                        cx.listener(|this, _, window, cx| this.edit_forge(true, window, cx)),
                    ),
                )
            })
            .child(
                ui::icon_button("forge-refresh", "retry", "Read again")
                    .size(px(26.))
                    .on_click(cx.listener(|this, _, _, cx| this.list_forge(cx))),
            );
        let list = match &f.list {
            None => div().p(px(16.)).child(ui::fine(faces, "Reading…")),
            Some(Err(e)) => div()
                .p(px(16.))
                .child(ui::fine(faces, format!("gh couldn't list them: {e}"))),
            Some(Ok(items)) if items.is_empty() => {
                div().p(px(16.)).child(ui::fine(faces, "None here."))
            }
            Some(Ok(items)) => {
                let mut rows = div().flex().flex_col().p(px(6.));
                for item in items {
                    rows = rows.child(self.forge_row(faces, item, cx));
                }
                rows
            }
        };
        let side = if f.creating {
            self.forge_form(faces, "New issue", "Create", cx)
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
                            .id("forge-list")
                            .w(px(400.))
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(theme::border())
                            .child(list)
                            .overflow_y_scrollbar(),
                    )
                    .child(side),
            )
    }

    /// The projects a page can be of.
    fn forge_projects(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut menu = div().flex().flex_col().w(px(240.));
        for (n, folder) in self.projects().into_iter().enumerate() {
            let on = self.forge.folder.as_ref() == Some(&folder);
            let name = folder.file_name().map_or_else(
                || folder.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            menu = menu.child(
                ui::menu_row(SharedString::from(format!("forge-project-{n}")), on)
                    .child(ui::text(faces, name).text_size(px(13.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.forge.folder = Some(folder.clone());
                        this.forge.chosen = None;
                        this.forge.detail = None;
                        this.popup = Popup::Closed;
                        this.list_forge(cx);
                    })),
            );
        }
        menu
    }

    /// One in the list: its state, number and title; who, its branch, when;
    /// its labels, checks, review and comments.
    fn forge_row(&self, faces: &Faces, item: &Item, cx: &mut Context<Self>) -> impl IntoElement {
        let n = item.number;
        let on = self.forge.chosen == Some(n);
        let (passed, failed, going) = item.checks;
        let mut meta = vec![format!("#{n}"), item.author.clone(), when_of(&item.updated)];
        if !item.branch.is_empty() {
            meta.insert(2, item.branch.clone());
        }
        div()
            .id(SharedString::from(format!("forge-row-{n}")))
            .flex()
            .gap(px(10.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::panel2()))
            .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
            .child(div().mt(px(6.)).child(ui::dot(tint(item))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .flex_1()
                    .min_w(px(0.))
                    .child(ui::one_line(
                        ui::text(faces, item.title.clone()).text_size(px(13.5)),
                    ))
                    .child(ui::one_line(ui::fine(faces, meta.join(" · "))))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .children(item.labels.iter().map(|l| {
                                ui::fine(faces, l.clone())
                                    .px(px(6.))
                                    .rounded(px(6.))
                                    .bg(theme::panel2())
                            }))
                            .when(passed + failed + going > 0, |d| {
                                d.child(
                                    ui::fine(faces, format!("✓{passed}")).text_color(theme::ok()),
                                )
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
                                d.child(ui::fine(faces, format!("{} said", item.comments)))
                            }),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.read_forge(n, cx)))
    }

    /// The one read: its head and what can be done with it, its tabs, and
    /// the line to say something.
    fn forge_detail(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let shell = div().flex().flex_col().flex_1().min_w(px(0.)).h_full();
        let d = match &f.detail {
            None if f.chosen.is_none() => {
                return shell.items_center().justify_center().child(ui::fine(
                    faces,
                    match f.kind {
                        Kind::Pr => "Pick a pull request to read it.",
                        Kind::Issue => "Pick an issue to read it.",
                    },
                ));
            }
            None => return shell.p(px(16.)).child(ui::fine(faces, "Reading…")),
            Some(Err(e)) => {
                return shell
                    .p(px(16.))
                    .child(ui::fine(faces, format!("gh couldn't read it: {e}")));
            }
            Some(Ok(d)) => d,
        };
        if f.editing {
            return self.forge_form(faces, "Edit", "Save", cx);
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
                            .text_size(px(17.))
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::button("forge-open", "Open on GitHub", Weight::Quiet)
                            .h(px(26.))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .child(self.forge_actions(faces, d, cx)),
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
                                "draft".to_owned()
                            } else {
                                item.state.clone()
                            },
                        )
                        .text_color(tint(item)),
                    )
                    .child(ui::fine(faces, format!("by {}", item.author)))
                    .when(!item.branch.is_empty(), |d| {
                        d.child(ui::fine_mono(
                            faces,
                            format!("{} → {}", item.branch, item.base),
                        ))
                    })
                    .when(d.conflicts, |d| {
                        d.child(
                            ui::fine(faces, "conflicts with its base").text_color(theme::coral()),
                        )
                    })
                    .child(ui::fine(faces, when_of(&item.updated))),
            );
        let tabs: &[(&str, Tab)] = match f.kind {
            Kind::Pr => &[
                ("Summary", Tab::Summary),
                ("Timeline", Tab::Timeline),
                ("Code", Tab::Code),
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
                    .children(tabs.iter().map(|(name, _)| KitTab::new().label(*name)))
                    .on_click(cx.listener(move |this, n: &usize, _, cx| {
                        this.forge.tab = picks[*n];
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(f.tab == Tab::Timeline, |d| {
                d.child(
                    ui::button(
                        "forge-order",
                        if f.newest {
                            "Newest first"
                        } else {
                            "Oldest first"
                        },
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
                .child(
                    Diff::new(&f.diff)
                        .size_full()
                        .soft_wrap(self.settings.diff_wrap),
                )
                .into_any_element(),
            Tab::Timeline => {
                let mut said: Vec<_> = d.said.iter().collect();
                if f.newest {
                    said.reverse();
                }
                let mut list = div().flex().flex_col().gap(px(14.)).px(px(20.)).py(px(14.));
                if said.is_empty() {
                    list = list.child(ui::fine(faces, "Nothing said yet."));
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
                .child(ui::fine(faces, label.to_owned()).w(px(80.)).flex_none())
                .child(
                    ui::text(faces, value)
                        .text_size(px(13.))
                        .text_color(theme::text2()),
                )
        };
        let none = |v: &[String]| {
            if v.is_empty() {
                "none".to_owned()
            } else {
                v.join(", ")
            }
        };
        let kind = self.forge.kind;
        let mut checks = div().flex().flex_col().gap(px(4.));
        for (n, c) in d.checks.iter().enumerate() {
            let url = c.url.clone();
            let failed = matches!(
                c.state.as_str(),
                "failure" | "error" | "timed_out" | "cancelled" | "action_required"
            );
            let run = url
                .split("/runs/")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .map(str::to_owned)
                .filter(|_| failed);
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
                        ui::one_line(ui::text(faces, c.name.clone()).text_size(px(13.))).flex_1(),
                    )
                    .child(ui::fine(faces, c.state.clone()))
                    .children(run.map(|run| {
                        ui::button(
                            SharedString::from(format!("rerun-{n}")),
                            "Run again",
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
                                "Open",
                                Weight::Quiet,
                            )
                            .h(px(22.))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
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
                el.child(row("Reviewers", none(&d.reviewers)))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().w(px(240.)).child(Input::new(&self.forge.who).small()))
                    .child(
                        ui::button("add-label", "Add label", Weight::Quiet)
                            .h(px(24.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_person_or_label(0, window, cx);
                            })),
                    )
                    .child(
                        ui::button(
                            "add-person",
                            if kind == Kind::Pr {
                                "Ask for review"
                            } else {
                                "Assign"
                            },
                            Weight::Quiet,
                        )
                        .h(px(24.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_person_or_label(1, window, cx);
                        })),
                    ),
            )
            .when(!d.checks.is_empty(), |el| {
                el.child(ui::label(faces, "Checks")).child(checks)
            })
            .child(ui::label(faces, "Description"))
            .child(if d.body.trim().is_empty() {
                ui::fine(faces, "No description.").into_any_element()
            } else {
                markdown::render(faces, "forge-body", &d.body, cx).into_any_element()
            })
    }

    /// Add what is on the line as a label (`0`), or as a reviewer or
    /// assignee.
    fn add_person_or_label(
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

    /// What can be done with the one read, under its "⋯".
    fn forge_actions(
        &self,
        faces: &Faces,
        d: &Detail,
        cx: &mut Context<Self>,
    ) -> gpui::component::popover::Popover {
        let trigger =
            ui::icon_button("forge-actions", "dots", "What can be done with it").size(px(26.));
        let (faces, detail) = (faces.clone(), d.clone());
        self.menu(
            Popup::ForgeActions,
            trigger,
            move |desk, _, cx| desk.forge_menu(&faces, &detail, cx),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    fn forge_menu(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let item = &d.item;
        let (open, pr) = (item.state == "open", self.forge.kind == Kind::Pr);
        let folder = self.forge.folder.clone().unwrap_or_default();
        let mut rows = Vec::new();
        let mut entry = |id: &'static str, label: String, act: Act3| {
            rows.push(
                ui::menu_row(id, false)
                    .child(ui::text(faces, label).text_size(px(13.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.popup = Popup::Closed;
                        act(this, window, cx);
                    })),
            );
        };
        let merge = |how: &'static str, auto: bool| -> Act3 {
            Box::new(move |d, _, cx| {
                d.act_forge(
                    Act::Merge {
                        how: how.into(),
                        auto,
                        delete: true,
                    },
                    cx,
                );
            })
        };
        if pr && open {
            entry(
                "merge-squash",
                "Squash and merge".into(),
                merge("squash", false),
            );
            entry("merge-merge", "Merge".into(), merge("merge", false));
            entry(
                "merge-rebase",
                "Rebase and merge".into(),
                merge("rebase", false),
            );
            entry(
                "merge-auto",
                "Merge when its checks pass".into(),
                merge(self.settings.merge.gh(), true),
            );
            entry(
                "ready",
                if item.draft {
                    "Ready for review"
                } else {
                    "Convert to draft"
                }
                .into(),
                if item.draft {
                    Box::new(|d, _, cx| d.act_forge(Act::Ready, cx))
                } else {
                    Box::new(|d, _, cx| d.act_forge(Act::Draft, cx))
                },
            );
        }
        if open {
            entry(
                "close",
                "Close".into(),
                Box::new(|d, _, cx| d.act_forge(Act::Close, cx)),
            );
        } else if item.state == "closed" {
            entry(
                "reopen",
                "Reopen".into(),
                Box::new(|d, _, cx| d.act_forge(Act::Reopen, cx)),
            );
        }
        entry(
            "edit",
            "Edit title and description".into(),
            Box::new(|d, window, cx| d.edit_forge(false, window, cx)),
        );
        let (n, title) = (item.number, item.title.clone());
        if pr {
            let (f2, t2) = (folder.clone(), title.clone());
            entry(
                "session-worktree",
                "A session on it, in a worktree".into(),
                Box::new(move |d, window, cx| {
                    d.start_from_pr(f2.clone(), n, t2.clone(), false, window, cx);
                }),
            );
            let (f3, t3) = (folder.clone(), title.clone());
            entry(
                "session-here",
                "A session on it, checked out here".into(),
                Box::new(move |d, window, cx| {
                    d.start_from_pr(f3.clone(), n, t3.clone(), true, window, cx);
                }),
            );
            entry(
                "copy-checkout",
                "Copy the checkout command".into(),
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                        "gh pr checkout {n}"
                    )));
                }),
            );
            entry(
                "reload",
                "Load new commits".into(),
                Box::new(move |d, _, cx| d.read_forge(n, cx)),
            );
        } else {
            let (f2, t2) = (folder.clone(), title.clone());
            entry(
                "session-issue",
                "A session about it".into(),
                Box::new(move |d, window, cx| {
                    d.start_from_issue(f2.clone(), n, &t2, false, window, cx);
                }),
            );
            entry(
                "triage",
                "Triage it".into(),
                Box::new(move |d, window, cx| {
                    d.start_from_issue(folder.clone(), n, &title, true, window, cx);
                }),
            );
        }
        div().flex().flex_col().w(px(240.)).children(rows)
    }

    /// The line to say something on the one read, and its buttons.
    fn forge_say(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let pr = self.forge.kind == Kind::Pr;
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
                            ui::button("review-changes", "Request changes", Weight::Quiet)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.say_forge(Some("request-changes"), window, cx);
                                })),
                        )
                        .child(
                            ui::button("review-approve", "Approve", Weight::Secondary).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.say_forge(Some("approve"), window, cx);
                                }),
                            ),
                        )
                    })
                    .child(
                        ui::button("say-comment", "Comment", Weight::Primary).on_click(
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
        done: &'static str,
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
                    .child(ui::button("form-cancel", "Cancel", Weight::Quiet).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.forge.editing = false;
                            this.forge.creating = false;
                            cx.notify();
                        }),
                    )),
            )
    }
}
