//! The column down the right, as the design draws it. For a session: tabs
//! for the session and its files, then what it has come to (the files it
//! changed, its plan, the commands it ran, how full its context is); a
//! changed file opens here whole. For an agent's page: what it is and how
//! Ghost runs it.

use std::time::SystemTime;

use agent_client_protocol_schema::v1::{ToolCallContent, ToolCallStatus};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Aside, Desk, RIGHT, TOPBAR, draggable},
    fonts::Faces,
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
            return Self::file_view(faces, s, path, cx);
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
            .children(s.branch.clone().map(|b| fine_mono(faces, b)));
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
            body.child(Self::changes(faces, s, cx))
                .child(Self::plan(faces, s))
                .child(Self::terminals(faces, s))
                .child(self.context(faces, s))
        };
        frame(head).child(body)
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
                    Desk::shown(&s.folder)
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

    fn terminals(faces: &Faces, s: &Session) -> Div {
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
            let text = t.call.title.trim().trim_matches('`').to_owned();
            sec = sec.child(
                frow()
                    .child(dot)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(thread::command(faces, &text, 12.5)),
                    )
                    .child(time),
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

    fn context(&self, faces: &Faces, s: &Session) -> Div {
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
            .child(key_value(
                faces,
                "Working directory",
                fine_mono(faces, Desk::shown(&s.folder)),
            ));
        if let Some(mode) = Desk::mode_name(s) {
            sec = sec.child(key_value(faces, "Mode", fine(faces, mode)));
        }
        if !s.items.is_empty() {
            let since = SystemTime::now()
                .duration_since(s.begun)
                .unwrap_or_default();
            let turns = s.turns();
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
        let total = s.changed().into_iter().find(|c| c.path == path);
        let head = top()
            .gap(px(8.))
            .child(
                ui::icon_button("back", "chevron-left", "Back to the session")
                    .on_click(cx.listener(|this, _, _, cx| this.show_aside(Aside::Session, cx))),
            )
            .child(path_text(faces, &name))
            .children(total.map(|c| added_gone(faces, c.added, c.gone)));
        let mut body = div()
            .id("aside-file")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .py(px(6.));
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
        frame(head).child(body)
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
