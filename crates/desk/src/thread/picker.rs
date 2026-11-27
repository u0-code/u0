//! The model picker, as T3 Code draws it: the agents down a rail, your
//! favourites over them; the chosen one's models in the middle; how hard
//! it thinks and its other options at the right; its mode at the foot.
//! A session that has begun is its agent's: ACP hands nothing over, so the
//! other agents wait for a new session.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory as Category;
use gpui::component::{
    Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    kbd::Kbd,
    list::ListItem,
    popover::Popover,
    scroll::ScrollableElement as _,
    separator::Separator,
};
use gpui::{Anchor, Context, Div, SharedString, Stateful, Window, div, prelude::*, px};
use u0ui::t3::composer::ComposerControl;

use super::{composer::over_box, select_choices};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    hub::{Auth, Stage},
    session::{Session, State},
    settings::Favourite,
    theme, ui,
};

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
    current: bool,
    gone: bool,
}

impl Desk {
    /// The model chip in the composer's footer (u0ui's `ComposerControl`):
    /// the agent's mark and its model, opening the picker.
    pub(crate) fn model_chip(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let base =
            Self::option_name(s, &Category::Model).unwrap_or_else(|| self.name_of(&s.meta.agent));
        let model = if !self.composer.draft.also_run.is_empty() && Self::is_draft(s) {
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
        if !self.composer.draft.also_run.is_empty() && Self::is_draft(s) {
            return None;
        }
        let (effort, levels) = (s.options.iter())
            .find(|o| o.category.as_ref() == Some(&Category::ThoughtLevel))
            .and_then(|o| crate::thread::select_choices(&o.kind))?;
        let at = levels.iter().position(|(_, _, on)| *on).unwrap_or_default();
        let level = effort_level_of(at, levels.len());
        let open = self.picker_open();
        let desk = cx.entity();
        Some(
            ComposerControl::new("chip-effort", SharedString::from(effort))
                .active(open)
                .child(u0ui::t3::composer::EffortGlyph::new(level))
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
        let draft = Self::is_draft(s);
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
            i.set_placeholder(crate::tr("Search models…"), window, cx);
        });
        self.popup = Popup::Model;
        cx.notify();
    }

    /// Whether the session has not begun: its agent can still be changed.
    fn is_draft(s: &Session) -> bool {
        s.state == State::Draft && s.items.is_empty()
    }

