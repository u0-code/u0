//! Answering what her work asks, and stopping it: the controls the right
//! column and Attention share.
//!
//! A piece of work waiting on you shows its question. When the work said
//! what to choose from, the options are buttons, and one click answers;
//! otherwise there is a box to answer in, and for something only you can
//! do, Done. Every open piece can be stopped, which asks once more before
//! it does. Each goes to her daemon as `work.answer` or `work.stop`, from
//! whoever sits at the desk, and comes back in her work list and her
//! journal like anything she does.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use ghost_link::protocol::{AskKind, Work};
use gpui::{Context, Div, Entity, SharedString, Stateful, div, prelude::*, px};

use crate::{
    daemons::Host,
    desk::Desk,
    fonts::Faces,
    input::{Submitted, TextInput},
    link::Order,
    theme, thread, ui,
};

/// Whose work: which link an answer or a stop goes by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Whom {
    /// The desk's own ghost, on the desk's link to it.
    Solver,
    /// The character open on screen, on hers.
    Her,
    /// A character a daemon runs, by her id, on a short connection of its
    /// own: from Attention.
    Listed { host: Host, ghost: String },
}

impl Whom {
    /// A key for the controls of the work `work` of hers.
    fn key(&self, work: &str) -> String {
        match self {
            Self::Solver => format!("solver|{work}"),
            Self::Her => format!("her|{work}"),
            Self::Listed { host, ghost } => format!("{}/{ghost}|{work}", host.key()),
        }
    }
}

/// A piece of work as the controls need it.
pub(crate) struct Ask<'a> {
    pub whom: Whom,
    pub work: &'a str,
    /// What it asks, while it waits on an answer.
    pub question: Option<&'a str>,
    pub kind: Option<AskKind>,
    pub options: &'a [String],
    /// Whether it is still open, and so can be stopped.
    pub open: bool,
}

impl<'a> Ask<'a> {
    /// The controls of a piece of work her daemon lists.
    pub fn of(whom: Whom, w: &'a Work) -> Self {
        Self {
            whom,
            work: &w.id,
            question: w.question.as_deref(),
            kind: w.kind,
            options: &w.options,
            open: !w.state.ended(),
        }
    }
}

/// A small outlined button.
fn chip(faces: &Faces, id: String, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .flex()
        .flex_none()
        .items_center()
        .h(px(24.))
        .px(px(9.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme::border2())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(
            ui::text(faces, text)
                .text_size(px(12.5))
                .text_color(theme::text2()),
        )
}

/// Every reply box that should be: one for each piece of work that waits on
/// an answer, wherever it shows, by its key.
pub(crate) fn wanted<'a>(
    asks: impl IntoIterator<Item = Ask<'a>>,
) -> HashSet<(String, Whom, String)> {
    asks.into_iter()
        .filter(|a| a.question.is_some())
        .map(|a| (a.whom.key(a.work), a.whom.clone(), a.work.to_owned()))
        .collect()
}

impl Desk {
    /// Do `order` to whoever `whom` is.
    pub(crate) fn order(&mut self, whom: &Whom, order: Order, cx: &mut Context<Self>) {
        match whom {
            Whom::Solver => self.solver.order(order),
            Whom::Her => {
                if let Some(open) = &self.open {
                    open.her.order(order);
                }
            }
            Whom::Listed { host, ghost } => {
                let who = self.settings.who.clone();
                if let Some(d) = self.daemons.iter().find(|d| d.host == *host) {
                    d.order(ghost, &who, order);
                }
            }
        }
        cx.notify();
    }

    /// Keep a reply box for each piece of work that waits on an answer
    /// where the desk shows it, and only for those.
    pub(crate) fn sync_replies(&mut self, cx: &mut Context<Self>) {
        let mut asks: Vec<Ask<'_>> = self
            .solver
            .work
            .iter()
            .map(|w| Ask::of(Whom::Solver, w))
            .collect();
        if let Some(open) = &self.open {
            asks.extend(open.her.work.iter().map(|w| Ask::of(Whom::Her, w)));
        }
        asks.extend(self.attention_asks());
        let wanted = wanted(asks);
        let keys: HashSet<&String> = wanted.iter().map(|(k, _, _)| k).collect();
        self.replies.retain(|k, _| keys.contains(k));
        for (key, whom, work) in wanted {
            if self.replies.contains_key(&key) {
                continue;
            }
            let field = cx.new(|cx| TextInput::new(cx, "Answer").field().wrapping(3));
            cx.subscribe(&field, move |this, field, Submitted(text), cx| {
                let order = Order::Answer {
                    work: work.clone(),
                    text: text.clone(),
                };
                this.order(&whom, order, cx);
                field.update(cx, |f, cx| f.set_text("", cx));
            })
            .detach();
            self.replies.insert(key, field);
        }
    }

