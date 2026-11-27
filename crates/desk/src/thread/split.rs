//! Split thread panes (`SplitSideChat` board): two threads side by side,
//! with the unfocused thread collapsing into a `40px` vertical rail while the
//! right column is open over the split.

use gpui::component::scroll::ScrollableElement as _;
use gpui::{
    BoxShadow, Context, Div, FontWeight, MouseButton, SharedString, Window, div, prelude::*, px,
};
use u0ui::{
    components::{Button, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon, IconSize},
    t3::{IdentitySize, ProjectMonogram, ProviderIcon, Spin},
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use crate::{
    desk::{Desk, Place, TOPBAR, draggable, lists},
    fonts::Faces,
    session::{Session, State},
    theme,
    thread::drawer::Grid,
    ui,
};

/// Two threads open side by side: `Desk::current` is the focused pane, and
/// `other` is the second pane (`other_left` places `other` to the left of
/// `current`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub other: u64,
    pub other_left: bool,
}

impl Desk {
    /// Open `key` in a split pane beside the current thread (`Open beside`,
    /// `⇧↵` in the command palette).
    pub fn open_beside(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cur) = self.current else {
            self.show(key, window, cx);
            return;
        };
        if cur == key {
            if let Some(other) = self.ordered().into_iter().find(|k| *k != cur) {
                self.split = Some(Split {
                    other,
                    other_left: false,
                });
                cx.notify();
            }
            return;
        }
        self.split = Some(Split {
            other: cur,
            other_left: true,
        });
        self.bring(key, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
    }

