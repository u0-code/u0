//! The column down the right, as the design draws it. For a session: tabs
//! for the session and its files, then what it has come to (the files it
//! changed, its plan, the commands it ran, how full its context is); a
//! changed file opens here whole. For an agent's page: what it is and how
//! Ghost runs it.

mod repo;

use std::time::SystemTime;

use agent_client_protocol_schema::v1::{ToolCallContent, ToolCallStatus};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Aside, Desk, RIGHT, TOPBAR, draggable},
    fonts::Faces,
    hub,
    hub::Stage,
    registry::{self, Way},
    session::{Change, Session, State},
    theme,
    thread::{self, drawn, kinds},
    ui::{self, Mark, Weight},
    when,
};

/// The column's frame: its width, its line on the left, a top row.
fn frame(top: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(RIGHT))
        .h_full()
        .border_l_1()
        .border_color(theme::border())
        .child(top)
}

fn top() -> Div {
    draggable()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(px(TOPBAR))
        .px(px(10.))
        .border_b_1()
        .border_color(theme::border())
}

/// A section: its head and what is under it, ruled off below.
fn section(faces: &Faces, head: &str, note: Option<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(16.))
        .py(px(14.))
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::label(faces, head).flex_1())
                .children(note),
        )
}

fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}

fn fine_mono(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::mono(faces, text)
        .flex_none()
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(theme::faint())
}

/// A row in a section: 26 high.
fn frow() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(26.))
        .text_size(px(13.))
}

fn path_text(faces: &Faces, text: &str) -> Div {
    ui::one_line(
        ui::mono(faces, ui::fit(text, 34))
            .text_size(px(12.5))
            .text_color(theme::text2()),
    )
    .flex_1()
}

fn empty(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(12.5))
        .line_height(px(18.))
        .text_color(theme::faint())
}

fn key_value(faces: &Faces, key: &str, value: impl IntoElement) -> Div {
    frow()
        .child(
            ui::text(faces, key.to_owned())
                .flex_1()
                .text_size(px(12.5))
                .text_color(theme::muted()),
        )
        .child(value)
}

fn added_gone(faces: &Faces, added: usize, gone: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .gap(px(4.))
        .when(added > 0 || gone == 0, |d| {
            d.child(fine_mono(faces, format!("+{added}")).text_color(theme::ok()))
        })
        .when(gone > 0, |d| {
            d.child(fine_mono(faces, format!("−{gone}")).text_color(theme::coral()))
        })
}

