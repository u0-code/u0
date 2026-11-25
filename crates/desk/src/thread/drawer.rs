//! The drawer under the thread (⌘J): your terminals beside the session, a
//! tab each, one shown or two side by side; the repository's own scripts,
//! each a click from running in a terminal of its own; a dev server's
//! address to open; and what a terminal shows, handed to the agent.

use gpui::component::{
    Sizable as _,
    input::{Input, InputState},
    menu::{ContextMenuExt as _, PopupMenuItem},
    separator::Separator,
    switch::Switch,
    tab::{Tab, TabBar},
};
use gpui::{
    Context, Div, Entity, FontWeight, InteractiveElement as _, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ScrollEvent, SharedString, StyledText, TextRun, Window, div,
    prelude::*, px,
};

use crate::{
    desk::{
        Desk,
        runs::{self, Layout},
    },
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
/// The room around a terminal's text: beside and above.
const PAD: f32 = 14.;
const TOP: f32 = 10.;

/// A terminal's grid: the size its text is set in, a line's height, a
/// cell's width in the desk's mono face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub size: f32,
    pub line: f32,
    pub cell: f32,
}

/// The grid of a terminal's text set as `size` says.
pub fn grid(window: &Window, faces: &Faces, size: crate::settings::TermSize) -> Grid {
    let (size, line) = size.points();
    let font = gpui::font(faces.mono.clone());
    let id = window.text_system().resolve_font(&font);
    let cell = window
        .text_system()
        .advance(id, px(size), 'm')
        .map_or(size * 0.6, |s| f32::from(s.width));
    Grid { size, line, cell }
}

/// The rows and columns a terminal pane `width` by `height` holds, as it
/// was laid out, inside its padding.
fn fits(width: f32, height: f32, Grid { line, cell, .. }: Grid) -> (u16, u16) {
    let count = |room: f32, each: f32| (room / each).floor().clamp(2., 500.) as u16;
    (
        count(height - 2. * TOP, line),
        count(width - 2. * PAD, cell),
    )
}

impl Desk {
    /// The drawer, under the thread when the column does not show your
    /// terminals; `grid` is a terminal's grid.
    pub(crate) fn drawer(&self, faces: &Faces, grid: Grid, cx: &mut Context<Self>) -> Option<Div> {
        (self.drawer.open && !self.terminals_beside()).then(|| {
            self.terminal_view(faces, grid, cx)
                .flex_none()
                .h(px(HEIGHT))
                .border_t_1()
                .border_color(theme::border())
                .bg(theme::well())
        })
    }

