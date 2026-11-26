//! Settings › Diagnostics, as t3code's: the processes Ghost runs, as a
//! tree (agents, what they started, your terminals) with their CPU and
//! memory and this Mac's load, each stopped from here; and the warnings
//! and errors in Ghost's log.

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{card, list_row};
use crate::{
    desk::Desk,
    fonts::Faces,
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// The Diagnostics tab's processes, this Mac, and the log's troubles.
    pub(super) fn diagnostics(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut tree = card().overflow_hidden();
        let (agents, processes) = (&self.pages.processes.0, &self.pages.processes.1);
        for (n, p) in processes.procs.iter().enumerate() {
            let agent = agents
                .iter()
                .find(|(_, pid)| *pid == p.pid)
                .map(|(id, _)| id.clone());
            let name = agent
                .as_ref()
                .map_or_else(|| p.name.clone(), |id| self.name_of(id));
            let pid = p.pid;
            let control = match agent {
                Some(id) => ui::button(
                    SharedString::from(format!("let-go-{n}")),
                    "Let it go",
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.let_go(&id);
                    this.weigh_agents();
                    cx.notify();
                })),
                None => ui::button(
                    SharedString::from(format!("stop-{n}")),
                    "Stop",
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, _, _| this.stop_process(pid))),
            };
            tree = tree.child(
                list_row(
                    faces,
                    &name,
                    &format!("{pid} · {:.1}% CPU · {} MB", p.cpu, p.kb / 1024),
                    control,
                )
                .pl(px(16. + 18. * (p.depth.saturating_sub(1)) as f32))
                .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
            );
        }
        if processes.procs.is_empty() {
            tree = tree.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .child(ui::fine(faces, "Ghost runs nothing else now.")),
            );
        }
        let memory = processes.memory / (1 << 30);
        let held: u64 = processes.procs.iter().map(|p| p.kb).sum::<u64>() / 1024;
        let mac = format!(
            "This Mac: load {} on {} cores, {memory} GB of memory; Ghost's processes hold {held} MB.",
            processes.load, processes.cores
        );
        let mut log = card().overflow_hidden();
        for (n, line) in self.pages.troubles.iter().enumerate() {
            log = log.child(
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::fine_mono(faces, line.clone()).text_color(
                        if line.contains(" ERROR ") {
                            theme::coral()
                        } else {
                            theme::text2()
                        },
                    )),
            );
        }
        if self.pages.troubles.is_empty() {
            log = log.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .child(ui::fine(faces, "Nothing went wrong lately.")),
            );
        }
        let section = |label: &str, body: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::label(faces, label))
                .child(body)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::fine(faces, mac).flex_1())
                    .child(
                        ui::button("diagnostics-read", "Read again", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, _| this.diagnose())),
                    ),
            )
            .child(section("Processes", tree))
            .child(section("Warnings and errors in the log", log))
    }
}
