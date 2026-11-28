//! The model picker, u0ui's `ModelPicker`, as T3 Code draws it: the
//! agents down a rail, your favourites over them; the chosen one's models
//! in the middle; how hard it thinks and its other options at the right;
//! its mode at the foot. A session that has begun is its agent's: ACP hands
//! nothing over, so the other agents wait for a new session.

use std::rc::Rc;

use agent_client_protocol::v1::schema::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory as Category,
    SessionConfigSelectOptions,
};
use gpui::component::{Selectable as _, kbd::Kbd, popover::Popover};
use gpui::{Anchor, App, Context, Entity, SharedString, Window, div, prelude::*, px};
use u0ui::icons::CarbonIcon;
use u0ui::t3::composer::{
    ComposerControl, EffortGlyph, EffortLevel, ModelPicker, ModelPickerTexts, PickerChoice,
    PickerMark, PickerModel, PickerOption, PickerProvider, PickerSwitch,
};

use super::{composer::over_box, select_choices};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    hub::{Auth, Stage},
    session::Session,
    settings::Favourite,
    ui,
};

/// What the picker leaves of the thread's column, at its edges together.
const PICKER_MARGIN: f32 = 16.;

/// The favourites' tab, among the agents' (whose ids are the registry's).
const FAVOURITES: &str = "\u{2605}favourites";

/// What the rail has chosen: your favourites, or one agent by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Rail {
    #[default]
    Favourites,
    Agent(String),
}

/// The picker as it is left between draws: the rail's choice, and the
/// row the keys have lit.
#[derive(Default)]
pub struct Picker {
    pub rail: Rail,
    pub at: usize,
}

/// A model the list offers: its agent, the agent's option it is a value
/// of, and whether it is the session's now or no longer listed.
#[derive(Clone)]
struct Model {
    agent: String,
    config: String,
    value: String,
    name: String,
    /// What its agent says of it.
    about: Option<String>,
    current: bool,
    gone: bool,
}

