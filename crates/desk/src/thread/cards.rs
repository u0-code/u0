//! The cards in a thread: how a turn ended, the agent stopping, and
//! signing in. A question asked before it goes on is `permission`'s.

use agent_client_protocol::v1::schema::StopReason;

use gpui::{Context, Div, SharedString, div, prelude::*, px};
use u0ui::components::alert::AlertVariant;

use super::{icon, meta, right, step, verb};
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Slot,
    session::{Item, Session},
    theme,
    ui::{self, Weight},
    when,
};

impl Desk {
    /// The files a turn changed, as git saw them from its checkpoint
    /// before to the one after (`TurnChanges`): each opens the turn's
    /// diff, as does Show in diff; while nothing goes on, Undo puts the
    /// files back as they were before it (Restore to before, for one not
    /// the latest), and once undone Redo puts its changes back.
    fn turn_changes(s: &Session, i: usize, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        use u0ui::t3::chat::{TurnChanges, TurnChangesTexts, TurnFile};
        let turn = s.turn_of(i)?;
        let marks = s.turns.get(turn)?;
        marks.before.as_ref()?;
        let diff = marks.diff.as_ref().filter(|d| !d.is_empty())?;
        let files = diff
            .iter()
            .map(|f| {
                TurnFile::new(f.path.clone(), f.added, f.gone)
                    .status(crate::aside::diff::file_status_of(f))
            })
            .collect();
        let idle = !matches!(
            s.state,
            crate::session::State::Working | crate::session::State::Opening
        );
        let undone = s.redo.as_ref().is_some_and(|(t, _)| *t == turn);
        let last = s.turns.iter().rposition(|t| t.before.is_some()) == Some(turn);
        let undo = crate::tr(if undone {
            "Redo"
        } else if last {
            "Undo"
        } else {
            "Restore to before"
        });
        let (review, open) = (cx.entity(), cx.entity());
        let mut card =
            TurnChanges::new(SharedString::from(format!("changes-{}-{i}", s.key)), files)
                .texts(TurnChangesTexts {
                    changed: std::rc::Rc::new(|n| {
                        if n == 1 {
                            crate::tr("1 file changed")
                        } else {
                            crate::tr_with("%{n} files changed", &[("n", &n)])
                        }
                    }),
                    review: crate::tr("Show in diff"),
                    undo,
                })
                .on_review(move |_, cx| {
                    review.update(cx, |this, cx| {
                        this.show_changes(crate::git::Scope::Turn(turn), cx);
                    });
                })
                .on_open_file(move |path, _, cx| {
                    open.update(cx, |this, cx| {
                        this.show_changes(crate::git::Scope::Turn(turn), cx);
                        this.reveal_in_diff(std::path::Path::new(path.as_ref()), cx);
                    });
                });
        if idle {
            let desk = cx.entity();
            card = card.on_undo(move |window, cx| {
                desk.update(cx, |this, cx| this.ask_undo(turn, window, cx));
            });
        }
        Some(card)
    }

    /// Over the answer to what you said at `you`: the agent's mark, its
    /// name and model, how long the turn took and when you asked; then
    /// its answer copied, quoted in your reply, or branched off after.
    pub(super) fn said_head(
        &self,
        faces: &Faces,
        s: &Session,
        you: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use agent_client_protocol::v1::schema::SessionConfigOptionCategory;
        use u0ui::t3::timeline::{AssistantMessage, AssistantMessageTexts};
        let end = s.items[you + 1..]
            .iter()
            .position(|x| matches!(x, Item::You(_) | Item::Told(_)))
            .map_or(s.items.len(), |p| you + 1 + p);
        let secs = s.items[you + 1..end].iter().find_map(|x| match x {
            Item::Ended { secs, .. } => Some(*secs),
            _ => None,
        });
        let answer = (you + 1..end).rev().find_map(|k| match &s.items[k] {
            Item::Agent(text) => Some((k, text.clone())),
            _ => None,
        });
        let mut who = self.name_of(&s.meta.agent);
        if let Some(model) = Self::option_name(s, &SessionConfigOptionCategory::Model) {
            who = format!("{who} · {model}");
        }
        let mut head = AssistantMessage::new(SharedString::from(format!("said-{}-{you}", s.key)))
            .mark(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
            .model(who)
            .texts(AssistantMessageTexts {
                copy: crate::tr("Copy"),
                reply: crate::tr("Quote in your reply"),
                fork: crate::tr("Branch off after this"),
            });
        if let Some(secs) = secs {
            head = head.elapsed(secs);
        }
        if let Some(t) = s.time_of(you) {
            head = head.time(when::said_at(
                t,
                std::time::SystemTime::now(),
                self.settings.twelve,
            ));
        }
        if let Some((at, text)) = answer {
            let key = s.key;
            let idle = matches!(
                s.state,
                crate::session::State::Idle | crate::session::State::Closed
            );
            let (copy, quoted) = (text.clone(), text);
            head = head.on_copy(move |_, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
            });
            let desk = cx.entity();
            head = head.on_reply(move |window, cx| {
                desk.update(cx, |this, cx| {
                    this.quote(&quoted, Some((key, at)), window, cx);
                });
            });
            if idle && s.through(at).is_some() && self.can_fork(s) {
                let desk = cx.entity();
                head = head.on_fork(move |_, cx| {
                    desk.update(cx, |this, cx| this.fork_after(key, at, cx));
                });
            }
        }
        div().px(px(10.)).child(head)
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
            // A turn that ended as it should says nothing more: its
            // head says how long it took, its changes are under it.
            StopReason::EndTurn => div(),
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
            .child(body)
            .children(Self::turn_changes(s, i, cx))
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
        let id = SharedString::from(format!("trouble-{}-{i}", s.key));
        let note = div().child(ui::trouble(id, text.to_owned()));
        let latest = !s.items[i..].iter().any(|x| matches!(x, Item::You(_)));
        if !latest || !text.to_lowercase().contains("limit") {
            return note;
        }
        let key = s.key;
        let now = std::time::SystemTime::now();
        // Going on by itself as the limit resets: a task said in this
        // session, which the server runs then.
        let resuming = self
            .tasks_on(s.host)
            .and_then(|r| r.as_ref().ok())
            .and_then(|list| {
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

    /// The agent stopped, or the session could not be opened
    /// (`ThreadStates`: the agent failed to start): what it said, then
    /// Restart thread.
    pub(super) fn broken(&self, s: &Session, why: &str, cx: &mut Context<Self>) -> Div {
        let name = self.name_of(&s.meta.agent);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                ui::alert(
                    SharedString::from(format!("broken-{}", s.key)),
                    AlertVariant::Error,
                )
                .title(crate::tr_with("%{name} stopped", &[("name", &name)]))
                .words(why.to_owned()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button("restart", crate::tr("Try again"), Weight::Secondary)
                            .on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
                    )
                    .child(
                        ui::button("diagnose", crate::tr("Open Diagnostics"), Weight::Quiet)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_tab(crate::pages::Tab::Diagnostics, window, cx);
                            })),
                    ),
            )
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
            .bg(theme::rail())
            .border_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
                    .child(
                        ui::text(
                            faces,
                            crate::tr_with("Sign in to %{name}", &[("name", &name)]),
                        )
                        .text_size(px(crate::style::BODY)),
                    ),
            )
            .child(
                ui::text(
                    faces,
                    crate::tr_with(
                        "%{name} signs you in; u0 never sees it. Then your message is sent.",
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
                        ui::button("auth-retry", crate::tr("I have signed in"), Weight::Quiet)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.retry_open(host, &agent, cx);
                            })),
                    ),
            )
    }
}
