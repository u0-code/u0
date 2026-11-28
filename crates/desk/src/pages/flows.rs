//! The Workflows workspace page (`Workflows` board): agent steps, commands
//! and questions, started by hand, on a schedule or by a webhook; run
//! history on the left, numbered step timeline on the right, and an inline
//! step editor (`WorkflowStepEditor`).

use std::{path::PathBuf, time::SystemTime};

use gpui::base::SelectableText;
use gpui::component::{input::Input, scroll::ScrollableElement as _};
use gpui::{Context, Div, IntoElement, SharedString, Window, div, prelude::*, px};
use u0ui::components::{
    Select,
    badge::{Badge, BadgeSize, BadgeVariant},
    list_box::ListItem,
};
use u0ui::prelude::*;
use u0ui::t3::{
    Spin,
    base::fmt_elapsed,
    workspace::{RunState, WorkflowRunRow},
};
use u0ui::u0::{
    fields::{InlineCheck, field_fill, line_input},
    tasks::WorkflowStepEditor,
};

use crate::{
    desk::{Desk, Place},
    proto::Call,
    store,
    tasks::{Schedule, Task},
    theme, when,
    workflows::{Run, State, Step, Workflow},
};

fn map_run_state(state: &State) -> RunState {
    match state {
        State::Going => RunState::Running,
        State::Asking => RunState::Waiting,
        State::Done => RunState::Succeeded,
        State::Paused(_) | State::Stopped(_) => RunState::Failed,
        State::Cancelled => RunState::Cancelled,
    }
}

fn run_state_badge(state: &State) -> Badge {
    match state {
        State::Going => Badge::new(crate::tr("Running"))
            .size(BadgeSize::Sm)
            .variant(BadgeVariant::Blue),
        State::Asking => {
            let (bg, fg) = if theme::is_light() {
                (gpui::rgb(0x00fc_f4d6).into(), gpui::rgb(0x0048_3700).into())
            } else {
                (gpui::rgb(0x0048_3700).into(), gpui::rgb(0x00f1_c21b).into())
            };
            Badge::new(crate::tr("Waiting"))
                .size(BadgeSize::Sm)
                .colors(bg, fg)
        }
        State::Done => Badge::new(crate::tr("Succeeded"))
            .size(BadgeSize::Sm)
            .variant(BadgeVariant::Green),
        State::Paused(_) | State::Stopped(_) => Badge::new(crate::tr("Failed"))
            .size(BadgeSize::Sm)
            .variant(BadgeVariant::Red),
        State::Cancelled => Badge::new(crate::tr("Cancelled"))
            .size(BadgeSize::Sm)
            .variant(BadgeVariant::Gray),
    }
}

impl Desk {
    /// Any scheduled/webhook task on `self.pages.on` that starts `workflow`.
    fn workflow_task(&self, workflow: &str) -> Option<&Task> {
        self.tasks_on(self.pages.on)?
            .as_ref()
            .ok()?
            .iter()
            .find(|t| t.workflow.as_deref() == Some(workflow))
    }

    /// Short trigger word for a workflow tab (`"webhook"`, `"weekdays 02:30"`, `"by hand"`).
    fn workflow_trigger_short(&self, workflow: &str) -> String {
        let Some(t) = self.workflow_task(workflow) else {
            return crate::tr("by hand").to_string();
        };
        match &t.schedule {
            Schedule::Hook(_) => crate::tr("webhook").to_string(),
            Schedule::Daily { minute, days } => {
                let at = when::clock(
                    when::at_minute(SystemTime::now(), 0, *minute),
                    self.settings.twelve,
                );
                if days.as_slice() == [1, 2, 3, 4, 5] {
                    crate::tr_with("weekdays %{at}", &[("at", &at)]).to_string()
                } else {
                    crate::tr_with("daily %{at}", &[("at", &at)]).to_string()
                }
            }
            Schedule::Every(m) => crate::tr_with("every %{m}m", &[("m", m)]).to_string(),
            Schedule::Once(_) => crate::tr("once").to_string(),
            Schedule::On { .. } => crate::tr("on event").to_string(),
        }
    }

