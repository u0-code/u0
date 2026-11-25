//! The column's tabs, as t3code's right panel has them: each session keeps
//! its own row of surfaces (its diff, its details, its files, one file, your
//! terminals, its pull request, a side chat). One is opened from the + after
//! the tabs (⌘T) or, while that menu is open, by its letter; a tab closes
//! from the mark that takes its icon's place under the pointer, a middle
//! click, ⌘W or its right-click menu; the last one closed opens again with
//! ⇧⌘T.

use gpui::component::{
    Sizable as _,
    kbd::Kbd,
    menu::{ContextMenuExt as _, PopupMenuItem},
    tab::{Tab, TabBar},
};
use gpui::{Context, Div, Keystroke, MouseButton, SharedString, div, prelude::*, px};

use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    session::Session,
    ui,
};

/// What a tab of the column shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    /// What the session has come to: its context, its plan, what runs
    /// beside it.
    Details,
    /// Its changes as git reads them, and the commits.
    Diff,
    /// The project's files.
    Files,
    /// One file, read.
    File(std::path::PathBuf),
    /// Your terminals beside it.
    Terminal,
    /// Its branch's pull request.
    PullRequest,
    /// A side chat of it, by key.
    Side(u64),
}

/// A session's tabs in order, the one shown, and those closed, the last
/// closed last.
#[derive(Debug, Clone)]
pub struct Column {
    pub tabs: Vec<Surface>,
    pub on: usize,
    pub closed: Vec<Surface>,
    /// The tab under the pointer, which shows its close mark.
    pub hovered: Option<usize>,
}

impl Default for Column {
    /// A session's changes, and beside them its details.
    fn default() -> Self {
        Self {
            tabs: vec![Surface::Diff, Surface::Details],
            on: 0,
            closed: Vec::new(),
            hovered: None,
        }
    }
}

impl Column {
    pub fn shown(&self) -> Option<&Surface> {
        self.tabs.get(self.on)
    }

    /// Show `surface`, opened after the others if it is not open yet.
    pub fn open(&mut self, surface: Surface) {
        self.on = self
            .tabs
            .iter()
            .position(|t| *t == surface)
            .unwrap_or_else(|| {
                self.tabs.push(surface);
                self.tabs.len() - 1
            });
    }

    /// Close the tabs at the places `which` picks; the one shown stays
    /// shown, or the one after it takes its place.
    pub fn close(&mut self, which: impl Fn(usize) -> bool) {
        let shown = self.shown().cloned();
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tabs)
            .into_iter()
            .enumerate()
            .partition(|(n, _)| which(*n));
        self.closed.extend(gone.into_iter().map(|(_, t)| t));
        self.tabs = kept.into_iter().map(|(_, t)| t).collect();
        self.on = shown
            .and_then(|s| self.tabs.iter().position(|t| *t == s))
            .unwrap_or_else(|| self.on.min(self.tabs.len().saturating_sub(1)));
        self.hovered = None;
    }

    /// Open the tab closed last again.
    pub fn reopen(&mut self) {
        if let Some(s) = self.closed.pop() {
            self.open(s);
        }
    }
}

/// What the Open… menu offers, each with the letter that opens it while the
/// menu is open: t3code's, for those Ghost has.
const OFFERS: [(&str, &str, char); 6] = [
    ("Terminal", "terminal", 'T'),
    ("Files", "folder", 'F'),
    ("Diff", "diff", 'D'),
    ("Pull request", "pull-request", 'P'),
    ("Side chat", "bulb", 'S'),
    ("Details", "list", 'I'),
];

impl Desk {
    /// The tabs of the session on screen.
    pub fn column(&self) -> Option<&Column> {
        self.current.and_then(|k| self.columns.get(&k))
    }

    fn column_mut(&mut self) -> Option<&mut Column> {
        let key = self.current?;
        Some(self.columns.entry(key).or_default())
    }

    /// What the column shows now.
    pub fn surface(&self) -> Option<Surface> {
        match self.column() {
            Some(c) => c.shown().cloned(),
            None => Column::default().shown().cloned(),
        }
    }

    /// Show `surface` in the column, opening it if it is not open.
    pub fn open_surface(&mut self, surface: Surface, cx: &mut Context<Self>) {
        if surface == Surface::Terminal
            && self.session_terminals().is_empty()
            && let Some((key, dir)) = self.session().map(|s| (s.key, s.meta.folder.clone()))
        {
            self.open_terminal_in(key, &dir, None, cx);
        }
        if let Some(c) = self.column_mut() {
            c.open(surface);
            let on = c.on;
            self.tab_scroll.scroll_to_item(on);
        }
        self.panes.right = true;
        cx.notify();
    }

    /// Show the `n`th tab.
    fn show_tab(&mut self, n: usize, cx: &mut Context<Self>) {
        if let Some(c) = self.column_mut() {
            c.on = n.min(c.tabs.len().saturating_sub(1));
        }
        cx.notify();
    }

