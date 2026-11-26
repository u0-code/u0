//! What an agent asks a person for beyond a permission (ACP's
//! elicitation): a form, drawn as gpui-kit's questionnaire in place of the
//! composer, one question at a time, answered or declined; or a page to
//! open. While it waits, the agent may go on with its turn.

use std::collections::BTreeMap;

use agent_client_protocol::v1::schema::{
    CreateElicitationResponse, ElicitationAcceptAction, ElicitationAction, ElicitationContentValue,
    ElicitationPropertySchema, ElicitationSchema, MultiSelectItems,
};
use gpui::component::{
    input::InputState,
    questionnaire::{
        Questionnaire, QuestionnaireActions, QuestionnaireChoice, QuestionnaireChoiceDefinition,
        QuestionnaireChoices, QuestionnaireDescription, QuestionnaireError, QuestionnaireEvent,
        QuestionnaireInput, QuestionnaireInputDefinition, QuestionnaireItem,
        QuestionnaireItemDefinition, QuestionnaireNext, QuestionnairePrevious,
        QuestionnaireProgress, QuestionnaireSkip, QuestionnaireState, QuestionnaireSubmission,
        QuestionnaireSubmit, QuestionnaireTitle,
    },
};
use gpui::{AppContext as _, Context, Div, Window, div, prelude::*, px};

use crate::{
    acp::Answer,
    desk::Desk,
    fonts::Faces,
    session::{Elicit, Session},
    theme, ui,
};

