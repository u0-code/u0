//! Theme actions for `Settings › Appearance`: choosing the active dark or light
//! theme, creating or duplicating a custom theme, editing its Carbon tokens
//! live in the `600px` drawer, importing VS Code or `gpui-kit` theme files,
//! resetting or removing custom themes, and choosing the thread backdrop.

use std::{path::PathBuf, rc::Rc};

use gpui::{App, Context, Focusable as _, Hsla, PathPromptOptions, Window};
use u0ui::tokens::{CarbonTheme, ColorRole};

use super::Desk;
use crate::{store, themes};

impl Desk {
    /// Draw the dark look (or the light) in the theme `name`; `None` uses
    /// Carbon's default (`G100` for dark, `White` for light).
    pub(crate) fn choose_theme(
        &mut self,
        dark: bool,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if dark {
            self.settings.theme_dark = name;
        } else {
            self.settings.theme_light = name;
        }
        self.save_settings();
        self.settings_took(cx);
        cx.notify();
    }

    /// Reload custom themes from `<home>/themes/` and re-apply the active one.
    pub(crate) fn read_themes(&mut self, cx: &mut Context<Self>) {
        self.pages.themes_refused = themes::load(&self.home, cx);
        if let Some(open) = self.pages.editing_theme.as_deref()
            && themes::find(cx, open).is_none()
        {
            self.pages.editing_theme = None;
        }
        self.settings_took(cx);
        cx.notify();
    }

    /// Open custom theme `name` in the `600px` `Customize <Theme>` drawer and
    /// populate the 12 hex fields from its current colours.
    pub(crate) fn open_theme_editor(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if themes::find(cx, name).is_none() {
            return;
        }
        self.pages.editing_theme = Some(name.to_owned());
        self.sync_theme_editor_hexes(window, cx);
        cx.notify();
    }

    /// Close the `600px` `Customize <Theme>` drawer.
    pub(crate) fn close_theme_editor(&mut self, cx: &mut Context<Self>) {
        self.pages.editing_theme = None;
        cx.notify();
    }

    /// Select `role` in the drawer so the right-hand `ColorPicker` edits it.
    pub(crate) fn select_editor_role(&mut self, role: ColorRole, cx: &mut Context<Self>) {
        self.pages.editing_role = role;
        cx.notify();
    }

    fn sync_theme_editor_hexes(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.pages.editing_theme.as_deref() else {
            return;
        };
        let Some(config) = themes::find(cx, name) else {
            return;
        };
        let values = themes::role_values(&config);
        for (idx, (_, color)) in values.into_iter().enumerate() {
            let hex = themes::to_hex(color);
            self.pages.theme_hexes[idx].update(cx, |input, cx| {
                input.set_value(hex, window, cx);
            });
        }
    }

    fn unique_theme_name(&self, base: &str, cx: &App) -> String {
        themes::free_name(base, |n| {
            themes::taken(cx, &self.home, n) != themes::Taken::Free
        })
    }

