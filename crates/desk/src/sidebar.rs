//! The column down the left, as t3code's: the window's lights with back
//! and ahead; the search, the projects shown and a new session; the list
//! (`list`); and at the foot the settings and what the agents need of you.

mod list;

pub(crate) use list::side_tree;

use gpui::component::InteractiveElementExt as _;
use gpui::component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    resizable::{HandleEdge, resize_handle},
    separator::Separator,
    tree::tree,
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, Page, Popup, TOPBAR, draggable},
    fonts::Faces,
    hub::Stage,
    theme, ui,
};

impl Desk {
    pub(crate) fn sidebar(
        &self,
        faces: &Faces,
        lights: f32,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        // The window's lights, and back and ahead through the sessions seen
        // at the end of the sidebar's part of the bar.
        let step = |id: &'static str, icon: &'static str, tip: &str, name: &str, ahead: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .child(ui::icon(icon, 14.))
                .tooltip(crate::keys::hint(cx, tip, name))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.step_trail(ahead, window, cx);
                }))
        };
        let head = draggable()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(TOPBAR))
            .pl(px(lights))
            .pr(px(8.))
            .child(div().flex_1())
            .child(step("back", "arrow-left", "Back", "back", false))
            .child(step("ahead", "arrow-right", "Ahead", "ahead", true));

        // t3code's row under it: the search, the projects to show, and a
        // new session (⇧-click: in the project on screen).
        let projects_faces = faces.clone();
        let controls = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .hover(|d| d.bg(theme::row_hover()))
                    .child(
                        Input::new(&self.search.input)
                            .appearance(false)
                            .cleanable(true)
                            .prefix(ui::icon("search", 16.).text_color(theme::muted())),
                    ),
            )
            .child(
                self.menu(
                    Popup::Projects,
                    Button::new("projects")
                        .ghost()
                        .small()
                        .child(ui::icon("folder", 16.))
                        .tooltip("Projects")
                        .selected(!self.filter.is_empty()),
                    move |d, _, cx| d.projects_menu(&projects_faces, cx),
                    cx,
                )
                .anchor(gpui::Anchor::TopRight),
            )
            .child(
                Button::new("new-session")
                    .ghost()
                    .small()
                    .child(ui::icon("square-pen", 16.))
                    .tooltip(crate::keys::hint(cx, "New session", "new_session"))
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                        if e.modifiers().shift {
                            this.new_default(window, cx);
                        } else {
                            this.new_anywhere(window, cx);
                        }
                    })),
            );

        // The list, gpui-kit's Tree, its rows drawn from the desk; with no
        // project yet, the way to add one.
        let desk = cx.entity();
        let menu_desk = desk.clone();
        let list = if self.projects().is_empty() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .pt(px(24.))
                .child(
                    ui::text(faces, "No projects yet")
                        .text_size(px(12.))
                        .text_color(theme::muted()),
                )
                .child(
                    Button::new("first-project")
                        .outline()
                        .small()
                        .label("Add project")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_project_page(window, cx);
                        })),
                )
                .flex_1()
        } else {
            div()
                .flex_1()
                .min_h(px(0.))
                .px(px(8.))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if this.side_key(&e.keystroke, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .child(
                    tree(&self.side, move |_, entry, _, _, cx| {
                        desk.read(cx).side_row(entry, &desk, cx)
                    })
                    .context_menu(move |_, entry, menu, window, cx| {
                        let id = entry.item().id.clone();
                        Desk::side_menu(&menu_desk, &id, menu, window, cx)
                    }),
                )
        };

        // t3code's foot: a row of icons, and at its end what the agents
        // need of you, each a click from its page.
        let mut pills = div().flex().items_center().gap(px(4.));
        for slot in &self.slots {
            let word = match &slot.stage {
                Stage::Ready | Stage::Running { .. }
                    if matches!(slot.auth, crate::hub::Auth::Needed(_)) =>
                {
                    "Sign in"
                }
                Stage::Missing(_) => "Install",
                Stage::Installing(_) => "Installing",
                Stage::Failed(_) => "Stopped",
                _ => continue,
            };
            let id = slot.entry.id.clone();
            pills = pills.child(
                Button::new(SharedString::from(format!("pill-{id}")))
                    .outline()
                    .xsmall()
                    .child(ui::mark(faces, &self.mark_of(&id), 12.))
                    .child(word)
                    .tooltip(slot.entry.name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_page(Page::Agent(id.clone()), window, cx);
                    })),
            );
        }
        // Each an action of the window's, its key in its tip.
        let nav = |id: &'static str, icon: &'static str, tip: &'static str, on: bool| {
            Button::new(id)
                .ghost()
                .child(ui::icon(icon, 16.))
                .tooltip(crate::keys::hint(cx, tip, id))
                .selected(on)
                .on_click(move |_, window, cx| {
                    if let Some(action) = crate::keys::action(id) {
                        window.dispatch_action(action, cx);
                    }
                })
        };
        let forge = |kind| self.page == Page::Forge(kind);
        let usage = self.pages.open && self.pages.tab == crate::pages::Tab::Usage;
        let foot = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(nav(
                "settings",
                "settings",
                "Settings",
                self.pages.open && !usage,
            ))
            .child(nav(
                "pull_requests",
                "pull-request",
                "Pull requests",
                forge(crate::github::Kind::Pr),
            ))
            .child(nav(
                "issues",
                "circle-dot",
                "Issues",
                forge(crate::github::Kind::Issue),
            ))
            .child(nav("usage", "gauge", "Usage", usage))
            .child(div().flex_1())
            .child(pills);

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::border())
            // Folders dropped from the Finder are added as projects.
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                for dir in paths.paths().iter().filter(|p| p.is_dir()) {
                    this.adopt(dir);
                }
                cx.notify();
            }))
            .child(head)
            .child(controls)
            .child(list)
            .child(foot)
            // Its edge: dragged, its width; double-clicked, t3code's.
            .child(
                resize_handle::<LeftEdge, LeftEdge>("left-edge", gpui::Axis::Horizontal)
                    .inside(HandleEdge::Trailing)
                    .on_drag(LeftEdge, |edge, _, _, cx| cx.new(|_| edge.clone()))
                    .on_double_click(cx.listener(|this, _, _, cx| {
                        this.settings.left = crate::settings::LEFT;
                        this.save_settings();
                        cx.notify();
                    })),
            )
    }

    /// The projects the sidebar shows, as t3code's picker: searched, each
    /// chosen or not (none chosen, all shown), and a project added.
    fn projects_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let typed = self.menu_search.read(cx).value().trim().to_owned();
        let query = crate::desk::rank::Query::new(&typed);
        let mut menu = div().flex().flex_col().w(px(240.)).child(
            div()
                .pb(px(4.))
                .child(Input::new(&self.menu_search).xsmall()),
        );
        for (n, p) in self.projects().into_iter().enumerate() {
            let name = self.project_name(&p);
            if !query.is_empty() && crate::desk::rank::terms(&query, &name, &[], true).is_none() {
                continue;
            }
            let on = self.filter.contains(&p);
            let icon = self.held.get(&p).and_then(|h| h.icon.clone());
            menu = menu.child(
                ui::menu_row(SharedString::from(format!("filter-{n}")), on)
                    .child(crate::desk::projects::project_icon(&p, icon, 14.))
                    .child(ui::one_line(ui::text(faces, name).text_size(px(13.))).flex_1())
                    .when(on, |d| d.child(ui::icon("check", 14.)))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_filter(&p, cx))),
            );
        }
        menu.child(Separator::horizontal().my(px(4.))).child(
            ui::menu_row("filter-add", false)
                .child(ui::icon("folder-plus", 14.))
                .child(ui::text(faces, "Add project").text_size(px(13.)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_popup(window, cx);
                    this.add_project_page(window, cx);
                })),
        )
    }
}

