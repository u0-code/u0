//! Settings › Workflows, as t3code's Workflows page: the workflows your
//! files define, each begun from here with an input; and their runs,
//! filtered to those that need you or are going, each with its steps'
//! results, its question to answer, and resume or cancel.

use std::time::SystemTime;

use gpui::component::{
    Sizable as _,
    input::Input,
    setting::{SettingGroup, SettingItem},
    tag::Tag,
};
use gpui::{Context, Div, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::{Desk, flows::Showing},
    fonts::Faces,
    store, theme,
    ui::{self, Weight},
    when,
    workflows::{Run, State, Step},
};

impl Desk {
    /// The Workflows tab: the workflows, and their runs.
    pub(super) fn flows(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let folder = crate::workflows::folder(self.home_on(self.pages.on));
        let mut defs: Vec<SettingItem> = Vec::new();
        if self.flows.input.is_some() {
            defs.push(item(
                w,
                "Input",
                "What the run begins with: Return starts it.",
                |d, _, _| {
                    (d.flows.input.as_ref())
                        .map_or_else(div, |i| div().w(px(240.)).child(Input::new(i).small()))
                        .into_any_element()
                },
            ));
        }
        if self.flows.list.is_empty() {
            defs.push(item(
                w,
                "No workflows yet",
                format!(
                    "Each is a JSON file in {}: its steps say a prompt to an agent, run a command, or ask you.",
                    Desk::shown(&folder)
                ),
                |_, _, _| div().into_any_element(),
            ));
        }
        for (n, flow) in self.flows.list.iter().enumerate() {
            defs.push(match flow {
                Err((name, why)) => {
                    let why = why.clone();
                    whole(w, &[], move |_, faces, _| {
                        ui::text(faces, why.clone())
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::coral())
                            .into_any_element()
                    })
                    .keywords([name.clone()])
                }
                Ok(flow) => {
                    let steps: Vec<&str> = flow.steps.iter().map(Step::name).collect();
                    let about = if flow.about.is_empty() {
                        steps.join(" → ")
                    } else {
                        format!("{}\n{}", flow.about, steps.join(" → "))
                    };
                    let name = flow.name.clone();
                    item(w, flow.name.clone(), about, move |_, _, cx| {
                        let name = name.clone();
                        ui::button(
                            SharedString::from(format!("flow-start-{n}")),
                            "Start",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_flow(name.clone(), window, cx);
                        }))
                        .into_any_element()
                    })
                }
            });
        }
        let showing = item(
            w,
            "Showing",
            "Those that need you, those going, or all.",
            |d, _, cx| {
                let showing = d.flows.showing;
                let show =
                    |label: &'static str, which: Showing| -> (&'static str, bool, ui::Pick) {
                        (
                            label,
                            showing == which,
                            Box::new(cx.listener(move |this, _, _, cx| {
                                this.flows.showing = which;
                                cx.notify();
                            })),
                        )
                    };
                ui::segmented(
                    "flow-showing",
                    vec![
                        show("Needs you", Showing::NeedsYou),
                        show("Going", Showing::Going),
                        show("All", Showing::All),
                    ],
                )
                .into_any_element()
            },
        );
        let mut runs = vec![showing];
        match &self.flows.runs {
            Err(why) => {
                let why = why.clone();
                runs.push(whole(w, &[], move |_, _, _| {
                    ui::trouble(why.clone()).into_any_element()
                }));
            }
            Ok(all) => {
                let shown: Vec<&Run> = (all.iter().rev())
                    .filter(|r| match self.flows.showing {
                        Showing::NeedsYou => matches!(r.state, State::Asking | State::Paused(_)),
                        Showing::Going => r.state == State::Going,
                        Showing::All => true,
                    })
                    .collect();
                if shown.is_empty() {
                    runs.push(item(w, "No runs here.", "", |_, _, _| {
                        div().into_any_element()
                    }));
                }
                for run in shown {
                    let (run, named) = (run.clone(), run.workflow.clone());
                    runs.push(
                        whole(w, &[], move |d, faces, cx| {
                            d.flow_run_row(faces, &run, cx).into_any_element()
                        })
                        .keywords([named]),
                    );
                }
            }
        }
        vec![group("Workflows", defs), group("Runs", runs)]
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
                            .text_size(px(crate::style::CAPTION))
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
                            .text_size(px(crate::style::CAPTION))
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
                        .text_size(px(crate::style::LABEL))
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
                        .text_size(px(crate::style::CAPTION))
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
                        .child(
                            ui::text(faces, step.name().to_owned())
                                .text_size(px(crate::style::CAPTION)),
                        ),
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::text(faces, run.workflow.clone()).text_size(px(crate::style::LABEL)))
                    .when(!run.input.is_empty(), |d| {
                        d.child(ui::one_line(
                            ui::text(faces, run.input.clone())
                                .text_size(px(crate::style::CAPTION))
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
