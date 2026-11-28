//! The column's changes as git reads them (`RightColumnStates` Columns 1 & 3):
//! multi-repository `.rp-bar` with the `376px` `Layer02` repository picker
//! popover and inline Carbon `Select` for `Changes` scope, commit message form,
//! and the `Pull request` tab with `.pr-box` merge readiness, `Watch` switch,
//! `ChecksList`, and conflict resolution card.

use gpui::component::{menu::PopupMenu, popover::Popover};
use gpui::{Context, Div, Entity, SharedString, div, prelude::*, px};
use u0ui::{
    components::{
        Badge, BadgeSize, BadgeVariant, Button, ButtonSize, ButtonVariant, FieldSize, Select,
        SelectType, Switch, SwitchSize, Textarea, Tooltip, list_box::ListItem as SelectItem,
    },
    icons::{CarbonIcon, Icon},
    t3::review::{Check, ChecksList, ChecksStatus, ReviewDecision, ReviewDecisionBadge},
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use super::git_button;
use u0ui::u0::foundations::truncate::{Truncate, TruncateMode};

/// The repository picker's width (`.rp-menu`): the 400px column less 12px
/// each side.
const REPO_MENU: f32 = 376.;
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    git::{Method, Scope},
    session::Session,
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// `.rp-bar` (`RightColumnStates`) with a repository or branch picker
    /// on the left, `.rp-scope` (`Main`) without one: the inline Carbon
    /// `Select` (`Changes`), then `Split view`.
    pub(super) fn diff_bar(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let chip = if s.git.repos.is_empty() {
            (s.branch.as_deref()).map(|branch| self.branch_chip(faces, s, branch, cx))
        } else {
            Some(self.repo_chip(faces, s, cx))
        };
        let mut bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .pl(px(if chip.is_some() { 12. } else { 16. }))
            .pr(px(8.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .children(chip);

        if s.git_dir().is_some() {
            bar = bar.child(div().flex_1().min_w(px(0.)).child(self.scope_select(s, cx)));
            if self.review.scope == Scope::Branch && !s.git.branches.is_empty() {
                bar = bar.child(self.compare_chip(faces, s, cx));
            }
        } else {
            bar = bar.child(div().flex_1());
        }
        bar.child(
            Button::new("diff-split", crate::tr("Split view"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(CarbonIcon::Grid)
                .icon_only(true)
                .selected(self.settings.side_by_side)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.side_by_side = !this.settings.side_by_side;
                    this.save_settings();
                    cx.notify();
                })),
        )
    }

    /// Inline Carbon `Select` (`Changes`: `Last turn`, `All turns`, `Uncommitted`, `Against <base>`).
    pub(super) fn scope_select(&self, s: &Session, cx: &mut Context<Self>) -> Select {
        let last = s.turns.iter().rposition(|t| t.diff.is_some());
        let turn_ix = match self.review.scope {
            Scope::Turn(n) => Some(n),
            _ => last,
        };
        let base = s
            .git
            .compare_to
            .as_deref()
            .or_else(|| s.git.pr.as_ref().map(|p| p.base.as_str()))
            .filter(|b| !b.is_empty())
            .unwrap_or("main");

        let mut options: Vec<(String, String, Scope)> = Vec::new();
        if let Some(n) = turn_ix {
            let label = if Some(n) == last {
                crate::tr("Last turn").to_string()
            } else {
                crate::tr_with("Turn %{n}", &[("n", &(n + 1))]).to_string()
            };
            options.push((format!("turn:{n}"), label, Scope::Turn(n)));
            options.push(("all".into(), crate::tr("All turns").to_string(), Scope::All));
            options.push((
                "wip".into(),
                crate::tr("Uncommitted").to_string(),
                Scope::Uncommitted,
            ));
        } else {
            options.push((
                "all".into(),
                crate::tr("Uncommitted").to_string(),
                Scope::All,
            ));
        }
        options.push((
            "base".into(),
            crate::tr_with("Against %{base}", &[("base", &base)]).to_string(),
            Scope::Branch,
        ));
        match &self.review.scope {
            Scope::Commit(id) => options.push((
                format!("commit:{id}"),
                crate::tr_with("Commit %{id}", &[("id", &id)]).to_string(),
                Scope::Commit(id.clone()),
            )),
            Scope::Call(i) => options.push((
                format!("call:{i}"),
                crate::tr("Edit").to_string(),
                Scope::Call(*i),
            )),
            _ => {}
        }

        let current_value = options
            .iter()
            .find(|(_, _, sc)| match (sc, &self.review.scope) {
                (Scope::Turn(_), Scope::Turn(_)) => true,
                (a, b) => a == b,
            })
            .map_or_else(|| "all".into(), |(v, _, _)| v.clone());

        let items: Vec<SelectItem> = options
            .iter()
            .map(|(v, l, _)| SelectItem::labeled(v.clone(), l.clone()))
            .collect();
        let desk = cx.entity();
        Select::new("diff-scope")
            .kind(SelectType::Inline)
            .size(FieldSize::Sm)
            .layer(LayerLevel::One)
            .label(crate::tr("Changes"))
            .items(items)
            .value(Some(current_value))
            .on_value_change(move |val, _, cx| {
                let Some(val) = val else { return };
                if let Some((_, _, to)) = options.iter().find(|(v, _, _)| v == val.as_ref()) {
                    let to = to.clone();
                    desk.update(cx, |this, cx| this.show_changes(to, cx));
                }
            })
    }

    /// Multi-repository picker (`RightColumnStates` Column 1): a ghost `sm`
    /// button (32px, its chevron 16) opening the `Layer02` popover listing
    /// each repository's branch and changes.
    pub(super) fn repo_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let shown = s
            .git
            .repo
            .as_deref()
            .and_then(|r| r.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| crate::tr("Repository").into());
        let link = theme::role(ColorRole::LinkPrimary);
        let chip = ui::opener("repository")
            .h(px(32.))
            .px(px(12.))
            .gap(px(8.))
            .child(
                ui::text(faces, shown)
                    .type_style(TypeStyle::BodyCompact01)
                    .text_color(link),
            )
            .child(
                Icon::carbon(CarbonIcon::ChevronDown)
                    .size_length(px(16.))
                    .color(link),
            );
        let (faces, key) = (faces.clone(), s.key);
        self.popover(
            Popup::Repository,
            chip,
            move |d, _, cx| d.repo_popover(&faces, key, cx),
            cx,
        )
        .p(px(0.))
        .rounded(px(0.))
        .anchor(gpui::Anchor::TopLeft)
    }

    /// `.rp-menu`: the `Layer02` popover, the column's width less its 12px
    /// sides, with its header and `.rp-opt` rows.
    fn repo_popover(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.sessions.at(key) else {
            return div();
        };
        let project = s.meta.project.file_name().map_or_else(
            || Desk::shown(&s.meta.project),
            |n| n.to_string_lossy().into_owned(),
        );
        let total = s.git.repos.len();
        let header = crate::tr_with(
            "%{project} \u{00b7} %{n} repositories \u{00b7} each turn checkpointed in all %{n}",
            &[("project", &project), ("n", &total)],
        );
        let mut list = div()
            .w(px(REPO_MENU))
            .flex()
            .flex_col()
            .bg(theme::role(ColorRole::Layer02))
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .child(
                        ui::text(faces, header)
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    ),
            );
        for (i, r) in s.git.repos.iter().enumerate() {
            let selected = s.git.repo.as_ref() == Some(r);
            let chosen = r.clone();
            let leaf = r.file_name().map_or_else(
                || r.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let rel = r
                .strip_prefix(&s.meta.folder)
                .unwrap_or(r)
                .display()
                .to_string();
            let summary = s.git.summaries.iter().find(|sum| &sum.path == r);
            let sub = match summary.and_then(|sum| sum.branch.as_deref()) {
                Some(b) => format!("{rel} \u{00b7} {b}"),
                None => rel,
            };
            let state_text = match summary {
                Some(sum) if sum.files > 0 => {
                    let files_word = if sum.files == 1 {
                        crate::tr("1 file").to_string()
                    } else {
                        crate::tr_with("%{n} files", &[("n", &sum.files)]).to_string()
                    };
                    SharedString::from(format!(
                        "+{} \u{2212}{} \u{00b7} {files_word}",
                        sum.added, sum.gone
                    ))
                }
                Some(sum) if sum.behind > 0 => crate::tr_with("%{n} behind", &[("n", &sum.behind)]),
                _ => crate::tr("No changes"),
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("repo-opt-{i}")))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(48.))
                    .px(px(12.))
                    .py(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme::role(ColorRole::LayerSelected02)))
                    .hover(|st| st.bg(theme::role(ColorRole::LayerHover02)))
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(theme::role(ColorRole::BorderInteractive)),
                        )
                    })
                    .child(
                        Icon::carbon(CarbonIcon::Folder)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(2.))
                            .child(ui::one_line(
                                ui::text(faces, leaf)
                                    .type_style(TypeStyle::Body01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            ))
                            .child(
                                // A path and a branch keep both ends (`LongAndMany`).
                                div()
                                    .min_w(px(0.))
                                    .font_family(faces.mono.clone())
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(
                                        Truncate::new(
                                            SharedString::from(format!("repo-sub-{i}")),
                                            sub,
                                        )
                                        .mode(TruncateMode::Middle),
                                    ),
                            ),
                    )
                    .child(
                        ui::text(faces, state_text)
                            .flex_none()
                            .max_w(px(88.))
                            .text_right()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.popup = Popup::Closed;
                        if let Some(s) = this.sessions.at_mut(key) {
                            s.git = crate::git::View {
                                repos: std::mem::take(&mut s.git.repos),
                                summaries: std::mem::take(&mut s.git.summaries),
                                repo: Some(chosen.clone()),
                                ..crate::git::View::default()
                            };
                        }
                        this.refresh_diffs(key);
                        this.refresh_git(key);
                        cx.notify();
                    })),
            );
        }
        list
    }

    /// What the branch is compared with: the main branch, or one chosen.
    pub(super) fn compare_chip(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Popover {
        let shown = s.git.compare_to.clone().unwrap_or_else(|| "main".into());
        let chip = ui::opener("compare")
            .h(px(24.))
            .px(px(6.))
            .child(
                div()
                    .max_w(px(120.))
                    .min_w(px(0.))
                    .font_family(faces.mono.clone())
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(Truncate::new("compare-chip-name", shown).mode(TruncateMode::Middle)),
            )
            .child(
                Icon::carbon(CarbonIcon::ChevronDown)
                    .size_length(px(12.))
                    .color(theme::role(ColorRole::IconSecondary)),
            );
        let key = s.key;
        self.menu(
            Popup::Compare,
            chip,
            move |d, menu, desk, _| d.compare_menu(key, menu, desk),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    /// The other branches: the one it is on compares with nothing.
    fn compare_menu(&self, key: u64, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let Some(s) = self.sessions.at(key) else {
            return menu;
        };
        let others = (s.git.branches.iter()).filter(|b| Some(*b) != s.branch.as_ref());
        let mut any = false;
        for b in others {
            any = true;
            let chosen = b.clone();
            menu = menu.item(
                Self::menu_item(desk, ui::fit(b, 28), move |this, _, cx| {
                    if let Some(s) = this.sessions.at_mut(key) {
                        s.git.compare_to = Some(chosen.clone());
                    }
                    this.refresh_diffs(key);
                    cx.notify();
                })
                .checked(s.git.compare_to.as_ref() == Some(b)),
            );
        }
        if any {
            menu
        } else {
            menu.label(crate::tr("No other branch here to compare with."))
        }
    }

    /// Inline commit message form under the diff.
    pub(super) fn commit_form(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let desk = cx.entity();
        let writer = self.writer_of(s);
        let write_tip = crate::tr_with(
            "%{name} writes the message from the changes, in a thread of its own",
            &[("name", &self.name_of(&writer))],
        );
        let write_label = if s.git.writing_message {
            crate::tr("Writing…")
        } else {
            crate::tr("Write it for me")
        };
        let write = Tooltip::new(
            "ask-msg-tip",
            Button::new("ask-message", write_label)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(CarbonIcon::MagicWand)
                .disabled(s.git.writing_message)
                .on_click(move |_, _, cx| {
                    desk.update(cx, |this, cx| this.ask_message(key, cx));
                }),
        )
        .content(write_tip);
        let branch = s
            .branch
            .clone()
            .map_or_else(|| crate::tr("its branch"), Into::into);
        let what = crate::tr_with(
            "All of %{folder} on %{branch}, committed as you",
            &[
                ("folder", &Desk::shown(&s.meta.folder)),
                ("branch", &branch),
            ],
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(28.))
                    .child(
                        ui::text(faces, crate::tr("Commit message"))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .child(write),
            )
            .child(
                Textarea::new("commit-msg", &self.review.message)
                    .placeholder(crate::tr("Commit message"))
                    .rows(3)
                    .layer(LayerLevel::One),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        git_button(
                            "do-commit",
                            "Commit",
                            Weight::Primary,
                            cx,
                            crate::repo::GitAction::Commit,
                        )
                        .tooltip(what),
                    )
                    .child(
                        git_button(
                            "do-push",
                            "Commit & push",
                            Weight::Secondary,
                            cx,
                            crate::repo::GitAction::CommitPush,
                        )
                        .tooltip(crate::tr(
                            "On the main branch, a branch of its own is made first",
                        )),
                    )
                    .child(
                        ui::button("do-pr", crate::tr("Create PR"), Weight::Secondary)
                            .tooltip(crate::tr(
                                "Its title and description by the writer, from the branch's work",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.ask_pr(key, cx);
                            })),
                    ),
            )
    }

    /// The `Pull request` tab (`RightColumnStates` Column 3).
    pub(super) fn pull_request(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let pr = s.git.pr.as_ref()?;
        let open = pr.state == "open" && !pr.draft;
        let (state_label, badge_variant) = if pr.draft {
            (crate::tr("Draft"), BadgeVariant::Gray)
        } else if pr.state == "merged" {
            (crate::tr("Merged"), BadgeVariant::Purple)
        } else if pr.state == "closed" {
            (crate::tr("Closed"), BadgeVariant::Red)
        } else {
            (crate::tr("Open"), BadgeVariant::Green)
        };
        let decision = match pr.review.as_str() {
            "approved" => Some(ReviewDecision::Approved),
            s if s.contains("changes") => Some(ReviewDecision::Changes),
            s if s.contains("review") => Some(ReviewDecision::Awaiting),
            _ => None,
        };
        let slug = pr
            .url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .skip(1)
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
        let head = if pr.head.is_empty() {
            s.branch.as_deref().unwrap_or("HEAD")
        } else {
            pr.head.as_str()
        };
        let base = if pr.base.is_empty() {
            "main"
        } else {
            pr.base.as_str()
        };
        let meta_line = if slug.is_empty() {
            format!("#{} \u{00b7} {head} \u{2192} {base}", pr.number)
        } else {
            format!(
                "{slug} \u{00b7} #{} \u{00b7} {head} \u{2192} {base}",
                pr.number
            )
        };

        // Condition rows inside `.pr-box`.
        let cond_row =
            |icon: CarbonIcon, ink: ColorRole, main: SharedString, sub: Option<String>| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div().mt(px(2.)).flex_none().child(
                            Icon::carbon(icon)
                                .size_length(px(16.))
                                .color(theme::role(ink)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::text(faces, main)
                                    .type_style(TypeStyle::Body01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            )
                            .children(sub.map(|small| {
                                ui::text(faces, small)
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                            })),
                    )
            };

        let mut conds = div().flex().flex_col().gap(px(8.));
        let mut any_cond = false;
        if pr.failed > 0 {
            any_cond = true;
            let main = if pr.failed == 1 {
                crate::tr_with(
                    "%{failed} check failed, %{passed} passed",
                    &[("failed", &pr.failed), ("passed", &pr.passed)],
                )
            } else {
                crate::tr_with(
                    "%{failed} checks failed, %{passed} passed",
                    &[("failed", &pr.failed), ("passed", &pr.passed)],
                )
            };
            let first_failed = pr
                .checks
                .iter()
                .find(|c| {
                    matches!(
                        c.state.as_str(),
                        "failure" | "error" | "cancelled" | "timed_out" | "action_required"
                    )
                })
                .map(|c| match c.duration.as_deref() {
                    Some(d) if !d.is_empty() => format!("{} ({d})", c.name),
                    _ => c.name.clone(),
                });
            conds = conds.child(cond_row(
                CarbonIcon::ErrorFilled,
                ColorRole::SupportError,
                main,
                first_failed,
            ));
        } else if pr.going > 0 {
            any_cond = true;
            let main = crate::tr_with(
                "%{going} checks running, %{passed} passed",
                &[("going", &pr.going), ("passed", &pr.passed)],
            );
            conds = conds.child(cond_row(
                CarbonIcon::InProgress,
                ColorRole::SupportWarning,
                main,
                None,
            ));
        } else if pr.passed > 0 {
            any_cond = true;
            let main = crate::tr_with("%{passed} checks passed", &[("passed", &pr.passed)]);
            conds = conds.child(cond_row(
                CarbonIcon::CheckmarkFilled,
                ColorRole::SupportSuccess,
                main,
                None,
            ));
        }

        if pr.behind {
            any_cond = true;
            let behind_n = s.git.upstream.1;
            let main = if behind_n > 0 {
                crate::tr_with(
                    "%{n} commits behind %{base}",
                    &[("n", &behind_n), ("base", &base)],
                )
            } else {
                crate::tr_with("Behind %{base}", &[("base", &base)])
            };
            let sub = (!pr.conflicts).then(|| crate::tr("No conflicts yet").to_string());
            conds = conds.child(cond_row(
                CarbonIcon::WarningFilled,
                ColorRole::SupportWarning,
                main,
                sub,
            ));
        }

        if let Some((who, _, body)) = &pr.review_note {
            any_cond = true;
            let main = if decision == Some(ReviewDecision::Changes) {
                crate::tr_with("Changes requested by %{who}", &[("who", &who)])
            } else {
                crate::tr_with("Review from %{who}", &[("who", &who)])
            };
            conds = conds.child(cond_row(
                CarbonIcon::WarningFilled,
                ColorRole::SupportWarning,
                main,
                Some(format!("\u{201c}{body}\u{201d}")),
            ));
        }

        let project_scope = crate::desk::Place::project_of(s);
        let merge_method = self.settings_in(Some(&project_scope)).merge;
        let merge_label = match merge_method {
            Method::Squash => crate::tr("Squash and merge…"),
            Method::Merge => crate::tr("Create merge commit…"),
            Method::Rebase => crate::tr("Rebase and merge…"),
        };
        // Why Merge waits, said where it is pressed (`title="1 check failed"`).
        let merge_waits = if pr.failed == 1 {
            Some(crate::tr("1 check failed"))
        } else if pr.failed > 1 {
            Some(crate::tr_with("%{n} checks failed", &[("n", &pr.failed)]))
        } else if pr.going > 0 {
            Some(crate::tr("Its checks are still running"))
        } else if pr.conflicts {
            Some(crate::tr_with(
                "It conflicts with %{base}",
                &[("base", &base)],
            ))
        } else if pr.draft {
            Some(crate::tr("A draft is not merged"))
        } else {
            None
        };
        let merge_blocked = !open || merge_waits.is_some();
        let (key, url) = (s.key, pr.url.clone());
        let desk = cx.entity();

        let has_fix_or_update = open && (pr.failed > 0 || (pr.behind && !pr.conflicts));
        let mut acts_top = div().flex().flex_wrap().gap(px(8.));
        if open && pr.failed > 0 {
            let d = desk.clone();
            acts_top = acts_top.child(
                Button::new("fix-checks", crate::tr("Have it fix the checks"))
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Chat)
                    .on_click(move |_, _, cx| {
                        d.update(cx, |this, cx| {
                            this.say(
                                "The pull request's checks failed. Look at them (gh pr checks, and the failed runs' logs), fix what broke, commit and push.",
                                cx,
                            );
                        });
                    }),
            );
        }
        if open && pr.behind && !pr.conflicts {
            let (d1, d2) = (desk.clone(), desk.clone());
            acts_top = acts_top
                .child(
                    Button::new("update-branch", crate::tr("Update branch"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            d1.update(cx, |this, cx| {
                                this.git_action(crate::repo::GitAction::UpdateBranch(false), cx);
                            });
                        }),
                )
                .child(
                    Button::new("update-rebase", crate::tr("Update with rebase"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            d2.update(cx, |this, cx| {
                                this.git_action(crate::repo::GitAction::UpdateBranch(true), cx);
                            });
                        }),
                );
        }

        let (d_merge, d_refresh) = (desk.clone(), desk.clone());
        let open_url = url.clone();
        let acts_bottom = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .when(pr.state == "open", |d| {
                let merge = Button::new("merge-pr", merge_label)
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Sm)
                    .disabled(merge_blocked)
                    .on_click(move |_, _, cx| {
                        d_merge.update(cx, |this, cx| {
                            let p = this.session().map(crate::desk::Place::project_of);
                            let cfg = this.settings_in(p.as_ref());
                            this.git_action(
                                crate::repo::GitAction::Merge(cfg.merge, cfg.uncredit),
                                cx,
                            );
                        });
                    });
                match merge_waits {
                    Some(why) => d.child(Tooltip::new("merge-pr-why", merge).content(why)),
                    None => d.child(merge),
                }
            })
            .child(
                Button::new("open-pr", crate::tr("Open"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Launch)
                    .on_click(cx.listener(move |this, _, _, cx| this.browse(&open_url, cx))),
            )
            .child(
                Button::new("refresh-pr", crate::tr("Check again"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, _, cx| {
                        d_refresh.update(cx, |this, _| this.refresh_git(key));
                    }),
            );

        let merge_box = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .bg(theme::role(ColorRole::Layer02))
            .border_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .when(any_cond, |d| d.child(conds))
            .when(has_fix_or_update, |d| d.child(acts_top))
            .child(acts_bottom);

        let watch_row = (pr.state == "open").then(|| {
            let (watching, number) = (s.meta.watch == Some(pr.number), pr.number);
            let agent_name = self.name_of(&s.meta.agent);
            let d_watch = desk.clone();
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(
                            ui::text(faces, crate::tr("Watch"))
                                .type_style(TypeStyle::Body01)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        )
                        .child(
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "Checked every two minutes; when its checks finish or a review arrives, %{name} is told in this thread.",
                                    &[("name", &agent_name)],
                                ),
                            )
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                        ),
                )
                .child(
                    Switch::new("watch-pr")
                        .label(crate::tr("Watch"))
                        .hide_label(true)
                        .size(SwitchSize::Sm)
                        .checked(watching)
                        .on_checked_change(move |on, _, cx| {
                            d_watch.update(cx, |this, cx| {
                                if let Some(s) = this.sessions.at_mut(key) {
                                    s.meta.watch = on.then_some(number);
                                }
                                this.save_index();
                                cx.notify();
                            });
                        }),
                )
        });

        let checks_list = (!pr.checks.is_empty()).then(|| {
            let desk = desk.clone();
            let urls: Vec<String> = pr.checks.iter().map(|c| c.url.clone()).collect();
            let items: Vec<Check> = pr
                .checks
                .iter()
                .map(|c| {
                    let st = match c.state.as_str() {
                        "success" | "neutral" | "skipped" => ChecksStatus::Passed,
                        "failure" | "error" | "cancelled" | "timed_out" | "action_required" => {
                            ChecksStatus::Failed
                        }
                        "pending" | "queued" | "in_progress" | "expected" | "" => {
                            ChecksStatus::Running
                        }
                        _ => ChecksStatus::Passed,
                    };
                    Check::new(c.name.clone(), st, c.duration.clone().unwrap_or_default())
                })
                .collect();
            ChecksList::new("pr-checks")
                .checks(items)
                .details_label(crate::tr("Details"))
                .on_details(move |ix, _, cx| {
                    if let Some(u) = urls.get(ix).filter(|u| !u.is_empty()) {
                        desk.update(cx, |d, cx| d.browse(u, cx));
                    }
                })
        });

        let conflict_box = (open && pr.conflicts).then(|| {
            let d_conf = desk.clone();
            let base_str = base.to_owned();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(16.))
                .bg(theme::role(ColorRole::Layer02))
                .border_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    ui::text(
                        faces,
                        crate::tr_with("Once %{base} moves on and it conflicts:", &[("base", &base_str)]),
                    )
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .child(cond_row(
                    CarbonIcon::ErrorFilled,
                    ColorRole::SupportError,
                    crate::tr_with("Conflicts with %{base}", &[("base", &base_str)]),
                    None,
                ))
                .child(
                    div().flex().gap(px(8.)).child(
                        Button::new("fix-conflicts", crate::tr("Have it resolve the conflicts"))
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Chat)
                            .on_click(move |_, _, cx| {
                                d_conf.update(cx, |this, cx| {
                                    this.say(
                                        "The pull request conflicts with its base branch. Bring the base in, resolve the conflicts, run the tests, commit and push.",
                                        cx,
                                    );
                                });
                            }),
                    ),
                )
        });

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .p(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            ui::mono(faces, meta_line)
                                .type_style(TypeStyle::Code01)
                                .text_color(theme::role(ColorRole::TextSecondary)),
                        )
                        .child(
                            // `.pr-title`: 20/28 at 600.
                            ui::text(faces, pr.title.clone())
                                .type_style(TypeStyle::Heading03)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    Badge::new(state_label)
                                        .variant(badge_variant)
                                        .size(BadgeSize::Sm),
                                )
                                .child(
                                    ui::text(faces, crate::tr("Opened"))
                                        .type_style(TypeStyle::Label01)
                                        .text_color(theme::role(ColorRole::TextSecondary)),
                                )
                                .children(decision.map(ReviewDecisionBadge::new)),
                        ),
                )
                .child(merge_box)
                .children(watch_row)
                .children(checks_list)
                .children(conflict_box),
        )
    }
}
