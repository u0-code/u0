//! Carbon modal dialogs for thread actions (`Dialogs` board):
//! restoring a turn's checkpoint, snoozing a thread, linking a pull request,
//! and deleting a thread with uncommitted worktree changes.

use std::time::SystemTime;

use gpui::component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    scroll::ScrollableElement as _,
};
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, SharedString, Window, div, prelude::*, px,
};
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, Checkbox, RadioGroup, RadioGroupItem, RadioOrientation,
        alert::{Alert, AlertVariant},
    },
    icons::{CarbonIcon, Icon, IconSize},
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::{fields, primitives::TimeField},
};

use crate::{
    desk::{Desk, Later, Over, Place, lists},
    fonts,
    git::Scope,
    github::Kind,
    session::{Item, Session},
    store::Link,
    theme, when,
};

/// Ephemeral form state for the thread modals.
pub struct ThreadDialogs {
    pub snooze_keys: Vec<u64>,
    pub snooze_choice: String,
    pub snooze_date: Entity<InputState>,
    pub snooze_time: String,
    pub link_key: Option<u64>,
    pub link_search: Entity<InputState>,
    pub link_chosen: Option<(u64, String)>,
    pub link_watch: bool,
    pub undo_turn: Option<(u64, usize)>,
    pub delete_keys: Vec<u64>,
    pub delete_keep_wt: bool,
}

impl ThreadDialogs {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let snooze_date = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe(&snooze_date, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let link_search = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe(&link_search, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            snooze_keys: Vec::new(),
            snooze_choice: "tomorrow".into(),
            snooze_date,
            snooze_time: "09:00".into(),
            link_key: None,
            link_search,
            link_chosen: None,
            link_watch: true,
            undo_turn: None,
            delete_keys: Vec::new(),
            delete_keep_wt: false,
        }
    }
}

