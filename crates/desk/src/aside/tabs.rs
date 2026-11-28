//! The column's tabs, as Carbon's `RightPanelTabs` draws them: each thread
//! keeps its own row of surfaces (its diff, its details, its files, one file,
//! your terminals, its pull request, devices, preview, a side chat). One is
//! opened from the + after the tabs (⌘T) or, while that menu is open, by its
//! letter; a tab closes from its close button, ⌘W or the menu; the last one
//! closed opens again with ⇧⌘T.

use gpui::component::{
    kbd::Kbd,
    list::ListItem,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui::{Context, Div, Entity, Keystroke, SharedString, div, prelude::*, px};
use u0ui::{
    components::MenuItem,
    icons::{CarbonIcon, Icon},
    t3::shell::{RightPanelTabs, SurfaceKind, SurfaceTab},
};

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
    /// The simulators and emulators of its server.
    Devices,
    /// Dev servers printed by its terminals, opened in your browser.
    Preview,
    /// Full output of a tool call, by its ACP id.
    Output(agent_client_protocol::v1::schema::ToolCallId),
}

/// A session's tabs in order, the one shown, and those closed, the last
/// closed last.
#[derive(Debug, Clone)]
pub struct Column {
    pub tabs: Vec<Surface>,
    pub on: usize,
    pub closed: Vec<Surface>,
}

