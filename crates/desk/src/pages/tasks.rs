//! Settings › Scheduled tasks, as t3code's: each task a row (what it says,
//! when, how its last run went) with its switch and its menu, and above
//! them the one being written.

use std::time::SystemTime;

use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Input, Textarea},
    menu::PopupMenu,
    time_field::TimeField,
};
use gpui::{Context, Div, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use u0ui::{
    icons::{CarbonIcon, Icon as CarbonGlyph},
    tokens::ColorRole,
};

use super::srow::{Group, Item, group, item, whole};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    store,
    tasks::Moment,
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
        // An event's task runs as it happens: its schedule says when.
        let when_next = match task.next {
            _ if task.paused => Some(crate::tr("Paused")),
            Some(at) if at <= store::secs(now) => Some(crate::tr("Due now")),
            Some(at) => Some(crate::tr_with(
                "Next run %{when}",
                &[("when", &when::said_at(store::time(at), now, twelve))],
            )),
            None if matches!(task.schedule, Schedule::Hook(_)) => Some(crate::tr("When called")),
            None if matches!(task.schedule, Schedule::On { .. }) => None,
            None => Some(crate::tr("Not scheduled")),
        };
        // How it last went, on the right: a status glyph and when.
        let last = task.last.as_ref().map(|r| {
            let (glyph, role, said) = match (&r.error, r.going) {
                (_, true) => (CarbonIcon::InProgress, ColorRole::SupportInfo, "Running"),
                (Some(_), _) => (CarbonIcon::ErrorFilled, ColorRole::SupportError, "Failed"),
                (None, _) => (
                    CarbonIcon::CheckmarkFilled,
                    ColorRole::SupportSuccess,
                    "Succeeded",
                ),
            };
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    CarbonGlyph::carbon(glyph)
                        .size_length(px(14.))
                        .color(theme::role(role)),
                )
                .child(ui::fine(
                    faces,
                    crate::tr_with(
                        "%{how} · last run %{when}",
                        &[
                            ("how", &crate::tr(said)),
                            ("when", &when::said_at(store::time(r.at), now, twelve)),
                        ],
                    ),
                ))
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
            "{} · {} · {}",
            task.schedule.said(twelve),
            self.name_of(&task.agent),
            self.project_name(&crate::desk::Place::new(
                self.pages.on,
                task.project.clone()
            )),
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
                            .children(task.last.as_ref().and_then(|r| r.error.clone()).map(
                                |why| {
                                    // Whole, folded where long, with Copy.
                                    ui::trouble(SharedString::from(format!("task-error-{id}")), why)
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(2.))
                    .flex_none()
                    .children(last)
                    .children(when_next.map(|n| ui::fine(faces, n)))
                    .children(session.map(|(key, title)| {
                        div()
                            .id(SharedString::from(format!("task-session-{id}")))
                            .cursor_pointer()
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::link())
                            .hover(gpui::Styled::underline)
                            .tooltip(move |window, cx| {
                                gpui::component::tooltip::Tooltip::new(title.clone())
                                    .build(window, cx)
                            })
                            .child(crate::tr("Open thread"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_settings(window, cx);
                                this.show(key, window, cx);
                            }))
                    })),
            )
            .child(
                u0ui::components::button::Button::new(
                    SharedString::from(format!("task-run-{id}")),
                    crate::tr("Run now"),
                )
                .variant(u0ui::components::button::ButtonVariant::Ghost)
                .size(u0ui::components::button::ButtonSize::Sm)
                .icon(CarbonIcon::Play)
                .icon_only(true)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.run_task(id);
                    cx.notify();
                })),
            )
            .child(super::srow::flip(
                SharedString::from(format!("task-on-{id}")),
                crate::tr("On"),
                !task.paused,
                cx,
                move |this, _, cx| {
                    this.pause_task(id);
                    cx.notify();
                },
            ))
            .child(self.task_menu(menu_id, cx))
    }

    /// A task's menu: change it, run it now, delete it.
    fn task_menu(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = ui::icon_button(
            SharedString::from(format!("task-menu-{id}")),
            "dots",
            crate::tr("What can be done with it"),
        )
        .selected(self.popup == Popup::Task(id));
        self.menu(
            Popup::Task(id),
            trigger,
            move |_, menu, desk, _| {
                menu.item(
                    Self::menu_item(desk, crate::tr("Edit"), move |this, window, cx| {
                        this.edit_task(id, window, cx);
                    })
                    .icon(ui::menu_glyph("pencil")),
                )
                .item(
                    Self::menu_item(desk, crate::tr("Run now"), move |this, _, cx| {
                        this.run_task(id);
                        cx.notify();
                    })
                    .icon(ui::menu_glyph("play")),
                )
                .item(
                    Self::menu_item(desk, crate::tr("Delete"), move |this, _, cx| {
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
    pub(super) fn tasks(&self, w: &WeakEntity<Desk>) -> Vec<Group> {
        let mut list: Vec<Item> = Vec::new();
        let has_tasks = matches!(self.tasks_on(self.pages.on), Some(Ok(t)) if !t.is_empty());
        match self.tasks_on(self.pages.on) {
            None => list.push(whole(w, &[], |_, _, _| {
                use u0ui::components::spinner::{Spinner, SpinnerSize};
                Spinner::new("tasks-reading")
                    .size(SpinnerSize::Inline)
                    .description(crate::tr("Reading…"))
                    .into_any_element()
            })),
            Some(Err(why)) => {
                let why = why.clone();
                list.push(whole(w, &[], move |d, _, _| {
                    crate::desk::banners::banner(
                        "tasks-bad-file",
                        u0ui::components::alert::AlertVariant::Error,
                        crate::tr("tasks.json does not read"),
                        d.tidy_why(&why),
                        None,
                        None,
                    )
                }));
            }
            Some(Ok(tasks)) if tasks.is_empty() => {
                let writing = self.tasks.draft.is_some();
                list.push(whole(w, &[], move |_, _, cx| {
                    use u0ui::components::empty::{
                        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
                    };
                    use u0ui::prelude::*;
                    Empty::new()
                        .child(
                            EmptyHeader::new()
                                .child(EmptyTitle::new(crate::tr("No tasks yet")))
                                .child(EmptyDescription::new(crate::tr(
                                    "Run a prompt with an agent on a schedule.",
                                ))),
                        )
                        .when(!writing, |e| {
                            e.child(
                                EmptyContent::new().child(
                                    Button::new("empty-new-task", crate::tr("New task"))
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::Add)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.new_task(window, cx);
                                        })),
                                ),
                            )
                        })
                        .into_any_element()
                }));
            }
            Some(Ok(tasks)) => {
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
        if self.tasks.draft.is_none() && has_tasks {
            list.push(whole(w, &["new"], |_, _, cx| {
                super::srow::button(
                    "new-task",
                    crate::tr("New scheduled task"),
                    Weight::Secondary,
                )
                .icon(CarbonIcon::Add)
                .on_click(cx.listener(|this, _, window, cx| this.new_task(window, cx)))
                .into_any_element()
            }));
        }
        let mut groups: Vec<Group> = self.task_form(w).into_iter().collect();
        groups.push(group(crate::tr("Tasks"), list));
        groups
    }

    /// The task being written: what it says and to whom, where, when, and
    /// where each run goes; then cancelled or kept.
    fn task_form(&self, w: &WeakEntity<Desk>) -> Option<Group> {
        let draft = self.tasks.draft.as_ref()?;
        let task = &draft.task;
        let mut items = Vec::new();
        if let Some(wrong) = draft.wrong.clone() {
            items.push(whole(w, &[], move |_, _, _| {
                ui::trouble("task-wrong", wrong.clone()).into_any_element()
            }));
        }
        items.extend([
            item(w, crate::tr("Name"), "", |d, _, _| {
                d.tasks.draft.as_ref().map_or_else(
                    || div().into_any_element(),
                    |draft| Input::new(&draft.title).small().into_any_element(),
                )
            })
            .stacked(),
            item(w, crate::tr("Prompt"), "", |d, _, _| {
                d.tasks.draft.as_ref().map_or_else(
                    || div().into_any_element(),
                    |draft| Textarea::new(&draft.prompt).into_any_element(),
                )
            })
            .stacked(),
            item(
                w,
                crate::tr("Agent"),
                crate::tr("Who it is said to, with which model."),
                |d, faces, cx| d.task_agent_pick(faces, cx).into_any_element(),
            ),
            item(
                w,
                crate::tr("Project"),
                crate::tr("Where it works."),
                |d, _, cx| d.task_project_pick(cx).into_any_element(),
            ),
            item(
                w,
                crate::tr("Each run"),
                crate::tr(
                    "Says its prompt to the agent, or begins a workflow with it as the input.",
                ),
                |d, _, cx| d.task_flow_pick(cx).into_any_element(),
            ),
            item(w, crate::tr("Schedule"), "", |d, faces, cx| {
                d.task_schedule(faces, cx).into_any_element()
            })
            .stacked(),
            item(
                w,
                crate::tr("Thread"),
                crate::tr("A thread of its own, or the next turn of the first run's."),
                |d, _, cx| {
                    let same = d.tasks.draft.as_ref().is_some_and(|d| d.task.same);
                    super::srow::picks(
                        "task-same",
                        vec![
                            (
                                crate::tr("A new thread each run"),
                                !same,
                                draft_pick(cx, |d| d.task.same = false),
                            ),
                            (
                                crate::tr("One thread, continued"),
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
                crate::tr("In a worktree of its own"),
                crate::tr("Each run's thread works on a new branch, away from your checkout."),
                |d, _, cx| {
                    super::srow::flip(
                        "task-worktree",
                        crate::tr("In a worktree of its own"),
                        d.tasks.draft.as_ref().is_some_and(|d| d.task.worktree),
                        cx,
                        |this, _, cx| {
                            if let Some(d) = &mut this.tasks.draft {
                                d.task.worktree = !d.task.worktree;
                            }
                            cx.notify();
                        },
                    )
                    .into_any_element()
                },
            ));
        }
        let (save, buttons) = (
            crate::tr(if task.id == 0 { "Add task" } else { "Save" }),
            w.clone(),
        );
        items.push(whole(w, &[], move |_, _, _| {
            let (cancel, keep) = (buttons.clone(), buttons.clone());
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .w_full()
                .child(
                    super::srow::button("task-cancel", crate::tr("Cancel"), Weight::Quiet)
                        .on_click(move |_, _, cx| {
                            let _ = cancel.update(cx, |this, cx| {
                                this.tasks.draft = None;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    super::srow::button("task-save", save.clone(), Weight::Primary).on_click(
                        move |_, _, cx| {
                            let _ = keep.update(cx, Desk::save_task);
                        },
                    ),
                )
                .into_any_element()
        }));
        Some(group(
            if task.id == 0 {
                "New task"
            } else {
                "Edit task"
            },
            items,
        ))
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
                    3 => Schedule::Hook(crate::tasks::Hook::new()),
                    _ => Schedule::On {
                        moment: Moment::TurnEnded,
                        here: true,
                    },
                };
            })
        };
        let schedule = super::srow::picks(
            "task-kind",
            vec![
                (
                    crate::tr("Every"),
                    matches!(task.schedule, Schedule::Every(_)),
                    kind(0, cx),
                ),
                (
                    crate::tr("Daily"),
                    matches!(task.schedule, Schedule::Daily { .. }),
                    kind(1, cx),
                ),
                (
                    crate::tr("Once"),
                    matches!(task.schedule, Schedule::Once(_)),
                    kind(2, cx),
                ),
                (
                    crate::tr("On webhook"),
                    matches!(task.schedule, Schedule::Hook(_)),
                    kind(3, cx),
                ),
                (
                    crate::tr("On an event"),
                    matches!(task.schedule, Schedule::On { .. }),
                    kind(4, cx),
                ),
            ],
        );
        let when_field = match &task.schedule {
            Schedule::Every(_) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(120.)).child(Input::new(&draft.every).small()))
                .child(ui::fine(faces, crate::tr("minutes, or a while such as 2h"))),
            Schedule::Daily { days, .. } => {
                let mut group = gpui::component::button::ButtonGroup::new("task-days")
                    .small()
                    .multiple(true);
                for (n, name) in DAYS.iter().enumerate() {
                    let on = days.contains(&u8::try_from(n).unwrap_or(0));
                    group = group.child(
                        gpui::component::button::Button::new(n)
                            .label(crate::tr(name))
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
                    .child(ui::fine(faces, crate::tr("none chosen: every day")))
            }
            Schedule::Once(_) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(200.)).child(Input::new(&draft.at).small()))
                .child(ui::fine(faces, crate::tr("tomorrow, 5pm, in 3h"))),
            Schedule::Hook(hook) => self.hook_addresses(faces, hook, cx),
            Schedule::On { moment, here } => {
                let (moment, here) = (*moment, *here);
                let on = |m: Moment, cx: &mut Context<Self>| {
                    draft_pick(cx, move |draft| {
                        if let Schedule::On { moment, .. } = &mut draft.task.schedule {
                            *moment = m;
                        }
                    })
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(super::srow::picks(
                        "task-moment",
                        vec![
                            (
                                crate::tr("A turn ends"),
                                moment == Moment::TurnEnded,
                                on(Moment::TurnEnded, cx),
                            ),
                            (
                                crate::tr("An agent asks"),
                                moment == Moment::Asks,
                                on(Moment::Asks, cx),
                            ),
                        ],
                    ))
                    .child(
                        super::srow::flip(
                            "task-here",
                            crate::tr("In this task's project alone"),
                            here,
                            cx,
                            move |this, _, cx| {
                                if let Some(Schedule::On { here: h, .. }) =
                                    this.tasks.draft.as_mut().map(|d| &mut d.task.schedule)
                                {
                                    *h = !here;
                                }
                                cx.notify();
                            },
                        )
                        .hide_label(false),
                    )
                    .child(ui::fine(
                        faces,
                        crate::tr("Only on your own threads. {{event}} is what happened, as outside data."),
                    ))
            }
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
            crate::tr("Choose an agent")
        } else {
            let model = task.model.as_ref().map_or_else(
                || crate::tr("its default").to_string(),
                |m| self.model_name(&task.agent, m).unwrap_or_else(|| m.clone()),
            );
            format!("{} · {model}", self.name_of(&task.agent)).into()
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

    /// What each run of the task being written does: says its prompt, or
    /// begins one of the workflows with it; and the menu to choose.
    fn task_flow_pick(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let flow = (self.tasks.draft.as_ref()).and_then(|d| d.task.workflow.clone());
        let trigger = ui::opener("task-flow")
            .selected(self.popup == Popup::TaskFlow)
            .child(flow.map_or_else(
                || crate::tr("Says its prompt"),
                |f| crate::tr_with("Begins %{workflow}", &[("workflow", &f)]),
            ));
        self.menu(
            Popup::TaskFlow,
            trigger,
            |d, menu, desk, _| {
                let chosen = d.tasks.draft.as_ref().and_then(|d| d.task.workflow.clone());
                let pick = |flow: Option<String>| {
                    move |this: &mut Desk, _: &mut gpui::Window, cx: &mut Context<Desk>| {
                        if let Some(draft) = &mut this.tasks.draft {
                            draft.task.workflow.clone_from(&flow);
                        }
                        cx.notify();
                    }
                };
                let says = Self::menu_item(desk, crate::tr("Says its prompt"), pick(None))
                    .checked(chosen.is_none());
                let flows = (d.flows.list.iter().filter_map(|f| f.as_ref().ok())).map(|f| {
                    let name = f.name.clone();
                    let begins = crate::tr_with("Begins %{workflow}", &[("workflow", &name)]);
                    Self::menu_item(desk, begins, pick(Some(name.clone())))
                        .checked(chosen.as_ref() == Some(&name))
                });
                std::iter::once(says)
                    .chain(flows)
                    .fold(menu.min_w(px(260.)), PopupMenu::item)
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
                crate::tr("Choose a project").to_string()
            } else {
                self.project_name(&crate::desk::Place::new(
                    self.pages.on,
                    task.project.clone(),
                ))
            });
        self.menu(
            Popup::TaskProject,
            trigger,
            |d, menu, desk, _| {
                let chosen = d.tasks.draft.as_ref().map(|d| &d.task.project);
                let items = d.edited().folders.iter().map(|folder| {
                    let f = folder.clone();
                    let name = d.project_name(&crate::desk::Place::new(d.pages.on, f.clone()));
                    Self::menu_item(desk, name, move |this, _, cx| {
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
    /// Where a webhook task is called: here, while u0 listens, and
    /// through the tunnel while one is up; a new address for it; whether a
    /// call must be signed, and by what secret; what a call must be.
    fn hook_addresses(
        &self,
        faces: &Faces,
        hook: &crate::tasks::Hook,
        cx: &mut Context<Self>,
    ) -> Div {
        let token = &hook.token;
        let shared = self.pages.shared.as_ref();
        let port = shared.and_then(|s| s.serving.clone()?.ok());
        let tunnel = shared.and_then(|s| match &s.tunnel {
            crate::share::Tunnel::Up(host) => Some(host.clone()),
            _ => None,
        });
        let mut at: Vec<String> = Vec::new();
        at.extend(port.map(|p| format!("http://127.0.0.1:{p}/hook/{token}")));
        at.extend(tunnel.map(|h| format!("https://{h}/hook/{token}")));
        let mut out = div().flex().flex_col().gap(px(6.));
        if at.is_empty() {
            out = out.child(ui::fine(
                faces,
                crate::tr_with(
                    "%{path}, where u0 listens: turn listening on in Devices for its address.",
                    &[("path", &format!("/hook/{token}"))],
                ),
            ));
        }
        for a in at {
            out = out.child(ui::fine_mono(faces, a));
        }
        let Some(draft) = self.tasks.draft.as_ref() else {
            return out;
        };
        let signed = hook.signed;
        let id = draft.task.id;
        let renew = ui::button("hook-renew", crate::tr("New address"), Weight::Quiet)
            .tooltip(crate::tr("The address it has now runs it no more"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if id != 0 {
                    this.call(this.pages.on, crate::proto::Call::RotateHook(id));
                }
                if let Some(Schedule::Hook(h)) =
                    this.tasks.draft.as_mut().map(|d| &mut d.task.schedule)
                {
                    h.token = crate::tasks::Hook::new().token;
                }
                cx.notify();
            }));
        let sign = super::srow::flip(
            "hook-signed",
            crate::tr("Signed"),
            signed,
            cx,
            move |this, _, cx| {
                if let Some(Schedule::Hook(h)) =
                    this.tasks.draft.as_mut().map(|d| &mut d.task.schedule)
                {
                    h.signed = !signed;
                }
                cx.notify();
            },
        )
        .hide_label(false);
        out = out.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(renew)
                .child(sign),
        );
        if signed {
            out = out
                .child(div().w(px(320.)).child(Input::new(&draft.secret).small()))
                .child(ui::fine(
                    faces,
                    crate::tr(
                        "HMAC-SHA256 of the body, in X-Hub-Signature-256 or X-Signature-256.",
                    ),
                ));
        }
        out.child(div().w(px(420.)).child(Input::new(&draft.only).small()))
            .child(ui::fine(
                faces,
                crate::tr("Only when body.<field> or header.<name> = a, b; separate rules with ;"),
            ))
            .child(ui::fine(
                faces,
                crate::tr("A POST runs it; {{body}} and {{header.<name>}} are untrusted. Lost if u0 is off."),
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
