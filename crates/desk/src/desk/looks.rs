//! What is done to themes from Settings › Appearance: one chosen for a
//! look, the one drawn made a theme of yours to change, yours removed,
//! a VS Code theme imported, themes read again; and the picture under the
//! thread chosen.

use gpui::{Context, PathPromptOptions, Window};

use super::Desk;
use crate::{store, themes};

impl Desk {
    /// Draw the dark look (or the light) in the theme `name`; none, Ghost's
    /// own.
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

    /// Read the themes again, yours as they are now in their folder, and
    /// draw the chosen ones from them.
    pub(crate) fn read_themes(&mut self, cx: &mut Context<Self>) {
        self.pages.themes_refused = themes::load(&self.home, cx);
        self.settings_took(cx);
        cx.notify();
    }

    /// Make the theme the look is drawn in now a theme of yours, called
    /// `name`, to change; it is chosen for this look and opened in your
    /// editor.
    pub(crate) fn duplicate_theme(&mut self, cx: &mut Context<Self>) {
        let dark = !crate::theme::is_light();
        let chosen = if dark {
            &self.settings.theme_dark
        } else {
            &self.settings.theme_light
        };
        let name = format!("{} (yours)", chosen.as_deref().unwrap_or("Ghost"));
        let (path, text) = match chosen.as_deref().and_then(|n| themes::find(cx, n)) {
            Some(config) => themes::duplicate(&self.home, &config, &name),
            None => themes::own(&self.home, &name, dark),
        };
        self.keep_theme(path, text, &name, dark, cx);
    }

    /// Ask for a VS Code theme file, and make it a theme of yours.
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
                let _ = this.update(cx, |this, cx| {
                    let read = std::fs::read_to_string(&file).map_err(|e| e.to_string());
                    match read.and_then(|t| themes::from_vscode(&this.home, &t)) {
                        Ok((path, text, name)) => {
                            let dark = !crate::theme::is_light();
                            this.keep_theme(path, text, &name, dark, cx);
                        }
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

    /// Write the theme of yours `name` (its `text`) at `path`, read it
    /// into the themes, choose it for the dark look or the light, and open
    /// it to be changed.
    fn keep_theme(
        &mut self,
        path: std::path::PathBuf,
        text: Vec<u8>,
        name: &str,
        dark: bool,
        cx: &mut Context<Self>,
    ) {
        let set: Option<gpui::component::theme::ThemeSet> = serde_json::from_slice(&text).ok();
        let mut all = cx
            .try_global::<themes::Themes>()
            .map(|t| t.0.clone())
            .unwrap_or_default();
        all.retain(|(t, _)| t.name != name);
        let made = set.into_iter().flat_map(|s| s.themes);
        all.extend(made.map(|t| (std::rc::Rc::new(t), Some(path.clone()))));
        cx.set_global(themes::Themes(all));

        store::queue(store::Job::Write(path.clone(), text));
        self.choose_theme(dark, Some(name.to_owned()), cx);
        self.done(
            crate::tr_with(
                "%{name} is a theme of yours now; change it, then read the themes again",
                &[("name", &name)],
            ),
            None,
        );
        self.edit(path, None);
    }

    /// Take your theme `name` away: its file, and its choice.
    pub(crate) fn remove_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(path) = cx
            .try_global::<themes::Themes>()
            .and_then(|t| t.0.iter().find(|(c, _)| c.name == name)?.1.clone())
        else {
            return;
        };
        store::queue(store::Job::Remove(path));
        let mut all = cx.global::<themes::Themes>().0.clone();
        all.retain(|(t, _)| t.name != name);
        cx.set_global(themes::Themes(all));
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
