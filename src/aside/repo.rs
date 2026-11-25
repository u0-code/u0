//! The column's changes as git reads them: all of the session's or one
//! turn's (between its checkpoints), what is not committed, the branch's
//! whole work against where it left the main one, or one commit's; then
//! what can be done with them. Beside them the recent commits, and the
//! branch's pull request with its checks.

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{added_gone, empty, fine, frow, git_button, path_text, section};
use crate::{
    desk::{Aside, Desk, Scope},
    fonts::Faces,
    git, hub,
    session::{Session, State},
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// The changes the scope asks for, as git read them, when it has.
    pub(super) fn scoped<'a>(&self, s: &'a Session) -> Option<&'a Vec<git::FileDiff>> {
        match &self.scope {
            Scope::All => s.total.as_ref(),
            Scope::Turn(n) => s.turns.get(*n).and_then(|t| t.diff.as_ref()),
            scope => s
                .scoped
                .as_ref()
                .filter(|(at, _)| at == scope)
                .map(|(_, files)| files),
        }
    }

    /// Which changes are shown: all of them, a turn's, the branch's, a
    /// commit's.
    pub(super) fn scope_tabs(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let last = s.turns.iter().rposition(|t| t.diff.is_some());
        let scope_tab = |id: &'static str, label: String, on: bool, to: Scope| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(22.))
                .px(px(7.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme::hover()))
                .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
                .child(ui::text(faces, label).text_size(px(12.)).text_color(if on {
                    theme::text()
                } else {
                    theme::muted()
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.show_changes(to.clone(), cx)))
        };
        let turn = match self.scope {
            Scope::Turn(n) => Some(n),
            _ => last,
        };
        div()
            .flex()
            .gap(px(2.))
            .child(scope_tab(
                "scope-all",
                if last.is_some() { "All" } else { "Uncommitted" }.into(),
                self.scope == Scope::All,
                Scope::All,
            ))
            .children(turn.map(|turn| {
                scope_tab(
                    "scope-turn",
                    format!("Turn {}", turn + 1),
                    matches!(self.scope, Scope::Turn(_)),
                    Scope::Turn(turn),
                )
            }))
            .child(scope_tab(
                "scope-branch",
                "Branch".into(),
                self.scope == Scope::Branch,
                Scope::Branch,
            ))
            .children(match &self.scope {
                Scope::Commit(id) => Some(scope_tab(
                    "scope-commit",
                    id.clone(),
                    true,
                    Scope::Commit(id.clone()),
                )),
                _ => None,
            })
    }

    /// The changes as git reads them between the session's checkpoints:
    /// all of them or one turn's, each file with how it changed; then what
    /// can be done with them. A session loaded, with no turn taken here
    /// yet, shows what is not committed. `None` before git has read
    /// anything.
    pub(super) fn git_changes(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let last = s.turns.iter().rposition(|t| t.diff.is_some());
        if last.is_none()
            && s.total.is_none()
            && !matches!(self.scope, Scope::Branch | Scope::Commit(_))
        {
            return None;
        }
        let reading =
            matches!(self.scope, Scope::Branch | Scope::Commit(_)) && self.scoped(s).is_none();
        let files = self.scoped(s).cloned().unwrap_or_default();
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let mut sec = section(
            faces,
            "Changes",
            (!files.is_empty()).then(|| added_gone(faces, added, gone)),
        )
        .child(self.scope_tabs(faces, s, cx).ml(px(-7.)));
        if reading {
            sec = sec.child(empty(faces, "Reading git…"));
        } else if files.is_empty() {
            sec = sec.child(empty(faces, "Nothing changed here."));
        }
        let mut rows = div().flex().flex_col();
        for (n, f) in files.iter().enumerate() {
            let tail = match f.status {
                git::Status::Added => fine(faces, "new"),
                git::Status::Deleted => fine(faces, "deleted").text_color(theme::coral()),
                git::Status::Renamed if f.added + f.gone == 0 => fine(faces, "renamed"),
                _ => added_gone(faces, f.added, f.gone),
            };
            let path = std::path::PathBuf::from(&f.path);
            rows = rows.child(
                frow()
                    .id(SharedString::from(format!("gchange-{n}")))
                    .px(px(6.))
                    .mx(px(-6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(path_text(faces, &f.path))
                    .child(tail)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_aside(Aside::File(path.clone()), cx);
                    })),
            );
        }
        sec = sec.child(rows);
        let idle = !matches!(s.state, State::Working | State::Opening);
        let first = files.first().map(|f| std::path::PathBuf::from(&f.path));
        sec = sec.child(
            div()
                .flex()
                .gap(px(6.))
                .children(first.map(|first| {
                    ui::button(
                        faces,
                        "review",
                        "Review diff",
                        if idle {
                            Weight::Primary
                        } else {
                            Weight::Secondary
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_aside(Aside::File(first.clone()), cx);
                    }))
                }))
                .child(
                    ui::button(faces, "commit", "Commit…", Weight::Secondary).on_click(
                        cx.listener(|this, _, window, cx| this.toggle_commit(window, cx)),
                    ),
                ),
        );
        if self.committing {
            sec = sec.child(self.commit_form(faces, s, cx));
        }
        if let Some(step) = &s.git_step {
            sec = sec.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::live_dot())
                    .child(fine(faces, format!("{step}…")).text_color(theme::accent())),
            );
        }
        Some(sec)
    }

    /// The message to commit with and what can be done with it: commit,
    /// commit and push, a pull request; the agent can write the message.
    pub(super) fn commit_form(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        // The agent can write the message: a turn of its own.
        let key = s.key;
        let name = self.name_of(&s.agent);
        let write = ui::led_button(
            faces,
            "ask-message",
            Some(ui::mark(faces, &ui::Mark::of(&s.agent, &name), 12.)),
            "Write it",
            Weight::Quiet,
        )
        .h(px(24.))
        .px(px(7.))
        .tooltip(ui::tip(format!(
            "{name} writes the message: a turn of its own, spent as any turn is"
        )))
        .on_click(cx.listener(move |this, _, _, cx| this.ask_message(key, cx)));
        let what = format!(
            "Everything in {} on {}, as you, with this message or the session's title",
            Desk::shown(&s.folder),
            s.branch.as_deref().unwrap_or("its branch")
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(34.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(8.))
                    .bg(theme::well())
                    .border_1()
                    .border_color(theme::border2())
                    .child(div().flex_1().min_w(px(0.)).child(self.message.clone()))
                    // Asked during a turn, its answer would be taken for
                    // that turn's: it waits for the turn to end.
                    .when(
                        !matches!(
                            s.state,
                            crate::session::State::Working | crate::session::State::Opening
                        ),
                        |d| d.child(write),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        git_button(
                            faces,
                            "do-commit",
                            "Commit",
                            Weight::Primary,
                            cx,
                            hub::GitAction::Commit,
                        )
                        .tooltip(ui::tip(what)),
                    )
                    .child(
                        git_button(
                            faces,
                            "do-push",
                            "Commit & push",
                            Weight::Secondary,
                            cx,
                            hub::GitAction::CommitPush,
                        )
                        .tooltip(ui::tip(
                            "On the main branch, a branch of its own is made first",
                        )),
                    )
                    .child(git_button(
                        faces,
                        "do-pr",
                        "Create PR",
                        Weight::Secondary,
                        cx,
                        hub::GitAction::PullRequest,
                    )),
            )
    }

    /// The recent commits where the session works; a click shows what one
    /// changed.
    pub(super) fn commits(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if s.commits.is_empty() {
            return None;
        }
        let mut rows = div().flex().flex_col();
        for (id, subject, when) in s.commits.iter().take(8) {
            let on = self.scope == Scope::Commit(id.clone());
            let to = Scope::Commit(id.clone());
            rows = rows.child(
                frow()
                    .id(SharedString::from(format!("commit-{id}")))
                    .px(px(6.))
                    .mx(px(-6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::hover()))
                    .hover(|st| st.bg(theme::hover()))
                    .child(
                        ui::mono(faces, id.clone())
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(theme::faint()),
                    )
                    .child(ui::one_line(
                        ui::text(faces, ui::fit(subject, 26))
                            .text_size(px(12.5))
                            .text_color(theme::text2()),
                    ))
                    .child(fine(faces, when.clone()).flex_none())
                    .on_click(cx.listener(move |this, _, _, cx| this.show_changes(to.clone(), cx))),
            );
        }
        Some(section(faces, "Commits", None).child(rows))
    }

    /// The branch's pull request: its state, its checks, the review asked.
    pub(super) fn pull_request(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let pr = s.pr.as_ref()?;
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
                            ui::button(faces, "open-pr", "Open", Weight::Secondary)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                        .child(
                            ui::button(faces, "refresh-pr", "Check again", Weight::Quiet)
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
                            d.child(ui::button(faces, "merge-pr", "Merge", Weight::Secondary).on_click(
                                cx.listener(|this, _, _, cx| this.git_action(hub::GitAction::Merge, cx)),
                            ))
                        })
                        .when(open && pr.failed > 0, |d| {
                            d.child(ui::button(faces, "fix-checks", "Have it fix the checks", Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request's checks failed. Look at them (gh pr checks, and the failed runs' logs), fix what broke, commit and push.", cx);
                                }),
                            ))
                        })
                        .when(open && pr.conflicts, |d| {
                            d.child(ui::button(faces, "fix-conflicts", "Have it resolve the conflicts", Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.say("The pull request conflicts with its base branch. Bring the base in, resolve the conflicts, run the tests, commit and push.", cx);
                                }),
                            ))
                        }),
                ),
        )
    }
}
