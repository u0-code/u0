//! The line you type into, and saying what is typed in it: to the little
//! ghost in the session on screen, or to the character open in your place
//! with her. Everything goes as `console.say`, as you, an operator of hers.
//!
//! It is one slim row, the width of the thread: the line, which wraps and
//! grows to six lines before it scrolls; how it is said (Say, Directive,
//! Task); with the little ghost, the brain and model it thinks with; and
//! send, which becomes Stop while she is busy with what you said there and
//! nothing new is typed. A directive takes the operator's colour on the
//! border. Task opens the form in `task.rs` in place of the line.

use ghost_link::protocol::SayMode;
use gpui::{AnyElement, Context, Corner, Div, Focusable as _, Window, div, prelude::*, px};

use super::{At, Desk, Menu, Mode};
use crate::{fonts::Faces, input::TextInput, link::Order, theme, thread, ui};

impl Desk {
    /// Say what was typed, as the mode has it.
    pub fn say(&mut self, text: String, cx: &mut Context<Self>) {
        let mode = if self.mode == Mode::Directive {
            SayMode::Directive
        } else {
            SayMode::Say
        };
        self.speak(text, mode, cx);
    }

    /// Say your last line again, after she could not think about it.
    pub(crate) fn say_again(&mut self, text: String, cx: &mut Context<Self>) {
        self.speak(text, SayMode::Say, cx);
    }

    /// Say `text` to whoever is on screen, as `mode` has it.
    pub(crate) fn speak(&mut self, text: String, mode: SayMode, cx: &mut Context<Self>) {
        // To a character you speak in your own place with her on the
        // console channel, and her thread turns to it.
        if let Some(open) = &mut self.open {
            let place = self.settings.who.to_lowercase();
            open.place = Some(thread::place_key(&place));
            open.her.say(place, text, mode);
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
        self.solver.say(place, text, mode);
        cx.notify();
    }

    /// Stop what she is doing about the place on screen: her look there,
    /// and the work taken on there.
    fn stop_here(&mut self, cx: &mut Context<Self>) {
        match &self.open {
            Some(open) => {
                let place = open.place(self.who());
                let id = thread::thread_key(&place)
                    .split_once(':')
                    .map(|(_, id)| id.to_owned())
                    .unwrap_or_default();
                open.her.order(Order::StopPlace { place: id });
            }
            None => self.solver.order(Order::StopPlace {
                place: self.session().id,
            }),
        }
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, TextInput::send);
    }

    /// What the line says while it is empty: whom it speaks to, and how.
    pub(super) fn placeholder(&self) -> String {
        let name = self.open.as_ref().map(|o| o.her.name());
        match (self.mode, name) {
            (Mode::Directive, Some(name)) => format!("Tell {name} what to do"),
            (Mode::Directive, None) => format!("Tell {} what to do", self.solver.name()),
            (_, Some(name)) => format!("Say something to {name}"),
            (_, None) => format!("Ask {} for anything on this machine", self.solver.name()),
        }
    }