impl Desk {
    pub(crate) fn aside(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.session() else {
            return frame(top());
        };
        if let Aside::File(path) = &self.aside {
            return self.file_view(faces, s, path, cx);
        }
        let tab = |id: &'static str, label: &'static str, on: bool, to: Aside| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(26.))
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme::hover()))
                .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(if on { theme::text() } else { theme::text2() }),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.show_aside(to.clone(), cx)))
        };
        let head = top()
            .child(tab(
                "tab-session",
                "Session",
                self.aside == Aside::Session,
                Aside::Session,
            ))
            .child(tab(
                "tab-files",
                "Files",
                self.aside == Aside::Files,
                Aside::Files,
            ))
            .child(div().flex_1())
            .children(
                s.branch
                    .as_deref()
                    .map(|b| self.branch_chip(faces, s, b, cx)),
            );
        let body = div()
            .id("aside")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll();
        let body = if self.aside == Aside::Files {
            body.child(Self::files(faces, s, cx))
        } else {
            body.child(
                self.git_changes(faces, s, cx)
                    .unwrap_or_else(|| Self::changes(faces, s, cx)),
            )
            .children(Self::pull_request(faces, s, cx))
            .children(self.commits(faces, s, cx))
            // A folder that is not a repository can be made one.
            .when(!s.repo && crate::remote::parse(&s.project).is_none() && s.state != crate::session::State::Draft, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(16.))
                        .py(px(12.))
                        .child(fine(faces, "Not a git repository: turns are not checkpointed, nothing is committed.").flex_1())
                        .child(
                            ui::button(faces, "git-init", "Make it one", ui::Weight::Quiet)
                                .h(px(24.))
                                .on_click(cx.listener(|this, _, _, cx| this.git_action(hub::GitAction::Init, cx))),
                        ),
                )
            })
            .child(Self::plan(faces, s))
            .child(Self::terminals(faces, s, cx))
            .child(self.context(faces, s, cx))
        };
        frame(head).child(body)
    }

    /// A file's diff with the files changed beside it, to go from one to
    /// the next, and under it the commit.
    fn review(
        &self,
        faces: &Faces,
        s: &Session,
        path: &std::path::Path,
        diff: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let files = self.scoped(s).cloned().unwrap_or_default();
        let mut rail = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(180.))
            .gap(px(2.))
            .px(px(6.))
            .py(px(8.))
            .border_r_1()
            .border_color(theme::border());
        for (n, f) in files.iter().enumerate() {
            let at = std::path::PathBuf::from(&f.path);
            let on = same_file(&f.path, path, &s.folder);
            let name = at
                .file_name()
                .map_or_else(|| f.path.clone(), |n| n.to_string_lossy().into_owned());
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("review-{n}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::hover()))
                    .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
                    .tooltip(ui::tip(f.path.clone()))
                    // Ticked, it goes in the next commit; not, it stays out.
                    .when(s.repo, |d| {
                        let (key, named) = (s.key, f.path.clone());
                        let goes = !s.left_out.contains(&f.path);
                        d.child(
                            div()
                                .id(SharedString::from(format!("goes-{n}")))
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size(px(14.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(if goes {
                                    theme::text2()
                                } else {
                                    theme::ghost_line()
                                })
                                .when(goes, |d| {
                                    d.child(ui::icon("check", 10.).text_color(theme::text()))
                                })
                                .tooltip(ui::tip(if goes {
                                    "In the next commit: click to leave it out"
                                } else {
                                    "Left out of the next commit"
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if let Some(s) = this.session_mut(key)
                                        && !s.left_out.remove(&named)
                                    {
                                        s.left_out.insert(named.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        ui::one_line(
                            ui::mono(faces, ui::fit(&name, 16))
                                .text_size(px(12.5))
                                .text_color(if on { theme::text() } else { theme::text2() }),
                        )
                        .flex_1(),
                    )
                    .child(match f.status {
                        crate::git::Status::Added => fine(faces, "new"),
                        crate::git::Status::Deleted => {
                            fine(faces, "deleted").text_color(theme::coral())
                        }
                        _ => added_gone(faces, f.added, 0),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_aside(Aside::File(at.clone()), cx);
                    })),
            );
        }
        let said = self.comments.len();
        rail = rail.child(div().px(px(10.)).pt(px(10.)).child(fine(
            faces,
            format!(
                "{} file{}{}{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                match s.left_out.len() {
                    0 => String::new(),
                    n => format!(" · {n} left out"),
                },
                match said {
                    0 => String::new(),
                    1 => " · 1 comment".into(),
                    n => format!(" · {n} comments"),
                }
            ),
        )));
        let commit = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(10.))
            .p(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::label(faces, "Commit").flex_1())
                    .child(fine(
                        faces,
                        format!(
                            "on {} · {} file{}",
                            s.branch.as_deref().unwrap_or("its branch"),
                            files.len(),
                            if files.len() == 1 { "" } else { "s" }
                        ),
                    )),
            )
            .child(self.commit_form(faces, s, cx));
        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .when(!files.is_empty(), |d| d.child(rail))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(diff)
                    .when(s.repo, |d| d.child(commit)),
            )
    }

    /// The branch where the session works; a click lists the others, to
    /// check one out.
    fn branch_chip(&self, faces: &Faces, s: &Session, branch: &str, cx: &mut Context<Self>) -> Div {
        let open = self.popup == crate::desk::Popup::Branch;
        let mut chip = div().relative().child(
            div()
                .id("branch-chip")
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .h(px(22.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|st| st.bg(theme::hover()))
                .child(fine_mono(faces, ui::fit(branch, 22)))
                .when(s.branches.len() > 1, |d| {
                    d.child(ui::icon("chevron-down", 10.).text_color(theme::faint()))
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_popup(crate::desk::Popup::Branch, cx);
                })),
        );
        if open && s.branches.len() > 1 {
            let mut menu = div().flex().flex_col().w(px(240.)).p(px(6.)).child(
                ui::text(faces, "Check out")
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(2.))
                    .text_size(px(11.5))
                    .text_color(theme::faint()),
            );
            for (n, b) in s.branches.iter().enumerate() {
                let (on, name) = (b == branch, b.clone());
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("branch-{n}")))
                        .flex()
                        .items_center()
                        .px(px(8.))
                        .h(px(28.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(theme::panel2()))
                        .hover(|st| st.bg(theme::hover()))
                        .child(ui::mono(faces, ui::fit(b, 30)).text_size(px(12.)))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.switch_branch(name.clone(), cx)),
                        ),
                );
            }
            chip = chip.child(gpui::deferred(
                gpui::anchored()
                    .anchor(gpui::Corner::TopRight)
                    .child(div().mt(px(24.)).child(ui::menu("branch-menu").child(menu))),
            ));
        }
        chip
    }

    fn changes(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = s.changed();
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let mut sec = section(
            faces,
            "Changes",
            (!files.is_empty()).then(|| added_gone(faces, added, gone)),
        );
        if files.is_empty() {
            return sec.child(empty(
                faces,
                format!(
                    "Files the agent changes in {} show here.",
                    s.folder.file_name().map_or_else(
                        || Desk::shown(&s.folder),
                        |n| n.to_string_lossy().into_owned()
                    )
                ),
            ));
        }
        let mut list = div().flex().flex_col();
        for (n, f) in files.iter().enumerate() {
            let name = f
                .path
                .strip_prefix(&s.folder)
                .unwrap_or(&f.path)
                .display()
                .to_string();
            let path = f.path.clone();
            let tail = match f.change {
                Change::New => fine(faces, "new"),
                Change::Deleted => fine(faces, "deleted").text_color(theme::coral()),
                Change::Edited => added_gone(faces, f.added, f.gone),
            };
            list = list.child(
                frow()
                    .id(SharedString::from(format!("change-{n}")))
                    .px(px(6.))
                    .mx(px(-6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::hover()))
                    .child(path_text(faces, &name))
                    .child(tail)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_aside(Aside::File(path.clone()), cx);
                    })),
            );
        }
        let first = files[0].path.clone();
        let folder = s.folder.clone();
        sec = sec.child(list).child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    ui::button(
                        faces,
                        "review",
                        "Review diff",
                        if s.state == State::Working {
                            Weight::Secondary
                        } else {
                            Weight::Primary
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_aside(Aside::File(first.clone()), cx);
                    })),
                )
                .child(
                    ui::button(faces, "open-folder", "Open in Finder", Weight::Quiet).on_click(
                        move |_, _, _| {
                            let _ = std::process::Command::new("open").arg(&folder).spawn();
                        },
                    ),
                ),
        );
        sec
    }

    fn plan(faces: &Faces, s: &Session) -> Div {
        let Some(plan) = s.plan.as_ref().filter(|p| !p.entries.is_empty()) else {
            return section(faces, "Plan", None)
                .child(empty(faces, "Shown when the agent makes one."));
        };
        let steps = thread::plan_steps(plan);
        let done = steps
            .iter()
            .filter(|(st, _)| *st == kinds::Status::Done)
            .count();
        let note = fine(faces, format!("{done} of {}", steps.len()))
            .when(done == steps.len(), |d| d.text_color(theme::ok()));
        section(faces, "Plan", Some(note)).child(drawn::plan(faces, &steps))
    }

    fn terminals(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let commands: Vec<_> = s.commands().collect();
        if commands.is_empty() {
            return section(faces, "Terminals", None)
                .child(empty(faces, "Commands the agent runs, live."));
        }
        let running = commands
            .iter()
            .filter(|t| {
                matches!(
                    t.call.status,
                    ToolCallStatus::Pending | ToolCallStatus::InProgress
                )
            })
            .count();
        let note = if running > 0 {
            format!("{running} running")
        } else {
            format!("{} finished", commands.len())
        };
        let mut sec = section(faces, "Terminals", Some(fine(faces, note)));
        for t in commands.iter().rev().take(6) {
            let live = matches!(
                t.call.status,
                ToolCallStatus::Pending | ToolCallStatus::InProgress
            );
            let failed = t.call.status == ToolCallStatus::Failed;
            let dot = if live {
                ui::live_dot().into_any_element()
            } else {
                ui::dot(if failed { theme::coral() } else { theme::ok() }).into_any_element()
            };
            let time = if live {
                fine(faces, when::took(t.started.elapsed())).text_color(theme::accent())
            } else if failed {
                fine(faces, "failed").text_color(theme::coral())
            } else {
                fine(faces, t.took.map(when::took_short).unwrap_or_default())
            };
            let text = t.command_line();
            let group = SharedString::from(format!("ran-{}", t.call.tool_call_id.0));
            let again = text.clone();
            sec = sec.child(
                frow()
                    .id(group.clone())
                    .group(group.clone())
                    .child(dot)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(thread::command(faces, &text, 12.5)),
                    )
                    // Run again in a terminal of yours, to see or change it:
                    // over its time, while the pointer is on the row.
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .child(time.when(!live, |d| {
                                d.group_hover(group.clone(), gpui::Styled::invisible)
                            }))
                            .when(!live, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .right(px(-4.))
                                        .invisible()
                                        .group_hover(group, gpui::Styled::visible)
                                        .child(
                                            ui::icon_button(
                                                SharedString::from(format!(
                                                    "rerun-{}",
                                                    t.call.tool_call_id.0
                                                )),
                                                "play",
                                                "Run it in a terminal of yours",
                                            )
                                            .size(px(22.))
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.open_terminal(Some(&again), window, cx);
                                                }),
                                            ),
                                        ),
                                )
                            }),
                    ),
            );
            if live
                && let Some(out) = t.call.content.iter().find_map(|c| match c {
                    ToolCallContent::Content(x) => Some(crate::session::text_of(&x.content)),
                    _ => None,
                })
            {
                let tail: Vec<&str> = out.lines().rev().take(3).collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                sec = sec.child(
                    thread::well(faces)
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .child(tail.join("\n")),
                );
            }
        }
        sec
    }

    fn context(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let note = s
            .usage
            .as_ref()
            .filter(|u| u.size > 0)
            .map(|u| fine(faces, format!("{}k of {}k", u.used / 1000, u.size / 1000)));
        let share = s
            .usage
            .as_ref()
            .filter(|u| u.size > 0)
            .map(|u| u.used as f32 / u.size as f32);
        let agent = match self.slot(&s.agent).map(|x| &x.stage) {
            Some(Stage::Running { init, .. }) => init.agent_info.as_ref().map_or_else(
                || self.name_of(&s.agent),
                |i| {
                    format!(
                        "{} {}",
                        i.name.rsplit('/').next().unwrap_or(&i.name),
                        i.version
                    )
                },
            ),
            _ => self.name_of(&s.agent),
        };
        let mut sec = section(faces, "Context", note)
            .border_b_0()
            .children(share.map(|share| {
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(theme::tag_line())
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(share.clamp(0., 1.)))
                            .bg(theme::muted()),
                    )
            }))
            .child(key_value(
                faces,
                "Agent",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::mark(faces, &Mark::of(&s.agent, &agent), 12.))
                    .child(fine(faces, format!("{agent} · ACP v1"))),
            ))
            .child(key_value(faces, "Working directory", {
                // A long one keeps its last folder, which says which it is.
                let dir = Desk::shown(&s.folder);
                let name = s.folder.file_name().map(|n| n.to_string_lossy());
                fine_mono(
                    faces,
                    match name {
                        Some(name) if dir.chars().count() > 20 => {
                            format!("…/{}", ui::fit(&name, 18))
                        }
                        _ => dir,
                    },
                )
            }));
        if let Some(wt) = &s.worktree {
            let removable = !matches!(s.state, State::Working | State::Opening);
            let wt_for_click = wt.clone();
            sec = sec.child(key_value(
                faces,
                "Worktree",
                // Its branch is the one named at the column's top.
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child({
                        let key = s.key;
                        div()
                            .id("new-in-worktree")
                            .cursor_pointer()
                            .child(fine(faces, "New session").text_color(theme::link()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.new_in_worktree(key, window, cx);
                            }))
                    })
                    .when(removable, |d| {
                        d.child(
                            div()
                                .id("remove-worktree")
                                .cursor_pointer()
                                .child(fine(faces, "Remove").text_color(theme::link()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.git_action(
                                        hub::GitAction::RemoveWorktree(wt_for_click.clone()),
                                        cx,
                                    );
                                })),
                        )
                    }),
            ));
        }
        if let Some(mode) = Desk::mode_name(s) {
            sec = sec.child(key_value(faces, "Mode", fine(faces, mode)));
        }
        if !s.items.is_empty() {
            let since = SystemTime::now()
                .duration_since(s.begun)
                .unwrap_or_default();
            let turns = s.turn_count();
            let words = format!(
                "{} · {} · {turns} {}",
                when::clock(s.begun),
                when::took(since),
                if turns == 1 { "turn" } else { "turns" }
            );
            sec = sec.child(key_value(faces, "Started", fine(faces, words)));
        }
        sec
    }

    /// Every file the session touched.
    fn files(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = s.files();
        let changed = s.changed();
        let mut sec = section(
            faces,
            "Files",
            Some(fine(faces, format!("{} touched", files.len()))),
        )
        .border_b_0();
        if files.is_empty() {
            return sec.child(empty(faces, "Files the agent reads or changes show here."));
        }
        for (n, path) in files.into_iter().enumerate() {
            let name = path
                .strip_prefix(&s.folder)
                .unwrap_or(&path)
                .display()
                .to_string();
            let change = changed.iter().find(|c| c.path == path);
            let tail = change.map_or_else(
                || fine(faces, "read"),
                |c| match c.change {
                    Change::New => fine(faces, "new"),
                    Change::Deleted => fine(faces, "deleted").text_color(theme::coral()),
                    Change::Edited => added_gone(faces, c.added, c.gone),
                },
            );
            let openable = change.is_some();
            sec = sec.child(
                frow()
                    .id(SharedString::from(format!("file-{n}")))
                    .px(px(6.))
                    .mx(px(-6.))
                    .rounded(px(6.))
                    .when(openable, |d| {
                        d.cursor_pointer().hover(|st| st.bg(theme::hover()))
                    })
                    .child(ui::icon("file", 14.).text_color(theme::faint()))
                    .child(path_text(faces, &name))
                    .child(tail)
                    .when(openable, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.show_aside(Aside::File(path.clone()), cx);
                        }))
                    }),
            );
        }
        sec
    }

    /// One file's changes, every edit to it, whole.
    fn file_view(
        &self,
        faces: &Faces,
        s: &Session,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = path
            .strip_prefix(&s.folder)
            .unwrap_or(path)
            .display()
            .to_string();
        let from_git = self
            .scoped(s)
            .and_then(|files| files.iter().find(|f| same_file(&f.path, path, &s.folder)));
        let total = s
            .changed()
            .into_iter()
            .find(|c| c.path == path)
            .map(|c| (c.added, c.gone))
            .or_else(|| from_git.map(|f| (f.added, f.gone)));
        let full = s.folder.join(path);
        let head = top()
            .gap(px(8.))
            .child(
                ui::icon_button("back", "chevron-left", "Back to the session")
                    .on_click(cx.listener(|this, _, _, cx| this.show_aside(Aside::Session, cx))),
            )
            .child(
                ui::mono(faces, ui::fit(&name, 34))
                    .flex_none()
                    .text_size(px(12.5))
                    .text_color(theme::text2()),
            )
            .child({
                let copied = name.clone();
                ui::icon_button("copy-path", "copy", "Copy its path")
                    .size(px(22.))
                    .text_color(theme::faint())
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                    })
            })
            .children(total.map(|(a, g)| added_gone(faces, a, g)))
            .child(div().flex_1())
            .when(s.repo, |d| d.child(self.scope_tabs(faces, s, cx)))
            .when(from_git.is_some(), |d| {
                d.child(
                    ui::icon_button(
                        "side-by-side",
                        "panel-right",
                        if self.settings.side_by_side {
                            "One above the other"
                        } else {
                            "Side by side"
                        },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.side_by_side = !this.settings.side_by_side;
                        this.save_settings();
                        cx.notify();
                    })),
                )
            })
            .child(
                ui::icon_button("open-editor", "pencil", "Open in your editor")
                    .on_click(move |_, _, _| crate::desk::open_in_editor(&full)),
            );
        let mut body = div()
            .id("aside-file")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .py(px(6.));
        body = body.children(self.comment_box(faces, s, path, cx));
        if let Some(file) = from_git {
            let hunks = if self.settings.side_by_side {
                drawn::hunks_split(faces, file)
            } else {
                // A line clicked is one to say something of, to the agent.
                let this = cx.entity().downgrade();
                let at = path.to_path_buf();
                let on: drawn::OnLine = std::rc::Rc::new(move |n, window, cx| {
                    let _ =
                        this.update(cx, |desk, cx| desk.begin_comment(at.clone(), n, window, cx));
                });
                drawn::hunks(faces, file, drawn::Fit::Column, Some(&on))
            };
            return frame(head).child(self.review(faces, s, path, body.child(hunks), cx));
        }
        for t in s.tools() {
            let mut n = 0;
            for c in &t.call.content {
                if let ToolCallContent::Diff(d) = c {
                    if d.path == path {
                        let lines = kinds::diff(d.old_text.as_deref().unwrap_or(""), &d.new_text);
                        let start = t.starts.get(n).copied().flatten();
                        body = body.child(
                            drawn::diff(faces, &lines, start, drawn::Fit::Column).mb(px(8.)),
                        );
                    }
                    n += 1;
                }
            }
        }
        frame(head).child(self.review(faces, s, path, body, cx))
    }

    /// What you are saying of lines of this file: the line being commented
    /// on and the line to write it in; those said; and sending them.
    fn comment_box(
        &self,
        faces: &Faces,
        s: &Session,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let here: Vec<&(std::path::PathBuf, usize, String)> =
            self.comments.iter().filter(|c| c.0 == path).collect();
        let writing = self
            .commenting
            .as_ref()
            .filter(|(p, _)| p == path)
            .map(|(_, n)| *n);
        if here.is_empty() && writing.is_none() && self.comments.is_empty() {
            return None;
        }
        let mut sec = div().flex().flex_col().gap(px(6.)).px(px(14.)).pb(px(10.));
        for (n, (_, line, text)) in here.iter().enumerate() {
            let (p, l) = (path.to_path_buf(), *line);
            sec = sec.child(
                frow()
                    .child(fine_mono(faces, format!("L{line}")))
                    .child(ui::one_line(
                        ui::text(faces, text.clone()).text_size(px(12.5)),
                    ))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("uncomment-{n}")),
                            "x",
                            "Take it back",
                        )
                        .size(px(20.))
                        .on_click(cx.listener(move |this, _, _, cx| this.drop_comment(&p, l, cx))),
                    ),
            );
        }
        if let Some(line) = writing {
            sec = sec.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(fine_mono(faces, format!("L{line}")))
                    .child(
                        div()
                            .flex_1()
                            .h(px(28.))
                            .px(px(6.))
                            .rounded(px(7.))
                            .bg(theme::well())
                            .border_1()
                            .border_color(theme::border2())
                            .child(self.comment.clone()),
                    ),
            );
        }
        if !self.comments.is_empty() {
            let name = self.name_of(&s.agent);
            let n = self.comments.len();
            sec = sec.child(
                ui::button(
                    faces,
                    "send-comments",
                    format!(
                        "Put {n} comment{} to {name} in the line",
                        if n == 1 { "" } else { "s" }
                    ),
                    Weight::Secondary,
                )
                .on_click(cx.listener(|this, _, window, cx| this.send_comments(window, cx))),
            );
        }
        Some(sec)
    }

    /// An agent's page's right column: what it is and how Ghost runs it.
    pub(crate) fn about(&self, faces: &Faces, id: &str, cx: &mut Context<Self>) -> Div {
        let head = top().px(px(16.)).child(ui::label(faces, "About"));
        let Some(e) = self.entry(id) else {
            return frame(head);
        };
        let name = self.name_of(id);
        let (distribution, command) = match e.way() {
            Way::Archive => {
                let b = &e.distribution.binary[&registry::platform()];
                (
                    format!("binary · {}", registry::platform()),
                    std::iter::once(b.cmd.trim_start_matches("./").to_owned())
                        .chain(b.args.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
            Way::Runner { runner, package } => (runner.to_owned(), format!("{runner} {package}")),
            Way::None => ("none for this machine".into(), String::new()),
        };
        let link = e.website.clone().or_else(|| e.repository.clone());
        let host = link.as_deref().map(|l| {
            l.trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or(l)
                .to_owned()
        });
        let mut about_rows = section(faces, "Agent", None)
            .child(key_value(faces, "Source", fine(faces, "ACP registry")))
            .child(key_value(
                faces,
                "Version",
                fine_mono(faces, e.version.clone()),
            ))
            .child(key_value(
                faces,
                "Distribution",
                fine_mono(faces, distribution),
            ))
            .child(key_value(
                faces,
                "Command",
                fine_mono(faces, ui::fit(&command, 30)),
            ))
            .child(key_value(
                faces,
                "Sign-in",
                fine(faces, format!("{name}'s own")),
            ));
        if let (Some(link), Some(host)) = (link, host) {
            about_rows = about_rows.child(key_value(
                faces,
                "Homepage",
                fine(faces, host)
                    .text_color(theme::link())
                    .id("homepage")
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&link))),
            ));
        }
        let what = section(faces, "What Ghost does with it", None)
            .border_b_0()
            .child(
                ui::text(
                    faces,
                    format!(
                        "Starts {name} in the session's folder, sends what you say, and draws what it reports. Edits, commands and permissions are {name}'s own; Ghost only asks you when it asks."
                    ),
                )
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(theme::text2()),
            );
        frame(head).child(about_rows).child(what)
    }
}

