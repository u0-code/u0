//! A subagent as its parent's thread shows it: u0ui's subagent row (its
//! kind, what it was set to do, how it stands, how long it took, its chat
//! a click away, Stop while it works), its calls under one fold as a
//! turn's are, and what it answered. Its whole transcript is its own
//! session, opened from the row.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use u0ui::t3::timeline::{SubagentRow, SubagentStatus, SubagentTexts};

use super::fold;
use crate::{
    desk::Desk,
    fonts::Faces,
    markdown,
    session::{End, Item, Session},
    theme, ui,
};

/// How the subagent `c` stands, in the design's words for it.
pub(crate) fn status(c: &Session) -> SubagentStatus {
    if c.sub.needs_you {
        SubagentStatus::Waiting
    } else if c.sub.working() {
        SubagentStatus::Running
    } else if matches!(c.state, crate::session::State::Broken(_)) {
        SubagentStatus::Failed
    } else {
        match c.sub.end {
            Some(End::Failed) => SubagentStatus::Failed,
            Some(End::Stopped) => SubagentStatus::Stopped,
            // One kept from before, whose end nothing said, was done.
            Some(End::Done) | None => SubagentStatus::Completed,
        }
    }
}

/// A subagent's mark where it is one line of a list (a tab, the column's
/// rows): its status dot in the 16 a mark takes.
pub(crate) fn mark(c: &Session) -> gpui::Div {
    div()
        .size(px(16.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(u0ui::t3::timeline::SubagentStatusDot::new(
            SharedString::from(format!("sub-dot-{}", c.key)),
            status(c),
        ))
}

/// The row's words, in your language.
fn texts() -> SubagentTexts {
    SubagentTexts {
        open: crate::tr("Open chat"),
        stop: crate::tr("Stop it"),
        label: std::rc::Rc::new(|s| {
            crate::tr(match s {
                SubagentStatus::Running => "Running",
                SubagentStatus::Queued => "Queued",
                SubagentStatus::Waiting => "Needs you",
                SubagentStatus::Idle => "Idle",
                SubagentStatus::Completed => "Done",
                SubagentStatus::Failed => "Failed",
                SubagentStatus::Stopped => "Stopped",
            })
        }),
    }
}

impl Desk {
    /// The card of the subagent at `i` of the session `s`. What it was
    /// told, its calls and its answer are its session's, when the desk
    /// has read them; its answer may be its parent's too, sent back to it.
    pub(super) fn subagent(
        &self,
        faces: &Faces,
        s: &Session,
        i: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(Item::Child {
            id,
            title,
            role,
            task,
            answer,
        }) = s.items.get(i)
        else {
            return div().into_any_element();
        };
        let key = format!("{}-{i}", s.key);
        let child = (self.sessions.all.iter())
            .find(|c| c.parent == Some(s.key) && c.meta.id.as_ref() == Some(id));
        let state = child.map_or(SubagentStatus::Completed, status);
        let at = child.map(|c| c.key);
        let mut row = SubagentRow::new(
            SharedString::from(format!("{key}-row")),
            role.clone()
                .unwrap_or_else(|| crate::tr("Subagent").to_string()),
            title.clone(),
        )
        .status(state)
        .texts(texts());
        // A time nobody said is none, not "0s".
        if let Some(took) = child.and_then(|c| c.sub.took()) {
            row = row.elapsed(took.as_secs());
        }
        if let Some(at) = at {
            let desk = cx.entity();
            row = row.on_open(move |window, cx| {
                desk.update(cx, |this, cx| this.show(at, window, cx));
            });
            if child.is_some_and(|c| c.sub.stoppable) {
                let desk = cx.entity();
                row = row.on_stop(move |_, cx| desk.update(cx, |this, cx| this.stop_sub(at, cx)));
            }
        }
        // Its calls, folded as a turn's work is; under them, open, what
        // it was told and each call.
        let calls: Vec<usize> = child.map_or_else(Vec::new, |c| {
            (c.items.iter().enumerate())
                .filter(|(_, x)| matches!(x, Item::Tool(_) | Item::Child { .. }))
                .map(|(j, _)| j)
                .collect()
        });
        let fold_key = format!("{key}-work");
        let open = self.expanded.contains(&fold_key);
        let work = child.filter(|_| !calls.is_empty()).map(|c| {
            let tools: Vec<Item> = calls.iter().map(|&j| c.items[j].clone()).collect();
            let mut words = fold::summary(&tools);
            if words.is_empty() {
                words = crate::tr("Worked").to_string();
            }
            let mut col = div().flex().flex_col().gap(px(4.)).child(Self::fold_row(
                faces,
                fold_key.clone(),
                &words,
                None,
                open,
                cx,
            ));
            if open {
                if let Some(task) = task {
                    col = col.child(
                        ui::text(faces, task.clone())
                            .px(px(10.))
                            .text_size(px(crate::style::LABEL))
                            .text_color(theme::muted()),
                    );
                }
                for &j in &calls {
                    col = col.child(self.item(faces, c, j, &c.items[j], cx));
                }
            }
            col
        });
        // What it answered: the message it sent back, else the last thing
        // it said, once it is no longer at work.
        let said = answer.clone().or_else(|| {
            let c = child.filter(|c| !c.sub.working())?;
            c.items.iter().rev().find_map(|x| match x {
                Item::Agent(t) => Some(t.clone()),
                _ => None,
            })
        });
        let said = said.map(|t| {
            div()
                .px(px(10.))
                .child(markdown::render(faces, &format!("{key}-answer"), &t, cx))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(row)
            .when(work.is_some() || said.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .ml(px(12.))
                        .pl(px(8.))
                        .border_l_1()
                        .border_color(theme::role(u0ui::tokens::ColorRole::BorderSubtle01))
                        .children(work)
                        .children(said),
                )
            })
            .into_any_element()
    }

    /// What another session told this one: in a subagent, the task its
    /// parent gave it, as a message of yours is drawn, said to be its.
    pub(super) fn told(faces: &Faces, key: &str, text: &str) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(4.))
            .child(
                ui::text(faces, crate::tr("From the agent that set it to work"))
                    .px(px(10.))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::muted()),
            )
            .child(super::you_said(
                faces,
                format!("{key}-told"),
                text.to_owned(),
            ))
            .into_any_element()
    }
}
