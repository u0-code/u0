//! An issue read beside the list, as the Issues board sets it: its state,
//! key and host over it, with it opened there and closed; its title,
//! labels, state, priority and who it is assigned to, and its words; the
//! threads about it, and a new one; what was said, and the line to say
//! something.

use gpui::component::{Sizable as _, input::Textarea, scroll::ScrollableElement as _};
use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px};
use u0ui::{
    prelude::*,
    t3::review::{IssueStateGlyph, LabelChip, Priority},
    u0::shell::{ThreadRow, ThreadStatus},
};

use crate::{
    desk::{Desk, lists},
    fonts::Faces,
    github::{Act, Detail},
    markdown, theme, ui,
};

impl Desk {
    /// The issue read, or why it is not.
    pub(super) fn issue_aside(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let shell = div().flex().flex_col().flex_1().min_h(px(0.));
        match &self.forge.detail {
            None => shell.p(px(24.)).child({
                use u0ui::components::spinner::{Spinner, SpinnerSize};
                Spinner::new("issue-reading")
                    .size(SpinnerSize::Inline)
                    .description(crate::tr("Reading…"))
            }),
            Some(Err(e)) => shell.p(px(24.)).child(ui::trouble(
                "issue-unread",
                crate::tr_with("Couldn't read it: %{e}", &[("e", e)]),
            )),
            Some(Ok(_)) if self.forge.editing => {
                shell.child(self.forge_form(faces, &crate::tr("Edit"), crate::tr("Save"), cx))
            }
            Some(Ok(d)) => shell.child(self.issue_head(d, cx)).child(
                div()
                    .id("issue-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            .p(px(24.))
                            .child(Self::issue_about(faces, d, cx))
                            .child(self.issue_threads_section(d, cx))
                            .child(self.issue_said(faces, d, cx)),
                    ),
            ),
        }
    }

