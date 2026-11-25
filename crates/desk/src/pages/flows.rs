//! Settings › Workflows, as t3code's Workflows page: the workflows your
//! files define, each begun from here with an input; and their runs,
//! filtered to those that need you or are going, each with its steps'
//! results, its question to answer, and resume or cancel.

use std::time::SystemTime;

use gpui::component::{Sizable as _, input::Input, tag::Tag};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{
    card,
    prefs::{page, said},
};
use crate::{
    desk::{Desk, flows::Showing},
    fonts::Faces,
    store, theme,
    ui::{self, Weight},
    when,
    workflows::{Run, State, Step},
};

impl Desk {
    /// The Workflows tab; none where the search `q` finds nothing in it.
    pub(super) fn flows_tab(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> Option<Div> {
        if !said(q, &["Workflows", "workflow", "runs", "steps"]) {
            return None;
        }
        let folder = crate::workflows::folder(&self.home);
        let mut defs = card().overflow_hidden();
        if self.flows.list.is_empty() {
            defs = defs.child(div().px(px(16.)).py(px(14.)).child(ui::fine(
                faces,
                format!(
                    "No workflows yet. Each is a JSON file in {}: its steps say a prompt to an agent, run a command, or ask you.",
                    Desk::shown(&folder)
                ),
            )));
        }
        for (n, flow) in self.flows.list.iter().enumerate() {
            let row = div()
                .flex()
                .items_center()
                .gap(px(16.))
                .px(px(16.))
                .py(px(12.))
                .when(n > 0, |d| d.border_t_1().border_color(theme::border()));
            defs = defs.child(match flow {
                Err((name, why)) => row.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(ui::text(faces, name.clone()).text_size(px(13.5)))
                        .child(
                            ui::text(faces, why.clone())
                                .text_size(px(12.5))
                                .text_color(theme::coral()),
                        ),
                ),
                Ok(flow) => {
                    let steps: Vec<&str> = flow.steps.iter().map(Step::name).collect();
                    let name = flow.name.clone();
                    row.child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(2.))
                            .child(ui::text(faces, flow.name.clone()).text_size(px(13.5)))
                            .when(!flow.about.is_empty(), |d| {
                                d.child(
                                    ui::text(faces, flow.about.clone())
                                        .text_size(px(12.5))
                                        .text_color(theme::text2()),
                                )
                            })
                            .child(ui::fine(faces, steps.join(" → "))),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("flow-start-{n}")),
                            "Start",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.start_flow(name.clone(), window, cx);
                            },
                        )),
                    )
                }
            });
        }
        let showing = self.flows.showing;
        let show = |label: &'static str,
                    which: Showing,
                    cx: &mut Context<Self>|
         -> (&'static str, bool, ui::Pick) {
            (
                label,
                showing == which,
                Box::new(cx.listener(move |this, _, _, cx| {
                    this.flows.showing = which;
                    cx.notify();
                })),
            )
        };
        let filter = ui::segmented(
            "flow-showing",
            vec![
                show("Needs you", Showing::NeedsYou, cx),
                show("Going", Showing::Going, cx),
                show("All", Showing::All, cx),
            ],
        );
        let mut runs = card().overflow_hidden();
        match &self.flows.runs {
            Err(why) => runs = runs.child(div().p(px(12.)).child(ui::trouble(why.clone()))),
            Ok(all) => {
                let shown: Vec<&Run> = (all.iter().rev())
                    .filter(|r| match showing {
                        Showing::NeedsYou => matches!(r.state, State::Asking | State::Paused(_)),
                        Showing::Going => r.state == State::Going,
                        Showing::All => true,
                    })
                    .collect();
                if shown.is_empty() {
                    runs = runs.child(
                        div()
                            .px(px(16.))
                            .py(px(14.))
                            .child(ui::fine(faces, "No runs here.")),
                    );
                }
                for (n, run) in shown.into_iter().enumerate() {
                    runs = runs.child(
                        self.flow_run_row(faces, run, cx)
                            .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
                    );
                }
            }
        }
        Some(
            page(
                faces,
                super::Tab::Workflows.title(),
                "Work in stages, run by Ghost's server as one run that outlives a restart: each step's result is kept as it ends, and a step that fails pauses the run there.",
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(ui::label(faces, "Workflows").flex_1())
                            .children(self.flows.input.as_ref().map(|i| div().w(px(240.)).child(Input::new(i).small()))),
                    )
                    .child(defs),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(ui::label(faces, "Runs").flex_1())
                            .child(filter),
                    )
                    .child(runs),
            ),
        )
    }

    /// A run: its workflow and input, where it stands, each step done with
    /// its result, and what you can do with it.
    fn flow_run_row(&self, faces: &Faces, run: &Run, cx: &mut Context<Self>) -> Div {
        let id = run.id;
        let tag = match &run.state {
            State::Going => Tag::info().small().child("Going"),
            State::Asking => Tag::warning().small().child("Needs you"),
            State::Paused(_) => Tag::danger().small().child("Paused"),
            State::Done => Tag::success().small().child("Done"),
            State::Stopped(_) => Tag::secondary().small().child("Stopped"),
            State::Cancelled => Tag::secondary().small().child("Cancelled"),
        };
        let began = when::said_at(
            store::time(run.began),
            SystemTime::now(),
            self.settings.twelve,
        );
        let flow = (self.flows.list.iter().flatten()).find(|f| f.name == run.workflow);
        let mut steps = div().flex().flex_col().gap(px(4.)).pt(px(6.));
        for (n, done) in run.done.iter().enumerate() {
            let session = done.session.as_ref().and_then(|(agent, sid)| {
                let s = (self.sessions.all.iter())
                    .find(|s| &s.meta.agent == agent && s.meta.id.as_ref() == Some(sid))?;
                Some(s.key)
            });
            steps = steps.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon("check", 12.).text_color(theme::ok()))
                    .child(
                        ui::text(faces, done.name.clone())
                            .text_size(px(12.5))
                            .flex_none(),
                    )
                    .child(
                        ui::one_line(
                            ui::text(
                                faces,
                                done.result
                                    .trim()
                                    .lines()
                                    .next()
                                    .unwrap_or_default()
                                    .to_owned(),
                            )
                            .text_size(px(12.5))
                            .text_color(theme::faint()),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .children(session.map(|key| {
                        ui::opener(SharedString::from(format!("run-{id}-session-{n}")))
                            .xsmall()
                            .label("Open")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_settings(window, cx);
                                this.show(key, window, cx);
                            }))
                    })),
            );
        }
        let at = flow.and_then(|f| f.steps.get(run.done.len()));
        let mut actions = div().flex().flex_wrap().items_center().gap(px(6.));
        match (&run.state, at) {
            (State::Asking, Some(Step::Ask { title, choices, .. })) => {
                steps = steps.child(
                    ui::text(faces, run.fill(title))
                        .text_size(px(13.))
                        .pt(px(4.)),
                );
                for (n, choice) in choices.iter().enumerate() {
                    let c = choice.clone();
                    actions = actions.child(
                        ui::button(
                            SharedString::from(format!("run-{id}-choice-{n}")),
                            choice.clone(),
                            Weight::Secondary,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, _| this.answer_flow(id, c.clone())),
                        ),
                    );
                }
            }
            (State::Paused(why), _) => {
                steps = steps.child(
                    ui::text(faces, why.clone())
                        .text_size(px(12.5))
                        .text_color(theme::coral()),
                );
                actions = actions.child(
                    ui::button(
                        SharedString::from(format!("run-{id}-resume")),
                        "Resume",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, _| this.resume_flow(id))),
                );
            }
            (State::Going, Some(step)) => {
                steps = steps.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::live_dot())
                        .child(ui::text(faces, step.name().to_owned()).text_size(px(12.5))),
                );
            }
            (State::Stopped(why), _) => steps = steps.child(ui::fine(faces, why.clone())),
            _ => {}
        }
        if !run.over() {
            actions = actions.child(
                ui::button(
                    SharedString::from(format!("run-{id}-cancel")),
                    "Cancel",
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, _, _| this.cancel_flow(id))),
            );
        }
        let total = flow.map_or(0, |f| f.steps.len());
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(16.))
            .py(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::text(faces, run.workflow.clone()).text_size(px(13.5)))
                    .when(!run.input.is_empty(), |d| {
                        d.child(ui::one_line(
                            ui::text(faces, run.input.clone())
                                .text_size(px(12.5))
                                .text_color(theme::text2()),
                        ))
                    })
                    .child(tag)
                    .child(div().flex_1())
                    .child(ui::fine(
                        faces,
                        format!("{}/{total} · {began}", run.done.len()),
                    )),
            )
            .child(steps)
            .child(actions.pt(px(6.)))
    }
}
