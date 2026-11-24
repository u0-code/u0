//! The pages that take the middle in place of a thread: an agent's own,
//! which says what it is and gets it ready to run, and the settings, where
//! agents are switched on and off.

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Stage,
    registry::{Missing, Way},
    style::{DISPLAY, META, NAME, S1, S2, S3, S4, SMALL},
    theme,
    ui::{self, Mark, Weight},
};

/// The width of a page's column.
const PAGE: f32 = 560.;

fn page() -> gpui::Stateful<Div> {
    div()
        .id("page")
        .flex()
        .flex_col()
        .items_center()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .py(px(56.))
}

impl Desk {
    pub(crate) fn agent_page(
        &self,
        faces: &Faces,
        id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(slot) = self.slot(id) else {
            return page()
                .child(ui::text(faces, "This agent is switched off.").text_color(theme::muted()));
        };
        let e = &slot.entry;
        let how = match e.way() {
            Way::Archive => {
                "Installed from its release archive, checked against the registry's digest."
                    .to_owned()
            }
            Way::Runner { runner, package } => {
                format!("Runs with `{runner} {package}`; the package is fetched on first start.")
            }
            Way::None => "No build for this machine.".to_owned(),
        };
        let (status, trouble) = match &slot.stage {
            Stage::Missing(m) => (m.say(&e.name), !matches!(m, Missing::Download)),
            Stage::Installing(stage) => (format!("{stage}…"), false),
            Stage::Ready => (
                "Installed. It starts with the first session.".to_owned(),
                false,
            ),
            Stage::Starting => ("Starting…".to_owned(), false),
            Stage::Running { init, .. } => {
                let who = init
                    .agent_info
                    .as_ref()
                    .map_or_else(String::new, |i| format!(" as {} {}", i.name, i.version));
                (format!("Running{who}."), false)
            }
            Stage::Failed(why) => (why.clone(), true),
        };
        let (a, b, c) = (id.to_owned(), id.to_owned(), id.to_owned());
        let mut actions = div().flex().gap(px(S2));
        match &slot.stage {
            Stage::Missing(Missing::Download) => {
                actions = actions.child(
                    ui::button(
                        faces,
                        "install",
                        format!("Install {}", e.name),
                        Weight::Primary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.install(&a, cx))),
                );
            }
            Stage::Missing(Missing::Runner(_)) | Stage::Failed(_) => {
                actions = actions.child(
                    ui::button(faces, "recheck", "Check again", Weight::Secondary)
                        .on_click(cx.listener(move |this, _, _, cx| this.recheck(&b, cx))),
                );
            }
            Stage::Ready | Stage::Running { .. } => {
                actions = actions.child(
                    ui::button(faces, "start", "New session", Weight::Primary).on_click(
                        cx.listener(move |this, _, window, cx| this.new_session(&c, window, cx)),
                    ),
                );
            }
            _ => {}
        }
        let link = e.website.clone().or_else(|| e.repository.clone());
        page().child(
            div()
                .flex()
                .flex_col()
                .gap(px(S4))
                .w(px(PAGE))
                .child(ui::mark(faces, &Mark::of(&e.id, &e.name), 48.))
                .child(ui::serif(faces, e.name.clone()).text_size(px(DISPLAY)))
                .child(ui::text(faces, e.description.clone()).text_color(theme::text2()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(S1))
                        .child(meta_row(faces, "Version", &e.version))
                        .child(meta_row(faces, "How", &how))
                        .children(link.map(|l| meta_row(faces, "Source", &l))),
                )
                .child(
                    ui::text(faces, status)
                        .text_size(px(SMALL))
                        .text_color(if trouble { theme::coral() } else { theme::text2() }),
                )
                .child(actions)
                .child(
                    ui::text(faces, "Each agent signs in its own way, with its own account: if it asks, run its CLI once in a terminal and sign in there.")
                        .text_size(px(META))
                        .text_color(theme::faint()),
                ),
        )
    }

    pub(crate) fn settings_page(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col();
        let mut agents: Vec<_> = self.registry.agents.iter().collect();
        // Those switched on first, in their order; then the rest by name.
        agents.sort_by_key(|e| {
            (
                self.settings
                    .enabled
                    .iter()
                    .position(|id| id == &e.id)
                    .unwrap_or(usize::MAX),
                e.name.to_lowercase(),
            )
        });
        for e in agents {
            let on = self.settings.enabled.contains(&e.id);
            let id = e.id.clone();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(S3))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(div().when(!on, |d| d.opacity(0.45)).child(ui::mark(
                        faces,
                        &Mark::of(&e.id, &e.name),
                        24.,
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::text(faces, e.name.clone()).text_size(px(NAME)))
                            .child(ui::one_line(
                                ui::text(faces, ui::fit(&e.description, 64))
                                    .text_size(px(META))
                                    .text_color(theme::faint()),
                            )),
                    )
                    .child(
                        toggle(SharedString::from(format!("toggle-{id}")), on).on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_agent(&id, cx)),
                        ),
                    ),
            );
        }
        let head = div()
            .flex()
            .items_end()
            .justify_between()
            .child(ui::serif(faces, "Agents").text_size(px(DISPLAY)))
            .child(
                ui::button(faces, "refresh", "Read the registry again", Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx))),
            );
        page().child(
            div()
                .flex()
                .flex_col()
                .gap(px(S4))
                .w(px(PAGE))
                .child(head)
                .child(
                    ui::text(faces, "Every agent that speaks the Agent Client Protocol, as its registry lists them. Those switched on are down the left.")
                        .text_size(px(SMALL))
                        .text_color(theme::muted()),
                )
                .children(self.settings_refused.clone().map(|why| trouble(faces, why)))
                .children(self.registry_trouble.clone().map(|why| trouble(faces, why)))
                .when(self.registry.agents.is_empty() && self.registry_trouble.is_none(), |d| {
                    d.child(ui::text(faces, "Reading the registry…").text_color(theme::muted()))
                })
                .child(list)
                .child(meta_row(faces, "Home", &Desk::shown(&self.home))),
        )
    }
}

fn meta_row(faces: &Faces, label: &str, value: &str) -> Div {
    div()
        .flex()
        .gap(px(S3))
        .child(
            ui::text(faces, label.to_owned())
                .w(px(72.))
                .flex_none()
                .text_size(px(META))
                .text_color(theme::faint()),
        )
        .child(
            ui::text(faces, value.to_owned())
                .text_size(px(META))
                .text_color(theme::text2()),
        )
}

fn trouble(faces: &Faces, why: String) -> Div {
    ui::text(faces, why)
        .text_size(px(SMALL))
        .text_color(theme::coral())
}

/// An on/off switch.
fn toggle(id: SharedString, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .w(px(30.))
        .h(px(18.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { theme::text2() } else { theme::border2() })
        .when(on, gpui::Styled::justify_end)
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(if on { theme::bg() } else { theme::muted() }),
        )
}