/// A button that does a git action with the commit message typed.
fn git_button(
    faces: &Faces,
    id: &'static str,
    label: &'static str,
    weight: Weight,
    cx: &mut Context<Desk>,
    make: fn(String) -> hub::GitAction,
) -> gpui::Stateful<Div> {
    ui::button(faces, id, label, weight).on_click(cx.listener(move |this, _, _, cx| {
        let message = this.commit_message(cx);
        this.committing = false;
        this.git_action(make(message), cx);
    }))
}

/// Whether git's `named` (from the repository's top) is the file at
/// `path`, which the agent often gives whole, in a session working in
/// `folder`.
fn same_file(named: &str, path: &std::path::Path, folder: &std::path::Path) -> bool {
    std::path::Path::new(named) == path || (path.starts_with(folder) && path.ends_with(named))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::same_file;

    #[test]
    fn a_file_git_names_from_the_top_is_the_one_the_agent_names_whole() {
        let folder = Path::new("/repo");
        assert!(same_file("src/a.rs", Path::new("src/a.rs"), folder));
        assert!(same_file("src/a.rs", Path::new("/repo/src/a.rs"), folder));
        assert!(!same_file(
            "src/a.rs",
            Path::new("/elsewhere/src/a.rs"),
            folder
        ));
        assert!(!same_file("src/a.rs", Path::new("/repo/src/b.rs"), folder));
    }
}
