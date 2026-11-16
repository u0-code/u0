//! The line you type into, and saying what is typed in it: to the solver
//! in the session on screen, or to the character open in her place.

use gpui::{Context, Corner, Div, div, prelude::*, px};

use super::{At, Desk, Menu};
use crate::{fonts::Faces, input::TextInput, link, theme, thread, ui};

impl Desk {
    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        // To a character you speak in your own place with her on the desk
        // channel, and her thread turns to it.
        if let Some(open) = &mut self.open {
            let place = self.settings.who.to_lowercase();
            open.place = Some(thread::place_key(&place));
            open.her.say(place, text);
            cx.notify();
            return;
        }
        let Some(session) = self.settings.sessions.get_mut(self.current) else {
            return;
        };
        let place = session.id.clone();
        if session.title.is_empty() {
            session.title = text.chars().take(48).collect();
            self.settings.save();
        }
        self.solver.say(place, text);
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    /// The line you type into, the way a coding agent's is: a box with the
    /// text, and a row under it with the brain on the left and send on the
    /// right.
    pub(super) fn composer(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = self.current_provider();
        let empty = self.input.read(cx).is_empty();
        // To a character the brain is hers, on her daemon; what the desk
        // can say under the line is who it speaks as.
        if self.open.is_some() {
            let note = format!("as {} · {} channel", self.who(), link::CHANNEL);
            return self.composer_with(
                ui::mono(faces, note).text_size(px(11.5)).px(px(6.)),
                empty,
                faces,
                cx,
            );
        }
        let (mark, label) = provider.as_ref().map_or_else(
            || {
                (
                    ui::icon("plug", 14.)
                        .text_color(theme::muted())
                        .into_any_element(),
                    "no brain".to_owned(),
                )
            },
            |p| (ui::mark(faces, p.mark(), 14.), p.label()),
        );
        let mut chip = div().relative().child(
            div()
                .id("brain")
                .flex()
                .items_center()
                .gap(px(7.))
                .h(px(28.))
                .px(px(10.))
                .rounded_full()
                .cursor_pointer()
                .when(self.menu_open_at(At::Composer), |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .child(mark)
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .child(ui::icon("chevron-down", 12.).text_color(theme::muted()))
                .on_click(
                    cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, At::Composer, cx)),
                ),
        );
        if let Some(menu) = self.menu_over(At::Composer, Corner::BottomLeft, faces, cx) {
            chip = chip.child(div().absolute().top(px(-8.)).left(px(0.)).child(menu));
        }
        self.composer_with(chip, empty, faces, cx)
    }

    /// The composer's box, with `under` at the left of the row under the
    /// line and send at its right.
    fn composer_with(&self, under: Div, empty: bool, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let send = div()
            .id("send")
            .size(px(30.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if empty {
                theme::border2()
            } else {
                theme::accent()
            })
            .child(
                ui::mono(faces, "↑")
                    .text_size(px(15.))
                    .text_color(if empty { theme::muted() } else { theme::bg() }),
            )
            .on_click(cx.listener(|this, _, _, cx| this.send(cx)));

        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(32.))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(px(thread::COLUMN))
                    .px(px(14.))
                    .pt(px(4.))
                    .pb(px(10.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::border2())
                    .rounded(px(16.))
                    .child(div().px(px(4.)).child(self.input.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(under)
                            .child(send),
                    ),
            )
            .children(self.notice.as_ref().map(|n| {
                div()
                    .id("notice")
                    .w_full()
                    .max_w(px(thread::COLUMN))
                    .px(px(16.))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, n.clone())
                            .text_size(px(12.))
                            .text_color(theme::muted()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    }))
            }))
    }
}
