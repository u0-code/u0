//! The sheet a brain is added or edited in: a small modal over the window.
//!
//! Adding goes in two steps. First a preset, from a grid of the providers
//! the core offers (`BrainList::presets`); then its few fields: the model,
//! offered from the provider's own list once its key is there; the
//! variable the key is in, with whether the core has it; and, folded away,
//! the base URL and the name it is saved under. Test asks it to think once.
//! Editing a brain opens the sheet at its fields, with a way to remove it.
//! Everything shown is the core's answer: it holds the keys, asks the
//! provider and writes the brain into its own configuration (`link::brains`).
//! The key itself is never shown, and never read by the desk.

use ghost_link::protocol::{BrainAsk, BrainModel, BrainSave, code};
use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, Hsla, SharedString,
    Stateful, Window, actions, deferred, div, prelude::*, px,
};

use crate::{
    brains::{self, Draft, Preset, Wire},
    desk::Desk,
    fonts::Faces,
    input::{Cancelled, Changed, Submitted, TextInput},
    link::{BrainAnswer, BrainCall, Refused},
    solver::Provider,
    style::{CAPTION, FINE, META, NAME},
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
    Failed(String),
}

/// Whether the core has a key in the variable the fields name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyStatus {
    /// Not known yet: the core is being asked.
    Unknown,
    Found,
    NotSet,
}

impl KeyStatus {
    /// What asking the core for a brain's models says of its key: a key
    /// that is not set is refused before anything is sent; anything the
    /// provider answers, even a refusal, means one was there to send.
    pub(crate) fn from_models(answer: &Result<BrainAnswer, Refused>) -> Self {
        match answer {
            Ok(_) => Self::Found,
            Err(Refused {
                code: Some(code::NO_KEY),
                ..
            }) => Self::NotSet,
            Err(Refused {
                code: Some(code::BRAIN_FAILED),
                ..
            }) => Self::Found,
            Err(_) => Self::Unknown,
        }
    }
}