    /// Your terminals beside the session: their tabs, the scripts, the
    /// terminal shown (or those side by side), in the drawer or the column.
    pub(crate) fn terminal_view(&self, faces: &Faces, grid: Grid, cx: &mut Context<Self>) -> Div {
        let terms = self.session_terminals();
        let on = self
            .drawer
            .on
            .filter(|id| terms.iter().any(|t| t.id == *id))
            .or_else(|| terms.last().map(|t| t.id));
        let scripts = self
            .session()
            .map(|s| s.scripts.clone())
            .unwrap_or_default();

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
        let ids: Vec<u64> = terms.iter().map(|t| t.id).collect();
        let chosen = on.and_then(|id| ids.iter().position(|i| *i == id));
        bar = bar.child(
            TabBar::new("terminal-tabs")
                .segmented()
                .small()
                .when_some(chosen, TabBar::selected_index)
                .children(terms.iter().map(|t| self.tab(faces, t, cx)))
                .on_click(cx.listener(move |this, ix: &usize, window, cx| {
                    this.drawer.on = ids.get(*ix).copied();
                    window.focus(&this.drawer.focus, cx);
                    cx.notify();
                })),
        );
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
                .tooltip(format!("{}  ·  right-click to change it", script.command))
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
                ui::button("open-preview", "Open preview", Weight::Quiet)
                    .tooltip(url.clone())
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        bar = bar
            .when(ruled, |d| {
                d.child(
                    Separator::vertical()
                        .h(px(18.))
                        .mx(px(4.))
                        .color(theme::tag_line()),
                )
            })
            .child(self.layout_button(
                "split-side",
                "panel-right",
                "Side by side",
                Layout::Side,
                cx,
            ))
            .child(self.layout_button(
                "split-stacked",
                "list",
                "One above another",
                Layout::Stacked,
                cx,
            ))
            .when(!self.terminals_beside(), |d| {
                d.child(
                    ui::icon_button(
                        "hide-drawer",
                        "chevron-down",
                        crate::keys::hint(cx, "Hide", "drawer"),
                    )
                    .size(px(26.))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_drawer(window, cx))),
                )
            });

        // The one keys go to; split, all of them, four at most.
        let stacked = self.drawer.layout == Layout::Stacked;
        let panes: Vec<&Terminal> = if self.drawer.layout == Layout::One {
            shown.into_iter().collect()
        } else {
            terms.iter().copied().take(SIDE_BY_SIDE).collect()
        };
        let mut body = div()
            .id("terminals")
            .track_focus(&self.drawer.focus)
            .flex()
            .when(stacked, gpui::Styled::flex_col)
            .flex_1()
            .min_h(px(0.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.terminal_key(e, cx)));
        for (n, t) in panes.iter().enumerate() {
            if n > 0 {
                body = body.child(if stacked {
                    Separator::horizontal().color(theme::border())
                } else {
                    Separator::vertical().color(theme::border())
                });
            }
            let focused = Some(t.id) == on;
            body = body.child(self.pane(faces, t, grid, focused, cx));
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(bar)
            .children(self.script_form(faces, cx))
            .child(body)
    }

    /// Split the drawer `layout`'s way (a second terminal opened when it
    /// holds one), or, split so already, back to one.
    fn layout_button(
        &self,
        id: &'static str,
        icon: &'static str,
        what: &'static str,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> gpui::component::button::Button {
        let on = self.drawer.layout == layout;
        ui::icon_button(id, icon, if on { "One terminal" } else { what })
            .size(px(26.))
            .when(on, |b| b.text_color(theme::text()))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.drawer.layout = if on { Layout::One } else { layout };
                if !on && this.session_terminals().len() < 2 {
                    this.open_terminal(None, window, cx);
                }
                cx.notify();
            }))
    }

    /// Do `what` to the terminal `id`.
    fn with_terminal(&mut self, id: u64, what: impl FnOnce(&mut Terminal), cx: &mut Context<Self>) {
        if let Some(t) = self.drawer.terminals.iter_mut().find(|t| t.id == id) {
            what(t);
        }
        cx.notify();
    }

    /// The script being written: its name and command, where it runs, and
    /// Save, Delete (a script already kept) and Cancel.
    fn script_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.drawer.writing?;
        let field = |input: &Entity<InputState>, width: f32| {
            div()
                .w(px(width))
                .font_family(faces.mono.clone())
                .child(Input::new(input).small())
        };
        let flag =
            |id: &'static str, label: &'static str, on: bool, set: fn(&mut runs::Writing)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Switch::new(id).small().checked(on).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Some(w) = &mut this.drawer.writing {
                                set(w);
                            }
                            cx.notify();
                        },
                    )))
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
            .child(field(&self.drawer.name, 120.))
            .child(field(&self.drawer.command, 280.))
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
                ui::button("script-save", "Save", Weight::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(true, cx))),
            );
        if w.at.is_some() {
            form = form.child(
                ui::button("script-delete", "Delete", Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(false, cx))),
            );
        }
        Some(form.child(
            ui::button("script-cancel", "Cancel", Weight::Quiet).on_click(cx.listener(
                |this, _, _, cx| {
                    this.drawer.writing = None;
                    cx.notify();
                },
            )),
        ))
    }

    /// A terminal's tab: its title, how it ended, and a button to close it
    /// that shows while the pointer is over the tab.
    fn tab(&self, faces: &Faces, t: &Terminal, cx: &mut Context<Self>) -> Tab {
        let id = t.id;
        // The pointer over it is kept by the desk: a hover group's style
        // never reached the button inside gpui-kit's tab.
        let closable = self.drawer.hovered == Some(id) || self.drawer.on == Some(id);
        Tab::new()
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                let now = if *over {
                    Some(id)
                } else {
                    this.drawer.hovered.filter(|h| *h != id)
                };
                if now != this.drawer.hovered {
                    this.drawer.hovered = now;
                    cx.notify();
                }
            }))
            .label(ui::fit(&t.title, 22))
            .prefix(ui::icon("terminal", 12.).text_color(if t.ended.is_some() {
                theme::quiet()
            } else {
                theme::muted()
            }))
            .suffix(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .children(t.ended.clone().map(|how| {
                        ui::text(faces, how)
                            .text_size(px(11.))
                            .text_color(theme::faint())
                    }))
                    .when(closable, |d| {
                        d.child(
                            ui::icon_button(
                                SharedString::from(format!("close-term-{id}")),
                                "x",
                                "Close",
                            )
                            .size(px(18.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.close_terminal(id, cx);
                                },
                            )),
                        )
                    }),
            )
    }

    /// A terminal's screen, its cursor where keys go; the cells chosen
    /// with the mouse, and over them what can be done with them.
    fn pane(
        &self,
        faces: &Faces,
        t: &Terminal,
        grid: Grid,
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
        let Grid {
            size: points,
            line,
            cell,
        } = grid;
        let at = |(row, col): (u16, u16)| (f32::from(col) * cell, f32::from(row) * line);

        // The screen's cells, with where its first one is kept for the
        // mouse to be read against.
        let origin = t.origin.clone();
        let mut screen = div()
            .relative()
            .font_family(faces.mono.clone())
            .text_size(px(points))
            .line_height(px(line))
            .whitespace_nowrap()
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        *origin
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = bounds.origin;
                    },
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
                        .h(px(line))
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
                    .h(px(line))
                    .bg(theme::text2())
                    .opacity(0.7),
            );
        }
        let (desk, size) = (cx.entity().downgrade(), t.size());
        let owner = cx.entity();
        let agent = self.session().map(|s| s.meta.agent.clone());
        let name = agent.as_deref().map(|a| self.name_of(a));
        let pop = t
            .chosen()
            .map(|chosen| self.chosen_pop(faces, id, at, chosen, cols, cell, cx));

        // Its shell is told the size of the grid as it was laid out, not as
        // the window's width guesses it: a pane narrower than guessed had
        // the prompt's right side wrap onto a line of its own.
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                let fit = fits(
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                    grid,
                );
                if fit != size {
                    cx.defer(move |cx| {
                        let _ = desk.update(cx, |d, cx| d.fit_terminal(id, fit.0, fit.1, cx));
                    });
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
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
            .child(measure)
            .child(screen)
            .children(pop)
            .on_scroll(cx.listener(move |this, e: &ScrollEvent, _, cx| {
                let dy = f32::from(e.delta.pixel_delta(px(line)).y);
                let lines = (dy / line).round() as isize;
                if lines != 0
                    && let Some(t) = this.drawer.terminals.iter_mut().find(|t| t.id == id)
                {
                    t.scroll(lines);
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.drawer.on = Some(id);
                    window.focus(&this.drawer.focus, cx);
                    if let Some(t) = this.drawer.terminals.iter_mut().find(|t| t.id == id) {
                        let at = Self::cell_at(t, e.position, grid);
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
                            this.open_named(&named);
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
                if let Some(t) = this.drawer.terminals.iter_mut().find(|t| t.id == id)
                    && let Some((from, _)) = t.selection
                {
                    t.selection = Some((from, Self::cell_at(t, e.position, grid)));
                    cx.notify();
                }
            }))
            .context_menu(move |menu, _, _| {
                let item = |label: String, act: fn(&mut Desk, u64, &mut gpui::Window, &mut Context<Desk>)| {
                    let desk = owner.clone();
                    PopupMenuItem::new(label)
                        .on_click(move |_, window, cx| desk.update(cx, |d, cx| act(d, id, window, cx)))
                };
                let menu = menu
                    .item(item("Copy".into(), |d, id, _, cx| d.copy_chosen(id, cx)))
                    .item(item("Paste".into(), |d, id, _, cx| d.paste_terminal(id, cx)))
                    .item(item("Select all".into(), |d, id, _, cx| d.with_terminal(id, Terminal::choose_all, cx)))
                    .item(item("Jump to latest".into(), |d, id, _, cx| d.with_terminal(id, Terminal::latest, cx)))
                    .item(item("Clear".into(), |d, id, _, cx| {
                        d.drawer.on = Some(id);
                        d.clear_terminal(cx);
                    }));
                match &name {
                    Some(name) => menu.separator().item(item(format!("Ask {name} about it"), |d, id, window, cx| {
                        d.ask_about_terminal(id, window, cx);
                    })),
                    None => menu,
                }
            })
    }

    /// The cell of `t` under the mouse at `position` on the window.
    fn cell_at(t: &Terminal, position: gpui::Point<gpui::Pixels>, grid: Grid) -> (u16, u16) {
        let (rows, cols) = t.size();
        let from = position
            - *t.origin
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = |by: f32, each: f32, most: u16| {
            (by / each).floor().clamp(0., f32::from(most - 1)) as u16
        };
        (
            count(f32::from(from.y), grid.line, rows),
            count(f32::from(from.x), grid.cell, cols),
        )
    }

    /// Over the cells chosen, under the last of them (over the first when
    /// there is no room below): what can be done with them.
    fn chosen_pop(
        &self,
        faces: &Faces,
        id: u64,
        at: impl Fn((u16, u16)) -> (f32, f32),
        ((r0, c0), (r1, _)): ((u16, u16), (u16, u16)),
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
            .map(|s| (s.meta.agent.clone(), self.name_of(&s.meta.agent)));
        div().absolute().left(px(PAD + x)).top(px(TOP + y)).child(
            ui::menu("term-chosen")
                .gap(px(2.))
                .p(px(4.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(name.map(|(agent, name)| {
                    ui::led_button(
                        "ask-chosen",
                        Some(ui::mark(faces, &self.mark_of(&agent), 12.)),
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
                    ui::button("copy-chosen", "Copy", Weight::Quiet)
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
        let grid = Grid {
            size: 12.5,
            line: 19.,
            cell: 7.5,
        };
        assert_eq!(fits(800., 302., grid), (14, 102));
        assert_eq!(fits(10., 302., grid), (14, 2));
    }
}
