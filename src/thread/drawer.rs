//! The drawer under the thread (⌘J): your terminals beside the session, a
//! tab each, one shown or two side by side; the repository's own scripts,
//! each a click from running in a terminal of its own; a dev server's
//! address to open; and what a terminal shows, handed to the agent.

use gpui::{
    Context, Div, FontWeight, InteractiveElement as _, KeyDownEvent, ScrollWheelEvent,
    SharedString, StyledText, TextRun, Window, div, prelude::*, px,
};

use crate::{
    desk::{Desk, runs},
    fonts::Faces,
    terminal::Terminal,
    theme,
    ui::{self, Weight},
};

/// How tall the drawer stands, its tab bar, and a terminal's line.
pub const HEIGHT: f32 = 300.;
const BAR: f32 = 36.;
const LINE: f32 = 18.;
/// The size terminal text is set in, and the room around it.
const SIZE: f32 = 12.5;
const PAD: f32 = 12.;

/// How wide a terminal's cell is in the desk's mono face.
pub fn cell_width(window: &Window, faces: &Faces) -> f32 {
    let font = gpui::font(faces.mono.clone());
    let id = window.text_system().resolve_font(&font);
    window
        .text_system()
        .advance(id, px(SIZE), 'm')
        .map_or(7.5, |s| f32::from(s.width))
}

/// The rows and columns a terminal pane `width` wide holds.
pub fn fits(width: f32, cell: f32) -> (u16, u16) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a count of cells, clamped"
    )]
    let count = |room: f32, each: f32| (room / each).floor().clamp(2., 500.) as u16;
    (
        count(HEIGHT - BAR - 2. * 8., LINE),
        count(width - 2. * PAD, cell),
    )
}

