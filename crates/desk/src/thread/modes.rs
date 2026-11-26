//! The session's mode, as Claude lists them: each one's name over what it
//! does, from the composer's chip and from the model picker's foot. One
//! list, whether the agent says its modes as modes or as an option.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory as Category;
use gpui::component::{
    Selectable as _,
    kbd::Kbd,
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
};
use gpui::{Anchor, Context, Entity, Window, div, prelude::*, px};

use super::composer::over_box;
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    session::Session,
    theme, ui,
};

/// A mode the session can be put in: its id, name and what it does, and
/// whether it is the one now.
struct Mode {
    id: String,
    name: String,
    about: Option<String>,
    on: bool,
}

/// The session's modes, and the option they are values of when the agent
/// says them as one rather than as modes.
fn modes_of(s: &Session) -> Option<(Option<String>, Vec<Mode>)> {
    use agent_client_protocol::v1::schema::{SessionConfigKind, SessionConfigSelectOptions};
    if let Some(o) = s
        .options
        .iter()
        .find(|o| o.category == Some(Category::Mode))
        && let SessionConfigKind::Select(select) = &o.kind
    {
        let all: Vec<_> = match &select.options {
            SessionConfigSelectOptions::Ungrouped(o) => o.iter().collect(),
            SessionConfigSelectOptions::Grouped(g) => {
                g.iter().flat_map(|g| g.options.iter()).collect()
            }
            _ => Vec::new(),
        };
        let modes = all
            .into_iter()
            .map(|m| Mode {
                id: m.value.0.to_string(),
                name: m.name.clone(),
                about: m.description.clone(),
                on: m.value == select.current_value,
            })
            .collect();
        return Some((Some(o.id.0.to_string()), modes));
    }
    let state = s.modes.as_ref()?;
    let modes = state
        .available_modes
        .iter()
        .map(|m| Mode {
            id: m.id.0.to_string(),
            name: m.name.clone(),
            about: m.description.clone(),
            on: m.id == state.current_mode_id,
        })
        .collect();
    Some((None, modes))
}

/// The icon a mode is drawn with, by what its id or name says it lets the
/// agent do: plan only, edit without asking, decide for itself, anything
/// at all, or ask first; agents name the same ways of working in their own
/// words. Each has an icon of its own.
fn mode_icon(id: &str, name: &str) -> &'static str {
    let words = format!("{id} {name}").to_lowercase();
    let has = |w: &[&str]| w.iter().any(|w| words.contains(w));
    if has(&["plan", "read"]) {
        "list"
    } else if has(&["bypass", "full", "yolo", "danger", "never"]) {
        "alert"
    } else if has(&["edit", "accept"]) {
        "pencil"
    } else if has(&["auto"]) {
        "play"
    } else {
        "user"
    }
}

impl Desk {
    /// The composer's mode chip: the mode's name, its menu over it.
    pub(crate) fn mode_chip(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<Popover> {
        let name = Self::mode_name(s)?;
        let icon = modes_of(s)
            .and_then(|(_, m)| m.into_iter().find(|m| m.on))
            .map_or("user", |m| mode_icon(&m.id, &m.name));
        let chip = ui::opener("chip-mode")
            .h(px(28.))
            .px(px(7.))
            .child(ui::icon(icon, 14.).text_color(theme::faint()))
            .child(
                ui::text(faces, name)
                    .text_size(px(crate::style::LABEL))
                    .text_color(theme::text2()),
            )
            .tooltip(crate::keys::hint(cx, "Choose the mode", "mode"));
        let faces = faces.clone();
        Some(over_box(
            self.menu(
                Popup::Mode,
                chip,
                move |d, menu, desk, _| d.mode_menu(&faces, menu, desk),
                cx,
            ),
            Anchor::BottomRight,
        ))
    }

    /// The picker's foot: the mode, its menu opening upward.
    pub(super) fn mode_button(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<Popover> {
        let name = Self::mode_name(s)?;
        let button = ui::opener("picker-mode")
            .h(px(28.))
            .selected(self.popup == Popup::ModelMode)
            .child(
                ui::text(faces, name)
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::muted()),
            )
            .child(ui::icon("chevron-up", 12.).text_color(theme::faint()));
        let faces = faces.clone();
        Some(
            self.menu(
                Popup::ModelMode,
                button,
                move |d, menu, desk, _| d.mode_menu(&faces, menu, desk),
                cx,
            )
            .anchor(Anchor::BottomRight),
        )
    }

    /// The session's modes as Claude lists them: each one's name over what
    /// it does, the one now ticked, ⌘1 to ⌘9 beside the first nine.
    fn mode_menu(&self, faces: &Faces, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        // Each has its icon, so the one chosen is ticked at the end.
        menu = (menu.label("Mode · this session").min_w(px(300.)))
            .check_side(gpui::component::Side::Right);
        let Some((config, modes)) = self.session().and_then(modes_of) else {
            return menu;
        };
        for (n, m) in modes.into_iter().enumerate() {
            let icon = mode_icon(&m.id, &m.name);
            let (config, id, on) = (config.clone(), m.id, m.on);
            let (faces, name, about) = (faces.clone(), m.name, m.about);
            menu = menu.item(
                PopupMenuItem::element(move |_, _| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .w(px(276.))
                        .child(ui::described(&faces, name.clone(), about.clone()).flex_1())
                        .when(n < 9, |d| d.child(Kbd::new(crate::keys::numbered(n + 1))))
                })
                .icon(ui::glyph(icon))
                .checked(on)
                .on_click(Self::does(desk, move |this, window, cx| {
                    this.pick_mode(config.clone(), id.clone(), cx);
                    this.close_popup(window, cx);
                })),
            );
        }
        menu
    }

    /// Put the session in the mode `id`: as a value of the option `config`
    /// when the agent says its modes as one.
    fn pick_mode(&mut self, config: Option<String>, id: String, cx: &mut Context<Self>) {
        match config {
            Some(config) => self.set_option(config, id, cx),
            None => self.set_mode(id, cx),
        }
    }

    /// Open the mode's menu (⇧⌘A).
    pub fn open_mode(&mut self, cx: &mut Context<Self>) {
        if self.session().and_then(Self::mode_name).is_some() {
            self.popup = Popup::Mode;
            cx.notify();
        }
    }

    /// Choose the `n`th of what the open picker or mode menu lists (⌘1 to
    /// ⌘9): false when neither is open.
    pub(crate) fn choose_nth(
        &mut self,
        n: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.popup {
            Popup::Model => self.pick_nth_model(n, window, cx),
            Popup::Mode | Popup::ModelMode => {
                if let Some((config, modes)) = self.session().and_then(modes_of)
                    && let Some(m) = modes.into_iter().nth(n)
                {
                    self.pick_mode(config, m.id, cx);
                    self.close_popup(window, cx);
                }
            }
            _ => return false,
        }
        true
    }
}
