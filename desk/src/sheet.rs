//! The sheet a brain is added or edited in: a small modal over the window.
//!
//! Adding goes in two steps. First a preset, from a grid of the providers
//! the desk knows (`brains::PRESETS`); then its few fields: the model,
//! offered from the provider's own list once its key is there; the
//! variable the key is in, with whether it is set; and, folded away, the
//! base URL and the name it is saved under. Test makes one real request.
//! Editing a brain opens the sheet at its fields, with a way to remove it.
//! Everything shown is checked for real; the key itself is never shown or
//! written.

use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, Hsla, SharedString,
    Stateful, Window, actions, deferred, div, prelude::*, px,
};

use crate::{
    brains::{self, Draft, Endpoint, KeyStatus, Mark, PRESETS, Preset, Probed, Wire},
    desk::Desk,
    fonts::Faces,
    input::{Cancelled, Changed, Submitted, TextInput},
    link,
    solver::{self, Provider},
    theme, ui,
};

actions!(brain_sheet, [Close]);

/// The context Esc closes the sheet in.
pub const CONTEXT: &str = "BrainSheet";

/// The models a brain's API offers, as far as the sheet knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Models {
    Unasked,
    Asking,
    Listed(Vec<String>),
    Failed(String),
}

/// What Test found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    Idle,
    Running,
    Passed(String),
    /// It went through, with something to look at.
    Warned(String),
    Failed(String),
}

// The few plain states of a small form read better as fields than folded
// into one enum.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Sheet {
    pub open: bool,
    /// The preset being filled in; `None` is the first step, the grid.
    pub preset: Option<&'static Preset>,
    /// The brain being edited, by the name it is saved under.
    pub editing: Option<String>,
    pub model: Entity<TextInput>,
    pub key: Entity<TextInput>,
    pub base: Entity<TextInput>,
    pub name: Entity<TextInput>,
    pub advanced: bool,
    pub key_status: KeyStatus,
    pub models: Models,
    /// Whether the model list is unfolded under its field.
    pub listing: bool,
    pub probe: Probe,
    /// Why the last save or removal was refused.
    pub refused: Option<String>,
    /// Bumped whenever what the sheet shows changes under a request, so
    /// that an answer to an older question is dropped.
    generation: u64,
    focus: FocusHandle,
}

impl Sheet {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let model = cx.new(|cx| TextInput::new(cx, "model").field());
        let key = cx.new(|cx| TextInput::new(cx, "e.g. MY_API_KEY").field());
        let base = cx.new(|cx| TextInput::new(cx, "https://…/v1").field());
        let name = cx.new(|cx| TextInput::new(cx, "name").field());
        for field in [&model, &key, &base, &name] {
            cx.subscribe_in(field, window, |this, _, _: &Cancelled, window, cx| {
                this.close_sheet(window, cx);
            })
            .detach();
            cx.subscribe_in(field, window, |this, _, Submitted(_), window, cx| {
                this.save_brain(false, window, cx);
            })
            .detach();
        }
        cx.subscribe(&key, |this, _, _: &Changed, cx| this.refresh_key(cx))
            .detach();
        cx.subscribe(&base, |this, _, _: &Changed, cx| {
            this.sheet.models = Models::Unasked;
            this.sheet.listing = false;
            this.refresh_key(cx);
        })
        .detach();
        Self {
            open: false,
            preset: None,
            editing: None,
            model,
            key,
            base,
            name,
            advanced: false,
            key_status: KeyStatus::NotSet,
            models: Models::Unasked,
            listing: false,
            probe: Probe::Idle,
            refused: None,
            generation: 0,
            focus: cx.focus_handle(),
        }
    }

    fn reset(&mut self) {
        self.generation += 1;
        self.models = Models::Unasked;
        self.listing = false;
        self.probe = Probe::Idle;
        self.refused = None;
    }
}

