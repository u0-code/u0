//! Settings › Appearance › Themes: the theme for the dark look and the
//! one for the light, chosen from Ghost's own, those it brings and yours;
//! yours made, imported, changed and removed; and a picture under the
//! thread.

use gpui::component::setting::SettingGroup;
use gpui::component::{
    Sizable as _,
    button::Button,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui::{App, Context, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::Desk,
    themes::{self, Themes},
    ui::{self, Weight},
};

impl Desk {
    /// The themes group.
    pub(super) fn themes_group(&self, w: &WeakEntity<Desk>, cx: &App) -> SettingGroup {
        let yours: Vec<(SharedString, Option<std::path::PathBuf>)> = cx
            .try_global::<Themes>()
            .into_iter()
            .flat_map(|t| t.0.iter())
            .filter(|(_, file)| file.is_some())
            .map(|(t, file)| (t.name.clone(), file.clone()))
            .collect();
        let mut rows = vec![
            item(
                w,
                crate::tr("Dark theme"),
                crate::tr(
                    "What the dark look is drawn in: Ghost's own, one it brings, or one of yours.",
                ),
                |d, _, cx| d.theme_pick(true, cx).into_any_element(),
            ),
            item(
                w,
                crate::tr("Light theme"),
                crate::tr("What the light look is drawn in."),
                |d, _, cx| d.theme_pick(false, cx).into_any_element(),
            ),
            item(
                w,
                crate::tr("Make a theme"),
                crate::tr(
                    "The theme drawn now, copied into a file of yours and opened in your editor; read the themes again to see what you changed.",
                ),
                |_, _, cx| {
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            ui::button(
                                "theme-duplicate",
                                crate::tr("Duplicate"),
                                Weight::Secondary,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.duplicate_theme(cx))),
                        )
                        .child(
                            ui::button(
                                "theme-import",
                                crate::tr("Import VS Code theme…"),
                                Weight::Secondary,
                            )
                            .on_click(
                                cx.listener(|_, _, window, cx| Desk::import_theme(window, cx)),
                            ),
                        )
                        .child(
                            ui::button("theme-read", crate::tr("Read again"), Weight::Secondary)
                                .on_click(cx.listener(|this, _, _, cx| this.read_themes(cx))),
                        )
                        .into_any_element()
                },
            ),
        ];
        let folder = themes::folder(&self.home);
        for (n, (name, path)) in yours.into_iter().enumerate() {
            let gone = name.to_string();
            rows.push(item(
                w,
                name,
                crate::tr_with("Yours, in %{folder}", &[("folder", &Desk::shown(&folder))]),
                move |_, _, cx| {
                    let (path, gone) = (path.clone(), gone.clone());
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            ui::button(
                                SharedString::from(format!("theme-edit-{n}")),
                                crate::tr("Edit"),
                                Weight::Secondary,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, _| {
                                    if let Some(p) = path.clone() {
                                        this.edit(p, None);
                                    }
                                },
                            )),
                        )
                        .child(
                            ui::button(
                                SharedString::from(format!("theme-remove-{n}")),
                                crate::tr("Remove"),
                                Weight::Quiet,
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_theme(&gone, cx)),
                            ),
                        )
                        .into_any_element()
                },
            ));
        }
        let backdrop = self
            .settings
            .backdrop
            .as_ref()
            .map_or_else(|| crate::tr("None").to_string(), |p| Desk::shown(p));
        rows.push(item(
            w,
            crate::tr("Picture under the thread"),
            crate::tr_with(
                "Drawn faintly behind the thread and the composer: %{backdrop}.",
                &[("backdrop", &backdrop)],
            ),
            |d, _, cx| {
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::button("backdrop-choose", crate::tr("Choose…"), Weight::Secondary)
                            .on_click(
                                cx.listener(|_, _, window, cx| Desk::choose_backdrop(window, cx)),
                            ),
                    )
                    .when(d.settings.backdrop.is_some(), |d| {
                        d.child(
                            ui::button("backdrop-none", crate::tr("None"), Weight::Quiet).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.settings.backdrop = None;
                                    this.save_settings();
                                    cx.notify();
                                }),
                            ),
                        )
                    })
                    .into_any_element()
            },
        ));
        // A theme that did not read says why, under them.
        for why in &self.pages.themes_refused {
            let why = crate::tr_with("A theme did not read: %{why}", &[("why", why)]);
            rows.push(whole(w, &[], move |_, _, _| {
                ui::trouble(why.clone()).into_any_element()
            }));
        }
        group(crate::tr("Themes"), rows)
    }

    /// The theme for the dark look (or the light), and a menu of those
    /// there are for it.
    fn theme_pick(&self, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = if dark {
            &self.settings.theme_dark
        } else {
            &self.settings.theme_light
        }
        .clone();
        let names: Vec<Option<SharedString>> = std::iter::once(None)
            .chain(
                cx.try_global::<Themes>()
                    .into_iter()
                    .flat_map(|t| t.0.iter())
                    .filter(|(t, _)| t.mode.is_dark() == dark)
                    .map(|(t, _)| Some(t.name.clone())),
            )
            .collect();
        let desk = cx.entity();
        let label = chosen.clone().unwrap_or_else(|| "Ghost".into());
        Button::new(if dark { "theme-dark" } else { "theme-light" })
            .outline()
            .small()
            .label(label)
            .dropdown_menu(move |mut menu, _, _| {
                for name in &names {
                    let (desk, name) = (desk.clone(), name.clone());
                    let on = chosen.as_deref() == name.as_deref();
                    menu = menu.item(
                        PopupMenuItem::new(name.clone().unwrap_or_else(|| "Ghost".into()))
                            .checked(on)
                            .on_click(move |_, _, cx| {
                                let name = name.as_ref().map(ToString::to_string);
                                desk.update(cx, |d, cx| d.choose_theme(dark, name, cx));
                            }),
                    );
                }
                menu
            })
    }
}
