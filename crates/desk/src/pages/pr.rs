//! A pull request reviewed, drawn: its changes with comments on their lines
//! and each file's Viewed box, its conversations, the guide the writer
//! wrote, and how much of it is code, tests, mocks, generated or docs.

use gpui::component::{
    Sizable as _,
    checkbox::Checkbox,
    diff::{Diff, DiffAnnotation, DiffLinePosition, DiffSide},
    input::Input,
    tag::Tag,
};
use gpui::{AnyElement, App, Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::Desk,
    fonts::Faces,
    github::{Detail, Item},
    markdown, review, theme,
    ui::{self, Weight},
};

/// When it was said, as read.
fn ago(text: &str) -> String {
    crate::when::parse(text).map_or_else(String::new, |t| {
        crate::when::ago(t, std::time::SystemTime::now())
    })
}

impl Desk {
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

    /// Its conversations: where each is, what was said, and answering or
    /// resolving it; those still open first.
    pub(super) fn pr_threads(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(12.)).px(px(20.)).py(px(14.));
        if d.threads.is_empty() {
            list = list.child(ui::fine(faces, crate::tr("No conversations on its lines.")));
        }
        // Resolved conversations, and those only bots spoke in, wait behind
        // a count, as t3code's filter keeps them.
        let all = self.forge.pr.all_threads;
        let (mut threads, hidden): (Vec<_>, Vec<_>) =
            (d.threads.iter()).partition(|t| all || !(t.resolved || t.bots));
        threads.sort_by_key(|t| t.resolved);
        if !hidden.is_empty() || all {
            let label = if all {
                crate::tr("Hide the resolved and the bots'")
            } else {
                crate::tr_with("Show %{n} hidden", &[("n", &hidden.len())])
            };
            list =
                list.child(
                    div().child(ui::button("pr-hidden", label, Weight::Quiet).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.forge.pr.all_threads = !this.forge.pr.all_threads;
                            cx.notify();
                        }),
                    )),
                );
        }
        for t in threads {
            let (id, resolved) = (t.id.clone(), t.resolved);
            let place = match t.line {
                Some(line) => format!("{}:{line}", t.path),
                None => {
                    crate::tr_with("%{path} (its line is gone)", &[("path", &t.path)]).to_string()
                }
            };
            let replying = self.forge.pr.replying.as_ref() == Some(&t.id);
            let mut card = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::border())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::one_line(ui::fine_mono(faces, place)).flex_1())
                        .when(resolved, |d| {
                            d.child(Tag::success().small().child(crate::tr("Resolved")))
                        })
                        .when(t.outdated, |d| {
                            d.child(Tag::secondary().small().child(crate::tr("Outdated")))
                        }),
                );
            for (n, s) in t.said.iter().enumerate() {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(ui::fine(faces, format!("{} · {}", s.who, ago(&s.when))))
                        .child(markdown::render(
                            faces,
                            &format!("thread-{}-{n}", t.id),
                            &s.body,
                            cx,
                        )),
                );
            }
            let (a, b) = (id.clone(), id.clone());
            card = card.child(if replying {
                div()
                    .child(Input::new(&self.forge.pr.reply).small())
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::button(
                            SharedString::from(format!("reply-{id}")),
                            crate::tr("Reply"),
                            Weight::Quiet,
                        )
                        .h(px(24.))
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.begin_reply(a.clone(), window, cx),
                        )),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("resolve-{id}")),
                            crate::tr(if resolved { "Reopen" } else { "Resolve" }),
                            Weight::Quiet,
                        )
                        .h(px(24.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let act = crate::github::Act::Resolve {
                                thread: b.clone(),
                                resolved: !resolved,
                            };
                            this.act_forge(act, cx);
                        })),
                    )
                    .into_any_element()
            });
            list = list.child(card);
        }
        list
    }

    /// The guide to reading it, which the writer writes when asked; one
    /// written at another commit is said to be.
    pub(super) fn pr_guide(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let guide = (self.forge.pr.guide.as_ref()).filter(|g| g.0 == d.item.number);
        let ask = |label: &'static str| {
            ui::button("write-guide", crate::tr(label), Weight::Secondary)
                .h(px(26.))
                .on_click(cx.listener(|this, _, _, cx| this.write_guide(cx)))
        };
        let body = div().flex().flex_col().gap(px(10.)).px(px(20.)).py(px(14.));
        match guide {
            None => body
                .child(ui::fine(
                    faces,
                    crate::tr("The writer orders the changes for reading, with what to check."),
                ))
                .child(div().child(ask("Write a review guide"))),
            Some((_, _, None)) => body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::live_dot())
                    .child(ui::fine(
                        faces,
                        crate::tr("The writer is reading the changes…"),
                    )),
            ),
            Some((_, _, Some(Err(e)))) => body
                .child(
                    ui::fine(
                        faces,
                        crate::tr_with("The writer couldn't write it: %{e}", &[("e", e)]),
                    )
                    .text_color(theme::coral()),
                )
                .child(div().child(ask("Try again"))),
            Some((_, head, Some(Ok(text)))) => {
                let stops = crate::desk::pr::parse_guide_stops(text);
                let active_stop = self.forge.pr.stop.min(stops.len().saturating_sub(1));
                body.when(*head != d.head, |b| {
                    b.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::fine(
                                faces,
                                crate::tr("Written before its latest commits."),
                            ))
                            .child(ask("Write it again")),
                    )
                })
                .when(!stops.is_empty(), |b| {
                    b.child(ui::fine(
                        faces,
                        format!("Stop {} of {}", active_stop + 1, stops.len()),
                    ))
                })
                .child(markdown::render(faces, "pr-guide", text, cx))
            }
        }
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
