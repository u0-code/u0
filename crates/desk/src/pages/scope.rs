//! Settings a project holds otherwise than everywhere else, as t3code's
//! project overrides: the banner atop a page that says which project its
//! choices are set for, and an `SRow` set there (`Settings::hold`) or
//! everywhere, with `Overridden` + `Set by … · Global value: … · Reset`.

use std::path::PathBuf;

use gpui::component::{Disableable as _, Selectable as _, menu::PopupMenu, setting::SettingGroup};
use gpui::{Context, Div, SharedString, WeakEntity, div, prelude::*, px};
use u0ui::tokens::ColorRole;

use super::prefs::{group, whole};
use super::srow::SRow;
use crate::{
    desk::{Desk, Place, Popup},
    fonts::Faces,
    settings::Settings,
    theme,
    ui::{self, Pick, Weight},
};

impl Desk {
    /// Legacy `SettingGroup` scope picker for unmigrated `SettingPage` tabs
    /// (`hosts.rs`).
    pub(super) fn scope_group(w: &WeakEntity<Desk>) -> SettingGroup {
        group(
            crate::tr("Applying to"),
            vec![whole(
                w,
                &["project", "override", "scope"],
                |d, faces, cx| d.scope_banner(faces, cx).into_any_element(),
            )],
        )
    }

