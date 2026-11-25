//! The thread as a list drawn only where it is seen: a session of a
//! thousand items lays out the few on screen, as t3code virtualizes its
//! rows. The list is kept in step with the session's rows, measured again
//! only from the first that changed, and stays at its foot while new rows
//! come in there. Over it, find (⌘F) and a map of the session's turns down
//! its right edge, both of which bring a row into view, opening what folds
//! it away.

use gpui::{
    AnyElement, Context, Div, ListOffset, SharedString, Window, div, list, prelude::*, px, relative,
};

use super::{fold, step, verb};
use crate::{
    desk::Desk,
    fonts,
    session::{Item, Session, State},
    theme, ui, when,
};

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Who, with what, where: over the first turn.
    Head,
    Row(fold::Row),
    /// Starting, signing in, or stopped: under the last turn.
    State,
    /// Where you stopped reading when you last left it.
    Since,
}

impl Desk {
    /// The lines of the session on screen, now; none for a draft not begun.
    fn lines_now(&self) -> Vec<Line> {
        let Some(s) = self.session() else {
            return Vec::new();
        };
        if s.items.is_empty() && s.state == State::Draft {
            return Vec::new();
        }
        let rows = fold::rows(
            s.key,
            &s.items,
            &self.expanded,
            |i| match &s.items[i] {
                Item::Tool(t) => self.tool_open(s.key, i, t),
                _ => false,
            },
            matches!(s.state, State::Working | State::Opening),
            self.settings.calls == crate::settings::Calls::Folded,
        );
        // Only what was said, if you asked for it: the work is left out.
        let rows: Vec<fold::Row> = if self.settings.calls == crate::settings::Calls::Hidden {
            rows.into_iter()
                .filter(|r| match r {
                    fold::Row::Item(i) => !matches!(
                        s.items[*i],
                        Item::Tool(_) | Item::Thought { .. } | Item::Plan(_)
                    ),
                    _ => false,
                })
                .collect()
        } else {
            rows
        };
        let mut out = vec![Line::Head];
        // What came while you were away is marked where it begins.
        let since = s.seen.filter(|n| *n > 0 && *n < s.items.len());
        let mut marked = false;
        for row in rows {
            if let (Some(n), false) = (since, marked) {
                let at = match &row {
                    fold::Row::Item(i)
                    | fold::Row::Group { from: i, .. }
                    | fold::Row::Turn { from: i, .. } => *i,
                };
                if at >= n {
                    out.push(Line::Since);
                    marked = true;
                }
            }
            out.push(Line::Row(row));
        }
        if matches!(s.state, State::Opening | State::SignIn | State::Broken(_)) {
            out.push(Line::State);
        }
        out
    }

    /// Bring the list in step with the session on screen: another session
    /// starts it afresh; in the same one, what follows the first line that
    /// changed is measured again, and a thread read at its foot stays there.
    pub(crate) fn sync_lines(&mut self) {
        let lines = self.lines_now();
        if self.lines_of == self.current {
            let follow = self.at_foot();
            let same = self
                .lines
                .iter()
                .zip(&lines)
                .take_while(|(a, b)| a == b)
                .count();
            if same < self.lines.len() || same < lines.len() {
                self.list.splice(same..self.lines.len(), lines.len() - same);
                if follow {
                    self.to_bottom();
                }
            }
        } else {
            self.lines_of = self.current;
            self.list.reset(lines.len());
        }
        self.lines = lines;
        if let Some(item) = self.reveal.take()
            && let Some(ix) = self
                .lines
                .iter()
                .position(|l| matches!(l, Line::Row(fold::Row::Item(i)) if *i == item))
        {
            self.list.scroll_to_reveal_item(ix);
        }
    }

    /// Whether the thread is read at its foot, within a line of it.
    fn at_foot(&self) -> bool {
        let below = self.list.max_offset_for_scrollbar().height
            + self.list.scroll_px_offset_for_scrollbar().y;
        below <= px(24.)
    }

    /// Go to the foot of the thread.
    pub fn to_bottom(&self) {
        self.list.scroll_to(ListOffset {
            item_ix: usize::MAX,
            offset_in_item: px(0.),
        });
    }

