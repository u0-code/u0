//! The column's changes as git reads them: all of the session's or one
//! turn's (between its checkpoints), what is not committed, the branch's
//! whole work against where it left the main one, or one commit's; then
//! what can be done with them. Beside them the recent commits, and the
//! branch's pull request with its checks.

use gpui::component::{
    Selectable as _, Sizable as _,
    input::Input,
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
    tab::{Tab, TabBar},
};
use gpui::{Context, Div, Entity, div, prelude::*, px};

use super::{fine, git_button, section};
use crate::{
    desk::Desk,
    fonts::Faces,
    git::Scope,
    session::Session,
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// Which changes are shown: all of them, a turn's, the branch's, a
    /// commit's (chosen from the recent ones), one call's.
    pub(super) fn scope_tabs(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let last = s.turns.iter().rposition(|t| t.diff.is_some());
        let turn = match self.review.scope {
            Scope::Turn(n) => Some(n),
            _ => last,
        };
        let all = if last.is_some() { "All" } else { "Uncommitted" };
        let mut tabs = vec![(all.to_owned(), Scope::All)];
        tabs.extend(turn.map(|n| (format!("Turn {}", n + 1), Scope::Turn(n))));
        tabs.push(("Branch".to_owned(), Scope::Branch));
        match &self.review.scope {
            Scope::Commit(id) => tabs.push((id.clone(), Scope::Commit(id.clone()))),
            Scope::Call(i) => tabs.push(("Edit".to_owned(), Scope::Call(*i))),
            _ => {}
        }
        let chosen = tabs
            .iter()
            .position(|(_, to)| match (to, &self.review.scope) {
                (Scope::Turn(_), Scope::Turn(_)) => true,
                (to, scope) => to == scope,
            });
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                TabBar::new("scope-tabs")
                    .segmented()
                    .xsmall()
                    .when_some(chosen, TabBar::selected_index)
                    .children(
                        tabs.iter()
                            .map(|(label, _)| Tab::new().label(label.clone())),
                    )
                    .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                        if let Some((_, to)) = tabs.get(*ix) {
                            this.show_changes(to.clone(), cx);
                        }
                    })),
            )
            .when(self.review.scope == Scope::Branch, |d| {
                d.child(self.compare_chip(faces, s, cx))
            })
            .when(!s.git.commits.is_empty(), |d| {
                let chip = ui::opener("commits")
                    .h(px(22.))
                    .px(px(6.))
                    .child(fine(faces, crate::tr("Commits")))
                    .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
                let (faces, key) = (faces.clone(), s.key);
                d.child(
                    self.menu(
                        crate::desk::Popup::Commits,
                        chip,
                        move |d, menu, desk, _| d.commits(&faces, key, menu, desk),
                        cx,
                    )
                    .anchor(gpui::Anchor::TopLeft),
                )
            })
    }

    /// Which of the repositories a folder of them holds git is asked about,
    /// for the changes, the commits and the pull request, as t3code's
    /// repository picker.
    pub(super) fn repo_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let shown = (s.git.repo.as_deref())
            .map(|r| {
                r.strip_prefix(&s.meta.folder)
                    .unwrap_or(r)
                    .display()
                    .to_string()
            })
            .unwrap_or_default();
        let chip = ui::opener("repository")
            .h(px(22.))
            .px(px(6.))
            .child(ui::one_line(fine(faces, shown)).max_w(px(140.)))
            .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
        let key = s.key;
        self.menu(
            crate::desk::Popup::Repository,
            chip,
            move |d, menu, desk, _| d.repo_menu(key, menu, desk),
            cx,
        )
        .anchor(gpui::Anchor::TopLeft)
    }

    /// The repositories of the session `key`'s folder: one chosen is read
    /// afresh, nothing of the one before kept.
    fn repo_menu(&self, key: u64, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let Some(s) = self.sessions.at(key) else {
            return menu;
        };
        for r in &s.git.repos {
            let chosen = r.clone();
            let name = r
                .strip_prefix(&s.meta.folder)
                .unwrap_or(r)
                .display()
                .to_string();
            menu = menu.item(
                Self::menu_item(desk, name, move |this, _, cx| {
                    if let Some(s) = this.sessions.at_mut(key) {
                        s.git = crate::git::View {
                            repos: std::mem::take(&mut s.git.repos),
                            repo: Some(chosen.clone()),
                            ..crate::git::View::default()
                        };
                    }
                    this.refresh_diffs(key);
                    this.refresh_git(key);
                    cx.notify();
                })
                .checked(s.git.repo.as_ref() == Some(r)),
            );
        }
        menu
    }

    /// What the branch is compared with: the main branch, or one chosen.
    fn compare_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let shown = s.git.compare_to.clone().unwrap_or_else(|| "main".into());
        let chip = ui::opener("compare")
            .h(px(22.))
            .px(px(6.))
            .child(fine(faces, "against"))
            .child(
                ui::one_line(ui::mono(faces, shown).text_size(px(crate::style::CAPTION)))
                    .max_w(px(140.))
                    .text_color(theme::text2()),
            )
            .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
        let key = s.key;
        // Under the chip, its right edge on the chip's.
        self.menu(
            crate::desk::Popup::Compare,
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

    /// The message to commit with and what can be done with it: commit,
    /// commit and push, a pull request; the writer can write the message.
    pub(super) fn commit_form(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        // The writer can write the message, in a session of its own.
        let key = s.key;
        let writer = self.writer_of(s);
        let write = ui::led_button(
            "ask-message",
            Some(ui::mark(faces, &self.mark_of(&writer), 12.)),
            "Write it",
            Weight::Quiet,
        )
        .h(px(24.))
        .px(px(7.))
        .loading(s.git.writing_message)
        .tooltip(format!(
            "{} writes the message from the changes, in a session of its own",
            self.name_of(&writer)
        ))
        .on_click(cx.listener(move |this, _, _, cx| this.ask_message(key, cx)));
        let what = format!(
            "Everything in {} on {}, as you, with this message or the session's title",
            Desk::shown(&s.meta.folder),
            s.branch.as_deref().unwrap_or("its branch")
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Input::new(&self.review.message).small().suffix(write))
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

    /// The recent commits where the session works; a click shows what one
    /// changed.
    fn commits(
        &self,
        faces: &Faces,
        key: u64,
        mut menu: PopupMenu,
        desk: &Entity<Self>,
    ) -> PopupMenu {
        let commits = self.sessions.at(key).map(|s| s.git.commits.as_slice());
        for (id, subject, when) in commits.unwrap_or_default() {
            let to = Scope::Commit(id.clone());
            let on = self.review.scope == to;
            let said = subject.clone();
            let (faces, id, subject, when) =
                (faces.clone(), id.clone(), subject.clone(), when.clone());
            menu = menu.item(
                PopupMenuItem::element(move |_, _| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w(px(300.))
                        .child(
                            ui::mono(&faces, id.clone())
                                .flex_none()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )
                        .child(
                            ui::one_line(
                                ui::text(&faces, subject.clone())
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::text2()),
                            )
                            .flex_1(),
                        )
                        .child(fine(&faces, when.clone()).flex_none())
                })
                .accessibility_label(said)
                .checked(on)
                .on_click(Self::does(desk, move |this, _, cx| {
                    this.show_changes(to.clone(), cx);
                })),
            );
        }
        menu
    }

    /// The branch's pull request: its state, its checks, the review asked.
    pub(super) fn pull_request(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let pr = s.git.pr.as_ref()?;
        let state = if pr.draft {
            "draft".to_owned()
        } else {
            pr.state.clone()
        };
        let mut checks = Vec::new();
        if pr.passed > 0 {
            checks.push((format!("{} passed", pr.passed), theme::ok()));
        }
        if pr.failed > 0 {
            checks.push((format!("{} failed", pr.failed), theme::coral()));
        }
        if pr.going > 0 {
            checks.push((format!("{} running", pr.going), theme::accent()));
        }
        let url = pr.url.clone();
        let key = s.key;
        let open = pr.state == "open" && !pr.draft;
        if pr.conflicts {
            checks.push(("conflicts with its base".into(), theme::coral()));
        }
        let mut line = div().flex().flex_wrap().gap(px(8.));
        for (text, color) in checks {
            line = line.child(fine(faces, text).text_color(color));
        }
        if !pr.review.is_empty() {
            line = line.child(fine(faces, pr.review.clone()));
        }
        Some(
            section(faces, crate::tr("Pull request"), Some(fine(faces, state)))
                .child(ui::one_line(
                    ui::text(faces, format!("#{} {}", pr.number, pr.title))
                        .text_size(px(crate::style::LABEL))
                        .text_color(theme::text()),
                ))
                .child(line)
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            ui::button("open-pr", crate::tr("Open"), Weight::Secondary)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                        .child(
                            ui::button("refresh-pr", crate::tr("Check again"), Weight::Quiet)
                                .on_click(cx.listener(move |this, _, _, _| this.refresh_git(key))),
                        )
                        .when(pr.state == "open", |d| {
                            let (watching, number) = (s.meta.watch == Some(pr.number), pr.number);
                            d.child(
                                ui::button(
                                    "watch-pr",
                                    if watching { "Watching" } else { "Watch" },
                                    Weight::Quiet,
                                )
                                .selected(watching)
                                .tooltip(crate::tr("Its checks finishing, or a review coming in, is said in this session"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(s) = this.sessions.at_mut(key) {
                                        s.meta.watch = (!watching).then_some(number);
                                    }
                                    this.save_index();
                                    cx.notify();
                                })),
                            )
                        }),
                )
                // The way on from where it stands: merged once clear; what
                // stops it, handed to the agent (a turn, spent as any is).
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .when(open && pr.failed == 0 && pr.going == 0 && !pr.conflicts, |d| {
                            d.child(ui::button("merge-pr", crate::tr("Merge"), Weight::Secondary).on_click(
                                cx.listener(|this, _, _, cx| {
                                    let p = this.session().map(crate::desk::Place::project_of);
                                    let s = this.settings_in(p.as_ref());
                                    this.git_action(crate::repo::GitAction::Merge(s.merge, s.uncredit), cx);
                                }),
                            ))
                        })
                        .when(open && pr.failed > 0, |d| {
                            d.child(ui::button("fix-checks", crate::tr("Have it fix the checks"), Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request's checks failed. Look at them (gh pr checks, and the failed runs' logs), fix what broke, commit and push.", cx);
                                }),
                            ))
                        })
                        .when(open && pr.conflicts, |d| {
                            d.child(ui::button("fix-conflicts", crate::tr("Have it resolve the conflicts"), Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request conflicts with its base branch. Bring the base in, resolve the conflicts, run the tests, commit and push.", cx);
                                }),
                            ))
                        }),
                ),
        )
    }
}
