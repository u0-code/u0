//! The thread as gpui-kit's `MessageScroller`, a list drawn only where it is
//! seen: a session of a thousand items lays out the few on screen. The
//! list is kept in step with the session's rows, measured again only from
//! the first that changed, and follows its foot while it is read there. Over it, find (⌘F), which brings a row into view, opening what folds
//! it away; ⌃⇧↑ and ⌃⇧↓ go from one thing you said to the next.

use gpui::{
    AnyElement, App, Context, Div, Entity, SharedString, StyleRefinement, Window, div, prelude::*,
    px,
};

use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::{
    Sizable as _,
    message_scroller::{MessageScroller, MessageScrollerState},
    tab::{Tab, TabBar},
};

use super::{fold, step, verb};
use crate::{
    desk::Desk,
    fonts,
    session::{Item, Session, State},
    theme, ui, when,
};

/// Find over the thread (⌘F): whether it is open, its line, and which of
/// what it found is on screen.
pub struct Find {
    pub open: bool,
    pub input: Entity<InputState>,
    pub at: usize,
}

impl Find {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in this session"));
        cx.subscribe(&input, |this, _, e: &InputEvent, cx| match e {
            InputEvent::Change => {
                this.find.at = usize::MAX;
                cx.notify();
            }
            // Return goes on to the next found, ⇧Return back.
            InputEvent::PressEnter { shift, .. } => this.find_next(*shift, cx),
            _ => {}
        })
        .detach();
        Self {
            open: false,
            input,
            at: 0,
        }
    }
}

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

