//! The drawer under the thread (⌘J): the commands run beside the session,
//! the last two side by side, each with how it stands, its output's last
//! lines, a way to stop it and, where it printed a local address, to open
//! it; the repository's own scripts; and the line a command is typed in.

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, runs},
    fonts::Faces,
    theme,
    ui::{self, Weight},
};

/// How tall the drawer stands.
const HEIGHT: f32 = 240.;

/// How many of a run's last lines show.
const LINES: usize = 60;

impl Desk {
    pub(crate) fn drawer(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        if !self.drawer {
            return None;
        }
        let folder = self.session().map(|s| s.folder.clone());
        let scripts = folder
            .as_deref()
            .map(runs::config)
            .unwrap_or_default()
            .scripts;
        let mut panes = div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .gap(px(1.))
            .bg(theme::border());
        let shown: Vec<&runs::Run> = self.runs.iter().rev().take(2).collect();
        for r in shown.into_iter().rev() {
            panes = panes.child(Self::run_pane(faces, r, cx));
        }
        if self.runs.is_empty() {
            panes = panes.child(
                div().flex_1().bg(theme::bg()).p(px(14.)).child(
                    ui::text(
                        faces,
                        "Commands run here, in the session's folder, show their output here.",
                    )
                    .text_size(px(12.5))
                    .text_color(theme::faint()),
                ),
            );
        }
        let mut line = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_t_1()
            .border_color(theme::border())
            .child(ui::mono(faces, "$").text_color(theme::faint()))
            .child(div().flex_1().min_w(px(0.)).child(self.command.clone()));
        for (n, script) in scripts.into_iter().enumerate() {
            let (dir, command) = (folder.clone(), script.command.clone());
            line = line.child(
                ui::button(
                    faces,
                    SharedString::from(format!("script-{n}")),
                    script.name.clone(),
                    Weight::Quiet,
                )
                .tooltip(ui::tip(script.command))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(dir) = dir.clone() {
                        this.run_command(dir, command.clone(), cx);
                    }
                })),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .h(px(HEIGHT))
                .border_t_1()
                .border_color(theme::border())
                .bg(theme::rail())
                .child(panes)
                .child(line),
        )
    }

    fn run_pane(faces: &Faces, r: &runs::Run, cx: &mut Context<Self>) -> Div {
        let id = r.id;
        let (word, color) = match &r.done {
            None => ("running".to_owned(), theme::accent()),
            Some(Ok(0)) => ("done".to_owned(), theme::ok()),
            Some(Ok(code)) => (format!("exit {code}"), theme::coral()),
            Some(Err(why)) => (why.clone(), theme::coral()),
        };
        let tail: Vec<&str> = r.output.lines().collect();
        let from = tail.len().saturating_sub(LINES);
        let address = r.address();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .bg(theme::bg())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(ui::one_line(
                        ui::mono(faces, r.command.clone())
                            .text_size(px(12.))
                            .text_color(theme::text()),
                    ))
                    .child(
                        ui::text(faces, word)
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(color),
                    )
                    .children(address.map(|url| {
                        ui::button(
                            faces,
                            SharedString::from(format!("open-{id}")),
                            "Open preview",
                            Weight::Quiet,
                        )
                        .tooltip(ui::tip(url.clone()))
                        .on_click(move |_, _, cx| cx.open_url(&url))
                    }))
                    .when(r.done.is_none(), |d| {
                        d.child(
                            ui::icon_button(
                                SharedString::from(format!("stop-{id}")),
                                "square",
                                "Stop",
                            )
                            .size(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
                        )
                    })
                    .child(
                        ui::icon_button(SharedString::from(format!("close-{id}")), "x", "Close")
                            .size(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.close_run(id, cx))),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("out-{id}")))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(12.))
                    .py(px(6.))
                    .child(
                        ui::mono(faces, tail[from..].join("\n"))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(theme::text2()),
                    ),
            )
    }
}