impl Desk {
    /// The model chip in the composer's footer (u0ui's `ComposerControl`):
    /// the agent's mark and its model, opening the picker.
    pub(crate) fn model_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let base =
            Self::option_name(s, &Category::Model).unwrap_or_else(|| self.name_of(&s.meta.agent));
        let model = if !self.composer.draft.also_run.is_empty() && s.is_draft() {
            format!("{base} +{}", self.composer.draft.also_run.len())
        } else {
            base
        };
        let open = self.picker_open();
        let chip = ComposerControl::new("chip-model", crate::tr("Choose the model"))
            .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 16.))
            .child(ui::fit(&model, 28));
        let search = gpui::Focusable::focus_handle(self.menu_search.read(cx), cx);
        let (desk, drawn, faces) = (cx.entity(), cx.entity(), faces.clone());
        over_box(Popover::new("model-picker"), Anchor::BottomLeft)
            .boundary(self.column)
            .p(px(0.))
            .open(open)
            .track_focus(&search)
            .trigger(chip.selected(open))
            .on_open_change(move |open, window, cx| {
                desk.update(cx, |d, cx| {
                    if *open {
                        d.open_model(window, cx);
                    } else if d.popup == Popup::Model {
                        d.close_popup(window, cx);
                    }
                });
            })
            .content(move |_, _, cx| drawn.update(cx, |d, cx| d.picker(&faces, cx)))
    }

    /// How hard the agent thinks, as bars (`EffortGlyph` in u0ui's
    /// `ComposerControl`), where its agent has a thought level to choose.
    pub(super) fn effort_chip(
        &self,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.composer.draft.also_run.is_empty() && s.is_draft() {
            return None;
        }
        let (effort, levels) = (s.options.iter())
            .find(|o| o.category.as_ref() == Some(&Category::ThoughtLevel))
            .and_then(|o| crate::thread::select_choices(&o.kind))?;
        let at = levels.iter().position(|(_, _, on)| *on).unwrap_or_default();
        let level = EffortLevel::among(at, levels.len());
        let open = self.picker_open();
        let desk = cx.entity();
        Some(
            ComposerControl::new("chip-effort", SharedString::from(effort))
                .active(open)
                .child(EffortGlyph::new(level).fast(speed(&s.options).is_some_and(|x| x.on)))
                .on_click(move |window, cx| {
                    desk.update(cx, |this, cx| {
                        if this.picker_open() {
                            this.close_popup(window, cx);
                        } else {
                            this.open_model(window, cx);
                        }
                    });
                })
                .into_any_element(),
        )
    }

    /// Whether the picker is open, or a menu within it.
    fn picker_open(&self) -> bool {
        matches!(
            self.popup,
            Popup::Model | Popup::ModelMode | Popup::Option(_)
        )
    }

    /// Open the picker (⇧⌘M): on your favourites when one can be picked
    /// now, else on the session's agent, the only one once it has begun.
    pub fn open_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let draft = s.is_draft();
        let any = self.settings.favourites.iter().any(|f| {
            self.slot(s.host, &f.agent)
                .is_some_and(|x| x.usable() && !matches!(x.auth, Auth::Needed(_)))
        });
        self.picker.rail = if draft && any {
            Rail::Favourites
        } else {
            Rail::Agent(s.meta.agent.clone())
        };
        self.picker.at = 0;
        self.menu_search.update(cx, |i, cx| {
            i.set_value("", window, cx);
            // The picker says what its search finds, over the field.
            i.set_placeholder("", window, cx);
        });
        self.popup = Popup::Model;
        cx.notify();
    }

    /// The models of `agent`, from its model option: the session's own for
    /// the session's agent, else what a session of it starts with; your own
    /// ids after the agent's; your favourites first.
    fn models_of(&self, s: &Session, agent: &str) -> Vec<Model> {
        let mine = agent == s.meta.agent;
        let options = self.options_of(s, agent);
        let Some(option) = options.iter().find(|o| o.category == Some(Category::Model)) else {
            return Vec::new();
        };
        let Some((current, choices)) = select_choices(&option.kind) else {
            return Vec::new();
        };
        let config = option.id.0.to_string();
        let model = |value: String, name: String, current: bool, gone: bool| Model {
            agent: agent.to_owned(),
            config: config.clone(),
            about: about_of(&option.kind, &value),
            value,
            name,
            current,
            gone,
        };
        let mut models: Vec<Model> = choices
            .into_iter()
            .map(|(value, name, on)| model(value, name, mine && on, false))
            .collect();
        for own in self
            .settings_on(s.host)
            .models
            .get(agent)
            .into_iter()
            .flatten()
        {
            if !models.iter().any(|m| &m.value == own) {
                models.push(model(own.clone(), own.clone(), false, false));
            }
        }
        // The session's model, which its agent no longer lists.
        if mine && !models.iter().any(|m| m.current) {
            models.push(model(current.clone(), current, true, true));
        }
        models.sort_by_key(|m| !self.favourite(m));
        models
    }

    fn favourite(&self, m: &Model) -> bool {
        self.settings
            .favourites
            .iter()
            .any(|f| f.agent == m.agent && f.model == m.value)
    }

    /// The models the list shows: those matching what is typed, across
    /// every agent the session can take; else the rail's.
    fn shown_models(&self, s: &Session, query: &str) -> Vec<Model> {
        let agents: Vec<&str> = if s.is_draft() {
            self.slots_on(s.host).map(|x| x.entry.id.as_str()).collect()
        } else {
            vec![s.meta.agent.as_str()]
        };
        let all = || agents.iter().flat_map(|a| self.models_of(s, a));
        if !query.is_empty() {
            let mut found: Vec<Model> = all()
                .filter(|m| {
                    shown_by(query, &m.value, &m.name)
                        || self.name_of(&m.agent).to_lowercase().contains(query)
                })
                .collect();
            found.sort_by_key(|m| !self.favourite(m));
            return found;
        }
        match &self.picker.rail {
            Rail::Favourites => all().filter(|m| self.favourite(m)).collect(),
            Rail::Agent(a) => self.models_of(s, a),
        }
    }

    /// The picker (u0ui's `ModelPicker`): the agents' tabs, the models
    /// found or the tab's, the session's options as its agent states them,
    /// and its foot; no wider than the thread's column, whose narrowest
    /// leaves the tabs only their marks.
    fn picker(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(s) = self.session() else {
            return div().into_any_element();
        };
        let (host, draft) = (s.host, s.is_draft());
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        let models = Rc::new(self.shown_models(s, &query));
        let several = !query.is_empty() || self.picker.rail == Rail::Favourites;
        let mark = |agent: &str| -> PickerMark {
            let (faces, mark) = (faces.clone(), self.mark_of(agent));
            Rc::new(move |size| ui::mark(&faces, &mark, size))
        };
        let mut tabs = vec![
            PickerProvider::new(FAVOURITES, crate::tr("Favourites")).icon(CarbonIcon::StarFilled),
        ];
        for slot in self.slots_on(host) {
            let id = slot.entry.id.as_str();
            let name = self.name_of(id);
            let said = |english: &str| crate::tr_with(english, &[("name", &name)]);
            let mut tab = PickerProvider::new(id.to_owned(), name.clone()).mark(mark(id));
            // The tab's note says what it is doing or what went wrong; the
            // notice over its models, what it needs of you.
            tab = match &slot.stage {
                Stage::Starting => tab.starting(true).notice_short(crate::tr("Starting")),
                Stage::Installing(_) => tab.notice_short(crate::tr("Installing")),
                Stage::Missing(_) => tab.notice_short(crate::tr("Not installed")),
                Stage::Failed(why) => {
                    tab.notice_short(crate::tr("Failed"))
                        .tooltip(crate::tr_with(
                            "%{name}: Failed to start. %{why}",
                            &[("name", &name), ("why", why)],
                        ))
                }
                _ => tab,
            };
            tab = match need_of(&slot.stage, &slot.auth) {
                Some(Need::Settings) => tab
                    .notice(said("%{name} cannot start yet"))
                    .notice_action(crate::tr("Agent settings")),
                Some(Need::SignIn) => tab
                    .notice(said("Sign in to %{name}"))
                    .notice_action(crate::tr("Sign in")),
                None => tab,
            };
            if !draft && id != s.meta.agent {
                tab = tab.disabled(true).tooltip(said(
                    "%{name} can't take over this thread. Start a new thread to use it.",
                ));
            }
            tabs.push(tab);
        }
        let rows = (models.iter().enumerate())
            .map(|(n, m)| {
                let pickable = draft || m.agent == s.meta.agent;
                let also_on = (self.composer.draft.also_run.iter())
                    .any(|(a, v)| a == &m.agent && v.as_deref() == Some(m.value.as_str()));
                let mut row = PickerModel::new(n.to_string(), m.name.clone())
                    .chosen(m.current)
                    .favourite(self.favourite(m))
                    .disabled(!pickable);
                if let Some(about) = &m.about {
                    row = row.description(about.clone());
                }
                if several {
                    row = row.mark(mark(&m.agent));
                }
                if m.gone {
                    row = row.badge(crate::tr("Unavailable"));
                }
                if draft && !m.current {
                    row = row.also(also_on);
                }
                if n < 9 && pickable {
                    row = row.key(Kbd::format(&crate::keys::numbered(n + 1)));
                }
                row
            })
            .collect();
        let (agent, options) = self.options_shown(s, &query);
        let effort = (options.iter())
            .find(|o| o.category == Some(Category::ThoughtLevel))
            .and_then(picker_option);
        let fast = speed(options);
        let others = (options.iter())
            .filter(|o| {
                !matches!(
                    o.category,
                    Some(Category::Model | Category::Mode | Category::ThoughtLevel)
                )
            })
            .filter(|o| fast.as_ref().is_none_or(|x| x.config != *o.id.0))
            .filter_map(picker_option)
            .collect();
        let empty = match (&self.picker.rail, query.is_empty()) {
            (_, false) => "No models found",
            (Rail::Favourites, true) => "No favourites yet",
            (Rail::Agent(_), true) => "It has not said its models yet",
        };
        let texts = ModelPickerTexts {
            needs_setup: crate::tr("Needs sign-in"),
            settings: crate::tr("Agent settings"),
            search: Rc::new(|_| crate::tr("Search models…")),
            speed: crate::tr("Speed"),
            empty: crate::tr(empty),
            add_favourite: crate::tr("Add to favourites"),
            remove_favourite: crate::tr("Remove from favourites"),
            also: crate::tr("Also run on"),
        };
        let tab = match &self.picker.rail {
            Rail::Favourites => FAVOURITES.into(),
            Rail::Agent(a) => SharedString::from(a.clone()),
        };
        let open_option = match &self.popup {
            Popup::Option(id) => Some(SharedString::from(id.clone())),
            _ => None,
        };
        let desk = cx.entity();
        let of = |f: fn(&mut Self, &Model, &mut Window, &mut Context<Self>)| {
            let (desk, models) = (desk.clone(), models.clone());
            move |id: SharedString, window: &mut Window, cx: &mut App| {
                if let Some(m) = id.parse().ok().and_then(|n: usize| models.get(n)) {
                    desk.update(cx, |d, cx| f(d, m, window, cx));
                }
            }
        };
        let mut picker = ModelPicker::new("model-picker", tabs, &self.menu_search)
            .provider(query.is_empty().then_some(tab))
            .models(rows)
            .highlighted(Some(self.picker.at.min(models.len().saturating_sub(1))))
            .levels(effort)
            .speed(fast.as_ref().map(|x| PickerSwitch {
                label: x.name.clone().into(),
                on: x.on,
            }))
            .options(others)
            .open_option(open_option)
            .max_width(self.column.size.width - px(PICKER_MARGIN))
            .texts(texts)
            .on_provider_change(Self::with(&desk, |d, id: SharedString, window, cx| {
                d.picker.rail = if id == FAVOURITES {
                    Rail::Favourites
                } else {
                    Rail::Agent(id.to_string())
                };
                d.picker.at = 0;
                // A tab chosen shows its own models, not what was searched.
                d.menu_search
                    .update(cx, |i, cx| i.set_value("", window, cx));
                cx.notify();
            }))
            .on_choose(of(|d, m, window, cx| {
                d.pick_model(m, window, cx);
                d.close_popup(window, cx);
            }))
            .on_highlight(Self::with(&desk, |d, n: usize, _, cx| {
                if d.picker.at != n {
                    d.picker.at = n;
                    cx.notify();
                }
            }))
            .on_favourite(of(|d, m, _, cx| {
                let f = Favourite {
                    agent: m.agent.clone(),
                    model: m.value.clone(),
                };
                d.toggle_favourite(&f, cx);
            }))
            .on_also(of(|d, m, _, cx| {
                d.toggle_also_run(&m.agent, Some(m.value.clone()), cx);
            }))
            .on_option_change({
                let (desk, agent) = (desk.clone(), agent.clone());
                move |config, value, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.choose_of(&agent, config.to_string(), value.to_string(), cx);
                        d.popup = Popup::Model;
                    });
                }
            })
            .on_option_open(Self::with(&desk, |d, open: Option<SharedString>, _, cx| {
                match open {
                    Some(id) => d.popup = Popup::Option(id.to_string()),
                    None if matches!(d.popup, Popup::Option(_)) => d.popup = Popup::Model,
                    None => {}
                }
                cx.notify();
            }))
            .on_speed_change({
                let (desk, fast) = (desk.clone(), fast.clone());
                move |_, _, cx| {
                    if let Some(Speed { config, other, .. }) = &fast {
                        desk.update(cx, |d, cx| {
                            d.choose_of(&agent, config.clone(), other.clone(), cx);
                            d.popup = Popup::Model;
                        });
                    }
                }
            })
            .on_notice_action(Self::with(&desk, move |d, id: SharedString, window, cx| {
                let Some(slot) = d.slot(host, &id) else {
                    return;
                };
                match need_of(&slot.stage, &slot.auth) {
                    Some(Need::Settings) => {
                        d.close_popup(window, cx);
                        d.open_tab(crate::pages::Tab::Agents, window, cx);
                    }
                    Some(Need::SignIn) => d.sign_in_agent(host, &id, cx),
                    None => {}
                }
            }))
            .on_settings({
                let desk = desk.clone();
                move |window, cx| {
                    desk.update(cx, |d, cx| {
                        d.close_popup(window, cx);
                        d.open_tab(crate::pages::Tab::Agents, window, cx);
                    });
                }
            });
        let usable = self.slots_on(host).filter(|x| x.usable()).count();
        if draft && usable > 1 {
            let all_on = self.composer.draft.also_run.len() + 1 >= usable;
            let agent = s.meta.agent.clone();
            picker = picker.fanout(
                crate::tr_with("Send to all %{n} agents", &[("n", &usable)]),
                Some(crate::tr(
                    "A thread each, begun with the same words, each in a worktree of its own",
                )),
                all_on,
                move |_, cx| {
                    desk.update(cx, |this, cx| {
                        if this.composer.draft.also_run.is_empty() {
                            if !this.worktree_each() {
                                return cx.notify();
                            }
                            this.composer.draft.also_run = (this.others(host, &agent).into_iter())
                                .map(|(id, _)| (id, None))
                                .collect();
                        } else {
                            this.composer.draft.also_run.clear();
                        }
                        cx.notify();
                    });
                },
            );
        }
        if let Some(mode) = self.mode_button(faces, s, cx) {
            picker = picker.mode(mode);
        }
        picker.into_any_element()
    }

    /// A handler of the picker's given what it hands back, run on the desk.
    fn with<T: 'static>(
        desk: &Entity<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let desk = desk.clone();
        move |t, window, cx| desk.update(cx, |d, cx| f(d, t, window, cx))
    }

    /// Add or remove `(agent, model)` among the additional models a draft
    /// runs on ("Also run on").
    pub fn toggle_also_run(&mut self, agent: &str, model: Option<String>, cx: &mut Context<Self>) {
        let at = self
            .composer
            .draft
            .also_run
            .iter()
            .position(|(a, m)| a == agent && *m == model);
        if let Some(at) = at {
            self.composer.draft.also_run.remove(at);
        } else if self.worktree_each() {
            self.composer.draft.also_run.push((agent.to_owned(), model));
        }
        cx.notify();
    }

    /// Before the draft on screen starts on more than one agent: each is
    /// to get a worktree of its own, never all of them one folder. Where
    /// there is no repository to make them from, it stays on one agent,
    /// and says why.
    fn worktree_each(&mut self) -> bool {
        let Some(s) = self.current.and_then(|k| self.sessions.at_mut(k)) else {
            return false;
        };
        if s.git_dir().is_none() {
            self.trouble(
                crate::tr("Several agents at once need a git repository"),
                Some(
                    crate::tr("Each works in a worktree of its own: never several in one folder")
                        .into(),
                ),
                None,
            );
            return false;
        }
        s.wants_worktree = true;
        true
    }

    /// Friendly label for an `(agent, model)` target in `"Also run on"`.
    pub(crate) fn also_label(&self, s: &Session, agent: &str, model: Option<&str>) -> String {
        let agent_name = self.name_of(agent);
        let Some(value) = model else {
            return agent_name;
        };
        let found = self
            .models_of(s, agent)
            .into_iter()
            .find(|m| m.value == value)
            .map_or_else(|| value.to_owned(), |m| m.name);
        format!("{agent_name} · {found}")
    }

    /// Pick `m`: on a new session of another agent, the session becomes
    /// that agent's first.
    fn pick_model(&mut self, m: &Model, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .draft
            .also_run
            .retain(|(a, v)| !(a == &m.agent && v.as_deref() == Some(m.value.as_str())));
        self.choose_of(&m.agent, m.config.clone(), m.value.clone(), cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
    }

    /// Set `agent`'s option `config` to `value`: on a new session of
    /// another agent, the session becomes that agent's first; a session
    /// begun with one agent takes no other's.
    fn choose_of(&mut self, agent: &str, config: String, value: String, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        if agent != s.meta.agent {
            if !s.is_draft() {
                return;
            }
            self.switch_agent(agent, cx);
        }
        self.set_option(config, value, cx);
    }

    /// The options of `agent`: the session's own for the session's agent,
    /// else what a session of it begins with (none while it has not said).
    fn options_of<'a>(&'a self, s: &'a Session, agent: &str) -> &'a [SessionConfigOption] {
        if agent == s.meta.agent {
            &s.options
        } else {
            (self.slot(s.host, agent).and_then(|x| x.probe.as_ref()))
                .map_or(&[], |p| p.options.as_slice())
        }
    }

    /// The options the picker shows beside the models, and whose they
    /// are: those of the agent whose tab is open; else, as models are
    /// searched or favourites shown, the session's own.
    fn options_shown<'a>(
        &'a self,
        s: &'a Session,
        query: &str,
    ) -> (String, &'a [SessionConfigOption]) {
        let agent = whose(&self.picker.rail, query, &s.meta.agent).to_owned();
        let options = self.options_of(s, &agent);
        (agent, options)
    }

    /// Star `f`, or take its star away.
    fn toggle_favourite(&mut self, f: &Favourite, cx: &mut Context<Self>) {
        let list = &mut self.settings.favourites;
        match list.iter().position(|x| x == f) {
            Some(at) => {
                list.remove(at);
            }
            None => list.push(f.clone()),
        }
        self.save_settings();
        cx.notify();
    }

    /// While the picker is open: ↑ and ↓ move through the list, Return
    /// picks and leaves it open for the effort, ⇧⌘↑ and ⇧⌘↓ go through
    /// the rail. True when the key was the picker's.
    pub(crate) fn picker_keys(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.popup != Popup::Model {
            return false;
        }
        let Some(s) = self.session() else {
            return false;
        };
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        let models = self.shown_models(s, &query);
        let m = &k.modifiers;
        match k.key.as_str() {
            "up" | "down" if m.platform && m.shift => {
                let mut tabs = vec![Rail::Favourites];
                tabs.extend((self.slots_on(s.host)).map(|x| Rail::Agent(x.entry.id.clone())));
                let at = tabs
                    .iter()
                    .position(|t| *t == self.picker.rail)
                    .unwrap_or(0);
                let to = if k.key == "up" {
                    at.checked_sub(1).unwrap_or(tabs.len() - 1)
                } else {
                    (at + 1) % tabs.len()
                };
                self.picker.rail = tabs.swap_remove(to);
                self.picker.at = 0;
                self.menu_search
                    .update(cx, |i, cx| i.set_value("", window, cx));
            }
            "up" if !m.modified() => self.picker.at = self.picker.at.saturating_sub(1),
            "down" if !m.modified() => {
                self.picker.at = (self.picker.at + 1).min(models.len().saturating_sub(1));
            }
            "enter" if !m.modified() => {
                if let Some(chosen) = models.get(self.picker.at) {
                    self.pick_model(chosen, window, cx);
                    self.popup = Popup::Model;
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Pick the `n`th model the list shows (⌘1 to ⌘9).
    pub(super) fn pick_nth_model(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        if let Some(chosen) = self.shown_models(s, &query).get(n).cloned() {
            self.pick_model(&chosen, window, cx);
            self.close_popup(window, cx);
        }
    }
}

/// Whose options the picker shows: the agent of the tab open, unless
/// models are searched across every agent or your favourites are shown,
/// when they are the session's agent's (`mine`).
fn whose<'a>(rail: &'a Rail, query: &str, mine: &'a str) -> &'a str {
    match rail {
        Rail::Agent(a) if query.is_empty() => a,
        _ => mine,
    }
}

/// An agent's speed option (ACP's `speed` category, as Codex's Fast mode)
/// where it is a switch: two choices, the standard one listed first, as
/// Codex lists Off before On. The switch shows the choice the agent
/// reports as current, and turns it to the other.
#[derive(Clone)]
struct Speed {
    config: String,
    name: String,
    /// Whether the second, faster choice is the current one.
    on: bool,
    /// The choice the switch turns it to.
    other: String,
}

fn speed(options: &[SessionConfigOption]) -> Option<Speed> {
    let o = options
        .iter()
        .find(|o| o.category == Some(Category::Speed))?;
    let (_, choices) = select_choices(&o.kind)?;
    let [(standard, _, _), (fast, _, on)] = choices.as_slice() else {
        return None;
    };
    Some(Speed {
        config: o.id.0.to_string(),
        name: o.name.clone(),
        on: *on,
        other: if *on { standard } else { fast }.clone(),
    })
}

/// What an agent needs before its models can be picked: its page in the
/// settings (it is not installed, installing, or failed to start), or you
/// to sign in (also while it starts). The tab's notice and its button are
/// both read from it, so they say and do the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    Settings,
    SignIn,
}

fn need_of(stage: &Stage, auth: &Auth) -> Option<Need> {
    match stage {
        Stage::Missing(_) | Stage::Installing(_) | Stage::Failed(_) => Some(Need::Settings),
        _ if matches!(auth, Auth::Needed(_)) => Some(Need::SignIn),
        _ => None,
    }
}

/// An option of the agent's as the picker draws it: its choices in its
/// order, the current one the one it reports.
fn picker_option(o: &SessionConfigOption) -> Option<PickerOption> {
    let (_, choices) = select_choices(&o.kind)?;
    let choices = (choices.into_iter())
        .map(|(value, name, on)| PickerChoice::new(value, name, on))
        .collect();
    Some(PickerOption::new(
        o.id.0.to_string(),
        o.name.clone(),
        choices,
    ))
}

/// What the agent says of its choice `value` of a select option.
fn about_of(kind: &SessionConfigKind, value: &str) -> Option<String> {
    let SessionConfigKind::Select(select) = kind else {
        return None;
    };
    let found = match &select.options {
        SessionConfigSelectOptions::Ungrouped(o) => o.iter().find(|o| &*o.value.0 == value),
        SessionConfigSelectOptions::Grouped(g) => {
            (g.iter().flat_map(|g| g.options.iter())).find(|o| &*o.value.0 == value)
        }
        _ => None,
    };
    found?.description.clone()
}

/// Whether a choice stands when `query` (lower case) is typed.
fn shown_by(query: &str, value: &str, label: &str) -> bool {
    query.is_empty() || label.to_lowercase().contains(query) || value.to_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use super::{Need, Rail, about_of, need_of, picker_option, speed, whose};
    use crate::hub::{Auth, Install, Stage};
    use agent_client_protocol::v1::schema::NewSessionResponse;

    #[test]
    fn codex_fast_mode_is_a_switch_that_turns_to_its_other_choice() {
        // Recorded from @agentclientprotocol/codex-acp 2.10.0: Fast mode on.
        let opened: NewSessionResponse = serde_json::from_str(include_str!(
            "../../../core/src/fixtures/codex-session-new.json"
        ))
        .unwrap();
        let mut options = opened.config_options.unwrap();
        let fast = speed(&options).expect("Codex says it has a speed option");
        assert_eq!(
            (fast.config.as_str(), fast.name.as_str()),
            ("fast-mode", "Fast mode")
        );
        assert!(fast.on);
        assert_eq!(fast.other, "off");

        let at = options.iter().position(|o| &*o.id.0 == "fast-mode");
        let off = serde_json::json!({
            "id": "fast-mode", "name": "Fast mode", "category": "speed",
            "type": "select", "currentValue": "off",
            "options": [{"value": "off", "name": "Off"}, {"value": "on", "name": "On"}],
        });
        options[at.unwrap()] = serde_json::from_value(off).unwrap();
        let fast = speed(&options).unwrap();
        assert!(!fast.on);
        assert_eq!(fast.other, "on");
    }

    #[test]
    fn a_speed_option_of_more_than_two_choices_is_no_switch() {
        let three = serde_json::json!([{
            "id": "speed", "name": "Speed", "category": "speed",
            "type": "select", "currentValue": "a",
            "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"},
                        {"value": "c", "name": "C"}],
        }]);
        assert!(speed(&serde_json::from_value::<Vec<_>>(three).unwrap()).is_none());
    }

    #[test]
    fn codex_options_are_drawn_as_it_states_them() {
        let opened: NewSessionResponse = serde_json::from_str(include_str!(
            "../../../core/src/fixtures/codex-session-new.json"
        ))
        .unwrap();
        let options = opened.config_options.unwrap();
        let of = |id: &str| options.iter().find(|o| &*o.id.0 == id).unwrap();
        // Its effort keeps its own name, its six levels and the current one.
        let effort = picker_option(of("reasoning_effort")).unwrap();
        assert_eq!(effort.name, "Reasoning effort");
        let names: Vec<_> = effort.choices.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["Low", "Medium", "High", "Xhigh", "Max", "Ultra"]);
        let on: Vec<_> = effort.choices.iter().filter(|c| c.on).collect();
        assert_eq!((on.len(), on[0].value.as_ref()), (1, "ultra"));
        // Collaboration mode is neither a level nor a speed: a menu row.
        let collab = picker_option(of("collaboration_mode")).unwrap();
        assert_eq!(collab.choices.len(), 2);
        assert!(collab.choices[0].on);
        // A model row says what Codex says of it.
        assert_eq!(
            about_of(&of("model").kind, "gpt-6-luna").as_deref(),
            Some("Fast and affordable model for easier tasks.")
        );
        assert_eq!(about_of(&of("model").kind, "gpt-unknown"), None);
    }

    #[test]
    fn another_agents_tab_shows_that_agents_options_not_the_sessions() {
        let other = Rail::Agent("gemini".into());
        assert_eq!(whose(&other, "", "codex-acp"), "gemini");
        let own = Rail::Agent("codex-acp".into());
        assert_eq!(whose(&own, "", "codex-acp"), "codex-acp");
        // Searching shows models of every agent, and favourites mix them:
        // the options beside them stay the session's.
        assert_eq!(whose(&other, "flash", "codex-acp"), "codex-acp");
        assert_eq!(whose(&Rail::Favourites, "", "codex-acp"), "codex-acp");
    }

    #[test]
    fn an_agents_notice_asks_for_what_it_needs() {
        let installing = Stage::Installing(Install {
            what: "npm".into(),
            share: None,
        });
        let signed_out = Auth::Needed("Sign in".into());
        // Installing, failed: its page, never a sign-in, signed out or not.
        assert_eq!(need_of(&installing, &signed_out), Some(Need::Settings));
        let failed = Stage::Failed("exit 1".into());
        assert_eq!(need_of(&failed, &Auth::Ready), Some(Need::Settings));
        // Starting and signed out: it still asks you to sign in.
        assert_eq!(need_of(&Stage::Starting, &signed_out), Some(Need::SignIn));
        assert_eq!(need_of(&Stage::Starting, &Auth::Unknown), None);
        assert_eq!(need_of(&Stage::Ready, &Auth::Ready), None);
    }
}
