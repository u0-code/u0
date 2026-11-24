//! The bar over the middle: what is on screen, and whether it is running.

use gpui::{Context, Div, div, prelude::*, px};

use super::meta;
use crate::{
    desk::{Desk, Page, TOPBAR, draggable},
    fonts::Faces,
    hub::Stage,
    session::State,
    theme,
    ui::{self, Mark},
    when,
};

impl Desk {
    pub(crate) fn topbar(&self, faces: &Faces, lights: f32, cx: &mut Context<Self>) -> Div {
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
                ui::icon_button("show-sidebar", "sidebar", "Show the sidebar · ⌘B")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            );
        }
        let title = |t: String| {
            ui::one_line(
                ui::text(faces, t)
                    .text_size(px(13.5))
                    .font_weight(gpui::FontWeight::MEDIUM),
            )
            .w_auto()
            .flex_shrink()
        };
        match &self.page {
            Page::Thread => {
                if let Some(s) = self.session() {
                    let name = self.name_of(&s.agent);
                    let folder = s.folder.file_name().map_or_else(
                        || s.folder.display().to_string(),
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
                        s.title.clone()
                    };
                    bar = bar
                        .child(ui::mark(faces, &Mark::of(&s.agent, &name), 16.))
                        .child(title(ui::fit(&shown, 64)))
                        .child(meta(faces, place).flex_none())
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
                    None if self.entry(id).is_some_and(|e| e.launch(&self.home).is_ok()) => "Off",
                    Some(Stage::Missing(_)) | None => "Not installed",
                    Some(Stage::Installing(_)) => "Installing",
                    Some(Stage::Running { .. }) => "Running",
                    Some(Stage::Failed(_)) => "Stopped",
                    Some(_) => "Installed",
                };
                bar = bar
                    .child(ui::mark(faces, &Mark::of(id, &name), 16.))
                    .child(title(name))
                    .child(meta(faces, state));
            }
            Page::Settings => {
                bar = bar.child(title("Settings".into()));
            }
        }
        bar
    }
}
