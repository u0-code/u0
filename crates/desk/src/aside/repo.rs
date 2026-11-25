//! The column's changes as git reads them: all of the session's or one
//! turn's (between its checkpoints), what is not committed, the branch's
//! whole work against where it left the main one, or one commit's; then
//! what can be done with them. Beside them the recent commits, and the
//! branch's pull request with its checks.

use gpui::component::{
    Sizable as _,
    input::Input,
    popover::Popover,
    tab::{Tab, TabBar},
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

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
                let faces = faces.clone();
                let chip = ui::opener("commits")
                    .h(px(22.))
                    .px(px(6.))
                    .child(fine(&faces, "Commits"))
                    .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
                let key = s.key;
                d.child(
                    self.menu(
                        crate::desk::Popup::Commits,
                        chip,
                        move |d, _, cx| d.commits(&faces, key, cx),
                        cx,
                    )
                    .anchor(gpui::Anchor::TopLeft),
                )
            })
    }

    /// What the branch is compared with: the main branch, or one chosen.
    fn compare_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let shown = s.git.compare_to.clone().unwrap_or_else(|| "main".into());
        let chip = ui::opener("compare")
            .h(px(22.))
            .px(px(6.))
            .child(fine(faces, "against"))
            .child(
                ui::mono(faces, ui::fit(&shown, 18))
                    .text_size(px(12.))
                    .text_color(theme::text2()),
            )
            .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
        let (faces, key) = (faces.clone(), s.key);
        // Under the chip, its right edge on the chip's.
        self.menu(
            crate::desk::Popup::Compare,
            chip,
            move |d, _, cx| d.compare_menu(&faces, key, cx),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    /// The other branches: the one it is on compares with nothing.
    fn compare_menu(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        let others: Vec<&String> = self.sessions.at(key).map_or_else(Vec::new, |s| {
            s.git
                .branches
                .iter()
                .filter(|b| Some(*b) != s.branch.as_ref())
                .collect()
        });
        let mut list = div().flex().flex_col().w(px(208.));
        if others.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .child(fine(faces, "No other branch here to compare with.")),
            );
        }
        for (n, b) in others.into_iter().enumerate() {
            let b = b.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("compare-{n}")))
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(ui::mono(faces, ui::fit(&b, 28)).text_size(px(12.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = this.sessions.at_mut(key) {
                            s.git.compare_to = Some(b.clone());
                        }
                        this.popup = crate::desk::Popup::Closed;
                        this.refresh_diffs(key);
                        cx.notify();
                    })),
            );
        }
        list
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
                        .tooltip("On the main branch, a branch of its own is made first"),
                    )
                    .child(
                        ui::button("do-pr", "Create PR", Weight::Secondary)
                            .tooltip(
                                "Its title and description by the writer, from the branch's work",
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.ask_pr(key, cx);
                            })),
                    ),
            )
    }

    /// The recent commits where the session works; a click shows what one
    /// changed.
    fn commits(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().w(px(320.));
        let commits = self.sessions.at(key).map(|s| s.git.commits.as_slice());
        for (id, subject, when) in commits.unwrap_or_default() {
            let to = Scope::Commit(id.clone());
            rows = rows.child(
                ui::menu_row(
                    SharedString::from(format!("commit-{id}")),
                    self.review.scope == to,
                )
                .child(
                    ui::mono(faces, id.clone())
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(theme::faint()),
                )
                .child(
                    ui::one_line(
                        ui::text(faces, ui::fit(subject, 30))
                            .text_size(px(12.5))
                            .text_color(theme::text2()),
                    )
                    .flex_1(),
                )
                .child(fine(faces, when.clone()).flex_none())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.popup = crate::desk::Popup::Closed;
                    this.show_changes(to.clone(), cx);
                })),
            );
        }
        rows
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
            section(faces, "Pull request", Some(fine(faces, state)))
                .child(ui::one_line(
                    ui::text(faces, format!("#{} {}", pr.number, pr.title))
                        .text_size(px(13.))
                        .text_color(theme::text()),
                ))
                .child(line)
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            ui::button("open-pr", "Open", Weight::Secondary)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                        .child(
                            ui::button("refresh-pr", "Check again", Weight::Quiet)
                                .on_click(cx.listener(move |this, _, _, _| this.refresh_git(key))),
                        ),
                )
                // The way on from where it stands: merged once clear; what
                // stops it, handed to the agent (a turn, spent as any is).
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .when(open && pr.failed == 0 && pr.going == 0 && !pr.conflicts, |d| {
                            d.child(ui::button("merge-pr", "Merge", Weight::Secondary).on_click(
                                cx.listener(|this, _, _, cx| this.git_action(crate::repo::GitAction::Merge(this.settings.merge), cx)),
                            ))
                        })
                        .when(open && pr.failed > 0, |d| {
                            d.child(ui::button("fix-checks", "Have it fix the checks", Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request's checks failed. Look at them (gh pr checks, and the failed runs' logs), fix what broke, commit and push.", cx);
                                }),
                            ))
                        })
                        .when(open && pr.conflicts, |d| {
                            d.child(ui::button("fix-conflicts", "Have it resolve the conflicts", Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request conflicts with its base branch. Bring the base in, resolve the conflicts, run the tests, commit and push.", cx);
                                }),
                            ))
                        }),
                ),
        )
    }
}
