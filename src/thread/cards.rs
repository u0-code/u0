//! The cards in a thread: a question the agent asks, how a turn ended,
//! the agent stopping, and signing in.

use agent_client_protocol_schema::v1::{
    PermissionOptionKind, StopReason, ToolCallContent, ToolKind,
};
use std::path::Path;

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{bare, icon, meta, right, step, verb, well};
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Stage,
    session::{Asked, Item, Session},
    theme,
    ui::{self, Mark, Weight},
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
        let name = self.name_of(&s.agent);
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
            let (request, chosen) = (asked.id.clone(), option.clone());
            let button = ui::button(
                faces,
                SharedString::from(format!("{key}-opt-{n}")),
                option.name.clone(),
                weight,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(&request, &chosen, cx)));
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
        let name = self.name_of(&s.agent);
        match reason {
            StopReason::EndTurn => {
                // The files changed since what you last said.
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
                let changed = match paths.len() {
                    0 => String::new(),
                    1 => " · 1 file changed".into(),
                    n => format!(" · {n} files changed"),
                };
                meta(faces, format!("{name} · {took}{changed}")).px(px(10.))
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
        }
    }

    /// The agent stopped, or the session could not be opened.
    pub(super) fn broken(
        &self,
        faces: &Faces,
        s: &Session,
        why: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = self.name_of(&s.agent);
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
        use agent_client_protocol_schema::v1::AuthMethod;
        let name = self.name_of(&s.agent);
        let methods: Vec<(String, String)> = match self.slot(&s.agent).map(|x| &x.stage) {
            Some(Stage::Running { init, .. }) => init
                .auth_methods
                .iter()
                .filter_map(|m| match m {
                    AuthMethod::Agent(a) => Some((a.id.0.to_string(), a.name.clone())),
                    AuthMethod::Terminal(t) => Some((t.id.0.to_string(), t.name.clone())),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let busy = self.signing.as_deref() == Some(s.agent.as_str());
        let mut buttons = div().flex().flex_wrap().gap(px(6.));
        if busy {
            buttons = buttons.child(step().px(px(0.)).child(ui::live_dot()).child(
                verb(faces, "Waiting for the sign-in to finish").text_color(theme::accent()),
            ));
        } else {
            for (n, (id, label)) in methods.into_iter().enumerate() {
                let agent = s.agent.clone();
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
        let agent = s.agent.clone();
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
                    .child(ui::mark(faces, &Mark::of(&s.agent, &name), 16.))
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
