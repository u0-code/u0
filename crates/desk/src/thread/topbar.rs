//! The bar over the middle: the session's title and its menu, the terminal
//! and the right column.

use gpui::component::{
    Selectable as _, Sizable as _, input::Input, list::ListItem, separator::Separator,
};
use gpui::{Context, Div, div, prelude::*, px};

use super::meta;
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page, Popup, Renaming, TOPBAR, draggable},
    fonts::Faces,
    hub::Stage,
    settings::View,
    theme, ui,
};

impl Desk {
    /// The bar over the middle, `pane` points wide.
    pub(crate) fn topbar(
        &self,
        faces: &Faces,
        lights: f32,
        pane: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut bar = draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(px(TOPBAR))
            .pl(px(if self.panes.left { 16. } else { lights }))
            .pr(px(12.))
            .border_b_1()
            .border_color(theme::border());
        if !self.panes.left {
            bar = bar.child(
                ui::icon_button(
                    "show-sidebar",
                    "sidebar",
                    crate::keys::hint(cx, "Show the sidebar", "toggle_sidebar"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            );
        }
        let title = |t: String| {
            ui::one_line(
                ui::text(faces, t)
                    .text_size(px(13.5))
                    .weight(gpui::FontWeight::MEDIUM),
            )
            .w_auto()
            .flex_shrink_1()
        };
        if self.page != Page::Thread {
            bar = bar.child(
                ui::icon_button("back", "chevron-left", "Back · Esc").on_click(
                    cx.listener(|this, _, window, cx| this.open_page(Page::Thread, window, cx)),
                ),
            );
        }
        match &self.page {
            Page::Thread => {
                if let Some(s) = self.session() {
                    // The title alone, as Claude sets it: a click renames
                    // it where it is, its caret opens the session's menu.
                    let new = s.items.is_empty();
                    let shown = if new {
                        "New session".to_owned()
                    } else {
                        ui::fit(&s.meta.title, ui::cols(pane - 200.))
                    };
                    let key = s.key;
                    let here = Renaming::Session { key, here: true };
                    let named: gpui::AnyElement = if self.renaming.as_ref() == Some(&here) {
                        div()
                            .w(px(320.))
                            .child(Input::new(&self.rename).small())
                            .into_any_element()
                    } else {
                        div()
                            .id("title")
                            .flex_shrink_1()
                            .min_w(px(0.))
                            .child(ui::one_line(
                                ui::text(faces, shown)
                                    .text_size(px(14.))
                                    .text_color(if new { theme::muted() } else { theme::text() }),
                            ))
                            // Not the bar's: a click here renames, it does
                            // not move the window.
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .when(!new, |d| {
                                d.cursor_text()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.begin_rename(
                                            Renaming::Session { key, here: true },
                                            window,
                                            cx,
                                        );
                                    }))
                            })
                            .into_any_element()
                    };
                    let caret = ui::icon_button("title-menu", "chevron-down", "This session")
                        .size(px(22.))
                        .text_color(theme::faint());
                    let faces = faces.clone();
                    bar = bar
                        .child(named)
                        .child(self.menu(
                            Popup::Title,
                            caret,
                            move |d, _, cx| d.title_menu(&faces, cx),
                            cx,
                        ))
                        .child(div().flex_1());
                } else {
                    bar = bar.child(div().flex_1());
                }
                bar = bar
                    .child(
                        ui::icon_button(
                            "toggle-drawer",
                            "terminal",
                            crate::keys::hint(cx, "Terminal", "drawer"),
                        )
                        .selected(self.drawer.open)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_drawer(window, cx)),
                        ),
                    )
                    .child(
                        ui::icon_button(
                            "toggle-aside",
                            "panel-right",
                            crate::keys::hint(cx, "Show or hide the right column", "right_column"),
                        )
                        .selected(self.panes.right)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_aside(cx))),
                    );
            }
            Page::Agent(id) => {
                let name = self.name_of(id);
                let state = match self.slot(id).map(|s| &s.stage) {
                    None if self.here.get(id).is_some_and(|h| h.launches) => "Off",
                    Some(Stage::Missing(_)) | None => "Not installed",
                    Some(Stage::Installing(_)) => "Installing",
                    Some(Stage::Running { .. }) => "Running",
                    Some(Stage::Failed(_)) => "Stopped",
                    Some(_) => "Installed",
                };
                bar = bar
                    .child(ui::mark(faces, &self.mark_of(id), 16.))
                    .child(title(name))
                    .child(meta(faces, state));
            }
        }
        bar
    }

    /// Under the title's caret: how the transcript shows (⌃O goes round),
    /// then the session's folder in your editor, its id and pull request
    /// to copy, and archiving it.
    fn title_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.session() else {
            return div();
        };
        let row = |id: &'static str, says: &str| {
            ListItem::new(id)
                .h(px(28.))
                .px(px(8.))
                .rounded(px(6.))
                .child(ui::text(faces, says.to_owned()).text_size(px(13.)))
        };
        let mut menu = div().flex().flex_col().w(px(240.)).child(
            ui::text(faces, "Transcript view")
                .px(px(8.))
                .pt(px(4.))
                .pb(px(4.))
                .text_size(px(12.))
                .text_color(theme::faint()),
        );
        for (id, says, view) in [
            ("view-normal", "Normal", View::Normal),
            ("view-thinking", "Thinking", View::Thinking),
            ("view-verbose", "Verbose", View::Verbose),
        ] {
            let on = self.settings.view == view;
            menu = menu.child(
                row(id, says)
                    .when(on, |d| {
                        d.suffix(|_, _| ui::icon("check", 13.).text_color(theme::text2()))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx))),
            );
        }
        let (key, pr) = (s.key, s.git.pr.as_ref().map(|p| p.url.clone()));
        menu = menu
            .child(Separator::horizontal().my(px(4.)))
            .child(
                row("title-editor", "Open in your editor").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.close_popup(window, cx);
                        if let Some(s) = this.session() {
                            this.edit(s.meta.folder.clone(), None);
                        }
                    },
                )),
            )
            .child(
                row("title-copy-id", "Copy the session's id").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.close_popup(window, cx);
                        this.copy_of_session(|s| s.meta.id.as_ref().map(|i| i.0.to_string()), cx);
                    },
                )),
            );
        if pr.is_some() {
            menu = menu.child(
                row("title-copy-pr", "Copy the pull request's address").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.close_popup(window, cx);
                        this.copy_of_session(|s| s.git.pr.as_ref().map(|p| p.url.clone()), cx);
                    },
                )),
            );
        }
        menu.child(row("title-archive", "Archive").on_click(cx.listener(
            move |this, _, window, cx| {
                this.close_popup(window, cx);
                this.archive(key, cx);
            },
        )))
    }

    /// Show the transcript as `view` says, and keep it so.
    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.settings.view = view;
        self.save_settings();
        cx.notify();
    }
}
