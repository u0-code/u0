//! A pull request read on its own page, as the `PullRequestDetail` board
//! sets it: its head; its tabs: the summary (its words, what happened on
//! it, its conversations on lines, your review), its changes with comments
//! on their lines and each file's Viewed box, the guide the writer wrote;
//! beside them the column of `aside`.

use gpui::component::scroll::ScrollableElement as _;
use gpui::component::{
    Sizable as _,
    checkbox::Checkbox,
    diff::{Diff, DiffAnnotation, DiffLinePosition, DiffSide},
    input::Input,
    tag::Tag,
};
use gpui::{AnyElement, App, Context, Div, SharedString, div, prelude::*, px};
use u0ui::{
    prelude::*,
    t3::{
        review::{
            DetailHeaderTexts, EventKind, PullRequestDetailHeader, PullRequestTimeline, Reaction,
            ReviewComment, ReviewDecision, ReviewForm, ReviewFormTexts, ReviewThreadCard,
            ReviewThreadTexts, ReviewVerdict, TimelineEvent,
        },
        shell::{Crumb, WorkspaceBreadcrumb},
    },
};

mod aside;

use crate::{
    desk::{Desk, forge::Tab},
    fonts::Faces,
    github::{Act, Detail, Item},
    markdown, review, theme, ui,
};

/// When it was said, as read.
fn ago(text: &str) -> String {
    crate::when::parse(text).map_or_else(String::new, |t| {
        crate::when::ago(t, std::time::SystemTime::now())
    })
}

