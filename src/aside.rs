//! The column down the right: what the session on screen has come to. Its
//! plan, the files it changed with the lines added and taken away, and how
//! full its context is, as the agent reports them.

use agent_client_protocol_schema::v1::PlanEntryStatus;
use gpui::{Context, Div, div, prelude::*, px};

use crate::{
    desk::{Desk, RIGHT, TOPBAR},
    fonts::Faces,
    session::Session,
    style::{FINE, META, NAME, S1, S2, S3, S4, SMALL},
    theme,
    thread::{drawn, kinds::Status},
    ui,
};

impl Desk {
    pub(crate) fn aside(&self, faces: &Faces, _cx: &mut Context<Self>) -> Div {
        let mut col = div()
            .id("aside")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(S4))
            .py(px(S3))
            .gap(px(S4 + S2));
        if let Some(s) = self.session() {
            col = col
                .child(
                    section(faces, "Folder")
                        .child(ui::mono(faces, Desk::shown(&s.folder)).text_color(theme::text2())),
                )
                .child(changes(faces, s))
                .children(plan(faces, s))
                .children(context(faces, s));
        }
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(RIGHT))
            .h_full()
            .border_l_1()
            .border_color(theme::border())
            .child(
                crate::desk::draggable()
                    .flex()
                    .items_center()
                    .h(px(TOPBAR))
                    .px(px(S4))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(
                        ui::text(faces, "Session")
                            .text_size(px(NAME))
                            .font_weight(gpui::FontWeight::MEDIUM),
                    ),
            )
            .child(col)
    }
}

/// A section of the column: its quiet head, and what is under it.
fn section(faces: &Faces, head: &str) -> Div {
    div().flex().flex_col().gap(px(S2)).child(
        ui::text(faces, head.to_owned())
            .text_size(px(FINE))
            .text_color(theme::faint()),
    )
}

fn changes(faces: &Faces, s: &Session) -> Div {
    let files = s.changed();
    let mut d = section(faces, "Changes");
    if files.is_empty() {
        return d.child(
            ui::text(faces, "No file changed yet.")
                .text_size(px(SMALL))
                .text_color(theme::muted()),
        );
    }
    for (path, added, gone) in files {
        let name = path
            .strip_prefix(&s.folder)
            .unwrap_or(&path)
            .display()
            .to_string();
        d = d.child(
            div()
                .flex()
                .items_center()
                .gap(px(S2))
                .child(ui::icon("file", 13.).flex_none())
                .child(ui::one_line(
                    ui::mono(faces, ui::fit(&name, 30)).text_color(theme::text2()),
                ))
                .child(
                    ui::mono(faces, format!("+{added}"))
                        .flex_none()
                        .text_color(theme::code_argument()),
                )
                .child(
                    ui::mono(faces, format!("−{gone}"))
                        .flex_none()
                        .text_color(theme::coral()),
                ),
        );
    }
    d
}

fn plan(faces: &Faces, s: &Session) -> Option<Div> {
    let plan = s.plan.as_ref().filter(|p| !p.entries.is_empty())?;
    let steps: Vec<(Status, String)> = plan
        .entries
        .iter()
        .map(|e| {
            let status = match e.status {
                PlanEntryStatus::Completed => Status::Done,
                PlanEntryStatus::InProgress => Status::Doing,
                _ => Status::Todo,
            };
            (status, e.content.clone())
        })
        .collect();
    Some(section(faces, "Plan").child(drawn::plan(faces, &steps)))
}

fn context(faces: &Faces, s: &Session) -> Option<Div> {
    let usage = s.usage.as_ref().filter(|u| u.size > 0)?;
    let share = (usage.used as f32 / usage.size as f32).clamp(0., 1.);
    let cost = usage
        .cost
        .as_ref()
        .map(|c| format!(" · {:.2} {}", c.amount, c.currency));
    Some(
        section(faces, "Context")
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(theme::panel2())
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(share))
                            .rounded(px(2.))
                            .bg(theme::muted()),
                    ),
            )
            .child(
                ui::text(
                    faces,
                    format!(
                        "{}k of {}k tokens{}",
                        usage.used / 1000,
                        usage.size / 1000,
                        cost.unwrap_or_default()
                    ),
                )
                .mt(px(S1))
                .text_size(px(META))
                .text_color(theme::muted()),
            ),
    )
}
