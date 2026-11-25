//! The drawer under the thread (⌘J): your terminals beside the session, a
//! tab each, one shown or two side by side; the repository's own scripts,
//! each a click from running in a terminal of its own; a dev server's
//! address to open; and what a terminal shows, handed to the agent.

use gpui::{
    Context, Div, FontWeight, InteractiveElement as _, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ScrollWheelEvent, SharedString, StyledText, TextRun, Window, div, prelude::*,
    px,
};

use crate::{
    desk::{Desk, runs},
    fonts::Faces,
    terminal::Terminal,
    theme,
    ui::{self, Weight},
};

/// How tall the drawer stands, its tab bar, and a terminal's line.
pub const HEIGHT: f32 = 340.;
/// How many terminals stand side by side at most.
pub const SIDE_BY_SIDE: usize = 4;
const BAR: f32 = 38.;
const LINE: f32 = 19.;
/// The size terminal text is set in, and the room around it: beside and
/// above.
const SIZE: f32 = 12.5;
const PAD: f32 = 14.;
const TOP: f32 = 10.;

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
        count(HEIGHT - BAR - 2. * TOP, LINE),
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
        let scripts_none = scripts.is_empty();
        if !scripts_none {
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
                ui::led_button(
                    faces,
                    SharedString::from(format!("script-{n}")),
                    Some(
                        ui::icon("play", 10.)
                            .text_color(theme::muted())
                            .into_any_element(),
                    ),
                    script.name.clone(),
                    Weight::Secondary,
                )
                .h(px(24.))
                .px(px(8.))
                .tooltip(ui::tip(format!(
                    "{}  ·  right-click to change it",
                    script.command
                )))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_terminal(Some(&command), window, cx);
                }))
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, _, window, cx| this.write_script(Some(n), window, cx)),
                ),
            );
        }
        if self.session().is_some_and(|s| s.repo) {
            bar = bar.child(
                ui::icon_button("add-script", "file-plus", "Add a script to ghost.json")
                    .size(px(24.))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.write_script(None, window, cx)),
                    ),
            );
        }
        let shown = on.and_then(|id| terms.iter().find(|t| t.id == id).copied());
        let preview = shown.and_then(|t| runs::address(&t.text()));
        // A rule between what the repository gives and the drawer's own.
        let ruled = !scripts_none || preview.is_some();
        if let Some(url) = preview {
            bar = bar.child(
                ui::button(faces, "open-preview", "Open preview", Weight::Quiet)
                    .tooltip(ui::tip(url.clone()))
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        bar = bar
            .when(ruled, |d| {
                d.child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme::tag_line()))
            })
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

        // The one keys go to; side by side, all of them, four at most.
        let panes: Vec<&Terminal> = if self.split {
            terms.iter().copied().take(SIDE_BY_SIDE).collect()
        } else {
            shown.into_iter().collect()
        };
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
            body = body.child(self.pane(faces, t, cell, focused, cx));
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
                .children(self.script_form(faces, cx))
                .child(body),
        )
    }

    /// The script being written: its name and command, where it runs, and
    /// Save, Delete (a script already kept) and Cancel.
    fn script_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.writing?;
        let field = |input: gpui::Entity<crate::input::TextInput>, width: f32| {
            div()
                .w(px(width))
                .h(px(28.))
                .flex()
                .items_center()
                .px(px(6.))
                .rounded(px(7.))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border2())
                .font_family(faces.mono.clone())
                .text_size(px(12.5))
                .child(input)
        };
        let flag =
            |id: &'static str, label: &'static str, on: bool, set: fn(&mut runs::Writing)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::switch(id, on).on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(w) = &mut this.writing {
                                set(w);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::text(faces, label)
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                    )
            };
        let mut form = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(field(self.script_name.clone(), 120.))
            .child(field(self.script_command.clone(), 280.))
            .child(flag(
                "script-worktree",
                "In each new worktree",
                w.worktree,
                |w| {
                    w.worktree = !w.worktree;
                },
            ))
            .when(w.worktree, |d| {
                d.child(flag(
                    "script-wait",
                    "Before the first message",
                    w.wait,
                    |w| {
                        w.wait = !w.wait;
                    },
                ))
            })
            .child(div().flex_1())
            .child(
                ui::button(faces, "script-save", "Save", Weight::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(true, cx))),
            );
        if w.at.is_some() {
            form = form.child(
                ui::button(faces, "script-delete", "Delete", Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(false, cx))),
            );
        }
        Some(form.child(
            ui::button(faces, "script-cancel", "Cancel", Weight::Quiet).on_click(cx.listener(
                |this, _, _, cx| {
                    this.writing = None;
                    cx.notify();
                },
            )),
        ))
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

    /// A terminal's screen, its cursor where keys go; the cells chosen
    /// with the mouse, and over them what can be done with them.
    fn pane(
        &self,
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
        let (_, cols) = t.size();
        let at = |(row, col): (u16, u16)| (f32::from(col) * cell, f32::from(row) * LINE);

        // The screen's cells, with where its first one is kept for the
        // mouse to be read against.
        let origin = t.origin.clone();
        let mut screen = div()
            .relative()
            .font_family(faces.mono.clone())
            .text_size(px(SIZE))
            .line_height(px(LINE))
            .whitespace_nowrap()
            .child(
                gpui::canvas(
                    move |bounds, _, _| origin.set(bounds.origin),
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if let Some(((r0, c0), (r1, c1))) = t.chosen() {
            let mut tint = theme::accent();
            tint.a = 0.25;
            for r in r0..=r1 {
                let from = if r == r0 { c0 } else { 0 };
                let to = if r == r1 { c1 + 1 } else { cols };
                let (x, y) = at((r, from));
                screen = screen.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(f32::from(to.saturating_sub(from)) * cell))
                        .h(px(LINE))
                        .bg(tint),
                );
            }
        }
        screen = screen.child(StyledText::new(SharedString::from(text)).with_runs(runs));
        if let Some(c) = cursor.filter(|_| focused) {
            let (x, y) = at(c);
            screen = screen.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(cell))
                    .h(px(LINE))
                    .bg(theme::text2())
                    .opacity(0.7),
            );
        }
        let pop = t.chosen().map(|((r0, c0), (r1, _))| {
            self.chosen_pop(faces, id, at, (r0, c0, r1), cols, cell, cx)
        });

        div()
            .id(SharedString::from(format!("term-{id}")))
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .px(px(PAD))
            .py(px(TOP))
            .cursor_text()
            .child(screen)
            .children(pop)
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.term_on = Some(id);
                    window.focus(&this.term_focus);
                    if let Some(t) = this.terminals.iter_mut().find(|t| t.id == id) {
                        let at = Self::cell_at(t, e.position, cell);
                        // ⌘ and a click opens an address; two clicks choose
                        // a word, three its line.
                        if e.modifiers.platform
                            && let Some(url) = t.link_at(at)
                        {
                            cx.open_url(&url);
                        } else if e.modifiers.platform
                            && crate::markdown::path_like(&t.word_text(at))
                        {
                            // ⌘ and a click on a file's path opens it.
                            let named = t.word_text(at);
                            this.open_named(&named, cx);
                        } else {
                            let last = t.size().1.saturating_sub(1);
                            t.selection = Some(match e.click_count {
                                2 => t.word_at(at),
                                n if n >= 3 => ((at.0, 0), (at.0, last)),
                                _ => (at, at),
                            });
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                if let Some(t) = this.terminals.iter_mut().find(|t| t.id == id)
                    && let Some((from, _)) = t.selection
                {
                    t.selection = Some((from, Self::cell_at(t, e.position, cell)));
                    cx.notify();
                }
            }))
    }

    /// The cell of `t` under the mouse at `position` on the window.
    fn cell_at(t: &Terminal, position: gpui::Point<gpui::Pixels>, cell: f32) -> (u16, u16) {
        let (rows, cols) = t.size();
        let from = position - t.origin.get();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a cell's place, clamped to the screen"
        )]
        let count = |by: f32, each: f32, most: u16| {
            (by / each).floor().clamp(0., f32::from(most - 1)) as u16
        };
        (
            count(f32::from(from.y), LINE, rows),
            count(f32::from(from.x), cell, cols),
        )
    }

    /// Over the cells chosen, under the last of them (over the first when
    /// there is no room below): what can be done with them.
    #[allow(
        clippy::too_many_arguments,
        reason = "where the cells are, to stand by them"
    )]
    fn chosen_pop(
        &self,
        faces: &Faces,
        id: u64,
        at: impl Fn((u16, u16)) -> (f32, f32),
        (r0, c0, r1): (u16, u16, u16),
        cols: u16,
        cell: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const WIDE: f32 = 290.;
        const HIGH: f32 = 34.;
        let room = HEIGHT - BAR - 2. * TOP;
        let (x, below) = at((r1 + 1, c0));
        let y = if below + 4. + HIGH <= room {
            below + 4.
        } else {
            (at((r0, c0)).1 - HIGH - 4.).max(0.)
        };
        let x = x.min(f32::from(cols) * cell - WIDE).max(0.);
        let name = self
            .session()
            .map(|s| (s.agent.clone(), self.name_of(&s.agent)));
        div().absolute().left(px(PAD + x)).top(px(TOP + y)).child(
            ui::menu("term-chosen")
                .gap(px(2.))
                .p(px(4.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(name.map(|(agent, name)| {
                    ui::led_button(
                        faces,
                        "ask-chosen",
                        Some(ui::mark(faces, &ui::Mark::of(&agent, &name), 12.)),
                        format!("Ask {name} about this"),
                        Weight::Quiet,
                    )
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.ask_about_terminal(id, window, cx);
                    }))
                }))
                .child(
                    ui::button(faces, "copy-chosen", "Copy", Weight::Quiet)
                        .h(px(24.))
                        .px(px(8.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy_chosen(id, cx);
                        })),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_holds_the_cells_its_room_allows() {
        assert_eq!(fits(800., 7.5), (14, 102));
        assert_eq!(fits(10., 7.5), (14, 2));
    }
}
