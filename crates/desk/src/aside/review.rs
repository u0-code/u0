//! The full-window Review & Commit workspace (`ReviewCommit` board), shown when
//! the Diff surface is widened (`⌘⇧B`):
//! - 38px unified window header with `WorkspaceBreadcrumb`, waiting permission
//!   jump link, other-threads status counts, turn-history overflow menu, and Close
//! - 36px `.rc-bar` with inline `Changes` scope `Select`, file count, `DiffStat`,
//!   `Hide whitespace` `Checkbox`, and `Unified` / `Side by side` `ToggleGroup`
//! - 360px left `Files`, `Commits on <branch>`, and `Stashes` history rail
//! - Center syntax-highlighted `Diff` with `Viewed` checkboxes and pending
//!   `.t3-rthread--pending` inline comment cards
//! - 400px right `Commit` rail with pending comments card and staged changes /
//!   message / unsent-comments warning / commit & push controls.

use std::rc::Rc;

use gpui::component::scroll::ScrollableElement as _;
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px};
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, Checkbox, MenuItem, Textarea, ToggleGroup,
        ToggleGroupItem, Tooltip, toggle::ToggleSize,
    },
    u0::panels::{CommitGraph, CommitRow, StashRow, StashRowTexts, git::check_box},
    icons::{CarbonIcon, Icon},
    t3::{
        review::{DiffFileTree, DiffStat, TreeFile},
        shell::{Crumb, IconMenu, WorkspaceBreadcrumb},
    },
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use super::{
    diff::{file_status_of, files_of, is_test},
    empty,
};
use crate::{
    desk::{Desk, lists},
    fonts::Faces,
    git::Scope,
    repo::GitAction,
    session::{Item, Session, State},
    theme, ui,
};

/// `.rc-h`: `display: flex; align-items: baseline; gap: 8px; padding: 16px 16px 8px; font-size: 12px; font-weight: 600`.
fn rc_heading(faces: &Faces, title: impl Into<SharedString>, meta: Option<SharedString>) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(8.))
        .pt(px(16.))
        .pb(px(8.))
        .px(px(16.))
        .child(
            ui::text(faces, title)
                .type_style(TypeStyle::Label01)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::role(ColorRole::TextPrimary)),
        )
        .children(meta.map(|m| {
            ui::text(faces, m)
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary))
        }))
}

/// `.rc-vsep`: `width: 1px; height: 20px; background: var(--border-subtle-01); margin: 0 4px`.
fn vsep() -> Div {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(20.))
        .mx(px(4.))
        .bg(theme::role(ColorRole::BorderSubtle01))
}

/// 8px round status dot in `.rc-need` / `.rc-others`.
fn status_dot(role: ColorRole) -> Div {
    div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .bg(theme::role(role))
}