    /// Make `imported` a theme of yours. Under a name taken by one of
    /// yours, ask first: replace that one, or keep both, the import under
    /// the next name free. A name u0's own themes have is never replaced.
    pub(crate) fn import(
        &mut self,
        imported: themes::Imported,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = imported.name.clone();
        let other = self.unique_theme_name(&name, cx);
        let yours = match themes::taken(cx, &self.home, &name) {
            themes::Taken::Free => return self.keep_import(imported, &name, None, window, cx),
            themes::Taken::Ours => return self.keep_import(imported, &other, None, window, cx),
            themes::Taken::Yours(yours) => yours,
        };
        let (replace, both, cancel) = (
            crate::tr("Replace"),
            crate::tr("Keep both"),
            crate::tr("Cancel"),
        );
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &crate::tr_with(
                "You have a theme called “%{name}” already",
                &[("name", &yours)],
            ),
            Some(&crate::tr_with(
                "Replace it with the one imported, or keep both: the one imported as “%{other}”.",
                &[("other", &other)],
            )),
            &[replace.as_ref(), both.as_ref(), cancel.as_ref()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let chosen = answer.await;
            let _ = this.update_in(cx, |this, window, cx| match chosen {
                Ok(0) => this.keep_import(imported, &name, Some(yours), window, cx),
                Ok(1) => this.keep_import(imported, &other, None, window, cx),
                _ => {}
            });
        })
        .detach();
    }

    /// Keep `imported` as yours under `name`, in place of `replaced`.
    fn keep_import(
        &mut self,
        imported: themes::Imported,
        name: &str,
        replaced: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = replaced.filter(|old| old != name) {
            let mut all = cx.global::<themes::Themes>().0.clone();
            all.retain(|(t, _)| t.name != old);
            cx.set_global(themes::Themes(all));
        }
        let dark = imported.dark;
        let (path, text) = imported.file(&self.home, name);
        self.save_and_open_theme(path, text, name, dark, window, cx);
    }

    /// Create a new custom theme from the active theme of the current look,
    /// select it, and open it in the `600px` drawer.
    pub(crate) fn new_custom_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = !crate::theme::is_light();
        let chosen = if dark {
            &self.settings.theme_dark
        } else {
            &self.settings.theme_light
        }
        .clone();
        let base_label = match (
            chosen.as_deref(),
            crate::theme::carbon_for(dark, chosen.as_deref()),
        ) {
            (_, Some(c)) => crate::theme::carbon_label(c).to_owned(),
            (Some(custom), None) => custom.to_owned(),
            (None, None) => "Carbon".to_owned(),
        };
        let name = self.unique_theme_name(&format!("{base_label} (yours)"), cx);
        let (path, text) = if let Some(config) = chosen.as_deref().and_then(|n| themes::find(cx, n))
        {
            themes::duplicate(&self.home, &config, &name)
        } else {
            let carbon = crate::theme::carbon_for(dark, chosen.as_deref()).unwrap_or(if dark {
                CarbonTheme::G100
            } else {
                CarbonTheme::White
            });
            themes::own_carbon(&self.home, &name, carbon)
        };
        self.save_and_open_theme(path, text, &name, dark, window, cx);
    }

    /// Duplicate built-in `base` into `<Label> (yours)`, select it for its
    /// mode, and open it in the `600px` drawer.
    pub(crate) fn customize_carbon_theme(
        &mut self,
        base: CarbonTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = crate::theme::carbon_label(base);
        let name = self.unique_theme_name(&format!("{label} (yours)"), cx);
        let (path, text) = themes::own_carbon(&self.home, &name, base);
        self.save_and_open_theme(path, text, &name, base.is_dark(), window, cx);
    }

    /// Duplicate custom theme `source_name` into `<source_name> copy`, select
    /// it for its mode, and open the copy in the drawer.
    pub(crate) fn duplicate_edited_theme(
        &mut self,
        source_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = themes::find(cx, source_name) else {
            return;
        };
        let dark = config.mode.is_dark();
        let name = self.unique_theme_name(&format!("{source_name} copy"), cx);
        let (path, text) = themes::duplicate(&self.home, &config, &name);
        self.save_and_open_theme(path, text, &name, dark, window, cx);
    }

    /// Update `role` on the open custom theme, persist `<home>/themes/*.json`,
    /// sync the token hex grid, and re-project the live window theme.
    pub(crate) fn set_edited_theme_role(
        &mut self,
        role: ColorRole,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self.pages.editing_theme.clone() else {
            return;
        };
        let Some(config) = themes::find(cx, &name) else {
            return;
        };
        let (path, text) = themes::set_role_color(&self.home, &config, role, color);
        Self::store_theme_bytes(path, text, &name, cx);
        if let Some(idx) = themes::EDITOR_ROLES.iter().position(|(r, _)| *r == role) {
            let input = &self.pages.theme_hexes[idx];
            if !input.focus_handle(cx).is_focused(window) {
                let hex = themes::to_hex(color);
                input.update(cx, |s, cx| s.set_value(hex, window, cx));
            }
        }
        self.settings_took(cx);
        cx.notify();
    }

    /// Apply a hex edit from `pages.theme_hexes[idx]` (`commit` normalizes or
    /// restores on blur / Enter; typing applies live once 6 hex digits parse).
    pub(crate) fn set_edited_theme_hex(
        &mut self,
        idx: usize,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((role, _)) = themes::EDITOR_ROLES.get(idx).copied() else {
            return;
        };
        let Some(name) = self.pages.editing_theme.clone() else {
            return;
        };
        let Some(config) = themes::find(cx, &name) else {
            return;
        };
        let raw = self.pages.theme_hexes[idx].read(cx).value().to_string();
        if let Some(norm) = u0ui::u0::primitives::color_picker::normalize_hex(&raw)
            && let Some(color) = themes::parse_hex(&norm)
        {
            self.pages.editing_role = role;
            let (path, text) = themes::set_role_color(&self.home, &config, role, color);
            Self::store_theme_bytes(path, text, &name, cx);
            if commit {
                self.pages.theme_hexes[idx].update(cx, |s, cx| s.set_value(norm, window, cx));
            }
            self.settings_took(cx);
            cx.notify();
        } else if commit {
            let current = themes::role_values(&config)[idx].1;
            let hex = themes::to_hex(current);
            self.pages.theme_hexes[idx].update(cx, |s, cx| s.set_value(hex, window, cx));
            cx.notify();
        }
    }

    /// Reset the open custom theme's colours to Carbon's defaults for its mode.
    pub(crate) fn reset_edited_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.pages.editing_theme.clone() else {
            return;
        };
        let Some(config) = themes::find(cx, &name) else {
            return;
        };
        let (path, text) = themes::reset_to_carbon(&self.home, &config);
        Self::store_theme_bytes(path, text, &name, cx);
        self.sync_theme_editor_hexes(window, cx);
        self.settings_took(cx);
        cx.notify();
    }

    /// Open custom theme `name`'s JSON file in the configured external editor.
    pub(crate) fn open_edited_theme_file(&self, name: &str, cx: &App) {
        if let Some(path) = themes::path_of(cx, name) {
            self.edit(path, None);
        }
    }

    /// Prompt for a VS Code JSONC colour theme file, import it into
    /// `<home>/themes/`, select it, and open it in the drawer.
    pub(crate) fn import_theme(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::tr("Import")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(file) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    let read = std::fs::read_to_string(&file).map_err(|e| e.to_string());
                    match read.and_then(|t| themes::from_vscode(&t)) {
                        Ok(imported) => this.import(imported, window, cx),
                        Err(e) => this.trouble(
                            crate::tr("That theme could not be imported"),
                            Some(e),
                            None,
                        ),
                    }
                });
            }
        })
        .detach();
    }

    /// Prompt for a `gpui-kit` `ThemeSet` JSON file, import it into
    /// `<home>/themes/`, select it, and open it in the drawer.
    pub(crate) fn import_kit_theme(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::tr("Import")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(file) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    let read = std::fs::read_to_string(&file).map_err(|e| e.to_string());
                    match read.and_then(|t| themes::from_kit_json(&t)) {
                        Ok(imported) => this.import(imported, window, cx),
                        Err(e) => this.trouble(
                            crate::tr("That theme could not be imported"),
                            Some(e),
                            None,
                        ),
                    }
                });
            }
        })
        .detach();
    }

    fn store_theme_bytes(path: PathBuf, text: Vec<u8>, name: &str, cx: &mut App) {
        let set: Option<gpui::component::theme::ThemeSet> = serde_json::from_slice(&text).ok();
        let mut all = cx
            .try_global::<themes::Themes>()
            .map(|t| t.0.clone())
            .unwrap_or_default();
        if let Some(pos) = all.iter().position(|(t, _)| t.name == name) {
            let mut replacement = set
                .into_iter()
                .flat_map(|s| s.themes)
                .map(|t| (Rc::new(t), Some(path.clone())));
            if let Some(first) = replacement.next() {
                all[pos] = first;
            }
        } else {
            let made = set.into_iter().flat_map(|s| s.themes);
            all.extend(made.map(|t| (Rc::new(t), Some(path.clone()))));
        }
        cx.set_global(themes::Themes(all));
        store::queue(store::Job::Write(path, text));
    }

    fn save_and_open_theme(
        &mut self,
        path: PathBuf,
        text: Vec<u8>,
        name: &str,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::store_theme_bytes(path, text, name, cx);
        self.choose_theme(dark, Some(name.to_owned()), cx);
        self.open_theme_editor(name, window, cx);
    }

    /// Delete custom theme `name`: its file on disk, any drawer open on it, and
    /// any dark/light selection pointing to it.
    pub(crate) fn remove_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(path) = themes::path_of(cx, name) else {
            return;
        };
        store::queue(store::Job::Remove(path));
        let mut all = cx.global::<themes::Themes>().0.clone();
        all.retain(|(t, _)| t.name != name);
        cx.set_global(themes::Themes(all));
        if self.pages.editing_theme.as_deref() == Some(name) {
            self.pages.editing_theme = None;
        }
        for dark in [true, false] {
            let chosen = if dark {
                &self.settings.theme_dark
            } else {
                &self.settings.theme_light
            };
            if chosen.as_deref() == Some(name) {
                self.choose_theme(dark, None, cx);
            }
        }
        cx.notify();
    }

    /// Ask for a picture to draw under the thread.
    pub(crate) fn choose_backdrop(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::tr("Choose")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(file) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.settings.backdrop = Some(file);
                    this.save_settings();
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