impl Desk {
    /// The top bar over a pull request read: the way back to the list and
    /// which one it is; it on its host.
    pub(crate) fn pr_topbar(&self, left: Div, right: Div, cx: &mut Context<Self>) -> (Div, Div) {
        let f = &self.forge;
        let repo = (f.folder.as_ref()).map(|p| self.forge_name(p));
        let which = format!(
            "{} #{}",
            repo.unwrap_or_default(),
            f.chosen.unwrap_or_default()
        );
        let desk = cx.entity();
        let crumbs = WorkspaceBreadcrumb::new(
            "pr-crumbs",
            [
                Crumb::new(crate::tr("Pull requests")).on_click(move |_, cx| {
                    desk.update(cx, Desk::close_forge_item);
                }),
                Crumb::new(which.trim().to_owned()),
            ],
        );
        let url = match &f.detail {
            Some(Ok(d)) => Some(d.item.url.clone()),
            _ => None,
        };
        let open = url.map(|url| {
            let label = crate::forge::Host::of_url(&url).map_or_else(
                || crate::tr("Open on its host"),
                |h| crate::tr_with("Open on %{host}", &[("host", &h.name())]),
            );
            Button::new("pr-open", label)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(CarbonIcon::Launch)
                .icon_only(true)
                .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx)))
        });
        (left.child(crumbs), right.children(open))
    }

    /// A pull request read, on a page of its own, as its board sets it:
    /// its head, its tabs (Summary, Code, Guide); beside the summary and
    /// the guide, its merge box, checks, what it is about, its stack and
    /// its guide's stops.
    pub(crate) fn pr_page(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let page = div().flex().flex_col().flex_1().min_h(px(0.));
        let d = match &f.detail {
            None => {
                use u0ui::components::spinner::{Spinner, SpinnerSize};
                return page.px(px(40.)).pt(px(24.)).child(
                    Spinner::new("pr-reading")
                        .size(SpinnerSize::Inline)
                        .description(crate::tr("Reading…")),
                );
            }
            Some(Err(e)) => {
                return page.px(px(40.)).pt(px(24.)).child(ui::trouble(
                    "pr-unread",
                    crate::tr_with("Couldn't read it: %{e}", &[("e", e)]),
                ));
            }
            Some(Ok(_)) if f.editing => {
                return page.child(self.forge_form(
                    faces,
                    &crate::tr("Edit"),
                    crate::tr("Save"),
                    cx,
                ));
            }
            Some(Ok(d)) => d,
        };
        let head = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(self.pr_head(d, cx))
            .child(self.pr_tabs(d, cx));
        if f.tab == Tab::Code {
            // The diff scrolls itself, the width of the page.
            return page.child(div().px(px(40.)).pt(px(24.)).child(head)).child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(40.))
                    .pt(px(16.))
                    .child(self.pr_code(d, cx)),
            );
        }
        let main = match f.tab {
            Tab::Guide => self.pr_guide(faces, d, cx),
            _ => self.pr_summary(faces, d, cx),
        };
        page.child(
            div()
                .id("pr-page")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .px(px(40.))
                        .pt(px(24.))
                        .pb(px(32.))
                        .child(head)
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(32.))
                                .child(main.flex_1().min_w(px(0.)))
                                .child(self.pr_aside(faces, d, cx)),
                        ),
                ),
        )
    }

    /// Its state, title, who wants it merged from where into where; it
    /// checked out in a worktree, its title edited, and the rest of what
    /// can be done with it.
    fn pr_head(&self, d: &Detail, cx: &mut Context<Self>) -> PullRequestDetailHeader {
        let item = &d.item;
        let (n, title) = (item.number, item.title.clone());
        let folder = self.forge.folder.clone().unwrap_or_default();
        PullRequestDetailHeader::new(
            "pr-head",
            u32::try_from(n).unwrap_or(u32::MAX),
            item.title.clone(),
        )
        .state(super::forge::pr_state(item))
        .repo(self.forge_name(&folder))
        .author(item.author.clone())
        .branches(item.branch.clone(), item.base.clone())
        .tabs(Vec::<SharedString>::new())
        .show_crumbs(false)
        .title_large(true)
        .divided(false)
        .texts(DetailHeaderTexts {
            back: crate::tr("Back"),
            edit_title: crate::tr("Edit title and description"),
            wants_to_merge: crate::tr("wants to merge"),
            into: crate::tr("into"),
        })
        .on_edit({
            let desk = cx.entity();
            move |window, cx| desk.update(cx, |this, cx| this.edit_forge(false, window, cx))
        })
        .top_action(
            Button::new("pr-worktree", crate::tr("Check out in a worktree"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(CarbonIcon::Branch)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_from_pr(folder.clone(), n, title.clone(), false, window, cx);
                })),
        )
        .primary(self.forge_actions(item, cx))
    }

    /// Its tabs: the summary, its code with how many of its files you
    /// viewed, its guide with how many stops.
    fn pr_tabs(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let seen = (f.pr.files.iter()).filter(|file| self.viewed(file)).count();
        let code = (!f.pr.files.is_empty()).then(|| {
            crate::tr_with(
                "%{seen} of %{total} files viewed",
                &[("seen", &seen), ("total", &f.pr.files.len())],
            )
        });
        let stops = self.pr_stops(d).len();
        let guide = (stops > 0).then(|| crate::tr_with("%{n} stops", &[("n", &stops)]));
        let tabs = [
            (Tab::Summary, "Summary", None),
            (Tab::Code, "Code", code),
            (Tab::Guide, "Guide", guide),
        ];
        div()
            .flex()
            .h(px(36.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .children(tabs.into_iter().map(|(tab, name, small)| {
                let on = f.tab == tab;
                gpui::base::Button::new(SharedString::from(format!("pr-tab-{name}")))
                    .accessibility_label(crate::tr(name))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .pr(px(16.))
                    .mr(px(16.))
                    .border_b_2()
                    .border_color(if on {
                        theme::role(ColorRole::BorderInteractive)
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.forge.tab = tab;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .type_style(if on {
                                TypeStyle::HeadingCompact01
                            } else {
                                TypeStyle::BodyCompact01
                            })
                            .text_color(theme::role(if on {
                                ColorRole::TextPrimary
                            } else {
                                ColorRole::TextSecondary
                            }))
                            .child(crate::tr(name)),
                    )
                    .children(small.map(|s| {
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(s)
                    }))
            }))
    }

    /// The summary: the guide when it is behind, its words, what happened
    /// on it, the conversations on its lines, and your review.
    fn pr_summary(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let section = |title: SharedString| {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .pt(px(24.))
                .border_t_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    div()
                        .type_style(TypeStyle::HeadingCompact01)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(title),
                )
        };
        let lines = self.forge.pr.lines.len();
        let texts = ReviewFormTexts {
            summary: crate::tr("Review summary"),
            placeholder: crate::tr("Leave a comment"),
            group: crate::tr("Review type"),
            options: [
                (
                    crate::tr("Comment"),
                    crate::tr("Submit general feedback without explicit approval."),
                ),
                (
                    crate::tr("Approve"),
                    crate::tr("Submit feedback and approve merging these changes."),
                ),
                (
                    crate::tr("Request changes"),
                    crate::tr("Submit feedback that must be addressed before merging."),
                ),
            ],
        };
        let desk = cx.entity();
        let review = ReviewForm::new("pr-review")
            .flat(true)
            .texts(texts)
            .submit_label(if lines > 0 {
                crate::tr_with("Submit review with %{lines} on lines", &[("lines", &lines)])
            } else {
                crate::tr("Submit review")
            })
            .hint(crate::tr(
                "A comment needs a summary or a line comment. Approve can be sent empty.",
            ))
            .require_content_unless_approve(true)
            .has_line_comments(lines > 0)
            .on_submit_review(move |verdict, said, _, cx| {
                desk.update(cx, |d, cx| d.review_forge(verdict, &said, cx));
            });
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .children(self.pr_guide_behind(d, cx))
            .child(if d.body.trim().is_empty() {
                ui::fine(faces, crate::tr("No description.")).into_any_element()
            } else {
                markdown::render(faces, "forge-body", &d.body, cx).into_any_element()
            })
            .child(section(crate::tr("Activity")).child(Self::pr_timeline(d, cx)))
            .when(!d.threads.is_empty(), |el| {
                el.child(
                    section(crate::tr_with(
                        "Conversations on its lines · %{n}",
                        &[("n", &d.threads.len())],
                    ))
                    .child(self.pr_threads(d, cx)),
                )
            })
            .child(section(crate::tr("Your review")).child(review))
    }

    /// What happened on it, first to last: its commits, what was said and
    /// each review's verdict.
    fn pr_timeline(d: &Detail, cx: &mut Context<Self>) -> AnyElement {
        // Each with when it was, and what was said, for the thread on
        // screen.
        let mut events: Vec<(&str, TimelineEvent, Option<String>)> = (d.commits.iter())
            .map(|c| {
                let e = TimelineEvent::new(EventKind::Commit, c.who.clone(), ago(&c.when))
                    .text(c.headline.clone());
                (c.when.as_str(), e, None)
            })
            .collect();
        // A review of only comments on lines says nothing here: they are
        // its conversations.
        let said = (d.said.iter()).filter(|s| !(s.how == "commented" && s.body.trim().is_empty()));
        let faces = crate::fonts::faces(cx).clone();
        for (at, s) in said.enumerate() {
            let verdict = super::forge::decision(&s.how).filter(|v| *v != ReviewDecision::Awaiting);
            let e = match verdict {
                Some(v) => {
                    TimelineEvent::new(EventKind::Review, s.who.clone(), ago(&s.when)).review(v)
                }
                None => TimelineEvent::new(EventKind::Comment, s.who.clone(), ago(&s.when)),
            }
            .content(markdown::render(
                &faces,
                &format!("pr-said-{at}"),
                &s.body,
                cx,
            ));
            let e = if s.reactions.is_empty() {
                e
            } else {
                e.reactions(
                    s.reactions
                        .iter()
                        .map(|(e, n)| Reaction::new(e.clone(), *n)),
                )
            };
            events.push((s.when.as_str(), e, Some(format!("{}: {}", s.who, s.body))));
        }
        if events.is_empty() {
            return ui::fine(crate::fonts::faces(cx), crate::tr("Nothing said yet."))
                .into_any_element();
        }
        events.sort_by(|a, b| a.0.cmp(b.0));
        let quoted: Vec<Option<String>> = events.iter().map(|(_, _, q)| q.clone()).collect();
        let desk = cx.entity();
        PullRequestTimeline::new("pr-timeline")
            .add_label(crate::tr("Add to chat"))
            .events(events.into_iter().map(|(_, e, _)| e))
            .on_add_to_thread(move |at, window, cx| {
                let Some(Some(said)) = quoted.get(at).cloned() else {
                    return;
                };
                desk.update(cx, |d, cx| d.put_line(&said, window, cx));
            })
            .into_any_element()
    }

    /// Send your review: a comment where nothing else goes with it, else a
    /// review with its verdict and the comments on lines.
    pub(crate) fn review_forge(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        cx: &mut Context<Self>,
    ) {
        let lines = std::mem::take(&mut self.forge.pr.lines);
        let body = body.trim().to_owned();
        let act = match (verdict, lines.is_empty()) {
            (ReviewVerdict::Comment, true) if body.is_empty() => return,
            (ReviewVerdict::Comment, true) => Act::Comment(body),
            (v, _) => Act::Review {
                verdict: match v {
                    ReviewVerdict::Approve => "approve",
                    ReviewVerdict::Changes => "request-changes",
                    ReviewVerdict::Comment => "comment",
                }
                .to_owned(),
                body,
                lines,
            },
        };
        self.act_forge(act, cx);
    }

    /// Its changes: each file's head with a Viewed box, its conversations
    /// and the comments to go out with the next review under their lines,
    /// a line's + to comment on it.
    pub(super) fn pr_code(&self, d: &Detail, cx: &mut Context<Self>) -> AnyElement {
        let desk = cx.entity();
        let r = &self.forge.pr;
        let at = |path: &str, line: u64| {
            DiffLinePosition::new(path.to_owned(), DiffSide::Modified, line as usize)
        };
        let mut notes: Vec<DiffAnnotation> = (d.threads.iter())
            .filter_map(|t| {
                let line = t.line?;
                Some(DiffAnnotation::line(
                    SharedString::from(format!("t:{}", t.id)),
                    at(&t.path, line),
                ))
            })
            .chain(r.lines.iter().map(|l| {
                DiffAnnotation::line(
                    SharedString::from(format!("l:{}:{}", l.path, l.line)),
                    at(&l.path, l.line),
                )
            }))
            .collect();
        if let Some((path, line)) = &r.commenting {
            notes.push(DiffAnnotation::line("saying", at(path, *line)));
        }
        let faces = crate::fonts::faces(cx).clone();
        let (threads, lines, field) = (d.threads.clone(), r.lines.clone(), r.note.clone());
        let viewed: Vec<String> = (r.files.iter())
            .filter(|f| self.viewed(f))
            .map(|f| f.path.clone())
            .collect();
        Diff::new(&self.forge.diff)
            .size_full()
            .soft_wrap(self.settings.diff_wrap)
            .annotations(notes)
            .render_annotation({
                let desk = desk.clone();
                move |note, _, _| -> AnyElement {
                    let id = note.id().to_string();
                    if id == "saying" {
                        return div()
                            .px(px(12.))
                            .py(px(6.))
                            .child(Input::new(&field).small())
                            .into_any_element();
                    }
                    if let Some(t) = id
                        .strip_prefix("t:")
                        .and_then(|t| threads.iter().find(|x| x.id == t))
                    {
                        return said_box(
                            &faces,
                            t.said.first().map_or("", |s| s.who.as_str()),
                            t.said.first().map_or("", |s| s.body.as_str()),
                            t.said.len().saturating_sub(1),
                        )
                        .when(t.resolved, |d| d.opacity(0.6))
                        .into_any_element();
                    }
                    let Some(l) = note.position().and_then(|p| {
                        lines
                            .iter()
                            .find(|l| l.path == p.path().as_ref() && l.line == p.line() as u64)
                    }) else {
                        return div().into_any_element();
                    };
                    let (path, line, desk) = (l.path.clone(), l.line, desk.clone());
                    said_box(&faces, &crate::tr("You, next review"), &l.body, 0)
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("unsay-{path}-{line}")),
                                "x",
                                crate::tr("Take it back"),
                            )
                            .size(px(20.))
                            .on_click(move |_, _, cx| {
                                desk.update(cx, |d, cx| d.drop_line(&path, line, cx));
                            }),
                        )
                        .into_any_element()
                }
            })
            .on_add_annotation({
                let desk = desk.clone();
                move |range, window, cx| {
                    let (path, line) = (range.path().to_string(), range.end() as u64);
                    desk.update(cx, |d, cx| d.begin_line(path, line, window, cx));
                }
            })
            .render_header_suffix(move |file, _, _| {
                let path = file.path().to_string();
                let on = viewed.contains(&path);
                let desk = desk.clone();
                Checkbox::new(SharedString::from(format!("viewed-{path}")))
                    .small()
                    .label(crate::tr("Viewed"))
                    .checked(on)
                    .on_click(move |_: &bool, _, cx: &mut App| {
                        desk.update(cx, |d, cx| d.toggle_viewed(&path, cx));
                    })
            })
            .into_any_element()
    }

    /// Its conversations on lines: where each is, what was said, answered
    /// with Enter, resolved or opened again, or taken to the thread on
    /// screen; those still open first, the resolved and the bots' behind
    /// a count.
    fn pr_threads(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let all = self.forge.pr.all_threads;
        let (mut threads, hidden): (Vec<_>, Vec<_>) =
            (d.threads.iter()).partition(|t| all || !(t.resolved || t.bots));
        threads.sort_by_key(|t| t.resolved);
        let desk = cx.entity();
        let mut list = div().flex().flex_col().gap(px(12.));
        for t in threads {
            let place = match t.line {
                Some(line) => format!("{}:{line}", t.path),
                None => {
                    crate::tr_with("%{path} (its line is gone)", &[("path", &t.path)]).to_string()
                }
            };
            let comments = t.said.iter().enumerate().map(|(n, s)| {
                let c = ReviewComment::new(s.who.clone(), ago(&s.when), s.body.clone());
                if n == 0 { c.note(place.clone()) } else { c }
            });
            let (id, resolved) = (t.id.clone(), t.resolved);
            let quoted = (t.said.first())
                .map(|s| format!("{place}: {}", s.body.lines().next().unwrap_or_default()));
            list = list.child(
                ReviewThreadCard::new(SharedString::from(format!("pr-thread-{}", t.id)))
                    .comments(comments.collect::<Vec<_>>())
                    .resolved(resolved)
                    .texts(ReviewThreadTexts {
                        reply: crate::tr("Reply…"),
                        reply_label: crate::tr("Reply"),
                        resolve: crate::tr("Resolve conversation"),
                        unresolve: crate::tr("Unresolve"),
                        add_to_chat: crate::tr("Add to chat"),
                        ..ReviewThreadTexts::default()
                    })
                    .on_reply({
                        let (desk, id) = (desk.clone(), id.clone());
                        move |said, _, cx| {
                            let act = Act::Reply {
                                thread: id.clone(),
                                body: said.to_string(),
                            };
                            desk.update(cx, |d, cx| d.act_forge(act, cx));
                        }
                    })
                    .on_resolve({
                        let (desk, id) = (desk.clone(), id.clone());
                        move |_, cx| {
                            let act = Act::Resolve {
                                thread: id.clone(),
                                resolved: !resolved,
                            };
                            desk.update(cx, |d, cx| d.act_forge(act, cx));
                        }
                    })
                    .on_add_to_chat({
                        let desk = desk.clone();
                        move |window, cx| {
                            let Some(quoted) = quoted.clone() else { return };
                            desk.update(cx, |d, cx| d.put_line(&quoted, window, cx));
                        }
                    }),
            );
        }
        if !hidden.is_empty() || all {
            let label = if all {
                crate::tr("Hide the resolved and the bots'")
            } else {
                crate::tr_with("Show %{n} hidden", &[("n", &hidden.len())])
            };
            list = list.child(
                div().child(
                    Button::new("pr-hidden", label)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.forge.pr.all_threads = !this.forge.pr.all_threads;
                            cx.notify();
                        })),
                ),
            );
        }
        list
    }

    /// The stops of the guide written for the pull request read, as it
    /// reads them: none while there is none.
    pub(super) fn pr_stops(&self, d: &Detail) -> Vec<crate::desk::pr::GuideStopData> {
        match &self.forge.pr.guide {
            Some((n, _, Some(Ok(text)))) if *n == d.item.number => {
                crate::desk::pr::parse_guide_stops(text)
            }
            _ => Vec::new(),
        }
    }

    /// A guide asked of the writer, or why one is not there yet.
    pub(super) fn pr_guide_ask(&self, d: &Detail, cx: &mut Context<Self>) -> AnyElement {
        let guide = (self.forge.pr.guide.as_ref()).filter(|g| g.0 == d.item.number);
        let ask = |label: &'static str| {
            Button::new("write-guide", crate::tr(label))
                .variant(ButtonVariant::Tertiary)
                .size(ButtonSize::Sm)
                .on_click(cx.listener(|this, _, _, cx| this.write_guide(cx)))
        };
        let faces = crate::fonts::faces(cx).clone();
        match guide {
            Some((_, _, None)) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(ui::live_dot())
                .child(ui::fine(
                    &faces,
                    crate::tr("The writer is reading the changes…"),
                ))
                .into_any_element(),
            Some((_, _, Some(Err(e)))) => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::trouble(
                    "guide-unwritten",
                    crate::tr_with("The writer couldn't write it: %{e}", &[("e", e)]),
                ))
                .child(div().child(ask("Try again")))
                .into_any_element(),
            _ => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::fine(
                    &faces,
                    crate::tr("The writer orders the changes for reading, with what to check."),
                ))
                .child(div().child(ask("Write a review guide")))
                .into_any_element(),
        }
    }

    /// Where the guide was written before its latest commits: so said,
    /// with the stop you were at and the guide written again.
    fn pr_guide_behind(&self, d: &Detail, cx: &mut Context<Self>) -> Option<Div> {
        let (n, head, _) = self.forge.pr.guide.as_ref()?;
        let stops = self.pr_stops(d);
        if *n != d.item.number || *head == d.head || stops.is_empty() {
            return None;
        }
        let at = self.forge.pr.stop.min(stops.len() - 1) + 1;
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .pl(px(16.))
                .pr(px(8.))
                .py(px(8.))
                .bg(theme::role(ColorRole::Layer01))
                .border_l_3()
                .border_color(theme::role(ColorRole::SupportWarning))
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
                                .child(crate::tr("The review guide is behind its latest commits")),
                        )
                        .child(
                            div()
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(crate::tr_with("You were at stop %{n}", &[("n", &at)])),
                        ),
                )
                .child(
                    Button::new(
                        "guide-resume",
                        crate::tr_with("Resume at stop %{n}", &[("n", &at)]),
                    )
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forge.tab = Tab::Guide;
                        cx.notify();
                    })),
                )
                .child(
                    Button::new("guide-refresh", crate::tr("Refresh guide"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|this, _, _, cx| this.write_guide(cx))),
                ),
        )
    }

    /// The guide to reading it, which the writer writes when asked: the
    /// stop you are at, the way to the next and the last, and the whole
    /// of it.
    fn pr_guide(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let body = div().flex().flex_col().gap(px(16.));
        let text = match &self.forge.pr.guide {
            Some((n, _, Some(Ok(text)))) if *n == d.item.number => text.clone(),
            _ => return body.child(self.pr_guide_ask(d, cx)),
        };
        let stops = self.pr_stops(d);
        let body = body.children(self.pr_guide_behind(d, cx));
        if stops.is_empty() {
            return body.child(markdown::render(faces, "pr-guide", &text, cx));
        }
        let at = self.forge.pr.stop.min(stops.len() - 1);
        let stop = &stops[at];
        let go = |id: &'static str, label: &'static str, to: Option<usize>| {
            Button::new(id, crate::tr(label))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .disabled(to.is_none())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(to) = to {
                        this.forge.pr.stop = to;
                        cx.notify();
                    }
                }))
        };
        let file = stop.file.clone();
        body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(16.))
                .bg(theme::role(ColorRole::Layer01))
                .child(
                    div()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(crate::tr_with(
                            "Stop %{at} of %{n}",
                            &[("at", &(at + 1)), ("n", &stops.len())],
                        )),
                )
                .child(
                    div()
                        .type_style(TypeStyle::HeadingCompact02)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(stop.title.clone()),
                )
                .when(!stop.note.is_empty(), |el| {
                    el.child(
                        div()
                            .type_style(TypeStyle::Body01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(stop.note.clone()),
                    )
                })
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(go("guide-back", "Previous stop", at.checked_sub(1)))
                        .child(go(
                            "guide-next",
                            "Next stop",
                            (at + 1 < stops.len()).then_some(at + 1),
                        ))
                        .children(file.map(|_| {
                            Button::new("guide-code", crate::tr("Its code"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::Code)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.forge.tab = Tab::Code;
                                    cx.notify();
                                }))
                        })),
                ),
        )
        .child(markdown::render(faces, "pr-guide", &text, cx))
    }
}