/// Carbon `.dlg-modal` frame: `layer-01` ground, square corners, top-right
/// ghost Close icon button, `16px` body padding and `32px` right-aligned footer.
fn modal_frame(
    eyebrow: impl Into<SharedString>,
    heading: impl Into<SharedString>,
    body: impl IntoElement,
    footer: Vec<AnyElement>,
) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .w_full()
        .bg(theme::role(ColorRole::Layer01))
        .text_color(theme::role(ColorRole::TextPrimary))
        .child(
            div().absolute().top(px(8.)).right(px(8.)).child(
                Button::new("dlg-close", crate::tr("Close"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Close)
                    .icon_only(true)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .pt(px(16.))
                .px(px(16.))
                .pb(px(24.))
                .child(
                    div()
                        .pr(px(40.))
                        .mb(px(-4.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(eyebrow.into()),
                )
                .child(
                    div()
                        .pr(px(32.))
                        .type_style(TypeStyle::Heading03)
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(heading.into()),
                )
                .child(body),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .px(px(16.))
                .pb(px(16.))
                .children(footer),
        )
}

fn open_carbon_modal(
    width: f32,
    draw: fn(&mut Desk, &mut Window, &mut Context<Desk>) -> AnyElement,
    window: &mut Window,
    cx: &mut Context<Desk>,
) {
    let desk = cx.entity();
    let shown = cx.new(|cx| Over::new(desk, draw, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(width))
            .p_0()
            .rounded_none()
            .border_0()
            .bg(theme::role(ColorRole::Layer01))
            .close_button(false)
            .overlay_closable(true)
            .child(shown.clone())
    });
}

/// Reset time mentioned in the session's latest limit note, if any.
fn session_limit_reset(s: &Session, now: SystemTime) -> Option<SystemTime> {
    s.items.iter().rev().find_map(|item| match item {
        Item::Note {
            text,
            trouble: true,
        } => when::reset_at(text, now),
        _ => None,
    })
}

impl Desk {
    /// Confirm before restoring files to before `turn` when `turn` changed
    /// files; Redo runs immediately.
    pub fn ask_undo(&mut self, turn: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let undone = s.redo.as_ref().is_some_and(|(t, _)| *t == turn);
        let has_files = s
            .turns
            .get(turn)
            .and_then(|t| t.diff.as_ref())
            .is_some_and(|d| !d.is_empty());
        if undone || !has_files {
            self.undo(turn, cx);
            return;
        }
        self.dialogs.undo_turn = Some((s.key, turn));
        open_carbon_modal(480., Self::undo_dialog_view, window, cx);
    }

    fn undo_dialog_view(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some((key, turn)) = self.dialogs.undo_turn else {
            return div().into_any_element();
        };
        let faces = fonts::faces(cx).clone();
        let files = self
            .sessions
            .at(key)
            .and_then(|s| s.turns.get(turn))
            .and_then(|t| t.diff.clone())
            .unwrap_or_default();
        let n = files.len();
        let turn_num = turn + 1;
        let eyebrow = crate::tr_with("Undo turn %{n}", &[("n", &turn_num)]);
        let heading = crate::tr_with(
            "Put files back as they were before turn %{n}?",
            &[("n", &turn_num)],
        );
        let mut tree = div()
            .id("undo-files")
            .flex()
            .flex_col()
            .max_h(px(220.))
            .border_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .bg(theme::role(ColorRole::Layer02))
            .overflow_y_scrollbar();
        for (ix, f) in files.iter().enumerate() {
            let status = if f.patch.contains("--- /dev/null") || f.patch.contains("new file mode") {
                ("A", theme::added())
            } else if f.patch.contains("+++ /dev/null") || f.patch.contains("deleted file mode") {
                ("D", theme::gone())
            } else {
                ("M", theme::role(ColorRole::TextSecondary))
            };
            tree = tree.child(
                div()
                    .id(SharedString::from(format!("undo-file-{ix}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .px(px(12.))
                    .when(ix > 0, |d| {
                        d.border_t_1()
                            .border_color(theme::role(ColorRole::BorderSubtle01))
                    })
                    .cursor_pointer()
                    .hover(|st| st.bg(theme::role(ColorRole::LayerHover02)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.close_dialog(cx);
                        this.show_changes(Scope::Turn(turn), cx);
                    }))
                    .child(
                        div()
                            .font_family(faces.mono.clone())
                            .type_style(TypeStyle::Code01)
                            .text_color(status.1)
                            .child(status.0),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .font_family(faces.mono.clone())
                            .type_style(TypeStyle::Code01)
                            .child(f.path.clone()),
                    )
                    .child(crate::ui::counts(
                        &faces,
                        f.added,
                        f.gone,
                        crate::style::CAPTION,
                    )),
            );
        }
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr(
                        "Only files change; the conversation stays as it is. Redo brings the changes back, and your own uncommitted work is kept. Click a file to preview what goes back.",
                    )),
            )
            .child(tree);
        let ok_label = if n == 1 {
            crate::tr("Undo 1 file change")
        } else {
            crate::tr_with("Undo %{n} file changes", &[("n", &n)])
        };
        let desk = cx.entity();
        let footer = vec![
            Button::new("undo-cancel", crate::tr("Cancel"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .on_click(|_, window, cx| window.close_dialog(cx))
                .into_any_element(),
            Button::new("undo-confirm", ok_label)
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    desk.update(cx, |d, cx| {
                        d.current = Some(key);
                        d.undo(turn, cx);
                    });
                })
                .into_any_element(),
        ];
        modal_frame(eyebrow, heading, body, footer).into_any_element()
    }

    /// Open the Carbon Snooze modal (`Dialogs` board) for `keys`.
    pub fn open_snooze_dialog(
        &mut self,
        keys: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keys.is_empty() {
            return;
        }
        let now = SystemTime::now();
        let has_later = when::at_hour(now, 0, 18) > now;
        self.dialogs.snooze_choice = if has_later {
            "later".into()
        } else {
            "tomorrow".into()
        };
        let default_date = when::said_at(when::at_hour(now, 1, 9), now, false)
            .split_whitespace()
            .next()
            .unwrap_or("tomorrow")
            .to_owned();
        self.dialogs
            .snooze_date
            .update(cx, |i, cx| i.set_value(default_date, window, cx));
        self.dialogs.snooze_time = "09:00".into();
        self.dialogs.snooze_keys = keys;
        self.popup = crate::desk::Popup::Closed;
        open_carbon_modal(440., Self::snooze_dialog_view, window, cx);
    }

    fn snooze_dialog_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let now = SystemTime::now();
        let twelve = self.settings.twelve;
        let heading = match self.dialogs.snooze_keys.as_slice() {
            [only] => crate::tr_with(
                "Snooze “%{title}”",
                &[(
                    "title",
                    &self
                        .sessions
                        .at(*only)
                        .map(|s| s.shown_title().to_owned())
                        .unwrap_or_default(),
                )],
            ),
            many => crate::tr_with("Snooze %{n} threads", &[("n", &many.len())]),
        };
        let later_at = {
            let six = when::at_hour(now, 0, 18);
            if six > now {
                six
            } else {
                now + std::time::Duration::from_hours(3)
            }
        };
        let later_label = if when::at_hour(now, 0, 18) > now {
            crate::tr_with(
                "Later today · %{time}",
                &[("time", &when::clock(later_at, twelve))],
            )
        } else {
            crate::tr_with(
                "In 3 hours · %{time}",
                &[("time", &when::clock(later_at, twelve))],
            )
        };
        let tomorrow_at = when::at_hour(now, 1, 9);
        let tomorrow_label = crate::tr_with(
            "Tomorrow · %{time}",
            &[("time", &when::clock(tomorrow_at, twelve))],
        );
        let limit_reset = self
            .dialogs
            .snooze_keys
            .first()
            .and_then(|k| self.sessions.at(*k))
            .and_then(|s| session_limit_reset(s, now));

        let desk_radio = cx.entity();
        let mut group = RadioGroup::new("snooze-until")
            .legend(crate::tr("Until"))
            .orientation(RadioOrientation::Vertical)
            .value(self.dialogs.snooze_choice.clone())
            .item(RadioGroupItem::new("later").label(later_label))
            .item(RadioGroupItem::new("tomorrow").label(tomorrow_label));
        if let Some(reset) = limit_reset {
            group = group.item(RadioGroupItem::new("limits").label(crate::tr_with(
                "When usage limits reset · %{time}",
                &[("time", &when::clock(reset, twelve))],
            )));
        }
        group = group
            .item(RadioGroupItem::new("custom").label(crate::tr("Pick a date and time")))
            .on_value_change(move |v, _, cx| {
                desk_radio.update(cx, |d, cx| {
                    d.dialogs.snooze_choice = v.to_string();
                    cx.notify();
                });
            });

        let desk_time = cx.entity();
        let custom_row = div()
            .flex()
            .items_end()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(crate::tr("Date")),
                    )
                    .child(fields::line_input(
                        &self.dialogs.snooze_date,
                        false,
                        fields::field_fill(true),
                        Some(crate::tr("tomorrow, next week, YYYY-MM-DD")),
                        window,
                        cx,
                    )),
            )
            .child(
                TimeField::new("snooze-time")
                    .label(crate::tr("Time"))
                    .hour12(twelve)
                    .value(self.dialogs.snooze_time.clone())
                    .on_value_change(move |v, _, cx| {
                        desk_time.update(cx, |d, cx| {
                            d.dialogs.snooze_choice = "custom".into();
                            d.dialogs.snooze_time = v.to_string();
                            cx.notify();
                        });
                    }),
            );

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(group)
            .child(custom_row);
        let desk_ok = cx.entity();
        let footer = vec![
            Button::new("snooze-cancel", crate::tr("Cancel"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .on_click(|_, window, cx| window.close_dialog(cx))
                .into_any_element(),
            Button::new("snooze-ok", crate::tr("Snooze thread"))
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    desk_ok.update(cx, Desk::confirm_snooze_dialog);
                })
                .into_any_element(),
        ];
        modal_frame(crate::tr("Snooze"), heading, body, footer).into_any_element()
    }

    fn confirm_snooze_dialog(&mut self, cx: &mut Context<Self>) {
        let now = SystemTime::now();
        let keys = std::mem::take(&mut self.dialogs.snooze_keys);
        if keys.is_empty() {
            return;
        }
        let target = match self.dialogs.snooze_choice.as_str() {
            "later" => {
                let six = when::at_hour(now, 0, 18);
                if six > now {
                    Later::At(six)
                } else {
                    Later::In(180)
                }
            }
            "limits" => keys
                .first()
                .and_then(|k| self.sessions.at(*k))
                .and_then(|s| session_limit_reset(s, now))
                .map_or(Later::Tomorrow, Later::At),
            "custom" => {
                let date_text = self.dialogs.snooze_date.read(cx).value().trim().to_owned();
                let (h, m) = u0ui::u0::primitives::time_field::parse_hm(&self.dialogs.snooze_time);
                let minute = u16::try_from(h * 60 + m).unwrap_or(540);
                let at = parse_custom_snooze(&date_text, minute, now);
                Later::At(at)
            }
            _ => Later::Tomorrow,
        };
        self.snooze(&keys, target, cx);
    }

    /// Open the Carbon Link a pull request modal (`Dialogs` board).
    pub fn open_link_pr_dialog(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let watch = self
            .sessions
            .at(key)
            .is_none_or(|s| s.meta.watch.is_some() || s.meta.links.is_empty());
        self.dialogs.link_key = Some(key);
        self.dialogs.link_chosen = None;
        self.dialogs.link_watch = watch;
        self.dialogs
            .link_search
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.want_offers();
        self.popup = crate::desk::Popup::Closed;
        open_carbon_modal(460., Self::link_pr_dialog_view, window, cx);
    }

    fn link_pr_dialog_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(key) = self.dialogs.link_key else {
            return div().into_any_element();
        };
        let faces = fonts::faces(cx).clone();
        let eyebrow = self
            .sessions
            .at(key)
            .map(|s| s.shown_title().to_owned())
            .unwrap_or_default();
        let prs = self
            .sessions
            .at(key)
            .map(Place::project_of)
            .and_then(|p| self.prs.get(&p).cloned())
            .unwrap_or_default();
        let query = self
            .dialogs
            .link_search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let filtered: Vec<(u64, String)> = prs
            .into_iter()
            .filter(|(n, title)| {
                query.is_empty()
                    || format!("#{n}").contains(&query)
                    || title.to_lowercase().contains(&query)
            })
            .take(24)
            .collect();
        let chosen = self.dialogs.link_chosen.clone();
        let mut list = div()
            .id("link-pr-list")
            .flex()
            .flex_col()
            .max_h(px(160.))
            .border_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .bg(theme::role(ColorRole::Field02))
            .overflow_y_scrollbar();
        if filtered.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr("No open pull requests found.")),
            );
        } else {
            for (n, title) in filtered {
                let on = chosen.as_ref().is_some_and(|(c, _)| *c == n);
                let pick_title = title.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("link-pr-{n}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_h(px(32.))
                        .px(px(12.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(theme::role(ColorRole::LayerSelected02)))
                        .when(!on, |d| {
                            d.hover(|st| st.bg(theme::role(ColorRole::LayerHover02)))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.dialogs.link_chosen = Some((n, pick_title.clone()));
                            cx.notify();
                        }))
                        .child(
                            div()
                                .font_family(faces.mono.clone())
                                .type_style(TypeStyle::Code01)
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(format!("#{n}")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .type_style(TypeStyle::BodyCompact01)
                                .child(title),
                        ),
                );
            }
        }
        let desk_watch = cx.entity();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr(
                        "The thread shows the PR's checks and reviews, and settles by itself once the pull request merges.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(crate::tr("Pull request")),
                    )
                    .child(fields::line_input(
                        &self.dialogs.link_search,
                        false,
                        fields::field_fill(true),
                        Some(crate::tr("Search by number or title")),
                        window,
                        cx,
                    ))
                    .child(list),
            )
            .child(
                Checkbox::new("link-watch")
                    .label(crate::tr("Watch checks and tell me when they fail"))
                    .checked(self.dialogs.link_watch)
                    .on_checked_change(move |v, _, cx| {
                        desk_watch.update(cx, |d, cx| {
                            d.dialogs.link_watch = v;
                            cx.notify();
                        });
                    }),
            );
        let desk_ok = cx.entity();
        let can_link = self.dialogs.link_chosen.is_some();
        let footer = vec![
            Button::new("link-pr-cancel", crate::tr("Cancel"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .on_click(|_, window, cx| window.close_dialog(cx))
                .into_any_element(),
            Button::new("link-pr-ok", crate::tr("Link pull request"))
                .size(ButtonSize::Sm)
                .disabled(!can_link)
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    desk_ok.update(cx, |d, cx| {
                        if let Some((number, title)) = d.dialogs.link_chosen.take() {
                            d.link(
                                key,
                                Link {
                                    kind: Kind::Pr,
                                    number,
                                    title,
                                },
                            );
                            if let Some(s) = d.sessions.at_mut(key) {
                                s.meta.watch = d.dialogs.link_watch.then_some(number);
                            }
                            d.save_index();
                            cx.notify();
                        }
                    });
                })
                .into_any_element(),
        ];
        modal_frame(eyebrow, crate::tr("Link a pull request"), body, footer).into_any_element()
    }

    /// Open the Carbon Delete thread alertdialog (`Dialogs` board), warning
    /// when the thread's sole worktree has uncommitted files.
    pub fn open_delete_dialog(
        &mut self,
        keys: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keys.is_empty() {
            return;
        }
        self.dialogs.delete_keys = keys;
        self.dialogs.delete_keep_wt = false;
        self.popup = crate::desk::Popup::Closed;
        open_carbon_modal(460., Self::delete_dialog_view, window, cx);
    }

    fn delete_dialog_view(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let keys = self.dialogs.delete_keys.clone();
        if keys.is_empty() {
            return div().into_any_element();
        }
        let sole_wt = match keys.as_slice() {
            [only] => self
                .sessions
                .at(*only)
                .and_then(|s| s.meta.worktree.clone())
                .filter(|wt| {
                    self.sessions
                        .all
                        .iter()
                        .filter(|o| o.meta.worktree.as_ref().is_some_and(|w| w.path == wt.path))
                        .count()
                        == 1
                }),
            _ => None,
        };
        let dirty: Vec<String> = match keys.as_slice() {
            [only] if sole_wt.is_some() => self
                .sessions
                .at(*only)
                .and_then(|s| s.total())
                .map(|files| files.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let heading = match keys.as_slice() {
            [only] => crate::tr_with(
                "Delete “%{title}”?",
                &[(
                    "title",
                    &self
                        .sessions
                        .at(*only)
                        .map(|s| s.shown_title().to_owned())
                        .unwrap_or_default(),
                )],
            ),
            many => crate::tr_with("Delete %{n} threads?", &[("n", &many.len())]),
        };
        let desc = if sole_wt.is_some() {
            crate::tr("The thread, its worktree and its checkpoints are removed for good.")
        } else {
            crate::tr("This permanently clears conversation history for this thread.")
        };
        let mut body = div().flex().flex_col().gap(px(12.)).child(
            div()
                .type_style(TypeStyle::Body01)
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(desc),
        );
        if !dirty.is_empty()
            && let Some(wt) = &sole_wt
        {
            let warn_title = if dirty.len() == 1 {
                crate::tr("1 uncommitted file")
            } else {
                crate::tr_with("%{n} uncommitted files", &[("n", &dirty.len())])
            };
            let listed = dirty.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let warn_desc = crate::tr_with(
                "On %{branch}, %{files} are not in any commit. They go too.",
                &[("branch", &wt.branch), ("files", &listed)],
            );
            body = body.child(
                Alert::new("delete-dirty-alert")
                    .variant(AlertVariant::Warning)
                    .title(warn_title)
                    .description(warn_desc),
            );
        }
        if sole_wt.is_some() {
            let desk_keep = cx.entity();
            body = body.child(
                Checkbox::new("delete-keep-worktree")
                    .label(crate::tr("Keep the worktree folder on disk"))
                    .checked(self.dialogs.delete_keep_wt)
                    .on_checked_change(move |v, _, cx| {
                        desk_keep.update(cx, |d, cx| {
                            d.dialogs.delete_keep_wt = v;
                            cx.notify();
                        });
                    }),
            );
        }
        let desk_ok = cx.entity();
        let footer = vec![
            Button::new("del-cancel", crate::tr("Cancel"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .on_click(|_, window, cx| window.close_dialog(cx))
                .into_any_element(),
            Button::new("del-confirm", crate::tr("Delete thread"))
                .variant(ButtonVariant::Destructive)
                .size(ButtonSize::Sm)
                .icon(Icon::carbon(CarbonIcon::TrashCan).size(IconSize::S16))
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    desk_ok.update(cx, |d, cx| {
                        let keys = std::mem::take(&mut d.dialogs.delete_keys);
                        if d.dialogs.delete_keep_wt
                            && let [only] = keys.as_slice()
                        {
                            d.keep_worktree.insert(*only);
                        }
                        d.undoable(lists::Undone::Delete, &keys, cx);
                    });
                })
                .into_any_element(),
        ];
        modal_frame(crate::tr("Delete"), heading, body, footer).into_any_element()
    }
}

/// Resolve a custom date phrase (`"tomorrow"`, `"next week"`, `"2026-11-30"`,
/// or relative span) plus `minute` (`0..1440`) into a `SystemTime`.
fn parse_custom_snooze(date_text: &str, minute: u16, now: SystemTime) -> SystemTime {
    let trimmed = date_text.trim();
    if let Ok(days) = trimmed.parse::<i32>() {
        return when::at_minute(now, days.max(0), minute);
    }
    if trimmed.eq_ignore_ascii_case("today") {
        let today = when::at_minute(now, 0, minute);
        return if today > now {
            today
        } else {
            when::at_minute(now, 1, minute)
        };
    }
    if trimmed.eq_ignore_ascii_case("tomorrow") || trimmed.is_empty() {
        return when::at_minute(now, 1, minute);
    }
    if trimmed.eq_ignore_ascii_case("next week") {
        return when::at_minute(now, when::to_monday(now), minute);
    }
    if let Some(rfc) = when::parse(&format!(
        "{trimmed}T{:02}:{:02}:00Z",
        minute / 60,
        minute % 60
    )) && rfc > now
    {
        return rfc;
    }
    when::ahead(trimmed, now).unwrap_or_else(|| when::at_minute(now, 1, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_snooze_resolves_tomorrow_and_today_at_chosen_minute() {
        let now = when::at_hour(SystemTime::now(), 0, 10);
        let tom = parse_custom_snooze("tomorrow", 9 * 60 + 30, now);
        assert!(tom > now);
        assert_eq!(when::minute_of_day(tom), 9 * 60 + 30);
        let later = parse_custom_snooze("today", 18 * 60, now);
        assert_eq!(later, when::at_minute(now, 0, 18 * 60));
    }
}