impl Desk {
    /// Open the sheet: at the grid of presets, or at the fields of the
    /// brain saved as `editing`.
    pub(crate) fn open_sheet(
        &mut self,
        editing: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.providers = self.deployment.providers();
        self.sheet.reset();
        self.sheet.open = true;
        self.sheet.preset = None;
        self.sheet.editing = None;
        self.sheet.focus.focus(window);
        if let Some(p) = editing.and_then(|name| self.providers.iter().find(|p| p.name == name)) {
            let p = p.clone();
            self.edit_brain(&p, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sheet.open {
            return;
        }
        self.sheet.reset();
        self.sheet.open = false;
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Fill the fields from `preset`, as a new brain.
    fn pick_preset(
        &mut self,
        preset: &'static Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A CLI is signed in to once; its tile opens the one there is.
        if preset.cli()
            && let Some(p) = self.providers.iter().find(|p| p.kind == preset.kind)
        {
            let p = p.clone();
            self.edit_brain(&p, window, cx);
            return;
        }
        let taken = |n: &str| self.providers.iter().any(|p| p.name == n);
        let name = (1..100)
            .map(|i| {
                if i == 1 {
                    preset.id.to_owned()
                } else {
                    format!("{}-{i}", preset.id)
                }
            })
            .find(|n| !taken(n))
            .unwrap_or_default();
        self.fill(
            preset,
            None,
            &Draft {
                model: preset.model.to_owned(),
                key_env: preset.key_env.unwrap_or_default().to_owned(),
                base_url: preset.base_url.to_owned(),
            },
            &name,
            window,
            cx,
        );
    }

    /// Fill the fields from a brain already saved.
    fn edit_brain(&mut self, p: &Provider, window: &mut Window, cx: &mut Context<Self>) {
        let preset = p
            .preset()
            .or_else(|| Preset::by_id("custom"))
            .expect("a custom preset");
        let draft = Draft {
            model: p.model.clone(),
            key_env: p.key_env.clone().unwrap_or_default(),
            // Where the table names none, the daemon's default is the
            // preset's own: OpenAI's or Anthropic's.
            base_url: p
                .base_url
                .clone()
                .unwrap_or_else(|| preset.base_url.to_owned()),
        };
        self.fill(preset, Some(p.name.clone()), &draft, &p.name, window, cx);
    }

    fn fill(
        &mut self,
        preset: &'static Preset,
        editing: Option<String>,
        draft: &Draft,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet.reset();
        self.sheet.preset = Some(preset);
        self.sheet.editing = editing;
        self.sheet.advanced = preset.id == "custom" && draft.base_url.is_empty();
        for (field, text) in [
            (&self.sheet.model, draft.model.as_str()),
            (&self.sheet.key, draft.key_env.as_str()),
            (&self.sheet.base, draft.base_url.as_str()),
            (&self.sheet.name, name),
        ] {
            field.update(cx, |f, cx| f.set_text(text, cx));
        }
        self.sheet.model.update(cx, |f, cx| {
            f.set_placeholder(
                if preset.id == "codex" {
                    "default"
                } else {
                    "model"
                },
                cx,
            );
        });
        self.refresh_key(cx);
        self.sheet.model.focus_handle(cx).focus(window);
        // The list is asked for at once where it can be had.
        if !preset.cli() && (preset.keyless() || self.sheet.key_status.found()) {
            self.fetch_models(cx);
        }
        cx.notify();
    }

    /// Back from the fields to the grid.
    fn back_to_presets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet.reset();
        self.sheet.preset = None;
        self.sheet.editing = None;
        self.sheet.focus.focus(window);
        cx.notify();
    }

    /// The variable the key is read from: the one typed, or the daemon's
    /// default for the kind when none is.
    fn key_var(&self, cx: &Context<Self>) -> String {
        let typed = self.sheet.key.read(cx).text();
        if !typed.is_empty() {
            return typed;
        }
        match self.sheet.preset.map(Preset::wire) {
            Some(Wire::Anthropic) => "ANTHROPIC_API_KEY".to_owned(),
            Some(Wire::OpenAi) => "OPENAI_API_KEY".to_owned(),
            _ => String::new(),
        }
    }

    /// Look again for the key: in the environment, then the checkout's `.env`.
    fn refresh_key(&mut self, cx: &mut Context<Self>) {
        let var = self.key_var(cx);
        let dotenv = self.deployment.dotenv();
        self.sheet.key_status = brains::key_status(&var, &solver::env, dotenv.as_deref());
        cx.notify();
    }

    /// Where the fields say the brain is, with its key, for a request.
    fn sheet_endpoint(&self, cx: &Context<Self>) -> Option<Endpoint> {
        let preset = self.sheet.preset?;
        let dotenv = self.deployment.dotenv();
        let key = if preset.keyless() {
            Some(brains::NO_KEY.to_owned())
        } else {
            brains::key_value(&self.key_var(cx), &solver::env, dotenv.as_deref())
        };
        Some(Endpoint {
            wire: preset.wire(),
            base_url: self.sheet.base.read(cx).text(),
            key,
            model: self.sheet.model.read(cx).text(),
        })
    }

    /// Why a request cannot be made from what the fields say, if it cannot.
    fn cannot_ask(&self, end: &Endpoint, cx: &Context<Self>) -> Option<String> {
        if end.base_url.is_empty() {
            Some("no base URL".to_owned())
        } else if end.key.is_none() {
            Some(format!("{} is not set", self.key_var(cx)))
        } else {
            None
        }
    }

    /// Run `work` on the desk's runtime and hand its answer to `done`,
    /// unless the sheet has moved on in the meantime.
    fn on_answer<T: Send + 'static>(
        &mut self,
        work: impl Future<Output = T> + Send + 'static,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T) + 'static,
    ) {
        let generation = self.sheet.generation;
        let task = link::runtime().spawn(work);
        cx.spawn(async move |this, cx| {
            let Ok(answer) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.sheet.open && this.sheet.generation == generation {
                    done(this, answer);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Ask the provider for its models.
    fn fetch_models(&mut self, cx: &mut Context<Self>) {
        let Some(end) = self.sheet_endpoint(cx) else {
            return;
        };
        if let Some(why) = self.cannot_ask(&end, cx) {
            self.sheet.models = Models::Failed(why);
            return;
        }
        let network = match self.deployment.network() {
            Ok(n) => n,
            Err(why) => {
                self.sheet.models = Models::Failed(why);
                return;
            }
        };
        self.sheet.models = Models::Asking;
        self.on_answer(
            async move { brains::models(&network, &end).await },
            cx,
            |this, answer| {
                this.sheet.models = match answer {
                    Ok(ids) if ids.is_empty() => Models::Failed("no models listed".to_owned()),
                    Ok(ids) => Models::Listed(ids),
                    Err(why) => Models::Failed(why.line()),
                };
            },
        );
    }

    /// Unfold the model list, asking for it first if it is not there.
    fn toggle_models(&mut self, cx: &mut Context<Self>) {
        self.sheet.listing = !self.sheet.listing;
        if self.sheet.listing && !matches!(self.sheet.models, Models::Listed(_) | Models::Asking) {
            self.fetch_models(cx);
        }
        cx.notify();
    }

    /// One real request: a CLI's version, or the API's model list, or a
    /// one-token answer where it lists none.
    fn test_brain(&mut self, cx: &mut Context<Self>) {
        let Some(preset) = self.sheet.preset else {
            return;
        };
        self.sheet.generation += 1;
        self.sheet.refused = None;
        if let Some(program) = preset.program {
            self.sheet.probe = Probe::Running;
            self.on_answer(
                async move {
                    tokio::task::spawn_blocking(move || brains::cli_version(program))
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                },
                cx,
                |this, answer| {
                    this.sheet.probe = match answer {
                        Ok(version) => Probe::Passed(format!("ok · {version}")),
                        Err(why) => Probe::Failed(why),
                    };
                },
            );
            cx.notify();
            return;
        }
        let Some(end) = self.sheet_endpoint(cx) else {
            return;
        };
        if let Some(why) = self.cannot_ask(&end, cx) {
            self.sheet.probe = Probe::Failed(why);
            cx.notify();
            return;
        }
        let network = match self.deployment.network() {
            Ok(n) => n,
            Err(why) => {
                self.sheet.probe = Probe::Failed(why);
                cx.notify();
                return;
            }
        };
        self.sheet.probe = Probe::Running;
        let model = end.model.clone();
        self.on_answer(
            async move { brains::probe(&network, &end).await },
            cx,
            move |this, answer| {
                this.sheet.probe = match answer {
                    Ok(Probed::Listed(ids)) => {
                        let probe = if model.is_empty() || ids.contains(&model) {
                            Probe::Passed(format!("ok · {} models", ids.len()))
                        } else {
                            Probe::Warned(format!("key works · {model} is not listed"))
                        };
                        if !ids.is_empty() {
                            this.sheet.models = Models::Listed(ids);
                        }
                        probe
                    }
                    Ok(Probed::Answered) => Probe::Passed("ok · it answered".to_owned()),
                    Err(why) => Probe::Failed(why),
                };
            },
        );
        cx.notify();
    }

    /// Write the brain into the deployment, and think with it if `use_it`.
    fn save_brain(&mut self, use_it: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = self.sheet.preset else {
            return;
        };
        let draft = Draft {
            model: self.sheet.model.read(cx).text(),
            key_env: self.sheet.key.read(cx).text(),
            base_url: self.sheet.base.read(cx).text(),
        };
        let typed = self.sheet.name.read(cx).text();
        let name = if typed.is_empty() {
            preset.id.to_owned()
        } else {
            typed
        };
        if !preset.cli() && (draft.model.is_empty() || draft.base_url.is_empty()) {
            self.sheet.refused = Some(if draft.model.is_empty() {
                "a model is needed".to_owned()
            } else {
                self.sheet.advanced = true;
                "a base URL is needed".to_owned()
            });
            cx.notify();
            return;
        }
        let editing = self.sheet.editing.clone();
        let (set, clear): (Vec<_>, Vec<_>) = if editing.is_some() {
            // An edit touches only the fields the sheet shows.
            let fields: Vec<(&str, &str)> = draft
                .managed()
                .into_iter()
                .filter(|(k, _)| preset.uses(k))
                .collect();
            (
                fields
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (*k, toml_edit::Value::from(*v)))
                    .collect(),
                fields
                    .iter()
                    .filter(|(_, v)| v.is_empty())
                    .map(|(k, _)| *k)
                    .collect(),
            )
        } else {
            (preset.fields(&draft), Vec::new())
        };
        if let Err(e) = self
            .deployment
            .save_provider(editing.as_deref(), &name, &set, &clear)
        {
            self.sheet.refused = Some(format!("{e:#}"));
            cx.notify();
            return;
        }
        let was = self.sheet.editing.clone();
        self.providers = self.deployment.providers();
        self.brain = self.deployment.brain();
        self.close_sheet(window, cx);
        if use_it {
            self.use_brain(&name, cx);
        } else if was.is_some() && self.brain.as_deref() == Some(name.as_str()) {
            // The brain in use changed under the daemon.
            self.restart(cx);
        }
    }

    /// Take the brain being edited out of the deployment.
    fn remove_brain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.sheet.editing.clone() else {
            return;
        };
        match self.deployment.remove_provider(&name) {
            Ok(()) => {
                self.providers = self.deployment.providers();
                self.close_sheet(window, cx);
            }
            Err(e) => {
                self.sheet.refused = Some(format!("{e:#}"));
                cx.notify();
            }
        }
    }

    /// The sheet over a dimmed window, if it is open.
    pub(crate) fn sheet_overlay(
        &self,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.sheet.open {
            return None;
        }
        let body = match self.sheet.preset {
            None => Self::presets_grid(faces, cx),
            Some(preset) => self.brain_fields(preset, faces, cx),
        };
        let sheet = div()
            .id("brain-sheet")
            .key_context(CONTEXT)
            .track_focus(&self.sheet.focus)
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close_sheet(window, cx)))
            .occlude()
            .flex()
            .flex_col()
            .w(px(440.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .rounded(px(14.))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_sheet(window, cx)))
            .child(self.sheet_head(faces, cx))
            .child(body);
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::black().opacity(0.5))
                    .occlude()
                    .child(sheet),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The sheet's head: what it is about, and a way out.
    fn sheet_head(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let back = (self.sheet.preset.is_some() && self.sheet.editing.is_none()).then(|| {
            small_button("sheet-back")
                .child(ui::icon("chevron-left", 14.).text_color(theme::muted()))
                .on_click(cx.listener(|this, _, window, cx| this.back_to_presets(window, cx)))
        });
        let title: AnyElement = match self.sheet.preset {
            None => ui::text(faces, "Add brain")
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .into_any_element(),
            Some(preset) => {
                let name = self
                    .sheet
                    .editing
                    .clone()
                    .unwrap_or_else(|| preset.name.to_owned());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .child(ui::mark(faces, Mark::Icon(preset.icon), 16.))
                    .child(
                        ui::text(faces, name)
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .into_any_element()
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(44.))
            .pl(px(if back.is_some() { 8. } else { 16. }))
            .pr(px(8.))
            .children(back)
            .child(div().flex_1().min_w(px(0.)).child(title))
            .child(
                small_button("sheet-close")
                    .child(ui::icon("x", 14.).text_color(theme::muted()))
                    .on_click(cx.listener(|this, _, window, cx| this.close_sheet(window, cx))),
            )
    }

    /// The first step: a tile for each preset.
    fn presets_grid(faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap(px(8.)).px(px(16.)).pb(px(16.));
        for preset in PRESETS {
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("preset-{}", preset.id)))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .w(px(96.))
                    .h(px(68.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(theme::border())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()).border_color(theme::border2()))
                    .child(ui::mark(faces, Mark::Icon(preset.icon), 20.))
                    .child(
                        ui::text(faces, preset.name)
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_preset(preset, window, cx);
                    })),
            );
        }
        grid
    }

    /// The second step: the few fields a brain has, and what to do with them.
    fn brain_fields(&self, preset: &'static Preset, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let api = !preset.cli();
        let mut body = div().flex().flex_col().gap(px(10.)).px(px(16.)).pb(px(14.));

        // The model, with the provider's own list under it when unfolded.
        let chevron = api.then(|| {
            small_button("models-toggle")
                .child(match self.sheet.models {
                    Models::Asking => ui::mono(faces, "…")
                        .text_color(theme::faint())
                        .into_any_element(),
                    _ => ui::icon(
                        if self.sheet.listing {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        12.,
                    )
                    .text_color(theme::muted())
                    .into_any_element(),
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_models(cx)))
        });
        body = body.child(field_row(
            faces,
            "Model",
            boxed(&self.sheet.model, faces, true).children(chevron),
        ));
        if api && self.sheet.listing {
            body = body.child(self.model_list(faces, cx));
        }

        // The key's variable, and whether it holds a key here.
        if preset.key_env.is_some() {
            let (word, color, tip) = match self.sheet.key_status {
                KeyStatus::Env => ("found", theme::green(), "in the environment"),
                KeyStatus::DotEnv => ("found", theme::green(), "in the checkout's .env"),
                KeyStatus::NotSet => ("not set", theme::accent(), "not in the environment or .env"),
            };
            let var = self.key_var(cx);
            body = body.child(field_row(
                faces,
                "Key variable",
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w(px(0.))
                    .child(boxed(&self.sheet.key, faces, true))
                    .child(
                        div()
                            .id("key-status")
                            .flex_none()
                            .child(status(faces, word, color))
                            .tooltip(ui::tip(format!("{var}: {tip}"))),
                    ),
            ));
        }

        // Folded: where its API is, and what it is saved as.
        body = body.child(
            div()
                .id("advanced")
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(22.))
                .cursor_pointer()
                .child(
                    ui::icon(
                        if self.sheet.advanced {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        12.,
                    )
                    .text_color(theme::faint()),
                )
                .child(
                    ui::text(faces, "Advanced")
                        .text_size(px(12.))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sheet.advanced = !this.sheet.advanced;
                    cx.notify();
                })),
        );
        if self.sheet.advanced {
            if api {
                body = body.child(field_row(
                    faces,
                    "Base URL",
                    boxed(&self.sheet.base, faces, true),
                ));
            }
            body = body.child(field_row(
                faces,
                "Name",
                boxed(&self.sheet.name, faces, true),
            ));
        }

        // The one line under the fields: what Test or Save found, or else
        // what becomes of the key.
        let line = match (&self.sheet.refused, &self.sheet.probe) {
            (Some(why), _) => Some((why.clone(), theme::coral())),
            (None, Probe::Running) => Some(("testing…".to_owned(), theme::muted())),
            (None, Probe::Passed(s)) => Some((s.clone(), theme::green())),
            (None, Probe::Warned(s)) => Some((s.clone(), theme::accent())),
            (None, Probe::Failed(s)) => Some((s.clone(), theme::coral())),
            (None, Probe::Idle) => preset.key_env.is_some().then(|| {
                (
                    "Only the variable's name is saved, never the key.".to_owned(),
                    theme::faint(),
                )
            }),
        };
        body = body.children(line.map(|(text, color)| {
            ui::one_line(ui::text(faces, text).text_size(px(12.)).text_color(color)).h(px(18.))
        }));

        // Remove at the left, when editing; Test, Save, Save & use at the right.
        let remove = self.sheet.editing.is_some().then(|| {
            div()
                .id("remove-brain")
                .cursor_pointer()
                .child(
                    ui::text(faces, "Remove")
                        .text_size(px(12.5))
                        .text_color(theme::coral()),
                )
                .on_click(cx.listener(|this, _, window, cx| this.remove_brain(window, cx)))
        });
        body.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(4.))
                .children(remove)
                .child(div().flex_1())
                .child(
                    button(faces, "sheet-test", "Test", false)
                        .on_click(cx.listener(|this, _, _, cx| this.test_brain(cx))),
                )
                .child(button(faces, "sheet-save", "Save", false).on_click(
                    cx.listener(|this, _, window, cx| this.save_brain(false, window, cx)),
                ))
                .child(button(faces, "sheet-use", "Save & use", true).on_click(
                    cx.listener(|this, _, window, cx| this.save_brain(true, window, cx)),
                )),
        )
    }

    /// The models the provider lists, to pick from; or why there are none.
    fn model_list(&self, faces: &Faces, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut list = div()
            .id("model-list")
            .flex()
            .flex_col()
            .ml(px(LABEL))
            .max_h(px(168.))
            .overflow_y_scroll()
            .p(px(3.))
            .rounded(px(7.))
            .bg(theme::well())
            .border_1()
            .border_color(theme::border());
        let current = self.sheet.model.read(cx).text();
        match &self.sheet.models {
            Models::Listed(ids) => {
                for (i, id) in ids.iter().enumerate() {
                    let pick = id.clone();
                    list = list.child(
                        div()
                            .id(("model", i))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(24.))
                            .px(px(7.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(ui::one_line(
                                ui::mono(faces, id.clone())
                                    .text_size(px(12.))
                                    .text_color(theme::text2()),
                            ))
                            .when(*id == current, |d| {
                                d.child(ui::icon("check", 12.).text_color(theme::accent()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.sheet.model.update(cx, |f, cx| f.set_text(&pick, cx));
                                this.sheet.listing = false;
                                cx.notify();
                            })),
                    );
                }
            }
            Models::Asking => {
                list = list.child(note(faces, "asking…", theme::muted()));
            }
            Models::Failed(why) => {
                list = list.child(note(faces, why.clone(), theme::coral()));
            }
            Models::Unasked => {}
        }
        list
    }
}

/// The width of a field's label.
const LABEL: f32 = 92.;

/// A label and its control, on one line.
fn field_row(faces: &Faces, label: &'static str, control: Div) -> Div {
    div()
        .flex()
        .items_center()
        .child(
            ui::text(faces, label)
                .w(px(LABEL))
                .flex_none()
                .text_size(px(12.5))
                .text_color(theme::muted()),
        )
        .child(control)
}

/// A field in its box; `mono` for names and URLs.
fn boxed(input: &Entity<TextInput>, faces: &Faces, mono: bool) -> Div {
    div()
        .flex()
        .items_center()
        .flex_1()
        .min_w(px(0.))
        .h(px(30.))
        .pl(px(9.))
        .pr(px(3.))
        .rounded(px(7.))
        .bg(theme::bg())
        .border_1()
        .border_color(theme::border2())
        .when(mono, |d| d.font_family(faces.mono.clone()))
        .child(input.clone())
}

/// A word with a dot before it: a state.
fn status(faces: &Faces, word: &str, color: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .child(ui::dot(color))
        .child(
            ui::mono(faces, word.to_owned())
                .text_size(px(11.5))
                .text_color(color),
        )
}

/// A line of small print in the model list.
fn note(faces: &Faces, text: impl Into<SharedString>, color: Hsla) -> Div {
    ui::one_line(ui::text(faces, text).text_size(px(12.)).text_color(color))
        .h(px(24.))
        .px(px(7.))
        .flex()
        .items_center()
}

/// A small square control.
fn small_button(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
}

/// A button: quiet, or the accent for the one that finishes.
fn button(faces: &Faces, id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(7.))
        .cursor_pointer()
        .when(primary, |d| d.bg(theme::accent()))
        .when(!primary, |d| {
            d.border_1()
                .border_color(theme::border2())
                .hover(|s| s.bg(theme::hover()))
        })
        .child(
            ui::text(faces, label)
                .text_size(px(12.5))
                .text_color(if primary { theme::bg() } else { theme::text2() }),
        )
}
