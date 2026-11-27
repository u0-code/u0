//! The drawer under the thread (`⌘J`, `TerminalBrowser` board): your terminals
//! beside the session drawn with `u0ui::t3::workspace::TerminalDrawer`, a tab
//! each, one shown or up to four side by side; the repository's own scripts,
//! each a click from running in a terminal of its own; a dev server's address
//! to open; and what a terminal shows, handed to the agent.

use std::rc::Rc;

use gpui::component::{
    Sizable as _,
    input::{Input, InputState},
    menu::{ContextMenuExt as _, PopupMenuItem},
    separator::Separator,
    switch::Switch,
};
use gpui::{
    Context, Div, Entity, FontWeight, InteractiveElement as _, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ScrollEvent, SharedString, StyledText, TextRun, Window, div,
    prelude::*, px,
};
use u0ui::{
    components::{Button, ButtonSize, ButtonVariant},
    icons::CarbonIcon,
    t3::workspace::{TerminalDrawer, TerminalTab, TerminalTexts},
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use crate::{
    aside::Surface,
    desk::{
        Desk,
        runs::{self, Layout},
    },
    fonts::Faces,
    terminal::Terminal,
    theme,
    ui::{self, Weight},
};

/// How tall the drawer stands (`TerminalBrowser` board: compact bottom drawer).
pub const HEIGHT: f32 = 240.;
/// How many terminals stand side by side at most.
pub const SIDE_BY_SIDE: usize = 4;
/// Height of `TerminalDrawer`'s tab bar (`rem(32.)`).
const BAR: f32 = 32.;
/// The room around a terminal's text: beside and above (`.t3-term-view { padding: 12px 24px }`).
const PAD: f32 = 24.;
const TOP: f32 = 12.;

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
                .border_color(theme::role(ColorRole::BorderSubtle01))
        })
    }

    /// Your terminals beside the session (`u0ui::t3::workspace::TerminalDrawer`):
    /// tabs, repository scripts, and the interactive PTY viewport.
    #[allow(
        clippy::too_many_lines,
        reason = "wires TerminalDrawer tabs, script controls, actions, and PTY panes together"
    )]
    pub(crate) fn terminal_view(&self, faces: &Faces, grid: Grid, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
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

        let ids: Vec<u64> = terms.iter().map(|t| t.id).collect();
        let chosen = on.and_then(|id| ids.iter().position(|i| *i == id));
        let tabs: Vec<TerminalTab> = terms
            .iter()
            .map(|t| {
                let raw = t.script.as_deref().unwrap_or(&t.title);
                let title = if raw.is_empty() {
                    "zsh".to_owned()
                } else {
                    ui::fit(raw, 20)
                };
                TerminalTab::new(title).running(t.ended.is_none())
            })
            .collect();

        let shown = on.and_then(|id| terms.iter().find(|t| t.id == id).copied());
        let preview = shown.and_then(|t| {
            scripts
                .iter()
                .find(|x| t.script.as_ref() == Some(&x.name))
                .and_then(|x| x.preview.clone())
                .or_else(|| runs::address(&t.text()))
        });

        let mut extras = div().flex().items_center().gap(px(4.)).pr(px(4.));
        for (n, script) in scripts.iter().enumerate() {
            let (name, d_run, d_edit) = (script.name.clone(), desk.clone(), desk.clone());
            extras = extras.child(
                div()
                    .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                        d_edit.update(cx, |this, cx| this.write_script(Some(n), window, cx));
                    })
                    .child(
                        Button::new(
                            SharedString::from(format!("script-{n}")),
                            script.name.clone(),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Play)
                        .on_click(move |_, window, cx| {
                            let s_name = name.clone();
                            d_run.update(cx, |this, cx| this.run_script(&s_name, window, cx));
                        }),
                    ),
            );
        }
        if self.session().is_some_and(|s| s.repo) {
            let d_add = desk.clone();
            extras = extras.child(
                Button::new("add-script", crate::tr("Add a script to u0.json"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::DocumentAdd)
                    .icon_only(true)
                    .on_click(move |_, window, cx| {
                        d_add.update(cx, |this, cx| this.write_script(None, window, cx));
                    }),
            );
        }
        if let Some((url, host)) = preview.zip(shown.map(|t| t.host)) {
            let d_prev = desk.clone();
            extras = extras.child(
                Button::new("open-preview", crate::tr("Open preview"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Launch)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        let target = url.clone();
                        d_prev.update(cx, |this, cx| this.open_address(host, target, cx));
                    }),
            );
        }

        let stacked = self.drawer.layout == Layout::Stacked;
        let panes: Vec<&Terminal> = if self.drawer.layout == Layout::One {
            shown.into_iter().collect()
        } else {
            terms.iter().copied().take(SIDE_BY_SIDE).collect()
        };
        let mut pty_row = div()
            .id("terminals")
            .key_context(crate::desk::runs::CONTEXT)
            .track_focus(&self.drawer.focus)
            .flex()
            .when(stacked, gpui::Styled::flex_col)
            .flex_1()
            .min_h(px(0.))
            .bg(theme::role(ColorRole::TerminalBackground))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.terminal_key(e, cx)));
        for (n, t) in panes.iter().enumerate() {
            if n > 0 {
                pty_row = pty_row.child(if stacked {
                    Separator::horizontal().color(theme::role(ColorRole::BorderSubtle01))
                } else {
                    Separator::vertical().color(theme::role(ColorRole::BorderSubtle01))
                });
            }
            let focused = Some(t.id) == on;
            pty_row = pty_row.child(self.pane(faces, t, grid, focused, cx));
        }

        let viewport = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .children(self.script_form(faces, cx))
            .child(pty_row);

        let texts = TerminalTexts {
            close: Rc::new(|t| crate::tr_with("Close %{title}", &[("title", &t)])),
            new_terminal: crate::tr("New terminal"),
            split: crate::tr("Split right"),
            add_to_chat: crate::tr("Add selection to chat"),
            maximize: crate::tr("Maximize"),
            hide: crate::keys::hint(cx, &crate::tr("Hide terminal"), "drawer").into(),
            exited: Rc::new(|n| crate::tr_with("Process exited with code %{n}", &[("n", &n)])),
        };

        let (d_sel, d_close, d_new, d_split, d_chat, d_max, d_hide) = (
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk,
        );
        let close_ids = ids.clone();
        let active_id = on;
        let beside = self.terminals_beside();

        let drawer = TerminalDrawer::new("terminal-drawer", tabs)
            .controlled(chosen.unwrap_or(0))
            .texts(texts)
            .bar_extra(extras)
            .viewport(viewport)
            .on_select(move |ix, window, cx| {
                let picked = ids.get(ix).copied();
                d_sel.update(cx, |this, cx| {
                    this.drawer.on = picked;
                    window.focus(&this.drawer.focus, cx);
                    cx.notify();
                });
            })
            .on_close(move |ix, _, cx| {
                if let Some(&id) = close_ids.get(ix) {
                    d_close.update(cx, |this, cx| this.close_terminal(id, cx));
                }
            })
            .on_new(move |window, cx| {
                d_new.update(cx, |this, cx| this.open_terminal(None, window, cx));
            })
            .on_split(move |window, cx| {
                d_split.update(cx, |this, cx| {
                    let was_side = this.drawer.layout == Layout::Side;
                    this.drawer.layout = if was_side { Layout::One } else { Layout::Side };
                    if !was_side && this.session_terminals().len() < 2 {
                        this.open_terminal(None, window, cx);
                    }
                    cx.notify();
                });
            })
            .on_add_to_chat(move |window, cx| {
                if let Some(id) = active_id {
                    d_chat.update(cx, |this, cx| this.ask_about_terminal(id, window, cx));
                }
            })
            .on_maximize(move |_, cx| {
                d_max.update(cx, |this, cx| {
                    if beside {
                        this.toggle_whole(cx);
                    } else {
                        this.open_surface(Surface::Terminal, cx);
                    }
                });
            })
            .on_hide(move |window, cx| {
                d_hide.update(cx, |this, cx| {
                    if beside {
                        this.close_shown(cx);
                    } else {
                        this.toggle_drawer(window, cx);
                    }
                });
            });

        div().flex().flex_col().flex_1().min_h(px(0.)).child(drawer)
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
                        ui::text(faces, crate::tr(label))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
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
            .bg(theme::role(ColorRole::Layer01))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(field(&self.drawer.name, 120.))
            .child(field(&self.drawer.command, 280.))
            .child(field(&self.drawer.preview, 220.))
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
            .child(flag(
                "script-settle",
                "As its thread settles",
                w.settle,
                |w| {
                    w.settle = !w.settle;
                },
            ))
            .child(flag(
                "script-stop",
                "Stop it as its thread settles",
                w.stop,
                |w| {
                    w.stop = !w.stop;
                },
            ))
            .child(div().flex_1())
            .child(
                ui::button("script-save", crate::tr("Save"), Weight::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(true, cx))),
            );
        if w.at.is_some() {
            form = form.child(
                ui::button("script-delete", crate::tr("Delete"), Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| this.save_script(false, cx))),
            );
        }
        Some(form.child(
            ui::button("script-cancel", crate::tr("Cancel"), Weight::Quiet).on_click(cx.listener(
                |this, _, _, cx| {
                    this.drawer.writing = None;
                    cx.notify();
                },
            )),
        ))
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
        let term_ink = theme::role(ColorRole::TerminalText);
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
                    color: term_ink,
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
            let mut tint = theme::role(ColorRole::Interactive);
            tint.a = 0.28;
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
                    .bg(term_ink)
                    .opacity(0.75),
            );
        }
        let (desk, size) = (cx.entity().downgrade(), t.size());
        let owner = cx.entity();
        let agent = self.session().map(|s| s.meta.agent.clone());
        let name = agent.as_deref().map(|a| self.name_of(a));
        let pop = t
            .chosen()
            .map(|chosen| self.chosen_pop(faces, id, at, chosen, cols, cell, cx));

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
                        if e.modifiers.platform
                            && let Some(url) = t.link_at(at)
                        {
                            let host = t.host;
                            this.open_address(host, url, cx);
                        } else if e.modifiers.platform
                            && crate::markdown::path_like(&t.word_text(at))
                        {
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
                let item =
                    |label: SharedString,
                     act: fn(&mut Desk, u64, &mut gpui::Window, &mut Context<Desk>)| {
                        let desk = owner.clone();
                        PopupMenuItem::new(label)
                            .on_click(move |_, window, cx| desk.update(cx, |d, cx| act(d, id, window, cx)))
                    };
                let menu = menu
                    .item(item(crate::tr("Copy"), |d, id, _, cx| d.copy_chosen(id, cx)))
                    .item(item(crate::tr("Paste"), |d, id, _, cx| d.paste_terminal(id, cx)))
                    .item(item(crate::tr("Select all"), |d, id, _, cx| {
                        d.with_terminal(id, Terminal::choose_all, cx);
                    }))
                    .item(item(crate::tr("Jump to latest"), |d, id, _, cx| {
                        d.with_terminal(id, Terminal::latest, cx);
                    }))
                    .item(item(crate::tr("Clear"), |d, id, _, cx| {
                        d.drawer.on = Some(id);
                        d.clear_terminal(cx);
                    }));
                match &name {
                    Some(name) => menu.separator().item(item(
                        crate::tr_with("Ask %{name} about it", &[("name", name)]),
                        |d, id, window, cx| {
                            d.ask_about_terminal(id, window, cx);
                        },
                    )),
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
                        crate::tr_with("Ask %{name} about this", &[("name", &name)]),
                        Weight::Quiet,
                    )
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.ask_about_terminal(id, window, cx);
                    }))
                }))
                .child(
                    ui::button("copy-chosen", crate::tr("Copy"), Weight::Quiet)
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
        assert_eq!(fits(800., 302., grid), (14, 100));
        assert_eq!(fits(10., 302., grid), (14, 2));
    }
}