impl Desk {
    /// The drawer, `width` wide; `cell` is a terminal cell's width.
    pub(crate) fn drawer(&self, faces: &Faces, cell: f32, cx: &mut Context<Self>) -> Option<Div> {
        if !self.drawer {
            return None;
        }
        let terms = self.session_terminals();
        let on = self
            .term_on
            .filter(|id| terms.iter().any(|t| t.id == *id))
            .or_else(|| terms.last().map(|t| t.id));
        let folder = self.session().map(|s| s.folder.clone());
        let scripts = folder
            .as_deref()
            .map(runs::config)
            .unwrap_or_default()
            .scripts;

        let mut bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(BAR))
            .pl(px(10.))
            .pr(px(8.))
            .border_b_1()
            .border_color(theme::border());
        for t in &terms {
            bar = bar.child(Self::tab(faces, t, Some(t.id) == on, cx));
        }
        bar = bar
            .child(
                ui::icon_button("new-terminal", "plus", "New terminal")
                    .size(px(24.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_terminal(None, window, cx);
                    })),
            )
            .child(div().flex_1());
        if !scripts.is_empty() {
            bar = bar.child(
                ui::text(faces, "Scripts")
                    .text_size(px(11.5))
                    .text_color(theme::faint())
                    .mr(px(2.)),
            );
        }
        for (n, script) in scripts.into_iter().enumerate() {
            let command = script.command.clone();
            bar = bar.child(
                ui::button(
                    faces,
                    SharedString::from(format!("script-{n}")),
                    script.name.clone(),
                    Weight::Quiet,
                )
                .tooltip(ui::tip(script.command))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_terminal(Some(&command), window, cx);
                })),
            );
        }
        let shown = on.and_then(|id| terms.iter().find(|t| t.id == id).copied());
        if let Some(url) = shown.and_then(|t| runs::address(&t.text())) {
            bar = bar.child(
                ui::button(faces, "open-preview", "Open preview", Weight::Quiet)
                    .tooltip(ui::tip(url.clone()))
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        if let (Some(t), Some(s)) = (shown, self.session()) {
            let id = t.id;
            bar = bar.child(
                ui::button(
                    faces,
                    "ask-terminal",
                    format!("Ask {}", self.name_of(&s.agent)),
                    Weight::Quiet,
                )
                .tooltip(ui::tip("What the terminal shows, put in your message"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ask_about_terminal(id, window, cx);
                })),
            );
        }
        bar = bar
            .child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme::tag_line()))
            .child(
                ui::icon_button(
                    "split-terminals",
                    "panel-right",
                    if self.split {
                        "One terminal"
                    } else {
                        "Two side by side"
                    },
                )
                .size(px(26.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.split = !this.split;
                    cx.notify();
                })),
            )
            .child(
                ui::icon_button("hide-drawer", "chevron-down", "Hide · ⌘J")
                    .size(px(26.))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_drawer(window, cx))),
            );

        // The one keys go to, and with two side by side the one before it.
        let mut panes: Vec<&Terminal> = shown.into_iter().collect();
        if self.split
            && let Some(at) = on.and_then(|id| terms.iter().position(|t| t.id == id))
            && let Some(other) = terms.get(if at > 0 { at - 1 } else { at + 1 })
        {
            panes.insert(0, *other);
        }
        let mut body = div()
            .id("terminals")
            .track_focus(&self.term_focus)
            .flex()
            .flex_1()
            .min_h(px(0.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.terminal_key(e, cx)));
        for (n, t) in panes.iter().enumerate() {
            if n > 0 {
                body = body.child(div().w(px(1.)).h_full().bg(theme::border()));
            }
            let focused = Some(t.id) == on;
            body = body.child(Self::pane(faces, t, cell, focused, cx));
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .h(px(HEIGHT))
                .border_t_1()
                .border_color(theme::border())
                .bg(theme::well())
                .child(bar)
                .child(body),
        )
    }

    fn tab(faces: &Faces, t: &Terminal, on: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let id = t.id;
        let group = SharedString::from(format!("term-tab-{id}"));
        div()
            .id(group.clone())
            .group(group.clone())
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .pl(px(10.))
            .pr(px(4.))
            .rounded(px(7.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::hover()))
            .when(!on, |d| d.hover(|st| st.bg(theme::hover())))
            .child(
                ui::icon("terminal", 12.)
                    .flex_none()
                    .text_color(if t.ended.is_some() {
                        theme::quiet()
                    } else {
                        theme::muted()
                    }),
            )
            .child(
                ui::text(faces, ui::fit(&t.title, 22))
                    .text_size(px(12.5))
                    .text_color(if on { theme::text() } else { theme::muted() }),
            )
            .children(t.ended.clone().map(|how| {
                ui::text(faces, how)
                    .text_size(px(11.))
                    .text_color(theme::faint())
            }))
            .child(
                div()
                    .invisible()
                    .group_hover(group, gpui::Styled::visible)
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("close-term-{id}")),
                            "x",
                            "Close",
                        )
                        .size(px(18.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_terminal(id, cx);
                            cx.stop_propagation();
                        })),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.term_on = Some(id);
                window.focus(&this.term_focus);
                cx.notify();
            }))
    }

    /// A terminal's screen, its cursor where keys go.
    fn pane(
        faces: &Faces,
        t: &Terminal,
        cell: f32,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (rows, cursor) = t.rows();
        let font = |bold: bool| gpui::Font {
            family: faces.mono.clone(),
            features: gpui::FontFeatures::default(),
            fallbacks: Some(crate::fonts::fallbacks(FontWeight::NORMAL)),
            weight: if bold {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            },
            style: gpui::FontStyle::Normal,
        };
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        for (n, row) in rows.iter().enumerate() {
            if n > 0 {
                text.push('\n');
                runs.push(TextRun {
                    len: 1,
                    font: font(false),
                    color: theme::text2(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            for s in row {
                text.push_str(&s.text);
                runs.push(TextRun {
                    len: s.text.len(),
                    font: font(s.bold),
                    color: s.color,
                    background_color: s.background,
                    underline: None,
                    strikethrough: None,
                });
            }
        }
        runs.retain(|r| r.len > 0);
        let id = t.id;
        div()
            .id(SharedString::from(format!("term-{id}")))
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .px(px(PAD))
            .py(px(8.))
            .cursor_text()
            .child(
                div()
                    .font_family(faces.mono.clone())
                    .text_size(px(SIZE))
                    .line_height(px(LINE))
                    .whitespace_nowrap()
                    .child(StyledText::new(SharedString::from(text)).with_runs(runs)),
            )
            .children(cursor.filter(|_| focused).map(|(row, col)| {
                div()
                    .absolute()
                    .left(px(PAD + f32::from(col) * cell))
                    .top(px(8. + f32::from(row) * LINE))
                    .w(px(cell))
                    .h(px(LINE))
                    .bg(theme::text2())
                    .opacity(0.7)
            }))
            .on_scroll_wheel(cx.listener(move |this, e: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(e.delta.pixel_delta(px(LINE)).y);
                #[allow(clippy::cast_possible_truncation, reason = "lines scrolled")]
                let lines = (dy / LINE).round() as isize;
                if lines != 0
                    && let Some(t) = this.terminals.iter_mut().find(|t| t.id == id)
                {
                    t.scroll(lines);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.term_on = Some(id);
                window.focus(&this.term_focus);
                cx.notify();
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_holds_the_cells_its_room_allows() {
        assert_eq!(fits(800., 7.5), (13, 103));
        assert_eq!(fits(10., 7.5), (13, 2));
    }
}
