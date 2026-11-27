//! Settings → Projects, as t3code's: the projects, each a click from its
//! page, where it is named, its icon shown, the agent and the way of
//! working its new sessions take chosen, pinned, archived, or removed.

use gpui::component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    setting::{SettingGroup, SettingItem, SettingPage},
    switch::Switch,
};
use gpui::{AnyElement, Context, Div, SharedString, WeakEntity, Window, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::{Desk, Place, Renaming},
    fonts::Faces,
    theme, ui,
};

impl Desk {
    /// The projects, each a row that opens its page; archived ones too.
    /// With a project chosen, its page.
    pub(super) fn projects_tab(&self, w: &WeakEntity<Desk>) -> SettingPage {
        if let Some(folder) = self.pages.project.clone() {
            return self.project_page(w, &folder);
        }
        let mut words: Vec<&'static str> = vec!["projects", "folders"];
        words.extend(["pinned", "archived"]);
        let list = whole(w, &words, |d, faces, cx| {
            d.project_list(faces, cx).into_any_element()
        });
        let adding = vec![
            item(
                w,
                crate::tr("New projects go in"),
                crate::tr_with(
                    "%{folder}: a project made or cloned in the palette.",
                    &[("folder", &Desk::shown(&self.projects_in(self.pages.on)))],
                ),
                |_, _, cx| {
                    // macOS's folder picker shows this Mac's folders.
                    ui::button("projects-in", crate::tr("Choose…"), ui::Weight::Secondary)
                        .on_click(cx.listener(|_, _, _, cx| Desk::choose_parent(cx)))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("Clone into host folders"),
                crate::tr("A clone goes in <host>/<owner>/<repo> there, as ghq lays them out."),
                |d, _, cx| {
                    Switch::new("host-folders")
                        .small()
                        .checked(d.edited().host_folders)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = &mut this.edited_mut().host_folders;
                            *on = !*on;
                            this.save_edited();
                            cx.notify();
                        }))
                        .into_any_element()
                },
            ),
        ];
        SettingPage::new(crate::tr("Projects"))
            .description(crate::tr(
                "The folders you work in, and how each one's sessions begin.",
            ))
            .group(SettingGroup::new().item(list))
            .group(SettingGroup::new().items(adding))
    }

    /// The projects, each a row that opens its page.
    fn project_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let on = self.pages.on;
        let mut folders: Vec<Place> = (self.edited().folders.iter())
            .map(|f| Place::new(on, f.clone()))
            .collect();
        folders.sort_by_key(|f| (self.prefs(f).archived, !self.prefs(f).pinned));
        div()
            .flex()
            .flex_col()
            .w_full()
            .children(folders.into_iter().enumerate().map(|(n, folder)| {
                let prefs = self.prefs(&folder);
                let open = folder.clone();
                let icon = self.icon_of(&folder);
                div()
                    .id(SharedString::from(format!("project-{n}")))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(4.))
                    .h(px(52.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme::row_hover()))
                    .child(crate::desk::projects::project_icon(icon, 16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::text(faces, self.project_name(&folder))
                                    .text_size(px(crate::style::LABEL)),
                            )
                            .child(ui::fine(faces, Desk::shown(&folder.folder))),
                    )
                    .when(prefs.pinned, |d| {
                        d.child(ui::fine(faces, crate::tr("Pinned")))
                    })
                    .when(prefs.archived, |d| {
                        d.child(ui::fine(faces, crate::tr("Archived")))
                    })
                    .child(ui::icon("chevron-right", 12.).text_color(theme::faint()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pages.project = Some(open.clone());
                        cx.notify();
                    }))
            }))
    }

    /// One project's page.
    fn project_page(&self, w: &WeakEntity<Desk>, folder: &Place) -> SettingPage {
        let dir = folder.clone();
        let mut rows = vec![
            item(
                w,
                crate::tr("Name"),
                crate::tr("What the sidebar calls it."),
                {
                    let dir = dir.clone();
                    move |d, faces, cx| d.project_name_control(&dir, faces, cx)
                },
            ),
            item(
                w,
                crate::tr("Icon"),
                crate::tr(
                    "A picture, or letters or an emoji on a colour; Automatic, its own icon file (favicon, app icon) where it has one.",
                ),
                {
                    let dir = dir.clone();
                    move |d, _, cx| {
                        let icon = d.icon_of(&dir);
                        d.icon_control(&dir, icon, cx).into_any_element()
                    }
                },
            ),
            item(
                w,
                crate::tr("Agent"),
                crate::tr("The agent its new sessions begin with."),
                {
                    let dir = dir.clone();
                    move |d, _, cx| d.project_agent(&dir, cx)
                },
            ),
        ];
        if self.held.get(folder).is_some_and(|h| h.repo) {
            let dir = dir.clone();
            rows.push(item(
                w,
                crate::tr("New sessions"),
                crate::tr(
                    "Where they work: in the folder, or a worktree of their own on a new branch.",
                ),
                move |d, _, cx| {
                    let worktree = d
                        .prefs(&dir)
                        .worktree
                        .unwrap_or(d.settings_on(dir.host).worktree);
                    let (local, own): (ui::Pick, ui::Pick) = (
                        Box::new(Self::on_project(cx, &dir, |d, f, _, cx| {
                            d.set_project_worktree(f, false, cx);
                        })),
                        Box::new(Self::on_project(cx, &dir, |d, f, _, cx| {
                            d.set_project_worktree(f, true, cx);
                        })),
                    );
                    ui::segmented(
                        "project-worktree",
                        vec![
                            (crate::tr("Local"), !worktree, local),
                            (crate::tr("New worktree"), worktree, own),
                        ],
                    )
                    .into_any_element()
                },
            ));
        }
        let switch =
            |id: &'static str,
             on: fn(&crate::settings::Prefs) -> bool,
             flip: fn(&mut Desk, &Place, &mut Window, &mut Context<Desk>)| {
                let dir = dir.clone();
                move |d: &mut Desk, _: &Faces, cx: &mut Context<Desk>| {
                    Switch::new(id)
                        .small()
                        .checked(on(&d.prefs(&dir)))
                        .on_click(Self::on_project(cx, &dir, flip))
                        .into_any_element()
                }
            };
        rows.push(item(
            w,
            crate::tr("Pinned"),
            crate::tr("Pinned projects come first in the sidebar and in project pickers."),
            switch(
                "project-pinned",
                |p| p.pinned,
                |d, f, _, cx| d.toggle_project_pin(f, cx),
            ),
        ));
        rows.push(item(
            w,
            crate::tr("Pull its default branch"),
            crate::tr("As u0 starts and each time it fetches, while it is checked out with nothing changed, and only where it moves straight on."),
            switch("project-pull", |p| p.pull, |d, f, _, cx| {
                d.set_prefs(f, cx, |p| p.pull = !p.pull);
            }),
        ));
        rows.push(item(
            w,
            crate::tr("Archived"),
            crate::tr("Hides the project from the sidebar and project pickers. Sessions and files are kept."),
            switch(
                "project-archived",
                |p| p.archived,
                |d, f, _, cx| {
                    if d.prefs(f).archived {
                        d.restore_project(f, cx);
                    } else {
                        d.archive_project(f, cx);
                    }
                },
            ),
        ));
        let danger = item(
            w,
            crate::tr("Remove project"),
            crate::tr("Forgets the project and its sessions here. Files on disk are not touched."),
            {
                let dir = dir.clone();
                move |_, _, cx| {
                    Button::new("remove-project")
                        .danger()
                        .small()
                        .label(crate::tr("Remove…"))
                        .on_click(Self::on_project(cx, &dir, Desk::ask_remove_project))
                        .into_any_element()
                }
            },
        );
        let back = whole(w, &[], |_, _, cx| {
            div()
                .flex()
                .child(
                    Button::new("all-projects")
                        .ghost()
                        .small()
                        .icon(ui::glyph("chevron-left"))
                        .label(crate::tr("Projects"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pages.project = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        });
        SettingPage::new(self.project_name(folder))
            .description(Desk::shown(&folder.folder))
            .group(SettingGroup::new().item(back).items(rows))
            .group(group(crate::tr("Checkouts"), self.checkouts(w, folder)))
            .group(group(crate::tr("Danger zone"), vec![danger]))
    }

    /// The project's name, and the way to rename it, or the field it is
    /// renamed in.
    fn project_name_control(
        &self,
        folder: &Place,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder) {
            return div()
                .w(px(220.))
                .child(Input::new(&self.rename).small())
                .into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::text(faces, self.project_name(folder)).text_size(px(crate::style::LABEL)))
            .child(
                Button::new("rename-project")
                    .ghost()
                    .small()
                    .label(crate::tr("Rename"))
                    .on_click(Self::on_project(cx, folder, |d, f, window, cx| {
                        d.begin_rename(Renaming::Project(f.clone()), window, cx);
                    })),
            )
            .into_any_element()
    }

    /// The agent the project's new sessions begin with, and the menu of
    /// those there are.
    fn project_agent(&self, folder: &Place, cx: &mut Context<Self>) -> AnyElement {
        let prefs = self.prefs(folder);
        let agent_name = prefs.agent.as_deref().map_or_else(
            || crate::tr("The one on screen").to_string(),
            |a| self.name_of(a),
        );
        let desk = cx.entity();
        let (menu_dir, menu_agent) = (folder.clone(), prefs.agent.clone());
        let agents: Vec<(Option<String>, String)> =
            std::iter::once((None, crate::tr("The one on screen").to_string()))
                .chain(
                    self.slots_on(folder.host)
                        .map(|s| (Some(s.entry.id.clone()), self.name_of(&s.entry.id))),
                )
                .collect();
        Button::new("project-agent")
            .outline()
            .small()
            .label(agent_name)
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name) in &agents {
                    let (desk, dir, id) = (desk.clone(), menu_dir.clone(), id.clone());
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(menu_agent == id)
                            .on_click(move |_, _, cx| {
                                desk.update(cx, |d, cx| d.set_project_agent(&dir, id.clone(), cx));
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// Where the project is checked out: its folder, and each of u0's
    /// worktrees of it, with the sessions in it, each one removable.
    fn checkouts(&self, w: &WeakEntity<Desk>, folder: &Place) -> Vec<SettingItem> {
        let mut worktrees: Vec<(crate::git::Worktree, u64, usize)> = Vec::new();
        for s in self
            .sessions
            .all
            .iter()
            .filter(|s| Place::project_of(s) == *folder)
        {
            let Some(wt) = &s.meta.worktree else { continue };
            match worktrees.iter_mut().find(|(w, ..)| w.path == wt.path) {
                Some((.., n)) => *n += 1,
                None => worktrees.push((wt.clone(), s.key, 1)),
            }
        }
        let mut rows = vec![item(
            w,
            Desk::shown(&folder.folder),
            "The project's own folder.",
            |_, _, _| div().into_any_element(),
        )];
        for (n, (wt, key, sessions)) in worktrees.into_iter().enumerate() {
            let about = crate::tr_with(
                if sessions == 1 {
                    "%{branch} · %{sessions} session"
                } else {
                    "%{branch} · %{sessions} sessions"
                },
                &[("branch", &wt.branch), ("sessions", &sessions)],
            );
            rows.push(item(w, Desk::shown(&wt.path), about, move |_, _, cx| {
                let wt = wt.clone();
                Button::new(SharedString::from(format!("remove-checkout-{n}")))
                    .ghost()
                    .small()
                    .label(crate::tr("Remove"))
                    .tooltip(crate::tr(
                        "Its branch is kept; one with changes not committed is refused",
                    ))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.remove_checkout(wt.clone(), key);
                    }))
                    .into_any_element()
            }));
        }
        rows
    }

    /// The project's icon, and the menu that changes it; its mark's
    /// letters typed in place.
    fn icon_control(
        &self,
        folder: &Place,
        icon: Option<crate::settings::Icon>,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::desk::projects::MARKS;
        let marking = matches!(&self.renaming, Some(Renaming::Mark(f)) if f == folder);
        let (desk, dir) = (cx.entity(), folder.clone());
        let menu = Button::new("project-icon")
            .outline()
            .small()
            .label(crate::tr("Change"))
            .dropdown_menu(move |mut menu, _, _| {
                let on = |f: fn(&mut Desk, &Place, &mut Window, &mut Context<Desk>)| {
                    let (desk, dir) = (desk.clone(), dir.clone());
                    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                        desk.update(cx, |d, cx| f(d, &dir, window, cx));
                    }
                };
                menu = menu
                    .item(
                        PopupMenuItem::new(crate::tr("Automatic")).on_click(on(|d, f, _, cx| {
                            d.set_prefs(f, cx, |p| p.icon = None);
                        })),
                    )
                    .item(
                        PopupMenuItem::new(crate::tr("Choose a picture…"))
                            .on_click(on(|_, f, _, cx| Desk::choose_icon(f, cx))),
                    )
                    .item(
                        PopupMenuItem::new(crate::tr("Letters or an emoji…")).on_click(on(
                            |d, f, w, cx| {
                                d.begin_rename(Renaming::Mark(f.clone()), w, cx);
                            },
                        )),
                    )
                    .separator();
                for (n, (name, _)) in MARKS.iter().enumerate() {
                    let (desk, dir) = (desk.clone(), dir.clone());
                    menu = menu.item(PopupMenuItem::new(crate::tr(name)).on_click(
                        move |_, _, cx| {
                            desk.update(cx, |d, cx| d.mark_colour(&dir, n as u8, cx));
                        },
                    ));
                }
                menu
            });
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(crate::desk::projects::project_icon(icon, 20.))
            .when(marking, |d| {
                d.child(div().w(px(80.)).child(Input::new(&self.rename).small()))
            })
            .child(menu)
    }

    /// A listener doing `f` to the project `dir`, whatever its event.
    fn on_project<E: 'static>(
        cx: &mut Context<Desk>,
        dir: &Place,
        f: fn(&mut Desk, &Place, &mut Window, &mut Context<Desk>),
    ) -> impl Fn(&E, &mut Window, &mut gpui::App) + 'static {
        let dir = dir.clone();
        cx.listener(move |this, _: &E, window, cx| f(this, &dir, window, cx))
    }

    /// Settings → Projects, at the page of `folder`.
    pub fn project_settings(
        &mut self,
        folder: &Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages.on = folder.host;
        self.pages.project = Some(folder.clone());
        self.open_tab(super::Tab::Projects, window, cx);
    }

    /// Ask before removing the project `folder`, in t3code's words: its
    /// sessions are forgotten here, the agents keep theirs.
    pub(crate) fn ask_remove_project(
        &mut self,
        folder: &Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::component::WindowExt as _;
        let n = self
            .sessions
            .all
            .iter()
            .filter(|s| Place::project_of(s) == *folder && s.parent.is_none())
            .count();
        let title = crate::tr_with(
            if n == 1 {
                "Remove project “%{name}” and forget its %{n} session?"
            } else {
                "Remove project “%{name}” and forget its %{n} sessions?"
            },
            &[("name", &self.project_name(folder)), ("n", &n)],
        );
        let said = crate::tr_with(
            "Path: %{path}\nu0 forgets these sessions and their records. Each agent keeps its own sessions.\nFiles on disk are not touched.\nThis cannot be undone.",
            &[("path", &Desk::shown(&folder.folder))],
        );
        let (desk, folder) = (cx.entity(), folder.clone());
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (desk, folder) = (desk.clone(), folder.clone());
            alert
                .title(title.clone())
                .description(said.clone())
                .ok_text(crate::tr("Remove"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    desk.update(cx, |d, cx| d.remove_project(&folder, cx));
                    true
                })
        });
    }

    /// Take the project `folder` off the list, and forget its sessions
    /// and their records; their worktrees and the files are left alone.
    fn remove_project(&mut self, folder: &Place, cx: &mut Context<Self>) {
        let keys: Vec<u64> = self
            .sessions
            .all
            .iter()
            .filter(|s| Place::project_of(s) == *folder && s.parent.is_none())
            .map(|s| s.key)
            .collect();
        for key in keys {
            self.keep_worktree.insert(key);
            self.forget_session(key, false, cx);
        }
        self.filter.retain(|f| f != folder);
        let settings = self.settings_on_mut(folder.host);
        settings.forget(&folder.folder);
        settings.prefs.remove(&folder.folder);
        self.closed.remove(folder);
        self.pages.project = None;
        self.save_settings_on(folder.host);
        self.save_index();
        cx.notify();
    }
}
