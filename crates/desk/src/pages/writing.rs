//! The writer's tab: which agent, with which model, writes what Ghost has
//! written for you, and your instructions for each kind of text.

use agent_client_protocol::v1::schema::SessionConfigOptionCategory as Category;
use gpui::component::{
    Selectable as _, Sizable as _, input::Textarea, scroll::ScrollableElement as _, switch::Switch,
};
use gpui::{Context, Div, Entity, SharedString, div, prelude::*, px};

use super::prefs::{group, page, said};
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    theme,
    thread::select_choices,
    ui,
};

impl Desk {
    /// The writer's tab; none when what is looked for, `q`, is in none of
    /// it.
    pub(super) fn writing_tab(
        &self,
        faces: &Faces,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let who = said(
            q,
            &["Writer", "Who writes", "model", "agent", "titles", "commit"],
        )
        .then(|| {
            super::prefs::setting(
                faces,
                "Who writes",
                "In a session of its own, deleted once it answers; it spends what a turn spends.",
                self.writer_pick(faces, cx),
            )
        });
        let next = said(q, &["suggest", "next", "Tab"]).then(|| {
            super::prefs::setting(
                faces,
                "Suggest what to say next",
                "After each turn, in the empty composer: Tab takes it, Esc lets it go.",
                Switch::new("suggest")
                    .small()
                    .checked(self.settings.writer.suggest)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.writer.suggest = !this.settings.writer.suggest;
                        this.save_settings();
                        cx.notify();
                    })),
            )
        });
        let field = |words: &[&str], name: &str, about: &str, input| {
            said(q, words).then(|| instructions(faces, name, about, input))
        };
        let yours = vec![
            field(
                &["title", "instructions"],
                "Titles",
                "Written when you ask a session for one again.",
                &self.pages.titles,
            ),
            field(
                &["branch", "worktree", "instructions"],
                "Worktree branches",
                "Named from what you first ask, once the worktree is made.",
                &self.pages.branches,
            ),
            field(
                &["commit", "pull request", "instructions"],
                "Commit messages and pull requests",
                "Your style for them, over the repository's own and its template.",
                &self.pages.changes,
            ),
            field(
                &["dictation", "dictated", "clean", "instructions"],
                "Dictation, cleaned up",
                "Where you asked for what you dictate to be cleaned up before it goes in.",
                &self.pages.transcripts,
            ),
        ];
        let groups: Vec<Div> = [
            group(faces, "The writer", vec![who, next]),
            group(faces, "Your instructions", yours),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!groups.is_empty()).then(|| {
            page(
                faces,
                super::Tab::Writing.title(),
                "Ghost has no model of its own: an agent of yours writes what Ghost writes for you.",
            )
            .children(groups)
        })
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
            move |d, _, cx| d.writer_menu(&faces, cx).overflow_y_scrollbar(),
            cx,
        )
    }

    /// The name `agent` gives the model `value`, as a session of it said.
    fn model_name(&self, agent: &str, value: &str) -> Option<String> {
        let options = &self.slot(agent)?.probe.as_ref()?.options;
        let option = options
            .iter()
            .find(|o| o.category == Some(Category::Model))?;
        let (_, choices) = select_choices(&option.kind)?;
        choices.into_iter().find(|c| c.0 == value).map(|c| c.1)
    }

    fn writer_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let w = &self.settings.writer;
        let pick = |id: String,
                    label: String,
                    on: bool,
                    agent: Option<String>,
                    model: Option<String>,
                    cx: &mut Context<Self>| {
            ui::menu_row(SharedString::from(id), on)
                .child(ui::one_line(ui::text(faces, label).text_size(px(13.))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.writer.agent.clone_from(&agent);
                    this.settings.writer.model.clone_from(&model);
                    this.save_settings();
                    this.popup = Popup::Closed;
                    cx.notify();
                }))
        };
        let mut list = div()
            .id("writer-menu")
            .flex()
            .flex_col()
            .w(px(300.))
            .max_h(px(420.))
            .child(pick(
                "writer-own".into(),
                "The session's agent".into(),
                w.agent.is_none(),
                None,
                None,
                cx,
            ));
        for slot in &self.slots {
            let agent = slot.entry.id.clone();
            let on = w.agent.as_ref() == Some(&agent);
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
                format!("writer-{agent}"),
                "Its default model".into(),
                on && w.model.is_none(),
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
                let chosen = on && w.model.as_ref() == Some(&value);
                list = list.child(pick(
                    format!("writer-{agent}-{value}"),
                    name,
                    chosen,
                    Some(agent.clone()),
                    Some(value),
                    cx,
                ));
            }
        }
        list
    }
}

/// A field for your instructions under what it is for.
pub(super) fn instructions(
    faces: &Faces,
    name: &str,
    about: &str,
    input: &Entity<gpui::component::input::TextareaState>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(16.))
        .py(px(12.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::text(faces, name.to_owned()).text_size(px(13.5)))
                .child(
                    ui::text(faces, about.to_owned())
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(theme::faint()),
                ),
        )
        .child(Textarea::new(input))
}
