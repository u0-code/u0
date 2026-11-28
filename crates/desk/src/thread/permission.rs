//! A question the agent, or one of u0's tools, asks before it goes on:
//! u0ui's `ApprovalCard` with the asker's options as its buttons (boards
//! `PermissionCard` and `PermissionCards`), in their order and under their
//! own names; once answered, one line that says what was chosen.

use agent_client_protocol::v1::schema::{PermissionOptionKind, ToolCallContent, ToolKind};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use serde_json::Value;
use u0ui::{
    icons::CarbonIcon,
    t3::timeline::{ApprovalCard, ApprovalKind, ApprovalOption},
};

use super::{bare, meta};
use crate::{
    acp::Asker,
    desk::Desk,
    fonts::Faces,
    session::{Asked, Session},
    ui::{self, Weight},
    when,
};

/// What a card says of the call, from what it was given: a line in mono,
/// fields by name, why, and where.
#[derive(Debug, Default, PartialEq)]
struct Said {
    detail: Option<String>,
    facts: Vec<(String, String)>,
    reason: Option<String>,
    context: Option<String>,
}

/// What a call asked about by the agent says of itself (a command, a file
/// or an address; what it is for; where it runs), or, by one of u0's
/// tools, the text it carries and the rest as fields.
fn said(asker: &Asker, input: Option<&Value>) -> Said {
    let Some(Value::Object(map)) = input else {
        return Said::default();
    };
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| map.get(*k)?.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if *asker == Asker::Agent {
        return Said {
            detail: text(&["file_path", "path", "url"]),
            facts: Vec::new(),
            reason: text(&["description", "reason", "justification"]),
            context: text(&["cwd", "directory", "workdir"]),
        };
    }
    let carried = ["text", "body", "message", "prompt"];
    let mut facts: Vec<(usize, &str, String)> = (map.iter())
        .filter(|(key, _)| !carried.contains(&key.as_str()))
        .filter_map(|(key, value)| Some((rank(key), key.as_str(), value_of(key, value)?)))
        .collect();
    facts.sort_by_key(|(rank, key, _)| (*rank, *key));
    let facts = (facts.into_iter())
        .map(|(_, key, shown)| (label_of(key).to_string(), shown))
        .collect();
    Said {
        detail: text(&carried),
        facts,
        reason: None,
        context: None,
    }
}

/// Where a field of u0's tools stands among the others on a card: what
/// it acts on first, then where, then the rest.
fn rank(key: &str) -> usize {
    match key {
        "number" => 0,
        "repo" | "repository" | "folder" | "project" => 1,
        "from" => 2,
        "base" => 3,
        "title" => 4,
        "draft" => 5,
        "how" | "method" => 6,
        "auto" => 7,
        "branch" => 8,
        "session" | "workflow" | "device" | "agent" | "model" => 9,
        "action" | "schedule" | "verdict" => 10,
        _ => 20,
    }
}

/// What a field of u0's tools is called on a card, in your language; one
/// it does not know, by its own name.
fn label_of(key: &str) -> SharedString {
    let said = match key {
        "repo" | "repository" => "Repository",
        "folder" => "Folder",
        "from" => "From",
        "base" => "Base",
        "branch" => "Branch",
        "title" => "Title",
        "draft" => "Draft",
        "number" => "Pull request",
        "how" | "method" => "Method",
        "auto" => "Wait for checks",
        "workflow" => "Workflow",
        "input" => "Input",
        "schedule" => "Schedule",
        "agent" => "Agent",
        "model" => "Model",
        "project" => "Project",
        "device" => "Device",
        "action" => "Action",
        "session" => "Thread",
        "request" => "Request",
        "option" => "Answer",
        "pin" => "Pin",
        "archive" => "Archive",
        "tags" => "Tags",
        "verdict" => "Verdict",
        "run" => "Run",
        "path" => "Path",
        "question" => "Question",
        "choices" => "Choices",
        "state" => "State",
        "worktree" => "Worktree",
        other => return SharedString::from(other.replace('_', " ")),
    };
    crate::tr(said)
}

/// A field's value as a card shows it: yes or no in your language, a pull
/// request's number with its #, a list joined, tags each with its #;
/// nothing for one empty or nested.
fn value_of(key: &str, value: &Value) -> Option<String> {
    match value {
        Value::Null | Value::Object(_) => None,
        Value::Bool(b) => Some(crate::tr(if *b { "Yes" } else { "No" }).to_string()),
        Value::Number(n) if key == "number" => Some(format!("#{n}")),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Array(items) => {
            let each: Vec<String> = (items.iter())
                .filter_map(|v| value_of("", v))
                .map(|v| match key {
                    "tags" if !v.starts_with('#') => format!("#{v}"),
                    _ => v,
                })
                .collect();
            (!each.is_empty()).then(|| each.join(if key == "tags" { " " } else { ", " }))
        }
    }
}