    /// Legacy project-scoped control for unmigrated `SettingPage` tabs
    /// (`hosts.rs`).
    pub(super) fn own<T: Copy + PartialEq + 'static, const N: usize>(
        &self,
        id: &str,
        choices: [(impl Into<SharedString>, T); N],
        get: fn(&Settings) -> T,
        set: fn(&mut Settings, T),
        cx: &mut Context<Self>,
    ) -> Div {
        let scope = self.pages.scope.clone();
        let shown = (scope.as_ref()).map_or_else(|| self.edited().clone(), |p| self.edited().of(p));
        let now = get(&shown);
        let held = scope.is_some() && now != get(self.edited());
        let choices = choices
            .into_iter()
            .map(|(label, value)| {
                let scope = scope.clone();
                let pick: Pick = Box::new(cx.listener(move |this, _, _, cx| {
                    match &scope {
                        Some(p) => this.edited_mut().hold(p, |s| set(s, value)),
                        None => set(this.edited_mut(), value),
                    }
                    this.save_edited();
                    this.settings_took(cx);
                    cx.notify();
                }));
                (crate::tr(&label.into()), value == now, pick)
            })
            .collect();
        let everywhere = get(self.edited());
        let marker = if held {
            ui::button(
                SharedString::from(format!("{id}-inherit")),
                crate::tr("This project's own · Reset"),
                Weight::Quiet,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = this.pages.scope.clone() {
                    this.edited_mut().hold(&p, |s| set(s, everywhere));
                    this.save_edited();
                    this.settings_took(cx);
                }
                cx.notify();
            }))
            .into_any_element()
        } else {
            ui::fine(
                crate::fonts::faces(cx),
                crate::tr("This project · as everywhere"),
            )
            .into_any_element()
        };
        let readonly = self.settings_readonly(self.pages.on);
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(2.))
            .child(ui::segmented(id, choices).disabled(readonly))
            .when(scope.is_some(), |d| d.child(marker))
    }
    /// Carbon `layer-01` scope bar atop pages with project-overridable settings
    /// (`General`, `New threads`).
    pub(super) fn scope_banner(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let shown = self.pages.scope.as_ref().map_or_else(
            || crate::tr("All projects").to_string(),
            |p| self.project_name(&Place::new(self.pages.on, p.clone())),
        );
        let trigger = ui::opener("settings-scope")
            .selected(self.popup == Popup::Scope)
            .child(ui::icon("folder", 14.))
            .child(shown);
        let menu = self.menu(
            Popup::Scope,
            trigger,
            |d, menu, desk, _| {
                let pick = |label: String, to: Option<PathBuf>| {
                    let on = d.pages.scope == to;
                    Desk::menu_item(desk, label, move |this, _, cx| {
                        this.pages.scope.clone_from(&to);
                        cx.notify();
                    })
                    .checked(on)
                };
                let all = pick(crate::tr("All projects").to_string(), None);
                (d.edited().folders.iter())
                    .map(|f| {
                        let p = Place::new(d.pages.on, f.clone());
                        pick(d.project_name(&p), Some(f.clone()))
                    })
                    .fold(menu.min_w(px(260.)).item(all), PopupMenu::item)
            },
            cx,
        );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.))
            .px(px(20.))
            .py(px(12.))
            .bg(theme::role(ColorRole::Layer01))
            .child(
                ui::text(faces, crate::tr("Applying to"))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::text2()),
            )
            .child(menu)
            .child(ui::fine(
                faces,
                crate::tr(if self.pages.scope.is_some() {
                    "Choices that say “This project” are its own; the rest hold everywhere."
                } else {
                    "Choose a project to give it choices of its own."
                }),
            ))
    }

    /// Project-overridable Carbon `SRow`: reads `Settings::of(project)`, writes
    /// with `Settings::hold` when a project scope is active, and displays the
    /// `Overridden` pill + `Set by <project> · Global value: <val> · Reset`
    /// line when the project overrides the global value.
    #[allow(
        clippy::too_many_arguments,
        reason = "a project-scoped setting names its id, copy, choices, getter and setter"
    )]
    pub(super) fn own_srow<T: Copy + PartialEq + 'static, const N: usize>(
        &self,
        id: &'static str,
        title: &'static str,
        about: &'static str,
        choices: [(&'static str, T); N],
        get: fn(&Settings) -> T,
        set: fn(&mut Settings, T),
        cx: &mut Context<Self>,
    ) -> SRow {
        let scope = self.pages.scope.clone();
        let shown = (scope.as_ref()).map_or_else(|| self.edited().clone(), |p| self.edited().of(p));
        let now = get(&shown);
        let everywhere = get(self.edited());
        let default_val = get(&Settings::default());
        let held = scope.is_some() && now != everywhere;
        let over_by = scope
            .as_ref()
            .map(|p| SharedString::from(self.project_name(&Place::new(self.pages.on, p.clone()))));
        let inherited = choices
            .iter()
            .find(|(_, v)| *v == everywhere)
            .map(|(l, _)| crate::tr(l))
            .unwrap_or_default();

        let translated: Vec<(SharedString, T)> = choices
            .into_iter()
            .map(|(l, v)| (crate::tr(l), v))
            .collect();

        let desk_reset = cx.entity();
        let desk_mod = cx.entity();
        let desk_pick = cx.entity();
        let scope_pick = scope.clone();

        let row = self
            .srow(id, crate::tr(title), crate::tr(about))
            .project_override(held, over_by, inherited, move |_, cx| {
                desk_reset.update(cx, |this, cx| {
                    if let Some(p) = this.pages.scope.clone() {
                        this.edited_mut().hold(&p, |s| set(s, everywhere));
                        this.save_edited();
                        this.settings_took(cx);
                    }
                    cx.notify();
                });
            })
            .modified(scope.is_none() && now != default_val, move |_, cx| {
                desk_mod.update(cx, |this, cx| {
                    set(this.edited_mut(), default_val);
                    this.save_edited();
                    this.settings_took(cx);
                    cx.notify();
                });
            });

        let apply = move |picked: T, _: &mut gpui::Window, cx: &mut gpui::App| {
            desk_pick.update(cx, |this, cx| {
                match &scope_pick {
                    Some(p) => this.edited_mut().hold(p, |s| set(s, picked)),
                    None => set(this.edited_mut(), picked),
                }
                this.save_edited();
                this.settings_took(cx);
                cx.notify();
            });
        };

        let fits_segmented = N <= 3 && translated.iter().all(|(l, _)| l.chars().count() <= 12);
        if fits_segmented {
            row.segmented(translated, now, apply)
        } else {
            row.select(translated, &now, apply)
        }
    }
}