    /// The models of `agent`, from its model option: the session's own for
    /// the session's agent, else what a session of it starts with; your own
    /// ids after the agent's; your favourites first.
    fn models_of(&self, s: &Session, agent: &str) -> Vec<Model> {
        let mine = agent == s.meta.agent;
        let options = if mine {
            &s.options
        } else {
            match self.slot(s.host, agent).and_then(|x| x.probe.as_ref()) {
                Some(p) => &p.options,
                None => return Vec::new(),
            }
        };
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
            value,
            name,
            current,
            gone,
        };
        let mut models: Vec<Model> = choices
            .into_iter()
            .map(|(value, name, on)| model(value, name, mine && on, false))
            .collect();
        for own in self.settings.models.get(agent).into_iter().flatten() {
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
        let agents: Vec<&str> = if Self::is_draft(s) {
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

    /// The picker: its three columns over its foot.
    fn picker(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some(s) = self.session() else {
            return div();
        };
        let query = self.menu_search.read(cx).value().trim().to_lowercase();
        let models = self.shown_models(s, &query);
        div()
            .flex()
            .flex_col()
            .rounded(px(0.))
            .bg(theme::role(u0ui::tokens::ColorRole::Layer01))
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .h(px(346.))
                    .when(query.is_empty(), |d| d.child(self.rail(faces, s, cx)))
                    .child(self.model_list(faces, s, &query, models, cx))
                    .child(self.efforts(faces, s, cx)),
            )
            .child(self.picker_foot(faces, s, cx))
    }

    /// The rail: your favourites, then each agent switched on, the one
    /// chosen marked at its edge.
    fn rail(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Stateful<Div> {
        let draft = Self::is_draft(s);
        let pill = || {
            div()
                .absolute()
                .top(px(8.))
                .right(px(-4.))
                .w(px(3.))
                .h(px(20.))
                .rounded(px(2.))
                .bg(theme::accent())
        };
        let favourites = self.picker.rail == Rail::Favourites;
        let mut rail = div()
            .id("picker-rail")
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .w(px(44.))
            .p(px(4.))
            .gap(px(4.))
            .bg(theme::rail())
            .overflow_y_scroll()
            .child(
                div()
                    .relative()
                    .child(
                        Button::new("rail-favourites")
                            .ghost()
                            .size(px(36.))
                            .child(ui::icon("star-fill", 18.).text_color(theme::text2()))
                            .tooltip(crate::tr("Favourites"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picker.rail = Rail::Favourites;
                                this.picker.at = 0;
                                cx.notify();
                            })),
                    )
                    .when(favourites, |d| d.child(pill())),
            )
            .child(Separator::horizontal().w(px(28.)));
        for slot in self.slots_on(s.host) {
            let id = slot.entry.id.clone();
            let name = self.name_of(&id);
            let bound = !draft && id != s.meta.agent;
            let needs = matches!(slot.auth, Auth::Needed(_));
            let said = |english: &str| crate::tr_with(english, &[("name", &name)]);
            let tip = if bound {
                said("%{name} can't take over this thread. Start a new thread to use it.")
            } else {
                match &slot.stage {
                    Stage::Starting => said("%{name}: Starting"),
                    Stage::Failed(why) => crate::tr_with(
                        "%{name}: Failed to start. %{why}",
                        &[("name", &name), ("why", why)],
                    ),
                    Stage::Missing(_) | Stage::Installing(_) => said("%{name}: Not installed yet"),
                    _ if needs => said("%{name}: Sign in to use it"),
                    _ => name.clone().into(),
                }
            };
            let chosen = self.picker.rail == Rail::Agent(id.clone());
            let ready = slot.usable() && !needs && !bound;
            let pick = id.clone();
            rail = rail.child(
                div()
                    .relative()
                    .when(!ready, |d| d.opacity(0.5))
                    .child(
                        Button::new(SharedString::from(format!("rail-{id}")))
                            .ghost()
                            .size(px(36.))
                            .disabled(bound)
                            .child(ui::mark(faces, &self.mark_of(&id), 20.))
                            .tooltip(tip)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.picker.rail = Rail::Agent(pick.clone());
                                this.picker.at = 0;
                                cx.notify();
                            })),
                    )
                    .when(chosen, |d| d.child(pill())),
            );
        }
        rail
    }

    /// The middle: the search, and the models found or the rail's.
    fn model_list(
        &self,
        faces: &Faces,
        s: &Session,
        query: &str,
        models: Vec<Model>,
        cx: &mut Context<Self>,
    ) -> Div {
        let draft = Self::is_draft(s);
        let at = self.picker.at.min(models.len().saturating_sub(1));
        let mut list = div()
            .id("picker-models")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .px(px(8.))
            .py(px(6.))
            .gap(px(2.))
            .overflow_y_scrollbar();
        if models.is_empty() {
            let says = match (&self.picker.rail, query.is_empty()) {
                (_, false) => "No models found",
                (Rail::Favourites, true) => "No favourites yet",
                (Rail::Agent(_), true) => "It has not said its models yet",
            };
            list = list.child(
                ui::text(faces, crate::tr(says))
                    .p(px(12.))
                    .text_size(px(crate::style::LABEL))
                    .text_color(theme::faint()),
            );
        }
        for (n, m) in models.into_iter().enumerate() {
            let star = self.favourite(&m);
            let highlighted = n == at;
            let pickable = draft || m.agent == s.meta.agent;
            let favourite = Favourite {
                agent: m.agent.clone(),
                model: m.value.clone(),
            };
            let picked = m.clone();
            let also_on = draft
                && !(m.current && m.agent == s.meta.agent)
                && self
                    .composer
                    .draft
                    .also_run
                    .iter()
                    .any(|(a, v)| a == &m.agent && v.as_deref() == Some(m.value.as_str()));
            let also_target = (m.agent.clone(), Some(m.value.clone()));
            let toggle_also =
                std::rc::Rc::new(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.toggle_also_run(&also_target.0, also_target.1.clone(), cx);
                    cx.stop_propagation();
                }));
            // The star is its own button: a click on it is not a pick.
            let toggle = std::rc::Rc::new(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.toggle_favourite(&favourite, cx);
                cx.stop_propagation();
            }));
            list = list.child(
                ListItem::new(SharedString::from(format!("model-{n}")))
                    .h(px(32.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .selected(highlighted)
                    .disabled(!pickable)
                    .on_mouse_enter(cx.listener(move |this, _, _, cx| {
                        if this.picker.at != n {
                            this.picker.at = n;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_model(&picked, window, cx);
                        this.close_popup(window, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w(px(0.))
                            .child(ui::mark(faces, &self.mark_of(&m.agent), 16.))
                            .child(ui::one_line(
                                ui::text(faces, m.name.clone())
                                    .text_size(px(crate::style::BODY))
                                    .line_height(px(20.)),
                            ))
                            .when(m.current, |d| {
                                d.child(ui::icon("check", 14.).text_color(theme::text2()))
                            })
                            .when(m.gone, |d| {
                                d.child(
                                    ui::text(faces, crate::tr("Unavailable"))
                                        .flex_none()
                                        .px(px(5.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(theme::border2())
                                        .text_size(px(crate::style::CAPTION))
                                        .text_color(theme::faint()),
                                )
                            }),
                    )
                    .suffix(move |_, _| {
                        let toggle = toggle.clone();
                        let toggle_also = toggle_also.clone();
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(4.))
                            .when(draft && !m.current && (highlighted || also_on), |d| {
                                d.child(
                                    ui::icon_button(
                                        SharedString::from(format!("also-{n}")),
                                        if also_on { "check" } else { "plus" },
                                        crate::tr("Also run on"),
                                    )
                                    .xsmall()
                                    .text_color(if also_on { theme::ok() } else { theme::faint() })
                                    .on_click(move |e, window, cx| toggle_also(e, window, cx)),
                                )
                            })
                            .when(highlighted || star, |d| {
                                d.child(
                                    ui::icon_button(
                                        SharedString::from(format!("star-{n}")),
                                        if star { "star-fill" } else { "star" },
                                        crate::tr(if star {
                                            "Remove from favourites"
                                        } else {
                                            "Add to favourites"
                                        }),
                                    )
                                    .xsmall()
                                    .text_color(if star { theme::coral() } else { theme::faint() })
                                    .on_click(move |e, window, cx| toggle(e, window, cx)),
                                )
                            })
                            .when(n < 9 && pickable, |d| {
                                d.child(Kbd::new(crate::keys::numbered(n + 1)))
                            })
                    }),
            );
        }
        let notice = match &self.picker.rail {
            Rail::Agent(a) if query.is_empty() => self.picker_notice(faces, a, cx),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .w(px(316.))
            .flex_none()
            .border_l_1()
            .border_r_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mx(px(12.))
                    .mt(px(10.))
                    .mb(px(4.))
                    .border_b_1()
                    .border_color(theme::border2())
                    .child(ui::icon("search", 16.).text_color(theme::faint()))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.menu_search).appearance(false).small()),
                    ),
            )
            .child(list)
            .children(notice)
    }

    /// Under the list, for an agent that needs you before it can be used.
    fn picker_notice(&self, faces: &Faces, agent: &str, cx: &mut Context<Self>) -> Option<Div> {
        let host = self.session()?.host;
        let slot = self.slot(host, agent)?;
        let name = self.name_of(agent);
        let (says, button) = match &slot.stage {
            Stage::Missing(_) | Stage::Failed(_) => (
                crate::tr_with("%{name} cannot start yet", &[("name", &name)]),
                ui::button(
                    "picker-agents",
                    crate::tr("Agent settings"),
                    ui::Weight::Secondary,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_popup(window, cx);
                    this.open_tab(crate::pages::Tab::Agents, window, cx);
                })),
            ),
            _ if matches!(slot.auth, Auth::Needed(_)) => {
                let agent = agent.to_owned();
                (
                    crate::tr_with("Sign in to %{name}", &[("name", &name)]),
                    ui::button("picker-sign-in", crate::tr("Sign in"), ui::Weight::Primary)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.sign_in_agent(host, &agent, cx)),
                        ),
                )
            }
            _ => return None,
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(10.))
                .border_t_1()
                .border_color(theme::border2())
                .child(
                    ui::text(faces, says)
                        .flex_1()
                        .text_size(px(crate::style::LABEL)),
                )
                .child(button),
        )
    }

    /// The right: how hard it thinks, then every other option it offers.
    fn efforts(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Stateful<Div> {
        let effort = s
            .options
            .iter()
            .find(|o| o.category == Some(Category::ThoughtLevel));
        let others: Vec<_> = s
            .options
            .iter()
            .filter(|o| {
                !matches!(
                    o.category,
                    Some(Category::Model | Category::Mode | Category::ThoughtLevel)
                )
            })
            .filter_map(|o| select_choices(&o.kind).map(|c| (o, c)))
            .collect();
        let head = match (effort, others.is_empty()) {
            (Some(o), _) => o.name.clone(),
            (None, false) => crate::tr("Options").to_string(),
            (None, true) => crate::tr("Effort").to_string(),
        };
        let mut column = div()
            .id("picker-options")
            .flex()
            .flex_col()
            .w(px(192.))
            .flex_none()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(33.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(theme::border2())
                    .child(
                        ui::text(faces, head)
                            .flex_1()
                            .text_size(px(crate::style::BODY))
                            .text_color(theme::muted()),
                    )
                    .children(Self::option_name(s, &Category::Model).map(|m| {
                        ui::one_line(
                            ui::text(faces, m)
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )
                        .w_auto()
                        .max_w(px(90.))
                    })),
            );
        let mut rows = div().flex().flex_col().p(px(6.)).gap(px(2.));
        match effort.map(|o| (o, select_choices(&o.kind))) {
            Some((o, Some((_, choices)))) => {
                let config = o.id.0.to_string();
                for (n, (value, name, on)) in choices.into_iter().enumerate() {
                    let config = config.clone();
                    rows = rows.child(
                        ListItem::new(SharedString::from(format!("effort-{n}")))
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .child(ui::text(faces, name).text_size(px(crate::style::BODY)))
                            .when(on, |d| {
                                d.suffix(|_, _| ui::icon("check", 14.).text_color(theme::text2()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_option(config.clone(), value.clone(), cx);
                                this.popup = Popup::Model;
                            })),
                    );
                }
            }
            _ if others.is_empty() => {
                rows = rows.child(
                    ui::text(faces, crate::tr("No effort levels"))
                        .p(px(8.))
                        .text_size(px(crate::style::LABEL))
                        .text_color(theme::faint()),
                );
            }
            _ => {}
        }
        column = column.child(rows);
        if !others.is_empty() {
            column = column.child(Separator::horizontal().mx(px(6.)));
        }
        for (n, (o, (current, choices))) in others.into_iter().enumerate() {
            column = column.child(self.option_row(faces, n, o, current, choices, cx));
        }
        column
    }

    /// An option beyond the model, its mode and its effort: its name over
    /// its value, its choices opening beside it.
    fn option_row(
        &self,
        faces: &Faces,
        n: usize,
        o: &agent_client_protocol::v1::schema::SessionConfigOption,
        current: String,
        choices: super::Choices,
        cx: &mut Context<Self>,
    ) -> Popover {
        let line = ui::opener(SharedString::from(format!("option-{n}")))
            .w_full()
            .h(px(44.))
            .px(px(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::text(faces, o.name.clone())
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::faint()),
                            )
                            .child(ui::one_line(
                                ui::text(faces, current).text_size(px(crate::style::BODY)),
                            )),
                    )
                    .child(ui::icon("chevron-right", 12.).text_color(theme::faint())),
            );
        let shown = Popup::Option(o.id.0.to_string());
        let config = o.id.0.to_string();
        let (desk, drawn, faces) = (cx.entity(), cx.entity(), faces.clone());
        Popover::new(SharedString::from(format!("option-{n}-choices")))
            .anchor(Anchor::LeftCenter)
            .trigger_style(gpui::StyleRefinement::default().w_full().px(px(6.)))
            .p(px(6.))
            .open(self.popup == shown)
            .trigger(line)
            .on_open_change(move |open, _, cx| {
                desk.update(cx, |d, cx| {
                    if *open {
                        d.popup = shown.clone();
                    } else if d.popup == shown {
                        d.popup = Popup::Model;
                    }
                    cx.notify();
                });
            })
            .content(move |_, _, cx| {
                drawn.update(cx, |_, cx| {
                    let mut list = div().flex().flex_col().w(px(220.));
                    for (k, (value, label, on)) in choices.clone().into_iter().enumerate() {
                        let config = config.clone();
                        list = list.child(
                            ListItem::new(SharedString::from(format!("option-{k}")))
                                .h(px(30.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .child(ui::text(&faces, label).text_size(px(crate::style::LABEL)))
                                .when(on, |d| {
                                    d.suffix(|_, _| {
                                        ui::icon("check", 13.).text_color(theme::text2())
                                    })
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_option(config.clone(), value.clone(), cx);
                                    this.popup = Popup::Model;
                                })),
                        );
                    }
                    list
                })
            })
    }

    /// The foot: a new session sent to every agent at once at the left,
    /// the session's mode at the right.
    fn picker_foot(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let usable = self.slots_on(s.host).filter(|x| x.usable()).count();
        let all_on = usable > 1 && self.composer.draft.also_run.len() + 1 >= usable;
        let (host, current_agent) = (s.host, s.meta.agent.clone());
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_t_1()
            .border_color(theme::border2())
            .bg(theme::rail())
            .when(Self::is_draft(s) && usable > 1, |d| {
                d.child(
                    ui::opener("fanout")
                        .selected(all_on)
                        .child(ui::icon("plus", 13.))
                        .child(
                            ui::text(
                                faces,
                                crate::tr_with("Send to all %{n} agents", &[("n", &usable)]),
                            )
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::muted()),
                        )
                        .tooltip(crate::tr(
                            "A thread each, begun with the same words, each in a worktree of its own",
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.composer.draft.also_run.is_empty() {
                                if !this.worktree_each() {
                                    return cx.notify();
                                }
                                this.composer.draft.also_run = this
                                    .others(host, &current_agent)
                                    .into_iter()
                                    .map(|(id, _)| (id, None))
                                    .collect();
                            } else {
                                this.composer.draft.also_run.clear();
                            }
                            cx.notify();
                        })),
                )
            })
            .child(div().flex_1())
            .children(self.mode_button(faces, s, cx))
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
        let Some(s) = self.session() else { return };
        if m.agent != s.meta.agent {
            if !Self::is_draft(s) {
                return;
            }
            self.switch_agent(&m.agent, cx);
        }
        self.composer
            .draft
            .also_run
            .retain(|(a, v)| !(a == &m.agent && v.as_deref() == Some(m.value.as_str())));
        self.set_option(m.config.clone(), m.value.clone(), cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
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

/// How many of `EffortGlyph`'s five bars a thought level lights: by where
/// it stands among the agent's own levels (`at` of `of`, lowest first),
/// whatever it names them.
fn effort_level_of(at: usize, of: usize) -> u0ui::t3::composer::EffortLevel {
    use u0ui::t3::composer::EffortLevel;
    if of < 2 {
        return EffortLevel::Medium;
    }
    let bars = EffortLevel::ALL.len() - 1;
    EffortLevel::ALL[(at.min(of - 1) * bars + (of - 1) / 2) / (of - 1)]
}

/// Whether a choice stands when `query` (lower case) is typed.
fn shown_by(query: &str, value: &str, label: &str) -> bool {
    query.is_empty() || label.to_lowercase().contains(query) || value.to_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use super::effort_level_of;
    use u0ui::t3::composer::EffortLevel;

    #[test]
    fn effort_bars_follow_where_a_level_stands_among_the_agents_own() {
        let three: Vec<_> = (0..3).map(|at| effort_level_of(at, 3)).collect();
        assert_eq!(
            three,
            [EffortLevel::Minimal, EffortLevel::Medium, EffortLevel::Max]
        );
        let five: Vec<_> = (0..5).map(|at| effort_level_of(at, 5)).collect();
        assert_eq!(five, EffortLevel::ALL);
        assert_eq!(effort_level_of(1, 4), EffortLevel::Low);
        assert_eq!(effort_level_of(2, 4), EffortLevel::High);
        assert_eq!(effort_level_of(0, 1), EffortLevel::Medium);
    }
}