    /// What a piece of work asks, and what can be done about it: the
    /// options, or a box to answer in; and Stop while it is open.
    pub(crate) fn ask_controls(&self, faces: &Faces, ask: &Ask<'_>, cx: &mut Context<Self>) -> Div {
        let key = ask.whom.key(ask.work);
        let mut body = div().flex().flex_col().gap(px(6.));
        if let Some(question) = ask.question {
            body = body.child(
                ui::text(faces, question.to_owned())
                    .text_size(px(12.5))
                    .line_height(px(18.))
                    .text_color(theme::purple()),
            );
            if ask.kind == Some(AskKind::Choose) && !ask.options.is_empty() {
                let mut options = div().flex().flex_wrap().gap(px(6.));
                for (i, option) in ask.options.iter().enumerate() {
                    let (whom, work, text) =
                        (ask.whom.clone(), ask.work.to_owned(), option.clone());
                    options = options.child(
                        chip(faces, format!("{key}:option:{i}"), option.clone()).on_click(
                            cx.listener(move |this, _, _, cx| {
                                let order = Order::Answer {
                                    work: work.clone(),
                                    text: text.clone(),
                                };
                                this.order(&whom, order, cx);
                            }),
                        ),
                    );
                }
                body = body.child(options);
            } else if let Some(field) = self.replies.get(&key) {
                let send = field.clone();
                let mut reply = div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .bg(theme::bg())
                            .border_1()
                            .border_color(theme::border2())
                            .child(field.clone()),
                    )
                    .child(
                        chip(faces, format!("{key}:send"), "Answer")
                            .mt(px(2.))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                send.update(cx, TextInput::send);
                            })),
                    );
                // Something only you can do is answered by saying it is done.
                if ask.kind == Some(AskKind::Act) {
                    let (whom, work) = (ask.whom.clone(), ask.work.to_owned());
                    reply = reply.child(
                        chip(faces, format!("{key}:done"), "Done")
                            .mt(px(2.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let order = Order::Answer {
                                    work: work.clone(),
                                    text: "Done.".to_owned(),
                                };
                                this.order(&whom, order, cx);
                            })),
                    );
                }
                body = body.child(reply);
            }
        }
        if ask.open {
            body = body.child(self.stop_control(faces, ask, &key, cx));
        }
        body
    }

    /// Stop, which asks once more before it stops anything.
    fn stop_control(&self, faces: &Faces, ask: &Ask<'_>, key: &str, cx: &mut Context<Self>) -> Div {
        let armed = format!("stop:{key}");
        let quiet = |id: String, text: &'static str, color: gpui::Hsla| {
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .h(px(22.))
                .px(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::text(faces, text).text_size(px(12.)).text_color(color))
        };
        let row = div().flex().items_center().gap(px(4.));
        if !self.expanded.contains(&armed) {
            let toggle = armed.clone();
            return row.child(
                quiet(format!("{key}:stop"), "Stop", theme::muted())
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
            );
        }
        let (whom, work, keep) = (ask.whom.clone(), ask.work.to_owned(), armed.clone());
        row.child(
            quiet(format!("{key}:stop-yes"), "Stop it", theme::coral()).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.expanded.remove(&armed);
                    this.order(&whom, Order::Stop { work: work.clone() }, cx);
                },
            )),
        )
        .child(
            quiet(format!("{key}:stop-no"), "Keep going", theme::muted())
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&keep, cx))),
        )
    }

    /// Her work as her daemon keeps it, most recently active first: five,
    /// then the rest when asked for; each with what it asks and Stop.
    pub(crate) fn her_work(&self, faces: &Faces, work: &[Work], cx: &mut Context<Self>) -> Div {
        use crate::aside::{
            ended_row, ended_summary, glyph, open_standing, running_lead, state_of, task_row,
        };
        if work.is_empty() {
            return div().child(
                ui::text(faces, "Nothing running in the background.")
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .py(px(4.)),
            );
        }
        let when = |t: &Option<String>| t.as_deref().and_then(|t| t.parse::<DateTime<Utc>>().ok());
        let mut list = div().flex().flex_col();
        for w in work.iter().filter(|w| state_of(w.state).open()) {
            let (right, color) = open_standing(state_of(w.state), when(&w.started));
            list = list.child(task_row(
                faces,
                SharedString::from(format!("her-work-{}", w.id)),
                running_lead(),
                w.brief.clone(),
                right,
                color,
            ));
            let controls = self.ask_controls(faces, &Ask::of(Whom::Her, w), cx);
            list = list.child(controls.pl(px(26.)).pb(px(4.)));
        }
        let ended: Vec<&Work> = work.iter().filter(|w| !state_of(w.state).open()).collect();
        if ended.is_empty() {
            return list;
        }
        let count =
            |state: thread::State| ended.iter().filter(|w| state_of(w.state) == state).count();
        let summary = ended_summary(
            count(thread::State::Finished),
            count(thread::State::Blocked),
            count(thread::State::Dropped),
        );
        let key = "more:her".to_owned();
        let open = self.expanded.contains(&key);
        list = list.child(ended_row(faces, "her-work-ended", summary, open, cx, key));
        if open {
            for w in ended {
                let ago = when(&w.last).map_or_else(String::new, |t| {
                    format!("{} ago", thread::span((Utc::now() - t).num_seconds()))
                });
                list = list.child(
                    task_row(
                        faces,
                        SharedString::from(format!("her-work-{}", w.id)),
                        glyph(state_of(w.state)).into_any_element(),
                        w.brief.clone(),
                        ago,
                        theme::faint(),
                    )
                    .pl(px(4.)),
                );
            }
        }
        list
    }
}

/// The reply boxes the desk holds, by the key of the work they answer.
pub(crate) type Replies = HashMap<String, Entity<TextInput>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(whom: Whom, work: &'static str, question: Option<&'static str>) -> Ask<'static> {
        Ask {
            whom,
            work,
            question,
            kind: None,
            options: &[],
            open: true,
        }
    }

    #[test]
    fn a_reply_box_is_kept_for_each_waiting_piece_wherever_it_shows() {
        let listed = Whom::Listed {
            host: Host::Remote("hk".into()),
            ghost: "weiwei".into(),
        };
        let wanted = wanted([
            ask(Whom::Her, "w1", Some("which week?")),
            ask(Whom::Her, "w2", None),
            ask(listed.clone(), "w1", Some("which week?")),
        ]);
        let mut keys: Vec<&str> = wanted.iter().map(|(k, _, _)| k.as_str()).collect();
        keys.sort_unstable();
        // The same work shown in two places has two boxes; work that asks
        // nothing has none.
        assert_eq!(keys, ["group:remote:hk/weiwei|w1", "her|w1"]);
    }
}
