//! The bar over the middle: what is on screen, and whether it is running.

use gpui::{Context, Div, div, prelude::*, px};

use super::meta;
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page, Renaming, TOPBAR, draggable},
    fonts::Faces,
    hub::Stage,
    session::State,
    theme, ui, when,
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
            .flex_shrink()
        };
        if self.page != Page::Thread {
            bar = bar.child(
                ui::icon_button("back", "chevron-left", "Back · Esc")
                    .on_click(cx.listener(|this, _, _, cx| this.open_page(Page::Thread, cx))),
            );
        }
        match &self.page {
            Page::Thread => {
                if let Some(s) = self.session() {
                    let name = self.name_of(&s.meta.agent);
                    // A worktree is named for its repository: its own folder
                    // is named for its branch, which follows.
                    let dir = s.meta.worktree.as_ref().map_or(&s.meta.folder, |w| &w.repo);
                    let folder = dir.file_name().map_or_else(
                        || dir.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let mut place = format!("{name} · {folder}");
                    if let Some(b) = &s.branch {
                        place.push_str(" · ");
                        place.push_str(b);
                    }
                    let shown = if s.items.is_empty() {
                        format!("New session with {name}")
                    } else {
                        s.meta.title.clone()
                    };
                    // The title first, then where it works in what room is
                    // left: beside them the mark, the state and the buttons.
                    let room = ui::cols(pane - 240.);
                    let shown = ui::fit(&shown, 48.min(room * 3 / 5));
                    let place = ui::fit(
                        &place,
                        room.saturating_sub(
                            unicode_width::UnicodeWidthStr::width(shown.as_str()) + 2,
                        ),
                    );
                    // A double click on the title renames it where it is.
                    let key = s.key;
                    let here = Renaming::Session { key, here: true };
                    let named: gpui::AnyElement = if self.renaming.as_ref() == Some(&here) {
                        div()
                            .w(px(320.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .px(px(6.))
                            .rounded(px(6.))
                            .bg(theme::panel())
                            .border_1()
                            .border_color(theme::ghost_line())
                            .child(self.rename.clone())
                            .into_any_element()
                    } else {
                        div()
                            .id("title")
                            .flex_none()
                            .child(title(shown))
                            // Not the bar's: a double click here renames, it
                            // does not zoom the window.
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                                if e.click_count() == 2 {
                                    this.begin_rename(
                                        Renaming::Session { key, here: true },
                                        window,
                                        cx,
                                    );
                                }
                            }))
                            .into_any_element()
                    };
                    bar = bar
                        .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
                        .child(named)
                        .child(
                            ui::one_line(meta(faces, place))
                                .w_auto()
                                .min_w(px(0.))
                                .flex_shrink(),
                        )
                        .child(div().flex_1());
                    if let (State::Working, Some(since)) = (&s.state, s.since) {
                        bar = bar.child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(6.))
                                .h(px(26.))
                                .px(px(8.))
                                .child(ui::live_dot())
                                .child(
                                    ui::text(
                                        faces,
                                        format!("Running · {}", when::took(since.elapsed())),
                                    )
                                    .text_size(px(12.5))
                                    .text_color(theme::accent()),
                                ),
                        );
                    }
                } else {
                    bar = bar.child(div().flex_1());
                }
                bar = bar.child(
                    ui::icon_button(
                        "toggle-aside",
                        "panel-right",
                        "Show or hide the right column",
                    )
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
            Page::NewProject => {
                bar = bar.child(title("Add a project".into()));
            }
        }
        bar
    }
}
