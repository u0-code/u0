//! What waits beside the composer: what you put aside from it (⌘S), and
//! what is to be said later in the session, each a chip under the box
//! with its list over it.

use gpui::component::{
    button::Button,
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
};
use gpui::{Context, Div, Entity, SharedString, Window, div, prelude::*, px};

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
            .px(px(8.))
            .rounded(px(0.))
            .text_color(theme::role(u0ui::tokens::ColorRole::TextSecondary))
            .child(ui::icon(icon, 14.))
            .when(count > 0, |d| {
                d.child(ui::text(faces, count.to_string()).text_size(px(crate::style::CAPTION)))
            })
            .tooltip(crate::tr(tip))
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
            move |d, menu, desk, _| d.stashed_list(&faces, menu, desk),
            cx,
        ))
    }

    /// A line of a menu that can be let go (its ×), as `let_go` does.
    fn going_item(
        desk: &Entity<Self>,
        id: SharedString,
        line: impl Fn(&Faces) -> Div + 'static,
        faces: &Faces,
        let_go: impl Fn(&mut Self) + 'static,
    ) -> PopupMenuItem {
        let (desk, faces, let_go) = (desk.clone(), faces.clone(), std::rc::Rc::new(let_go));
        PopupMenuItem::element(move |_, _| {
            let (desk, let_go) = (desk.clone(), let_go.clone());
            line(&faces)
                .w(px(300.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    ui::icon_button(id.clone(), "x", crate::tr("Let it go"))
                        .size(px(22.))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            desk.update(cx, |d, cx| {
                                let_go(d);
                                d.menu_changed += 1;
                                cx.notify();
                            });
                        }),
                )
        })
    }

    /// What was put aside, the latest first: a click takes it back.
    fn stashed_list(&self, faces: &Faces, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        menu = menu.label(crate::tr("Put aside"));
        for (n, stashed) in self.composer.stashed.iter().enumerate().rev() {
            let first = stashed.text.lines().next().unwrap_or_default().to_owned();
            let with = stashed.attached.len();
            let line = move |faces: &Faces| {
                div()
                    .child(
                        ui::one_line(
                            ui::text(faces, first.clone()).text_size(px(crate::style::LABEL)),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .when(with > 0, |d| {
                        d.child(
                            ui::text(faces, format!("+{with}"))
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )
                    })
            };
            let id = SharedString::from(format!("stash-drop-{n}"));
            menu = menu.item(
                Self::going_item(desk, id, line, faces, move |this| {
                    if n < this.composer.stashed.len() {
                        this.composer.stashed.remove(n);
                        this.save_stash();
                    }
                })
                .on_click(Self::does(desk, move |this, window, cx| {
                    this.take_stashed(n, window, cx);
                })),
            );
        }
        menu
    }

    /// The clock: what is in the composer said later, once or each day,
    /// and what is waiting to be said, each can be let go.
    pub(super) fn later_menu(
        &self,
        faces: &Faces,
        s: &crate::session::Session,
        cx: &mut Context<Self>,
    ) -> Popover {
        let mark = Self::popup_chip(faces, "clock", self.waiting_in(s).len(), "Say it later");
        let (faces, key) = (faces.clone(), s.key);
        over_line(self.menu(
            Popup::Later,
            mark,
            move |d, menu, desk, cx| d.later_list(&faces, key, menu, desk, cx),
            cx,
        ))
    }

    /// When to say what is written, and what waits to be said.
    fn later_list(
        &self,
        faces: &Faces,
        key: u64,
        mut menu: PopupMenu,
        desk: &Entity<Self>,
        cx: &gpui::App,
    ) -> PopupMenu {
        use crate::desk::Later;
        let written = !self.typed(cx).trim().is_empty();
        menu = menu.label(crate::tr(if written {
            "Say what you wrote"
        } else {
            "Write it first, then say when"
        }));
        if written {
            for (label, when) in [
                ("In 30 minutes", Later::In(30)),
                ("In an hour", Later::In(60)),
                ("In 3 hours", Later::In(180)),
                ("Tomorrow at 9:00", Later::Tomorrow),
                ("Each day at 9:00", Later::Daily),
            ] {
                menu = menu.item(Self::menu_item(
                    desk,
                    crate::tr(label),
                    move |this, window, cx| {
                        this.send_later(when, window, cx);
                    },
                ));
            }
        }
        let waiting = self
            .sessions
            .at(key)
            .map(|s| self.waiting_in(s))
            .unwrap_or_default();
        if !waiting.is_empty() {
            let now = std::time::SystemTime::now();
            menu = menu.separator().label(crate::tr("Waiting to be said"));
            for task in waiting {
                let id = task.id;
                let when = match &task.schedule {
                    crate::tasks::Schedule::Once(at) => {
                        crate::when::said_at(crate::store::time(*at), now, self.settings.twelve)
                    }
                    other => other.said(self.settings.twelve),
                };
                let prompt = task.prompt.lines().next().unwrap_or_default().to_owned();
                let line = move |faces: &Faces| {
                    div()
                        .child(
                            ui::text(faces, when.clone())
                                .flex_none()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )
                        .child(
                            ui::one_line(
                                ui::text(faces, prompt.clone()).text_size(px(crate::style::LABEL)),
                            )
                            .flex_1()
                            .min_w(px(0.)),
                        )
                };
                let drop = SharedString::from(format!("later-drop-{id}"));
                menu = menu.item(Self::going_item(desk, drop, line, faces, move |this| {
                    this.drop_task(id);
                }));
            }
        }
        menu
    }
    /// The scheduled tasks said in the session `s` that are still to run.
    fn waiting_in(&self, s: &crate::session::Session) -> Vec<&crate::tasks::Task> {
        let tasks = self
            .tasks_on(s.host)
            .and_then(|r| r.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default();
        (tasks.iter())
            .filter(|t| t.same && !t.paused && t.next.is_some() && t.agent == s.meta.agent)
            .filter(|t| t.session.is_some() && t.session == s.meta.id)
            .collect()
    }

    /// Take back what was put aside `n`th: into the line, after what is
    /// in it, and what was attached to it with it.
    pub(crate) fn take_stashed(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if n >= self.composer.stashed.len() {
            return;
        }
        let crate::desk::mention::Stashed { text, attached } = self.composer.stashed.remove(n);
        self.composer.draft.attached.extend(attached);
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