    /// Focus the unfocused split pane, swapping `current` and `split.other`.
    pub fn focus_split_other(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(sp), Some(cur)) = (self.split, self.current) else {
            return;
        };
        self.split = None;
        self.bring(sp.other, window, cx);
        self.split = Some(Split {
            other: cur,
            other_left: !sp.other_left,
        });
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Close either the focused pane or the unfocused pane of the split.
    pub fn close_split(
        &mut self,
        close_focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sp) = self.split.take() else {
            return;
        };
        if close_focused {
            self.show(sp.other, window, cx);
        } else {
            cx.notify();
        }
    }

    /// Keep `split` valid when session `gone` is archived or forgotten.
    pub(crate) fn clean_split(&mut self, gone: u64) {
        if self.split.is_some_and(|sp| sp.other == gone) {
            self.split = None;
        } else if self.current == Some(gone)
            && let Some(sp) = self.split.take()
        {
            self.current = Some(sp.other);
        }
    }

    /// Header bar for one split pane (`ph` / `ph--focus` in `SplitSideChat`).
    fn split_pane_header(
        &self,
        faces: &Faces,
        s: &Session,
        focused: bool,
        left_inset: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let project = self.project_name(&Place::project_of(s));
        let status = lists::status(s);
        let border_bottom = if focused {
            theme::role(ColorRole::BorderInteractive)
        } else {
            theme::role(ColorRole::BorderSubtle01)
        };
        let mut bar = draggable()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(TOPBAR))
            .pl(px(left_inset))
            .pr(px(8.))
            .bg(theme::role(ColorRole::Background))
            .border_b_1()
            .border_color(border_bottom);
        if focused {
            bar = bar.shadow(vec![
                BoxShadow::new(px(0.), px(-1.), theme::role(ColorRole::BorderInteractive)).inset(),
            ]);
        }
        if left_inset > 20. && !self.panes.left {
            bar = bar.child(
                ui::icon_button(
                    SharedString::from(format!("split-sb-{}", s.key)),
                    "sidebar",
                    crate::keys::hint(cx, &crate::tr("Show the sidebar"), "toggle_sidebar"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            );
        }
        let title_color = if focused {
            theme::role(ColorRole::TextPrimary)
        } else {
            theme::role(ColorRole::TextSecondary)
        };
        bar.child(
            div()
                .id(SharedString::from(format!("split-head-{}", s.key)))
                .flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .gap(px(8.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when(!focused, |d| {
                    d.cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus_split_other(window, cx);
                        }))
                })
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .type_style(TypeStyle::HeadingCompact01)
                        .font_weight(if focused {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(title_color)
                        .child(s.shown_title().to_owned()),
                )
                .child(
                    div()
                        .flex_none()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(project),
                )
                .when_some(s.branch.as_deref(), |d, b| {
                    d.child(
                        div()
                            .flex_none()
                            .max_w(px(140.))
                            .truncate()
                            .font_family(faces.mono.clone())
                            .type_style(TypeStyle::Code01)
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(b.to_owned()),
                    )
                })
                .when_some(status, |d, st| {
                    d.child(
                        div()
                            .flex_none()
                            .type_style(TypeStyle::Label01)
                            .text_color(st.color())
                            .child(crate::tr(st.word())),
                    )
                }),
        )
        .when(focused, |d| {
            d.child(
                ui::icon_button(
                    "split-toggle-drawer",
                    "terminal",
                    crate::keys::hint(cx, &crate::tr("Terminal"), "drawer"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_drawer(window, cx))),
            )
            .child(
                ui::icon_button(
                    "split-toggle-aside",
                    "panel-right",
                    crate::keys::hint(
                        cx,
                        &crate::tr("Show or hide the right column"),
                        "right_column",
                    ),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_aside(cx))),
            )
        })
        .child(
            Button::new(
                SharedString::from(format!("close-split-{}", s.key)),
                crate::tr("Close pane"),
            )
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .icon(CarbonIcon::Close)
            .icon_only(true)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_split(focused, window, cx);
            })),
        )
    }

    /// Unfocused split pane: scrollable conversation + click-to-focus composer.
    fn split_other_pane(
        &self,
        faces: &Faces,
        s: &Session,
        left_inset: f32,
        pane_width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let col = self.settings.width.column(pane_width);
        let start = s.items.len().saturating_sub(40);
        let mut items = div().flex().flex_col().gap(px(14.)).w(px(col)).py(px(24.));
        if s.items.is_empty() {
            items = items.child(
                div()
                    .py(px(40.))
                    .text_center()
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr("Click to write in this thread")),
            );
        } else {
            for (offset, item) in s.items[start..].iter().enumerate() {
                items = items.child(self.item(faces, s, start + offset, item, cx));
            }
        }
        let agent_name = self.name_of(&s.meta.agent);
        let draft = if s.meta.unsent.trim().is_empty() {
            crate::tr_with("Reply to %{name}…", &[("name", &agent_name)]).to_string()
        } else {
            s.meta.unsent.clone()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .child(self.split_pane_header(faces, s, false, left_inset, cx))
            .child(
                div()
                    .id("split-other-scroll")
                    .flex()
                    .flex_col()
                    .items_center()
                    .flex_1()
                    .min_h(px(0.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_split_other(window, cx);
                    }))
                    .child(items)
                    .overflow_y_scrollbar(),
            )
            .child(
                div().flex().justify_center().px(px(24.)).pb(px(16.)).child(
                    div()
                        .id("split-other-composer")
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .w(px(col))
                        .min_h(px(44.))
                        .px(px(12.))
                        .py(px(10.))
                        .bg(theme::role(ColorRole::Field01))
                        .border_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .cursor_text()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus_split_other(window, cx);
                        }))
                        .child(ProviderIcon::new(agent_name.clone()).size(IdentitySize::Xs))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .type_style(TypeStyle::BodyCompact01)
                                .text_color(if s.meta.unsent.trim().is_empty() {
                                    theme::role(ColorRole::TextPlaceholder)
                                } else {
                                    theme::role(ColorRole::TextPrimary)
                                })
                                .child(draft),
                        ),
                ),
            )
    }

    /// Middle area when `self.split` is active: either two full panes side by
    /// side, or the focused pane alone while `right_shown` collapses `other`
    /// into `split_rail`.
    #[allow(
        clippy::too_many_arguments,
        reason = "layout inputs computed once in Desk::render"
    )]
    pub(crate) fn split_middle(
        &self,
        faces: &Faces,
        lights: f32,
        left_beside: bool,
        pane_width: f32,
        right_shown: bool,
        cell: Grid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sp = self.split?;
        let cur = self.session()?;
        let other = self.sessions.at(sp.other)?;
        let first_inset = if left_beside { 16. } else { lights };
        if right_shown {
            let col = self.settings.width.column(pane_width);
            return Some(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .child(self.split_pane_header(faces, cur, true, first_inset, cx))
                    .child(self.thread(faces, col, window, cx))
                    .child(self.composer(faces, col, window, cx))
                    .children(self.drawer(faces, cell, cx)),
            );
        }
        let half = ((pane_width - 1.) / 2.).max(240.);
        let col = self.settings.width.column(half);
        let focused_inset = if sp.other_left { 16. } else { first_inset };
        let other_inset = if sp.other_left { first_inset } else { 16. };
        let focused_pane = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .when(!sp.other_left, |d| {
                d.border_r_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
            })
            .child(self.split_pane_header(faces, cur, true, focused_inset, cx))
            .child(self.thread(faces, col, window, cx))
            .child(self.composer(faces, col, window, cx))
            .children(self.drawer(faces, cell, cx));
        let other_pane = self
            .split_other_pane(faces, other, other_inset, half, cx)
            .when(sp.other_left, |d| {
                d.border_r_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
            });
        Some(
            div()
                .flex()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .overflow_hidden()
                .children(if sp.other_left {
                    vec![other_pane, focused_pane]
                } else {
                    vec![focused_pane, other_pane]
                }),
        )
    }

    /// `40px` collapsed rail (`SplitSideChat` `.rail`) shown on the far right
    /// while `panes.right` is open over a split view.
    pub(crate) fn split_rail(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let sp = self.split?;
        let other = self.sessions.at(sp.other)?;
        let project = self.project_name(&Place::project_of(other));
        let st = lists::status(other);
        let st_word = st.map_or_else(|| crate::tr("Done"), |s| crate::tr(s.word()));
        let tip = format!(
            "{} · {} · {} — {}",
            other.shown_title(),
            project,
            st_word,
            crate::tr("Click to expand")
        );
        let status_mark = match &other.state {
            State::Working | State::Opening => Spin::new("rail-spin").size(14.).into_any_element(),
            State::Broken(_) => Icon::carbon(CarbonIcon::ErrorFilled)
                .size(IconSize::S16)
                .color(theme::role(ColorRole::SupportError))
                .into_any_element(),
            _ if other.asks() => Icon::carbon(CarbonIcon::WarningAltFilled)
                .size(IconSize::S16)
                .color(theme::role(ColorRole::SupportWarning))
                .into_any_element(),
            _ => Icon::carbon(CarbonIcon::CheckmarkFilled)
                .size(IconSize::S16)
                .color(theme::role(ColorRole::IconSecondary))
                .into_any_element(),
        };
        Some(
            div()
                .id("split-rail")
                .flex()
                .flex_col()
                .flex_none()
                .items_center()
                .w(px(40.))
                .h_full()
                .bg(theme::role(ColorRole::Layer01))
                .border_l_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .cursor_pointer()
                .tooltip(ui::tip(tip.clone()))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.panes.right = false;
                    this.focus_split_other(window, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(TOPBAR))
                        .border_b_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            Button::new("rail-expand", tip)
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .icon(CarbonIcon::ChevronLeft)
                                .icon_only(true)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.panes.right = false;
                                    this.focus_split_other(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .pt(px(14.))
                        .child(status_mark)
                        .child(
                            ProviderIcon::new(self.name_of(&other.meta.agent))
                                .size(IdentitySize::Xs),
                        )
                        .child(ProjectMonogram::new(project).size(IdentitySize::Xs)),
                ),
        )
    }
}