impl Desk {
    /// Full-window 3-column `ReviewCommit` workspace.
    pub(super) fn review_workspace(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Div {
        let header = self.review_header(faces, s, cx);
        let subbar = self.review_subbar(faces, s, cx);
        let left_rail = self.review_left_rail(faces, s, cx);
        let center_diff = self.review_center_diff(faces, s, cx);
        let right_rail = self.review_commit_rail(faces, s, cx);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(theme::role(ColorRole::Background))
            .child(header)
            .child(subbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(left_rail)
                    .child(center_diff)
                    .child(right_rail),
            )
    }

    /// 38px top header (`TitleBar` inset + `WorkspaceBreadcrumb` + waiting permission +
    /// other-threads status + turn-history `IconMenu` + `Close`).
    fn review_header(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
        let project_name = s.meta.project.file_name().map_or_else(
            || Desk::shown(&s.meta.project),
            |n| n.to_string_lossy().into_owned(),
        );
        let (d_proj, d_thread) = (desk.clone(), desk.clone());
        let crumbs = WorkspaceBreadcrumb::new(
            "rc-crumbs",
            [
                Crumb::new(project_name).on_click(move |_, cx| {
                    d_proj.update(cx, |this, cx| {
                        this.panes.whole = false;
                        cx.notify();
                    });
                }),
                Crumb::new(s.meta.title.clone()).on_click(move |_, cx| {
                    d_thread.update(cx, |this, cx| {
                        this.panes.whole = false;
                        cx.notify();
                    });
                }),
                Crumb::new(crate::tr("Review changes")),
            ],
        );

        let waiting_asked = s.items.iter().rev().find_map(|it| match it {
            Item::Asked(a) if a.chosen.is_none() => Some(a.title.clone()),
            _ => None,
        });
        let need_link = waiting_asked.map(|title| {
            let label = if title.trim().is_empty() {
                crate::tr("Answer the permission request")
            } else {
                crate::tr_with("Answer the %{title} request", &[("title", &title.trim())])
            };
            let d_back = desk.clone();
            div()
                .id("rc-need")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(32.))
                .px(px(8.))
                .cursor_pointer()
                .hover(|st| st.bg(theme::role(ColorRole::LayerHover01)))
                .on_click(move |_, _, cx| {
                    d_back.update(cx, |this, cx| {
                        this.panes.whole = false;
                        cx.notify();
                    });
                })
                .child(status_dot(ColorRole::SupportWarning))
                .child(
                    ui::text(faces, label)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::LinkPrimary)),
                )
                .child(
                    Icon::carbon(CarbonIcon::ArrowRight)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::LinkPrimary)),
                )
        });

        let (mut need_n, mut work_n, mut fail_n) = (0usize, 0usize, 0usize);
        for other in self
            .sessions
            .all
            .iter()
            .filter(|o| o.key != s.key && self.listed_now(o))
        {
            if lists::needs_you_why(other).is_some() {
                need_n += 1;
            } else {
                match lists::status(other) {
                    Some(lists::Status::Working) => work_n += 1,
                    Some(lists::Status::Failed) => fail_n += 1,
                    _ => {}
                }
            }
        }
        let has_others = need_n + work_n + fail_n > 0;
        let others = has_others.then(|| {
            let mut row = div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(8.));
            if need_n > 0 {
                row = row.child(status_dot(ColorRole::SupportWarning)).child(
                    ui::text(faces, crate::tr_with("%{n} needs you", &[("n", &need_n)]))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                );
            }
            if work_n > 0 {
                row = row
                    .child(
                        div()
                            .when(need_n > 0, |d| d.ml(px(4.)))
                            .child(status_dot(ColorRole::SupportInfo)),
                    )
                    .child(
                        ui::text(faces, crate::tr_with("%{n} working", &[("n", &work_n)]))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    );
            }
            if fail_n > 0 {
                row = row
                    .child(
                        div()
                            .when(need_n + work_n > 0, |d| d.ml(px(4.)))
                            .child(status_dot(ColorRole::SupportError)),
                    )
                    .child(
                        ui::text(faces, crate::tr_with("%{n} failed", &[("n", &fail_n)]))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    );
            }
            row.child(
                ui::text(faces, crate::tr("in other threads"))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextHelper)),
            )
        });

        let mut turn_items: Vec<MenuItem> = Vec::new();
        for (i, t) in s.turns.iter().enumerate().rev() {
            let is_redo = s.redo.as_ref().is_some_and(|(rt, _)| *rt == i);
            let can_act = is_redo || t.before.is_some();
            let label = if is_redo {
                crate::tr_with("Redo turn %{n}", &[("n", &(i + 1))])
            } else {
                crate::tr_with("Undo turn %{n}", &[("n", &(i + 1))])
            };
            let d_undo = desk.clone();
            turn_items.push(
                MenuItem::item(label)
                    .icon(if is_redo {
                        CarbonIcon::Redo
                    } else {
                        CarbonIcon::Undo
                    })
                    .disabled(!can_act)
                    .on_select(move |_, cx| {
                        d_undo.update(cx, |this, cx| this.undo(i, cx));
                    }),
            );
        }
        if turn_items.is_empty() {
            turn_items.push(MenuItem::item(crate::tr("No turns checkpointed yet")).disabled(true));
        }
        let history_menu = IconMenu::new(
            "rc-turn-history",
            crate::tr("Turn history: undo or restore a checkpoint"),
        )
        .icon(CarbonIcon::OverflowMenuHorizontal)
        .variant(ButtonVariant::Ghost)
        .items(turn_items);

        let d_close = desk;
        let close_btn = Button::new("rc-close", crate::tr("Close review (Esc)"))
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .icon(CarbonIcon::Close)
            .icon_only(true)
            .on_click(move |_, _, cx| {
                d_close.update(cx, |this, cx| {
                    this.panes.whole = false;
                    cx.notify();
                });
            });

        ui::title_bar(true)
            .child(div().flex_1().min_w(px(0.)).child(crumbs))
            .when_some(need_link, |d, link| d.child(link).child(vsep()))
            .when_some(others, |d, row| d.child(row).child(vsep()))
            .child(history_menu)
            .child(close_btn)
    }

    /// 36px `.rc-bar`: inline `Select` (`Changes`), file count, `DiffStat` with bar,
    /// `Hide whitespace` `Checkbox`, and `Unified` / `Side by side` `ToggleGroup`.
    fn review_subbar(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = files_of(s, &self.review.scope).unwrap_or_default();
        let (added, gone) = files
            .iter()
            .fold((0usize, 0usize), |(a, g), f| (a + f.added, g + f.gone));
        let count_text = if files.len() == 1 {
            crate::tr("1 file")
        } else {
            crate::tr_with("%{n} files", &[("n", &files.len())])
        };
        let key = s.key;
        let (d_ws, d_mode) = (cx.entity(), cx.entity());

        let hide_ws = Checkbox::new("rc-hide-ws")
            .label(crate::tr("Hide whitespace"))
            .checked(self.settings.no_spacing)
            .on_checked_change(move |on, _, cx| {
                d_ws.update(cx, |this, cx| {
                    this.settings.no_spacing = on;
                    this.save_settings();
                    this.refresh_diffs(key);
                    cx.notify();
                });
            });

        let mode_val = if self.settings.side_by_side {
            "split"
        } else {
            "unified"
        };
        let switcher = ToggleGroup::new("rc-diff-mode")
            .size(ToggleSize::Sm)
            .value([mode_val])
            .item(ToggleGroupItem::new("unified").child(crate::tr("Unified")))
            .item(ToggleGroupItem::new("split").child(crate::tr("Side by side")))
            .on_value_change(move |vals, _, cx| {
                let split = vals.first().is_some_and(|v| v.as_ref() == "split");
                d_mode.update(cx, |this, cx| {
                    this.settings.side_by_side = split;
                    this.save_settings();
                    cx.notify();
                });
            });

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .h(px(36.))
            .px(px(16.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .bg(theme::role(ColorRole::Background))
            .when(s.git_dir().is_some(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .w(px(240.))
                        .child(self.scope_select(s, cx)),
                )
            })
            .child(
                ui::text(faces, count_text)
                    .flex_none()
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary)),
            )
            .when(!files.is_empty(), |d| {
                d.child(DiffStat::new(added, gone).bar(true))
            })
            .child(div().flex_1())
            .child(div().flex_none().child(hide_ws))
            .child(div().flex_none().w(px(208.)).child(switcher))
    }

    /// 360px left column: `Files` (`DiffFileTree`), `Commits on <branch>` (`CommitRow`),
    /// and `Stashes` (`StashRow` + warning note).
    fn review_left_rail(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = files_of(s, &self.review.scope).unwrap_or_default();
        let viewed_count = files
            .iter()
            .filter(|f| s.git.viewed.contains(&f.path))
            .count();
        let tree_files: Vec<TreeFile> = files
            .iter()
            .filter(|f| !(self.review.no_tests && is_test(&f.path)))
            .map(|f| {
                TreeFile::new(f.path.clone(), file_status_of(f), f.added, f.gone)
                    .viewed(s.git.viewed.contains(&f.path))
            })
            .collect();
        let selected_file = self
            .review
            .selected_file
            .clone()
            .filter(|sel| tree_files.iter().any(|f| f.path.as_ref() == sel.as_str()))
            .or_else(|| tree_files.first().map(|f| f.path.to_string()))
            .map(SharedString::from);

        let desk = cx.entity();
        let diff_state = self.review.diff.clone();
        let d_select = desk.clone();
        let file_tree = DiffFileTree::new("rc-file-tree")
            .files(tree_files)
            .selected(selected_file)
            .viewed_label(crate::tr("Viewed"))
            .on_select(move |path, _, cx| {
                let path_str = path.to_string();
                d_select.update(cx, |this, cx| {
                    this.review.selected_file = Some(path_str.clone());
                    cx.notify();
                });
                diff_state.update(cx, |st, cx| {
                    st.set_file_collapsed(&path_str, false, cx);
                    st.scroll_to_file(&path_str, cx);
                });
            });

        let mut rail = div()
            .flex()
            .flex_col()
            .child(rc_heading(
                faces,
                crate::tr("Files"),
                Some(crate::tr_with(
                    "%{viewed} of %{total} viewed",
                    &[("viewed", &viewed_count), ("total", &files.len())],
                )),
            ))
            .child(file_tree);

        if !s.git.commits.is_empty() {
            let branch = s.branch.as_deref().unwrap_or("HEAD");
            let unpushed = s.git.commits.iter().filter(|c| c.unpushed).count();
            let meta =
                (unpushed > 0).then(|| crate::tr_with("%{n} not pushed", &[("n", &unpushed)]));
            rail = rail.child(div().mt(px(16.)).child(rc_heading(
                faces,
                crate::tr_with("Commits on %{branch}", &[("branch", &branch)]),
                meta,
            )));
            let total_commits = s.git.commits.len();
            for (idx, c) in s.git.commits.iter().enumerate() {
                let graph = if idx == 0 {
                    CommitGraph::Start
                } else if idx + 1 == total_commits {
                    CommitGraph::End
                } else {
                    CommitGraph::Line
                };
                let current = match &self.review.scope {
                    Scope::Commit(id) => id == &c.id,
                    _ => idx == 0,
                };
                let cid = c.id.clone();
                let d_commit = desk.clone();
                rail = rail.child(
                    CommitRow::new(
                        SharedString::from(format!("rc-commit-{cid}")),
                        c.id.clone(),
                        c.subject.clone(),
                    )
                    .author(c.author.clone())
                    .time(c.ago.clone())
                    .refs(c.refs.iter().cloned())
                    .stat(c.added, c.gone)
                    .graph(graph)
                    .current(current)
                    .on_click(move |_, cx| {
                        let cid = cid.clone();
                        d_commit.update(cx, |this, cx| {
                            if this.review.scope == Scope::Commit(cid.clone()) {
                                this.show_changes(Scope::All, cx);
                            } else {
                                this.show_commit(&cid, cx);
                            }
                        });
                    }),
                );
            }
        }

        if !s.git.stashes.is_empty() {
            rail = rail.child(div().mt(px(16.)).child(rc_heading(
                faces,
                crate::tr("Stashes"),
                None,
            )));
            let uncommitted_n = files_of(s, &Scope::All).map_or(0, |f| f.len());
            for st in &s.git.stashes {
                let index = st.index;
                let (d_apply, d_pop, d_drop) = (desk.clone(), desk.clone(), desk.clone());
                let texts = StashRowTexts {
                    files: Rc::new(|n| {
                        if n == 1 {
                            crate::tr("1 file")
                        } else {
                            crate::tr_with("%{n} files", &[("n", &n)])
                        }
                    }),
                    apply: crate::tr("Apply"),
                    pop: crate::tr("Pop"),
                    drop: crate::tr("Drop stash"),
                };
                rail = rail.child(
                    StashRow::new(
                        SharedString::from(format!("rc-stash-{index}")),
                        index,
                        st.message.clone(),
                    )
                    .branch(st.branch.clone())
                    .files(st.files)
                    .time(st.ago.clone())
                    .texts(texts)
                    .on_apply(move |_, cx| {
                        d_apply.update(cx, |this, cx| {
                            this.git_action(GitAction::StashApply { index, pop: false }, cx);
                        });
                    })
                    .on_pop(move |_, cx| {
                        d_pop.update(cx, |this, cx| {
                            this.git_action(GitAction::StashApply { index, pop: true }, cx);
                        });
                    })
                    .on_drop(move |_, cx| {
                        d_drop.update(cx, |this, cx| {
                            this.git_action(GitAction::StashDrop(index), cx);
                        });
                    }),
                );
            }
            if let Some(first_stash) = s.git.stashes.first().filter(|st| !st.branch.is_empty()) {
                let note_text = if uncommitted_n > 0 {
                    crate::tr_with(
                        "Made on %{branch}. Apply and Pop change your %{n} uncommitted files.",
                        &[("branch", &first_stash.branch), ("n", &uncommitted_n)],
                    )
                } else {
                    crate::tr_with("Made on %{branch}.", &[("branch", &first_stash.branch)])
                };
                rail = rail.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .px(px(16.))
                        .py(px(12.))
                        .child(
                            Icon::carbon(CarbonIcon::WarningAltFilled)
                                .size_length(px(16.))
                                .color(theme::role(ColorRole::SupportWarning))
                                .flex_none(),
                        )
                        .child(
                            ui::text(faces, note_text)
                                .flex_1()
                                .min_w(px(0.))
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary)),
                        ),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(360.))
            .h_full()
            .border_r_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .bg(theme::role(ColorRole::Layer01))
            .child(
                div()
                    .id("rc-left-rail")
                    .flex_1()
                    .min_h(px(0.))
                    .child(rail.pb(px(16.)))
                    .overflow_y_scrollbar(),
            )
    }

    /// Center column: syntax-highlighted `Diff` with `Viewed` checkboxes and
    /// inline pending review comment cards.
    fn review_center_diff(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = files_of(s, &self.review.scope);
        let reading = matches!(
            self.review.scope,
            Scope::Branch | Scope::Commit(_) | Scope::Uncommitted
        ) && files.is_none();
        let files = files.unwrap_or_default();
        let shown = files
            .iter()
            .filter(|f| !(self.review.no_tests && is_test(&f.path)))
            .count();

        let content = if reading {
            div()
                .flex_1()
                .p(px(24.))
                .child(empty(faces, crate::tr("Reading git…")))
        } else if shown == 0 {
            div().flex_1().p(px(24.)).child(empty(
                faces,
                if s.git_dir().is_some() {
                    crate::tr("Nothing changed here.")
                } else {
                    crate::tr("Files the agent changes show here.")
                },
            ))
        } else {
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .child(self.diff_view(s, cx))
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .bg(theme::role(ColorRole::Background))
            .child(content)
    }

    /// 400px right `Commit` rail: pending comments card + staged files, commit
    /// message `Textarea`, unsent-comments warning, `Commit N files` & `Commit & push`.
    fn review_commit_rail(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
        let key = s.key;
        let comment_count = self.review.comments.len();
        let agent_short = self.name_of(&s.meta.agent);

        let pending_card = (comment_count > 0).then(|| {
            let heading = if comment_count == 1 {
                crate::tr("1 pending comment")
            } else {
                crate::tr_with("%{n} pending comments", &[("n", &comment_count)])
            };
            let places = self
                .review
                .comments
                .iter()
                .map(|(p, line, _)| {
                    let short = p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    format!("{short}:{line}")
                })
                .collect::<Vec<_>>()
                .join(" \u{00b7} ");
            let d_send = desk.clone();
            div().p(px(16.)).bg(theme::role(ColorRole::Layer02)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::text(faces, heading)
                                    .type_style(TypeStyle::HeadingCompact01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            )
                            .child(ui::one_line(
                                ui::text(faces, places)
                                    .mt(px(2.))
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary)),
                            )),
                    )
                    .child(
                        Button::new(
                            "rc-send-comments",
                            crate::tr_with("Send to %{name}", &[("name", &agent_short)]),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            d_send.update(cx, |this, cx| {
                                this.send_comments(window, cx);
                                this.panes.whole = false;
                                cx.notify();
                            });
                        }),
                    ),
            )
        });

        let work_files = files_of(s, &Scope::All).unwrap_or_default();
        let total_files = work_files.len();
        let staged_files = work_files
            .iter()
            .filter(|f| !s.git.left_out.contains(&f.path))
            .count();
        let branch = s.branch.as_deref().unwrap_or("HEAD");
        let idle = !matches!(s.state, State::Working | State::Opening);

        let mut file_rows = div().flex().flex_col().pb(px(16.));
        for (i, f) in work_files.iter().enumerate() {
            let path = f.path.clone();
            let checked = !s.git.left_out.contains(&path);
            let d_chk = desk.clone();
            let p_toggle = path.clone();
            file_rows = file_rows.child(
                div()
                    .id(SharedString::from(format!("rc-stage-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(32.))
                    .px(px(16.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::role(ColorRole::LayerHover02)))
                    .on_click(move |_, _, cx| {
                        let p = p_toggle.clone();
                        d_chk.update(cx, |this, cx| {
                            if let Some(s) = this.sessions.at_mut(key)
                                && !s.git.left_out.remove(&p)
                            {
                                s.git.left_out.insert(p);
                            }
                            cx.notify();
                        });
                    })
                    .child(check_box(checked, false))
                    .child(
                        ui::one_line(
                            ui::mono(faces, path)
                                .type_style(TypeStyle::Code01)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(DiffStat::new(f.added, f.gone)),
            );
        }

        let writer = self.writer_of(s);
        let writer_tip = crate::tr_with(
            "%{name} writes the message from the changes, in a thread of its own",
            &[("name", &self.name_of(&writer))],
        );
        let d_write = desk.clone();
        let write_btn = Tooltip::new(
            "rc-write-tip",
            Button::new(
                "rc-write-msg",
                if s.git.writing_message {
                    crate::tr("Writing…")
                } else {
                    crate::tr("Write it for me")
                },
            )
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .icon(CarbonIcon::MagicWand)
            .disabled(s.git.writing_message || total_files == 0)
            .on_click(move |_, _, cx| {
                d_write.update(cx, |this, cx| this.ask_message(key, cx));
            }),
        )
        .content(writer_tip);

        let warn_banner = (comment_count > 0).then(|| {
            let (bold_part, rest_part) = if comment_count == 1 {
                (
                    crate::tr("1 comment isn’t sent yet."),
                    crate::tr("Committing now leaves it for a later turn."),
                )
            } else {
                (
                    crate::tr_with("%{n} comments aren’t sent yet.", &[("n", &comment_count)]),
                    crate::tr("Committing now leaves them for a later turn."),
                )
            };
            div()
                .relative()
                .flex()
                .items_start()
                .gap(px(8.))
                .mt(px(16.))
                .mx(px(16.))
                .pl(px(15.))
                .pr(px(12.))
                .py(px(8.))
                .bg(theme::role(ColorRole::NotificationBackgroundWarning))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(theme::role(ColorRole::SupportWarning)),
                )
                .child(
                    Icon::carbon(CarbonIcon::WarningAltFilled)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::SupportWarning))
                        .flex_none(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(
                            ui::text(faces, format!("{bold_part} {rest_part}"))
                                .type_style(TypeStyle::Label01),
                        ),
                )
        });

        let commit_label = if staged_files == 1 {
            crate::tr("Commit 1 file")
        } else {
            crate::tr_with("Commit %{n} files", &[("n", &staged_files)])
        };
        let can_commit = s.git_dir().is_some() && staged_files > 0 && idle;
        let (d_do_commit, d_do_push) = (desk.clone(), desk);

        let ahead = s.git.upstream.0;
        let first_unpushed_sha = s
            .git
            .commits
            .iter()
            .find(|c| c.unpushed)
            .map(|c| c.id.clone());
        let ahead_note = (ahead > 0).then(|| {
            let (lead, tail) = if ahead == 1 {
                let tail = match first_unpushed_sha {
                    Some(sha) => crate::tr_with(
                        "(%{sha}). Commit & push sends it with this one.",
                        &[("sha", &sha)],
                    ),
                    None => crate::tr("Commit & push sends it with this one."),
                };
                (crate::tr("1 earlier commit isn’t pushed"), tail)
            } else {
                (
                    crate::tr_with("%{n} earlier commits aren’t pushed.", &[("n", &ahead)]),
                    crate::tr("Commit & push sends them with this one."),
                )
            };
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .px(px(16.))
                .pb(px(16.))
                .child(
                    ui::text(faces, lead)
                        .type_style(TypeStyle::Label01)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::role(ColorRole::TextPrimary)),
                )
                .child(
                    ui::text(faces, tail)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
        });

        let commit_card = div()
            .flex()
            .flex_col()
            .bg(theme::role(ColorRole::Layer02))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(40.))
                    .px(px(16.))
                    .child(
                        ui::text(faces, crate::tr("Changes"))
                            .type_style(TypeStyle::HeadingCompact01)
                            .text_color(theme::role(ColorRole::TextPrimary)),
                    )
                    .child(
                        ui::text(
                            faces,
                            crate::tr_with(
                                "%{staged} of %{total} staged",
                                &[("staged", &staged_files), ("total", &total_files)],
                            ),
                        )
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                Icon::carbon(CarbonIcon::Branch)
                                    .size_length(px(16.))
                                    .color(theme::role(ColorRole::IconSecondary)),
                            )
                            .child(
                                ui::mono(faces, branch.to_owned())
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            ),
                    ),
            )
            .when(total_files > 0, |d| d.child(file_rows))
            .child(
                div()
                    .px(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(32.))
                            .child(
                                ui::text(faces, crate::tr("Commit message"))
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary)),
                            )
                            .child(write_btn),
                    )
                    .child(
                        Textarea::new("rc-commit-msg", &self.review.message)
                            .placeholder(crate::tr("Commit message"))
                            .rows(5)
                            .layer(LayerLevel::Two),
                    ),
            )
            .children(warn_banner)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .p(px(16.))
                    .child(
                        Button::new("rc-do-commit", commit_label)
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .disabled(!can_commit)
                            .on_click(move |_, _, cx| {
                                d_do_commit.update(cx, |this, cx| {
                                    let msg = this.commit_message(cx);
                                    this.git_action(GitAction::Commit(msg), cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("rc-do-push", crate::tr("Commit & push"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .disabled(!can_commit)
                            .on_click(move |_, _, cx| {
                                d_do_push.update(cx, |this, cx| {
                                    let msg = this.commit_message(cx);
                                    this.git_action(GitAction::CommitPush(msg), cx);
                                });
                            }),
                    ),
            )
            .children(ahead_note);

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(400.))
            .h_full()
            .border_l_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .bg(theme::role(ColorRole::Layer01))
            .child(
                div()
                    .id("rc-commit-rail")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .p(px(16.))
                    .gap(px(24.))
                    .children(pending_card)
                    .when(s.git_dir().is_some(), |d| d.child(commit_card))
                    .overflow_y_scrollbar(),
            )
    }
}
