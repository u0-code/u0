//! The bar across the top of the thread, and the controls beside the
//! window's lights: what is on screen, its clock, and with a character
//! the place her thread shows.

use gpui::{Context, Corner, Div, FontWeight, anchored, deferred, div, prelude::*, px};

use super::{Desk, Open, Popup, TOPBAR, draggable};
use crate::{fonts::Faces, model::Status, theme, ui};

impl Desk {
    /// A small square control in the row beside the window's lights.
    fn control(id: &'static str, icon: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(enabled, |d| {
                d.cursor_pointer().hover(|s| s.bg(theme::hover()))
            })
            .child(ui::icon(icon, 16.).text_color(if enabled {
                theme::muted()
            } else {
                theme::ghost_line()
            }))
    }

    /// The controls beside the window's lights: the sidebar, back, forward.
    /// They sit at the top of the sidebar, or of the thread when it is hidden.
    pub(crate) fn controls(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                Self::control("sidebar", "sidebar", true)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                Self::control(
                    "back",
                    "chevron-left",
                    self.open.is_some() || !self.back.is_empty(),
                )
                .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
            )
            .child(
                Self::control("forward", "chevron-right", !self.forward.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
            )
    }

    /// The bar across the top of the thread: the session's name, the clock,
    /// and, when the sidebar is hidden, the controls beside the lights.
    pub(super) fn topbar(
        &self,
        faces: &Faces,
        lights: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // With a character open: her name, the place her thread shows, and
        // her clock; with the solver, the session's name and its clock.
        let (title, status, picker): (String, Option<&Status>, Option<Div>) = match &self.open {
            _ if self.attending() => ("Attention".to_owned(), None, None),
            Some(open) => (
                open.her.name(),
                open.her.status.as_ref(),
                Some(self.place_picker(open, faces, cx)),
            ),
            None => (self.session().label(), self.solver.status.as_ref(), None),
        };
        let right = status.map(|s| {
            div()
                .flex()
                .items_baseline()
                .gap(px(6.))
                .child(ui::mono(faces, s.hhmm()).text_color(theme::text()))
                .child(ui::mono(faces, s.city.clone()))
        });
        draggable()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .h(px(TOPBAR))
            .pl(px(if self.sidebar { 24. } else { lights }))
            .pr(px(24.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .flex_1()
                    .min_w(px(0.))
                    .when(!self.sidebar, |d| d.child(self.controls(cx)))
                    .child({
                        let title = ui::text(faces, title)
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(13.5));
                        // Beside a picker her name keeps its width; alone,
                        // a session's name trails off.
                        if picker.is_some() {
                            title.flex_none()
                        } else {
                            ui::one_line(title)
                        }
                    })
                    .children(picker),
            )
            .children(right)
    }

    /// The place her thread shows, which opens a menu of every place in
    /// her journal's tail, the most recent first.
    fn place_picker(&self, open: &Open, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let who = self.who();
        let places = open.places(who);
        let on = open.place(who);
        let label = places
            .iter()
            .find(|p| p.key == on)
            .map_or_else(|| on.clone(), |p| p.label.clone());
        let chip = div()
            .id("place")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(10.))
            .min_w(px(0.))
            .rounded_full()
            .cursor_pointer()
            .when(self.popup == Popup::Places, |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(ui::one_line(
                ui::mono(faces, label)
                    .text_size(px(12.))
                    .text_color(theme::text2()),
            ))
            .child(ui::icon("chevron-down", 12.).text_color(theme::muted()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.popup = if this.popup == Popup::Places {
                    Popup::Closed
                } else {
                    Popup::Places
                };
                cx.notify();
            }));
        let mut picker = div().relative().flex_1().min_w(px(0.)).child(chip);
        if self.popup == Popup::Places {
            let mut menu = div()
                .id("place-menu")
                .occlude()
                .flex()
                .flex_col()
                .gap(px(2.))
                .w(px(300.))
                .max_h(px(420.))
                .overflow_y_scroll()
                .p(px(6.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .rounded(px(12.))
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.popup = Popup::Closed;
                    cx.notify();
                }));
            for (i, place) in places.into_iter().enumerate() {
                let here = place.key == on;
                let key = place.key.clone();
                menu = menu.child(
                    div()
                        .id(("place-item", i))
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(here, |d| d.bg(theme::panel2()))
                        .hover(|s| s.bg(theme::hover()))
                        .child(ui::one_line(
                            ui::text(faces, place.label).text_size(px(13.)),
                        ))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.choose_place(key.clone(), cx)),
                        ),
                );
            }
            // Under the chip, its left edge on the chip's.
            picker = picker.child(
                div().absolute().top(px(30.)).left_0().child(deferred(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu),
                )),
            );
        }
        picker
    }
}