    /// Say, Directive or Task. Task opens the form; the others go back to
    /// the line.
    pub(crate) fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        let placeholder = self.placeholder();
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
        if mode == Mode::Task {
            self.task.what.focus_handle(cx).focus(window);
        } else {
            self.input.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// The three ways of saying something, as a quiet row of words.
    fn modes(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().flex_none().items_center().gap(px(1.));
        for (mode, word, id) in [
            (Mode::Say, "Say", "mode-say"),
            (Mode::Directive, "Directive", "mode-directive"),
            (Mode::Task, "Task", "mode-task"),
        ] {
            let on = self.mode == mode;
            let color = match (on, mode) {
                (true, Mode::Directive) => theme::purple(),
                (true, _) => theme::text(),
                _ => theme::faint(),
            };
            row = row.child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .h(px(22.))
                    .px(px(6.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .when(!on, |d| d.hover(|s| s.bg(theme::hover())))
                    .child(ui::text(faces, word).text_size(px(12.)).text_color(color))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_mode(mode, window, cx);
                    })),
            );
        }
        row
    }

    /// The brain and model the little ghost thinks with, as small muted
    /// text that opens the brain menu.
    fn brain_chip(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let label = self
            .current_provider()
            .map_or_else(|| "no brain".to_owned(), |p| self.brain_label(&p));
        let mut chip = div().relative().flex_none().child(
            div()
                .id("brain")
                .flex()
                .items_center()
                .gap(px(3.))
                .h(px(22.))
                .px(px(6.))
                .rounded(px(5.))
                .cursor_pointer()
                .when(self.menu_open_at(At::Composer), |d| d.bg(theme::panel2()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    ui::text(faces, label)
                        .text_size(px(12.))
                        .text_color(theme::muted()),
                )
                .child(ui::icon("chevron-down", 11.).text_color(theme::faint()))
                .on_click(
                    cx.listener(|this, _, _, cx| this.open_menu(Menu::Brains, At::Composer, cx)),
                ),
        );
        if let Some(menu) = self.menu_over(At::Composer, Corner::BottomLeft, faces, cx) {
            chip = chip.child(div().absolute().top(px(-8.)).left(px(0.)).child(menu));
        }
        chip
    }

    /// Send, a small round arrow; or, while she is busy with what you said
    /// here and nothing new is typed, Stop, a square.
    fn send_button(faces: &Faces, empty: bool, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let button = div()
            .size(px(26.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer();
        if busy && empty {
            return button
                .id("stop")
                .bg(theme::text2())
                .hover(|s| s.bg(theme::text()))
                .child(div().size(px(9.)).rounded(px(2.)).bg(theme::bg()))
                .tooltip(ui::tip("Stop"))
                .on_click(cx.listener(|this, _, _, cx| this.stop_here(cx)))
                .into_any_element();
        }
        button
            .id("send")
            .bg(if empty {
                theme::border2()
            } else {
                theme::accent()
            })
            .child(
                ui::mono(faces, "↑")
                    .text_size(px(13.))
                    .text_color(if empty { theme::muted() } else { theme::bg() }),
            )
            .on_click(cx.listener(|this, _, _, cx| this.send(cx)))
            .into_any_element()
    }

    /// The composer: one slim row, or the task form over the same row.
    pub(super) fn composer(
        &self,
        faces: &Faces,
        busy: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let empty = self.input.read(cx).is_empty();
        let focused = self.input.focus_handle(cx).is_focused(window);
        let task = self.mode == Mode::Task;
        let border = match self.mode {
            Mode::Directive => theme::purple().opacity(if focused { 0.7 } else { 0.45 }),
            _ if focused => theme::ghost_line(),
            _ => theme::border2(),
        };
        let controls = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .child(self.modes(faces, cx))
            .when(self.open.is_none(), |d| d.child(self.brain_chip(faces, cx)));
        let row = if task {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .pt(px(2.))
                .child(controls)
        } else {
            div()
                .flex()
                .items_end()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pl(px(6.))
                        .child(self.input.clone()),
                )
                .child(div().pb(px(4.)).child(controls))
                .child(
                    div()
                        .pb(px(4.))
                        .child(Self::send_button(faces, empty, busy, cx)),
                )
        };
        let field = div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(thread::COLUMN))
            .px(px(5.))
            .py(px(4.))
            .bg(theme::panel())
            .border_1()
            .border_color(border)
            .rounded(px(10.))
            .when(task, |d| d.child(self.task_form(faces, cx)))
            .child(row);
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(32.))
            .pb(px(14.))
            .child(field)
            .children(self.notice_line(faces, cx))
    }

    /// A small line saying what the desk just did or could not; a click
    /// puts it away.
    pub(crate) fn notice_line(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.notice.as_ref()?;
        Some(
            div()
                .id("notice")
                .w_full()
                .max_w(px(thread::COLUMN))
                .px(px(12.))
                .cursor_pointer()
                .child(
                    ui::text(faces, n.clone())
                        .text_size(px(11.5))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.notice = None;
                    cx.notify();
                }))
                .into_any_element(),
        )
    }
}
