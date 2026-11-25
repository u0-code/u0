//! Settings › Appearance › Themes: the theme for the dark look and the
//! one for the light, chosen from Ghost's own, those it brings and yours;
//! yours made, imported, changed and removed; and a picture under the
//! thread.

use gpui::component::{
    Sizable as _,
    button::Button,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui::{Context, Div, IntoElement, SharedString, div, prelude::*, px};

use super::prefs::{group, row};
use crate::{
    desk::Desk,
    fonts::Faces,
    themes::{self, Themes},
    ui::{self, Weight},
};

impl Desk {
    /// The themes group; none where the search `q` finds nothing in it.
    pub(super) fn themes_group(
        &self,
        faces: &Faces,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let yours: Vec<SharedString> = cx
            .try_global::<Themes>()
            .into_iter()
            .flat_map(|t| t.0.iter())
            .filter(|(_, file)| file.is_some())
            .map(|(t, _)| t.name.clone())
            .collect();
        let mut rows = vec![
            row(
                faces,
                q,
                "Dark theme",
                "What the dark look is drawn in: Ghost's own, one it brings, or one of yours.",
                self.theme_pick(true, cx),
            ),
            row(
                faces,
                q,
                "Light theme",
                "What the light look is drawn in.",
                self.theme_pick(false, cx),
            ),
            row(
                faces,
                q,
                "Make a theme",
                "The theme drawn now, copied into a file of yours and opened in your editor; read the themes again to see what you changed.",
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::button("theme-duplicate", "Duplicate", Weight::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| this.duplicate_theme(cx))),
                    )
                    .child(
                        ui::button("theme-import", "Import VS Code theme…", Weight::Secondary)
                            .on_click(
                                cx.listener(|_, _, window, cx| Desk::import_theme(window, cx)),
                            ),
                    )
                    .child(
                        ui::button("theme-read", "Read again", Weight::Quiet)
                            .on_click(cx.listener(|this, _, _, cx| this.read_themes(cx))),
                    ),
            ),
        ];
        let folder = themes::folder(&self.home);
        for (n, name) in yours.into_iter().enumerate() {
            let (path, gone) = (
                cx.try_global::<Themes>()
                    .and_then(|t| t.0.iter().find(|(c, _)| c.name == name)?.1.clone()),
                name.to_string(),
            );
            rows.push(row(
                faces,
                q,
                &name,
                &format!("Yours, in {}", Desk::shown(&folder)),
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::button(
                            SharedString::from(format!("theme-edit-{n}")),
                            "Edit",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            if let Some(p) = path.clone() {
                                this.edit(p, None);
                            }
                        })),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("theme-remove-{n}")),
                            "Remove",
                            Weight::Quiet,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_theme(&gone, cx))),
                    ),
            ));
        }
        let backdrop = self
            .settings
            .backdrop
            .as_ref()
            .map_or_else(|| "None".to_owned(), |p| Desk::shown(p));
        rows.push(row(
            faces,
            q,
            "Picture under the thread",
            &format!("Drawn faintly behind the thread and the composer: {backdrop}."),
            div()
                .flex()
                .gap(px(8.))
                .child(
                    ui::button("backdrop-choose", "Choose…", Weight::Secondary).on_click(
                        cx.listener(|_, _, window, cx| Desk::choose_backdrop(window, cx)),
                    ),
                )
                .when(self.settings.backdrop.is_some(), |d| {
                    d.child(ui::button("backdrop-none", "None", Weight::Quiet).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.settings.backdrop = None;
                            this.save_settings();
                            cx.notify();
                        }),
                    ))
                }),
        ));
        group(faces, "Themes", rows).map(|g| {
            g.children(
                self.pages
                    .themes_refused
                    .iter()
                    .map(|why| ui::trouble(format!("A theme did not read: {why}"))),
            )
        })
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
