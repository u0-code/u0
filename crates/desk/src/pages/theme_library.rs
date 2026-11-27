//! Settings › Appearance › Themes (board `SetAppearance`): the themes as
//! u0ui's `ThemeCard`s, three to a row, chosen with a click and customized
//! from a card; a search over them and over Open VSX; the writer asked
//! for one; VS Code and gpui-kit themes imported, a new one made.

use gpui::component::{Sizable as _, input::Input};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, div, prelude::FluentBuilder as _, px,
};
use u0ui::{
    components::button::{Button as CarbonButton, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::workspace::ThemeCard,
    tokens::{CarbonTheme, ColorRole},
};

use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    settings::Look,
    theme,
    themes::{self, Themes},
    ui,
};

struct LibraryCard {
    key: String,
    title: SharedString,
    mode_label: SharedString,
    author: SharedString,
    dark: bool,
    colors: [Hsla; 5],
    selected: bool,
    source: CardSource,
}

#[derive(Clone)]
enum CardSource {
    BuiltIn(CarbonTheme),
    Custom(String),
}

impl Desk {
    fn active_theme_title(&self, dark: bool) -> String {
        let chosen = if dark {
            &self.settings.theme_dark
        } else {
            &self.settings.theme_light
        };
        match (chosen, theme::carbon_for(dark, chosen.as_deref())) {
            (_, Some(c)) => crate::tr(theme::carbon_label(c)).to_string(),
            (Some(custom), None) => custom.clone(),
            (None, None) => crate::tr(theme::carbon_label(if dark {
                CarbonTheme::G100
            } else {
                CarbonTheme::White
            }))
            .to_string(),
        }
    }

    fn library_cards(&self, cx: &App) -> Vec<LibraryCard> {
        let dark_active = self.settings.theme_dark.as_deref();
        let light_active = self.settings.theme_light.as_deref();
        let active_dark_carbon = theme::carbon_for(true, dark_active);
        let active_light_carbon = theme::carbon_for(false, light_active);

        let mut cards = Vec::new();
        for carbon in [
            CarbonTheme::White,
            CarbonTheme::G10,
            CarbonTheme::G90,
            CarbonTheme::G100,
        ] {
            let dark = carbon.is_dark();
            let selected = match self.settings.look {
                Look::Dark => dark && active_dark_carbon == Some(carbon),
                Look::Light => !dark && active_light_carbon == Some(carbon),
                Look::System => {
                    if dark {
                        active_dark_carbon == Some(carbon)
                    } else {
                        active_light_carbon == Some(carbon)
                    }
                }
            };
            cards.push(LibraryCard {
                key: format!("builtin-{}", carbon.id()),
                title: crate::tr(theme::carbon_label(carbon)),
                mode_label: crate::tr(if dark { "Dark" } else { "Light" }),
                author: crate::tr("Built in"),
                dark,
                colors: themes::preview_colors_of_carbon(carbon),
                selected,
                source: CardSource::BuiltIn(carbon),
            });
        }

        if let Some(customs) = cx.try_global::<Themes>() {
            for (config, _) in &customs.0 {
                let dark = config.mode.is_dark();
                let name_str = config.name.to_string();
                let selected = match self.settings.look {
                    Look::Dark => {
                        dark && active_dark_carbon.is_none() && dark_active == Some(&name_str)
                    }
                    Look::Light => {
                        !dark && active_light_carbon.is_none() && light_active == Some(&name_str)
                    }
                    Look::System => {
                        if dark {
                            active_dark_carbon.is_none() && dark_active == Some(&name_str)
                        } else {
                            active_light_carbon.is_none() && light_active == Some(&name_str)
                        }
                    }
                };
                cards.push(LibraryCard {
                    key: format!("custom-{}", crate::git::slug(&name_str)),
                    title: config.name.clone(),
                    mode_label: crate::tr(if dark { "Dark" } else { "Light" }),
                    author: crate::tr("You"),
                    dark,
                    colors: themes::preview_colors_of_custom(config),
                    selected,
                    source: CardSource::Custom(name_str),
                });
            }
        }
        cards
    }