    /// Its state, key, host and team; it opened there, closed or opened
    /// again, and the rest of what can be done with it.
    fn issue_head(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let item = &d.item;
        let host = crate::forge::Host::of_url(&item.url);
        let key = (item.key.clone()).unwrap_or_else(|| format!("#{}", item.number));
        let said: Vec<String> = [Some(key), host.map(|h| h.name().to_owned())]
            .into_iter()
            .chain([d.team_name.clone()])
            .flatten()
            .collect();
        let url = item.url.clone();
        let open = item.state == "open";
        let open_on = host.map_or_else(
            || crate::tr("Open on its host"),
            |h| crate::tr_with("Open in %{host}", &[("host", &h.name())]),
        );
        div()
            .h(px(38.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(24.))
            .pr(px(12.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(IssueStateGlyph::new(super::issue_state(item)))
            .child(
                ui::one_line(
                    div()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(said.join(" · ")),
                )
                .flex_1(),
            )
            .child(
                Button::new("issue-open", open_on)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Launch)
                    .icon_only(true)
                    .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx))),
            )
            .child(
                Button::new(
                    "issue-close",
                    crate::tr(if open { "Close issue" } else { "Reopen issue" }),
                )
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(if open {
                    CarbonIcon::Close
                } else {
                    CarbonIcon::Renew
                })
                .icon_only(true)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.act_forge(if open { Act::Close } else { Act::Reopen }, cx);
                })),
            )
            .child(self.forge_actions(item, cx))
    }

    /// Its title; its labels, state, priority and who it is assigned to;
    /// its words.
    fn issue_about(faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let item = &d.item;
        let mut meta = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .type_style(TypeStyle::Label01)
            .text_color(theme::role(ColorRole::TextSecondary))
            .children(
                (super::labels_of(item).into_iter()).map(|l| LabelChip::new(l.name, l.color)),
            );
        let mut words = vec![crate::tr(super::issue_state(item).label()).to_string()];
        if let Some(p) = super::priority_of(item) {
            words.push(
                crate::tr(match p {
                    Priority::Urgent => "Urgent",
                    Priority::High => "High priority",
                    Priority::Medium => "Medium priority",
                    Priority::Low => "Low priority",
                })
                .to_string(),
            );
        }
        if !d.assignees.is_empty() {
            let who = d.assignees.join(", ");
            words.push(crate::tr_with("Assigned to %{who}", &[("who", &who)]).to_string());
        }
        for w in words {
            meta = meta.child(div().child(w));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .type_style(TypeStyle::Heading03)
                    .text_color(theme::role(ColorRole::TextPrimary))
                    .child(item.title.clone()),
            )
            .child(meta)
            .child(if d.body.trim().is_empty() {
                ui::fine(faces, crate::tr("No description.")).into_any_element()
            } else {
                markdown::render(faces, "issue-words", &d.body, cx).into_any_element()
            })
    }

    /// The threads about it, the latest first, each opened with a press;
    /// one begun, or triage asked for.
    fn issue_threads_section(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let Some(place) = self.forge.folder.clone() else {
            return div();
        };
        let n = d.item.number;
        let threads = self.issue_threads(&place, n);
        let now = std::time::SystemTime::now();
        let rows: Vec<AnyElement> = (threads.iter())
            .map(|s| {
                let key = s.key;
                let status = match lists::status(s) {
                    Some(lists::Status::Working) => ThreadStatus::Working,
                    Some(lists::Status::Approval | lists::Status::SignIn | lists::Status::Plan) => {
                        ThreadStatus::NeedsYou
                    }
                    Some(lists::Status::Failed | lists::Status::Limited) => ThreadStatus::Failed,
                    _ => ThreadStatus::Idle,
                };
                ThreadRow::new(
                    SharedString::from(format!("issue-thread-{key}")),
                    s.meta.title.clone(),
                )
                .status(status)
                .time(crate::when::ago(s.meta.updated, now))
                .unread(s.meta.flags.unread)
                .on_click({
                    let desk = cx.entity();
                    move |window, cx| desk.update(cx, |this, cx| this.show(key, window, cx))
                })
                .into_any_element()
            })
            .collect();
        let title = d.item.title.clone();
        let (p2, t2) = (place.clone(), title.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr(if threads.len() == 1 {
                        "Thread on this issue"
                    } else {
                        "Threads on this issue"
                    })),
            )
            .children(rows)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .mt(px(4.))
                    .child(
                        Button::new("issue-start", crate::tr("Start thread"))
                            .variant(if threads.is_empty() {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Add)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_from_issue(p2.clone(), n, &t2, false, window, cx);
                            })),
                    )
                    .child(
                        Button::new("issue-triage", crate::tr("Triage it"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_from_issue(place.clone(), n, &title, true, window, cx);
                            })),
                    ),
            )
    }

    /// What was said on it, oldest first, and the line to say something,
    /// sent with ⌘↵ or its button.
    fn issue_said(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let host = crate::forge::Host::of_url(&d.item.url).map_or_else(
            || crate::tr("Its host").to_string(),
            |h| h.name().to_owned(),
        );
        let empty = self.forge.line.read(cx).value().trim().is_empty();
        let mut said = div().flex().flex_col().gap(px(4.)).child(
            div()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(crate::tr_with("Comments · %{n}", &[("n", &d.said.len())])),
        );
        for (n, s) in d.said.iter().enumerate() {
            said = said.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .pt(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(s.who.clone()),
                            )
                            .child(super::when_of(&s.when)),
                    )
                    .child(markdown::render(
                        faces,
                        &format!("issue-said-{n}"),
                        &s.body,
                        cx,
                    )),
            );
        }
        said.child(div().h(px(12.)))
            .child(Textarea::new(&self.forge.line).small())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mt(px(8.))
                    .child(
                        ui::one_line(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextHelper))
                                .child(match &self.forge.doing {
                                    Some(doing) => format!("{doing}…"),
                                    None => crate::tr_with(
                                        "⌘↵ to post · syncs to %{host}",
                                        &[("host", &host)],
                                    )
                                    .to_string(),
                                }),
                        )
                        .flex_1(),
                    )
                    .child(
                        Button::new("issue-comment", crate::tr("Comment"))
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .disabled(empty)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.say_forge(None, window, cx)),
                            ),
                    ),
            )
    }
}
