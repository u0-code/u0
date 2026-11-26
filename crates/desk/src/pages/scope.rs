//! Settings a project holds otherwise than everywhere else, as t3code's
//! project overrides: the line atop a page that says which project its
//! choices are set for, and a choice that is set there (`Settings::hold`)
//! or everywhere, with the way back to everywhere's.

use std::path::PathBuf;

use gpui::component::{Selectable as _, menu::PopupMenu, setting::SettingGroup};
use gpui::{Context, Div, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, whole};
use crate::{
    desk::{Desk, Popup},
    settings::Settings,
    ui::{self, Pick, Weight},
};

impl Desk {
    /// The line atop a page with settings a project may hold: which
    /// project its choices marked so are set for.
    pub(super) fn scope_group(w: &WeakEntity<Desk>) -> SettingGroup {
        group(
            "Applying to",
            vec![whole(
                w,
                &["project", "override", "scope"],
                |d, faces, cx| {
                    let shown = d.pages.scope.as_ref().map_or_else(
                        || "All projects".to_owned(),
                        |p| d.project_name(&crate::desk::Place::new(d.pages.on, p.clone())),
                    );
                    let trigger = ui::opener("settings-scope")
                        .selected(d.popup == Popup::Scope)
                        .child(ui::icon("folder", 14.))
                        .child(shown);
                    let menu = d.menu(
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
                            let all = pick("All projects".into(), None);
                            (d.edited().folders.iter())
                                .map(|f| {
                                    let p = crate::desk::Place::new(d.pages.on, f.clone());
                                    pick(d.project_name(&p), Some(f.clone()))
                                })
                                .fold(menu.min_w(px(260.)).item(all), PopupMenu::item)
                        },
                        cx,
                    );
                    div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(menu)
                    .child(ui::fine(
                        faces,
                        if d.pages.scope.is_some() {
                            "Choices that say “This project” are its own; the rest hold everywhere."
                        } else {
                            "Choose a project to give it choices of its own."
                        },
                    ))
                    .into_any_element()
                },
            )],
        )
    }

    /// A choice a project may hold: as `pick`, set for the project the
    /// page applies to (else everywhere), and, where that project holds
    /// its own, the way back to everywhere's.
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
                (label, value == now, pick)
            })
            .collect();
        let everywhere = get(self.edited());
        let marker = if held {
            ui::button(
                SharedString::from(format!("{id}-inherit")),
                "This project's own · Reset",
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
            ui::fine(crate::fonts::faces(cx), "This project · as everywhere").into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(2.))
            .child(ui::segmented(id, choices))
            .when(scope.is_some(), |d| d.child(marker))
    }
}