/// Whether `row` of `items` is drawn in the view `view`: thinking only in
/// the thinking and verbose views; a call that went well not in the quiet
/// one; in zen, what you said, its answers and its questions.
fn shown(view: crate::settings::View, items: &[Item], row: &fold::Row) -> bool {
    use crate::settings::View;
    let went_well = |i: usize| {
        matches!(&items[i], Item::Tool(t)
            if t.call.status == agent_client_protocol::v1::schema::ToolCallStatus::Completed)
    };
    match (view, row) {
        (View::Thinking | View::Verbose, _) => true,
        (View::Zen, fold::Row::Item(i)) => matches!(
            items[*i],
            Item::You(_)
                | Item::Agent(_)
                | Item::Asked(_)
                | Item::Note { .. }
                | Item::Image { yours: true, .. }
                | Item::File { .. }
        ),
        (View::Zen, _) => false,
        (View::Quiet, fold::Row::Group { from, to, .. }) => !(*from..*to).all(went_well),
        (_, fold::Row::Item(i)) => {
            let thought = matches!(items[*i], Item::Thought { .. });
            !thought && (view != View::Quiet || !went_well(*i))
        }
        _ => true,
    }
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
            self.settings.view != crate::settings::View::Verbose,
        );
        let rows: Vec<fold::Row> = rows
            .into_iter()
            .filter(|r| shown(self.settings.view, &s.items, r))
            .collect();
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
    /// changed is measured again. A thread read at its foot follows it.
    pub(crate) fn sync_lines(&mut self, cx: &mut App) {
        let lines = self.lines_now();
        let (old, now) = (self.lines.len(), lines.len());
        if self.lines_of == self.current {
            let same = self
                .lines
                .iter()
                .zip(&lines)
                .take_while(|(a, b)| a == b)
                .count();
            if same < old || same < now {
                self.list
                    .update(cx, |l, cx| l.splice(same..old, now - same, cx));
            }
        } else {
            self.lines_of = self.current;
            self.list.update(cx, |l, cx| l.reset(now, cx));
        }
        self.lines = lines;
        if let Some(item) = self.reveal.take()
            && let Some(ix) = self
                .lines
                .iter()
                .position(|l| matches!(l, Line::Row(fold::Row::Item(i)) if *i == item))
        {
            self.list.update(cx, |l, cx| l.scroll_to_item(ix, cx));
        }
    }

    /// Go to the foot of the thread, and follow it.
    pub fn to_bottom(&self, cx: &mut App) {
        self.list.update(cx, MessageScrollerState::scroll_to_end);
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
                self.settings.view != crate::settings::View::Verbose,
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

    /// Bring into view what you said next (`ahead`) or before, from the
    /// line at the top of the thread.
    pub fn step_turn(&mut self, ahead: bool, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let top = self.list.read(cx).logical_scroll_top().item_ix;
        let at = (self.lines.iter().take(top + 1).rev()).find_map(|l| match l {
            Line::Row(
                fold::Row::Item(i)
                | fold::Row::Group { from: i, .. }
                | fold::Row::Turn { from: i, .. },
            ) => Some(*i),
            _ => None,
        });
        let yours = |i: &usize| matches!(s.items[*i], Item::You(_));
        let to = if ahead {
            (at.map_or(0, |a| a + 1)..s.items.len()).find(yours)
        } else {
            (0..at.unwrap_or(0)).rev().find(yours)
        };
        if let Some(to) = to {
            self.reveal_item(to, cx);
        }
    }

    /// The thread as a list, with find over it and the kit's scrollbar
    /// beside it.
    pub(crate) fn thread_list(&self, column: f32, cx: &mut Context<Self>) -> Div {
        let this = cx.entity().downgrade();
        // Rows are spaced by themselves. The way to the end floats over the
        // prose on a raised ground of its own, so text never runs under a
        // bare arrow.
        let list = MessageScroller::new("thread", self.list.clone(), move |ix, window, cx| {
            this.update(cx, |desk, cx| desk.line(ix, column, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .with_list_style(StyleRefinement::default().p_0())
        .with_row_style(StyleRefinement::default().p_0())
        .with_jump_button_label("To the end")
        .with_jump_button_style(
            StyleRefinement::default()
                .bg(theme::panel())
                .border_color(theme::border2())
                .shadow_md(),
        )
        .size_full();
        let faces = fonts::faces(cx).clone();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .children(self.family_tabs(cx))
            .children(self.find.open.then(|| self.find_bar(&faces, cx)))
            .child(div().flex_1().min_h(px(0.)).child(list))
    }

    /// Tabs over the thread: the session's, and one for each subagent at
    /// work or waiting on you, and the one on screen. A subagent's tab goes
    /// as it finishes; × hides it, its work going on (the right column
    /// brings it back). Past the thread's width the tabs scroll
    /// sideways. None while no subagent has a tab.
    fn family_tabs(&self, cx: &mut Context<Self>) -> Option<Div> {
        let s = self.session()?;
        let family = s.parent.unwrap_or(s.key);
        let parent = self.sessions.at(family)?;
        let kids: Vec<&crate::session::Session> = self
            .sessions
            .all
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
            let group = SharedString::from(format!("family-{key}"));
            Tab::new()
                .group(group.clone())
                .label(title.to_owned())
                .prefix(mark)
                .when(closable, |t| {
                    t.suffix(
                        div()
                            .invisible()
                            .group_hover(group, gpui::Styled::visible)
                            .when(key == s.key, gpui::Styled::visible)
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
        };
        let keys: Vec<u64> = std::iter::once(parent.key)
            .chain(kids.iter().map(|c| c.key))
            .collect();
        let chosen = keys.iter().position(|k| *k == s.key);
        let mut bar = TabBar::new("family-tabs")
            .segmented()
            .small()
            .max_width(px(220.))
            .when_some(chosen, TabBar::selected_index)
            .child(tab(
                parent.key,
                div().child(ui::icon("list", 13.).text_color(theme::faint())),
                &parent.meta.title,
                false,
                cx,
            ));
        for c in kids {
            bar = bar.child(tab(c.key, super::sub_mark(&c.sub), &c.meta.title, true, cx));
        }
        Some(
            div()
                .flex_none()
                .w_full()
                .min_w(px(0.))
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme::border())
                .child(
                    bar.on_click(cx.listener(move |this, ix: &usize, window, cx| {
                        if let Some(key) = keys.get(*ix) {
                            this.show(*key, window, cx);
                        }
                    })),
                ),
        )
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
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::link()),
                )
                .child(div().flex_1().h(px(1.)).bg(theme::link().opacity(0.35)))
                .into_any_element(),
            Line::State => match &s.state {
                State::Opening => step()
                    .child(div().mx(px(2.)).child(ui::live_dot()))
                    .child(
                        verb(&faces, format!("Starting {}", self.name_of(&s.meta.agent)))
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
                let mut words = fold::summary(&s.items[*from..*to]);
                if words.is_empty() {
                    "Worked".clone_into(&mut words);
                }
                let took = (*secs).map(|secs| {
                    format!(
                        "The turn took {}",
                        when::took(std::time::Duration::from_secs(secs))
                    )
                });
                Self::fold_row(faces, fold::turn_key(s.key, *from), &words, took, *open, cx)
            }
            fold::Row::Group { from, to, open } => Self::fold_row(
                faces,
                fold::group_key(s.key, *from),
                &fold::summary(&s.items[*from..*to]),
                None,
                *open,
                cx,
            ),
        }
    }

    /// The items of the session on screen that hold what is looked for.
    pub(crate) fn find_hits(&self, cx: &gpui::App) -> Vec<usize> {
        let query = self.find.input.read(cx).value().trim().to_lowercase();
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
        self.find.at = match (self.find.at < hits.len(), back) {
            (false, false) => 0,
            (false, true) => hits.len() - 1,
            (true, false) => (self.find.at + 1) % hits.len(),
            (true, true) => (self.find.at + hits.len() - 1) % hits.len(),
        };
        self.reveal_item(hits[self.find.at], cx);
    }

    /// Open the find bar over the thread, or close it (⌘F).
    pub fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = !self.find.open;
        if self.find.open {
            window.focus(
                &gpui::Focusable::focus_handle(self.find.input.read(cx), cx),
                cx,
            );
        } else {
            window.focus(
                &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
                cx,
            );
        }
        cx.notify();
    }

    fn find_bar(&self, faces: &fonts::Faces, cx: &mut Context<Self>) -> Div {
        let hits = self.find_hits(cx);
        let at = if hits.is_empty() {
            "No match".to_owned()
        } else {
            format!("{} of {}", self.find.at.min(hits.len() - 1) + 1, hits.len())
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
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(Input::new(&self.find.input).appearance(false)),
                )
                .child(
                    ui::text(faces, at)
                        .flex_none()
                        .text_size(px(crate::style::CAPTION))
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
}
