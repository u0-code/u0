//! The brain menu: one menu of the brains the deployment names and the
//! models each offers, opened from the composer, the foot of the sidebar
//! or the environment down the right. Brains on the left, the models of
//! the one looked at on the right; a model is chosen in one click, brain
//! and all.
//!
//! The brains are the core's: which there are, which models each offers
//! and whether one can think are asked of it (`link::brains`), and a brain
//! chosen is chosen by it. It finds a brain's models on its own machine,
//! where the brain runs: what an API lists, or what a command-line agent
//! installed there knows. Whether a brain can run is asked when it is
//! chosen, not shown in advance: the core asks it to think once, as little
//! as there is, the way her mind would (a CLI that answers for its version
//! may still be signed out), and only then has her think with it, starting
//! her mind again on it. If it cannot, the desk says why and goes on with
//! the brain it had.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Context, Corner, KeyDownEvent, SharedString, Window, anchored, deferred, div,
    prelude::*, px,
};

use ghost_link::protocol::{BrainAsk, BrainList, BrainModel};
use tracing::debug;

use super::{Desk, composer::Notice};
use crate::{
    brains,
    brains::Preset,
    fonts::Faces,
    link::{self, BrainAnswer, BrainCall, Refused},
    solver::Provider,
    theme, ui,
};

/// Which menu is open, if one is: the brain picker. Adding and editing a
/// brain is done in the sheet (`sheet.rs`), never inside the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Menu {
    Closed,
    Brains,
}

/// The control the brain menu opened from. It is one menu, anchored to
/// whichever of the three places to change the brain was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// The chip under the line you type into.
    Composer,
    /// The solver's row at the foot of the sidebar.
    Account,
    /// The Brain row of the environment, down the right.
    Aside,
}

/// What the menu shows besides the brains.
#[derive(Default)]
pub(crate) struct Picker {
    /// The brain whose models show on the right.
    looking: Option<String>,
    /// The models each brain offers, by its name, once asked.
    models: HashMap<String, Models>,
    /// The brains whose listing has been asked for afresh this run, so
    /// that a kept one is not asked for again every time it is looked at.
    refreshed: HashSet<String>,
    /// The brain a switch to is being checked.
    checking: Option<String>,
    /// The menu a press outside it just closed, and when: the press that
    /// closes a menu from its own button must not open it again.
    dismissed: Option<(Menu, At, Instant)>,
}

/// A model to choose. An empty `id` is a command-line agent's own
/// default: no model is named to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub id: String,
    pub label: String,
}

/// The models a brain offers as choices, by the names it gives them, the
/// one it is set to first where it does not list that one itself.
fn choices(current: &str, listed: Vec<BrainModel>) -> Vec<Choice> {
    let mut all: Vec<Choice> = listed
        .into_iter()
        .map(|m| {
            let label = match (m.id.is_empty(), m.name.is_empty()) {
                (true, true) => "Default".to_owned(),
                (true, false) => format!("Default · {}", m.name),
                (false, true) => m.id.clone(),
                (false, false) => m.name,
            };
            Choice { id: m.id, label }
        })
        .collect();
    if !current.is_empty() && !all.iter().any(|c| c.id == current) {
        all.insert(
            0,
            Choice {
                id: current.to_owned(),
                label: current.to_owned(),
            },
        );
    }
    all
}

/// A brain's models, as far as they are known.
enum Models {
    Asking,
    Listed(Vec<Choice>),
    /// They could not be listed: only the one it is set to is offered.
    Unlisted(String),
}

impl Desk {
    /// Open a menu from one of the places it opens from, or close it if
    /// it is open there already. An open menu has the keyboard.
    pub(crate) fn open_menu(
        &mut self,
        menu: Menu,
        at: At,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let just_closed =
            self.picker.dismissed.take().is_some_and(|(m, a, when)| {
                m == menu && a == at && when.elapsed().as_millis() < 400
            });
        let same = self.menu == menu && self.menu_at == at;
        if same || just_closed {
            self.close_menu(window, cx);
            return;
        }
        self.menu = menu;
        self.menu_at = at;
        self.menu_focus.focus(window);
        if self.menu == Menu::Brains {
            // What the desk holds shows at once; the core's answer, when
            // it comes, takes its place.
            self.refresh_brains(1, cx);
            let look = self
                .brain
                .clone()
                .or_else(|| self.providers.first().map(|p| p.name.clone()));
            if let Some(name) = look {
                self.look_at(&name, cx);
            }
        }
        cx.notify();
    }

