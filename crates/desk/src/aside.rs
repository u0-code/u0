//! The column down the right, as the design draws it. For a session: tabs
//! for the session and its files, then what it has come to (the files it
//! changed, its plan, the commands it ran, how full its context is); a
//! changed file opens here whole. For an agent's page: what it is and how
//! Ghost runs it.

mod diff;
mod files;
mod repo;
mod side;
mod tabs;

pub use files::Viewer;
pub use tabs::{Column, Surface};

use std::time::SystemTime;

use agent_client_protocol::v1::schema::{ToolCallContent, ToolCallStatus};
use gpui::component::{
    Sizable as _, button::Button, input::Input, popover::Popover, progress::Progress,
    scroll::ScrollableElement as _,
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, RIGHT, TOPBAR, draggable},
    fonts::Faces,
    registry::{self, Way},
    session::{Session, State},
    settings::Detail,
    theme,
    thread::{self, drawn, kinds},
    ui::{self, Weight},
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

/// Small print that keeps its width in a row.
fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::fine(faces, text).flex_none()
}

fn fine_mono(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::fine_mono(faces, text).flex_none()
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

impl Desk {
    pub(crate) fn aside(&self, faces: &Faces, cell: f32, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.session() else {
            return frame(top());
        };
        let head = top()
            .child(self.strip(faces, s, cx))
            .when(self.surface() == Some(Surface::Details), |d| {
                d.child(self.details_chooser(faces, cx))
            })
            .children(
                s.branch
                    .as_deref()
                    .map(|b| self.branch_chip(faces, s, b, cx)),
            )
            .child(self.whole_button(cx));
        let scrolled = |body: Div| {
            div()
                .id("aside")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .child(body)
                .overflow_y_scrollbar()
        };
        let body = match self.surface() {
            None => self.no_tabs(faces, cx).into_any_element(),
            Some(Surface::Details) => scrolled(self.details(faces, s, cx)).into_any_element(),
            Some(Surface::Diff) => self.diff_tab(faces, s, cx).into_any_element(),
            Some(Surface::Files) => self.files_tab(faces, s, cx).into_any_element(),
            Some(Surface::File(path)) => self.file_tab(faces, s, &path, cx).into_any_element(),
            Some(Surface::Terminal) => self.terminal_view(faces, cell, cx).into_any_element(),
            Some(Surface::PullRequest) => {
                scrolled(Self::pull_request(faces, s, cx).unwrap_or_else(|| {
                    section(faces, "Pull request", None).border_b_0().child(empty(
                    faces,
                    "None for this branch yet: commit, push and open one from the Diff tab.",
                ))
                }))
                .into_any_element()
            }
            Some(Surface::Side(key)) => self.side_chat(faces, key, cx).into_any_element(),
        };
        frame(head).child(body)
    }

    /// The Details tab, as Claude stacks its cards: the context, the plan,
    /// and what goes on in the background; those you hid left out.
    fn details(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let shown = |d: Detail| !self.settings.hidden_details.contains(&d);
        div()
            .flex()
            .flex_col()
            .when(shown(Detail::Context), |d| {
                d.child(self.context(faces, s, cx))
            })
            .when(shown(Detail::Plan), |d| d.child(Self::plan(faces, s)))
            .when(shown(Detail::Background), |d| {
                d.children(self.background(faces, s, cx))
            })
            .when(shown(Detail::Servers), |d| {
                d.children(self.servers(faces, s, cx))
            })
            .when(shown(Detail::Terminals), |d| {
                d.child(Self::commands(faces, s, cx))
            })
    }

    /// What opens the choice of the details shown.
    fn details_chooser(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> gpui::component::popover::Popover {
        let faces = faces.clone();
        let chooser =
            ui::icon_button("details-rows", "sliders", "Choose what shows here").size(px(24.));
        self.menu(
            crate::desk::Popup::Details,
            chooser,
            move |d, _, cx| d.details_menu(&faces, cx),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    /// Which of the details show, each ticked or not, and all again.
    fn details_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut menu = div().flex().flex_col().w(px(200.));
        for (n, d) in Detail::ALL.into_iter().enumerate() {
            let on = !self.settings.hidden_details.contains(&d);
            menu = menu.child(
                ui::menu_row(SharedString::from(format!("detail-{n}")), false)
                    .child(ui::text(faces, d.head()).flex_1().text_size(px(13.)))
                    .when(on, |r| r.child(ui::icon("check", 13.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let hidden = &mut this.settings.hidden_details;
                        if on {
                            hidden.push(d);
                        } else {
                            hidden.retain(|h| *h != d);
                        }
                        this.save_settings();
                        cx.notify();
                    })),
            );
        }
        menu.child(
            ui::menu_row("details-reset", false)
                .child(ui::text(faces, "Show them all").text_size(px(13.)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.hidden_details.clear();
                    this.save_settings();
                    cx.notify();
                })),
        )
    }

    /// What goes on beside the session (or its parent, on a subagent's own
    /// page), as one list: its subagents and the commands its agent left
    /// going in the background. Those going or waiting on you first, with
    /// how long and a stop where they can be stopped; those over folded in
    /// one line, "N finished", a click away.
    fn background(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let family = s.parent.unwrap_or(s.key);
        let kids: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|c| c.parent == Some(family))
            .collect();
        let tasks: Vec<(u64, &crate::session::Task)> = self
            .sessions
            .all
            .iter()
            .filter(|c| c.key == family || c.parent == Some(family))
            .flat_map(|c| c.tasks.iter().map(move |t| (c.key, t)))
            .collect();
        if kids.is_empty() && tasks.is_empty() {
            return None;
        }
        let live = |c: &Session| c.sub.working() || c.sub.needs_you;
        let going = kids.iter().filter(|c| live(c)).count()
            + tasks.iter().filter(|(_, t)| t.ended.is_none()).count();
        let over = kids.iter().filter(|c| !live(c)).count()
            + tasks.iter().filter(|(_, t)| t.ended.is_some()).count();
        let fold = format!("background-{family}");
        let open = self.expanded.contains(&fold);
        let mut sec = section(
            faces,
            Detail::Background.head(),
            (going > 0)
                .then(|| fine(faces, format!("{going} running")).text_color(theme::accent())),
        );
        let row_of = |id: SharedString| {
            frow()
                .id(id)
                .px(px(6.))
                .mx(px(-6.))
                .rounded(px(6.))
                .hover(|st| st.bg(theme::hover()))
        };
        let title = |text: &str| {
            ui::one_line(ui::text(faces, ui::fit(text, 40)).text_size(px(13.)))
                .flex_1()
                .min_w(px(0.))
        };
        let stop = |id: SharedString| ui::icon_button(id, "square", "Stop it").size(px(20.));
        let sub_row = |c: &Session, cx: &mut Context<Self>| {
            let key = c.key;
            row_of(SharedString::from(format!("bg-sub-{key}")))
                .cursor_pointer()
                .when(key == s.key, |d| d.bg(theme::hover()))
                .child(crate::thread::sub_mark(&c.sub))
                .child(title(&c.meta.title))
                .children(c.sub.time().map(|t| fine(faces, t)))
                .when(c.sub.working() && c.sub.stoppable, |d| {
                    d.child(
                        stop(SharedString::from(format!("bg-sub-stop-{key}"))).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.stop_sub(key, cx);
                            }),
                        ),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.closed_tabs.remove(&key);
                    this.show(key, window, cx);
                }))
        };
        let task_row = |at: u64, t: &crate::session::Task, cx: &mut Context<Self>| {
            let (id, running) = (t.id.clone(), t.ended.is_none());
            let time = match &t.ended {
                Some((_, took)) => crate::when::took(*took),
                None => crate::when::took(t.since.elapsed().unwrap_or_default()),
            };
            row_of(SharedString::from(format!("bg-task-{at}-{}", t.id)))
                .child(
                    div()
                        .size(px(16.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .child(if running {
                            ui::live_dot().into_any_element()
                        } else {
                            ui::icon("terminal", 13.)
                                .text_color(match t.ended.as_ref().map(|(how, _)| how.as_str()) {
                                    Some("failed") => theme::coral(),
                                    _ => theme::faint(),
                                })
                                .into_any_element()
                        }),
                )
                .child(title(&t.name))
                .child(fine(faces, time))
                .when(running && t.stoppable, |d| {
                    d.child(
                        stop(SharedString::from(format!("bg-task-stop-{at}-{}", t.id))).on_click(
                            cx.listener(move |this, _, _, cx| this.stop_task(at, id.clone(), cx)),
                        ),
                    )
                })
        };
        let mut list = div().flex().flex_col();
        for c in kids.iter().filter(|c| c.sub.needs_you) {
            list = list.child(sub_row(c, cx));
        }
        for c in kids.iter().filter(|c| c.sub.working() && !c.sub.needs_you) {
            list = list.child(sub_row(c, cx));
        }
        for (at, t) in tasks.iter().filter(|(_, t)| t.ended.is_none()) {
            list = list.child(task_row(*at, t, cx));
        }
        if over > 0 {
            let toggle = fold.clone();
            list = list.child(
                row_of(SharedString::from(format!("bg-over-{family}")))
                    .cursor_pointer()
                    .child(
                        ui::icon(
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            12.,
                        )
                        .text_color(theme::faint()),
                    )
                    .child(fine(faces, format!("{over} finished")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&toggle) {
                            this.expanded.insert(toggle.clone());
                        }
                        cx.notify();
                    })),
            );
            if open {
                for c in kids.iter().filter(|c| !live(c)) {
                    list = list.child(sub_row(c, cx));
                }
                for (at, t) in tasks.iter().filter(|(_, t)| t.ended.is_some()) {
                    list = list.child(task_row(*at, t, cx));
                }
            }
        }
        sec = sec.child(list);
        Some(sec)
    }

    /// Widen the column over the thread, or give the thread back.
    fn whole_button(&self, cx: &mut Context<Self>) -> Button {
        let what = if self.panes.whole {
            "Back beside the thread"
        } else {
            "Widen over the thread"
        };
        ui::icon_button(
            "whole",
            "expand",
            crate::keys::hint(cx, what, "whole_right"),
        )
        .when(self.panes.whole, |d| d.text_color(theme::text()))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_whole(cx)))
    }

    /// The branch where the session works; a click lists the others, to
    /// check one out.
    fn branch_chip(
        &self,
        faces: &Faces,
        s: &Session,
        branch: &str,
        cx: &mut Context<Self>,
    ) -> Popover {
        let chip = ui::opener("branch-chip")
            .h(px(22.))
            .px(px(6.))
            .child(fine_mono(faces, ui::fit(branch, 22)))
            .child(ui::icon("chevron-down", 10.).text_color(theme::faint()));
        let (faces, key, branch) = (faces.clone(), s.key, branch.to_owned());
        self.menu(
            crate::desk::Popup::Branch,
            chip,
            move |d, _, cx| d.branch_menu(&faces, key, &branch, cx),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    /// The branches to check out, and a line for a new one.
    fn branch_menu(&self, faces: &Faces, key: u64, branch: &str, cx: &mut Context<Self>) -> Div {
        let mut menu = div().flex().flex_col().w(px(228.)).child(
            ui::text(faces, "Check out")
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(11.5))
                .text_color(theme::faint()),
        );
        let branches = self.sessions.at(key).map(|s| s.git.branches.as_slice());
        for (n, b) in branches.unwrap_or_default().iter().enumerate() {
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
                    .child(ui::mono(faces, ui::fit(b, 30)).text_size(px(12.)).flex_1())
                    .child({
                        let copied = name.clone();
                        ui::icon_button(
                            SharedString::from(format!("branch-copy-{n}")),
                            "copy",
                            "Copy its name",
                        )
                        .size(px(20.))
                        .text_color(theme::faint())
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                        })
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.switch_branch(name.clone(), cx)),
                    ),
            );
        }
        // A new one, from where it is: its name, then Return.
        menu.child(
            div()
                .mt(px(4.))
                .mx(px(4.))
                .child(Input::new(&self.menu_search).xsmall()),
        )
    }

    fn plan(faces: &Faces, s: &Session) -> Div {
        let Some(plan) = s.plan.as_ref().filter(|p| !p.entries.is_empty()) else {
            return section(faces, Detail::Plan.head(), None)
                .child(empty(faces, "Shown when the agent makes one."));
        };
        let steps = thread::plan_steps(plan);
        let done = steps
            .iter()
            .filter(|(st, _)| *st == kinds::Status::Done)
            .count();
        let note = fine(faces, format!("{done} of {}", steps.len()))
            .when(done == steps.len(), |d| d.text_color(theme::ok()));
        section(faces, Detail::Plan.head(), Some(note)).child(drawn::plan(faces, &steps))
    }

    /// The servers the session's terminals run (each a local address one
    /// printed): opened in the browser, stopped with their terminal, or,
    /// on another machine, forwarded here first.
    fn servers(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let found: Vec<(u64, String)> = self
            .drawer
            .terminals
            .iter()
            .filter(|t| t.session == s.key && t.ended.is_none())
            .filter_map(|t| crate::desk::runs::address(&t.text()).map(|a| (t.id, a)))
            .collect();
        if found.is_empty() {
            return None;
        }
        let remote = crate::remote::parse(&s.meta.project).map(|(host, _)| host);
        let mut sec = section(
            faces,
            Detail::Servers.head(),
            Some(fine(faces, found.len().to_string())),
        );
        for (n, (id, address)) in found.into_iter().enumerate() {
            let (open, forward) = (address.clone(), address.clone());
            let host = remote.clone();
            sec = sec.child(
                frow()
                    .child(ui::dot(theme::ok()))
                    .child(ui::one_line(fine_mono(faces, address.clone())).flex_1())
                    .child(match host {
                        Some(host) => ui::button(
                            SharedString::from(format!("forward-{n}")),
                            "Forward",
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.forward(&host, &forward, cx)),
                        ),
                        None => ui::button(
                            SharedString::from(format!("serve-open-{n}")),
                            "Open",
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(move |_, _, cx| cx.open_url(&open)),
                    })
                    .child(
                        ui::button(
                            SharedString::from(format!("serve-stop-{n}")),
                            "Stop",
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| this.close_terminal(id, cx))),
                    ),
            );
        }
        Some(sec)
    }

    fn commands(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let commands: Vec<_> = s.commands().collect();
        if commands.is_empty() {
            return section(faces, Detail::Terminals.head(), None)
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
        let mut sec = section(faces, Detail::Terminals.head(), Some(fine(faces, note)));
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
        let agent = match self.slot(&s.meta.agent).filter(|x| x.running()) {
            Some(x) => x
                .known
                .as_ref()
                .and_then(|k| k.agent_info.as_ref())
                .map_or_else(
                    || self.name_of(&s.meta.agent),
                    |i| {
                        format!(
                            "{} {}",
                            i.name.rsplit('/').next().unwrap_or(&i.name),
                            i.version
                        )
                    },
                ),
            _ => self.name_of(&s.meta.agent),
        };
        let mut sec = section(faces, Detail::Context.head(), note)
            .border_b_0()
            .children(share.map(|share| {
                Progress::new("context-share")
                    .xsmall()
                    .value(share * 100.)
                    .color(theme::muted())
            }))
            .child(key_value(
                faces,
                "Agent",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 12.))
                    .child(fine(faces, format!("{agent} · ACP v1"))),
            ))
            .child(key_value(faces, "Working directory", {
                // A long one keeps its last folder, which says which it is.
                let dir = Desk::shown(&s.meta.folder);
                let name = s.meta.folder.file_name().map(|n| n.to_string_lossy());
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
        for folder in &s.meta.also {
            sec = sec.child(key_value(
                faces,
                "And in",
                fine_mono(faces, ui::fit(&Desk::shown(folder), 24)),
            ));
        }
        if let Some(wt) = &s.meta.worktree {
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
                                        crate::repo::GitAction::RemoveWorktree(
                                            wt_for_click.clone(),
                                        ),
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
                .duration_since(s.meta.begun)
                .unwrap_or_default();
            let turns = s.turn_count();
            let words = format!(
                "{} · {} · {turns} {}",
                when::clock(s.meta.begun, self.settings.twelve),
                when::took(since),
                if turns == 1 { "turn" } else { "turns" }
            );
            sec = sec.child(key_value(faces, "Started", fine(faces, words)));
        }
        sec
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
    id: &'static str,
    label: &'static str,
    weight: Weight,
    cx: &mut Context<Desk>,
    make: fn(String) -> crate::repo::GitAction,
) -> Button {
    ui::button(id, label, weight).on_click(cx.listener(move |this, _, _, cx| {
        let message = this.commit_message(cx);
        this.review.committing = false;
        this.git_action(make(message), cx);
    }))
}
