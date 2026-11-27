//! The writer's tab: which agent, with which model, writes what u0 has
//! written for you, and your instructions for each kind of text.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory as Category;
use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Textarea, TextareaState},
    menu::PopupMenu,
    setting::{SettingItem, SettingPage},
    switch::Switch,
};
use gpui::{Axis, Context, Entity, WeakEntity, prelude::*, px};

use super::prefs::{group, item};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    thread::select_choices,
    ui,
};

impl Desk {
    /// The writer's tab.
    pub(super) fn writing_tab(
        w: &WeakEntity<Desk>,
        banners: Option<gpui::component::setting::SettingGroup>,
    ) -> SettingPage {
        let writer = vec![
            item(
                w,
                crate::tr("Who writes"),
                crate::tr("In a thread of its own, deleted once it answers; it spends what a turn spends."),
                |d, faces, cx| d.writer_pick(faces, cx).into_any_element(),
            )
            .keywords(["Writer", "model", "agent", "titles", "commit"]),
            item(
                w,
                crate::tr("Suggest what to say next"),
                crate::tr("After each turn, in the empty composer: Tab takes it, Esc lets it go."),
                |d, _, cx| {
                    Switch::new("suggest")
                        .accessibility_label(crate::tr("Suggest what to say next"))
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
                "Written when you ask a thread for one again.",
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
        let mut page = SettingPage::new(super::Tab::Writing.title()).description(crate::tr(
            "u0 has no model of its own: an agent of yours writes what u0 writes for you.",
        ));
        if let Some(b) = banners {
            page = page.group(b);
        }
        page.group(group(crate::tr("The writer"), writer))
            .group(group(crate::tr("Your instructions"), yours))
    }

    /// The writer, and the menu to choose another: the session's own
    /// agent, or one switched on with one of its models.
    fn writer_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let w = &self.settings.writer;
        let shown = match &w.agent {
            None => crate::tr("The thread's agent").to_string(),
            Some(agent) => {
                let model = w.model.as_ref().map_or_else(
                    || crate::tr("its default").to_string(),
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
        self.menu(
            Popup::Writer,
            trigger,
            |d, menu, desk, _| {
                let w = &d.settings.writer;
                d.models_menu(
                    menu,
                    desk,
                    Some("The thread's agent"),
                    (w.agent.as_deref(), w.model.as_deref()),
                    |d, agent, model| {
                        (d.settings.writer.agent, d.settings.writer.model) = (agent, model);
                        d.save_settings();
                    },
                )
            },
            cx,
        )
    }

    /// The name `agent` gives the model `value`, as a session of it said.
    pub(super) fn model_name(&self, agent: &str, value: &str) -> Option<String> {
        let options = &self.slot(self.pages.on, agent)?.probe.as_ref()?.options;
        let option = options
            .iter()
            .find(|o| o.category == Some(Category::Model))?;
        let (_, choices) = select_choices(&option.kind)?;
        choices.into_iter().find(|c| c.0 == value).map(|c| c.1)
    }

    /// The agents switched on, each with its models, to choose one from;
    /// `own`, where given, names a choice of none first. `chosen` is
    /// ticked; picking one hands it to `set`.
    pub(super) fn models_menu(
        &self,
        mut menu: PopupMenu,
        desk: &Entity<Self>,
        own: Option<&str>,
        chosen: (Option<&str>, Option<&str>),
        set: fn(&mut Self, Option<String>, Option<String>),
    ) -> PopupMenu {
        let pick = |label: String, on: bool, agent: Option<String>, model: Option<String>| {
            Self::menu_item(desk, label, move |this, _, cx| {
                set(this, agent.clone(), model.clone());
                cx.notify();
            })
            .checked(on)
        };
        if let Some(own) = own {
            menu = menu.item(pick(crate::tr(own).into(), chosen.0.is_none(), None, None));
        }
        for slot in &self.slots {
            let agent = slot.entry.id.clone();
            let on = chosen.0 == Some(agent.as_str());
            menu = menu.label(self.name_of(&agent)).item(pick(
                crate::tr("Its default model").into(),
                on && chosen.1.is_none(),
                Some(agent.clone()),
                None,
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
                menu = menu.item(pick(name, is_chosen, Some(agent.clone()), Some(value)));
            }
        }
        menu.min_w(px(300.))
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