impl Default for Column {
    /// A session's changes, and beside them its details.
    fn default() -> Self {
        Self {
            tabs: vec![Surface::Diff, Surface::Details],
            on: 0,
            closed: Vec::new(),
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
    }

    /// Open the tab closed last again.
    pub fn reopen(&mut self) {
        if let Some(s) = self.closed.pop() {
            self.open(s);
        }
    }
}

/// What the Open… menu offers, each with its Carbon icon and the letter that
/// opens it while the menu is open.
const OFFERS: [(&str, CarbonIcon, char); 8] = [
    ("Terminal", CarbonIcon::Terminal, 'T'),
    ("Preview", CarbonIcon::Earth, 'B'),
    ("Files", CarbonIcon::Folder, 'F'),
    ("Diff", CarbonIcon::Code, 'D'),
    ("Pull request", CarbonIcon::PullRequest, 'P'),
    ("Side chat", CarbonIcon::Chat, 'S'),
    ("Devices", CarbonIcon::Mobile, 'V'),
    ("Details", CarbonIcon::Information, 'I'),
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

    /// Close the tab shown in the right column (⌘W), returning whether one
    /// was open to close.
    pub fn close_shown(&mut self, cx: &mut Context<Self>) -> bool {
        if self.panes.right
            && let Some(on) = self
                .column()
                .and_then(|c| (!c.tabs.is_empty()).then_some(c.on))
        {
            self.close_tabs(|n| n == on, cx);
            return true;
        }
        false
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
            'B' => Surface::Preview,
            'F' => Surface::Files,
            'D' => Surface::Diff,
            'P' => Surface::PullRequest,
            'V' => Surface::Devices,
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
            'P' => s.git_dir().is_some(),
            'S' => s.meta.id.is_some() && s.beside.is_none(),
            _ => true,
        })
    }

    /// The tab strip: Carbon's `RightPanelTabs`, with + for the Open… menu
    /// (`Main`: no other control in the row; ⇧⌘B widens the column).
    pub(super) fn strip(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> RightPanelTabs {
        let column = self.column().cloned().unwrap_or_default();
        let desk = cx.entity();
        let tabs: Vec<SurfaceTab> = column
            .tabs
            .iter()
            .enumerate()
            .map(|(n, surface)| {
                let (label, kind, closable) = self.tab_meta(s, surface, cx);
                SurfaceTab::new(format!("tab-{n}"), label, kind).closable(closable)
            })
            .collect();
        let active_id = format!("tab-{}", column.on);
        let add_items: Vec<MenuItem> = OFFERS
            .iter()
            .map(|(name, icon, letter)| {
                let desk = desk.clone();
                let letter = *letter;
                MenuItem::item(crate::tr(name))
                    .icon(*icon)
                    .shortcut(letter.to_string())
                    .disabled(!self.offered(letter))
                    .on_select(move |_, cx| {
                        desk.update(cx, |d, cx| d.offer(letter, cx));
                    })
            })
            .collect();
        let select_desk = desk.clone();
        let close_desk = desk.clone();
        let mut strip = RightPanelTabs::new("column-tab-bar", tabs)
            .value(active_id)
            .max_visible(if self.panes.whole { 6 } else { 3 })
            .labels(
                crate::keys::hint(cx, "Open another tab", "open_surface"),
                crate::tr("Close {}"),
                crate::tr("{} more surfaces"),
            )
            .add_menu(add_items)
            .on_value_change(move |id, _, cx| {
                if let Some(n) = id
                    .strip_prefix("tab-")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    select_desk.update(cx, |d, cx| d.show_tab(n, cx));
                }
            })
            .on_close(move |id, _, cx| {
                if let Some(n) = id
                    .strip_prefix("tab-")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    close_desk.update(cx, |d, cx| d.close_tabs(|at| at == n, cx));
                }
            });
        // Keyboard-opened Open… menu (⌘T) anchor so letter keys continue to work.
        if self.popup == Popup::Surfaces {
            let faces2 = faces.clone();
            let kbd_menu = self
                .menu(
                    Popup::Surfaces,
                    ui::opener("kbd-surfaces").size(px(1.)),
                    move |d, menu, desk, _| d.surface_menu(&faces2, menu, desk),
                    cx,
                )
                .anchor(gpui::Anchor::TopLeft);
            strip = strip.trailing(kbd_menu);
        }
        strip
    }

    /// A tab's display label, `SurfaceKind`, and whether it shows a permanent
    /// close button.
    fn tab_meta(
        &self,
        s: &Session,
        surface: &Surface,
        cx: &gpui::App,
    ) -> (String, SurfaceKind, bool) {
        match surface {
            Surface::Details => (crate::tr("Details").into(), SurfaceKind::Details, false),
            Surface::Diff => (crate::tr("Diff").into(), SurfaceKind::Diff, false),
            Surface::Files => (crate::tr("Files").into(), SurfaceKind::Files, false),
            Surface::File(path) => {
                let full = if path.is_absolute() {
                    path.clone()
                } else {
                    s.meta.folder.join(path)
                };
                let base_name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let unsaved = self.is_file_unsaved(&full, cx);
                let label = if unsaved {
                    format!("{base_name} \u{00b7} {}", crate::tr("unsaved"))
                } else {
                    base_name
                };
                let is_code = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(
                        e,
                        "rs" | "ts"
                            | "tsx"
                            | "js"
                            | "jsx"
                            | "json"
                            | "py"
                            | "go"
                            | "c"
                            | "cpp"
                            | "h"
                            | "css"
                            | "html"
                            | "htm"
                            | "toml"
                            | "yml"
                            | "yaml"
                    )
                });
                (
                    label,
                    if is_code {
                        SurfaceKind::Diff
                    } else {
                        SurfaceKind::Other
                    },
                    // `FilesDevices`: a file with edits not saved keeps its tab.
                    !unsaved,
                )
            }
            Surface::Terminal => (crate::tr("Terminal").into(), SurfaceKind::Terminal, false),
            Surface::Preview => (crate::tr("Preview").into(), SurfaceKind::Browser, true),
            Surface::Devices => (crate::tr("Devices").into(), SurfaceKind::Device, false),
            Surface::PullRequest => (
                s.git.pr.as_ref().map_or_else(
                    || crate::tr("Pull request").into(),
                    |p| format!("#{}", p.number),
                ),
                SurfaceKind::Pr,
                false,
            ),
            Surface::Side(_) => (crate::tr("Side chat").into(), SurfaceKind::Chat, true),
            Surface::Output(id) => (Self::output_tab_name(s, id), SurfaceKind::Terminal, true),
        }
    }

    /// The key that opens what `letter` stands for while the Open… menu is
    /// open.
    fn letter_key(letter: char) -> Kbd {
        Kbd::new(Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: letter.to_ascii_lowercase().to_string(),
            key_char: None,
        })
    }

    /// The Open… menu (for ⌘T): what can be opened in the column, each with
    /// its letter.
    fn surface_menu(&self, faces: &Faces, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        for (name, icon, letter) in OFFERS {
            let faces = faces.clone();
            menu = menu.item(
                PopupMenuItem::element(move |_, _| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .w(px(200.))
                        .child(Icon::carbon(icon).size_length(px(16.)))
                        .child(
                            ui::text(&faces, crate::tr(name))
                                .flex_1()
                                .text_size(px(crate::style::LABEL)),
                        )
                        .child(Self::letter_key(letter))
                })
                .accessibility_label(name)
                .disabled(!self.offered(letter))
                .on_click(Self::does(desk, move |this, _, cx| this.offer(letter, cx))),
            );
        }
        menu
    }

    /// What can be opened in the column, each with its letter: the
    /// column's page when every tab is closed.
    pub(super) fn surface_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col();
        for (name, icon, letter) in OFFERS {
            let on = self.offered(letter);
            list = list.child(
                ListItem::new(SharedString::from(format!("offer-{letter}")))
                    .disabled(!on)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(Icon::carbon(icon).size_length(px(16.)))
                            .child(
                                ui::text(faces, crate::tr(name))
                                    .flex_1()
                                    .text_size(px(crate::style::LABEL)),
                            )
                            .child(Self::letter_key(letter)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.offer(letter, cx))),
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
            .child(ui::text(faces, crate::tr("Open a surface")).text_size(px(crate::style::BODY)))
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
