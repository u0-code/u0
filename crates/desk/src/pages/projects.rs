//! Settings → Projects, as t3code's: the projects, each a click from its
//! page, where it is named, its icon shown, the agent and the way of
//! working its new sessions take chosen, pinned, archived, or removed.

use std::path::{Path, PathBuf};

use gpui::component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
};
use gpui::{Context, Div, SharedString, Window, div, prelude::*, px};

use super::prefs::{group, group_of, page, setting};
use crate::{
    desk::{Desk, Renaming},
    fonts::Faces,
    theme, ui,
};

impl Desk {
    /// The projects, each a row that opens its page; archived ones too.
    pub(super) fn projects_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        if let Some(folder) = self.pages.project.clone() {
            return self.project_page(faces, &folder, cx);
        }
        let mut folders = self.settings.folders.clone();
        folders.sort_by_key(|f| (self.prefs(f).archived, !self.prefs(f).pinned));
        let rows = folders
            .into_iter()
            .enumerate()
            .map(|(n, folder)| {
                let prefs = self.prefs(&folder);
                let open = folder.clone();
                let icon = self.icon_of(&folder);
                div()
                    .id(SharedString::from(format!("project-{n}")))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .h(px(52.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme::row_hover()))
                    .child(crate::desk::projects::project_icon(&folder, icon, 16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::text(faces, self.project_name(&folder)).text_size(px(13.5)))
                            .child(ui::fine(faces, Desk::shown(&folder))),
                    )
                    .when(prefs.pinned, |d| d.child(ui::fine(faces, "Pinned")))
                    .when(prefs.archived, |d| d.child(ui::fine(faces, "Archived")))
                    .child(ui::icon("chevron-right", 12.).text_color(theme::faint()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pages.project = Some(open.clone());
                        cx.notify();
                    }))
            })
            .map(|row| div().child(row))
            .collect();
        let adding = vec![
            setting(
                faces,
                "New projects go in",
                &format!(
                    "{}: a project made or cloned in the palette.",
                    Desk::shown(&self.projects_in())
                ),
                ui::button("projects-in", "Choose…", ui::Weight::Secondary)
                    .on_click(cx.listener(|_, _, _, cx| Desk::choose_parent(cx))),
            ),
            setting(
                faces,
                "Clone into host folders",
                "A clone goes in <host>/<owner>/<repo> there, as ghq lays them out.",
                Switch::new("host-folders")
                    .small()
                    .checked(self.settings.host_folders)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.host_folders = !this.settings.host_folders;
                        this.save_settings();
                        cx.notify();
                    })),
            ),
        ];
        page(
            faces,
            "Projects",
            "The folders you work in, and how each one's sessions begin.",
        )
        .child(group_of(rows))
        .child(group_of(adding))
    }

    /// One project's page.
    fn project_page(&self, faces: &Faces, folder: &Path, cx: &mut Context<Self>) -> Div {
        let prefs = self.prefs(folder);
        let dir = folder.to_path_buf();
        let name = if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder) {
            div()
                .w(px(220.))
                .child(Input::new(&self.rename).small())
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::text(faces, self.project_name(folder)).text_size(px(13.5)))
                .child(
                    Button::new("rename-project")
                        .ghost()
                        .small()
                        .label("Rename")
                        .on_click(Self::on_project(cx, &dir, |d, f, window, cx| {
                            d.begin_rename(Renaming::Project(f.to_path_buf()), window, cx);
                        })),
                )
                .into_any_element()
        };
        let icon = self.icon_of(folder);
        let agent_name = prefs
            .agent
            .as_deref()
            .map_or_else(|| "The one on screen".to_owned(), |a| self.name_of(a));
        let desk = cx.entity();
        let (menu_dir, menu_agent) = (dir.clone(), prefs.agent.clone());
        let agents: Vec<(Option<String>, String)> =
            std::iter::once((None, "The one on screen".to_owned()))
                .chain(
                    self.slots
                        .iter()
                        .map(|s| (Some(s.entry.id.clone()), self.name_of(&s.entry.id))),
                )
                .collect();
        let agent = Button::new("project-agent")
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
            });
        let worktree = prefs.worktree.unwrap_or(self.settings.worktree);
        let mut rows = vec![
            setting(faces, "Name", "What the sidebar calls it.", name),
            setting(
                faces,
                "Icon",
                "A picture, or letters or an emoji on a colour; Automatic, its own icon file (favicon, app icon) where it has one.",
                self.icon_control(folder, icon, cx),
            ),
            setting(
                faces,
                "Agent",
                "The agent its new sessions begin with.",
                agent,
            ),
        ];
        if self.held.get(folder).is_some_and(|h| h.repo) {
            let (local, own): (ui::Pick, ui::Pick) = (
                Box::new(Self::on_project(cx, &dir, |d, f, _, cx| {
                    d.set_project_worktree(f, false, cx);
                })),
                Box::new(Self::on_project(cx, &dir, |d, f, _, cx| {
                    d.set_project_worktree(f, true, cx);
                })),
            );
            rows.push(setting(
                faces,
                "New sessions",
                "Where they work: in the folder, or a worktree of their own on a new branch.",
                ui::segmented(
                    "project-worktree",
                    vec![("Local", !worktree, local), ("New worktree", worktree, own)],
                ),
            ));
        }
        rows.push(setting(
            faces,
            "Pinned",
            "Pinned projects come first in the sidebar and in project pickers.",
            Switch::new("project-pinned")
                .small()
                .checked(prefs.pinned)
                .on_click(Self::on_project(cx, &dir, |d, f, _, cx| {
                    d.toggle_project_pin(f, cx);
                })),
        ));
        rows.push(setting(
            faces,
            "Pull its default branch",
            "As Ghost starts and each time it fetches, while it is checked out with nothing changed, and only where it moves straight on.",
            Switch::new("project-pull")
                .small()
                .checked(prefs.pull)
                .on_click(Self::on_project(cx, &dir, |d, f, _, cx| {
                    d.set_prefs(f, cx, |p| p.pull = !p.pull);
                })),
        ));
        rows.push(setting(
            faces,
            "Archived",
            "Hides the project from the sidebar and project pickers. Sessions and files are kept.",
            Switch::new("project-archived")
                .small()
                .checked(prefs.archived)
                .on_click(Self::on_project(cx, &dir, |d, f, _, cx| {
                    if d.prefs(f).archived {
                        d.restore_project(f, cx);
                    } else {
                        d.archive_project(f, cx);
                    }
                })),
        ));
        let danger = setting(
            faces,
            "Remove project",
            "Forgets the project and its sessions here. Files on disk are not touched.",
            Button::new("remove-project")
                .danger()
                .small()
                .label("Remove…")
                .on_click(Self::on_project(cx, &dir, Desk::ask_remove_project)),
        );
        page(faces, &self.project_name(folder), &Desk::shown(folder))
            .child(
                div().flex().child(
                    Button::new("all-projects")
                        .ghost()
                        .small()
                        .icon(Icon::empty().path("icons/chevron-left.svg"))
                        .label("Projects")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pages.project = None;
                            cx.notify();
                        })),
                ),
            )
            .child(group_of(rows))
            .children(group(faces, "Checkouts", self.checkouts(faces, folder, cx)))
            .children(group(faces, "Danger zone", vec![Some(danger)]))
    }

    /// Where the project is checked out: its folder, and each of Ghost's
    /// worktrees of it, with the sessions in it, each one removable.
    fn checkouts(&self, faces: &Faces, folder: &Path, cx: &mut Context<Self>) -> Vec<Option<Div>> {
        let mut worktrees: Vec<(crate::git::Worktree, u64, usize)> = Vec::new();
        for s in self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.project == folder)
        {
            let Some(wt) = &s.meta.worktree else { continue };
            match worktrees.iter_mut().find(|(w, ..)| w.path == wt.path) {
                Some((.., n)) => *n += 1,
                None => worktrees.push((wt.clone(), s.key, 1)),
            }
        }
        let mut rows = vec![setting(
            faces,
            &Desk::shown(folder),
            "The project's own folder.",
            div(),
        )];
        for (n, (wt, key, sessions)) in worktrees.into_iter().enumerate() {
            let about = format!(
                "{} · {sessions} session{}",
                wt.branch,
                if sessions == 1 { "" } else { "s" }
            );
            rows.push(setting(
                faces,
                &Desk::shown(&wt.path),
                &about,
                Button::new(SharedString::from(format!("remove-checkout-{n}")))
                    .ghost()
                    .small()
                    .label("Remove")
                    .tooltip("Its branch is kept; one with changes not committed is refused")
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.remove_checkout(wt.clone(), key);
                    })),
            ));
        }
        rows.into_iter().map(Some).collect()
    }

    /// The project's icon, and the menu that changes it; its mark's
    /// letters typed in place.
    fn icon_control(
        &self,
        folder: &Path,
        icon: Option<crate::settings::Icon>,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::desk::projects::MARKS;
        let marking = matches!(&self.renaming, Some(Renaming::Mark(f)) if f == folder);
        let (desk, dir) = (cx.entity(), folder.to_path_buf());
        let menu = Button::new("project-icon")
            .outline()
            .small()
            .label("Change")
            .dropdown_menu(move |mut menu, _, _| {
                let on = |f: fn(&mut Desk, &Path, &mut Window, &mut Context<Desk>)| {
                    let (desk, dir) = (desk.clone(), dir.clone());
                    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                        desk.update(cx, |d, cx| f(d, &dir, window, cx));
                    }
                };
                menu = menu
                    .item(PopupMenuItem::new("Automatic").on_click(on(|d, f, _, cx| {
                        d.set_prefs(f, cx, |p| p.icon = None);
                    })))
                    .item(
                        PopupMenuItem::new("Choose a picture…")
                            .on_click(on(|_, f, _, cx| Desk::choose_icon(f, cx))),
                    )
                    .item(
                        PopupMenuItem::new("Letters or an emoji…").on_click(on(|d, f, w, cx| {
                            d.begin_rename(Renaming::Mark(f.to_path_buf()), w, cx);
                        })),
                    )
                    .separator();
                for (n, (name, _)) in MARKS.iter().enumerate() {
                    let (desk, dir) = (desk.clone(), dir.clone());
                    menu = menu.item(PopupMenuItem::new(*name).on_click(move |_, _, cx| {
                        desk.update(cx, |d, cx| d.mark_colour(&dir, n as u8, cx));
                    }));
                }
                menu
            });
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(crate::desk::projects::project_icon(folder, icon, 20.))
            .when(marking, |d| {
                d.child(div().w(px(80.)).child(Input::new(&self.rename).small()))
            })
            .child(menu)
    }

    /// A listener doing `f` to the project `dir`, whatever its event.
    fn on_project<E: 'static>(
        cx: &mut Context<Desk>,
        dir: &Path,
        f: fn(&mut Desk, &Path, &mut Window, &mut Context<Desk>),
    ) -> impl Fn(&E, &mut Window, &mut gpui::App) + 'static {
        let dir = dir.to_path_buf();
        cx.listener(move |this, _: &E, window, cx| f(this, &dir, window, cx))
    }

    /// Settings → Projects, at the page of `folder`.
    pub fn project_settings(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.project = Some(folder.to_path_buf());
        self.open_tab(super::Tab::Projects, window, cx);
    }

    /// Ask before removing the project `folder`, in t3code's words: its
    /// sessions are forgotten here, the agents keep theirs.
    pub(crate) fn ask_remove_project(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::component::WindowExt as _;
        let n = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.project == folder && s.parent.is_none())
            .count();
        let title = format!(
            "Remove project \u{201c}{}\u{201d} and forget its {n} session{}?",
            self.project_name(folder),
            if n == 1 { "" } else { "s" }
        );
        let said = format!(
            "Path: {}\nGhost forgets these sessions and their records. Each agent keeps its own sessions.\nFiles on disk are not touched.\nThis cannot be undone.",
            Desk::shown(folder)
        );
        let (desk, folder) = (cx.entity(), folder.to_path_buf());
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (desk, folder) = (desk.clone(), folder.clone());
            alert
                .title(title.clone())
                .description(said.clone())
                .ok_text("Remove")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    desk.update(cx, |d, cx| d.remove_project(&folder, cx));
                    true
                })
        });
    }

    /// Take the project `folder` off the list, and forget its sessions
    /// and their records; their worktrees and the files are left alone.
    fn remove_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let keys: Vec<u64> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.project == folder && s.parent.is_none())
            .map(|s| s.key)
            .collect();
        for key in keys {
            self.keep_worktree.insert(key);
            self.forget_session(key, false, cx);
        }
        self.filter.retain(|f| f != folder);
        self.settings.forget(folder);
        self.settings.prefs.remove(folder);
        self.closed.remove(folder);
        self.pages.project = None::<PathBuf>;
        self.save_settings();
        self.save_index();
        cx.notify();
    }
}
