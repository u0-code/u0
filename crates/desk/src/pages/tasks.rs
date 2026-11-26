//! Settings › Scheduled tasks, as t3code's: each task a row (what it says,
//! when, how its last run went) with its switch and its menu, and above
//! them the one being written.

use std::time::SystemTime;

use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Input, Textarea},
    menu::PopupMenu,
    setting::{SettingGroup, SettingItem},
    switch::Switch,
    tag::Tag,
    time_field::TimeField,
};
use gpui::{Axis, Context, Div, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    store,
    tasks::{DAYS, Schedule, Task},
    theme,
    ui::{self, Weight},
    when,
};

impl Desk {
    /// A task: its name and prompt, when it runs and how it last went;
    /// its switch, and its menu.
    fn task_row(&self, faces: &Faces, task: &Task, cx: &mut Context<Self>) -> Div {
        let now = SystemTime::now();
        let twelve = self.settings.twelve;
        let when_next = match task.next {
            _ if task.paused => "Paused".to_owned(),
            Some(at) if at <= store::secs(now) => "Due now".to_owned(),
            Some(at) => format!("Next {}", when::said_at(store::time(at), now, twelve)),
            None if matches!(task.schedule, Schedule::Hook(_)) => "When called".to_owned(),
            None => "Not scheduled".to_owned(),
        };
        let status = task.last.as_ref().map(|r| match (&r.error, r.going) {
            (_, true) => Tag::info().small().child("Running"),
            (Some(_), _) => Tag::danger().small().child("Failed"),
            (None, _) => Tag::success().small().child("Succeeded"),
        });
        let session =
            task.last
                .as_ref()
                .and_then(|r| r.session.clone())
                .and_then(|id| {
                    let s =
                        self.sessions.all.iter().find(|s| {
                            s.meta.agent == task.agent && s.meta.id.as_ref() == Some(&id)
                        })?;
                    Some((s.key, s.meta.title.clone()))
                });
        let (id, menu_id) = (task.id, task.id);
        let line = format!(
            "{} · {} · {} · {when_next}",
            task.schedule.said(twelve),
            self.name_of(&task.agent),
            self.project_name(&task.project),
        );
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(ui::text(faces, task.title.clone()).text_size(px(crate::style::LABEL)))
                    .child(ui::one_line(
                        ui::text(
                            faces,
                            task.prompt.lines().next().unwrap_or_default().to_owned(),
                        )
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::text2()),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .pt(px(2.))
                            .child(ui::fine(faces, line))
                            .children(status)
                            .children(session.map(|(key, title)| {
                                ui::opener(SharedString::from(format!("task-session-{id}")))
                                    .xsmall()
                                    .label(ui::fit(&title, 32))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.close_settings(window, cx);
                                        this.show(key, window, cx);
                                    }))
                            }))
                            .children(task.last.as_ref().and_then(|r| r.error.clone()).map(
                                |why| {
                                    ui::one_line(
                                        ui::text(faces, why)
                                            .text_size(px(crate::style::CAPTION))
                                            .text_color(theme::coral()),
                                    )
                                },
                            )),
                    ),
            )
            .child(
                Switch::new(SharedString::from(format!("task-on-{id}")))
                    .accessibility_label("On")
                    .small()
                    .checked(!task.paused)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pause_task(id);
                        cx.notify();
                    })),
            )
            .child(self.task_menu(menu_id, cx))
    }

    /// A task's menu: change it, run it now, delete it.
    fn task_menu(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = ui::icon_button(
            SharedString::from(format!("task-menu-{id}")),
            "dots",
            "More",
        )
        .selected(self.popup == Popup::Task(id));
        self.menu(
            Popup::Task(id),
            trigger,
            move |_, menu, desk, _| {
                menu.item(
                    Self::menu_item(desk, "Edit", move |this, window, cx| {
                        this.edit_task(id, window, cx);
                    })
                    .icon(ui::menu_glyph("pencil")),
                )
                .item(
                    Self::menu_item(desk, "Run now", move |this, _, cx| {
                        this.run_task(id);
                        cx.notify();
                    })
                    .icon(ui::menu_glyph("play")),
                )
                .item(
                    Self::menu_item(desk, "Delete", move |this, _, cx| {
                        this.drop_task(id);
                        cx.notify();
                    })
                    .icon(ui::menu_glyph("trash")),
                )
                .min_w(px(180.))
            },
            cx,
        )
    }

    /// The Scheduled tasks tab: the one being written, and the tasks.
    pub(super) fn tasks(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let mut list: Vec<SettingItem> = Vec::new();
        if self.tasks.draft.is_none() {
            list.push(item(
                w,
                "A new task",
                "Its prompt, said to an agent on a schedule.",
                |_, _, cx| {
                    ui::button("new-task", "New task", Weight::Secondary)
                        .on_click(cx.listener(|this, _, window, cx| this.new_task(window, cx)))
                        .into_any_element()
                },
            ));
        }
        match &self.tasks.list {
            Err(why) => {
                let why = why.clone();
                list.push(whole(w, &[], move |_, _, _| {
                    ui::trouble(why.clone()).into_any_element()
                }));
            }
            Ok(tasks) if tasks.is_empty() => list.push(item(
                w,
                "No tasks yet",
                "A task says its prompt to an agent on a schedule, as a session of its own.",
                |_, _, _| div().into_any_element(),
            )),
            Ok(tasks) => {
                for task in tasks {
                    let (task, named) = (task.clone(), task.title.clone());
                    list.push(
                        whole(w, &[], move |d, faces, cx| {
                            d.task_row(faces, &task, cx).into_any_element()
                        })
                        .keywords([named]),
                    );
                }
            }
        }
        let mut groups: Vec<SettingGroup> = self.task_form(w).into_iter().collect();
        groups.push(group("Tasks", list));
        groups
    }

    /// The task being written: what it says and to whom, where, when, and
    /// where each run goes; then cancelled or kept.
    fn task_form(&self, w: &WeakEntity<Desk>) -> Option<SettingGroup> {
        let draft = self.tasks.draft.as_ref()?;
        let task = &draft.task;
        let mut items = Vec::new();
        if let Some(wrong) = draft.wrong.clone() {
            items.push(whole(w, &[], move |_, _, _| {
                ui::trouble(wrong.clone()).into_any_element()
            }));
        }
        items.extend([
            item(w, "Name", "", |d, _, _| {
                d.tasks.draft.as_ref().map_or_else(
                    || div().into_any_element(),
                    |draft| Input::new(&draft.title).small().into_any_element(),
                )
            })
            .layout(Axis::Vertical),
            item(w, "Prompt", "", |d, _, _| {
                d.tasks.draft.as_ref().map_or_else(
                    || div().into_any_element(),
                    |draft| Textarea::new(&draft.prompt).into_any_element(),
                )
            })
            .layout(Axis::Vertical),
            item(
                w,
                "Agent",
                "Who it is said to, with which model.",
                |d, faces, cx| d.task_agent_pick(faces, cx).into_any_element(),
            ),
            item(w, "Project", "Where it works.", |d, _, cx| {
                d.task_project_pick(cx).into_any_element()
            }),
            item(w, "Schedule", "", |d, faces, cx| {
                d.task_schedule(faces, cx).into_any_element()
            })
            .layout(Axis::Vertical),
            item(
                w,
                "Each run",
                "A session of its own, or the next turn of the first run's.",
                |d, _, cx| {
                    let same = d.tasks.draft.as_ref().is_some_and(|d| d.task.same);
                    ui::segmented(
                        "task-same",
                        vec![
                            (
                                "A new session each run",
                                !same,
                                draft_pick(cx, |d| d.task.same = false),
                            ),
                            (
                                "One session, continued",
                                same,
                                draft_pick(cx, |d| d.task.same = true),
                            ),
                        ],
                    )
                    .into_any_element()
                },
            ),
        ]);
        if !task.same {
            items.push(item(
                w,
                "In a worktree of its own",
                "Each run's session works on a new branch, away from your checkout.",
                |d, _, cx| {
                    Switch::new("task-worktree")
                        .accessibility_label("In a worktree of its own")
                        .small()
                        .checked(d.tasks.draft.as_ref().is_some_and(|d| d.task.worktree))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(d) = &mut this.tasks.draft {
                                d.task.worktree = !d.task.worktree;
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                },
            ));
        }
        let (save, buttons) = (if task.id == 0 { "Add task" } else { "Save" }, w.clone());
        Some(
            group(
                if task.id == 0 {
                    "New task"
                } else {
                    "Edit task"
                },
                items,
            )
            .footer(move |_, _| {
                let (cancel, keep) = (buttons.clone(), buttons.clone());
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .w_full()
                    .child(ui::button("task-cancel", "Cancel", Weight::Quiet).on_click(
                        move |_, _, cx| {
                            let _ = cancel.update(cx, |this, cx| {
                                this.tasks.draft = None;
                                cx.notify();
                            });
                        },
                    ))
                    .child(ui::button("task-save", save, Weight::Primary).on_click(
                        move |_, _, cx| {
                            let _ = keep.update(cx, Desk::save_task);
                        },
                    ))
            }),
        )
    }

    /// When the task being written runs: every while, each day at a time
    /// (on some days), or once.
    fn task_schedule(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some(draft) = self.tasks.draft.as_ref() else {
            return div();
        };
        let task = &draft.task;
        let kind = |k: u8, cx: &mut Context<Self>| {
            draft_pick(cx, move |draft| {
                draft.task.schedule = match k {
                    0 => Schedule::Every(60),
                    1 => Schedule::Daily {
                        minute: 9 * 60,
                        days: Vec::new(),
                    },
                    2 => Schedule::Once(0),
                    _ => Schedule::Hook(crate::access::random::<16>()),
                };
            })
        };
        let schedule = ui::segmented(
            "task-kind",
            vec![
                (
                    "Every",
                    matches!(task.schedule, Schedule::Every(_)),
                    kind(0, cx),
                ),
                (
                    "Daily",
                    matches!(task.schedule, Schedule::Daily { .. }),
                    kind(1, cx),
                ),
                (
                    "Once",
                    matches!(task.schedule, Schedule::Once(_)),
                    kind(2, cx),
                ),
                (
                    "On webhook",
                    matches!(task.schedule, Schedule::Hook(_)),
                    kind(3, cx),
                ),
            ],
        );
        let when_field = match &task.schedule {
            Schedule::Every(_) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(120.)).child(Input::new(&draft.every).small()))
                .child(ui::fine(faces, "minutes, or a while such as 2h")),
            Schedule::Daily { days, .. } => {
                let mut group = gpui::component::button::ButtonGroup::new("task-days")
                    .small()
                    .multiple(true);
                for (n, name) in DAYS.iter().enumerate() {
                    let on = days.contains(&u8::try_from(n).unwrap_or(0));
                    group = group.child(
                        gpui::component::button::Button::new(n)
                            .label(*name)
                            .selected(on),
                    );
                }
                let group = group.on_click(cx.listener(|this, chosen: &Vec<usize>, _, cx| {
                    if let Some(Schedule::Daily { days, .. }) =
                        this.tasks.draft.as_mut().map(|d| &mut d.task.schedule)
                    {
                        *days = chosen
                            .iter()
                            .filter_map(|n| u8::try_from(*n).ok())
                            .collect();
                    }
                    cx.notify();
                }));
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(TimeField::new(&draft.time))
                    .child(group)
                    .child(ui::fine(faces, "none chosen: every day"))
            }
            Schedule::Once(_) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(200.)).child(Input::new(&draft.at).small()))
                .child(ui::fine(faces, "tomorrow, 5pm, in 3h")),
            Schedule::Hook(token) => self.hook_addresses(faces, token),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(schedule)
            .child(when_field)
    }

    /// The agent and model the task being written is said to, and the menu
    /// to choose another.
    fn task_agent_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let task = &self
            .tasks
            .draft
            .as_ref()
            .expect("a task is being written")
            .task;
        let shown = if task.agent.is_empty() {
            "Choose an agent".to_owned()
        } else {
            let model = task.model.as_ref().map_or_else(
                || "its default".to_owned(),
                |m| self.model_name(&task.agent, m).unwrap_or_else(|| m.clone()),
            );
            format!("{} · {model}", self.name_of(&task.agent))
        };
        let trigger = ui::opener("task-agent")
            .selected(self.popup == Popup::TaskAgent)
            .children(
                (!task.agent.is_empty()).then(|| ui::mark(faces, &self.mark_of(&task.agent), 14.)),
            )
            .child(shown);
        self.menu(
            Popup::TaskAgent,
            trigger,
            |d, menu, desk, _| {
                let task = d.tasks.draft.as_ref().map(|d| &d.task);
                let (agent, model) =
                    task.map_or((None, None), |t| (Some(&*t.agent), t.model.as_deref()));
                d.models_menu(menu, desk, None, (agent, model), |d, agent, model| {
                    if let (Some(draft), Some(agent)) = (&mut d.tasks.draft, agent) {
                        (draft.task.agent, draft.task.model) = (agent, model);
                    }
                })
            },
            cx,
        )
    }

    /// The project the task being written works in, and the menu of the
    /// projects to choose another.
    fn task_project_pick(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let task = &self
            .tasks
            .draft
            .as_ref()
            .expect("a task is being written")
            .task;
        let trigger = ui::opener("task-project")
            .selected(self.popup == Popup::TaskProject)
            .child(ui::icon("folder", 14.))
            .child(if task.project.as_os_str().is_empty() {
                "Choose a project".to_owned()
            } else {
                self.project_name(&task.project)
            });
        self.menu(
            Popup::TaskProject,
            trigger,
            |d, menu, desk, _| {
                let chosen = d.tasks.draft.as_ref().map(|d| &d.task.project);
                let items = d.settings.folders.iter().map(|folder| {
                    let f = folder.clone();
                    Self::menu_item(desk, d.project_name(folder), move |this, _, cx| {
                        if let Some(draft) = &mut this.tasks.draft {
                            draft.task.project.clone_from(&f);
                        }
                        cx.notify();
                    })
                    .checked(chosen == Some(folder))
                });
                items.fold(menu.min_w(px(260.)), PopupMenu::item)
            },
            cx,
        )
    }
}