/// The choices a property of a form offers, each by its value and what it
/// is called, each once: none for one answered in words.
fn listed(p: &ElicitationPropertySchema) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = match p {
        ElicitationPropertySchema::String(p) => match (&p.one_of, &p.enum_values) {
            (Some(o), _) => o
                .iter()
                .map(|o| (o.value.clone(), o.title.clone()))
                .collect(),
            (_, Some(e)) => e.iter().map(|v| (v.clone(), v.clone())).collect(),
            _ => Vec::new(),
        },
        ElicitationPropertySchema::Boolean(_) => {
            vec![
                ("true".into(), crate::tr("Yes").into()),
                ("false".into(), crate::tr("No").into()),
            ]
        }
        ElicitationPropertySchema::Array(p) => match &p.items {
            MultiSelectItems::String(i) => {
                i.values.iter().map(|v| (v.clone(), v.clone())).collect()
            }
            MultiSelectItems::Titled(i) => (i.options.iter())
                .map(|o| (o.value.clone(), o.title.clone()))
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    out.retain(|(v, _)| seen.insert(v.clone()));
    out
}

/// The questions of one form, in its schema's order, each with its
/// choices; a question with none is answered in words.
fn questions(
    schema: &ElicitationSchema,
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> Vec<QuestionnaireItemDefinition> {
    let required = schema.required.clone().unwrap_or_default();
    schema
        .properties
        .iter()
        .map(|(name, p)| {
            let (title, about) = match p {
                ElicitationPropertySchema::String(p) => (&p.title, &p.description),
                ElicitationPropertySchema::Number(p) => (&p.title, &p.description),
                ElicitationPropertySchema::Integer(p) => (&p.title, &p.description),
                ElicitationPropertySchema::Boolean(p) => (&p.title, &p.description),
                ElicitationPropertySchema::Array(p) => (&p.title, &p.description),
                _ => (&None, &None),
            };
            let mut item = QuestionnaireItemDefinition::new(
                name.clone(),
                title.clone().unwrap_or(name.clone()),
            )
            .with_required(required.contains(name));
            if let Some(about) = about {
                item = item.with_description(about.clone());
            }
            // What the agent gave as the default is chosen to begin with.
            let on = |v: &str| match p {
                ElicitationPropertySchema::String(p) => p.default.as_deref() == Some(v),
                ElicitationPropertySchema::Boolean(p) => {
                    p.default.map(|d| d.to_string()).as_deref() == Some(v)
                }
                ElicitationPropertySchema::Array(p) => {
                    p.default.as_ref().is_some_and(|d| d.iter().any(|x| x == v))
                }
                _ => false,
            };
            let choices = listed(p);
            match p {
                _ if !choices.is_empty() => item
                    .with_multiple(matches!(p, ElicitationPropertySchema::Array(_)))
                    .with_choices(choices.iter().map(|(v, l)| {
                        QuestionnaireChoiceDefinition::new(v.clone(), l.clone())
                            .with_default_selected(on(v))
                    })),
                _ => {
                    let start = match p {
                        ElicitationPropertySchema::String(p) => p.default.clone(),
                        ElicitationPropertySchema::Number(p) => p.default.map(|d| d.to_string()),
                        ElicitationPropertySchema::Integer(p) => p.default.map(|d| d.to_string()),
                        _ => None,
                    };
                    let input = cx.new(|cx| {
                        let input =
                            InputState::new(window, cx).placeholder(crate::tr("Your answer"));
                        match start {
                            Some(s) => input.default_value(s),
                            None => input,
                        }
                    });
                    let number = matches!(
                        p,
                        ElicitationPropertySchema::Number(_)
                            | ElicitationPropertySchema::Integer(_)
                    );
                    item.with_input(QuestionnaireInputDefinition::new(
                        input,
                        crate::tr("Your answer"),
                    ))
                    .with_validator(move |c| match c.answer().freeform() {
                        Some(v) if number && v.parse::<f64>().is_err() => {
                            Err(crate::tr("A number, please."))
                        }
                        _ => Ok(()),
                    })
                }
            }
        })
        .collect()
}

/// What was put in the form, as the agent asked for it: each answer in
/// its property's type; an answer left out is not sent.
fn content(
    schema: &ElicitationSchema,
    submission: &QuestionnaireSubmission,
) -> BTreeMap<String, ElicitationContentValue> {
    let mut out = BTreeMap::new();
    for item in submission.items() {
        let name = item.name().to_string();
        let answer = item.answer();
        let first = answer
            .choices()
            .first()
            .map(ToString::to_string)
            .or_else(|| answer.freeform().map(ToString::to_string))
            .filter(|v| !v.is_empty());
        let value = match schema.properties.get(&name) {
            Some(ElicitationPropertySchema::Boolean(_)) => {
                first.map(|v| ElicitationContentValue::Boolean(v == "true"))
            }
            Some(ElicitationPropertySchema::Integer(_)) => first
                .and_then(|v| v.parse().ok())
                .map(ElicitationContentValue::Integer),
            Some(ElicitationPropertySchema::Number(_)) => first
                .and_then(|v| v.parse().ok())
                .map(ElicitationContentValue::Number),
            Some(ElicitationPropertySchema::Array(_)) => {
                let mut all: Vec<String> =
                    answer.choices().iter().map(ToString::to_string).collect();
                all.extend(
                    answer
                        .freeform()
                        .map(ToString::to_string)
                        .filter(|v| !v.is_empty()),
                );
                (!all.is_empty()).then_some(ElicitationContentValue::StringArray(all))
            }
            _ => first.map(ElicitationContentValue::String),
        };
        if let Some(value) = value {
            out.insert(name, value);
        }
    }
    out
}

/// What was answered, in words, for the thread.
fn said(content: &BTreeMap<String, ElicitationContentValue>) -> String {
    content
        .iter()
        .map(|(k, v)| {
            let v = match v {
                ElicitationContentValue::String(s) => s.clone(),
                ElicitationContentValue::Integer(n) => n.to_string(),
                ElicitationContentValue::Number(n) => n.to_string(),
                ElicitationContentValue::Boolean(b) => {
                    crate::tr(if *b { "yes" } else { "no" }).into()
                }
                ElicitationContentValue::StringArray(a) => a.join(", "),
                _ => String::new(),
            };
            format!("{k}: {v}")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Desk {
    /// The agent `agent` asks, in the session `key`, for the form `id`:
    /// its questions made ready to be drawn over the composer.
    pub(crate) fn form_asked(
        &mut self,
        agent: &str,
        key: u64,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(schema) = self
            .sessions
            .at(key)
            .and_then(Session::eliciting)
            .and_then(|e| e.form.clone())
        else {
            return;
        };
        let items = questions(&schema, window, cx);
        // Its choices are each once and at most one is the default (see
        // `listed`), all a questionnaire asks of its questions.
        let state = cx.new(|cx| QuestionnaireState::new(items, cx).expect("a well-formed form"));
        let agent = agent.to_owned();
        cx.subscribe(&state, move |this, _, e: &QuestionnaireEvent, cx| {
            if let QuestionnaireEvent::Submit(submission) = e {
                let content = content(&schema, submission);
                let words = said(&content);
                let answer = CreateElicitationResponse::new(ElicitationAction::Accept(
                    ElicitationAcceptAction::new().content(content),
                ));
                this.answer_form(&agent, key, id, Answer::Form(answer), &words, cx);
            }
        })
        .detach();
        self.forms.insert(id, state);
    }

    /// In place of the composer, what the agent asks for: the form, a
    /// question at a time, or the page to open; either can be declined.
    pub(super) fn form_panel(
        &self,
        faces: &Faces,
        s: &Session,
        e: &Elicit,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let (agent, key, id) = (s.meta.agent.clone(), s.key, e.id);
        let decline = {
            let agent = agent.clone();
            ui::button("form-decline", crate::tr("Decline"), ui::Weight::Quiet).on_click(
                cx.listener(move |this, _, _, cx| {
                    let no = CreateElicitationResponse::new(ElicitationAction::Decline);
                    this.answer_form(
                        &agent,
                        key,
                        id,
                        Answer::Form(no),
                        &crate::tr("Declined"),
                        cx,
                    );
                }),
            )
        };
        let panel = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(column))
            .p(px(14.))
            .rounded(px(12.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon("user", 15.).text_color(theme::accent()))
                    .child(
                        ui::text(
                            faces,
                            crate::tr_with("%{name} asks", &[("name", &self.name_of(&agent))]),
                        )
                        .flex_1()
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::faint()),
                    )
                    .child(decline),
            )
            .child(ui::text(faces, e.message.clone()).text_size(px(crate::style::BODY)));
        if let Some(url) = e.url.clone() {
            return panel.child(div().flex().justify_end().child(
                ui::button("form-open", crate::tr("Open the page"), ui::Weight::Primary).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.open_url(&url);
                        let yes = CreateElicitationResponse::new(ElicitationAction::Accept(
                            ElicitationAcceptAction::new(),
                        ));
                        let words = crate::tr_with("Opened %{url}", &[("url", &url)]);
                        this.answer_form(&agent, key, id, Answer::Form(yes), &words, cx);
                    }),
                ),
            ));
        }
        let (Some(state), Some(schema)) = (self.forms.get(&id), &e.form) else {
            return panel;
        };
        let mut form = Questionnaire::new(state).child(QuestionnaireProgress::new(state));
        for (name, p) in &schema.properties {
            let mut choices = QuestionnaireChoices::new(state, name.clone());
            for (v, _) in listed(p) {
                choices = choices.child(QuestionnaireChoice::new(state, name.clone(), v));
            }
            form = form.child(
                QuestionnaireItem::new(state, name.clone())
                    .child(QuestionnaireTitle::new(state, name.clone()))
                    .child(QuestionnaireDescription::new(state, name.clone()))
                    .child(choices.child(QuestionnaireInput::new(state, name.clone())))
                    .child(QuestionnaireError::new(state, name.clone())),
            );
        }
        panel.child(
            form.child(
                QuestionnaireActions::new(state)
                    .child(QuestionnairePrevious::new(state))
                    .child(QuestionnaireSkip::new(state))
                    .child(QuestionnaireNext::new(state))
                    .child(QuestionnaireSubmit::new(state)),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_answer_is_sent_in_each_property_s_type() {
        let schema: ElicitationSchema = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "db": {"type": "string", "enum": ["pg", "sqlite"]},
                "tests": {"type": "boolean"},
                "port": {"type": "integer"},
            },
        }))
        .unwrap();
        let mut answers = BTreeMap::new();
        answers.insert(
            "db".to_owned(),
            ElicitationContentValue::String("pg".into()),
        );
        answers.insert("tests".to_owned(), ElicitationContentValue::Boolean(true));
        answers.insert("port".to_owned(), ElicitationContentValue::Integer(8080));
        assert_eq!(said(&answers), "db: pg · port: 8080 · tests: yes");
        assert!(schema.properties.contains_key("port"));
    }
}