    /// Close the tabs `which` picks, by place.
    pub fn close_tabs(&mut self, which: impl Fn(usize) -> bool, cx: &mut Context<Self>) {
        if let Some(c) = self.column_mut() {
            c.close(which);
        }
        cx.notify();
    }

    /// Close the tab shown (⌘W).
    pub fn close_shown(&mut self, cx: &mut Context<Self>) {
        if let Some(on) = self
            .column()
            .map_or(Some(0), |c| (!c.tabs.is_empty()).then_some(c.on))
        {
            self.close_tabs(|n| n == on, cx);
        }
    }

    /// Open the tab closed last again.
    pub fn reopen_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.column_mut() {
            c.reopen();
        }
        self.panes.right = true;
        cx.notify();
    }

    /// Open or close the Open… menu (⌘T), the column shown for it.
    pub fn toggle_surfaces(&mut self, cx: &mut Context<Self>) {
        self.popup = if self.popup == Popup::Surfaces {
            Popup::Closed
        } else {
            Popup::Surfaces
        };
        self.panes.right = true;
        cx.notify();
    }

    /// Open what the letter `key` stands for in the Open… menu, while it
    /// is open: true when it stood for one.
    pub(crate) fn surface_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.popup != Popup::Surfaces || key.modifiers.modified() {
            return false;
        }
        let Some((_, _, letter)) = OFFERS
            .iter()
            .find(|(_, _, l)| key.key.eq_ignore_ascii_case(&l.to_string()))
        else {
            return false;
        };
        self.offer(*letter, cx);
        true
    }

    /// Open what the Open… menu offers under `letter`.
    fn offer(&mut self, letter: char, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let surface = match letter {
            'T' => Surface::Terminal,
            'F' => Surface::Files,
            'D' => Surface::Diff,
            'P' => Surface::PullRequest,
            'S' => {
                // A new side chat, which opens here once it has begun.
                if let Some(key) = self.current.filter(|_| self.offered('S')) {
                    self.ask_beside(key, cx);
                }
                return cx.notify();
            }
            _ => Surface::Details,
        };
        self.open_surface(surface, cx);
    }

    /// Whether what `letter` stands for can be opened for this session: a
    /// pull request needs a repository, a side chat a session begun.
    fn offered(&self, letter: char) -> bool {
        self.session().is_some_and(|s| match letter {
            'P' => s.repo,
            'S' => s.meta.id.is_some() && s.beside.is_none(),
            _ => true,
        })
    }

    /// The tab strip: the session's tabs, then + for the Open… menu.
    pub(super) fn strip(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let column = self.column().cloned().unwrap_or_default();
        let add = ui::icon_button(
            "open-surface",
            "plus",
            crate::keys::hint(cx, "Open…", "open_surface"),
        )
        .size(px(24.));
        let (faces2, add) = (faces.clone(), add);
        let menu = self
            .menu(
                Popup::Surfaces,
                add,
                move |d, _, cx| d.surface_list(&faces2, cx).w(px(232.)),
                cx,
            )
            .anchor(gpui::Anchor::TopLeft);
        let tabs = column
            .tabs
            .iter()
            .enumerate()
            .map(|(n, t)| self.tab_of(s, t, n, &column, cx))
            .collect::<Vec<_>>();
        let desk = cx.entity();
        div()
            .id("column-tabs")
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_1()
            .min_w(px(0.))
            .overflow_hidden()
            .when(!column.tabs.is_empty(), |d| {
                d.child(
                    TabBar::new("column-tab-bar")
                        .segmented()
                        .small()
                        .menu(true)
                        .track_scroll(&self.tab_scroll)
                        .max_width(px(144.))
                        .min_w(px(0.))
                        .flex_shrink(1.)
                        .selected_index(column.on)
                        .children(tabs)
                        .on_click(cx.listener(|this, n: &usize, _, cx| this.show_tab(*n, cx))),
                )
            })
            .child(menu)
            // Right-click a tab: what can be done with it and the others.
            .context_menu(move |menu, _, cx| {
                let (n, shown) = {
                    let d = desk.read(cx);
                    let c = d.column();
                    (
                        c.and_then(|c| c.hovered),
                        c.and_then(|c| c.hovered.and_then(|n| c.tabs.get(n)).cloned()),
                    )
                };
                let Some(n) = n else { return menu };
                let close = |label: &str, which: fn(usize, usize) -> bool| {
                    let desk = desk.clone();
                    PopupMenuItem::new(label.to_owned()).on_click(move |_, _, cx| {
                        desk.update(cx, |d, cx| d.close_tabs(|at| which(at, n), cx));
                    })
                };
                let mut menu = menu;
                if let Some(Surface::File(path)) = &shown {
                    let copied = path.display().to_string();
                    menu = menu
                        .item(PopupMenuItem::new("Copy path").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                        }))
                        .separator();
                }
                if let Some(Surface::Side(key)) = shown {
                    let desk = desk.clone();
                    menu = menu
                        .item(PopupMenuItem::new("Open in main view").on_click(
                            move |_, window, cx| {
                                desk.update(cx, |d, cx| d.show(key, window, cx));
                            },
                        ))
                        .separator();
                }
                menu.item(close("Close", |at, n| at == n))
                    .item(close("Close others", |at, n| at != n))
                    .item(close("Close to the right", |at, n| at > n))
                    .item(close("Close all", |_, _| true))
            })
    }

    /// One tab: its icon, which turns into its close mark under the
    /// pointer, and its name; a middle click closes it.
    fn tab_of(
        &self,
        s: &Session,
        surface: &Surface,
        n: usize,
        column: &Column,
        cx: &mut Context<Self>,
    ) -> Tab {
        let (name, icon) = self.named(s, surface);
        let under = column.hovered == Some(n);
        let mark = if under {
            ui::icon_button(SharedString::from(format!("close-tab-{n}")), "x", "Close")
                .size(px(16.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tabs(|at| at == n, cx);
                }))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(16.))
                .child(ui::icon(icon, 12.))
                .into_any_element()
        };
        Tab::new()
            .label(name)
            .prefix(mark)
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                let Some(c) = this.column_mut() else { return };
                let now = if *over {
                    Some(n)
                } else {
                    c.hovered.filter(|h| *h != n)
                };
                if now != c.hovered {
                    c.hovered = now;
                    cx.notify();
                }
            }))
            // The tab right-clicked is the one its menu is of, though the
            // pointer came straight to it.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, _| {
                    if let Some(c) = this.column_mut() {
                        c.hovered = Some(n);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| this.close_tabs(|at| at == n, cx)),
            )
    }

    /// A tab's name and icon.
    fn named(&self, s: &Session, surface: &Surface) -> (String, &'static str) {
        match surface {
            Surface::Details => ("Details".into(), "list"),
            Surface::Diff => ("Diff".into(), "diff"),
            Surface::Files => ("Files".into(), "folder"),
            Surface::File(path) => (
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                ),
                "file",
            ),
            Surface::Terminal => ("Terminal".into(), "terminal"),
            Surface::PullRequest => (
                s.git
                    .pr
                    .as_ref()
                    .map_or_else(|| "Pull request".into(), |p| format!("#{}", p.number)),
                "pull-request",
            ),
            Surface::Side(key) => (
                self.sessions
                    .at(*key)
                    .map_or_else(|| "Side chat".into(), |c| c.meta.title.clone()),
                "bulb",
            ),
        }
    }

    /// What can be opened in the column, each with its letter: the Open…
    /// menu, and the column's page when every tab is closed.
    pub(super) fn surface_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col();
        for (name, icon, letter) in OFFERS {
            let on = self.offered(letter);
            list = list.child(
                ui::menu_row(SharedString::from(format!("offer-{letter}")), false)
                    .when(!on, |d| d.opacity(0.45).cursor_default())
                    .child(ui::icon(icon, 15.))
                    .child(ui::text(faces, name).flex_1().text_size(px(13.)))
                    .child(Kbd::new(Keystroke {
                        modifiers: gpui::Modifiers::default(),
                        key: letter.to_ascii_lowercase().to_string(),
                        key_char: None,
                    }))
                    .when(on, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| this.offer(letter, cx)))
                    }),
            );
        }
        list
    }

    /// The column with every tab closed: what can be opened in it.
    pub(super) fn no_tabs(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(ui::text(faces, "Open a surface").text_size(px(14.)))
            .child(self.surface_list(faces, cx).w(px(240.)))
    }

    /// Whether the column shows your terminals now, so the drawer does not.
    pub(crate) fn terminals_beside(&self) -> bool {
        self.panes.right && self.surface() == Some(Surface::Terminal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_opened_twice_is_one_tab() {
        let mut c = Column::default();
        c.open(Surface::Files);
        c.open(Surface::Diff);
        c.open(Surface::Files);
        assert_eq!(c.tabs, [Surface::Diff, Surface::Details, Surface::Files]);
        assert_eq!(c.shown(), Some(&Surface::Files));
    }

    #[test]
    fn closing_the_shown_tab_shows_the_one_after_it() {
        let mut c = Column::default();
        c.open(Surface::Files);
        c.on = 1;
        c.close(|n| n == 1);
        assert_eq!(c.shown(), Some(&Surface::Files));
        c.close(|_| true);
        assert_eq!(c.shown(), None);
    }

    #[test]
    fn the_last_tab_closed_is_the_first_reopened() {
        let mut c = Column::default();
        c.open(Surface::Terminal);
        c.close(|n| n == 0);
        c.close(|n| n == 1);
        c.reopen();
        assert_eq!(c.shown(), Some(&Surface::Terminal));
        c.reopen();
        assert_eq!(c.tabs, [Surface::Details, Surface::Terminal, Surface::Diff]);
    }

    #[test]
    fn closing_others_keeps_the_one_shown() {
        let mut c = Column::default();
        c.open(Surface::Files);
        c.on = 1;
        c.close(|n| n != 1);
        assert_eq!(c.tabs, [Surface::Details]);
        assert_eq!(c.on, 0);
    }
}
