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
use gpui::{AppContext as _, Context, Window, div, prelude::*, px};

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

    /// Inline card in the thread for an unanswered elicitation (`ThreadQuestions`
    /// board): `RequestCard` for URL elicitations, `UserInputCard` for choice
    /// questionnaires, and `Questionnaire` for numeric/freeform schemas, while
    /// leaving the main composer open below.
    pub(super) fn inline_elicit_card(
        &self,
        faces: &Faces,
        s: &Session,
        e: &Elicit,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use u0ui::t3::timeline::{
            InputOption, InputQuestion, RequestCard, UserInputCard, UserInputTexts,
        };
        let (agent, key, id) = (s.meta.agent.clone(), s.key, e.id);
        let title = crate::tr_with("%{name} asks", &[("name", &self.name_of(&agent))]);
        if let Some(url) = e.url.clone() {
            let desk_yes = cx.entity();
            let desk_no = cx.entity();
            let (agent_yes, agent_no) = (agent.clone(), agent);
            // A page to open is how an agent asks you to sign in (the
            // Guest board): what opens, who keeps what you enter there,
            // and what going on without it does.
            let name = self.name_of(&agent_yes);
            let row = |k: gpui::SharedString, v: gpui::SharedString| {
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(12.))
                    .child(
                        ui::text(faces, k)
                            .w(px(96.))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                    )
                    .child(ui::text(faces, v).flex_1().min_w(px(0.)))
            };
            let about = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(row(
                    crate::tr("Opens"),
                    crate::tr_with(
                        "The link %{name} gave, in your default browser",
                        &[("name", &name)],
                    ),
                ))
                .child(row(
                    crate::tr("Kept by"),
                    crate::tr_with(
                        "%{name}, as it keeps its own sign-ins; u0 stores nothing",
                        &[("name", &name)],
                    ),
                ))
                .child(
                    ui::text(
                        faces,
                        crate::tr_with(
                            "The thread waits. Go on without it to let %{name} continue.",
                            &[("name", &name)],
                        ),
                    )
                    .text_size(px(12.))
                    .text_color(theme::text2()),
                );
            return RequestCard::new(
                gpui::SharedString::from(format!("elicit-url-{id}")),
                crate::tr_with("%{name} needs you to sign in", &[("name", &name)]),
                crate::tr("Open in browser"),
                crate::tr("Go on without it"),
            )
            .icon(u0ui::Icon::carbon(u0ui::icons::CarbonIcon::Password))
            .body(e.message.clone())
            .preview(about)
            .on_primary(move |_, cx| {
                desk_yes.update(cx, |this, cx| {
                    this.browse(&url, cx);
                    let yes = CreateElicitationResponse::new(ElicitationAction::Accept(
                        ElicitationAcceptAction::new(),
                    ));
                    let words = crate::tr_with("Opened %{url}", &[("url", &url)]);
                    this.answer_form(&agent_yes, key, id, Answer::Form(yes), &words, cx);
                });
            })
            .on_secondary(move |_, cx| {
                desk_no.update(cx, |this, cx| {
                    let no = CreateElicitationResponse::new(ElicitationAction::Decline);
                    this.answer_form(
                        &agent_no,
                        key,
                        id,
                        Answer::Form(no),
                        &crate::tr("Declined"),
                        cx,
                    );
                });
            })
            .into_any_element();
        }
        if let Some(schema) = &e.form
            && !schema.properties.is_empty()
            && schema.properties.values().all(|p| !listed(p).is_empty())
        {
            type ChoicePairs = Vec<(String, String)>;
            let prop_meta: Vec<(String, ElicitationPropertySchema, ChoicePairs)> = schema
                .properties
                .iter()
                .map(|(name, p)| (name.clone(), p.clone(), listed(p)))
                .collect();
            let qs: Vec<InputQuestion> = prop_meta
                .iter()
                .map(|(name, p, opts)| {
                    let q_text = match p {
                        ElicitationPropertySchema::String(s) => {
                            s.title.clone().or_else(|| s.description.clone())
                        }
                        ElicitationPropertySchema::Boolean(b) => {
                            b.title.clone().or_else(|| b.description.clone())
                        }
                        ElicitationPropertySchema::Array(a) => {
                            a.title.clone().or_else(|| a.description.clone())
                        }
                        _ => None,
                    }
                    .unwrap_or_else(|| {
                        if prop_meta.len() == 1 && !e.message.trim().is_empty() {
                            e.message.clone()
                        } else {
                            name.clone()
                        }
                    });
                    let allow_other = matches!(p, ElicitationPropertySchema::String(_));
                    let options = opts
                        .iter()
                        .map(|(val, label)| {
                            let is_def = match p {
                                ElicitationPropertySchema::String(s) => {
                                    s.default.as_deref() == Some(val.as_str())
                                }
                                ElicitationPropertySchema::Boolean(b) => {
                                    b.default.map(|d| d.to_string()).as_deref()
                                        == Some(val.as_str())
                                }
                                _ => false,
                            };
                            InputOption::new(label.clone()).default_choice(is_def)
                        })
                        .collect();
                    InputQuestion::new(q_text, options).allow_other(allow_other)
                })
                .collect();
            let desk_sub = cx.entity();
            let desk_skip = cx.entity();
            let (agent_sub, agent_skip) = (agent.clone(), agent);
            let texts = UserInputTexts {
                title: title.clone(),
                progress: std::rc::Rc::new(|i, n| {
                    crate::tr_with("Question %{i} of %{n}", &[("i", &i), ("n", &n)])
                }),
                other_placeholder: crate::tr("Something else…"),
                back: crate::tr("Back"),
                skip: crate::tr("Decline"),
                next: crate::tr("Next question"),
                submit_one: crate::tr("Submit answer"),
                submit_all: crate::tr("Submit answers"),
            };
            return UserInputCard::new(gpui::SharedString::from(format!("elicit-card-{id}")), qs)
                .title(title)
                .texts(texts)
                .on_submit(move |answers, _, cx| {
                    let mut out = BTreeMap::new();
                    for (idx, chosen_label) in answers {
                        let Some((name, p, opts)) = prop_meta.get(idx) else {
                            continue;
                        };
                        let raw = opts
                            .iter()
                            .find(|(_, l)| l == chosen_label.as_ref())
                            .map_or_else(|| chosen_label.to_string(), |(v, _)| v.clone());
                        let val = match p {
                            ElicitationPropertySchema::Boolean(_) => {
                                ElicitationContentValue::Boolean(raw == "true")
                            }
                            ElicitationPropertySchema::Array(_) => {
                                ElicitationContentValue::StringArray(vec![raw])
                            }
                            _ => ElicitationContentValue::String(raw),
                        };
                        out.insert(name.clone(), val);
                    }
                    let words = said(&out);
                    let resp = CreateElicitationResponse::new(ElicitationAction::Accept(
                        ElicitationAcceptAction::new().content(out),
                    ));
                    desk_sub.update(cx, |this, cx| {
                        this.answer_form(&agent_sub, key, id, Answer::Form(resp), &words, cx);
                    });
                })
                .on_skip(move |_, cx| {
                    desk_skip.update(cx, |this, cx| {
                        let no = CreateElicitationResponse::new(ElicitationAction::Decline);
                        this.answer_form(
                            &agent_skip,
                            key,
                            id,
                            Answer::Form(no),
                            &crate::tr("Declined"),
                            cx,
                        );
                    });
                })
                .into_any_element();
        }
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
            .w_full()
            .p(px(14.))
            .bg(theme::role(u0ui::tokens::ColorRole::Layer01))
            .border_1()
            .border_color(theme::role(u0ui::tokens::ColorRole::BorderSubtle01))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon("user", 15.).text_color(theme::accent()))
                    .child(
                        ui::text(faces, title)
                            .flex_1()
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::faint()),
                    )
                    .child(decline),
            )
            .child(ui::text(faces, e.message.clone()).text_size(px(crate::style::BODY)));
        let (Some(state), Some(schema)) = (self.forms.get(&id), &e.form) else {
            return panel.into_any_element();
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
        panel
            .child(
                form.child(
                    QuestionnaireActions::new(state)
                        .child(QuestionnairePrevious::new(state))
                        .child(QuestionnaireSkip::new(state))
                        .child(QuestionnaireNext::new(state))
                        .child(QuestionnaireSubmit::new(state)),
                ),
            )
            .into_any_element()
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
