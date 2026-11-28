//! Beside a pull request's summary, as its board sets it: the merge box
//! (why it cannot merge yet, its method, auto-merge), its checks (the log
//! of one that failed, run again, a thread to fix them), what it is about
//! (its labels, reviewers and their verdicts, assignees, the issue it
//! closes, how much of it is code, tests or docs), the stack it is in, and
//! the stops of its guide.

use gpui::component::{Sizable as _, input::Input, scroll::ScrollableElement as _};
use gpui::{Context, Div, SharedString, div, prelude::*, px};
use u0ui::{
    prelude::*,
    t3::review::{
        Check, ChecksList, ChecksStatus, GuideRail, GuideStop, LabelChip, MergeControls,
        MergeMethod, MergeTexts, PullRequestStack, ReviewDecision, ReviewDecisionBadge, StackLayer,
    },
};

use crate::{
    desk::{Desk, forge::Tab},
    fonts::Faces,
    github::{Act, Detail},
    theme, ui,
};

/// A card of the column: on `layer-01`, its title and what it says of
/// itself beside it.
fn card(title: SharedString, aside: Option<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(16.))
        .bg(theme::role(ColorRole::Layer01))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .type_style(TypeStyle::HeadingCompact01)
                .text_color(theme::role(ColorRole::TextPrimary))
                .child(title)
                .children(aside.map(|a| {
                    div()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(a)
                })),
        )
}

/// A note under something, in the helper's colour.
fn note(text: impl Into<SharedString>) -> Div {
    div()
        .type_style(TypeStyle::Label01)
        .text_color(theme::role(ColorRole::TextHelper))
        .child(text.into())
}

/// How a check stands, as u0ui draws it.
fn check_status(c: &crate::github::Check) -> ChecksStatus {
    match c.state.as_str() {
        "success" | "neutral" | "skipped" | "completed" => ChecksStatus::Passed,
        _ if c.failed() => ChecksStatus::Failed,
        _ => ChecksStatus::Running,
    }
}

