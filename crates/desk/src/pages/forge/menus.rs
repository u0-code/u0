//! The PRs and Issues pages' menus: the project shown, the views saved,
//! and what can be done with one, under its "⋯" and on its row.

use gpui::component::menu::PopupMenu;
use gpui::{Context, Entity, prelude::*, px};

use crate::{
    desk::{Desk, Popup},
    github::{Act, Item, Kind},
    ui,
};

/// What a line of a menu does.
type Act3 = Box<dyn Fn(&mut Desk, &mut gpui::Window, &mut Context<Desk>)>;

impl Desk {
    /// The projects a page can be of: each repository of one that holds
    /// several.
    pub(super) fn forge_projects(&self, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        menu = menu
            .item(
                Self::menu_item(desk, crate::tr("All projects"), |this, window, cx| {
                    this.list_projects(true, None, window, cx);
                })
                .checked(self.forge.every.is_some()),
            )
            .separator();
        for folder in self.forge_places() {
            let on = self.forge.every.is_none() && self.forge.folder.as_ref() == Some(&folder);
            let name = self.forge_name(&folder);
            menu = menu.item(
                Self::menu_item(desk, name, move |this, window, cx| {
                    this.list_projects(false, Some(folder.clone()), window, cx);
                })
                .checked(on),
            );
        }
        menu.min_w(px(240.))
    }

    /// The views of this page saved, each chosen to show it; saving the one
    /// shown, or removing one.
    pub(super) fn forge_views(&self, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let kind = self.forge.kind;
        let views: Vec<_> = (self.settings.views.iter())
            .filter(|v| v.kind == kind)
            .cloned()
            .collect();
        for v in &views {
            let view = v.clone();
            menu = menu.item(Self::menu_item(
                desk,
                v.name.clone(),
                move |this, window, cx| {
                    this.show_view(&view, window, cx);
                },
            ));
        }
        if !views.is_empty() {
            menu = menu.separator();
        }
        menu = menu.item(Self::menu_item(
            desk,
            crate::tr("Save this view"),
            |this, _, cx| {
                this.save_view(cx);
            },
        ));
        for v in views {
            menu = menu.item(Self::menu_item(
                desk,
                crate::tr_with("Remove %{name}", &[("name", &v.name)]),
                move |this, _, cx| {
                    this.remove_view(&v.name, cx);
                },
            ));
        }
        menu.min_w(px(220.))
    }

    /// What can be done with the one read, under its "⋯".
    pub(super) fn forge_actions(
        &self,
        item: &Item,
        cx: &mut Context<Self>,
    ) -> gpui::component::popover::Popover {
        let trigger = ui::icon_button(
            "forge-actions",
            "dots",
            crate::tr("What can be done with it"),
        )
        .size(px(26.));
        let (item, folder) = (item.clone(), self.forge.folder.clone().unwrap_or_default());
        self.menu(
            Popup::ForgeActions,
            trigger,
            move |d, menu, desk, _| d.forge_menu(&item, folder.clone(), menu, desk),
            cx,
        )
        .anchor(gpui::Anchor::TopRight)
    }

    /// What can be done with `item`, of the project `folder`: under the
    /// "⋯" of the one read, and on a row's menu.
    pub(super) fn forge_menu(
        &self,
        item: &Item,
        folder: crate::desk::Place,
        menu: PopupMenu,
        desk: &Entity<Self>,
    ) -> PopupMenu {
        let (open, pr) = (item.state == "open", self.forge.kind == Kind::Pr);
        let n = item.number;
        let mut rows = Vec::new();
        let mut entry = |label: &str, act: Act3| {
            rows.push(Self::menu_item(
                desk,
                crate::tr(label),
                move |this, window, cx| {
                    act(this, window, cx);
                },
            ));
        };
        // What is asked of the forge is asked in the row's project, which
        // in the list of every project is not always the one read.
        let at = folder.clone();
        let act = |a: Act| -> Act3 {
            let at = at.clone();
            Box::new(move |d, _, cx| {
                d.forge.folder = Some(at.clone());
                d.act_on(n, a.clone(), cx);
            })
        };
        let merge = |how: &'static str, auto: bool| -> Act3 {
            let at = at.clone();
            Box::new(move |d, _, cx| {
                d.forge.folder = Some(at.clone());
                d.act_on(
                    n,
                    Act::Merge {
                        how: how.into(),
                        auto,
                        delete: true,
                        uncredit: d.settings_in(d.forge.folder.as_ref()).uncredit,
                    },
                    cx,
                );
            })
        };
        if pr && open {
            entry("Squash and merge", merge("squash", false));
            entry("Merge", merge("merge", false));
            entry("Rebase and merge", merge("rebase", false));
            entry(
                "Merge when its checks pass",
                merge(
                    self.settings_in(self.forge.folder.as_ref()).merge.gh(),
                    true,
                ),
            );
            entry(
                if item.draft {
                    "Ready for review"
                } else {
                    "Convert to draft"
                },
                act(if item.draft { Act::Ready } else { Act::Draft }),
            );
        }
        if open {
            entry("Close", act(Act::Close));
        } else if item.state == "closed" {
            entry("Reopen", act(Act::Reopen));
        }
        if self.forge.chosen == Some(n) && self.forge.folder.as_ref() == Some(&folder) {
            entry(
                "Edit title and description",
                Box::new(|d, window, cx| d.edit_forge(false, window, cx)),
            );
        }
        let url = item.url.clone();
        entry(
            "Open on its host",
            Box::new({
                let url = url.clone();
                move |_, _, cx| cx.open_url(&url)
            }),
        );
        entry(
            "Copy its link",
            Box::new(move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
            }),
        );
        let title = item.title.clone();
        if pr {
            let (f2, t2) = (folder.clone(), title.clone());
            entry(
                "A session on it, in a worktree",
                Box::new(move |d, window, cx| {
                    d.start_from_pr(f2.clone(), n, t2.clone(), false, window, cx);
                }),
            );
            let (f3, t3) = (folder.clone(), title.clone());
            entry(
                "A session on it, checked out here",
                Box::new(move |d, window, cx| {
                    d.start_from_pr(f3.clone(), n, t3.clone(), true, window, cx);
                }),
            );
            entry(
                "Copy the checkout command",
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                        "gh pr checkout {n}"
                    )));
                }),
            );
            entry(
                "Load new commits",
                Box::new({
                    let at = folder.clone();
                    move |d, _, cx| {
                        d.forge.folder = Some(at.clone());
                        d.read_forge(n, cx);
                    }
                }),
            );
        } else {
            let (f2, t2) = (folder.clone(), title.clone());
            entry(
                "A session about it",
                Box::new(move |d, window, cx| {
                    d.start_from_issue(f2.clone(), n, &t2, false, window, cx);
                }),
            );
            entry(
                "Triage it",
                Box::new(move |d, window, cx| {
                    d.start_from_issue(folder.clone(), n, &title, true, window, cx);
                }),
            );
        }
        rows.into_iter().fold(menu.min_w(px(240.)), PopupMenu::item)
    }
}
