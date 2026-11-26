//! The writer's tab: which agent, with which model, writes what Ghost has
//! written for you, and your instructions for each kind of text.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory as Category;
use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Textarea, TextareaState},
    scroll::ScrollableElement as _,
    setting::{SettingItem, SettingPage},
    switch::Switch,
};
use gpui::{Axis, Context, Div, Entity, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    theme,
    thread::select_choices,
    ui,
};

impl Desk {
    /// The writer's tab.
    pub(super) fn writing_tab(w: &WeakEntity<Desk>) -> SettingPage {
        let writer = vec![
            item(
                w,
                "Who writes",
                "In a session of its own, deleted once it answers; it spends what a turn spends.",
                |d, faces, cx| d.writer_pick(faces, cx).into_any_element(),
            )
            .keywords(["Writer", "model", "agent", "titles", "commit"]),
            item(
                w,
                "Suggest what to say next",
                "After each turn, in the empty composer: Tab takes it, Esc lets it go.",
                |d, _, cx| {
                    Switch::new("suggest")
                        .small()
                        .checked(d.settings.writer.suggest)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.writer.suggest = !this.settings.writer.suggest;
                            this.save_settings();
                            cx.notify();
                        }))
                        .into_any_element()
                },
            )
            .keywords(["Tab"]),
        ];
        let yours = vec![
            instructions(
                w,
                "Titles",
                "Written when you ask a session for one again.",
                |d| &d.pages.titles,
            ),
            instructions(
                w,
                "Worktree branches",
                "Named from what you first ask, once the worktree is made.",
                |d| &d.pages.branches,
            ),
            instructions(
                w,
                "Commit messages and pull requests",
                "Your style for them, over the repository's own and its template.",
                |d| &d.pages.changes,
            ),
            instructions(
                w,
                "Dictation, cleaned up",
                "Where you asked for what you dictate to be cleaned up before it goes in.",
                |d| &d.pages.transcripts,
            ),
        ];
        SettingPage::new(super::Tab::Writing.title())
            .description(
                "Ghost has no model of its own: an agent of yours writes what Ghost writes for you.",
            )
            .group(group("The writer", writer))
            .group(group("Your instructions", yours))
    }

    /// The writer, and the menu to choose another: the session's own
    /// agent, or one switched on with one of its models.
    fn writer_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let w = &self.settings.writer;
        let shown = match &w.agent {
            None => "The session's agent".to_owned(),
            Some(agent) => {
                let model = w.model.as_ref().map_or_else(
                    || "its default".to_owned(),
                    |m| self.model_name(agent, m).unwrap_or_else(|| m.clone()),
                );
                format!("{} · {model}", self.name_of(agent))
            }
        };
        let trigger = ui::opener("writer")
            .selected(self.popup == Popup::Writer)
            .children(
                w.agent
                    .as_ref()
                    .map(|a| ui::mark(faces, &self.mark_of(a), 14.)),
            )
            .child(shown);
        let faces = faces.clone();
        self.menu(
            Popup::Writer,
            trigger,
            move |d, _, cx| {
                let w = d.settings.writer.clone();
                d.models_menu(
                    &faces,
                    Some("The session's agent"),
                    (w.agent.as_deref(), w.model.as_deref()),
                    |d, agent, model| {
                        (d.settings.writer.agent, d.settings.writer.model) = (agent, model);
                        d.save_settings();
                    },
                    cx,
                )
                .overflow_y_scrollbar()
            },
            cx,
        )
    }

    /// The name `agent` gives the model `value`, as a session of it said.
    pub(super) fn model_name(&self, agent: &str, value: &str) -> Option<String> {
        let options = &self.slot(agent)?.probe.as_ref()?.options;
        let option = options
            .iter()
            .find(|o| o.category == Some(Category::Model))?;
        let (_, choices) = select_choices(&option.kind)?;
        choices.into_iter().find(|c| c.0 == value).map(|c| c.1)
    }

    /// The agents switched on, each with its models, to choose one from;
    /// `own`, where given, names a choice of none first. `chosen` is lit;
    /// picking one hands it to `set`.
    pub(super) fn models_menu(
        &self,
        faces: &Faces,
        own: Option<&str>,
        chosen: (Option<&str>, Option<&str>),
        set: fn(&mut Self, Option<String>, Option<String>),
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let pick = |id: String,
                    label: String,
                    on: bool,
                    agent: Option<String>,
                    model: Option<String>,
                    cx: &mut Context<Self>| {
            ui::menu_row(SharedString::from(id), on)
                .child(ui::one_line(ui::text(faces, label).text_size(px(13.))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    set(this, agent.clone(), model.clone());
                    this.popup = Popup::Closed;
                    cx.notify();
                }))
        };
        let mut list = div()
            .id("models-menu")
            .flex()
            .flex_col()
            .w(px(300.))
            .max_h(px(420.));
        if let Some(own) = own {
            list = list.child(pick(
                "models-own".into(),
                own.into(),
                chosen.0.is_none(),
                None,
                None,
                cx,
            ));
        }
        for slot in &self.slots {
            let agent = slot.entry.id.clone();
            let on = chosen.0 == Some(agent.as_str());
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .pt(px(10.))
                    .pb(px(4.))
                    .child(ui::mark(faces, &self.mark_of(&agent), 14.))
                    .child(
                        ui::text(faces, self.name_of(&agent))
                            .text_size(px(12.))
                            .text_color(theme::muted()),
                    ),
            );
            list = list.child(pick(
                format!("models-{agent}"),
                "Its default model".into(),
                on && chosen.1.is_none(),
                Some(agent.clone()),
                None,
                cx,
            ));
            let models = slot
                .probe
                .as_ref()
                .and_then(|p| {
                    p.options
                        .iter()
                        .find(|o| o.category == Some(Category::Model))
                })
                .and_then(|o| select_choices(&o.kind))
                .map(|c| c.1)
                .unwrap_or_default();
            for (value, name, _) in models {
                let is_chosen = on && chosen.1 == Some(value.as_str());
                list = list.child(pick(
                    format!("models-{agent}-{value}"),
                    name,
                    is_chosen,
                    Some(agent.clone()),
                    Some(value),
                    cx,
                ));
            }
        }
        list
    }
}

/// A field for your instructions, `of` the desk's, under what it is for.
pub(super) fn instructions(
    w: &WeakEntity<Desk>,
    name: &str,
    about: &str,
    of: fn(&Desk) -> &Entity<TextareaState>,
) -> SettingItem {
    item(w, name.to_owned(), about.to_owned(), move |d, _, _| {
        Textarea::new(of(d)).into_any_element()
    })
    .layout(Axis::Vertical)
    .keywords(["instructions"])
}
