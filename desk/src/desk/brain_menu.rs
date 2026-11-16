//! The brain menu: one menu of the brains the deployment names and the
//! models each offers, opened from the composer, the foot of the sidebar
//! or the environment down the right. Brains on the left, the models of
//! the one looked at on the right; a model is chosen in one click, brain
//! and all.
//!
//! The composer's Mode menu is drawn here too, on the same panel: a
//! menu is one thing to open, close and steer from the keyboard, whatever
//! it offers.
//!
//! Whether a brain can run here is asked when it is chosen, not shown in
//! advance: it is asked to think once, as little as there is, the way the
//! daemon would ask it (a CLI that answers for its version may still be
//! signed out). If it cannot, the desk says why and goes on with the brain
//! it had.

use std::{collections::HashMap, time::Instant};

use gpui::{
    AnyElement, Context, Corner, KeyDownEvent, SharedString, Window, anchored, deferred, div,
    prelude::*, px,
};

use super::{
    Desk, Mode,
    composer::{Notice, mode_color},
};
use crate::{
    brains::{self, Endpoint, Wire},
    fonts::Faces,
    link,
    models::{self, Choice},
    solver::{self, Provider},
    theme, ui,
};

/// Which menu is open, if one is: the brain picker, or the composer's
/// modes. Adding and editing a brain is done in the sheet (`sheet.rs`),
/// never inside the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Menu {
    Closed,
    Brains,
    Modes,
}

/// The ways of saying something, in the order the Mode menu offers them
/// and Shift-Tab goes round them, with their names.
pub(crate) const MODES: [(Mode, &str); 3] = [
    (Mode::Say, "Say"),
    (Mode::Directive, "Directive"),
    (Mode::Task, "Task"),
];

