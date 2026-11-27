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
            let allowed = allows(*kind);
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
                .child(verb(faces, crate::tr(word)))
                .child(
                    ui::one_line(
                        ui::mono(faces, bare(&asked.title))
                            .text_size(px(crate::style::LABEL))
                            .text_color(theme::text()),
                    )
                    .w_auto(),
                )
                .when(!tail.is_empty(), |d| d.child(right(faces, crate::tr(tail))));
        }
        let name = self.name_of(&s.meta.agent);
        let call = s
            .tools()
            .filter(|t| bare(&t.call.title) == bare(&asked.title))
            .last()
            .map(|t| &t.call);
        // Leaving plan mode is asked with the plan: Claude's plan card.
        let plan = call
            .filter(|c| c.kind == ToolKind::SwitchMode)
            .map(|c| {
                c.content
                    .iter()
                    .filter_map(|x| match x {
                        ToolCallContent::Content(c) => Some(crate::session::text_of(&c.content)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .filter(|p| !p.trim().is_empty());
        let headline = match (plan.is_some(), call.map(|c| c.kind)) {
            (true, _) => "%{name} proposed a plan",
            (_, Some(ToolKind::Execute)) => "%{name} wants to run a command",
            (_, Some(ToolKind::Edit)) => "%{name} wants to edit a file",
            (_, Some(ToolKind::Delete)) => "%{name} wants to delete a file",
            (_, Some(ToolKind::Fetch)) => "%{name} wants to fetch a page",
            _ => "%{name} asks before it goes on",
        };
        let headline = crate::tr_with(headline, &[("name", &name)]);
        // The agent's options are the buttons: what turns it down at the
        // left, what lets it go on at the right.
        let mut reject = div().flex().gap(px(6.));
        let mut allow = div().flex().gap(px(6.));
        let primary = default_option(&asked.options);
        for (n, option) in asked.options.iter().enumerate() {
            let weight = match option.kind {
                _ if Some(n) == primary => Weight::Primary,
                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways => {
                    Weight::Secondary
                }
                _ => Weight::Quiet,
            };
            let (at, request, chosen) = (s.key, asked.id, option.clone());
            let button = ui::button(
                SharedString::from(format!("{key}-opt-{n}")),
                option.name.clone(),
                weight,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(at, request, &chosen, cx)));
            if allows(option.kind) {
                allow = allow.child(button);
            } else {
                reject = reject.child(button);
            }
        }
        // Turned down with a word: opens an inline note box on the card, or
        // sends your reply once rejected.
        if asked
            .options
            .iter()
            .any(|o| o.kind == PermissionOptionKind::RejectOnce)
        {
            let (at, request) = (s.key, asked.id);
            reject = reject.child(
                ui::button(
                    SharedString::from(format!("{key}-instead")),
                    crate::tr("No, and tell it…"),
                    Weight::Quiet,
                )
                .tooltip(crate::tr("Reject it, then write what it should do instead"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_tell(at, request, false, window, cx);
                })),
            );
        }
        // Revise: write changes to the plan inline on the card.
        if plan.is_some() {
            let (at, request) = (s.key, asked.id);
            reject = reject.child(
                ui::button(
                    SharedString::from(format!("{key}-revise")),
                    crate::tr("Revise…"),
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_tell(at, request, true, window, cx);
                })),
            );
        }
        let body = match plan {
            Some(plan) => div()
                .max_h(px(360.))
                .overflow_hidden()
                .child(crate::markdown::render(
                    faces,
                    &format!("{key}-plan"),
                    &plan,
                    cx,
                ))
                .into_any_element(),
            None => well(faces)
                .px(px(12.))
                .py(px(8.))
                .child(bare(&asked.title))
                .into_any_element(),
        };
        let fields = (!asked.facts.is_empty()).then(|| {
            let mut rows = div().flex().flex_col().gap(px(4.));
            for (k, v) in &asked.facts {
                rows = rows.child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(10.))
                        .child(
                            ui::mono(faces, k.clone())
                                .w(px(120.))
                                .flex_none()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )
                        .child(
                            ui::mono(faces, v.clone())
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::text2()),
                        ),
                );
            }
            rows
        });
        let source = asked.tool.as_deref().map(|tool| {
            meta(
                faces,
                crate::tr_with("u0 tool · %{tool}", &[("tool", &tool)]),
            )
        });
        let footer = if let Some((_, _, revise)) = self
            .telling
            .filter(|(k, r, _)| *k == s.key && *r == asked.id)
        {
            let (at, request) = (s.key, asked.id);
            let label = if revise {
                crate::tr("What to change in the plan")
            } else {
                crate::tr_with("Tell %{name} what to do instead", &[("name", &name)])
            };
            let submit = crate::tr(if revise {
                "Send the changes"
            } else {
                "Reject once and send"
            });
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(meta(faces, label).text_color(theme::text2()))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(theme::panel())
                        .border_1()
                        .border_color(theme::border2())
                        .child(
                            gpui::component::input::Textarea::new(&self.tell_input)
                                .appearance(false),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::button(
                                SharedString::from(format!("{key}-tell-back")),
                                crate::tr("Back to the options"),
                                Weight::Quiet,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.cancel_tell(window, cx);
                                },
                            )),
                        )
                        .child(div().flex_1())
                        .child(
                            ui::button(
                                SharedString::from(format!("{key}-tell-send")),
                                submit,
                                Weight::Primary,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.reject_and_tell(at, request, revise, window, cx);
                                },
                            )),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(reject)
                        .child(div().flex_1())
                        .child(allow),
                )
                .child(meta(
                    faces,
                    crate::tr(
                        "Or type in the composer below — sending while this waits rejects once and sends your message.",
                    ),
                ))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(theme::well())
            .border_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::text(faces, headline)
                            .flex_1()
                            .text_size(px(crate::style::BODY)),
                    )
                    .children(source),
            )
            .child(body)
            .children(fields)
            .child(footer)
    }

    /// How a turn ended.
    /// What can be done with a turn whose files were checkpointed: see its
    /// diff, put the files back as they were before it, or after undoing,
    /// put it back again.
    fn turn_tools(s: &Session, i: usize, cx: &mut Context<Self>) -> Option<Div> {
        let turn = s.turn_of(i)?;
        let marks = s.turns.get(turn)?;
        marks.before.as_ref()?;
        let idle = !matches!(
            s.state,
            crate::session::State::Working | crate::session::State::Opening
        );
        let undone = s.redo.as_ref().is_some_and(|(t, _)| *t == turn);
        let last = s.turns.iter().rposition(|t| t.before.is_some()) == Some(turn);
        let label = crate::tr(if undone {
            "Redo"
        } else if last {
            "Undo"
        } else {
            "Restore to before"
        });
        let mut tools = div().flex().flex_none().gap(px(2.));
        if marks.diff.as_ref().is_some_and(|d| !d.is_empty()) {
            tools = tools.child(
                ui::button(
                    SharedString::from(format!("diff-{i}")),
                    crate::tr("Diff"),
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
                    SharedString::from(format!("undo-{i}")),
                    label,
                    Weight::Quiet,
                )
                .tooltip(crate::tr(if undone {
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
                let n = git.map_or(paths.len(), Vec::len);
                let said = match n {
                    0 => "%{name} · %{took}",
                    1 => "%{name} · %{took} · 1 file changed",
                    _ => "%{name} · %{took} · %{n} files changed",
                };
                // The last turn, in a mode that only plans, with a plan:
                // it can be carried out from here.
                let last = !s.items[i + 1..].iter().any(|x| matches!(x, Item::You(_)));
                let ready = last && s.plan.is_some() && s.planning();
                let key = s.key;
                meta(
                    faces,
                    crate::tr_with(said, &[("name", &name), ("took", &took), ("n", &n)]),
                )
                .px(px(10.))
                .when(ready, |d| {
                    d.flex().items_center().gap(px(10.)).child(
                        ui::button(
                            "run-plan",
                            crate::tr("Carry out the plan"),
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.run_plan(key, cx))),
                    )
                })
            }
            StopReason::Cancelled => step()
                .child(icon("square"))
                .child(verb(faces, crate::tr("You stopped this turn")))
                .child(right(
                    faces,
                    crate::tr_with("after %{took}", &[("took", &took)]),
                )),
            StopReason::Refusal => step().child(icon("alert")).child(verb(
                faces,
                crate::tr_with("%{name} refused to go on.", &[("name", &name)]),
            )),
            other => {
                let words = crate::tr(if other == StopReason::MaxTokens {
                    "Stopped at the output limit."
                } else {
                    "Stopped at the turn's limit of requests."
                });
                let key = s.key;
                let mut go = div().flex().flex_1().justify_end().gap(px(2.)).child(
                    ui::button(
                        SharedString::from(format!("continue-{i}")),
                        crate::tr("Continue"),
                        Weight::Quiet,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.say("Continue", cx))),
                );
                // Or go on with another agent, the conversation with it.
                for (id, other) in self.others(s.host, &s.meta.agent) {
                    go = go.child(
                        ui::button(
                            SharedString::from(format!("continue-{i}-{id}")),
                            crate::tr_with("Continue with %{name}", &[("name", &other)]),
                            Weight::Quiet,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.hand_off(key, &id, window, cx);
                            },
                        )),
                    );
                }
                step()
                    .child(icon("alert"))
                    .child(verb(faces, words))
                    .child(go)
            }
        };
        let checkpoint_err = s
            .turn_of(i)
            .and_then(|t| s.turns.get(t))
            .and_then(|t| t.checkpoint_error.as_deref());
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w(px(0.)).child(body))
                    .children(Self::turn_tools(s, i, cx)),
            )
            .when_some(checkpoint_err, |d, why| {
                d.child(
                    step()
                        .child(icon("alert").text_color(theme::accent()))
                        .child(verb(faces, crate::tr("This turn can't be undone")))
                        .child(meta(
                            faces,
                            crate::tr_with(
                                "Its files could not be checkpointed: %{why}",
                                &[("why", &why)],
                            ),
                        )),
                )
            })
    }

    /// What went wrong in a turn; where it is a limit the agent hit, the
    /// ways on under it: try again now, or when it says the limit resets
    /// (said then, or the session snoozed till then), or go on with
    /// another agent.
    pub(super) fn trouble_note(
        &self,
        s: &Session,
        i: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let note = div().child(ui::trouble(text.to_owned()));
        let latest = !s.items[i..].iter().any(|x| matches!(x, Item::You(_)));
        if !latest || !text.to_lowercase().contains("limit") {
            return note;
        }
        let key = s.key;
        let now = std::time::SystemTime::now();
        // Going on by itself as the limit resets: a task said in this
        // session, which the server runs then.
        let resuming = self.tasks.list.as_ref().ok().and_then(|list| {
            list.iter().find(|t| {
                t.same
                    && t.session == s.meta.id
                    && t.prompt == "Continue"
                    && matches!(t.schedule, crate::tasks::Schedule::Once(_))
                    && t.next.is_some()
            })
        });
        if let Some(task) = resuming {
            let (id, at) = (task.id, task.next.map(crate::store::time));
            let goes = at.map_or_else(
                || crate::tr("Goes on, as the limit resets."),
                |at| {
                    let clock = when::said_at(at, now, self.settings.twelve);
                    crate::tr_with(
                        "Goes on at %{clock}, as the limit resets.",
                        &[("clock", &clock)],
                    )
                },
            );
            return note.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(6.))
                    .child(ui::fine(crate::fonts::faces(cx), goes))
                    .child(
                        ui::button("limit-not", crate::tr("Don't go on"), Weight::Quiet)
                            .on_click(cx.listener(move |this, _, _, _| this.drop_task(id))),
                    ),
            );
        }
        let mut ways = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(2.))
            .pt(px(6.))
            .child(
                ui::button("limit-retry", crate::tr("Try again"), Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| this.say("Continue", cx))),
            );
        if let Some(at) = when::reset_at(text, now) {
            let clock = when::said_at(at, now, self.settings.twelve);
            ways = ways
                .child(
                    ui::button(
                        "limit-later",
                        crate::tr_with("Try again at %{clock}", &[("clock", &clock)]),
                        Weight::Quiet,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.retry_at(key, at, cx))),
                )
                .child(
                    ui::button(
                        "limit-snooze",
                        crate::tr_with("Snooze till %{clock}", &[("clock", &clock)]),
                        Weight::Quiet,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.snooze(&[key], crate::desk::Later::At(at), cx);
                    })),
                );
        }
        for (id, other) in self.others(s.host, &s.meta.agent) {
            ways = ways.child(
                ui::button(
                    SharedString::from(format!("limit-{id}")),
                    crate::tr_with("Continue with %{name}", &[("name", &other)]),
                    Weight::Quiet,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.hand_off(key, &id, window, cx)),
                ),
            );
        }
        note.child(ways)
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
                    .child(
                        ui::text(faces, crate::tr_with("%{name} stopped", &[("name", &name)]))
                            .text_size(px(crate::style::BODY)),
                    ),
            )
            .child(
                well(faces)
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(crate::style::CAPTION))
                    .child(why.to_owned()),
            )
            .child({
                let why = why.to_owned();
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::button("restart", crate::tr("Restart thread"), Weight::Secondary)
                            .on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
                    )
                    .child(
                        ui::button("copy-why", crate::tr("Copy the error"), Weight::Quiet)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(why.clone()));
                            }),
                    )
            })
    }

    /// The agent asks to be signed in before it opens the session.
    pub(super) fn sign_in_card(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let name = self.name_of(&s.meta.agent);
        let methods: Vec<(String, String)> = self
            .slot(s.host, &s.meta.agent)
            .into_iter()
            .flat_map(Slot::sign_ins)
            .map(|m: &agent_client_protocol::v1::schema::AuthMethod| {
                (m.id().0.to_string(), m.name().to_owned())
            })
            .collect();
        let busy = (self.signing.as_ref()).is_some_and(|(h, a)| s.of(*h, a));
        let mut buttons = div().flex().flex_wrap().gap(px(6.));
        if busy {
            buttons = buttons.child(
                step().px(px(0.)).child(ui::live_dot()).child(
                    verb(faces, crate::tr("Waiting for the sign-in to finish"))
                        .text_color(theme::accent()),
                ),
            );
        } else {
            let host = s.host;
            for (n, (id, label)) in methods.into_iter().enumerate() {
                let agent = s.meta.agent.clone();
                let weight = if n == 0 {
                    Weight::Primary
                } else {
                    Weight::Secondary
                };
                buttons = buttons.child(
                    ui::button(SharedString::from(format!("auth-{n}")), label, weight).on_click(
                        cx.listener(move |this, _, _, cx| this.sign_in(host, &agent, &id, cx)),
                    ),
                );
            }
        }
        let (host, agent) = (s.host, s.meta.agent.clone());
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
                    .child(ui::text(faces, crate::tr_with("Sign in to %{name}", &[("name", &name)])).text_size(px(crate::style::BODY))),
            )
            .child(
                ui::text(
                    faces,
                    crate::tr_with(
                        "%{name} signs in with its own account; u0 never sees it. What you said is sent once you are in.",
                        &[("name", &name)],
                    ),
                )
                .text_size(px(crate::style::LABEL))
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
                        ui::button("auth-retry", crate::tr("I have signed in"), Weight::Quiet).on_click(
                            cx.listener(move |this, _, _, cx| this.retry_open(host, &agent, cx)),
                        ),
                    ),
            )
    }
}

/// Whether choosing `kind` lets the agent go on.
fn allows(kind: PermissionOptionKind) -> bool {
    matches!(
        kind,
        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
    )
}

/// The option a permission card offers first: the narrowest yes, so that
/// the default never grants more than this once.
fn default_option(
    options: &[agent_client_protocol::v1::schema::PermissionOption],
) -> Option<usize> {
    (options.iter())
        .position(|o| o.kind == PermissionOptionKind::AllowOnce)
        .or_else(|| options.iter().position(|o| allows(o.kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permission_card_offers_allow_once_before_allow_always() {
        let options: Vec<agent_client_protocol::v1::schema::PermissionOption> =
            serde_json::from_str(
                r#"[{"optionId":"always","name":"Allow for this session","kind":"allow_always"},
                    {"optionId":"allow","name":"Allow","kind":"allow_once"},
                    {"optionId":"no","name":"Reject","kind":"reject_once"}]"#,
            )
            .unwrap();
        assert_eq!(default_option(&options), Some(1));
        assert_eq!(default_option(&options[..1]), Some(0));
        assert_eq!(default_option(&options[2..]), None);
    }
}