impl Desk {
    /// Where a webhook task is called: here, while Ghost listens, and
    /// through the tunnel while one is up; what it does with the call.
    fn hook_addresses(&self, faces: &Faces, token: &str) -> Div {
        let shared = self.pages.shared.as_ref();
        let port = shared.and_then(|s| s.serving.clone()?.ok());
        let tunnel = shared.and_then(|s| match &s.tunnel {
            crate::share::Tunnel::Up(host) => Some(host.clone()),
            _ => None,
        });
        let mut at: Vec<String> = Vec::new();
        at.extend(port.map(|p| format!("http://127.0.0.1:{p}/hook/{token}")));
        at.extend(tunnel.map(|h| format!("https://{h}/hook/{token}")));
        let mut out = div().flex().flex_col().gap(px(4.));
        if at.is_empty() {
            out = out.child(ui::fine(
                faces,
                format!("/hook/{token}, where Ghost listens: turn listening on in Devices for its address."),
            ));
        }
        for a in at {
            out = out.child(ui::fine_mono(faces, a));
        }
        out.child(ui::fine(
            faces,
            "A POST to it runs the task; {{body}} in what it says is what was sent, marked as data from outside. A call made while Ghost is not running is not held: the sender has to send it again.",
        ))
    }
}

/// What picking a choice of the task being written does: `change` it.
fn draft_pick(
    cx: &mut Context<Desk>,
    change: impl Fn(&mut crate::desk::tasks::Draft) + 'static,
) -> ui::Pick {
    Box::new(cx.listener(move |this, _, _, cx| {
        if let Some(draft) = &mut this.tasks.draft {
            change(draft);
        }
        cx.notify();
    }))
}
