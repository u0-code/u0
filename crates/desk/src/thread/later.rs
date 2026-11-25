//! What waits beside the composer: what you put aside from it (⌘S), and
//! what is to be said later in the session, each a chip under the box
//! with its list over it.

use gpui::component::{button::Button, popover::Popover};
use gpui::{Context, Div, SharedString, Window, div, prelude::*, px};

use super::composer::over_line;
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    theme, ui,
};

impl Desk {
    /// The mark that opens `popup`: its icon, and how many it holds when
    /// it holds any.
    fn popup_chip(faces: &Faces, icon: &'static str, count: usize, tip: &'static str) -> Button {
        ui::opener(SharedString::from(format!("chip-{icon}")))
            .h(px(26.))
            .px(px(7.))
            .text_color(theme::faint())
            .child(ui::icon(icon, 14.))
            .when(count > 0, |d| {
                d.child(ui::text(faces, count.to_string()).text_size(px(12.)))
            })
            .tooltip(tip)
    }

    /// What you put aside (⌘S): a mark with how many, and under it each,
    /// the latest first, with how many things are attached to it, to take
    /// back into the line or let go.
    pub(super) fn stash_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Popover {
        let mark = Self::popup_chip(
            faces,
            "archive",
            self.composer.stashed.len(),
            "Put aside (⌘S), to take back (⌘S on an empty line)",
        );
        let faces = faces.clone();
        over_line(self.menu(
            Popup::Stash,
            mark,
            move |d, _, cx| d.stashed_list(&faces, cx),
            cx,
        ))
    }

    /// What was put aside, the latest first: a click takes it back.
    fn stashed_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().w(px(348.)).child(
            ui::label(faces, "Put aside")
                .px(px(8.))
                .pt(px(6.))
                .pb(px(4.)),
        );
        for (n, stashed) in self.composer.stashed.iter().enumerate().rev() {
            let first = stashed.text.lines().next().unwrap_or_default().to_owned();
            let with = stashed.attached.len();
            list = list.child(
                ui::menu_row(SharedString::from(format!("stash-{n}")), false)
                    .child(
                        ui::one_line(ui::text(faces, first).text_size(px(13.)))
                            .flex_1()
                            .min_w(px(0.)),
                    )
                    .when(with > 0, |d| {
                        d.child(
                            ui::text(faces, format!("+{with}"))
                                .text_size(px(12.))
                                .text_color(theme::faint()),
                        )
                    })
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("stash-drop-{n}")),
                            "x",
                            "Let it go",
                        )
                        .size(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if n < this.composer.stashed.len() {
                                this.composer.stashed.remove(n);
                                this.save_stash();
                            }
                            cx.notify();
                        })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.take_stashed(n, window, cx);
                    })),
            );
        }
        list
    }

    /// The clock: what is in the composer said later, once or each day,
    /// and what is waiting to be said, each can be let go.
    pub(super) fn later_menu(
        &self,
        faces: &Faces,
        s: &crate::session::Session,
        cx: &mut Context<Self>,
    ) -> Popover {
        let mark = Self::popup_chip(faces, "clock", s.meta.later.len(), "Say it later");
        let (faces, key) = (faces.clone(), s.key);
        over_line(self.menu(
            Popup::Later,
            mark,
            move |d, _, cx| d.later_list(&faces, key, cx),
            cx,
        ))
    }

    /// When to say what is written, and what waits to be said.
    fn later_list(&self, faces: &Faces, key: u64, cx: &mut Context<Self>) -> Div {
        use crate::desk::Later;
        let written = !self.typed(cx).trim().is_empty();
        let mut list = div().flex().flex_col().w(px(308.)).child(
            ui::label(
                faces,
                if written {
                    "Say what you wrote"
                } else {
                    "Write it first, then say when"
                },
            )
            .px(px(8.))
            .pt(px(6.))
            .pb(px(4.)),
        );
        if written {
            for (n, (label, when)) in [
                ("In 30 minutes", Later::In(30)),
                ("In an hour", Later::In(60)),
                ("In 3 hours", Later::In(180)),
                ("Tomorrow at 9:00", Later::Tomorrow),
                ("Each day at 9:00", Later::Daily),
            ]
            .into_iter()
            .enumerate()
            {
                list = list.child(
                    ui::menu_row(SharedString::from(format!("later-{n}")), false)
                        .child(ui::text(faces, label).flex_1().text_size(px(13.)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.send_later(when, window, cx);
                        })),
                );
            }
        }
        let waiting = self
            .sessions
            .at(key)
            .map(|s| s.meta.later.as_slice())
            .unwrap_or_default();
        if !waiting.is_empty() {
            let now = std::time::SystemTime::now();
            list = list.child(
                ui::label(faces, "Waiting to be said")
                    .px(px(8.))
                    .pt(px(10.))
                    .pb(px(4.)),
            );
            for (n, l) in waiting.iter().enumerate() {
                let at = crate::store::time(l.at);
                let when = if l.daily {
                    format!("each day {}", crate::when::clock(at, self.settings.twelve))
                } else {
                    crate::when::said_at(at, now, self.settings.twelve)
                };
                list = list.child(
                    ui::menu_row(SharedString::from(format!("waiting-{n}")), false)
                        .child(
                            ui::text(faces, when)
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(theme::faint()),
                        )
                        .child(
                            ui::one_line(
                                ui::text(
                                    faces,
                                    l.text.lines().next().unwrap_or_default().to_owned(),
                                )
                                .text_size(px(13.)),
                            )
                            .flex_1()
                            .min_w(px(0.)),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("later-drop-{n}")),
                                "x",
                                "Let it go",
                            )
                            .size(px(22.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(s) = this.sessions.at_mut(key)
                                        && n < s.meta.later.len()
                                    {
                                        s.meta.later.remove(n);
                                    }
                                    this.save_index();
                                    cx.notify();
                                },
                            )),
                        ),
                );
            }
        }
        list
    }

    /// Take back what was put aside `n`th: into the line, after what is
    /// in it, and what was attached to it with it.
    pub(crate) fn take_stashed(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if n >= self.composer.stashed.len() {
            return;
        }
        let crate::desk::mention::Stashed { text, attached } = self.composer.stashed.remove(n);
        self.composer.attached.extend(attached);
        self.save_stash();
        self.popup = Popup::Closed;
        let typed = self.typed(cx);
        let line = if typed.is_empty() {
            text
        } else {
            format!("{typed}\n{text}")
        };
        self.put_line(&line, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }
}