    /// Close the open menu, and give the keyboard back to where you type.
    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu == Menu::Closed {
            return;
        }
        self.menu = Menu::Closed;
        if self.menu_focus.is_focused(window) {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// A key pressed while the menu is open: Esc closes it; up and down go
    /// between brains, a number picks a model, Return the brain looked at.
    fn menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        if keys.modifiers.platform || keys.modifiers.control || keys.modifiers.alt {
            return;
        }
        let digit = keys
            .key
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=9).contains(n));
        match (self.menu, keys.key.as_str(), digit) {
            (_, "escape", _) => self.close_menu(window, cx),
            (Menu::Brains, "up" | "down", _) => {
                let at = self
                    .picker
                    .looking
                    .as_deref()
                    .and_then(|n| self.providers.iter().position(|p| p.name == n))
                    .unwrap_or(0);
                let next = if keys.key == "up" {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|i| *i < self.providers.len())
                };
                if let Some(name) = next
                    .and_then(|i| self.providers.get(i))
                    .map(|p| p.name.clone())
                {
                    self.look_at(&name, cx);
                }
            }
            (Menu::Brains, "enter", _) => {
                if let Some(name) = self.picker.looking.clone() {
                    self.focus_composer(window, cx);
                    self.use_brain(&name, cx);
                }
            }
            (Menu::Brains, _, Some(n)) => {
                let Some(p) = self
                    .picker
                    .looking
                    .as_deref()
                    .and_then(|name| self.providers.iter().find(|p| p.name == name))
                else {
                    return;
                };
                if let Some(c) = self.choices(p).0.get(n - 1) {
                    let (name, model) = (p.name.clone(), c.id.clone());
                    self.focus_composer(window, cx);
                    self.use_model(&name, &model, cx);
                }
            }
            _ => {}
        }
    }

    /// Show the models of the brain `name` on the right.
    fn look_at(&mut self, name: &str, cx: &mut Context<Self>) {
        self.picker.looking = Some(name.to_owned());
        self.list_models(name, cx);
        cx.notify();
    }

    /// Ask the core for the models of every brain it can run, in the
    /// background, so that the picker has them before it is opened.
    pub(crate) fn warm_models(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .providers
            .iter()
            .filter(|p| p.blocker.is_none())
            .map(|p| p.name.clone())
            .collect();
        for name in names {
            self.list_models(&name, cx);
        }
    }

    /// Learn which models the brain `name` offers, from the core, once a
    /// run: a listing that fails later does not take one's place.
    fn list_models(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(current) = self
            .providers
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.model.clone())
        else {
            return;
        };
        if !self.picker.refreshed.insert(name.to_owned()) {
            return;
        }
        if !self.has_listed(name) {
            self.picker.models.insert(name.to_owned(), Models::Asking);
        }
        let ask = BrainAsk {
            name: Some(name.to_owned()),
            ..BrainAsk::default()
        };
        let name = name.to_owned();
        self.ask_brains(BrainCall::Models(ask), cx, move |this, answer, _| {
            let models = match answer {
                Ok(BrainAnswer::Models(listed)) if !listed.is_empty() => {
                    Models::Listed(choices(&current, listed))
                }
                Ok(_) => Models::Unlisted("it lists no models".to_owned()),
                Err(refused) => Models::Unlisted(refused.why),
            };
            if matches!(models, Models::Listed(_)) || !this.has_listed(&name) {
                this.picker.models.insert(name, models);
            }
        });
        cx.notify();
    }

    /// Where the core on this machine listens, and its token.
    pub(crate) fn core_address(&self) -> (String, Option<String>) {
        (self.settings.solver_url.clone(), super::token())
    }

    /// Ask the core on this machine `call` about its brains, on a
    /// connection of its own, and hand what it answered to `done`.
    pub(crate) fn ask_brains(
        &mut self,
        call: BrainCall,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<BrainAnswer, Refused>, &mut Context<Self>) + 'static,
    ) {
        let (url, token) = self.core_address();
        let task = link::runtime().spawn(link::brains(url, token, call));
        cx.spawn(async move |this, cx| {
            let answer = task.await.unwrap_or_else(|e| {
                Err(Refused {
                    code: None,
                    why: e.to_string(),
                })
            });
            let _ = this.update(cx, |this, cx| {
                done(this, answer, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask the core which brains it has and which one she thinks with; if
    /// it cannot be reached, as while it comes up, ask again every half
    /// second, `tries` times in all.
    pub(crate) fn refresh_brains(&mut self, tries: u32, cx: &mut Context<Self>) {
        let first = self.providers.is_empty();
        self.ask_brains(BrainCall::List, cx, move |this, answer, cx| match answer {
            Ok(BrainAnswer::List(list)) => {
                this.take_brains(list);
                // The brains' models, so that the picker has them when
                // opened.
                if first {
                    this.warm_models(cx);
                }
            }
            Ok(_) => {}
            Err(refused) if tries > 1 && refused.code.is_none() => {
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    let _ = this.update(cx, |this, cx| this.refresh_brains(tries - 1, cx));
                })
                .detach();
            }
            Err(refused) => debug!("the core did not say which brains it has: {}", refused.why),
        });
    }

    /// Hold the brains as the core said they are, and the presets it
    /// offers to add one from.
    pub(crate) fn take_brains(&mut self, list: BrainList) {
        self.presets = list.presets.into_iter().map(Preset).collect();
        self.providers = list
            .brains
            .into_iter()
            .map(|b| Provider::new(b, &self.presets))
            .collect();
        self.brain = list.think;
    }

    /// Whether the picker already has a listing for the brain `name`.
    fn has_listed(&self, name: &str) -> bool {
        matches!(self.picker.models.get(name), Some(Models::Listed(_)))
    }

    /// Think with `model` on the brain `name` from now on, once it is clear
    /// the core can run it: the core has it answer one small request, and
    /// only then starts her mind again on it. If it cannot, say why, and
    /// keep the brain there was.
    pub(crate) fn use_model(&mut self, name: &str, model: &str, cx: &mut Context<Self>) {
        self.menu = Menu::Closed;
        let Some(p) = self.providers.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if self.brain.as_deref() == Some(name) && p.model == model {
            cx.notify();
            return;
        }
        self.picker.checking = Some(name.to_owned());
        self.notice = Some(Notice::said(format!("Checking {}…", p.title())));
        if let Some(why) = p.blocker.clone() {
            self.picker.checking = None;
            self.cannot_think(&p, &why);
            cx.notify();
            return;
        }
        let call = BrainCall::Use {
            name: name.to_owned(),
            model: Some(model.to_owned()),
        };
        self.ask_brains(call, cx, move |this, answer, _| {
            this.picker.checking = None;
            match answer {
                Ok(BrainAnswer::List(list)) => {
                    this.take_brains(list);
                    let now = this
                        .current_provider()
                        .map_or_else(|| p.label(), Provider::label);
                    this.notice = Some(Notice::said(format!(
                        "{} thinks with {now} now.",
                        this.solver.name()
                    )));
                }
                Ok(_) => {}
                Err(refused) => this.cannot_think(&p, &refused.why),
            }
        });
        cx.notify();
    }

    /// Say that the brain `p` cannot think, and why, and what she still
    /// thinks with. A CLI that is signed out says how to sign it in, which
    /// stands on its own; any other reason is cut to its gist. Either way
    /// the whole of what was said is kept under Details.
    fn cannot_think(&mut self, p: &Provider, why: &str) {
        let still = self
            .current_provider()
            .map_or_else(|| "no brain".to_owned(), Provider::label);
        let title = p.title();
        let told = match &p.program {
            Some(_) => brains::cli_failure(&p.kind, why),
            None => why.to_owned(),
        };
        let line = if told.starts_with(&title) {
            told
        } else {
            format!("{title} can't think here: {}", brains::gist(&told))
        };
        self.notice = Some(
            Notice::trouble(format!("{line}. Still thinking with {still}."))
                .with_detail(why.to_owned()),
        );
    }

    /// How the brain in use is named on the composer's chip: the brain and
    /// its model, by the model's own name once the brain has listed it; or
    /// that a switch is being checked.
    pub(crate) fn brain_label(&self, p: &Provider) -> String {
        if self.picker.checking.is_some() {
            return "Checking…".to_owned();
        }
        let listed = match self.picker.models.get(&p.name) {
            Some(Models::Listed(c)) => c.iter().find(|c| c.id == p.model).map(|c| c.label.as_str()),
            _ => None,
        };
        let model = listed.unwrap_or(&p.model);
        if model.is_empty() {
            p.title()
        } else {
            format!("{} · {model}", p.title())
        }
    }

    /// Think with the brain `name`, on the model it is set to.
    pub(crate) fn use_brain(&mut self, name: &str, cx: &mut Context<Self>) {
        let model = self
            .providers
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.model.clone())
            .unwrap_or_default();
        self.use_model(name, &model, cx);
    }

    pub(crate) fn current_provider(&self) -> Option<&Provider> {
        let brain = self.brain.as_deref()?;
        self.providers.iter().find(|p| p.name == brain)
    }

    pub(crate) fn menu_open_at(&self, at: At) -> bool {
        self.menu != Menu::Closed && self.menu_at == at
    }

    /// The menu open from `at`, if one is, anchored by `corner` to where
    /// the caller puts it. It holds the keyboard while it is open, and a
    /// press anywhere else closes it.
    pub(crate) fn menu_over(
        &self,
        at: At,
        corner: Corner,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.menu_open_at(at) {
            return None;
        }
        let panel = self
            .brain_menu(faces, cx)
            .track_focus(&self.menu_focus)
            .on_key_down(cx.listener(Self::menu_key))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.picker.dismissed = Some((this.menu, this.menu_at, Instant::now()));
                this.close_menu(window, cx);
            }));
        Some(
            deferred(
                anchored()
                    .anchor(corner)
                    .snap_to_window_with_margin(px(8.))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The brain picker: the brains down the left, with a check on the one
    /// in use and a pencil that opens one in the sheet; the models of the
    /// one looked at down the right, with a check on the one in use. A
    /// model is chosen in one click. Adding a brain is done in the sheet.
    fn brain_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let mut left = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(250.))
            .p(px(4.))
            .border_r_1()
            .border_color(theme::border())
            .child(ui::menu_head(faces, "Brain"));
        for p in &self.providers {
            left = left.child(self.brain_choice(p, faces, cx));
        }
        if !self.providers.is_empty() {
            left = left.child(div().h(px(1.)).mx(px(4.)).my(px(4.)).bg(theme::border()));
        }
        left = left.child(
            div()
                .id("add-brain")
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(32.))
                .px(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon("plus", 15.).text_color(theme::muted()))
                .child(
                    ui::text(faces, "Add brain…")
                        .text_size(px(13.))
                        .text_color(theme::text2()),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu = Menu::Closed;
                    this.open_sheet(None, window, cx);
                })),
        );
        ui::menu("brain-menu")
            .w(px(540.))
            .max_h(px(420.))
            .child(left)
            .child(self.model_column(faces, cx))
    }

    /// A brain on the left: its mark and name, a check if it is in use.
    /// Clicking it shows its models; its pencil opens it in the sheet.
    fn brain_choice(&self, p: &Provider, faces: &Faces, cx: &mut Context<Self>) -> AnyElement {
        let on = self.brain.as_deref() == Some(p.name.as_str());
        let looking = self.picker.looking.as_deref() == Some(p.name.as_str());
        let group = SharedString::from(format!("brain-{}", p.name));
        let (name, edit) = (p.name.clone(), p.name.clone());
        let pencil = div()
            .id(SharedString::from(format!("brain-edit-{}", p.name)))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(20.))
            .rounded(px(5.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::panel2()))
            .child(
                ui::icon("pencil", 12.)
                    .text_color(gpui::transparent_black())
                    .group_hover(group.clone(), |s| s.text_color(theme::muted())),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.menu = Menu::Closed;
                this.open_sheet(Some(edit.clone()), window, cx);
            }));
        let about = ui::one_line(ui::text(faces, p.about()))
            .text_size(px(11.5))
            .line_height(px(15.))
            .text_color(if p.blocker.is_some() {
                theme::coral()
            } else {
                theme::faint()
            });
        div()
            .id(group.clone())
            .group(group)
            .flex()
            .items_center()
            .gap(px(10.))
            .py(px(6.))
            .pl(px(10.))
            .pr(px(4.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(looking, |d| d.bg(theme::panel2()))
            .hover(|s| s.bg(theme::hover()))
            .child(ui::mark(faces, p.mark(), 16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        ui::text(faces, p.title())
                            .w_full()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.)),
                    )
                    .child(about),
            )
            .child(pencil)
            .child(div().flex_none().w(px(14.)).when(on, |d| {
                d.child(ui::icon("check", 13.).text_color(theme::accent()))
            }))
            .child(
                ui::icon("chevron-right", 11.)
                    .flex_none()
                    .text_color(if looking {
                        theme::muted()
                    } else {
                        theme::faint()
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.look_at(&name, cx)))
            .into_any_element()
    }

    /// The models of the brain looked at, one click each.
    fn model_column(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let column = div()
            .id("brain-models")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .p(px(4.))
            .overflow_y_scroll();
        let Some(p) = self
            .picker
            .looking
            .as_deref()
            .and_then(|n| self.providers.iter().find(|p| p.name == n))
        else {
            return column.child(
                ui::text(faces, "Add a brain to choose its model.")
                    .text_size(px(12.5))
                    .text_color(theme::muted())
                    .p(px(8.)),
            );
        };
        let head = ui::menu_head(faces, format!("Model · {}", p.title()));
        let note = |text: String| {
            ui::text(faces, text)
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme::muted())
                .px(px(10.))
                .py(px(6.))
        };
        let (choices, why) = self.choices(p);
        let in_use = self.brain.as_deref() == Some(p.name.as_str());
        let mut column = column.child(head);
        for (i, c) in choices.iter().enumerate() {
            let on = in_use && c.id == p.model;
            let (name, model) = (p.name.clone(), c.id.clone());
            column = column.child(
                div()
                    .id(SharedString::from(format!("model-{}-{i}", p.name)))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::one_line(
                        ui::text(faces, c.label.clone())
                            .text_size(px(13.))
                            .text_color(if on { theme::text() } else { theme::text2() }),
                    ))
                    .child(div().flex_none().w(px(14.)).when(on, |d| {
                        d.child(ui::icon("check", 13.).text_color(theme::accent()))
                    }))
                    .when(i < 9, |d| {
                        d.child(ui::key(faces, (i + 1).to_string()).w(px(10.)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus_composer(window, cx);
                        this.use_model(&name, &model, cx);
                    })),
            );
        }
        column.children(why.map(note))
    }

    /// The models the brain `p` offers as far as they are known, and a
    /// note when they are still being asked for or could not be listed.
    fn choices(&self, p: &Provider) -> (std::borrow::Cow<'_, [Choice]>, Option<String>) {
        use std::borrow::Cow;
        let only = || Cow::Owned(choices(&p.model, Vec::new()));
        match self.picker.models.get(&p.name) {
            Some(Models::Listed(c)) => (Cow::Borrowed(c.as_slice()), None),
            Some(Models::Asking) => (only(), Some("Asking for its models…".to_owned())),
            Some(Models::Unlisted(why)) => (
                only(),
                Some(format!("Its models could not be listed: {why}.")),
            ),
            None => (only(), None),
        }
    }
}