/// The option of ACP's as u0ui's card takes it.
fn option_of(o: &agent_client_protocol::v1::schema::PermissionOption) -> ApprovalOption {
    use u0ui::t3::timeline::PermissionOptionKind as Kind;
    let kind = match o.kind {
        PermissionOptionKind::AllowOnce => Kind::AllowOnce,
        PermissionOptionKind::AllowAlways => Kind::AllowAlways,
        PermissionOptionKind::RejectAlways => Kind::RejectAlways,
        _ => Kind::RejectOnce,
    };
    ApprovalOption::new(o.name.clone(), kind)
}

impl Desk {
    /// The card for what `asked` asks in the session `s`: its options as
    /// buttons; once answered, what was chosen.
    pub(super) fn asked(
        &self,
        faces: &Faces,
        s: &Session,
        key: &str,
        asked: &Asked,
        cx: &mut Context<Self>,
    ) -> ApprovalCard {
        let name = self.name_of(&s.meta.agent);
        let call = (s.tools())
            .filter(|t| t.call.tool_call_id == asked.call)
            .last()
            .map(|t| &t.call);
        // Leaving plan mode is asked with the plan: Claude's plan card.
        let plan = call
            .filter(|c| c.kind == ToolKind::SwitchMode)
            .map(|c| {
                (c.content.iter())
                    .filter_map(|x| match x {
                        ToolCallContent::Content(c) => Some(crate::session::text_of(&c.content)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .filter(|p| !p.trim().is_empty());
        let kind = asked.kind.or_else(|| call.map(|c| c.kind));
        let mut said = said(&asked.asker, asked.input.as_ref());
        if asked.asker == Asker::Agent && plan.is_none() && said.detail.is_none() {
            said.detail = match (kind, call) {
                (Some(ToolKind::Execute), Some(c)) => Some(
                    crate::session::Tool {
                        call: c.clone(),
                        started: None,
                        took: None,
                        starts: Vec::new(),
                    }
                    .command_line(),
                ),
                (_, Some(c)) => {
                    let diffs: Vec<_> = (c.content.iter())
                        .filter_map(|x| match x {
                            ToolCallContent::Diff(d) => Some(crate::git::edit_diff(d, None)),
                            _ => None,
                        })
                        .collect();
                    let (added, gone) = diffs
                        .iter()
                        .fold((0, 0), |(a, g), d| (a + d.added, g + d.gone));
                    match diffs.len() {
                        0 => None,
                        1 => Some(format!("+{added} −{gone}")),
                        n => Some(
                            crate::tr_with(
                                "+%{added} −%{gone} · %{n} files",
                                &[("added", &added), ("gone", &gone), ("n", &n)],
                            )
                            .into(),
                        ),
                    }
                }
                _ => None,
            };
        }
        let title = match (&asked.asker, plan.is_some(), kind) {
            (_, true, _) => crate::tr("Proposed a plan"),
            (Asker::Agent, _, Some(ToolKind::Execute))
                if said.detail.as_deref() == Some(bare(&asked.title).as_str()) =>
            {
                crate::tr("Run a command")
            }
            (Asker::Agent, _, None | Some(ToolKind::Other)) if said.detail.is_none() => {
                said.detail = Some(bare(&asked.title));
                crate::tr_with("%{name} asks before it goes on", &[("name", &name)])
            }
            _ => SharedString::from(bare(&asked.title)),
        };
        let waiting = (asked.chosen.is_none())
            .then(|| asked.asked_at.elapsed().ok())
            .flatten()
            .filter(|d| d.as_secs() > 0)
            .map(|d| {
                crate::tr_with(
                    "waiting %{time}",
                    &[("time", &u0ui::t3::fmt_elapsed(d.as_secs()))],
                )
            });
        let who = match &asked.asker {
            Asker::Agent => SharedString::from(name.clone()),
            Asker::Tool(tool) => crate::tr_with(
                "%{name} asks to use u0’s %{tool}",
                &[("name", &name), ("tool", tool)],
            ),
            Asker::Outside(tool) => {
                crate::tr_with("An agent outside u0 · %{tool}", &[("tool", tool)])
            }
            Asker::Device(tool) => {
                crate::tr_with("An agent on another device · %{tool}", &[("tool", tool)])
            }
        };
        let provider = match (plan.as_ref().and(Self::mode_name(s)), waiting) {
            (Some(mode), _) => format!("{who} · {mode}"),
            (None, Some(w)) => format!("{who} · {w}"),
            (None, None) => who.to_string(),
        };
        let options: Vec<ApprovalOption> = asked.options.iter().map(option_of).collect();
        let (at, request) = (s.key, asked.id);
        let mut card = ApprovalCard::options(SharedString::from(format!("{key}-ask")), options)
            .kind(match (plan.is_some(), kind) {
                (true, _) | (_, Some(ToolKind::Edit | ToolKind::Delete | ToolKind::Move)) => {
                    ApprovalKind::File
                }
                (_, Some(ToolKind::Execute)) => ApprovalKind::Command,
                _ => ApprovalKind::Permission,
            })
            .tool(asked.asker != Asker::Agent)
            .title(title)
            .provider(provider)
            .facts(said.facts);
        if let Some(detail) = said.detail {
            card = card.detail(detail);
        }
        if let Some(reason) = said.reason {
            card = card.reason(reason);
        }
        if let Some(context) = said.context {
            card = card.context(context);
        }
        if let Some((option, kind)) = &asked.chosen {
            let cancelled = option == "Cancelled";
            let spoken = match kind {
                _ if cancelled => crate::tr("Cancelled"),
                PermissionOptionKind::AllowOnce => crate::tr("You allowed it once"),
                PermissionOptionKind::AllowAlways => crate::tr("You allowed it always"),
                PermissionOptionKind::RejectAlways => crate::tr("You rejected it always"),
                _ => crate::tr("You rejected it"),
            };
            let mut tail = Vec::new();
            if !cancelled {
                tail.push(format!("“{option}”"));
            }
            if let Some(t) = asked.answered_at {
                tail.push(when::said_at(
                    t,
                    std::time::SystemTime::now(),
                    self.settings.twelve,
                ));
            }
            if asked.noted {
                tail.push(crate::tr("with a note").into());
            }
            let allowed = matches!(
                kind,
                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
            );
            return card.answered(allowed, spoken, tail.join(" · "));
        }
        if let Some(plan) = &plan {
            card = card.plan(crate::markdown::render(
                faces,
                &format!("{key}-plan"),
                plan,
                cx,
            ));
        }
        let desk = cx.entity();
        let chosen = asked.options.clone();
        card = card.on_pick(move |n, _, cx| {
            if let Some(option) = chosen.get(n) {
                let option = option.clone();
                desk.update(cx, |this, cx| this.choose(at, request, &option, cx));
            }
        });
        // Turned down with a word: what to do instead, or what to change
        // in the plan, written on the card and said next.
        let tells = !asked
            .options
            .iter()
            .any(|o| o.name.to_lowercase().contains("tell "));
        if asked.asker == Asker::Agent && asked.no().is_some() && tells {
            let desk = cx.entity();
            card = card.extra_action(
                crate::tr("No, and tell it…"),
                u0ui::Icon::carbon(CarbonIcon::Chat),
                move |window, cx| {
                    desk.update(cx, |this, cx| {
                        this.begin_tell(at, request, false, window, cx);
                    });
                },
            );
        }
        if plan.is_some() && asked.no().is_some() {
            let desk = cx.entity();
            card = card.extra_action(
                crate::tr("Revise…"),
                u0ui::Icon::carbon(CarbonIcon::Edit),
                move |window, cx| {
                    desk.update(cx, |this, cx| {
                        this.begin_tell(at, request, true, window, cx);
                    });
                },
            );
        }
        if let Some((_, _, revise)) = self.telling.filter(|(k, r, _)| *k == at && *r == request) {
            card = card.editor(self.tell_editor(faces, key, &name, at, request, revise, cx));
        }
        card.note(Self::card_note(&name, asked))
    }

    /// The foot of a pending card: what its options are, and, for u0's
    /// tools, what Always allow does or why it is not offered, read from
    /// the options as the server sent them, not from who asks.
    fn card_note(name: &str, asked: &Asked) -> SharedString {
        let always = (asked.options.iter()).any(|o| o.kind == PermissionOptionKind::AllowAlways);
        match &asked.asker {
            Asker::Agent => {
                let first = u0ui::t3::timeline::default_option(
                    &asked.options.iter().map(option_of).collect::<Vec<_>>(),
                )
                .and_then(|n| asked.options.get(n));
                match first {
                    Some(o) => crate::tr_with(
                        "The options %{name} sent, in its order. Return takes “%{option}”.",
                        &[("name", &name), ("option", &o.name)],
                    ),
                    None => crate::tr_with(
                        "The options %{name} sent, in its order.",
                        &[("name", &name)],
                    ),
                }
            }
            Asker::Tool(tool) | Asker::Device(tool) if always => crate::tr_with(
                "“Always allow” lets %{tool} act from this thread without asking again.",
                &[("tool", tool)],
            ),
            Asker::Outside(_) => {
                crate::tr("An agent outside u0 shares one key: it is never allowed always.")
            }
            Asker::Tool(tool) | Asker::Device(tool)
                if crate::acp::EACH_TIME.contains(&tool.as_str()) =>
            {
                crate::tr_with(
                    "Asked each time: %{tool} can be steered by unvetted text.",
                    &[("tool", tool)],
                )
            }
            Asker::Tool(_) | Asker::Device(_) => {
                crate::tr("It acts on another thread, so a yes is for this once.")
            }
        }
    }

    /// In place of a card's options while you write what it should do
    /// instead (or change in the plan): the note, and the buttons that
    /// send it or go back.
    #[allow(
        clippy::too_many_arguments,
        reason = "the note box names its card, its agent, its question and which note it is"
    )]
    fn tell_editor(
        &self,
        faces: &Faces,
        key: &str,
        name: &str,
        at: u64,
        request: u64,
        revise: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if revise {
            crate::tr("What to change in the plan")
        } else {
            crate::tr_with("Tell %{name} what to do instead", &[("name", &name)])
        };
        let submit = crate::tr(if revise {
            "Send the changes"
        } else {
            "Reject once and send"
        });
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(meta(faces, label))
            .child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .bg(crate::theme::role(u0ui::tokens::ColorRole::Field02))
                    .border_b_1()
                    .border_color(crate::theme::role(u0ui::tokens::ColorRole::BorderStrong01))
                    .child(
                        gpui::component::input::Textarea::new(&self.tell_input).appearance(false),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button(
                            SharedString::from(format!("{key}-tell-send")),
                            submit,
                            Weight::Primary,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.reject_and_tell(at, request, revise, window, cx);
                            },
                        )),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("{key}-tell-back")),
                            crate::tr("Back to the options"),
                            Weight::Quiet,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.cancel_tell(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_call_says_what_it_runs_and_why_and_u0s_tool_its_fields() {
        let input = serde_json::json!({
            "description": "Runs the visual tests",
            "cwd": "/work/carbon-ui",
            "path": "src/tokens.json",
        });
        let agent = said(&Asker::Agent, Some(&input));
        assert_eq!(agent.detail.as_deref(), Some("src/tokens.json"));
        assert_eq!(agent.reason.as_deref(), Some("Runs the visual tests"));
        assert_eq!(agent.context.as_deref(), Some("/work/carbon-ui"));
        assert!(agent.facts.is_empty());
        let input =
            serde_json::json!({"text": "Rebase first.", "session": "s1", "tags": ["a", "b"]});
        let tool = said(&Asker::Tool("send_to_session".into()), Some(&input));
        assert_eq!(tool.detail.as_deref(), Some("Rebase first."));
        assert_eq!(
            tool.facts,
            [
                ("Thread".into(), "s1".into()),
                ("Tags".into(), "#a #b".into())
            ]
        );
    }

    #[test]
    fn u0s_tool_fields_are_named_in_words_with_the_pull_request_first() {
        let input =
            serde_json::json!({"how": "squash", "auto": true, "number": 483, "wait_secs": 5});
        let merge = said(&Asker::Tool("merge_pr".into()), Some(&input));
        assert_eq!(
            merge.facts,
            [
                ("Pull request".into(), "#483".into()),
                ("Method".into(), "squash".into()),
                ("Wait for checks".into(), "Yes".into()),
                ("wait secs".into(), "5".into()),
            ]
        );
    }

    #[test]
    fn a_device_offered_always_is_not_told_it_never_is() {
        let options = |json: &str| -> Vec<agent_client_protocol::v1::schema::PermissionOption> {
            serde_json::from_str(json).unwrap()
        };
        let with_always = options(
            r#"[{"optionId":"once","name":"Allow","kind":"allow_once"},
                {"optionId":"always","name":"Always allow","kind":"allow_always"},
                {"optionId":"no","name":"Reject","kind":"reject_once"}]"#,
        );
        let asked = |asker: Asker, options| Asked {
            id: 1,
            title: "Tap".into(),
            options,
            kind: None,
            asker,
            call: agent_client_protocol::v1::schema::ToolCallId::new("c"),
            input: None,
            asked_at: std::time::SystemTime::now(),
            chosen: None,
            answered_at: None,
            noted: false,
        };
        let device = asked(Asker::Device("use_device".into()), with_always.clone());
        let note = Desk::card_note("Claude", &device);
        assert!(note.contains("use_device"), "{note}");
        assert!(!note.contains("never"), "{note}");
        let merge = asked(Asker::Device("merge_pr".into()), with_always[..1].to_vec());
        assert!(Desk::card_note("Claude", &merge).contains("merge_pr"));
    }
}