/// The sidebar's edge, dragged.
#[derive(Clone)]
pub struct LeftEdge;

impl gpui::Render for LeftEdge {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A session's row being dragged, as it shows under the pointer.
#[derive(Clone)]
pub(crate) struct Dragged {
    key: u64,
    title: SharedString,
}

impl gpui::Render for Dragged {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(5.))
            .rounded(px(8.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .shadow_lg()
            .text_size(px(13.))
            .text_color(theme::text())
            .child(self.title.clone())
    }
}

/// What an agent said, as a line of plain words for a card: its prose
/// only (no code, no drawn trees or tables), the marks of Markdown taken
/// off, about two lines of a tip.
pub(crate) fn gist(said: &str) -> String {
    let mut words = String::new();
    let mut code = false;
    for line in said.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        let drawn = line.contains(['│', '├', '└', '┌', '─']) || line.starts_with('|');
        if code || drawn || line.is_empty() {
            continue;
        }
        let line = line
            .trim_start_matches(['#', '>', '-', '*', ' '])
            .replace(['`', '*'], "");
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(line.trim());
        if words.chars().count() > 120 {
            break;
        }
    }
    ui::fit(&words, 110)
}

#[cfg(test)]
mod tests {
    use super::gist;

    #[test]
    fn a_cards_words_are_the_prose_without_code_trees_or_marks() {
        let said = "这次调研生成的文件都在 `/Users/x/jepa` 下：\n```\n/Users/x/jepa/\n├── README.md\n└── reports/\n```\n## 结论\n- **核心思想**：JEPA 在表征空间里预测。";
        assert_eq!(
            gist(said),
            "这次调研生成的文件都在 /Users/x/jepa 下： 结论 核心思想：JEPA 在表征空间里预测。"
        );
    }
}
