//! The bar over the middle: the session's title and its menu, the terminal
//! and the right column.

use gpui::component::{Selectable as _, Sizable as _, input::Input, menu::PopupMenu};
use gpui::{Context, Div, Entity, div, prelude::*, px};

use super::meta;
use crate::fonts::Weighted as _;
use crate::{
    desk::{Desk, Page, Popup, Renaming},
    fonts::Faces,
    hub::Stage,
    settings::View,
    theme, ui,
};

/// The command box's widest (`flex: 0 1 400px`).
const COMMAND_BOX_MOST: f32 = 400.;
/// The command box's least: its magnifier alone.
const COMMAND_BOX_LEAST: f32 = 32.;

impl Desk {
    /// The bar over the middle (`TitleBar` + thread/page header).
    pub(crate) fn topbar(&self, faces: &Faces, left_shown: bool, cx: &mut Context<Self>) -> Div {
        // Where the command box is, once it is drawn; nowhere on a page
        // without it.
        self.palette.under.set(None);
        let mut bar = ui::title_bar(!left_shown);
        if !left_shown {
            bar = bar.child(
                ui::icon_button(
                    "show-sidebar",
                    "sidebar",
                    crate::keys::hint(cx, &crate::tr("Show the sidebar"), "toggle_sidebar"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx))),
            );
        }
        let title = |t: String| {
            ui::one_line(
                ui::text(faces, t)
                    .text_size(px(crate::style::LABEL))
                    .weight(gpui::FontWeight::MEDIUM),
            )
            .w_auto()
            .flex_shrink_1()
        };
        if !matches!(self.page, Page::Thread | Page::Workflows | Page::Usage) {
            bar = bar.child(
                ui::icon_button("back", "chevron-left", crate::tr("Back · Esc")).on_click(
                    cx.listener(|this, _, window, cx| this.open_page(Page::Thread, window, cx)),
                ),
            );
        }
        // The title side and the buttons' side, each as wide as the other,
        // the command box between them (`CommandBox`).
        let mut left = div()
            .flex()
            .flex_1()
            .flex_basis(px(0.))
            .items_center()
            .gap(px(8.));
        let mut right = div()
            .flex()
            .flex_1()
            .flex_basis(px(0.))
            .justify_end()
            .items_center()
            .gap(px(8.));
        match &self.page {
            Page::Thread => {
                if let Some(m) = self.guest_machine() {
                    let now = std::time::SystemTime::now();
                    let left_label = match m.guest_until {
                        Some(ts) => {
                            let w = crate::when::short_at(
                                crate::store::time(ts),
                                now,
                                self.settings.twelve,
                            );
                            crate::tr_with(
                                "u0 · %{host} · read only, until %{when}",
                                &[("host", &m.name), ("when", &w)],
                            )
                        }
                        None => crate::tr_with("u0 · %{host} · read only", &[("host", &m.name)]),
                    };
                    let on_mac = m
                        .guest_label
                        .clone()
                        .unwrap_or_else(|| crate::tr("This Mac").to_string());
                    return bar
                        .bg(theme::role(u0ui::tokens::ColorRole::Layer01))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(theme::role(u0ui::tokens::ColorRole::TextSecondary))
                                .child(
                                    u0ui::Icon::named("dns")
                                        .size_length(px(16.))
                                        .color(theme::role(u0ui::tokens::ColorRole::IconSecondary))
                                        .flex_none(),
                                )
                                .child(left_label),
                        )
                        .child(div().flex_1())
                        .child(
                            ui::text(faces, crate::tr_with("u0 on %{name}", &[("name", &on_mac)]))
                                .text_size(px(12.))
                                .text_color(theme::role(u0ui::tokens::ColorRole::TextHelper)),
                        )
                        .child(
                            ui::icon_button(
                                "toggle-aside",
                                "panel-right",
                                crate::keys::hint(
                                    cx,
                                    &crate::tr("Show or hide the right column"),
                                    "right_column",
                                ),
                            )
                            .selected(self.panes.right && self.session().is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_aside(cx))),
                        );
                }
                if let Some(s) = self.session() {
                    // The title alone, as Claude sets it: a click renames
                    // it where it is, its caret opens the session's menu.
                    let new = s.items.is_empty();
                    let shown = s.shown_title().to_owned();
                    let key = s.key;
                    let here = Renaming::Session { key, here: true };
                    let named: gpui::AnyElement = if self.renaming.as_ref() == Some(&here) {
                        div()
                            .w(px(320.))
                            .child(Input::new(&self.rename).small())
                            .into_any_element()
                    } else {
                        // Its whole width, up to half the column: the
                        // command box beside it narrows first.
                        div()
                            .id("title")
                            .flex()
                            .flex_none()
                            .max_w(px(f32::from(self.column.size.width) / 2.))
                            .child(ui::one_line(
                                ui::text(faces, shown)
                                    .text_size(px(crate::style::BODY))
                                    .text_color(if new { theme::muted() } else { theme::text() }),
                            ))
                            // Not the bar's: a click here renames, it does
                            // not move the window.
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .when(!new, |d| {
                                d.cursor_text()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.begin_rename(
                                            Renaming::Session { key, here: true },
                                            window,
                                            cx,
                                        );
                                    }))
                            })
                            .into_any_element()
                    };
                    let caret =
                        ui::icon_button("title-menu", "chevron-down", crate::tr("This thread"))
                            .size(px(22.))
                            .text_color(theme::faint());
                    left = left.child(named).child(self.menu(
                        Popup::Title,
                        caret,
                        Self::title_menu,
                        cx,
                    ));
                }
                right = right
                    .child(
                        ui::icon_button(
                            "toggle-drawer",
                            "terminal",
                            crate::keys::hint(cx, &crate::tr("Terminal"), "drawer"),
                        )
                        .selected(self.drawer.open)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_drawer(window, cx)),
                        ),
                    )
                    .child(
                        ui::icon_button(
                            "toggle-aside",
                            "panel-right",
                            crate::keys::hint(
                                cx,
                                &crate::tr("Show or hide the right column"),
                                "right_column",
                            ),
                        )
                        .selected(self.panes.right && self.session().is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_aside(cx))),
                    );
            }
            Page::Agent(host, id) => {
                let host = *host;
                let name = self.name_of(id);
                let state = match self.slot(host, id).map(|s| &s.stage) {
                    None if self.here_on(host, id).is_some_and(|h| h.missing.is_none()) => "Off",
                    Some(Stage::Missing(_)) | None => "Not installed",
                    Some(Stage::Installing(_)) => "Installing",
                    Some(Stage::Running { .. }) => "Running",
                    Some(Stage::Failed(_)) => "Stopped",
                    Some(_) => "Installed",
                };
                return bar
                    .child(ui::mark(faces, &self.mark_of(id), 16.))
                    .child(title(name))
                    .child(meta(faces, crate::tr(state)));
            }
            Page::Forge(kind) => {
                left = left.child(title(
                    crate::tr(match kind {
                        crate::github::Kind::Pr => "Pull requests",
                        crate::github::Kind::Issue => "Issues",
                    })
                    .to_string(),
                ));
            }
            Page::Workflows => (left, right) = self.workflows_topbar(left, right, cx),
            Page::Usage => (left, right) = self.usage_topbar(left, right, cx),
        }
        // Centred while there is room, else narrowed, at last to its
        // magnifier.
        bar.child(left)
            .child(
                div()
                    .flex()
                    .flex_shrink_1()
                    .flex_basis(px(COMMAND_BOX_MOST))
                    .min_w(px(COMMAND_BOX_LEAST))
                    .child(self.command_box(cx)),
            )
            .child(right)
    }

    /// The command box: what the palette (⌘K) looks through, and its key;
    /// a press opens the palette under it. Where it is drawn is kept, for
    /// the palette to hang from.
    fn command_box(&self, cx: &mut Context<Self>) -> gpui::Div {
        let desk = cx.entity();
        let at = self.palette.under.clone();
        div()
            .relative()
            .flex()
            .w_full()
            // Not the bar's: a press here opens the palette, it does not
            // move the window.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                gpui::canvas(move |bounds, _, _| at.set(Some(bounds)), |_, (), _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                u0ui::u0::shell::CommandBox::new(
                    "command-box",
                    crate::tr(crate::desk::palette::SEARCHES),
                )
                .shortcut(crate::keys::written(&crate::keys::keys_of(cx, "palette")))
                .open(self.palette.open)
                .on_click(move |window, cx| {
                    desk.update(cx, |d, cx| {
                        d.open_palette(crate::desk::palette::Through::Everything, window, cx);
                    });
                }),
            )
    }

    /// Thread menu under the title's caret (`Dialogs` board): Rename, Pin,
    /// Settle, Snooze, Open beside, Branch, New side chat, Copy thread link,
    /// Share read only, Link PR, Tags, Transcript view, Switch/Continue agent,
    /// Archive and Delete.
    fn title_menu(&self, mut menu: PopupMenu, desk: &Entity<Self>, cx: &gpui::App) -> PopupMenu {
        let Some(s) = self.session() else {
            return menu;
        };
        let key = s.key;
        let pinned = s.meta.flags.pinned.is_some();
        let settled = s.meta.flags.settled.is_some();
        let can_fork = self.can_fork(s);
        let repo = s.repo;
        let pr = s.git.pr.is_some();
        let link_str = s
            .meta
            .id
            .as_ref()
            .map(|id| crate::desk::links::Link::to_session(&s.meta.agent, &id.0));
        let pin_keys = crate::keys::written(&crate::keys::keys_of(cx, "pin"));
        let settle_keys = crate::keys::written(&crate::keys::keys_of(cx, "settle"));
        let with_key = |label: gpui::SharedString, shortcut: &str| {
            if shortcut.is_empty() {
                label.to_string()
            } else {
                format!("{label}    {shortcut}")
            }
        };

        menu = menu
            .min_w(px(272.))
            .item(
                Self::menu_item(
                    desk,
                    with_key(crate::tr("Rename"), "F2"),
                    move |this, window, cx| {
                        this.begin_rename(Renaming::Session { key, here: true }, window, cx);
                    },
                )
                .icon(ui::menu_glyph("pencil")),
            )
            .item(
                Self::menu_item(
                    desk,
                    with_key(crate::tr(if pinned { "Unpin" } else { "Pin" }), &pin_keys),
                    move |this, _, cx| this.toggle_pin(key, cx),
                )
                .icon(ui::menu_glyph("pin")),
            )
            .item(
                Self::menu_item(
                    desk,
                    with_key(
                        crate::tr(if settled { "Un-settle" } else { "Settle" }),
                        &settle_keys,
                    ),
                    move |this, _, cx| this.toggle_settled(key, cx),
                )
                .icon(ui::menu_glyph("check")),
            )
            .item(
                Self::menu_item(desk, crate::tr("Snooze…"), move |this, window, cx| {
                    this.open_snooze_dialog(vec![key], window, cx);
                })
                .icon(ui::menu_glyph("clock")),
            )
            .separator()
            .item(
                Self::menu_item(desk, crate::tr("Open beside"), move |this, window, cx| {
                    this.open_beside(key, window, cx);
                })
                .icon(ui::menu_glyph("panel-right")),
            );
        if can_fork {
            menu = menu.item(
                Self::menu_item(
                    desk,
                    crate::tr("Branch a new thread from here"),
                    move |this, _, cx| this.fork(key, cx),
                )
                .icon(ui::menu_glyph("branch")),
            );
            if s.beside.is_none() {
                menu = menu.item(
                    Self::menu_item(desk, crate::tr("New side chat"), move |this, _, cx| {
                        this.ask_beside(key, cx);
                    })
                    .icon(ui::menu_glyph("bulb")),
                );
            }
        }
        menu = menu
            .separator()
            .item(
                Self::menu_item(desk, crate::tr("Copy thread link"), move |this, _, cx| {
                    if let Some(url) = &link_str {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                        this.done(crate::tr("Copied"), None);
                        cx.notify();
                    } else {
                        this.copy_of_session(|s| s.meta.id.as_ref().map(|i| i.0.to_string()), cx);
                    }
                })
                .icon(ui::menu_glyph("copy")),
            )
            .item(
                Self::menu_item(
                    desk,
                    crate::tr("Share read only with a device…"),
                    |this, window, cx| {
                        this.pages.grant.scope = crate::access::Scope::Read;
                        this.share(crate::share::Share::Serve(true));
                        this.new_link();
                        this.open_tab(crate::pages::Tab::Devices, window, cx);
                    },
                )
                .icon(ui::menu_glyph("laptop")),
            );
        if repo {
            menu = menu.item(
                Self::menu_item(
                    desk,
                    crate::tr("Link a pull request…"),
                    move |this, window, cx| {
                        this.open_link_pr_dialog(key, window, cx);
                    },
                )
                .icon(ui::menu_glyph("pull-request")),
            );
        }
        menu = menu.item(
            Self::menu_item(desk, crate::tr("Tags…"), |this, window, cx| {
                this.open_tag_page(window, cx);
            })
            .icon(ui::menu_glyph("list")),
        );

        let others = if s.meta.id.is_some() {
            self.others(s.host, &s.meta.agent)
        } else {
            Vec::new()
        };
        if !others.is_empty() {
            menu = menu.separator();
            for (id, name) in others {
                menu = menu.item(Self::menu_item(
                    desk,
                    crate::tr_with("Continue with %{name}", &[("name", &name)]),
                    move |this, window, cx| this.hand_off(key, &id, window, cx),
                ));
            }
        }

        menu = menu.separator().label(crate::tr("Transcript view"));
        for (says, view) in [
            ("Answers only", View::Zen),
            ("Calls that went wrong", View::Quiet),
            ("Normal", View::Normal),
            ("Thinking", View::Thinking),
            ("Verbose", View::Verbose),
        ] {
            let on = self.settings.view == view;
            menu = menu.item(
                Self::menu_item(desk, crate::tr(says), move |this, _, cx| {
                    this.set_view(view, cx);
                })
                .checked(on),
            );
        }
        menu = menu.separator().item(
            Self::menu_item(desk, crate::tr("Open in your editor"), |this, _, _| {
                if let Some(s) = this.session() {
                    this.edit(s.meta.folder.clone(), None);
                }
            })
            .icon(ui::menu_glyph("folder")),
        );
        if pr {
            menu = menu.item(
                Self::menu_item(
                    desk,
                    crate::tr("Copy the pull request's address"),
                    |this, _, cx| {
                        this.copy_of_session(|s| s.git.pr.as_ref().map(|p| p.url.clone()), cx);
                    },
                )
                .icon(ui::menu_glyph("copy")),
            );
        }
        menu.item(Self::menu_item(
            desk,
            crate::tr("Share read-only link…"),
            |this, window, cx| {
                this.open_pairing_dialog(true, window, cx);
            },
        ))
        .separator()
        .item(
            Self::menu_item(desk, crate::tr("Archive"), move |this, _, cx| {
                this.archive(key, cx);
            })
            .icon(ui::menu_glyph("archive")),
        )
        .item(
            Self::menu_item(desk, crate::tr("Delete…"), move |this, window, cx| {
                this.ask_delete(vec![key], window, cx);
            })
            .icon(ui::menu_glyph("trash")),
        )
    }

    /// Show the transcript as `view` says, and keep it so.
    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.settings.view = view;
        self.save_settings();
        cx.notify();
    }
}
