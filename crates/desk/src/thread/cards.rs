//! The cards in a thread: a question the agent asks, how a turn ended,
//! the agent stopping, and signing in.

use agent_client_protocol::v1::schema::{
    PermissionOptionKind, StopReason, ToolCallContent, ToolKind,
};
use std::path::Path;

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{bare, icon, meta, right, step, verb, well};
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Slot,
    session::{Asked, Item, Session},
    theme,
    ui::{self, Weight},
    when,
};

impl Desk {
    /// A question the agent asks, as a card with its choices; once answered,
    /// a quiet row saying what was chosen.
    pub(super) fn asked(
        &self,
        faces: &Faces,
        s: &Session,
        key: &str,
        asked: &Asked,
        cx: &mut Context<Self>,
    ) -> Div {
        if let Some((name, kind)) = &asked.chosen {
            let allowed = matches!(
                kind,
                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
            );
            let (word, tail) = match kind {
                PermissionOptionKind::AllowOnce => ("You allowed", "once"),
                PermissionOptionKind::AllowAlways => ("You allowed", "always"),
                _ if name == "Cancelled" => ("Cancelled", ""),
                _ => ("You rejected", ""),
            };
            return step()
                .child(
                    ui::icon(if allowed { "check" } else { "x" }, 16.).text_color(if allowed {
                        theme::ok()
                    } else {
                        theme::faint()
                    }),
                )
                .child(verb(faces, word))
                .child(
                    ui::one_line(
                        ui::mono(faces, ui::fit(&bare(&asked.title), 64))
                            .text_size(px(13.5))
                            .text_color(theme::text()),
                    )
                    .w_auto(),
                )
                .when(!tail.is_empty(), |d| d.child(right(faces, tail)));
        }
        let name = self.name_of(&s.meta.agent);
        let kind = s
            .tools()
            .filter(|t| bare(&t.call.title) == bare(&asked.title))
            .last()
            .map(|t| t.call.kind);
        let headline = match kind {
            Some(ToolKind::Execute) => format!("{name} wants to run a command"),
            Some(ToolKind::Edit) => format!("{name} wants to edit a file"),
            Some(ToolKind::Delete) => format!("{name} wants to delete a file"),
            Some(ToolKind::Fetch) => format!("{name} wants to fetch a page"),
            _ => format!("{name} asks before it goes on"),
        };
        let mut allow = div().flex().gap(px(6.));
        let mut reject = div().flex().gap(px(6.));
        for (n, option) in asked.options.iter().enumerate() {
            let weight = match option.kind {
                PermissionOptionKind::AllowOnce => Weight::Primary,
                PermissionOptionKind::AllowAlways => Weight::Secondary,
                _ => Weight::Quiet,
            };
            let (request, chosen) = (asked.id, option.clone());
            let button = ui::button(
                faces,
                SharedString::from(format!("{key}-opt-{n}")),
                option.name.clone(),
                weight,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(request, &chosen, cx)));
            if weight == Weight::Quiet {
                reject = reject.child(button);
            } else {
                allow = allow.child(button);
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(16.))
            .py(px(14.))
            .rounded(px(12.))
            .bg(theme::rail())
            .border_1()
            .border_color(theme::border2())
            .child(ui::text(faces, headline).text_size(px(14.)))
            .child(well(faces).px(px(12.)).py(px(8.)).child(bare(&asked.title)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(allow)
                    .child(div().flex_1())
                    .child(reject),
            )
    }

    /// How a turn ended.
    /// What can be done with a turn whose files were checkpointed: see its
    /// diff, put the files back as they were before it, or after undoing,
    /// put it back again.
    fn turn_tools(faces: &Faces, s: &Session, i: usize, cx: &mut Context<Self>) -> Option<Div> {
        let turn = s.turn_of(i)?;
        let marks = s.turns.get(turn)?;
        marks.before.as_ref()?;
        let idle = !matches!(
            s.state,
            crate::session::State::Working | crate::session::State::Opening
        );
        let undone = s.redo.as_ref().is_some_and(|(t, _)| *t == turn);
        let last = s.turns.iter().rposition(|t| t.before.is_some()) == Some(turn);
        let label = if undone {
            "Redo"
        } else if last {
            "Undo"
        } else {
            "Restore to before"
        };
        let mut tools = div().flex().flex_none().gap(px(2.));
        if marks.diff.as_ref().is_some_and(|d| !d.is_empty()) {
            tools = tools.child(
                ui::button(
                    faces,
                    SharedString::from(format!("diff-{i}")),
                    "Diff",
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.show_changes(crate::git::Scope::Turn(turn), cx);
                })),
            );
        }
        if idle {
            tools = tools.child(
                ui::button(
                    faces,
                    SharedString::from(format!("undo-{i}")),
                    label,
                    Weight::Quiet,
                )
                .tooltip(ui::tip(if undone {
                    "Put this turn's changes back in the files"
                } else {
                    "Put the files back as they were before this turn"
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.undo(turn, cx))),
            );
        }
        Some(tools)
    }

