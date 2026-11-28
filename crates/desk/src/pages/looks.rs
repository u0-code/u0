//! `Settings › Appearance` (`SetAppearance.dc.html`): Carbon `Mode`,
//! `Fonts`, `Layout`, and `Motion and effects` sections, and the themes
//! under them (`theme_library`); a theme is customized in `theme_drawer`.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div, px,
};
use u0ui::{
    components::button::{Button as CarbonButton, ButtonSize, ButtonVariant},
    density::{self, Density},
};

use super::{
    Tab,
    prefs::managed_info_banner,
    srow::{SGroup, SRow, carbon_page_with_tail},
};
use crate::{
    desk::Desk,
    fonts::Faces,
    settings::{CodeFont, Look, Motion, SCALES, Settings, Shell, Sort, TermSize, Width},
    theme,
    themes::Themes,
};

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
                    "For threads and projects alike; pinned ones stay on top.",
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
}