/// What a mode does, in a line, to `name`.
fn mode_about(mode: Mode, name: &str) -> String {
    match mode {
        Mode::Say => format!("Talk. {name} answers, and decides what to do."),
        Mode::Directive => format!("An order from you as operator. {name} carries it out."),
        Mode::Task => {
            format!("A piece of work with its checks. {name} ends it when every one holds.")
        }
    }
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
    /// The brain a switch to is being checked.
    checking: Option<String>,
    /// The menu a press outside it just closed, and when: the press that
    /// closes a menu from its own button must not open it again.
    dismissed: Option<(Menu, At, Instant)>,
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
            self.providers = self.deployment.providers();
            self.brain = self.deployment.brain();
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

    /// A key pressed while a menu is open: Esc closes it; in the Mode
    /// menu a number picks a mode; in the brain menu up and down go
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
            (Menu::Modes, _, Some(n)) => {
                if let Some((mode, ..)) = MODES.get(n - 1) {
                    self.menu = Menu::Closed;
                    self.set_mode(*mode, window, cx);
                }
            }
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

    /// Learn which models the brain `name` offers, once: a CLI's from what
    /// it keeps on this machine, an API's from its model list.
    fn list_models(&mut self, name: &str, cx: &mut Context<Self>) {
        if matches!(
            self.picker.models.get(name),
            Some(Models::Asking | Models::Listed(_))
        ) {
            return;
        }
        let Some(p) = self.providers.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if let Some(choices) = models::of_cli(&p.kind, home.as_deref()) {
            self.picker
                .models
                .insert(name.to_owned(), Models::Listed(choices));
            return;
        }
        let (network, end) = match self.endpoint(&p, &p.model) {
            Ok(found) => found,
            Err(why) => {
                self.picker
                    .models
                    .insert(name.to_owned(), Models::Unlisted(why));
                return;
            }
        };
        self.picker.models.insert(name.to_owned(), Models::Asking);
        let task = link::runtime().spawn(async move { brains::models(&network, &end).await });
        let name = name.to_owned();
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |this, cx| {
                let models = match answer {
                    Ok(Ok(ids)) if !ids.is_empty() => Models::Listed(models::of_api(&p.model, ids)),
                    Ok(Ok(_)) => Models::Unlisted("it lists no models".to_owned()),
                    Ok(Err(why)) => Models::Unlisted(why.line()),
                    Err(e) => Models::Unlisted(e.to_string()),
                };
                this.picker.models.insert(name, models);
                cx.notify();
            });
        })
        .detach();
    }

    /// How to reach the API `p` with `model`: the network the daemon would
    /// use, and its address and key; or why it cannot be reached.
    fn endpoint(
        &self,
        p: &Provider,
        model: &str,
    ) -> Result<(ghost_net::Network, Endpoint), String> {
        let dotenv = self.deployment.dotenv();
        let key = p
            .key_env
            .as_deref()
            .and_then(|var| brains::key_value(var, &solver::env, dotenv.as_deref()));
        if let (Some(var), None) = (&p.key_env, &key) {
            return Err(format!("{var} is not set"));
        }
        let network = self.deployment.network()?;
        let end = Endpoint {
            wire: Wire::of(&p.kind),
            base_url: p
                .base_url
                .clone()
                .or_else(|| p.preset().map(|x| x.base_url.to_owned()))
                .unwrap_or_default(),
            key,
            model: model.to_owned(),
        };
        Ok((network, end))
    }

    /// Think with `model` on the brain `name` from now on, once it is clear
    /// this machine can run it: it answers one small request, within a
    /// minute. If it cannot, say why, and keep the brain there was.
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
        // One real, small request, as the daemon would make it: a CLI that
        // answers for its version may still be signed out.
        let blocker = p.blocker.clone();
        let api = match &p.program {
            Some(_) => None,
            None => Some(self.endpoint(&p, model)),
        };
        let (kind, program, asked) = (p.kind.clone(), p.program.clone(), model.to_owned());
        let task = link::runtime().spawn(async move {
            if let Some(why) = blocker {
                return Err(why);
            }
            match (program, api) {
                (Some(program), _) => brains::cli_probe(&kind, &program, &asked).await,
                (None, Some(Ok((network, end)))) => {
                    let deadline = std::time::Duration::from_secs(60);
                    match tokio::time::timeout(deadline, brains::probe(&network, &end)).await {
                        Ok(probed) => probed.map(drop),
                        Err(_) => Err("it did not answer in 60s".to_owned()),
                    }
                }
                (None, Some(Err(why))) => Err(why),
                (None, None) => Ok(()),
            }
        });
        let (name, model) = (name.to_owned(), model.to_owned());
        cx.spawn(async move |this, cx| {
            let answer = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.picker.checking = None;
                match answer {
                    Ok(()) => this.switch(&p, &name, &model, cx),
                    Err(why) => {
                        let still = this
                            .current_provider()
                            .map_or_else(|| "no brain".to_owned(), Provider::label);
                        // A CLI that is signed out says how to sign it in,
                        // which stands on its own; any other reason is cut
                        // to its gist. Either way the whole of what was
                        // said is kept under Details.
                        let title = p.title();
                        let told = match &p.program {
                            Some(_) => brains::cli_failure(&p.kind, &why),
                            None => why.clone(),
                        };
                        let line = if told.starts_with(&title) {
                            told
                        } else {
                            format!("{title} can't think here: {}", brains::gist(&told))
                        };
                        this.notice = Some(
                            Notice::trouble(format!("{line}. Still thinking with {still}."))
                                .with_detail(why),
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Write the choice into the deployment and start the solver again.
    fn switch(&mut self, p: &Provider, name: &str, model: &str, cx: &mut Context<Self>) {
        let written = if p.model == model {
            Ok(())
        } else {
            self.deployment.set_model(name, model)
        }
        .and_then(|()| self.deployment.choose(name));
        if let Err(e) = written {
            self.notice = Some(Notice::trouble(format!("{e:#}")));
            cx.notify();
            return;
        }
        self.brain = Some(name.to_owned());
        self.providers = self.deployment.providers();
        self.restart(cx);
    }

    /// How the brain in use is named on the composer's chip: the brain and
    /// its model, by the model's own name where the desk knows it; or that
    /// a switch is being checked.
    pub(crate) fn brain_label(&self, p: &Provider) -> String {
        if self.picker.checking.is_some() {
            return "Checking…".to_owned();
        }
        let listed = match self.picker.models.get(&p.name) {
            Some(Models::Listed(c)) => c.iter().find(|c| c.id == p.model).map(|c| c.label.as_str()),
            _ => None,
        };
        // Drawn every frame, so nothing is read or listed for it: Claude
        // Code's names are known, Codex's once the menu has listed them.
        let model = listed
            .or_else(|| models::cli_label(&p.kind, &p.model))
            .unwrap_or(&p.model);
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
        let panel = match self.menu {
            Menu::Modes => self.mode_menu(faces, cx),
            _ => self.brain_menu(faces, cx),
        }
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

    /// The Mode menu: each way of saying something, what it does, the key
    /// that picks it, and a check on the one in use.
    fn mode_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let name = self.speaking_to();
        let mut list = div()
            .flex()
            .flex_col()
            .w(px(360.))
            .p(px(4.))
            .child(ui::menu_head(faces, "Mode"));
        for (i, (mode, word)) in MODES.into_iter().enumerate() {
            let on = self.mode == mode;
            list = list.child(
                div()
                    .id(SharedString::from(format!("mode-{word}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::panel2()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(1.))
                            .child(
                                ui::text(faces, word)
                                    .text_size(px(13.5))
                                    .text_color(mode_color(mode, true)),
                            )
                            .child(
                                ui::text(faces, mode_about(mode, &name))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(theme::muted()),
                            ),
                    )
                    .child(div().flex_none().w(px(14.)).when(on, |d| {
                        d.child(ui::icon("check", 13.).text_color(theme::accent()))
                    }))
                    .child(ui::key(faces, (i + 1).to_string()).w(px(10.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.menu = Menu::Closed;
                        this.set_mode(mode, window, cx);
                    })),
            );
        }
        ui::menu("mode-menu").child(list)
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
        // In a column, a line that trails off fills the width, rather than
        // growing down as `ui::one_line` would have it.
        let about = ui::text(faces, p.about())
            .w_full()
            .truncate()
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
        let only = || Cow::Owned(models::of_api(&p.model, Vec::new()));
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