    /// Top bar of the Workflows workspace page (`.r2-top`).
    pub(crate) fn workflows_topbar(
        &self,
        left: Div,
        right: Div,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let count = self.flows.list.iter().filter(|r| r.is_ok()).count();
        let next_at = (self.tasks_on(self.pages.on).and_then(|r| r.as_ref().ok())).and_then(|ts| {
            ts.iter()
                .filter(|t| !t.paused && t.workflow.is_some())
                .filter_map(|t| t.next)
                .min()
        });
        let now = SystemTime::now();
        let ctx_line = match next_at {
            Some(ts) => {
                let at = when::short_at(store::time(ts), now, self.settings.twelve);
                crate::tr_with(
                    "%{n} workflows · next run %{when}",
                    &[("n", &count), ("when", &at)],
                )
            }
            None => crate::tr_with("%{n} workflows", &[("n", &count)]),
        };
        let desk = cx.entity();
        let line = div()
            .type_style(TypeStyle::Label01)
            .text_color(theme::role(ColorRole::TextSecondary))
            .child(ctx_line);
        (
            left.child(line),
            right.child(
                Button::new("wf-new-btn", crate::tr("New workflow"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Add)
                    .on_click(move |_, window, cx| {
                        desk.update(cx, |this, cx| this.begin_edit_flow(None, window, cx));
                    }),
            ),
        )
    }

    /// What does not read: runs.json, said in a banner, and the workflow
    /// files, each with why and, on this Mac, Open; none, nothing.
    fn unreadable_flows(&self, cx: &mut Context<Self>) -> Option<Div> {
        let folder = crate::workflows::folder(self.home_on(self.pages.on));
        let here = self.pages.on == crate::hub::Host::HERE;
        let rows: Vec<Div> = self
            .flows
            .list
            .iter()
            .enumerate()
            .filter_map(|(n, f)| f.as_ref().err().map(|e| (n, e)))
            .map(|(n, (name, why))| {
                let file_name = if std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                {
                    name.clone()
                } else {
                    format!("{name}.json")
                };
                let path = folder.join(&file_name);
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(48.))
                    .px(px(16.))
                    .py(px(8.))
                    .bg(theme::role(ColorRole::Layer01))
                    .border_1()
                    .border_color(theme::role(ColorRole::SupportError))
                    .child(
                        Icon::carbon(CarbonIcon::ErrorFilled)
                            .size(IconSize::S16)
                            .color(theme::role(ColorRole::SupportError)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .type_style(TypeStyle::BodyCompact01)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(file_name),
                            )
                            .child(
                                div()
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::SupportError))
                                    .child(SelectableText::new(("flow-err-why", n), why.clone())),
                            ),
                    )
                    .when(here, |d| {
                        d.child(
                            Button::new(
                                SharedString::from(format!("flow-open-err-{n}")),
                                crate::tr("Open"),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Edit)
                            .on_click(cx.listener(
                                move |this, _, _, _| {
                                    this.edit(path.clone(), None);
                                },
                            )),
                        )
                    })
            })
            .collect();
        let runs_bad = self.flows.runs.as_ref().err().map(|why| {
            crate::desk::banners::banner(
                "runs-bad-file",
                u0ui::components::alert::AlertVariant::Error,
                crate::tr("runs.json does not read"),
                self.tidy_why(why),
                None,
                None,
            )
        });
        (runs_bad.is_some() || !rows.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(runs_bad)
                .children(rows)
        })
    }

    /// Full Workflows workspace page (`Workflows` board).
    pub(crate) fn workflows_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let valid: Vec<&Workflow> = self.flows.list.iter().flatten().collect();
        let header = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .type_style(TypeStyle::Heading04)
                    .text_color(theme::role(ColorRole::TextPrimary))
                    .child(crate::tr("Workflows")),
            )
            .child(
                div()
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr(
                        "Agent steps, commands and questions, run by hand, schedule or webhook.",
                    )),
            );

        if let Some(draft) = &self.flows.editing {
            return div()
                .id("workflows-workspace")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .max_w(px(1080.))
                        .pt(px(28.))
                        .pb(px(32.))
                        .px(px(40.))
                        .child(header)
                        .child(self.workflow_editor(draft, window, cx)),
                );
        }

        if valid.is_empty() {
            use u0ui::components::empty::{
                Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
            };
            let path = crate::workflows::folder(self.home_on(self.pages.on));
            let folder = Desk::shown(&path);
            let here = self.pages.on == crate::hub::Host::HERE;
            return div()
                .id("workflows-workspace")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .w_full()
                        .max_w(px(1080.))
                        .pt(px(28.))
                        .pb(px(32.))
                        .px(px(40.))
                        .child(header)
                        .children(self.unreadable_flows(cx))
                        .child(
                            Empty::new()
                                .child(
                                    EmptyHeader::new()
                                        .child(EmptyTitle::new(crate::tr("No workflows yet")))
                                        .child(EmptyDescription::new(crate::tr_with(
                                            "A workflow is a JSON file of yours in %{folder}: steps an agent, a command or a question. It is read as it is listed or started.",
                                            &[("folder", &folder)],
                                        ))),
                                )
                                // The folder is revealed where it is on this
                                // Mac; before the first workflow is saved it
                                // is not there yet, and its home is shown.
                                .when(here, |e| {
                                    e.child(
                                        EmptyContent::new().child(
                                            Button::new(
                                                "wf-empty-folder",
                                                crate::tr("Open the folder"),
                                            )
                                            .variant(ButtonVariant::Secondary)
                                            .size(ButtonSize::Sm)
                                            .icon(CarbonIcon::FolderOpen)
                                            .on_click(move |_, _, cx| {
                                                let shown = Some(path.as_path())
                                                    .filter(|p| p.is_dir())
                                                    .or(path.parent())
                                                    .unwrap_or(&path);
                                                cx.reveal_path(shown);
                                            }),
                                        ),
                                    )
                                }),
                        ),
                );
        }

        let chosen = self
            .flows
            .selected
            .as_deref()
            .and_then(|name| valid.iter().copied().find(|f| f.name == name))
            .or_else(|| {
                let latest = self.flows.runs.as_ref().ok()?.last()?;
                valid.iter().copied().find(|f| f.name == latest.workflow)
            })
            .unwrap_or(valid[0]);

        // Workflow tabs (`.wf-tabs`, 36px, two-line tab).
        let tabs = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mt(px(24.))
            .h(px(36.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .children(valid.iter().enumerate().map(|(i, f)| {
                let on = f.name == chosen.name;
                let name = f.name.clone();
                let trigger = self.workflow_trigger_short(&f.name);
                let sub = steps_line(f.steps.len(), &trigger);
                let desk_tab = cx.entity();
                gpui::base::Button::new(SharedString::from(format!("wf-tab-{i}")))
                    .accessibility_label(f.name.clone())
                    .relative()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .h_full()
                    .pr(px(16.))
                    .mr(px(16.))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let picked = name.clone();
                        desk_tab.update(cx, |this, cx| {
                            this.flows.selected = Some(picked);
                            this.flows.selected_run = None;
                            cx.notify();
                        });
                    })
                    .child(
                        div()
                            .type_style(if on {
                                TypeStyle::HeadingCompact01
                            } else {
                                TypeStyle::BodyCompact01
                            })
                            .line_height(px(18.))
                            .text_color(theme::role(if on {
                                ColorRole::TextPrimary
                            } else {
                                ColorRole::TextSecondary
                            }))
                            .child(f.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(14.))
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(sub),
                    )
                    .when(on, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(px(-1.))
                                .h(px(2.))
                                .bg(theme::role(ColorRole::BorderInteractive)),
                        )
                    })
            }));

        // Metadata + action row (`.wf-metarow`).
        let task = self.workflow_task(&chosen.name).cloned();
        let meta_trigger = match &task {
            Some(t) => match &t.schedule {
                Schedule::Hook(h) if h.signed => {
                    crate::tr("Webhook, HMAC-signed · payload is data, not instructions")
                }
                Schedule::Hook(_) => crate::tr("Webhook · payload is data, not instructions"),
                _ => crate::tr("Scheduled · runs unattended"),
            },
            None if !chosen.about.is_empty() => SharedString::from(chosen.about.clone()),
            None => crate::tr("Started by hand"),
        };
        let proj_name = chosen.project.as_ref().map_or_else(
            || crate::tr("All projects").to_string(),
            |p| self.project_name(&Place::new(self.pages.on, p.clone())),
        );
        let wt_note = if chosen.worktree {
            crate::tr("worktree from main")
        } else {
            crate::tr("local checkout")
        };
        let machine_name = self
            .machine(self.pages.on)
            .map_or_else(|| "This Mac".to_owned(), |m| m.name.clone());
        let meta_where = format!("{proj_name} · {wt_note} · {machine_name}");

        let has_trigger = task.is_some();
        let flow_clone = chosen.clone();
        let flow_run_name = chosen.name.clone();
        let desk_edit = cx.entity();
        let desk_run = cx.entity();
        let mut metarow = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mt(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .overflow_hidden()
                    .gap(px(24.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(div().truncate().child(meta_trigger))
                    .child(
                        div()
                            .flex_none()
                            .w(px(1.))
                            .h(px(12.))
                            .bg(theme::role(ColorRole::BorderSubtle01)),
                    )
                    .child(div().flex_none().whitespace_nowrap().child(meta_where)),
            )
            .child(div().flex_1());

        if let Some(t) = task {
            let host = self.pages.on;
            let desk_pause = cx.entity();
            let tip = if t.paused {
                crate::tr("Resume trigger")
            } else {
                crate::tr("Pause trigger")
            };
            let icon = if t.paused {
                CarbonIcon::Play
            } else {
                CarbonIcon::Pause
            };
            metarow = metarow.child(
                Button::new("wf-pause-trigger", tip)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon_only(true)
                    .icon(icon)
                    .on_click(move |_, _, cx| {
                        let mut next = t.clone();
                        next.paused = !next.paused;
                        desk_pause.update(cx, |this, cx| {
                            this.call(host, Call::SetTask(next));
                            cx.notify();
                        });
                    }),
            );
        }

        if !has_trigger && let Some(inp) = &self.flows.input {
            metarow = metarow.child(
                div()
                    .w(px(160.))
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .bg(theme::role(ColorRole::Field01))
                    .border_b_1()
                    .border_color(theme::role(ColorRole::BorderStrong01))
                    .child(Input::new(inp).appearance(false)),
            );
        }

        metarow = metarow
            .child(
                Button::new("wf-edit-btn", crate::tr("Edit"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Edit)
                    .on_click(move |_, window, cx| {
                        let f = flow_clone.clone();
                        desk_edit.update(cx, |this, cx| {
                            this.begin_edit_flow(Some(&f), window, cx);
                        });
                    }),
            )
            .child(
                Button::new("wf-run-now-btn", crate::tr("Run now"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Play)
                    .on_click(move |_, window, cx| {
                        let n = flow_run_name.clone();
                        desk_run.update(cx, |this, cx| this.start_flow(n, window, cx));
                    }),
            );

        // Two-column Runs + Run Detail.
        let now = SystemTime::now();
        let now_secs = store::secs(now);
        let flow_runs: Vec<&Run> = self
            .flows
            .runs
            .as_ref()
            .map(|all| {
                all.iter()
                    .rev()
                    .filter(|r| r.workflow == chosen.name)
                    .collect()
            })
            .unwrap_or_default();

        let active_run = self
            .flows
            .selected_run
            .and_then(|id| flow_runs.iter().copied().find(|r| r.id == id))
            .or_else(|| flow_runs.first().copied());

        let mut runs_col = div().flex().flex_col().flex_none().w(px(288.)).child(
            div()
                .flex()
                .items_center()
                .h(px(32.))
                .pl(px(12.))
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(crate::tr("Runs")),
        );

        if flow_runs.is_empty() {
            runs_col = runs_col.child(
                div()
                    .pl(px(12.))
                    .py(px(8.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextHelper))
                    .child(crate::tr("No runs yet")),
            );
        } else {
            for r in &flow_runs {
                let rid = r.id;
                let selected = active_run.is_some_and(|a| a.id == rid);
                let elapsed = r.ended.unwrap_or(now_secs).saturating_sub(r.began);
                let proj_or_input = if r.input.is_empty() {
                    proj_name.clone()
                } else {
                    r.input.clone()
                };
                let trig = if r.trigger.is_empty() {
                    crate::tr("By hand").to_string()
                } else {
                    r.trigger.clone()
                };
                let started = started_said(r.began, now);
                let desk_r = cx.entity();
                runs_col = runs_col.child(
                    WorkflowRunRow::new(
                        SharedString::from(format!("wf-run-{rid}")),
                        crate::tr_with("Run #%{id}", &[("id", &rid)]),
                        map_run_state(&r.state),
                    )
                    .selected(selected)
                    .show_icon(false)
                    .project(proj_or_input)
                    .trigger(trig)
                    .started(started)
                    .elapsed(elapsed)
                    .on_click(move |_, cx| {
                        desk_r.update(cx, |this, cx| {
                            this.flows.selected_run = Some(rid);
                            cx.notify();
                        });
                    }),
                );
            }
        }

        let detail_col = match active_run {
            Some(run) => self.workflow_run_detail(chosen, run, now, cx),
            None => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .pl(px(32.))
                .border_l_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    div()
                        .py(px(12.))
                        .type_style(TypeStyle::Body01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(crate::tr("No runs yet")),
                ),
        };

        div()
            .id("workflows-workspace")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(px(1080.))
                    .pt(px(28.))
                    .pb(px(32.))
                    .px(px(40.))
                    .child(header)
                    .children(self.unreadable_flows(cx).map(|d| d.mt(px(16.))))
                    .child(tabs)
                    .child(metarow)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(32.))
                            .mt(px(24.))
                            .child(runs_col)
                            .child(detail_col),
                    ),
            )
    }

    /// Right-column detail timeline for a single workflow `run` (`.wf-detail`).
    fn workflow_run_detail(
        &self,
        flow: &Workflow,
        run: &Run,
        now: SystemTime,
        cx: &mut Context<Self>,
    ) -> Div {
        // Determine which step index failed (either recorded with non-zero exit_code at the end of `done`, or paused at `done.len()`).
        let last_done_failed = matches!(run.state, State::Paused(_))
            && run
                .done
                .last()
                .is_some_and(|d| d.exit_code.is_some_and(|c| c != 0));
        let failed_idx = match &run.state {
            State::Paused(_) if last_done_failed => Some(run.done.len().saturating_sub(1)),
            State::Paused(_) => Some(run.done.len()),
            _ => None,
        };

        let sub_line = started_line(run, failed_idx, now);

        let total_steps = flow.steps.len().max(run.done.len());
        let show_full_log = self.flows.full_log == Some(run.id);
        let mut items: Vec<gpui::AnyElement> = Vec::new();

        for idx in 0..total_steps {
            let is_last = idx + 1 == total_steps;
            let step_def = flow.steps.get(idx);
            let done = run.done.get(idx);
            let step_name = step_def
                .map(|s| s.name().to_owned())
                .or_else(|| done.map(|d| d.name.clone()))
                .unwrap_or_else(|| format!("Step {}", idx + 1));
            let numbered = format!("{}. {step_name}", idx + 1);

            let is_failed = failed_idx == Some(idx);
            let is_succeeded = !is_failed && done.is_some();
            let is_asking = matches!(run.state, State::Asking) && idx == run.done.len();
            let is_running = matches!(run.state, State::Going) && idx == run.done.len();

            let mark: gpui::AnyElement = if is_running {
                Spin::new(SharedString::from(format!("wf-spin-{}-{idx}", run.id)))
                    .into_any_element()
            } else if is_succeeded {
                Icon::carbon(CarbonIcon::CheckmarkFilled)
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::SupportSuccess))
                    .into_any_element()
            } else if is_failed {
                Icon::carbon(CarbonIcon::ErrorFilled)
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::SupportError))
                    .into_any_element()
            } else if is_asking || matches!(step_def, Some(Step::Ask { .. })) {
                Icon::carbon(CarbonIcon::Help)
                    .size_length(px(16.))
                    .color(theme::role(if is_asking {
                        ColorRole::SupportWarning
                    } else {
                        ColorRole::IconSecondary
                    }))
                    .into_any_element()
            } else {
                Icon::carbon(CarbonIcon::Locked)
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::IconSecondary))
                    .into_any_element()
            };

            let pending = !is_succeeded && !is_failed && !is_asking && !is_running;
            let mut right_meta = div()
                .flex()
                .items_baseline()
                .gap(px(6.))
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(if pending {
                    ColorRole::TextHelper
                } else {
                    ColorRole::TextSecondary
                }));

            if let Some(d) = done {
                right_meta = right_meta.child(fmt_elapsed(d.secs));
                if is_failed && let Some(code) = d.exit_code.filter(|c| *c != 0) {
                    right_meta = right_meta.child("·").child(
                        div()
                            .text_color(theme::role(ColorRole::TextError))
                            .child(crate::tr_with("exit %{code}", &[("code", &code)])),
                    );
                }
            } else if pending {
                right_meta = right_meta.child(crate::tr("not reached"));
            }

            let mut body = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(if is_failed { 12. } else { 8. }))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .type_style(if pending {
                                    TypeStyle::Body01
                                } else {
                                    TypeStyle::HeadingCompact01
                                })
                                .text_color(theme::role(if pending {
                                    ColorRole::TextSecondary
                                } else {
                                    ColorRole::TextPrimary
                                }))
                                .child(numbered),
                        )
                        .child(right_meta),
                );

            // Step detail line for succeeded / running steps.
            if is_succeeded {
                if let Some(Step::Run { command, .. }) = step_def {
                    body = body.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap(px(6.))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(format!("{} ·", crate::tr("Command")))
                            .child(
                                div()
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(command.clone()),
                            ),
                    );
                } else if let Some(d) = done {
                    let sess_link = d.session.as_ref().and_then(|(agent, sid)| {
                        let s =
                            self.sessions.all.iter().find(|s| {
                                &s.meta.agent == agent && s.meta.id.as_ref() == Some(sid)
                            })?;
                        Some((s.key, s.meta.title.clone()))
                    });
                    let mode_label = match step_def {
                        Some(Step::Agent { mode: Some(m), .. }) => Some(m.clone()),
                        _ => None,
                    };
                    let mut detail_row = div()
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap(px(6.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary));
                    if let Some(m) = mode_label {
                        detail_row = detail_row.child(format!("{m} ·"));
                    }
                    if let Some((key, title)) = sess_link {
                        let desk_open = cx.entity();
                        detail_row = detail_row.child(
                            div()
                                .id(SharedString::from(format!("wf-open-sess-{}-{idx}", run.id)))
                                .cursor_pointer()
                                .text_color(theme::role(ColorRole::LinkPrimary))
                                .hover(|s| s.text_color(theme::role(ColorRole::LinkPrimaryHover)))
                                .child(crate::tr_with("thread “%{title}”", &[("title", &title)]))
                                .on_click(move |_, window, cx| {
                                    desk_open.update(cx, |this, cx| this.show(key, window, cx));
                                }),
                        );
                    }
                    let summary = d.result.lines().next().unwrap_or_default().trim();
                    if !summary.is_empty() {
                        detail_row = detail_row.child(format!("· {summary}"));
                    }
                    body = body.child(detail_row);
                }
            }

            // Failed step log box + actions (`Fix in a thread`, `Re-run from step N`, `Full log`).
            if is_failed {
                let err_text = match (&run.state, done) {
                    (_, Some(d)) if !d.result.trim().is_empty() => d.result.clone(),
                    (State::Paused(why), _) => why.clone(),
                    _ => String::new(),
                };
                let max_lines = if show_full_log { 40 } else { 4 };
                let log_lines: Vec<String> = err_text
                    .lines()
                    .take(max_lines)
                    .map(str::to_owned)
                    .collect();
                if !log_lines.is_empty() {
                    body = body.child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .py(px(10.))
                            .px(px(12.))
                            .bg(theme::role(ColorRole::Layer01))
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left_0()
                                    .w(px(2.))
                                    .bg(theme::role(ColorRole::SupportError)),
                            )
                            .child({
                                // Its first line in `text-error`, the rest
                                // `text-primary`; selectable as one.
                                let first = log_lines[0].len();
                                div()
                                    .type_style(TypeStyle::Code01)
                                    .line_height(px(18.))
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(
                                        SelectableText::new(
                                            SharedString::from(format!(
                                                "wf-log-text-{}-{idx}",
                                                run.id
                                            )),
                                            log_lines.join("\n"),
                                        )
                                        .highlights([(
                                            0..first,
                                            gpui::HighlightStyle {
                                                color: Some(theme::role(ColorRole::TextError)),
                                                ..Default::default()
                                            },
                                        )]),
                                    )
                            }),
                    );
                }

                let run_clone = run.clone();
                let title_clone = step_name.clone();
                let err_clone = err_text.clone();
                let rid = run.id;
                let desk_fix = cx.entity();
                let desk_rerun = cx.entity();
                let desk_log = cx.entity();
                let copied = err_text.clone();
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Button::new(
                                SharedString::from(format!("wf-fix-{rid}-{idx}")),
                                crate::tr("Fix in a thread"),
                            )
                            .size(ButtonSize::Sm)
                            .on_click(move |_, window, cx| {
                                let r = run_clone.clone();
                                let t = title_clone.clone();
                                let e = err_clone.clone();
                                desk_fix.update(cx, |this, cx| {
                                    this.fix_flow_in_thread(&r, &t, &e, window, cx);
                                });
                            }),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("wf-rerun-{rid}-{idx}")),
                                crate::tr_with("Re-run from step %{n}", &[("n", &(idx + 1))]),
                            )
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_rerun
                                    .update(cx, |this, cx| this.rerun_flow_from(rid, idx, cx));
                            }),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("wf-log-{rid}-{idx}")),
                                crate::tr(if show_full_log {
                                    "Hide log"
                                } else {
                                    "Full log"
                                }),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_log.update(cx, |this, cx| {
                                    this.flows.full_log =
                                        if show_full_log { None } else { Some(rid) };
                                    cx.notify();
                                });
                            }),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("wf-copy-{rid}-{idx}")),
                                crate::tr("Copy"),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Copy)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    copied.clone(),
                                ));
                            }),
                        ),
                );
                if run.worktree.is_some() {
                    body = body.child(
                        div()
                            .mt(px(-4.))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(crate::tr_with(
                                "Fix it in this run’s worktree, then Re-run from step %{n}.",
                                &[("n", &(idx + 1))],
                            )),
                    );
                }
            }

            // Waiting `Ask` step choices.
            if is_asking && let Some(Step::Ask { title, choices, .. }) = step_def {
                let rid = run.id;
                body = body
                    .child(
                        div()
                            .type_style(TypeStyle::Body01)
                            .text_color(theme::role(ColorRole::TextPrimary))
                            .child(run.fill(title)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .children(choices.iter().enumerate().map(|(ci, choice)| {
                                let c = choice.clone();
                                let desk_ans = cx.entity();
                                Button::new(
                                    SharedString::from(format!("wf-ans-{rid}-{ci}")),
                                    choice.clone(),
                                )
                                .variant(if ci == 0 {
                                    ButtonVariant::Default
                                } else {
                                    ButtonVariant::Secondary
                                })
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    let chosen_ans = c.clone();
                                    desk_ans.update(cx, |this, _| {
                                        this.answer_flow(rid, chosen_ans);
                                    });
                                })
                            })),
                    );
            }

            if is_running {
                let rid = run.id;
                let desk_cancel = cx.entity();
                body = body.child(
                    div().flex().child(
                        Button::new(
                            SharedString::from(format!("wf-cancel-{rid}")),
                            crate::tr("Cancel"),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk_cancel.update(cx, |this, _| this.cancel_flow(rid));
                        }),
                    ),
                );
            }

            items.push(
                div()
                    .relative()
                    .flex()
                    .gap(px(12.))
                    .pb(px(if is_last {
                        0.
                    } else if is_failed {
                        28.
                    } else {
                        20.
                    }))
                    .when(!is_last, |li| {
                        li.child(
                            div()
                                .absolute()
                                .left(px(7.5))
                                .top(px(22.))
                                .bottom_0()
                                .w(px(1.))
                                .bg(theme::role(ColorRole::BorderSubtle01)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .w(px(16.))
                            .h(px(20.))
                            .child(mark),
                    )
                    .child(body)
                    .into_any_element(),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(20.))
            .pl(px(32.))
            .border_l_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .type_style(TypeStyle::Heading02)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(crate::tr_with("Run #%{id}", &[("id", &run.id)])),
                            )
                            .child(run_state_badge(&run.state)),
                    )
                    .child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(sub_line),
                    ),
            )
            .child(div().flex().flex_col().children(items))
    }

    /// Inline workflow editor using `u0ui::u0::tasks::WorkflowStepEditor`.
    fn workflow_editor(
        &self,
        draft: &crate::desk::flows::FlowDraft,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let agents: Vec<SharedString> = {
            let mut list: Vec<SharedString> = self
                .slots_on(self.pages.on)
                .map(|s| SharedString::from(s.entry.id.clone()))
                .collect();
            if list.is_empty() {
                list.push("claude-acp".into());
            }
            list
        };
        let mut modes: Vec<SharedString> = vec!["Default".into()];
        for s in self.slots_on(self.pages.on) {
            if let Some(st) = s.probe.as_ref().and_then(|p| p.modes.as_ref()) {
                for m in &st.available_modes {
                    let mid = SharedString::from(m.id.0.to_string());
                    if !modes.contains(&mid) {
                        modes.push(mid);
                    }
                }
            }
        }
        if modes.len() == 1 {
            modes.extend(["plan".into(), "acceptEdits".into()]);
        }

        let mut proj_items: Vec<ListItem> = Vec::new();
        for f in &self.edited().folders {
            proj_items.push(ListItem::labeled(
                f.to_string_lossy().into_owned(),
                self.project_name(&Place::new(self.pages.on, f.clone())),
            ));
        }
        let proj_val = draft
            .project
            .as_ref()
            .map(|p| SharedString::from(p.to_string_lossy().into_owned()));
        let desk_proj = cx.entity();
        let proj_select = Select::new("wf-ed-project")
            .layer(LayerLevel::Two)
            .items(proj_items)
            .value(proj_val)
            .on_value_change(move |val, _, cx| {
                let picked = val.map(|v| PathBuf::from(v.as_ref()));
                desk_proj.update(cx, |this, cx| {
                    if let Some(d) = &mut this.flows.editing {
                        d.project = picked;
                    }
                    cx.notify();
                });
            });

        let desk_wt = cx.entity();
        let wt_check = InlineCheck::new("wf-ed-worktree")
            .checked(draft.worktree)
            .label(crate::tr("Run each run in an isolated worktree"))
            .on_change(move |checked, _, cx| {
                desk_wt.update(cx, |this, cx| {
                    if let Some(d) = &mut this.flows.editing {
                        d.worktree = checked;
                    }
                    cx.notify();
                });
            });

        let desk_steps = cx.entity();
        let step_editor =
            WorkflowStepEditor::new(SharedString::from(format!("wf-step-ed-{}", draft.seq)))
                .steps(draft.steps.clone())
                .agents(agents)
                .modes(modes)
                .on_change(move |steps, _, cx| {
                    desk_steps.update(cx, |this, _| {
                        if let Some(d) = &mut this.flows.editing {
                            d.steps = steps;
                        }
                    });
                });

        let desk_save = cx.entity();
        let desk_cancel = cx.entity();
        let delete_orig = draft.original.clone();
        let desk_del = cx.entity();

        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .mt(px(24.))
            .p(px(24.))
            .bg(theme::role(ColorRole::Layer01))
            .when_some(draft.error.clone(), |d, err| {
                d.child(
                    div()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextError))
                        .child(SelectableText::new("flow-draft-error", err)),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .w(px(240.))
                            .child(
                                div()
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(crate::tr("Workflow name")),
                            )
                            .child(line_input(
                                &draft.name,
                                true,
                                field_fill(true),
                                Some("release-check".into()),
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(crate::tr("Description")),
                            )
                            .child(line_input(
                                &draft.about,
                                false,
                                field_fill(true),
                                Some(crate::tr("What this workflow does")),
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .w(px(220.))
                            .child(
                                div()
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(crate::tr("Project")),
                            )
                            .child(proj_select),
                    ),
            )
            .child(wt_check)
            .child(step_editor)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(8.))
                    .child(
                        Button::new("wf-ed-save", crate::tr("Save workflow"))
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_save.update(cx, Desk::save_edited_flow);
                            }),
                    )
                    .child(
                        Button::new("wf-ed-cancel", crate::tr("Cancel"))
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_cancel.update(cx, |this, cx| {
                                    this.flows.editing = None;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(div().flex_1())
                    .when_some(delete_orig, |d, orig| {
                        d.child(
                            Button::new("wf-ed-delete", crate::tr("Delete workflow"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::TrashCan)
                                .on_click(move |_, _, cx| {
                                    let name = orig.clone();
                                    desk_del.update(cx, |this, cx| this.delete_flow(&name, cx));
                                }),
                        )
                    }),
            )
    }
}

/// A run's line under its name: when it began, what began it, and where it
/// is now. What began it is the server's word ("By hand", "Webhook"), set
/// apart, so none reads "by by hand".
fn started_line(run: &Run, failed: Option<usize>, now: SystemTime) -> SharedString {
    let elapsed = fmt_elapsed(
        run.ended
            .unwrap_or_else(|| store::secs(now))
            .saturating_sub(run.began),
    );
    let when = started_said(run.began, now);
    let trigger = if run.trigger.is_empty() {
        crate::tr("by hand").to_string()
    } else {
        run.trigger.to_lowercase()
    };
    let said: [(&str, &dyn std::fmt::Display); 3] = [
        ("when", &when),
        ("trigger", &trigger),
        ("elapsed", &elapsed),
    ];
    match (&run.state, failed) {
        (State::Paused(_), Some(idx)) => crate::tr_with(
            "Started %{when} · %{trigger} · stopped at step %{step} after %{elapsed}",
            &[said[0], said[1], said[2], ("step", &(idx + 1))],
        ),
        (State::Done, _) => crate::tr_with(
            "Started %{when} · %{trigger} · completed in %{elapsed}",
            &said,
        ),
        _ => crate::tr_with(
            "Started %{when} · %{trigger} · running for %{elapsed}",
            &said,
        ),
    }
}

/// When a run began, as its row and its line say: "now", "27m ago", or
/// the day ("Fri") once it is older.
fn started_said(began: u64, now: SystemTime) -> String {
    match when::ago(store::time(began), now).as_str() {
        "now" => crate::tr("now").to_string(),
        s if s.ends_with('m') || s.ends_with('h') => {
            crate::tr_with("%{ago} ago", &[("ago", &s)]).to_string()
        }
        other => other.to_owned(),
    }
}

/// A workflow's tab line: how many steps, and what starts it.
fn steps_line(steps: usize, trigger: &str) -> SharedString {
    if steps == 1 {
        crate::tr_with("1 step · %{trigger}", &[("trigger", &trigger)])
    } else {
        crate::tr_with(
            "%{n} steps · %{trigger}",
            &[("n", &steps), ("trigger", &trigger)],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_started_by_hand_says_by_once() {
        let now = SystemTime::now();
        let run = Run {
            id: 1,
            workflow: "release".into(),
            input: String::new(),
            trigger: "By hand".into(),
            began: store::secs(now) - 120,
            ended: Some(store::secs(now) - 60),
            state: State::Done,
            done: Vec::new(),
            turn: None,
            worktree: None,
        };
        let line = started_line(&run, None, now);
        assert!(line.starts_with("Started 2m ago · by hand"), "{line}");
        assert!(!line.contains("by by"), "{line}");
    }

    #[test]
    fn a_workflow_of_one_step_says_step_not_steps() {
        assert_eq!(steps_line(1, "by hand").as_ref(), "1 step · by hand");
        assert_eq!(steps_line(3, "webhook").as_ref(), "3 steps · webhook");
    }
}