impl Desk {
    /// The column beside its summary or guide.
    pub(super) fn pr_aside(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        div()
            .w(px(320.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children((d.item.state == "open").then(|| self.pr_merge(d, cx)))
            .children((!d.checks.is_empty()).then(|| self.pr_checks(faces, d, cx)))
            .child(self.pr_about(d, cx))
            .children(self.pr_stack(d, cx))
            .child(self.pr_rail(d, cx))
    }

    /// Why it cannot be merged yet, in a few words; none when it can.
    fn merge_blocked(d: &Detail) -> Option<String> {
        let (_, failed, going) = d.item.checks;
        let mut why = Vec::new();
        if d.item.draft {
            why.push(crate::tr("Draft").to_string());
        }
        if d.conflicts {
            why.push(crate::tr("Conflicts with its base").to_string());
        }
        if crate::pages::forge::decision(&d.item.review) == Some(ReviewDecision::Changes) {
            why.push(crate::tr("Changes requested").to_string());
        }
        if failed > 0 {
            why.push(crate::tr_with("%{n} failing", &[("n", &failed)]).to_string());
        }
        if going > 0 {
            why.push(crate::tr_with("%{n} running", &[("n", &going)]).to_string());
        }
        (!why.is_empty()).then(|| why.join(" · "))
    }

    /// The merge box: merged by the method chosen, or, while it cannot
    /// be, why, and auto-merge to merge it once it can.
    fn pr_merge(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let how = self.forge.pr.method;
        let n = d.item.number;
        let merge = move |d: &mut Desk, auto: bool, cx: &mut Context<Desk>| {
            let uncredit = d.settings_in(d.forge.folder.as_ref()).uncredit;
            let act = Act::Merge {
                how: how.name().into(),
                auto,
                delete: true,
                uncredit,
            };
            d.act_on(n, act, cx);
        };
        let texts = MergeTexts {
            ready: crate::tr("Ready to merge"),
            menu: crate::tr("Merge method"),
            method: crate::tr("Method"),
            auto_merge: crate::tr("Enable auto-merge"),
        };
        let blocked = Self::merge_blocked(d);
        let desk = cx.entity();
        let controls = MergeControls::new("pr-merge")
            .method(how)
            .texts(texts)
            .when_some(blocked.clone(), MergeControls::blocked)
            .on_merge({
                let desk = desk.clone();
                move |_, cx| desk.update(cx, |d, cx| merge(d, false, cx))
            })
            .on_method({
                let desk = desk.clone();
                move |m, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.forge.pr.method = m;
                        cx.notify();
                    });
                }
            })
            .on_auto_merge({
                let desk = desk.clone();
                move |_, cx| desk.update(cx, |d, cx| merge(d, true, cx))
            });
        let how_word = crate::tr(match how {
            MergeMethod::Squash => "squash",
            MergeMethod::Merge => "merge commit",
            MergeMethod::Rebase => "rebase",
        });
        div()
            .flex()
            .flex_col()
            .bg(theme::role(ColorRole::Layer01))
            .child(controls)
            .when(blocked.is_some(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .pl(px(4.))
                        .pr(px(16.))
                        .pb(px(12.))
                        .child(
                            Button::new("pr-auto-merge", crate::tr("Enable auto-merge"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    desk.update(cx, |d, cx| merge(d, true, cx));
                                }),
                        )
                        .child(note(crate::tr_with(
                            "%{how}, when it can be",
                            &[("how", &how_word)],
                        ))),
                )
            })
    }

    /// Its checks: how many failed, run and passed; each, a failed one's
    /// log read with Details; its failed ones run again, or fixed in a
    /// thread.
    fn pr_checks(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let (passed, failed, going) = d.item.checks;
        let mut counts = Vec::new();
        if failed > 0 {
            counts.push(crate::tr_with("%{n} failing", &[("n", &failed)]).to_string());
        }
        if going > 0 {
            counts.push(crate::tr_with("%{n} running", &[("n", &going)]).to_string());
        }
        if passed > 0 {
            counts.push(crate::tr_with("%{n} passed", &[("n", &passed)]).to_string());
        }
        let checks: Vec<_> = d.checks.clone();
        let rows = (checks.iter()).map(|c| {
            Check::new(
                c.name.clone(),
                check_status(c),
                c.duration.clone().unwrap_or_default(),
            )
        });
        let list = ChecksList::new("pr-checks")
            .show_header(false)
            .compact(true)
            .details_failed_only(true)
            .details_label(crate::tr("Details"))
            .checks(rows.collect::<Vec<_>>())
            .on_details({
                let desk = cx.entity();
                move |at, _, cx| {
                    let Some(c) = checks.get(at) else { return };
                    desk.update(cx, |this, cx| match c.run() {
                        Some(run) => this.read_log(run.to_owned(), cx),
                        None => this.browse(&c.url, cx),
                    });
                }
            });
        let run = (d.checks.iter())
            .find(|c| c.failed())
            .and_then(|c| c.run().map(str::to_owned));
        let folder = self.forge.folder.clone().unwrap_or_default();
        let (n, title) = (d.item.number, d.item.title.clone());
        card(crate::tr("Checks"), Some(counts.join(" · ").into()))
            .child(list)
            .when(failed > 0, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .pt(px(4.))
                        .child(
                            Button::new("pr-fix-checks", crate::tr("Start a thread to fix checks"))
                                .variant(ButtonVariant::Tertiary)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::Chat)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.start_from_pr(
                                        folder.clone(),
                                        n,
                                        title.clone(),
                                        false,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .children(run.map(|run| {
                            Button::new("pr-rerun", crate::tr("Run again"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::Renew)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.act_forge(Act::Rerun(run.clone()), cx);
                                }))
                        })),
                )
                .child(note(crate::tr(
                    "The thread works in a worktree of its branch; nothing is pushed to it unless you ask.",
                )))
            })
            .children(self.forge.log.as_ref().map(|(run, log)| {
                let (text, bad) = match log {
                    Ok(t) => (t.clone(), false),
                    Err(e) => (e.clone(), true),
                };
                div()
                    .id("forge-log")
                    .max_h(px(320.))
                    .p(px(10.))
                    .bg(theme::well())
                    .child(note(crate::tr_with(
                        "Run %{run}, where it failed",
                        &[("run", run)],
                    )))
                    .child(
                        ui::fine_mono_said(faces, "forge-log-text", text)
                            .when(bad, |d| d.text_color(theme::coral())),
                    )
                    .overflow_y_scrollbar()
            }))
    }

    /// What it is about: its labels, reviewers and how each stands, who
    /// it is assigned to, the issue it closes, how much of it is what; a
    /// label or a person added.
    fn pr_about(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let row = |label: &str, value: gpui::AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .line_height(px(22.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(crate::tr(label)),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w(px(0.))
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .type_style(TypeStyle::BodyCompact01)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(value),
                )
        };
        let none = || {
            div()
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr("none"))
                .into_any_element()
        };
        let labels = crate::pages::forge::labels_of(&d.item);
        let labels = if labels.is_empty() {
            none()
        } else {
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(labels.into_iter().map(|l| LabelChip::new(l.name, l.color)))
                .into_any_element()
        };
        let reviewers = if d.reviewer_states.is_empty() {
            none()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(d.reviewer_states.iter().map(|(who, said)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(who.clone())
                        .children(crate::pages::forge::decision(said).map(ReviewDecisionBadge::new))
                }))
                .into_any_element()
        };
        let assignees = if d.assignees.is_empty() {
            none()
        } else {
            div().child(d.assignees.join(", ")).into_any_element()
        };
        let files = &self.forge.pr.files;
        card(crate::tr("About"), None)
            .child(row("Labels", labels))
            .child(row("Reviewers", reviewers))
            .child(row("Assignees", assignees))
            .children(d.linked_issue.as_ref().map(|(n, title)| {
                row(
                    "Linked issue",
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_color(theme::role(ColorRole::LinkPrimary))
                                .child(format!("#{n}")),
                        )
                        .child(title.clone())
                        .into_any_element(),
                )
            }))
            .when(!files.is_empty(), |el| {
                el.child(row("Changes", super::breakdown(files).into_any_element()))
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div().w_full().child(
                            Input::new(&self.forge.who)
                                .list(&self.forge.offered)
                                .small(),
                        ),
                    )
                    .child(
                        Button::new("add-label", crate::tr("Add label"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_person_or_label(0, window, cx);
                            })),
                    )
                    .child(
                        Button::new("add-person", crate::tr("Ask for review"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_person_or_label(1, window, cx);
                            })),
                    ),
            )
    }

    /// The pull requests it is stacked on and those stacked on it, among
    /// those listed, each read with a press; none where it stands alone.
    fn pr_stack(&self, d: &Detail, cx: &mut Context<Self>) -> Option<Div> {
        let listed = self.forge.rows.of(self.forge.folder.as_ref());
        let (under, over) = super::stack(&d.item, listed);
        if under.is_none() && over.is_empty() {
            return None;
        }
        let layer = |n: u64| -> Option<StackLayer> {
            let item = std::iter::once(&d.item)
                .chain(listed)
                .find(|i| i.number == n)?;
            let mut l = StackLayer::new(
                u32::try_from(n).ok()?,
                item.title.clone(),
                crate::pages::forge::pr_state(item),
            );
            if let Some(c) = crate::pages::forge::checks_of(item.checks) {
                l = l.checks(c);
            }
            Some(l)
        };
        let numbers = under.into_iter().chain([d.item.number]).chain(over);
        let layers: Vec<_> = numbers.filter_map(layer).collect();
        let count = layers.len();
        Some(
            card(
                crate::tr("Stack"),
                Some(crate::tr_with("%{n} pull requests", &[("n", &count)])),
            )
            .child(
                PullRequestStack::new()
                    .flat(true)
                    .layers(layers)
                    .current(u32::try_from(d.item.number).unwrap_or(u32::MAX))
                    .on_select({
                        let desk = cx.entity();
                        move |n, _, cx| {
                            desk.update(cx, |this, cx| {
                                this.forge.tab = Tab::Summary;
                                this.read_forge(u64::from(n), cx);
                            });
                        }
                    }),
            ),
        )
    }

    /// The stops of its guide, the one you are at marked, each opened in
    /// the Guide tab; or a guide asked for.
    fn pr_rail(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let stops = self.pr_stops(d);
        let behind = (self.forge.pr.guide.as_ref())
            .is_some_and(|(n, head, _)| *n == d.item.number && *head != d.head);
        let mut aside = Vec::new();
        if !stops.is_empty() {
            aside.push(crate::tr_with("%{n} stops", &[("n", &stops.len())]).to_string());
        }
        if behind {
            aside.push(crate::tr("behind its latest commits").to_string());
        }
        let card = card(
            crate::tr("Review guide"),
            (!aside.is_empty()).then(|| aside.join(" · ").into()),
        );
        if stops.is_empty() {
            return card.child(self.pr_guide_ask(d, cx));
        }
        let rail = stops
            .iter()
            .map(|s| GuideStop::new(s.title.clone(), s.files.max(1), s.note.clone()));
        card.child(
            GuideRail::new("pr-rail")
                .stops(rail.collect::<Vec<_>>())
                .current(self.forge.pr.stop.min(stops.len() - 1))
                .files_label(crate::tr("files"))
                .on_select({
                    let desk = cx.entity();
                    move |at, _, cx| {
                        desk.update(cx, |this, cx| {
                            this.forge.pr.stop = at;
                            this.forge.tab = Tab::Guide;
                            cx.notify();
                        });
                    }
                }),
        )
    }
}
