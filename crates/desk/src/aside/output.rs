//! Full tool-call output in the right column (`Surface::Output`), drawn as a
//! virtualized line list so multi-megabyte outputs scroll without stutter and
//! `⌘F` searches in them (`LongAndMany`).

use agent_client_protocol::v1::schema::{ToolCallContent, ToolCallId};
use gpui::component::{Sizable as _, input::Input};
use gpui::{ClipboardItem, Context, Div, SharedString, div, prelude::*, px, uniform_list};
use u0ui::prelude::*;

use crate::{
    desk::Desk,
    fonts::Faces,
    session::{Session, text_of},
    theme,
    thread::output::{self, Line},
    ui,
};

impl Desk {
    /// Full text of the tool call `id` in session `s`, unfenced as the thread
    /// prints it.
    pub(crate) fn tool_output_text(s: &Session, id: &ToolCallId) -> String {
        let Some(tool) = s.tools().find(|t| &t.call.tool_call_id == id) else {
            return String::new();
        };
        tool.call
            .content
            .iter()
            .filter_map(|c| match c {
                ToolCallContent::Content(x) => {
                    let t = text_of(&x.content);
                    (!t.trim().is_empty()).then(|| output::unfenced(&t).to_owned())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Tab label for `Surface::Output(id)`.
    pub(crate) fn output_tab_name(s: &Session, id: &ToolCallId) -> String {
        s.tools().find(|t| &t.call.tool_call_id == id).map_or_else(
            || crate::tr("Output").to_string(),
            |t| {
                let cmd = t.command_line();
                let short = cmd.split_whitespace().next().unwrap_or(&t.call.title);
                ui::fit(short, 24)
            },
        )
    }

    /// The right-column viewer for `Surface::Output(id)`: command header with
    /// size and Copy, optional `⌘F` search bar, and virtualized mono lines.
    pub(super) fn output_tab(
        &self,
        faces: &Faces,
        s: &Session,
        id: &ToolCallId,
        cx: &mut Context<Self>,
    ) -> Div {
        let full = Self::tool_output_text(s, id);
        let parsed: std::rc::Rc<[Line]> = output::lines(&full).into();
        let total = parsed.len();
        let bytes = crate::tidy::shown(full.len() as u64);
        let tool = s.tools().find(|t| &t.call.tool_call_id == id);
        let title = tool.map_or_else(|| id.0.to_string(), crate::session::Tool::command_line);
        // `LongAndMany`: how long it took, where that is known, and its size.
        let said = tool
            .and_then(crate::session::Tool::took_said)
            .map(crate::when::took)
            .map_or_else(|| bytes.clone(), |took| format!("{took} \u{b7} {bytes}"));

        let query = if self.find.open {
            self.find.input.read(cx).value().trim().to_lowercase()
        } else {
            String::new()
        };
        let hits: Vec<usize> = if query.is_empty() {
            Vec::new()
        } else {
            parsed
                .iter()
                .enumerate()
                .filter(|(_, l)| l.text.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect()
        };
        let active_line = (!hits.is_empty()).then(|| hits[self.find.at.min(hits.len() - 1)]);

        let copy_text = full;
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .pl(px(12.))
            .pr(px(8.))
            .bg(color(ColorRole::Layer01))
            .border_b_1()
            .border_color(theme::border())
            .child(Icon::carbon(CarbonIcon::Terminal).size(IconSize::S16))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .type_style(TypeStyle::Code01)
                    .text_color(theme::text())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::muted())
                    .child(said),
            )
            .child(
                Button::new("copy-full-output", crate::tr("Copy"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Copy)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                    }),
            );

        let search = self.find.open.then(|| {
            let count = if query.is_empty() {
                SharedString::default()
            } else if hits.is_empty() {
                crate::tr("No match")
            } else {
                crate::tr_with(
                    "%{n} of %{all}",
                    &[
                        ("n", &(self.find.at.min(hits.len() - 1) + 1)),
                        ("all", &hits.len()),
                    ],
                )
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(6.))
                .bg(color(ColorRole::Layer01))
                .border_b_1()
                .border_color(theme::border())
                .child(div().flex_1().child(Input::new(&self.find.input).xsmall()))
                .when(!count.is_empty(), |d| {
                    d.child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::muted())
                            .child(count),
                    )
                })
                .child(
                    Button::new("output-find-prev", crate::tr("Previous"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::ChevronUp)
                        .icon_only(true)
                        .on_click(cx.listener(|this, _, _, cx| this.find_next(true, cx))),
                )
                .child(
                    Button::new("output-find-next", crate::tr("Next"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::ChevronDown)
                        .icon_only(true)
                        .on_click(cx.listener(|this, _, _, cx| this.find_next(false, cx))),
                )
                .child(
                    Button::new("output-find-close", crate::tr("Close"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Close)
                        .icon_only(true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_find(window, cx);
                        })),
                )
        });

        let faces_list = faces.clone();
        let query_list = query;
        let list = uniform_list("full-tool-output", total, move |range, _, _| {
            range
                .map(|ix| {
                    let line = &parsed[ix];
                    let matched =
                        !query_list.is_empty() && line.text.to_lowercase().contains(&query_list);
                    let current = active_line == Some(ix);
                    // `.lmb-out pre`: mono 12/18 in `text-primary`, 12px at each side.
                    div()
                        .h(px(crate::style::MONO_LINE))
                        .px(px(12.))
                        .when(current, |d| d.bg(color(ColorRole::LayerSelected02)))
                        .when(matched && !current, |d| {
                            d.bg(color(ColorRole::LayerSelected01))
                        })
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(faces_list.mono.clone())
                        .text_size(px(crate::style::MONO))
                        .line_height(px(crate::style::MONO_LINE))
                        .text_color(theme::text())
                        .child(output::styled_one(&faces_list, line))
                })
                .collect()
        })
        .track_scroll(&self.output_scroll)
        .py(px(8.))
        .flex_1()
        .min_h(px(0.));

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .bg(color(ColorRole::Layer01))
            .child(head)
            .children(search)
            .child(list)
    }
}
