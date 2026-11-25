//! Adding a project, as t3code adds one: a folder already here, a
//! repository cloned from its address (its progress shown, and stoppable),
//! a new one made, a folder on another machine reached over SSH, or a
//! scratch session in a folder of its own.

use gpui::{Context, Div, div, prelude::*, px};

use super::{card, fine};
use crate::fonts::Weighted as _;
use crate::{
    desk::Desk,
    fonts::Faces,
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// The folders the agents already worked in that are not projects,
    /// each a click from being one.
    fn worked_here(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let found = self.found_elsewhere();
        if found.is_empty() {
            return None;
        }
        let mut rows = card();
        for (n, (folder, count, agents)) in found.into_iter().take(6).enumerate() {
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(40.))
                .px(px(12.))
                .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                .child(ui::icon("folder", 14.).text_color(theme::faint()))
                .child(
                    ui::one_line(
                        ui::mono(faces, Desk::shown(&folder))
                            .text_size(px(12.5))
                            .text_color(theme::text()),
                    )
                    .flex_1(),
                );
            for agent in &agents {
                row = row.child(ui::mark(
                    faces,
                    &ui::Mark::of(agent, &self.name_of(agent)),
                    14.,
                ));
            }
            row = row
                .child(fine(
                    faces,
                    format!("{count} session{}", if count == 1 { "" } else { "s" }),
                ))
                .child(
                    ui::button(
                        faces,
                        gpui::SharedString::from(format!("adopt-{n}")),
                        "Add",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.adopt(&folder);
                        cx.notify();
                    })),
                );
            rows = rows.child(row);
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pb(px(40.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(ui::label(faces, "Agents already worked here").flex_1())
                        .child(fine(faces, "from their own session lists")),
                )
                .child(rows),
        )
    }

    pub(crate) fn new_project_page(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let field = |child: gpui::Entity<crate::input::TextInput>| {
            div()
                .flex()
                .items_center()
                .h(px(34.))
                .px(px(8.))
                .rounded(px(8.))
                .bg(theme::well())
                .border_1()
                .border_color(theme::border2())
                .child(div().flex_1().child(child))
        };
        let heading = |text: &'static str| ui::text(faces, text).weight(gpui::FontWeight::MEDIUM);
        let parent = div()
            .id("parent")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(8.))
            .rounded(px(7.))
            .border_1()
            .border_color(theme::tag_line())
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(ui::icon("folder", 12.).text_color(theme::faint()))
            .child(
                ui::mono(faces, Desk::shown(&self.parent))
                    .text_size(px(12.))
                    .text_color(theme::text2()),
            )
            .on_click(cx.listener(|_, _, _, cx| Self::choose_parent(cx)));

        let mut clone = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .child(heading("Clone a repository"))
            .child(field(self.clone_url.clone()));
        if let Some(c) = &self.cloning {
            clone = clone
                .children(c.share.map(|share| {
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded(px(2.))
                        .bg(theme::tag_line())
                        .overflow_hidden()
                        .child(div().h_full().w(gpui::relative(share)).bg(theme::accent()))
                }))
                .child(fine(faces, c.what.clone()))
                .child(
                    div().flex().child(
                        ui::button(faces, "cancel-clone", "Cancel", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_clone(cx))),
                    ),
                );
        } else {
            clone = clone.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button(faces, "do-clone", "Clone", Weight::Primary)
                            .on_click(cx.listener(|this, _, _, cx| this.start_clone(cx))),
                    )
                    .child(fine(
                        faces,
                        "into a folder of its name, in the folder above",
                    )),
            );
        }

        let create = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .border_t_1()
            .border_color(theme::border())
            .child(heading("Make a new project"))
            .child(field(self.new_name.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button(faces, "do-create", "Create", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.create_project(cx))),
                    )
                    .child(fine(
                        faces,
                        "a repository with a README, in the folder above",
                    )),
            );

        let remote = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .border_t_1()
            .border_color(theme::border())
            .child(heading("On another machine"))
            .child(field(self.remote_at.clone()))
            .when(!self.hosts.is_empty(), |d| {
                let mut chips = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(fine(faces, "From ~/.ssh/config"));
                for (n, host) in self.hosts.iter().take(8).enumerate() {
                    let host = host.clone();
                    chips = chips.child(
                        div()
                            .id(gpui::SharedString::from(format!("host-{n}")))
                            .flex()
                            .items_center()
                            .h(px(24.))
                            .px(px(8.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme::tag_line())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(
                                ui::mono(faces, host.clone())
                                    .text_size(px(12.))
                                    .text_color(theme::text2()),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick_host(&host, window, cx);
                            })),
                    );
                }
                d.child(chips)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button(faces, "do-remote", "Add", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.add_remote(cx))),
                    )
                    .child(fine(
                        faces,
                        "over SSH with your key; the agent runs there by npx or uvx",
                    )),
            );

        let elsewhere = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .border_t_1()
            .border_color(theme::border())
            .child(heading("Or"))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::button(faces, "open-folder", "Open a folder…", Weight::Secondary)
                            .on_click(cx.listener(|_, _, _, cx| Self::add_project(cx))),
                    )
                    .child(
                        ui::button(faces, "scratch", "Scratch session", Weight::Quiet).on_click(
                            cx.listener(|this, _, window, cx| this.new_scratch(window, cx)),
                        ),
                    ),
            );

        div().flex().flex_1().justify_center().child(
            div()
                .flex()
                .flex_col()
                .gap(px(18.))
                .w(px(560.))
                .pt(px(56.))
                .child(
                    ui::text(faces, "Add a project")
                        .text_size(px(22.))
                        .line_height(px(30.))
                        .weight(gpui::FontWeight::SEMIBOLD),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(fine(faces, "New and cloned projects go in"))
                        .child(parent),
                )
                .children(
                    self.adding_trouble
                        .clone()
                        .map(|why| ui::trouble(faces, why)),
                )
                .child(
                    card()
                        .child(clone)
                        .child(create)
                        .child(remote)
                        .child(elsewhere),
                )
                .children(self.worked_here(faces, cx)),
        )
    }
}