/// What was said on a line, by whom, on one line under it (a diff's note
/// is one line high); and how many more answered.
fn said_box(faces: &Faces, who: &str, body: &str, more: usize) -> Div {
    let first = body.lines().next().unwrap_or_default();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mx(px(12.))
        .my(px(4.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(8.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border2())
        .child(ui::fine(faces, who.to_owned()).flex_none())
        .child(
            ui::one_line(ui::text(faces, first.to_owned()).text_size(px(crate::style::CAPTION)))
                .flex_1(),
        )
        .when(more > 0, |d| d.child(ui::fine(faces, format!("+{more}"))))
}

/// How much of it is code, tests, mocks, generated or docs, as lines added
/// and removed, t3code's change breakdown.
pub fn breakdown(files: &[review::File]) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .children(
            review::breakdown(files)
                .into_iter()
                .map(|(part, added, removed)| {
                    Tag::secondary().small().child(crate::tr_with(
                        "%{part} +%{added} −%{removed}",
                        &[
                            ("part", &crate::tr(part.name())),
                            ("added", &added),
                            ("removed", &removed),
                        ],
                    ))
                }),
        )
}

/// The pull requests its branch is stacked on, and those stacked on it,
/// among those listed: by number.
pub fn stack(item: &Item, listed: &[Item]) -> (Option<u64>, Vec<u64>) {
    let under = (listed.iter())
        .find(|i| i.number != item.number && !item.base.is_empty() && i.branch == item.base)
        .map(|i| i.number);
    let over = (listed.iter())
        .filter(|i| i.number != item.number && !item.branch.is_empty() && i.base == item.branch)
        .map(|i| i.number)
        .collect();
    (under, over)
}
