//! Settings › Scheduled tasks, as t3code's: each task a row (what it says,
//! when, how its last run went) with its switch and its menu, and above
//! them the one being written.

use std::time::SystemTime;

use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Input, Textarea},
    scroll::ScrollableElement as _,
    switch::Switch,
    tag::Tag,
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{card, list_of, list_row};
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
    /// The Scheduled tasks tab: the one being written, and the tasks.
    pub(super) fn tasks_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let new = ui::button("new-task", "New task", Weight::Secondary)
            .on_click(cx.listener(|this, _, window, cx| this.new_task(window, cx)));
        let head = div()
            .flex()
            .items_center()
            .child(ui::label(faces, "Tasks").flex_1())
            .when(self.tasks.draft.is_none(), |d| d.child(new));
        let mut list = card().overflow_hidden();
        match &self.tasks.list {
            Err(why) => list = list.child(div().p(px(12.)).child(ui::trouble(why.clone()))),
            Ok(tasks) if tasks.is_empty() => {
                list = list.child(div().px(px(16.)).py(px(14.)).child(ui::fine(
                    faces,
                    "No tasks yet. A task says its prompt to an agent on a schedule, as a session of its own.",
                )));
            }
            Ok(tasks) => {
                for (n, task) in tasks.iter().enumerate() {
                    list = list.child(
                        self.task_row(faces, task, cx)
                            .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
                    );
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .w_full()
            .children(self.task_form(faces, cx))
            .child(div().flex().flex_col().gap(px(8.)).child(head).child(list))
    }

    /// A task: its name and prompt, when it runs and how it last went;
    /// its switch, and its menu.
    fn task_row(&self, faces: &Faces, task: &Task, cx: &mut Context<Self>) -> Div {
        let now = SystemTime::now();
        let twelve = self.settings.twelve;
        let when_next = match task.next {
            _ if task.paused => "Paused".to_owned(),
            Some(at) if at <= store::secs(now) => "Due now".to_owned(),
            Some(at) => format!("Next {}", when::said_at(store::time(at), now, twelve)),
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
            .px(px(16.))
            .py(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(ui::text(faces, task.title.clone()).text_size(px(13.5)))
                    .child(ui::one_line(
                        ui::text(
                            faces,
                            task.prompt.lines().next().unwrap_or_default().to_owned(),
                        )
                        .text_size(px(12.5))
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
                                            .text_size(px(11.5))
                                            .text_color(theme::coral()),
                                    )
                                },
                            )),
                    ),
            )
            .child(
                Switch::new(SharedString::from(format!("task-on-{id}")))
                    .small()
                    .checked(!task.paused)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pause_task(id);
                        cx.notify();
                    })),
            )
            .child(self.task_menu(faces, menu_id, cx))
    }

    /// A task's menu: change it, run it now, delete it.
    fn task_menu(&self, faces: &Faces, id: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = ui::icon_button(
            SharedString::from(format!("task-menu-{id}")),
            "dots",
            "More",
        )
        .selected(self.popup == Popup::Task(id));
        let faces = faces.clone();
        self.menu(
            Popup::Task(id),
            trigger,
            move |_, _, cx| {
                let item = |n: &str, icon: &str, label: &str| {
                    ui::menu_row(SharedString::from(format!("task-{n}-{id}")), false)
                        .child(ui::icon(icon, 14.).text_color(theme::muted()))
                        .child(ui::text(&faces, label.to_owned()).text_size(px(13.)))
                };
                div()
                    .flex()
                    .flex_col()
                    .w(px(180.))
                    .child(item("edit", "pencil", "Edit").on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.popup = Popup::Closed;
                            this.edit_task(id, window, cx);
                        },
                    )))
                    .child(item("run", "play", "Run now").on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.popup = Popup::Closed;
                            this.run_task(id);
                            cx.notify();
                        },
                    )))
                    .child(item("drop", "trash", "Delete").on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.popup = Popup::Closed;
                            this.drop_task(id);
                            cx.notify();
                        },
                    )))
            },
            cx,
        )
    }

    /// The task being written: what it says and to whom, where, when, and
    /// where each run goes.
    fn task_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let draft = self.tasks.draft.as_ref()?;
        let task = &draft.task;
        let field = |name: &str, input: gpui::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(16.))
                .py(px(12.))
                .child(ui::text(faces, name.to_owned()).text_size(px(13.5)))
                .child(input)
        };
        let kind = |k: u8, cx: &mut Context<Self>| {
            draft_pick(cx, move |draft| {
                draft.task.schedule = match k {
                    0 => Schedule::Every(60),
                    1 => Schedule::Daily {
                        minute: 9 * 60,
                        days: Vec::new(),
                    },
                    _ => Schedule::Once(0),
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
                    .child(div().w(px(120.)).child(Input::new(&draft.at).small()))
                    .child(group)
                    .child(ui::fine(faces, "none chosen: every day"))
            }
            Schedule::Once(_) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(200.)).child(Input::new(&draft.at).small()))
                .child(ui::fine(faces, "tomorrow, 5pm, in 3h")),
        };
        let runs = ui::segmented(
            "task-same",
            vec![
                (
                    "A new session each run",
                    !task.same,
                    draft_pick(cx, |d| d.task.same = false),
                ),
                (
                    "One session, continued",
                    task.same,
                    draft_pick(cx, |d| d.task.same = true),
                ),
            ],
        );
        let worktree = Switch::new("task-worktree")
            .small()
            .checked(task.worktree)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(d) = &mut this.tasks.draft {
                    d.task.worktree = !d.task.worktree;
                }
                cx.notify();
            }));
        let mut rows = vec![
            field("Name", Input::new(&draft.title).small().into_any_element()),
            field("Prompt", Textarea::new(&draft.prompt).into_any_element()),
            list_row(
                faces,
                "Agent",
                "Who it is said to, with which model.",
                self.task_agent_pick(faces, cx),
            ),
            list_row(
                faces,
                "Project",
                "Where it works.",
                self.task_project_pick(faces, cx),
            ),
            field(
                "Schedule",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(schedule)
                    .child(when_field)
                    .into_any_element(),
            ),
            list_row(
                faces,
                "Each run",
                "A session of its own, or the next turn of the first run's.",
                runs,
            ),
        ];
        if !task.same {
            rows.push(list_row(
                faces,
                "In a worktree of its own",
                "Each run's session works on a new branch, away from your checkout.",
                worktree,
            ));
        }
        let buttons = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .child(
                ui::button("task-cancel", "Cancel", Weight::Quiet).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.tasks.draft = None;
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::button(
                    "task-save",
                    if task.id == 0 { "Add task" } else { "Save" },
                    Weight::Primary,
                )
                .on_click(cx.listener(|this, _, _, cx| this.save_task(cx))),
            );
        rows.push(buttons);
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::label(
                    faces,
                    if task.id == 0 {
                        "New task"
                    } else {
                        "Edit task"
                    },
                ))
                .children(draft.wrong.clone().map(ui::trouble))
                .child(list_of(rows)),
        )
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
        let faces = faces.clone();
        self.menu(
            Popup::TaskAgent,
            trigger,
            move |d, _, cx| {
                let task = d.tasks.draft.as_ref().map(|d| d.task.clone());
                let (agent, model) = task.map(|t| (Some(t.agent), t.model)).unwrap_or_default();
                d.models_menu(
                    &faces,
                    None,
                    (agent.as_deref(), model.as_deref()),
                    |d, agent, model| {
                        if let (Some(draft), Some(agent)) = (&mut d.tasks.draft, agent) {
                            (draft.task.agent, draft.task.model) = (agent, model);
                        }
                    },
                    cx,
                )
                .overflow_y_scrollbar()
            },
            cx,
        )
    }

    /// The project the task being written works in, and the menu of the
    /// projects to choose another.
    fn task_project_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
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
        let faces = faces.clone();
        self.menu(
            Popup::TaskProject,
            trigger,
            move |d, _, cx| {
                let chosen = d.tasks.draft.as_ref().map(|d| d.task.project.clone());
                let mut list = div().flex().flex_col().w(px(260.));
                for (n, folder) in d.settings.folders.iter().enumerate() {
                    let f = folder.clone();
                    list = list.child(
                        ui::menu_row(
                            SharedString::from(format!("task-project-{n}")),
                            chosen.as_ref() == Some(folder),
                        )
                        .child(ui::one_line(
                            ui::text(&faces, d.project_name(folder)).text_size(px(13.)),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(draft) = &mut this.tasks.draft {
                                draft.task.project.clone_from(&f);
                            }
                            this.popup = Popup::Closed;
                            cx.notify();
                        })),
                    );
                }
                list
            },
            cx,
        )
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
