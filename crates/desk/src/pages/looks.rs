//! `Settings › Appearance` (`SetAppearance.dc.html`): Carbon `Mode`, `Fonts`,
//! `Layout`, and `Motion and effects` sections, the `Themes` 3-column
//! `ThemeCard` library, and the `600px` right-side `Customize <Theme>` drawer
//! inside the settings dialog.

use gpui::component::{Sizable as _, input::Input};
use gpui::{
    AnyElement, App, BoxShadow, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    div, hsla, point, prelude::FluentBuilder as _, px, rgb, white,
};
use u0ui::{
    components::{
        button::{Button as CarbonButton, ButtonSize, ButtonVariant},
        dropdown_menu::{DropdownMenu, DropdownMenuKind, DropdownMenuSize},
        menu::MenuItem,
    },
    density::{self, Density},
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::workspace::ThemeCard,
    tokens::{CarbonTheme, ColorRole},
    u0::{
        primitives::ColorPicker as InlineColorPicker,
        settings::{MIN_CONTRAST, contrast_ratio},
    },
};

use super::{
    Tab,
    prefs::managed_info_banner,
    srow::{SGroup, SRow, carbon_page_with_tail},
};
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    settings::{CodeFont, Look, Motion, SCALES, Settings, Shell, Sort, TermSize, Width},
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
    /// Carbon `Appearance` page (`SetAppearance.dc.html`).
    pub(super) fn appearance_tab(
        &self,
        query: &str,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let defaults = Settings::default();
        let any_managed = [
            "look",
            "theme_light",
            "theme_dark",
            "contrast",
            "scale",
            "mono",
            "terminal_size",
            "shell",
            "diff_wrap",
            "width",
            "sort",
            "blue_orange",
            "side_by_side",
            "diff_folded",
            "no_spacing",
            "twelve",
            "math",
            "glass",
            "backdrop",
        ]
        .into_iter()
        .any(|k| crate::policy::locked(k).is_some());

        let banner = any_managed.then(|| managed_info_banner(faces).into_any_element());
        let groups = vec![
            self.mode_sgroup(&defaults, cx),
            self.fonts_sgroup(&defaults, cx),
            self.layout_sgroup(&defaults, cx),
            self.motion_sgroup(&defaults, cx),
        ];
        let tail = self.themes_library_section(query, faces, cx);

        carbon_page_with_tail(
            "settings-appearance",
            Tab::Appearance.title(),
            crate::tr("Mode, fonts, density and themes for this app."),
            banner,
            groups,
            tail,
            query,
            faces,
        )
    }

    fn mode_sgroup(&self, defaults: &Settings, cx: &mut Context<Self>) -> SGroup {
        let current_look = self.settings.look;
        let look_def = defaults.look;
        let desk_look = cx.entity();
        let desk_look_reset = cx.entity();
        let look_row = self
            .srow("look", crate::tr("Color scheme"), SharedString::default())
            .modified(current_look != look_def, move |window, cx| {
                desk_look_reset.update(cx, |this, cx| {
                    this.settings.look = look_def;
                    this.save_settings();
                    this.sync_appearance(window, cx);
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .segmented(
                [
                    (crate::tr("System"), Look::System),
                    (crate::tr("Light"), Look::Light),
                    (crate::tr("Dark"), Look::Dark),
                ],
                current_look,
                move |picked, window, cx| {
                    desk_look.update(cx, |this, cx| {
                        this.settings.look = picked;
                        this.save_settings();
                        this.sync_appearance(window, cx);
                        this.settings_took(cx);
                        cx.notify();
                    });
                },
            );

        SGroup::new(
            crate::tr("Mode"),
            vec![
                look_row,
                self.theme_select_srow(false, cx),
                self.theme_select_srow(true, cx),
                self.global_segmented_srow(
                    "contrast",
                    "Contrast",
                    "Stronger borders and text for low-vision use.",
                    [("Standard", false), ("More", true)],
                    self.settings.contrast,
                    defaults.contrast,
                    |s, v| s.contrast = v,
                    false,
                    cx,
                ),
            ],
        )
        .description(crate::tr(
            "Light, dark or follow the system, and the theme each one uses.",
        ))
    }

    fn theme_select_srow(&self, dark: bool, cx: &mut Context<Self>) -> SRow {
        let id = if dark { "theme_dark" } else { "theme_light" };
        let title = if dark {
            crate::tr("Dark theme")
        } else {
            crate::tr("Light theme")
        };
        let builtins = if dark {
            theme::DARK_CARBON
        } else {
            theme::LIGHT_CARBON
        };
        let raw_chosen = if dark {
            &self.settings.theme_dark
        } else {
            &self.settings.theme_light
        };
        let normalized: Option<String> =
            match (raw_chosen, theme::carbon_for(dark, raw_chosen.as_deref())) {
                (_, Some(c)) if c == builtins[0] => None,
                (_, Some(c)) => Some(c.id().to_owned()),
                (Some(custom), None) => Some(custom.clone()),
                (None, None) => None,
            };

        let mut choices: Vec<(SharedString, Option<String>)> = builtins
            .into_iter()
            .enumerate()
            .map(|(ix, c)| {
                let val = (ix > 0).then(|| c.id().to_owned());
                (crate::tr(theme::carbon_label(c)), val)
            })
            .collect();
        choices.extend(
            cx.try_global::<Themes>()
                .into_iter()
                .flat_map(|t| t.0.iter())
                .filter(|(t, _)| t.mode.is_dark() == dark)
                .map(|(t, _)| (t.name.clone(), Some(t.name.to_string()))),
        );
        if let Some(missing) = &normalized
            && !choices.iter().any(|(_, v)| v.as_ref() == Some(missing))
        {
            choices.push((SharedString::from(missing.clone()), Some(missing.clone())));
        }

        let desk_pick = cx.entity();
        let desk_reset = cx.entity();
        self.srow(id, title, SharedString::default())
            .modified(raw_chosen.is_some(), move |_, cx| {
                desk_reset.update(cx, |this, cx| this.choose_theme(dark, None, cx));
            })
            .select(choices, &normalized, move |picked, _, cx| {
                desk_pick.update(cx, |this, cx| this.choose_theme(dark, picked, cx));
            })
    }

    fn fonts_sgroup(&self, defaults: &Settings, cx: &mut Context<Self>) -> SGroup {
        let current_scale = self.settings.scale;
        let scale_def = defaults.scale;
        let scale_choices: Vec<(SharedString, u16)> = SCALES
            .into_iter()
            .map(|s| (SharedString::from(format!("{s}%")), s))
            .collect();
        let desk_scale = cx.entity();
        let desk_scale_reset = cx.entity();
        let scale_row = self
            .srow(
                "scale",
                crate::tr("Text size"),
                crate::tr("Everything in the window, larger or smaller: ⌘+, ⌘− and ⌘0."),
            )
            .modified(current_scale != scale_def, move |_, cx| {
                desk_scale_reset.update(cx, |this, cx| {
                    this.settings.scale = scale_def;
                    this.save_settings();
                    cx.notify();
                });
            })
            .select(scale_choices, &current_scale, move |picked, _, cx| {
                desk_scale.update(cx, |this, cx| {
                    this.settings.scale = picked;
                    this.save_settings();
                    cx.notify();
                });
            });

        let current_wrap = self.settings.diff_wrap;
        let wrap_def = defaults.diff_wrap;
        let desk_wrap = cx.entity();
        let desk_wrap_reset = cx.entity();
        let wrap_row = self
            .srow(
                "diff_wrap",
                crate::tr("Word wrap"),
                crate::tr("Wrap long lines in code blocks and diffs."),
            )
            .modified(current_wrap != wrap_def, move |_, cx| {
                desk_wrap_reset.update(cx, |this, cx| {
                    this.settings.diff_wrap = wrap_def;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .switch(current_wrap, move |on, _, cx| {
                desk_wrap.update(cx, |this, cx| {
                    this.settings.diff_wrap = on;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            });

        SGroup::new(
            crate::tr("Fonts"),
            vec![
                scale_row,
                self.global_select_srow(
                    "mono",
                    "Code font",
                    "What code, commands and numbers are set in.",
                    [
                        ("IBM Plex Mono", CodeFont::PlexMono),
                        ("JetBrains Mono", CodeFont::JetBrains),
                        ("SF Mono", CodeFont::SfMono),
                        ("Menlo", CodeFont::Menlo),
                    ],
                    self.settings.mono,
                    defaults.mono,
                    |s, v| s.mono = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "terminal_size",
                    "Terminal text",
                    "How large your terminals' text is, in the code font.",
                    [
                        ("Small", TermSize::Small),
                        ("Medium", TermSize::Medium),
                        ("Large", TermSize::Large),
                    ],
                    self.settings.terminal_size,
                    defaults.terminal_size,
                    |s, v| s.terminal_size = v,
                    false,
                    cx,
                ),
                self.global_select_srow(
                    "shell",
                    "Terminal shell",
                    "What a new terminal runs: your login shell, or one named.",
                    [
                        ("Login shell", Shell::Login),
                        ("zsh", Shell::Zsh),
                        ("bash", Shell::Bash),
                        ("fish", Shell::Fish),
                    ],
                    self.settings.shell,
                    defaults.shell,
                    |s, v| s.shell = v,
                    false,
                    cx,
                ),
                wrap_row,
            ],
        )
        .description(crate::tr(
            "Typefaces and sizes for the interface, chat and code.",
        ))
    }

    fn layout_sgroup(&self, defaults: &Settings, cx: &mut Context<Self>) -> SGroup {
        let current_density = density::current();
        let desk_density_reset = cx.entity();
        let desk_density_pick = cx.entity();
        let density_row = SRow::new("density", crate::tr("Density"))
            .description(crate::tr(
                "Row height and spacing in lists, panels and settings.",
            ))
            .unreadable(self.edited_unreadable())
            .modified(current_density != Density::Compact, move |_, cx| {
                density::set_density(cx, Density::Compact);
                desk_density_reset.update(cx, |_, cx| cx.notify());
            })
            .segmented(
                [
                    (crate::tr("Compact"), Density::Compact),
                    (crate::tr("Comfortable"), Density::Comfortable),
                ],
                current_density,
                move |picked, _, cx| {
                    density::set_density(cx, picked);
                    desk_density_pick.update(cx, |_, cx| cx.notify());
                },
            );

        let current_math = self.settings.math;
        let math_def = defaults.math;
        let desk_math = cx.entity();
        let desk_math_reset = cx.entity();
        let math_row = self
            .srow(
                "math",
                crate::tr("Render math"),
                crate::tr("Show LaTeX in replies as formulas."),
            )
            .modified(current_math != math_def, move |_, cx| {
                desk_math_reset.update(cx, |this, cx| {
                    this.settings.math = math_def;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .switch(current_math, move |on, _, cx| {
                desk_math.update(cx, |this, cx| {
                    this.settings.math = on;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            });

        SGroup::new(
            crate::tr("Layout"),
            vec![
                density_row,
                self.global_select_srow(
                    "width",
                    "Chat width",
                    "Comfortable is 720px, Wide 880px; Full fills the thread column.",
                    [
                        ("Comfortable", Width::Comfortable),
                        ("Wide", Width::Wide),
                        ("Full", Width::Full),
                    ],
                    self.settings.width,
                    defaults.width,
                    |s, v| s.width = v,
                    false,
                    cx,
                ),
                self.global_select_srow(
                    "sort",
                    "The sidebar sorts by",
                    "A project's threads, and the projects: your last message, anything happening, when each began, or name. Pinned ones stay on top.",
                    [
                        ("Your message", Sort::Used),
                        ("Activity", Sort::Updated),
                        ("Begun", Sort::Begun),
                        ("Name", Sort::Name),
                    ],
                    self.settings.sort,
                    defaults.sort,
                    |s, v| s.sort = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "blue_orange",
                    "What a change added and took away",
                    "Blue and orange tell them apart without green and red.",
                    [("Green and red", false), ("Blue and orange", true)],
                    self.settings.blue_orange,
                    defaults.blue_orange,
                    |s, v| s.blue_orange = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "side_by_side",
                    "A file's diff opens",
                    "Side by side widens the column beside the thread.",
                    [("Stacked", false), ("Side by side", true)],
                    self.settings.side_by_side,
                    defaults.side_by_side,
                    |s, v| s.side_by_side = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "diff_folded",
                    "A diff's files",
                    "Folded, each shows its name and counts until opened.",
                    [("Open", false), ("Folded", true)],
                    self.settings.diff_folded,
                    defaults.diff_folded,
                    |s, v| s.diff_folded = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "no_spacing",
                    "Changes only in spacing",
                    "Left out, a diff shows what changed in the words alone.",
                    [("Shown", false), ("Left out", true)],
                    self.settings.no_spacing,
                    defaults.no_spacing,
                    |s, v| s.no_spacing = v,
                    false,
                    cx,
                ),
                self.global_segmented_srow(
                    "twelve",
                    "Times",
                    "When a message was said, and what u0 says of times.",
                    [("24-hour", false), ("12-hour", true)],
                    self.settings.twelve,
                    defaults.twelve,
                    |s, v| s.twelve = v,
                    false,
                    cx,
                ),
                math_row,
            ],
        )
        .description(crate::tr("Density and where panels sit."))
    }

    fn motion_sgroup(&self, defaults: &Settings, cx: &mut Context<Self>) -> SGroup {
        let reduce_row = self.global_segmented_srow(
            "motion",
            "Reduce motion",
            "On turns off panel and thread animations.",
            [
                ("System", Motion::System),
                ("On", Motion::Reduce),
                ("Off", Motion::Full),
            ],
            self.settings.motion,
            defaults.motion,
            |s, v| s.motion = v,
            false,
            cx,
        );

        let current_glass = self.settings.glass;
        let glass_def = defaults.glass;
        let desk_glass = cx.entity();
        let desk_glass_reset = cx.entity();
        let glass_row = self
            .srow(
                "glass",
                crate::tr("Glass"),
                crate::tr("The window's grounds let the desktop show through, blurred."),
            )
            .modified(current_glass != glass_def, move |_, cx| {
                desk_glass_reset.update(cx, |this, cx| {
                    this.settings.glass = glass_def;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .switch(current_glass, move |on, _, cx| {
                desk_glass.update(cx, |this, cx| {
                    this.settings.glass = on;
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            });

        let backdrop_desc = match &self.settings.backdrop {
            None => crate::tr("None. Shown behind the thread at low opacity."),
            Some(path) => crate::tr_with(
                "Shown behind the thread: %{backdrop}.",
                &[("backdrop", &Desk::shown(path))],
            ),
        };
        let backdrop_row = self.srow("backdrop", crate::tr("Background image"), backdrop_desc);
        let backdrop_row = if self.settings.backdrop.is_none() {
            let desk_choose = cx.entity();
            backdrop_row.button(
                crate::tr("Choose image…"),
                ButtonVariant::Tertiary,
                move |window, cx| {
                    desk_choose.update(cx, |_, cx| Desk::choose_backdrop(window, cx));
                },
            )
        } else {
            let desk_clear = cx.entity();
            let desk_choose = cx.entity();
            backdrop_row.control(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        CarbonButton::new("backdrop-none", crate::tr("None"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_clear.update(cx, |this, cx| {
                                    this.settings.backdrop = None;
                                    this.save_settings();
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        CarbonButton::new("backdrop-choose", crate::tr("Choose image…"))
                            .variant(ButtonVariant::Tertiary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, window, cx| {
                                desk_choose.update(cx, |_, cx| Desk::choose_backdrop(window, cx));
                            }),
                    ),
            )
        };

        SGroup::new(
            crate::tr("Motion and effects"),
            vec![reduce_row, glass_row, backdrop_row],
        )
        .description(crate::tr("Animation, glass and the thread background."))
    }

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

    fn themes_library_section(
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
                    .on_click(move |_, window, cx| {
                        desk_new.update(cx, |this, cx| this.new_custom_theme(window, cx));
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
                            .on_click(move |_, window, cx| {
                                let source = edit_source.clone();
                                desk_edit.update(cx, |this, cx| match source {
                                    CardSource::BuiltIn(c) => {
                                        this.customize_carbon_theme(c, window, cx);
                                    }
                                    CardSource::Custom(name) => {
                                        this.open_theme_editor(&name, window, cx);
                                    }
                                });
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The `600px` right-side `Customize <Theme>` drawer + scrim overlaid
    /// inside `Desk::settings_dialog` (`SetAppearance.dc.html`).
    pub(super) fn theme_editor_drawer(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let name = self.pages.editing_theme.clone()?;
        let config = themes::find(cx, &name)?;
        let dark = config.mode.is_dark();
        let role_vals = themes::role_values(&config);
        let editing_role = self.pages.editing_role;
        let active_color = role_vals
            .iter()
            .find_map(|(r, c)| (*r == editing_role).then_some(*c))
            .unwrap_or_else(|| theme::role(editing_role));
        let active_hex = themes::to_hex(active_color);

        let title = crate::tr_with("Customize %{name}", &[("name", &name)]);
        let subtitle = if dark {
            crate::tr("Your dark theme. Edits save as you go.")
        } else {
            crate::tr("Your light theme. Edits save as you go.")
        };

        let desk_scrim = cx.entity();
        let desk_close = cx.entity();
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .gap(px(16.))
            .pl(px(24.))
            .pr(px(16.))
            .py(px(16.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(
                        ui::text(faces, title)
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::text(faces, subtitle)
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text2()),
                    ),
            )
            .child(
                CarbonButton::new("drawer-close", crate::tr("Close"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Close)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        desk_close.update(cx, Desk::close_theme_editor);
                    }),
            );

        let picker_id = SharedString::from(format!(
            "theme-picker-{}-{}",
            crate::git::slug(&name),
            editing_role.name()
        ));
        let desk_picker = cx.entity();
        let picker = InlineColorPicker::new(picker_id)
            .label(editing_role.name())
            .value(active_hex)
            .labels([
                crate::tr("Saturation and brightness"),
                crate::tr("Hue"),
                crate::tr("Hex colour"),
                crate::tr("Swatches"),
            ])
            .on_value_change(move |hex, window, cx| {
                if let Some(parsed) = themes::parse_hex(&hex) {
                    desk_picker.update(cx, |this, cx| {
                        this.set_edited_theme_role(editing_role, parsed, window, cx);
                    });
                }
            });

        let left_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(16.))
            .child(Self::drawer_primary_swatches(
                faces,
                &role_vals,
                editing_role,
                cx,
            ))
            .child(Self::drawer_preview_card(faces, &role_vals));

        let right_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(16.))
            .child(picker)
            .child(Self::drawer_contrast_checks(faces, &role_vals));

        let body = div()
            .id("theme-drawer-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p(px(24.))
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(24.))
                    .child(left_col)
                    .child(right_col),
            )
            .child(self.drawer_tokens_grid(faces, &role_vals, editing_role, cx));

        let footer = Self::drawer_footer(&name, cx);

        Some(
            div()
                .id("theme-drawer-layer")
                .absolute()
                .inset_0()
                .occlude()
                .child(
                    div()
                        .id("theme-drawer-scrim")
                        .absolute()
                        .inset_0()
                        .bg(hsla(0., 0., 0.086, 0.4))
                        .on_click(move |_, _, cx| {
                            desk_scrim.update(cx, Desk::close_theme_editor);
                        }),
                )
                .child(
                    div()
                        .id("theme-drawer")
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .w(px(600.))
                        .flex()
                        .flex_col()
                        .bg(theme::role(ColorRole::Layer01))
                        .border_l_1()
                        .border_color(theme::border())
                        .shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.18),
                            offset: point(px(-2.), px(0.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .occlude()
                        .child(header)
                        .child(body)
                        .child(footer),
                )
                .into_any_element(),
        )
    }

    fn drawer_primary_swatches(
        faces: &Faces,
        role_vals: &[(ColorRole, Hsla); 12],
        editing_role: ColorRole,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().flex().flex_col().gap(px(8.));
        for role in themes::PRIMARY_ROLES {
            let val = role_vals
                .iter()
                .find_map(|(r, c)| (*r == role).then_some(*c))
                .unwrap_or_else(|| theme::role(role));
            let active = role == editing_role;
            let desk_pick = cx.entity();
            let hover_bg = theme::role(ColorRole::FieldHover02);
            list = list.child(
                div()
                    .id(SharedString::from(format!("primary-sw-{}", role.name())))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .p(px(4.))
                    .bg(theme::role(ColorRole::Field02))
                    .hover(move |s| s.bg(hover_bg))
                    .cursor_pointer()
                    .when(active, |d| {
                        d.shadow(vec![BoxShadow {
                            color: theme::role(ColorRole::Focus),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(2.),
                            inset: true,
                        }])
                    })
                    .on_click(move |_, _, cx| {
                        desk_pick.update(cx, |this, cx| this.select_editor_role(role, cx));
                    })
                    .child(
                        div()
                            .flex_none()
                            .size(px(28.))
                            .bg(val)
                            .shadow(vec![BoxShadow {
                                color: theme::role(ColorRole::BorderSubtle01),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(1.),
                                inset: true,
                            }]),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::mono(faces, role.name())
                                    .text_size(px(crate::style::CAPTION))
                                    .line_height(px(16.))
                                    .text_color(theme::text())
                                    .truncate(),
                            )
                            .child(
                                ui::mono(faces, themes::to_hex(val))
                                    .text_size(px(crate::style::CAPTION))
                                    .line_height(px(16.))
                                    .text_color(theme::text2())
                                    .truncate(),
                            ),
                    ),
            );
        }
        list
    }

    fn drawer_preview_card(faces: &Faces, role_vals: &[(ColorRole, Hsla); 12]) -> Div {
        let lookup = |target: ColorRole| {
            role_vals
                .iter()
                .find_map(|(r, c)| (*r == target).then_some(*c))
                .unwrap_or_else(|| theme::role(target))
        };
        let bg_color = lookup(ColorRole::Background);
        let layer_color = lookup(ColorRole::Layer01);
        let fg_color = lookup(ColorRole::TextPrimary);
        let dim_color = lookup(ColorRole::TextSecondary);
        let accent_color = lookup(ColorRole::Interactive);
        let err_color = lookup(ColorRole::SupportError);
        let btn_ink = button_ink_on(accent_color);

        div()
            .p(px(16.))
            .bg(bg_color)
            .text_color(fg_color)
            .shadow(vec![BoxShadow {
                color: theme::role(ColorRole::BorderSubtle01),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: true,
            }])
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .p(px(12.))
                    .bg(layer_color)
                    .child(
                        ui::text(faces, crate::tr("Preview"))
                            .text_size(px(crate::style::BODY))
                            .line_height(px(20.))
                            .weight(FontWeight::SEMIBOLD)
                            .text_color(fg_color),
                    )
                    .child(
                        ui::text(faces, crate::tr("Secondary text on layer."))
                            .mt(px(4.))
                            .mb(px(8.))
                            .text_size(px(crate::style::BODY))
                            .line_height(px(20.))
                            .text_color(dim_color),
                    )
                    .child(
                        div().px(px(12.)).py(px(4.)).bg(accent_color).child(
                            ui::text(faces, crate::tr("Primary"))
                                .text_size(px(crate::style::BODY))
                                .line_height(px(20.))
                                .text_color(btn_ink),
                        ),
                    )
                    .child(
                        ui::text(faces, crate::tr("Build failed: 1 error"))
                            .mt(px(8.))
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(err_color),
                    ),
            )
    }

    fn drawer_contrast_checks(faces: &Faces, role_vals: &[(ColorRole, Hsla); 12]) -> Div {
        let lookup = |target: ColorRole| {
            role_vals
                .iter()
                .find_map(|(r, c)| (*r == target).then_some(*c))
                .unwrap_or_else(|| theme::role(target))
        };
        let bg_color = lookup(ColorRole::Background);
        let layer_color = lookup(ColorRole::Layer01);
        let fg_color = lookup(ColorRole::TextPrimary);
        let dim_color = lookup(ColorRole::TextSecondary);
        let accent_color = lookup(ColorRole::Interactive);
        let btn_ink = button_ink_on(accent_color);

        let rows = [
            (
                crate::tr("Text on background"),
                contrast_ratio(fg_color, bg_color),
            ),
            (
                crate::tr("Secondary text on layer"),
                contrast_ratio(dim_color, layer_color),
            ),
            (
                crate::tr("Button text on interactive"),
                contrast_ratio(btn_ink, accent_color),
            ),
        ];

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                ui::text(faces, crate::tr("Contrast"))
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(16.))
                    .weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text()),
            )
            .children(rows.into_iter().map(|(what, ratio)| {
                let passes = ratio >= MIN_CONTRAST;
                let (glyph, glyph_color) = if passes {
                    (
                        CarbonIcon::CheckmarkFilled,
                        theme::role(ColorRole::SupportSuccess),
                    )
                } else {
                    (
                        CarbonIcon::WarningAltFilled,
                        theme::role(ColorRole::SupportWarning),
                    )
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        CarbonGlyph::carbon(glyph)
                            .size_length(px(16.))
                            .color(glyph_color)
                            .flex_none(),
                    )
                    .child(
                        ui::text(faces, what)
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text2()),
                    )
                    .child(
                        ui::mono(faces, format!("{ratio:.1}:1"))
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text()),
                    )
            }))
    }

    fn drawer_tokens_grid(
        &self,
        faces: &Faces,
        role_vals: &[(ColorRole, Hsla); 12],
        editing_role: ColorRole,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut pairs: Vec<Div> = Vec::with_capacity(6);
        for row_ix in 0..6 {
            let mut row = div().flex().items_center().gap(px(16.));
            for col_ix in 0..2 {
                let idx = row_ix * 2 + col_ix;
                let (role, val) = role_vals[idx];
                let active = role == editing_role;
                let desk_pick = cx.entity();
                row = row.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(32.))
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id(SharedString::from(format!("tok-pick-{}", role.name())))
                                .flex()
                                .flex_1()
                                .min_w(px(0.))
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    desk_pick
                                        .update(cx, |this, cx| this.select_editor_role(role, cx));
                                })
                                .child(div().flex_none().size(px(20.)).bg(val).shadow(vec![
                                    BoxShadow {
                                        color: theme::role(if active {
                                            ColorRole::Focus
                                        } else {
                                            ColorRole::BorderStrong01
                                        }),
                                        offset: point(px(0.), px(0.)),
                                        blur_radius: px(0.),
                                        spread_radius: px(if active { 2. } else { 1. }),
                                        inset: true,
                                    },
                                ]))
                                .child(
                                    ui::mono(faces, role.name())
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(px(crate::style::CAPTION))
                                        .line_height(px(16.))
                                        .text_color(theme::text())
                                        .truncate(),
                                ),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .h(px(28.))
                                .flex()
                                .flex_none()
                                .items_center()
                                .bg(theme::role(ColorRole::Field02))
                                .border_b_1()
                                .border_color(theme::role(ColorRole::BorderStrong01))
                                .font_family(faces.mono.clone())
                                .text_size(px(crate::style::CAPTION))
                                .child(
                                    div().flex_1().min_w(px(0.)).child(
                                        Input::new(&self.pages.theme_hexes[idx])
                                            .appearance(false)
                                            .small(),
                                    ),
                                ),
                        ),
                );
            }
            pairs.push(row);
        }

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        ui::text(faces, crate::tr("Tokens"))
                            .text_size(px(crate::style::BODY))
                            .line_height(px(20.))
                            .weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::text(
                            faces,
                            crate::tr(
                                "Each colour of the theme, as gpui-kit's theme file names it. Pick a swatch or type a hex.",
                            ),
                        )
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(16.))
                        .text_color(theme::text2()),
                    ),
            )
            .child(div().flex().flex_col().gap(px(4.)).children(pairs))
    }

    fn drawer_footer(name: &str, cx: &mut Context<Self>) -> Div {
        let dup_name = name.to_owned();
        let file_name = name.to_owned();
        let rm_name = name.to_owned();
        let desk_dup = cx.entity();
        let desk_open_file = cx.entity();
        let desk_reload = cx.entity();
        let desk_reset = cx.entity();
        let desk_remove = cx.entity();

        let overflow = DropdownMenu::new("drawer-overflow")
            .kind(DropdownMenuKind::Ghost)
            .size(DropdownMenuSize::Sm)
            .overflow_icon(CarbonIcon::OverflowMenuHorizontal)
            .overflow_label(crate::tr("Export as a theme file, or reset"))
            .item(
                MenuItem::item(crate::tr("Open file in editor")).on_select(move |_, cx| {
                    let target = file_name.clone();
                    desk_open_file.update(cx, |this, cx| this.open_edited_theme_file(&target, cx));
                }),
            )
            .item(
                MenuItem::item(crate::tr("Read themes again")).on_select(move |_, cx| {
                    desk_reload.update(cx, Desk::read_themes);
                }),
            )
            .item(MenuItem::divider())
            .item(
                MenuItem::item(crate::tr("Reset to Carbon defaults")).on_select(
                    move |window, cx| {
                        desk_reset.update(cx, |this, cx| this.reset_edited_theme(window, cx));
                    },
                ),
            );

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(24.))
            .py(px(16.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                CarbonButton::new("drawer-duplicate", crate::tr("Duplicate"))
                    .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        let src = dup_name.clone();
                        desk_dup
                            .update(cx, |this, cx| this.duplicate_edited_theme(&src, window, cx));
                    }),
            )
            .child(
                div()
                    .shadow(vec![BoxShadow {
                        color: theme::role(ColorRole::BorderStrong01),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: true,
                    }])
                    .child(overflow),
            )
            .child(div().flex_1())
            .child(
                CarbonButton::new("drawer-remove", crate::tr("Remove theme"))
                    .variant(ButtonVariant::DangerGhost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, _, cx| {
                        let gone = rm_name.clone();
                        desk_remove.update(cx, |this, cx| this.remove_theme(&gone, cx));
                    }),
            )
    }
}

fn button_ink_on(accent: Hsla) -> Hsla {
    let dark_ink: Hsla = rgb(0x000b_1220).into();
    let light_ink: Hsla = white();
    if contrast_ratio(dark_ink, accent) >= contrast_ratio(light_ink, accent) {
        dark_ink
    } else {
        light_ink
    }
}