    pub(super) fn themes_library_section(
        &self,
        settings_query: &str,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let all_cards = self.library_cards(cx);
        if !settings_query.is_empty() {
            let q = settings_query.to_lowercase();
            let hit = "themes".contains(&q)
                || "vs code".contains(&q)
                || "vscode".contains(&q)
                || "carbon".contains(&q)
                || all_cards
                    .iter()
                    .any(|c| c.title.to_lowercase().contains(&q));
            if !hit {
                return None;
            }
        }

        let theme_query = self
            .pages
            .theme_search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let filtered: Vec<LibraryCard> = all_cards
            .into_iter()
            .filter(|c| {
                theme_query.is_empty()
                    || c.title.to_lowercase().contains(&theme_query)
                    || c.author.to_lowercase().contains(&theme_query)
                    || c.mode_label.to_lowercase().contains(&theme_query)
            })
            .collect();

        let total = filtered.len();
        let limit_to_six = theme_query.is_empty() && !self.pages.show_all_themes && total > 6;
        let visible: Vec<LibraryCard> = if limit_to_six {
            filtered.into_iter().take(6).collect()
        } else {
            filtered
        };
        let shown_count = visible.len();

        let now_dark = !theme::is_light();
        let active_name = self.active_theme_title(now_dark);
        let subtitle = if self.settings.look == Look::System {
            let other_name = self.active_theme_title(!now_dark);
            let other_mode = crate::tr(if now_dark { "light" } else { "dark" });
            crate::tr_with(
                "Built-in, imported from VS Code or Open VSX, or your own. %{active} is in use now; %{other} takes over when the system turns %{mode}.",
                &[
                    ("active", &active_name),
                    ("other", &other_name),
                    ("mode", &other_mode),
                ],
            )
        } else {
            crate::tr_with(
                "Built-in, imported from VS Code or Open VSX, or your own. %{active} is in use now.",
                &[("active", &active_name)],
            )
        };

        let desk_ask = cx.entity();
        let desk_vscode = cx.entity();
        let desk_kit = cx.entity();
        let desk_new = cx.entity();
        // The search keeps room to read what is typed; the buttons go
        // under it where a language's words leave too little.
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(220.))
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .px(px(10.))
                    .bg(theme::role(ColorRole::Field01))
                    .border_b_1()
                    .border_color(theme::role(ColorRole::BorderStrong01))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Search)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconSecondary))
                            .flex_none(),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&self.pages.theme_search)
                                .appearance(false)
                                .small()
                                .cleanable(true),
                        ),
                    ),
            )
            .child(
                CarbonButton::new("theme-ask", crate::tr("Ask an agent"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        desk_ask.update(cx, |this, cx| this.toggle_theme_ask(window, cx));
                    }),
            )
            .child(
                CarbonButton::new("theme-import-vscode", crate::tr("Import a VS Code theme…"))
                    .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        desk_vscode.update(cx, |_, cx| Desk::import_theme(window, cx));
                    }),
            )
            .child(
                CarbonButton::new(
                    "theme-import-kit",
                    crate::tr("Import a gpui-kit theme JSON…"),
                )
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| {
                    desk_kit.update(cx, |_, cx| Desk::import_kit_theme(window, cx));
                }),
            )
            .child(
                CarbonButton::new("theme-new", crate::tr("New theme"))
                    .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Add)
                    .on_click(move |_, _, cx| {
                        desk_new.update(cx, Desk::new_custom_theme);
                    }),
            );

        let mut grid_rows: Vec<Div> = Vec::new();
        let mut iter = visible.into_iter();
        loop {
            let mut row_items: Vec<AnyElement> = Vec::with_capacity(3);
            for _ in 0..3 {
                if let Some(card) = iter.next() {
                    row_items.push(Self::theme_card_cell(card, cx));
                }
            }
            if row_items.is_empty() {
                break;
            }
            while row_items.len() < 3 {
                row_items.push(div().w(px(208.)).flex_none().into_any_element());
            }
            grid_rows.push(
                div()
                    .flex()
                    .w_full()
                    .justify_between()
                    .gap(px(16.))
                    .children(row_items),
            );
        }

        let refused: Vec<AnyElement> = self
            .pages
            .themes_refused
            .iter()
            .map(|why| {
                ui::trouble(crate::tr_with(
                    "A theme did not read: %{why}",
                    &[("why", why)],
                ))
                .into_any_element()
            })
            .collect();

        let footer = (theme_query.is_empty() && total > 6).then(|| {
            let expanded = self.pages.show_all_themes;
            let desk_toggle = cx.entity();
            let toggle_label = if expanded {
                crate::tr("Show fewer")
            } else {
                crate::tr_with("Show all %{total}", &[("total", &total)])
            };
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(crate::style::CAPTION))
                .line_height(px(16.))
                .text_color(theme::text2())
                .child(crate::tr_with(
                    "Showing %{shown} of %{total} themes.",
                    &[("shown", &shown_count), ("total", &total)],
                ))
                .child(
                    CarbonButton::new("themes-show-all", toggle_label)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk_toggle.update(cx, |this, cx| {
                                this.pages.show_all_themes = !this.pages.show_all_themes;
                                cx.notify();
                            });
                        }),
                )
        });

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            ui::text(faces, crate::tr("Themes"))
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text()),
                        )
                        .child(
                            ui::text(faces, subtitle)
                                .text_size(px(crate::style::BODY))
                                .line_height(px(20.))
                                .text_color(theme::text2()),
                        ),
                )
                .child(toolbar)
                .children(self.theme_ask_row(faces, cx))
                .child(div().flex().flex_col().gap(px(16.)).children(grid_rows))
                .children(self.vsx_found(faces, &theme_query, cx))
                .children(footer)
                .children(refused)
                .into_any_element(),
        )
    }

    /// Under the tools: the line you describe a theme for the writer on,
    /// or, while it writes one, that it does.
    fn theme_ask_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        if self.pages.theme_asked {
            return Some(
                div().child(
                    ui::text(faces, crate::tr("The writer is writing a theme…"))
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::text2()),
                ),
            );
        }
        let input = self.pages.theme_ask.as_ref()?;
        let (desk_ask, desk_cancel) = (cx.entity(), cx.entity());
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(Input::new(input).small()),
                )
                .child(
                    CarbonButton::new("theme-ask-send", crate::tr("Ask"))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk_ask.update(cx, |this, cx| this.ask_for_theme(window, cx));
                        }),
                )
                .child(
                    CarbonButton::new("theme-ask-cancel", crate::tr("Cancel"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk_cancel.update(cx, |this, cx| this.toggle_theme_ask(window, cx));
                        }),
                ),
        )
    }

    /// The theme extensions Open VSX has for the search, each to add.
    fn vsx_found(&self, faces: &Faces, query: &str, cx: &mut Context<Self>) -> Option<Div> {
        let (asked, found) = self.pages.vsx.as_ref()?;
        if asked.to_lowercase() != query {
            return None;
        }
        let head = ui::text(faces, crate::tr("On Open VSX"))
            .text_size(px(crate::style::BODY))
            .weight(FontWeight::SEMIBOLD)
            .text_color(theme::text());
        let said = |text: SharedString| {
            ui::text(faces, text)
                .text_size(px(crate::style::CAPTION))
                .text_color(theme::text2())
        };
        let rows: Vec<AnyElement> = match found {
            Err(why) => vec![
                said(crate::tr_with(
                    "Open VSX could not be searched: %{why}",
                    &[("why", why)],
                ))
                .into_any_element(),
            ],
            Ok(list) if list.is_empty() => vec![
                said(crate::tr_with(
                    "Nothing on Open VSX for “%{query}”",
                    &[("query", asked)],
                ))
                .into_any_element(),
            ],
            Ok(list) => list
                .iter()
                .enumerate()
                .map(|(n, listed)| {
                    let adding = self.pages.vsx_adding.as_deref() == Some(listed.title.as_str());
                    let (desk, listed_add) = (cx.entity(), listed.clone());
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .min_h(px(40.))
                        .border_b_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(ui::text(faces, listed.title.clone()))
                                .child(said(crate::tr_with("by %{who}", &[("who", &listed.by)]))),
                        )
                        .child(
                            CarbonButton::new(
                                SharedString::from(format!("vsx-add-{n}")),
                                crate::tr(if adding { "Adding…" } else { "Add" }),
                            )
                            .variant(ButtonVariant::Tertiary)
                            .size(ButtonSize::Sm)
                            .disabled(self.pages.vsx_adding.is_some())
                            .on_click(move |_, _, cx| {
                                let listed = listed_add.clone();
                                desk.update(cx, |this, cx| this.add_vsx(listed, cx));
                            }),
                        )
                        .into_any_element()
                })
                .collect(),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(head)
                .children(rows),
        )
    }

    fn theme_card_cell(card: LibraryCard, cx: &mut Context<Self>) -> AnyElement {
        let group_name = SharedString::from(format!("tcard-grp-{}", card.key));
        let card_id = SharedString::from(format!("tcard-{}", card.key));
        let edit_id = SharedString::from(format!("tcard-edit-{}", card.key));
        let is_custom = matches!(card.source, CardSource::Custom(_));
        let pick_source = card.source.clone();
        let edit_source = card.source;
        let dark = card.dark;
        let desk_pick = cx.entity();
        let desk_edit = cx.entity();

        let theme_card = ThemeCard::new(card_id, card.title)
            .colors(card.colors)
            .mode(card.mode_label)
            .author(card.author)
            .selected(card.selected)
            .on_click(move |_, cx| {
                let source = pick_source.clone();
                desk_pick.update(cx, |this, cx| {
                    let chosen = match source {
                        CardSource::BuiltIn(c) => {
                            let defaults = if dark {
                                theme::DARK_CARBON[0]
                            } else {
                                theme::LIGHT_CARBON[0]
                            };
                            (c != defaults).then(|| c.id().to_owned())
                        }
                        CardSource::Custom(name) => Some(name),
                    };
                    if this.settings.look != Look::System {
                        this.settings.look = if dark { Look::Dark } else { Look::Light };
                    }
                    this.choose_theme(dark, chosen, cx);
                });
            });

        div()
            .group(group_name.clone())
            .relative()
            .flex_none()
            .child(theme_card)
            .child(
                div()
                    .absolute()
                    .right(px(8.))
                    .bottom(px(8.))
                    .occlude()
                    .when(!is_custom, |d| {
                        d.invisible().group_hover(group_name, gpui::Styled::visible)
                    })
                    .child(
                        CarbonButton::new(edit_id, crate::tr("Customize"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                let source = edit_source.clone();
                                desk_edit.update(cx, |this, cx| match source {
                                    CardSource::BuiltIn(c) => {
                                        this.customize_carbon_theme(c, cx);
                                    }
                                    CardSource::Custom(name) => {
                                        this.open_theme_editor(&name, cx);
                                    }
                                });
                            }),
                    ),
            )
            .into_any_element()
    }
}