    pub(super) fn ended(
        &self,
        faces: &Faces,
        s: &Session,
        i: usize,
        reason: StopReason,
        secs: u64,
        cx: &mut Context<Self>,
    ) -> Div {
        let took = when::took(std::time::Duration::from_secs(secs));
        let name = self.name_of(&s.meta.agent);
        let body = match reason {
            StopReason::EndTurn => {
                // The files changed since what you last said: the turn's
                // git diff when the folder is a repository, which also sees
                // what the agent wrote without telling; else its diffs.
                let start = s.items[..i]
                    .iter()
                    .rposition(|x| matches!(x, Item::You(_)))
                    .unwrap_or(0);
                let mut paths: Vec<&Path> = Vec::new();
                for x in &s.items[start..i] {
                    if let Item::Tool(t) = x {
                        for c in &t.call.content {
                            if let ToolCallContent::Diff(d) = c
                                && !paths.contains(&d.path.as_path())
                            {
                                paths.push(&d.path);
                            }
                        }
                    }
                }
                let git = s
                    .turn_of(i)
                    .and_then(|t| s.turns.get(t))
                    .and_then(|t| t.diff.as_ref());
                let changed = match git.map_or(paths.len(), Vec::len) {
                    0 => String::new(),
                    1 => " · 1 file changed".into(),
                    n => format!(" · {n} files changed"),
                };
                // The last turn, in a mode that only plans, with a plan:
                // it can be carried out from here.
                let last = !s.items[i + 1..].iter().any(|x| matches!(x, Item::You(_)));
                let ready = last && s.plan.is_some() && s.planning();
                let key = s.key;
                meta(faces, format!("{name} · {took}{changed}"))
                    .px(px(10.))
                    .when(ready, |d| {
                        d.flex().items_center().gap(px(10.)).child(
                            ui::button(faces, "run-plan", "Carry out the plan", Weight::Secondary)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.run_plan(key, cx)),
                                ),
                        )
                    })
            }
            StopReason::Cancelled => step()
                .child(icon("square"))
                .child(verb(faces, "You stopped this turn"))
                .child(right(faces, format!("after {took}"))),
            StopReason::Refusal => step()
                .child(icon("alert"))
                .child(verb(faces, format!("{name} refused to go on."))),
            other => {
                let words = if other == StopReason::MaxTokens {
                    "Stopped at the output limit."
                } else {
                    "Stopped at the turn's limit of requests."
                };
                step().child(icon("alert")).child(verb(faces, words)).child(
                    div().flex().flex_1().justify_end().child(
                        ui::button(
                            faces,
                            SharedString::from(format!("continue-{i}")),
                            "Continue",
                            Weight::Quiet,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.say("Continue", cx))),
                    ),
                )
            }
        };
        div()
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(body))
            .children(Self::turn_tools(faces, s, i, cx))
    }

    /// The agent stopped, or the session could not be opened.
    pub(super) fn broken(
        &self,
        faces: &Faces,
        s: &Session,
        why: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = self.name_of(&s.meta.agent);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(12.))
            .border_1()
            .border_color(theme::coral().opacity(0.35))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::dot(theme::coral()))
                    .child(ui::text(faces, format!("{name} stopped")).text_size(px(14.))),
            )
            .child(
                well(faces)
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .child(why.to_owned()),
            )
            .child(
                div().flex().gap(px(6.)).child(
                    ui::button(faces, "restart", "Restart session", Weight::Secondary)
                        .on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
                ),
            )
    }

    /// The agent asks to be signed in before it opens the session.
    pub(super) fn sign_in_card(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let name = self.name_of(&s.meta.agent);
        let methods: Vec<(String, String)> = self
            .slot(&s.meta.agent)
            .into_iter()
            .flat_map(Slot::sign_ins)
            .map(|m: &agent_client_protocol::v1::schema::AuthMethod| {
                (m.id().0.to_string(), m.name().to_owned())
            })
            .collect();
        let busy = self.signing.as_deref() == Some(s.meta.agent.as_str());
        let mut buttons = div().flex().flex_wrap().gap(px(6.));
        if busy {
            buttons = buttons.child(step().px(px(0.)).child(ui::live_dot()).child(
                verb(faces, "Waiting for the sign-in to finish").text_color(theme::accent()),
            ));
        } else {
            for (n, (id, label)) in methods.into_iter().enumerate() {
                let agent = s.meta.agent.clone();
                let weight = if n == 0 {
                    Weight::Primary
                } else {
                    Weight::Secondary
                };
                buttons = buttons.child(
                    ui::button(
                        faces,
                        SharedString::from(format!("auth-{n}")),
                        label,
                        weight,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.sign_in(&agent, &id, cx))),
                );
            }
        }
        let agent = s.meta.agent.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(16.))
            .py(px(14.))
            .rounded(px(12.))
            .bg(theme::rail())
            .border_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
                    .child(ui::text(faces, format!("Sign in to {name}")).text_size(px(14.))),
            )
            .child(
                ui::text(
                    faces,
                    format!(
                        "{name} signs in with its own account; Ghost never sees it. What you said is sent once you are in."
                    ),
                )
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(theme::text2()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(buttons)
                    .child(div().flex_1())
                    .child(
                        ui::button(faces, "auth-retry", "I have signed in", Weight::Quiet).on_click(
                            cx.listener(move |this, _, _, cx| this.retry_open(&agent, cx)),
                        ),
                    ),
            )
    }
}