// The few plain states of a small form read better as fields than folded
// into one enum.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Sheet {
    pub open: bool,
    /// The preset being filled in; `None` is the first step, the grid.
    pub preset: Option<Preset>,
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
            this.sheet.generation += 1;
            this.sheet.models = Models::Unasked;
            this.sheet.listing = false;
            cx.notify();
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
            key_status: KeyStatus::Unknown,
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
        self.refresh_brains(1, cx);
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
    fn pick_preset(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<Self>) {
        // A CLI is signed in to once; its tile opens the one there is.
        if preset.cli()
            && let Some(p) = self.providers.iter().find(|p| p.kind == preset.kind())
        {
            let p = p.clone();
            self.edit_brain(&p, window, cx);
            return;
        }
        let taken = |n: &str| self.providers.iter().any(|p| p.name == n);
        let name = (1..100)
            .map(|i| {
                if i == 1 {
                    preset.id().to_owned()
                } else {
                    format!("{}-{i}", preset.id())
                }
            })
            .find(|n| !taken(n))
            .unwrap_or_default();
        let draft = Draft {
            model: preset.model().to_owned(),
            key_env: preset.key_env().to_owned(),
            base_url: preset.base_url().to_owned(),
        };
        self.fill(preset, None, &draft, &name, window, cx);
    }

    /// Fill the fields from a brain already saved.
    fn edit_brain(&mut self, p: &Provider, window: &mut Window, cx: &mut Context<Self>) {
        let preset = p
            .preset()
            .cloned()
            .unwrap_or_else(|| Preset::of_kind(&p.kind));
        let draft = Draft {
            model: p.model.clone(),
            key_env: p.key_env.clone().unwrap_or_default(),
            // Where the table names none, the daemon's default is the
            // preset's own: OpenAI's or Anthropic's.
            base_url: p
                .base_url
                .clone()
                .unwrap_or_else(|| preset.base_url().to_owned()),
        };
        self.fill(preset, Some(p.name.clone()), &draft, &p.name, window, cx);
    }

    fn fill(
        &mut self,
        preset: Preset,
        editing: Option<String>,
        draft: &Draft,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet.reset();
        self.sheet.advanced = !preset.cli() && draft.base_url.is_empty();
        let placeholder = if preset.cli() && preset.model().is_empty() {
            "default"
        } else {
            "model"
        };
        self.sheet.preset = Some(preset);
        self.sheet.editing = editing;
        for (field, text) in [
            (&self.sheet.model, draft.model.as_str()),
            (&self.sheet.key, draft.key_env.as_str()),
            (&self.sheet.base, draft.base_url.as_str()),
            (&self.sheet.name, name),
        ] {
            field.update(cx, |f, cx| f.set_text(text, cx));
        }
        self.sheet
            .model
            .update(cx, |f, cx| f.set_placeholder(placeholder, cx));
        self.sheet.model.focus_handle(cx).focus(window);
        // What the core said of a brain saved is known at once; anything
        // else is asked, with its model list, which says it too.
        self.refresh_key(cx);
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
        match self.sheet.preset.as_ref().map(Preset::wire) {
            Some(Wire::Anthropic) => "ANTHROPIC_API_KEY".to_owned(),
            Some(Wire::OpenAi) => "OPENAI_API_KEY".to_owned(),
            _ => String::new(),
        }
    }

    /// Learn again whether the core has the key the fields name: from what
    /// it said of the brain being edited, if the variable is the one it
    /// reads; otherwise by asking it for the models, which it answers only
    /// with a key.
    fn refresh_key(&mut self, cx: &mut Context<Self>) {
        let Some(cli) = self.sheet.preset.as_ref().map(Preset::cli) else {
            return;
        };
        let var = self.key_var(cx);
        let saved = self
            .sheet
            .editing
            .as_deref()
            .and_then(|name| self.providers.iter().find(|p| p.name == name))
            .filter(|p| p.key_env.as_deref() == Some(var.as_str()));
        self.sheet.key_status = match saved {
            Some(p) if p.key_set => KeyStatus::Found,
            Some(_) => KeyStatus::NotSet,
            None => KeyStatus::Unknown,
        };
        if !cli {
            self.fetch_models(cx);
        }
        cx.notify();
    }

    /// The brain as the fields describe it, for the core: what a new one
    /// would be written with, or, for one being edited, the fields laid
    /// over what is saved.
    fn sheet_ask(&self, model: Option<String>, cx: &Context<Self>) -> Option<BrainAsk> {
        let preset = self.sheet.preset.as_ref()?;
        let draft = Draft {
            model: self.sheet.model.read(cx).text(),
            key_env: self.sheet.key.read(cx).text(),
            base_url: self.sheet.base.read(cx).text(),
        };
        Some(BrainAsk {
            name: self.sheet.editing.clone(),
            provider: Some(preset.fields(&draft)),
            model,
        })
    }

    /// Ask the core `call` for the sheet, and hand its answer to `done`,
    /// unless the sheet has moved on in the meantime.
    fn sheet_call(
        &mut self,
        call: BrainCall,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<BrainAnswer, Refused>) + 'static,
    ) {
        let generation = self.sheet.generation;
        self.ask_brains(call, cx, move |this, answer, _| {
            if this.sheet.open && this.sheet.generation == generation {
                done(this, answer);
            }
        });
    }

    /// The same, for an answer that closes the sheet, which needs the
    /// window to give the keyboard back.
    fn sheet_call_in(
        &mut self,
        call: BrainCall,
        window: &Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<BrainAnswer, Refused>, &mut Window, &mut Context<Self>)
        + 'static,
    ) {
        let generation = self.sheet.generation;
        let task = crate::link::runtime().spawn(self.brains(call));
        cx.spawn_in(window, async move |this, cx| {
            let answer = task.await.unwrap_or_else(|e| {
                Err(Refused {
                    code: None,
                    why: e.to_string(),
                })
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.sheet.open && this.sheet.generation == generation {
                    done(this, answer, window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Ask the core for the provider's models, which says too whether it
    /// has the key.
    fn fetch_models(&mut self, cx: &mut Context<Self>) {
        let Some(ask) = self.sheet_ask(None, cx) else {
            return;
        };
        self.sheet.generation += 1;
        self.sheet.models = Models::Asking;
        self.sheet_call(BrainCall::Models(ask), cx, |this, answer| {
            let keyed = this.sheet.preset.as_ref().is_some_and(Preset::keyed);
            if keyed {
                this.sheet.key_status = KeyStatus::from_models(&answer);
            }
            this.sheet.models = match answer {
                Ok(BrainAnswer::Models(listed)) if !listed.is_empty() => {
                    Models::Listed(listed.into_iter().map(|m: BrainModel| m.id).collect())
                }
                Ok(_) => Models::Failed("no models listed".to_owned()),
                Err(refused) => Models::Failed(refused.why),
            };
        });
    }

    /// Unfold the model list, asking for it first if it is not there.
    fn toggle_models(&mut self, cx: &mut Context<Self>) {
        self.sheet.listing = !self.sheet.listing;
        if self.sheet.listing && !matches!(self.sheet.models, Models::Listed(_) | Models::Asking) {
            self.fetch_models(cx);
        }
        cx.notify();
    }

    /// One real request, made by the core: the brain the fields describe
    /// is asked to think once, as her mind would ask it.
    fn test_brain(&mut self, cx: &mut Context<Self>) {
        let Some(preset) = self.sheet.preset.clone() else {
            return;
        };
        let model = self.sheet.model.read(cx).text();
        let Some(ask) = self.sheet_ask(Some(model), cx) else {
            return;
        };
        self.sheet.generation += 1;
        self.sheet.refused = None;
        self.sheet.probe = Probe::Running;
        self.sheet_call(BrainCall::Check(ask), cx, move |this, answer| {
            this.sheet.probe = match answer {
                Ok(_) => Probe::Passed("ok · it answered".to_owned()),
                Err(refused) if preset.cli() => {
                    Probe::Failed(brains::cli_failure(preset.kind(), &refused.why))
                }
                Err(refused) => Probe::Failed(refused.why),
            };
        });
        cx.notify();
    }

    /// Have the core write the brain into its configuration, and think with
    /// it if `use_it`. A change to the brain in use the core takes up by
    /// itself.
    fn save_brain(&mut self, use_it: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = self.sheet.preset.clone() else {
            return;
        };
        let draft = Draft {
            model: self.sheet.model.read(cx).text(),
            key_env: self.sheet.key.read(cx).text(),
            base_url: self.sheet.base.read(cx).text(),
        };
        let typed = self.sheet.name.read(cx).text();
        let name = if typed.is_empty() {
            preset.id().to_owned()
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
        let save = save_of(&preset, self.sheet.editing.clone(), &name, &draft);
        self.sheet.generation += 1;
        self.sheet.refused = None;
        self.sheet_call_in(
            BrainCall::Save(save),
            window,
            cx,
            move |this, answer, window, cx| match answer {
                Ok(BrainAnswer::List(list)) => {
                    this.take_brains(list);
                    this.close_sheet(window, cx);
                    if use_it {
                        this.use_brain(&name, cx);
                    }
                }
                Ok(_) => {}
                Err(refused) => this.sheet.refused = Some(refused.why),
            },
        );
        cx.notify();
    }

    /// Have the core take the brain being edited out.
    fn remove_brain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.sheet.editing.clone() else {
            return;
        };
        self.sheet.generation += 1;
        self.sheet.refused = None;
        self.sheet_call_in(
            BrainCall::Remove(name),
            window,
            cx,
            |this, answer, window, cx| match answer {
                Ok(BrainAnswer::List(list)) => {
                    this.take_brains(list);
                    this.close_sheet(window, cx);
                }
                Ok(_) => {}
                Err(refused) => this.sheet.refused = Some(refused.why),
            },
        );
        cx.notify();
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
        let body = match &self.sheet.preset {
            None => self.presets_grid(faces, cx),
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
            .w(px(WIDE))
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
        let title: AnyElement = match &self.sheet.preset {
            None => ui::text(faces, "Add brain")
                .text_size(px(NAME))
                .font_weight(FontWeight::MEDIUM)
                .into_any_element(),
            Some(preset) => {
                let name = self
                    .sheet
                    .editing
                    .clone()
                    .unwrap_or_else(|| preset.name().to_owned());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .child(ui::mark(faces, preset.mark(), 16.))
                    .child(
                        ui::text(faces, name)
                            .text_size(px(NAME))
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

    /// The first step: a tile for each preset the core offers.
    fn presets_grid(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap(px(8.)).px(px(16.)).pb(px(16.));
        for preset in &self.presets {
            let picked = preset.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("preset-{}", preset.id())))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    // Four to a row across the sheet, with nothing left
                    // over at the right.
                    .w(px(94.))
                    .h(px(68.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(theme::border())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()).border_color(theme::border2()))
                    .child(ui::mark(faces, preset.mark(), 20.))
                    .child(
                        ui::text(faces, preset.name().to_owned())
                            .text_size(px(CAPTION))
                            .text_color(theme::text2()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_preset(picked.clone(), window, cx);
                    })),
            );
        }
        grid
    }

    /// The second step: the few fields a brain has, and what to do with them.
    fn brain_fields(&self, preset: &Preset, faces: &Faces, cx: &mut Context<Self>) -> Div {
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
        if preset.keyed() {
            let (word, color, tip) = match self.sheet.key_status {
                KeyStatus::Found => ("found", theme::green(), "the core has it"),
                KeyStatus::NotSet => (
                    "not set",
                    theme::accent(),
                    "not in the core's environment or its .env",
                ),
                KeyStatus::Unknown => ("…", theme::muted(), "asking the core"),
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
                        .text_size(px(CAPTION))
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
            (None, Probe::Failed(s)) => Some((s.clone(), theme::coral())),
            (None, Probe::Idle) => preset.keyed().then(|| {
                (
                    "Only the variable's name is saved, never the key.".to_owned(),
                    theme::faint(),
                )
            }),
        };
        body = body.children(line.map(|(text, color)| {
            ui::one_line(
                ui::text(faces, ui::fit(&text, ui::cols(ROOM)))
                    .text_size(px(CAPTION))
                    .text_color(color),
            )
            .h(px(18.))
        }));

        // Remove at the left, when editing; Test, Save, Save & use at the right.
        let remove = self.sheet.editing.is_some().then(|| {
            div()
                .id("remove-brain")
                .cursor_pointer()
                .child(
                    ui::text(faces, "Remove")
                        .text_size(px(META))
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
                                ui::mono(faces, ui::fit(id, ui::cols(ROOM - LABEL - 20.)))
                                    .text_size(px(CAPTION))
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
                list = list.child(note(faces, why, theme::coral()));
            }
            Models::Unasked => {}
        }
        list
    }
}

/// The width of a field's label.
const LABEL: f32 = 92.;
/// The sheet's width, and the room its lines have inside its insets.
const WIDE: f32 = 440.;
const ROOM: f32 = WIDE - 32.;

/// A label and its control, on one line.
fn field_row(faces: &Faces, label: &'static str, control: Div) -> Div {
    div()
        .flex()
        .items_center()
        .child(
            ui::text(faces, label)
                .w(px(LABEL))
                .flex_none()
                .text_size(px(META))
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
                .text_size(px(FINE))
                .text_color(color),
        )
}

/// A line of small print in the model list.
fn note(faces: &Faces, text: &str, color: Hsla) -> Div {
    ui::one_line(
        ui::text(faces, ui::fit(text, ui::cols(ROOM - LABEL - 20.)))
            .text_size(px(CAPTION))
            .text_color(color),
    )
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

/// A button: an outline, or the desk's primary for the one that finishes.
fn button(faces: &Faces, id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    let weight = if primary {
        ui::Weight::Primary
    } else {
        ui::Weight::Secondary
    };
    ui::button(faces, id, label, weight)
}

/// What saving a brain from `preset` with the fields `draft` asks the core
/// to write: every field for a new one; for one saved as `editing`, only
/// the fields the sheet shows, an emptied one taken out. No key is among
/// them: the sheet names the variable, and the core holds the key.
pub(crate) fn save_of(
    preset: &Preset,
    editing: Option<String>,
    name: &str,
    draft: &Draft,
) -> BrainSave {
    let (set, clear) = if editing.is_some() {
        let shown: Vec<(&str, &str)> = draft
            .managed()
            .into_iter()
            .filter(|(k, _)| preset.uses(k))
            .collect();
        (
            shown
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| ((*k).to_owned(), serde_json::Value::from(*v)))
                .collect(),
            shown
                .iter()
                .filter(|(_, v)| v.is_empty())
                .map(|(k, _)| (*k).to_owned())
                .collect(),
        )
    } else {
        (preset.fields(draft), Vec::new())
    };
    BrainSave {
        was: editing,
        name: name.to_owned(),
        set,
        clear,
        key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brains::tests::by_id;

    fn draft(preset: &Preset) -> Draft {
        Draft {
            model: preset.model().to_owned(),
            key_env: preset.key_env().to_owned(),
            base_url: preset.base_url().to_owned(),
        }
    }

    #[test]
    fn a_new_brain_asks_the_core_for_its_whole_table_and_an_edit_for_what_the_sheet_shows() {
        let deepseek = by_id("deepseek");
        let new = save_of(&deepseek, None, "deepseek", &draft(&deepseek));
        assert_eq!(new.was, None);
        assert_eq!(
            serde_json::Value::Object(new.set.clone()),
            serde_json::json!({
                "kind": "openai", "model": "deepseek-flash",
                "base_url": "https://api.deepseek.com", "api_key_env": "DEEPSEEK_API_KEY",
                "json_mode": true
            })
        );
        assert!(new.clear.is_empty() && new.key.is_none());

        let mut changed = draft(&deepseek);
        changed.model = "deepseek-v4-pro".into();
        changed.base_url = String::new();
        let edit = save_of(&deepseek, Some("main".into()), "main", &changed);
        assert_eq!(edit.was.as_deref(), Some("main"));
        assert_eq!(
            serde_json::Value::Object(edit.set),
            serde_json::json!({"model": "deepseek-v4-pro", "api_key_env": "DEEPSEEK_API_KEY"}),
            "what the preset always adds is not written again"
        );
        assert_eq!(edit.clear, ["base_url"], "an emptied field is taken out");
        // A CLI has no address or key to clear.
        let codex = by_id("codex");
        let edit = save_of(&codex, Some("codex".into()), "codex", &draft(&codex));
        assert_eq!(edit.clear, ["model"], "its own default");
    }

    #[test]
    fn what_the_core_answers_for_the_models_says_whether_it_has_the_key() {
        let refused = |code| {
            Err(Refused {
                code,
                why: "…".into(),
            })
        };
        assert_eq!(
            KeyStatus::from_models(&Ok(BrainAnswer::Models(Vec::new()))),
            KeyStatus::Found
        );
        assert_eq!(
            KeyStatus::from_models(&refused(Some(code::NO_KEY))),
            KeyStatus::NotSet
        );
        assert_eq!(
            KeyStatus::from_models(&refused(Some(code::BRAIN_FAILED))),
            KeyStatus::Found,
            "the provider refused it, so there was one to send"
        );
        assert_eq!(
            KeyStatus::from_models(&refused(None)),
            KeyStatus::Unknown,
            "the core could not be reached"
        );
    }
}