    /// Bring the item `item` into view, opening the folds it is in.
    pub fn reveal_item(&mut self, item: usize, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let key = s.key;
        // A turn folded away, then a run of calls folded inside it.
        for _ in 0..2 {
            let Some(s) = self.session() else { return };
            let rows = fold::rows(
                key,
                &s.items,
                &self.expanded,
                |i| match &s.items[i] {
                    Item::Tool(t) => self.tool_open(key, i, t),
                    _ => false,
                },
                matches!(s.state, State::Working | State::Opening),
                self.settings.calls == crate::settings::Calls::Folded,
            );
            for row in rows {
                match row {
                    fold::Row::Turn { from, to, open, .. }
                        if !open && (from..to).contains(&item) =>
                    {
                        self.expanded.insert(fold::turn_key(key, from));
                    }
                    fold::Row::Group { from, to, open } if !open && (from..to).contains(&item) => {
                        self.expanded.insert(fold::group_key(key, from));
                    }
                    _ => {}
                }
            }
        }
        self.reveal = Some(item);
        cx.notify();
    }

    /// The thread as a list, with find over it and the map of its turns
    /// beside it.
    pub(crate) fn thread_list(&self, column: f32, cx: &mut Context<Self>) -> Div {
        let this = cx.entity().downgrade();
        let list = list(self.list.clone(), move |ix, window, cx| {
            this.update(cx, |desk, cx| desk.line(ix, column, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        let faces = fonts::faces(cx).clone();
        super::select::new_frame();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .children(self.family_tabs(&faces, cx))
            .children(self.finding.then(|| self.find_bar(&faces, cx)))
            // Words are chosen by pressing on them and dragging.
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(list)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                            this.choosing = super::select::press(e.position);
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                        if this.choosing && e.pressed_button == Some(gpui::MouseButton::Left) {
                            super::select::drag(e.position);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, _| this.choosing = false),
                    ),
            )
            // Read back up the thread: the way down again.
            .when(!self.at_foot(), |d| {
                d.child(
                    div()
                        .absolute()
                        .bottom(px(14.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            ui::icon_button("to-end", "arrow-down", "To the end")
                                .bg(theme::panel())
                                .border_1()
                                .border_color(theme::border2())
                                .rounded_full()
                                .shadow_lg()
                                .text_color(theme::text2())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.to_bottom();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .children(self.session().map(|s| Self::turn_map(s, cx)))
    }

    /// Tabs over the thread: the session's, and one for each subagent at
    /// work or waiting on you, and the one on screen. A subagent's tab goes
    /// as it finishes; × hides it, its work going on (the right column
    /// brings it back). The tabs shrink to the thread's width, then scroll
    /// sideways. None while no subagent has a tab.
    fn family_tabs(
        &self,
        faces: &crate::fonts::Faces,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        let s = self.session()?;
        let family = s.parent.unwrap_or(s.key);
        let parent = self.sessions.iter().find(|p| p.key == family)?;
        let kids: Vec<&crate::session::Session> = self
            .sessions
            .iter()
            .filter(|c| c.parent == Some(family))
            .filter(|c| {
                c.key == s.key
                    || ((c.sub.working() || c.sub.needs_you) && !self.closed_tabs.contains(&c.key))
            })
            .collect();
        if kids.is_empty() {
            return None;
        }
        let tab = |key: u64, mark: Div, title: &str, closable: bool, cx: &mut Context<Self>| {
            let on = key == s.key;
            let group = SharedString::from(format!("family-{key}"));
            div()
                .id(group.clone())
                .group(group.clone())
                .flex()
                .flex_shrink()
                .min_w(px(96.))
                .max_w(px(220.))
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .pl(px(8.))
                .pr(px(if closable { 2. } else { 8. }))
                .rounded(px(7.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme::panel2()))
                .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
                .child(mark)
                .child(
                    ui::text(faces, title.to_owned())
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.5))
                        .text_color(if on { theme::text() } else { theme::text2() }),
                )
                .when(closable, |d| {
                    d.child(
                        div()
                            .invisible()
                            .group_hover(group.clone(), gpui::Styled::visible)
                            .when(on, gpui::Styled::visible)
                            .child(
                                ui::icon_button(
                                    SharedString::from(format!("family-x-{key}")),
                                    "x",
                                    "Hide this tab; it goes on",
                                )
                                .size(px(18.))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.closed_tabs.insert(key);
                                        if this.current == Some(key) {
                                            this.show(family, window, cx);
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| this.show(key, window, cx)))
        };
        let mut bar = div()
            .id("family-tabs")
            .flex()
            .flex_none()
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap(px(2.))
            .px(px(12.))
            .py(px(6.))
            .overflow_x_scroll()
            .border_b_1()
            .border_color(theme::border())
            .child(tab(
                parent.key,
                div().child(ui::icon("list", 13.).text_color(theme::faint())),
                &parent.title,
                false,
                cx,
            ));
        for c in kids {
            bar = bar.child(tab(c.key, super::sub_mark(&c.sub), &c.title, true, cx));
        }
        Some(bar)
    }

    /// The line `ix` of the list, in the reading column.
    fn line(&self, ix: usize, column: f32, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let faces = fonts::faces(cx).clone();
        let (Some(line), Some(s)) = (self.lines.get(ix), self.session()) else {
            return div().into_any_element();
        };
        // A one-line row (a quiet call, a fold) sits right under another;
        // everything else stands 12 apart.
        let one = |l: &Line| match l {
            Line::Row(fold::Row::Item(i)) => match &s.items[*i] {
                Item::Tool(t) => !self.tool_open(s.key, *i, t),
                _ => false,
            },
            Line::Row(fold::Row::Group { .. }) => true,
            _ => false,
        };
        let before = ix.checked_sub(1).and_then(|p| self.lines.get(p));
        let gap = match before {
            None => 28.,
            Some(b) if one(b) && one(line) => 0.,
            Some(_) => 12.,
        };
        let last = ix + 1 == self.lines.len();
        let el: AnyElement = match line {
            Line::Head => self.turn_header(&faces, s, cx).into_any_element(),
            Line::Row(row) => self.row(&faces, s, row, cx),
            Line::Since => div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().flex_1().h(px(1.)).bg(theme::link().opacity(0.35)))
                .child(
                    ui::text(&faces, "New since you last looked")
                        .text_size(px(12.))
                        .text_color(theme::link()),
                )
                .child(div().flex_1().h(px(1.)).bg(theme::link().opacity(0.35)))
                .into_any_element(),
            Line::State => match &s.state {
                State::Opening => step()
                    .child(div().mx(px(2.)).child(ui::live_dot()))
                    .child(
                        verb(&faces, format!("Starting {}", self.name_of(&s.agent)))
                            .text_color(theme::accent()),
                    )
                    .into_any_element(),
                State::SignIn => self.sign_in_card(&faces, s, cx).into_any_element(),
                State::Broken(why) => self.broken(&faces, s, why, cx).into_any_element(),
                _ => div().into_any_element(),
            },
        };
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(column))
                    .pt(px(gap))
                    .when(last, |d| d.pb(px(28.)))
                    .child(el),
            )
            .into_any_element()
    }

    /// One of the session's rows: an item, a folded turn or run of calls.
    fn row(
        &self,
        faces: &fonts::Faces,
        s: &Session,
        row: &fold::Row,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            fold::Row::Item(i) => self.item(faces, s, *i, &s.items[*i], cx),
            fold::Row::Turn {
                from,
                to,
                secs,
                open,
            } => {
                let summary = fold::summary(&s.items[*from..*to]);
                let mut words = match secs {
                    Some(secs) => format!(
                        "Worked for {}",
                        when::took(std::time::Duration::from_secs(*secs))
                    ),
                    None => "Worked".to_owned(),
                };
                if !summary.is_empty() {
                    words = format!("{words} · {summary}");
                }
                Self::fold_row(faces, fold::turn_key(s.key, *from), &words, *open, cx)
            }
            fold::Row::Group { from, to, open } => Self::fold_row(
                faces,
                fold::group_key(s.key, *from),
                &fold::summary(&s.items[*from..*to]),
                *open,
                cx,
            ),
        }
    }

    /// The items of the session on screen that hold what is looked for.
    pub(crate) fn find_hits(&self, cx: &gpui::App) -> Vec<usize> {
        let query = self.find.read(cx).text().trim().to_lowercase();
        let Some(s) = self.session() else {
            return Vec::new();
        };
        if query.is_empty() {
            return Vec::new();
        }
        s.items
            .iter()
            .enumerate()
            .filter(|(_, item)| match item {
                Item::You(t) | Item::Agent(t) | Item::Thought { text: t, .. } => {
                    t.to_lowercase().contains(&query)
                }
                Item::Tool(t) => t.call.title.to_lowercase().contains(&query),
                Item::Note { text, .. } => text.to_lowercase().contains(&query),
                _ => false,
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Go to the next (or, `back`, the one before) item that holds what is
    /// looked for.
    pub fn find_next(&mut self, back: bool, cx: &mut Context<Self>) {
        let hits = self.find_hits(cx);
        if hits.is_empty() {
            return;
        }
        // A new search begins at its first match (or, going back, its last).
        self.found_at = match (self.found_at < hits.len(), back) {
            (false, false) => 0,
            (false, true) => hits.len() - 1,
            (true, false) => (self.found_at + 1) % hits.len(),
            (true, true) => (self.found_at + hits.len() - 1) % hits.len(),
        };
        self.reveal_item(hits[self.found_at], cx);
    }

    /// Open the find bar over the thread, or close it (⌘F).
    pub fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = !self.finding;
        if self.finding {
            window.focus(&gpui::Focusable::focus_handle(self.find.read(cx), cx));
        } else {
            window.focus(&gpui::Focusable::focus_handle(self.input.read(cx), cx));
        }
        cx.notify();
    }

    fn find_bar(&self, faces: &fonts::Faces, cx: &mut Context<Self>) -> Div {
        let hits = self.find_hits(cx);
        let at = if hits.is_empty() {
            "No match".to_owned()
        } else {
            format!(
                "{} of {}",
                self.found_at.min(hits.len() - 1) + 1,
                hits.len()
            )
        };
        div().flex().justify_center().pt(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .w(px(360.))
                .h(px(34.))
                .pl(px(10.))
                .pr(px(4.))
                .rounded(px(10.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .child(ui::icon("search", 14.).text_color(theme::faint()))
                .child(div().flex_1().min_w(px(0.)).child(self.find.clone()))
                .child(
                    ui::text(faces, at)
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(theme::faint()),
                )
                .child(
                    ui::icon_button("find-up", "chevron-left", "Previous · ⇧Return")
                        .size(px(24.))
                        .on_click(cx.listener(|this, _, _, cx| this.find_next(true, cx))),
                )
                .child(
                    ui::icon_button("find-down", "chevron-right", "Next · Return")
                        .size(px(24.))
                        .on_click(cx.listener(|this, _, _, cx| this.find_next(false, cx))),
                )
                .child(
                    ui::icon_button("find-close", "x", "Close · Esc")
                        .size(px(24.))
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_find(window, cx))),
                ),
        )
    }

    /// A map of the session down the thread's right edge: a mark for each
    /// thing you said, where it falls in the session; a click goes there.
    fn turn_map(s: &Session, cx: &mut Context<Self>) -> Div {
        let n = s.items.len().max(1);
        let mut map = div()
            .absolute()
            .top(px(8.))
            .bottom(px(8.))
            .right(px(6.))
            .w(px(10.));
        for (i, item) in s.items.iter().enumerate() {
            let Item::You(text) = item else { continue };
            let at = i as f32 / n as f32;
            map = map.child(
                div()
                    .id(SharedString::from(format!("map-{i}")))
                    .absolute()
                    .top(relative(at))
                    .left(px(1.))
                    .w(px(8.))
                    .h(px(3.))
                    .rounded(px(1.5))
                    .bg(theme::faint())
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::text()))
                    .tooltip(ui::tip(ui::fit(text, 60)))
                    .on_click(cx.listener(move |this, _, _, cx| this.reveal_item(i, cx))),
            );
        }
        map
    }
}
