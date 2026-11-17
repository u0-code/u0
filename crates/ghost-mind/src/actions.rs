//! What a brain hands back: not a reply, a set of things she does.
//!
//! The format is plain JSON in the text of a completion, so it works on any
//! provider, including the CLI relays. Parsing is deliberately forgiving:
//! models wrap JSON in prose and code fences, write numbers as strings, and
//! invent fields. Anything unusable is dropped rather than failing the whole
//! deliberation.

use ghost_brain::{Call, ToolSpec};
use ghost_core::narrate;
use ghost_work::catalog::{self, Need};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};
use tracing::debug;

use crate::Prompts;

/// One thing she decides to do.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    /// Send one message bubble.
    Say {
        text: String,
        #[serde(default, deserialize_with = "opt_reference")]
        reply_to: Option<i64>,
    },
    /// React to a message.
    React {
        #[serde(deserialize_with = "reference")]
        to: i64,
        emoji: String,
    },
    /// Read it and say nothing.
    Silence {
        #[serde(default)]
        why: String,
    },
    /// Come back to this conversation later, on her own.
    Later {
        #[serde(deserialize_with = "number")]
        minutes: f64,
        #[serde(default)]
        why: String,
    },
    /// Keep something about a person.
    Note {
        #[serde(default)]
        about: Option<String>,
        text: String,
    },
    /// A shift in how she feels about someone.
    Feel {
        #[serde(default)]
        toward: Option<String>,
        #[serde(deserialize_with = "number")]
        delta: f64,
        #[serde(default)]
        why: String,
    },
    /// Something to do or ask later.
    Intend {
        #[serde(deserialize_with = "number")]
        hours: f64,
        what: String,
        #[serde(default)]
        about: Option<String>,
    },
    /// A new hard fact about her own life.
    Canon { fact: String },
    /// Search her memory, then think again.
    Recall { query: String },
    /// Go back to what one memory was made from, then think again.
    Reread {
        #[serde(deserialize_with = "reference")]
        memory: i64,
    },
    /// Hold back in a group for a while, having been asked to: look only
    /// at what is said to her there, for this many hours.
    Quiet {
        #[serde(deserialize_with = "number")]
        hours: f64,
    },
    /// End a relationship.
    Leave {
        #[serde(default)]
        why: String,
    },
    /// Post publicly, to no one in particular.
    Post { text: String },
    /// Hand over something she has: a thing she kept, by its number or its
    /// file name; an image in a message in this conversation, by `message`
    /// and `image` (counting from 1) as with `Keep`; or a file she made, by
    /// its name.
    SendFile {
        #[serde(default)]
        file: Option<String>,
        #[serde(default, deserialize_with = "opt_reference")]
        message: Option<i64>,
        #[serde(default, deserialize_with = "opt_reference")]
        image: Option<i64>,
        #[serde(default)]
        text: Option<String>,
        /// What it is, in her words, for her own line about what she sent.
        #[serde(default)]
        what: Option<String>,
    },
    /// Keep something she has seen, to have it again: an image in a
    /// message, by `message` and `image` (counting from 1, the first when
    /// left out), or an image she opened in this conversation, by `file`.
    Keep {
        #[serde(default, deserialize_with = "opt_reference")]
        message: Option<i64>,
        #[serde(default, deserialize_with = "opt_reference")]
        image: Option<i64>,
        #[serde(default)]
        file: Option<String>,
        note: String,
    },
    /// Let go of something she kept. `thing` is its number.
    LetGo {
        #[serde(deserialize_with = "reference")]
        thing: i64,
    },
    /// Take on something that needs real work, to be done in her own time.
    Undertake {
        brief: String,
        /// Who it is for. Absent when it is for herself.
        #[serde(default, rename = "for")]
        for_whom: Option<String>,
    },
    /// Pass something on to work she has under way. `work` is the number
    /// she knows it by.
    Steer {
        #[serde(deserialize_with = "reference")]
        work: i64,
        text: String,
    },
    /// Stop work she has under way, for good.
    Stop {
        #[serde(deserialize_with = "reference")]
        work: i64,
    },
}

/// The outcome of one look at something.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Deliberation {
    /// One private line. Journaled as the reason for whatever she did.
    pub thought: String,
    pub actions: Vec<Action>,
}

impl Deliberation {
    /// The memory query she asked for, if she asked for one.
    pub fn recall(&self) -> Option<&str> {
        self.actions.iter().find_map(|a| match a {
            Action::Recall { query } if !query.trim().is_empty() => Some(query.as_str()),
            _ => None,
        })
    }

    /// The memory she asked to go back to, if she asked.
    pub fn reread(&self) -> Option<i64> {
        self.actions.iter().find_map(|a| match a {
            Action::Reread { memory } => Some(*memory),
            _ => None,
        })
    }
}

/// One thing she does, from a tool call. A call that cannot be read comes
/// back as a sentence saying why, for the brain to put right.
pub fn from_call(call: &Call) -> Result<Action, String> {
    let Value::Object(args) = &call.args else {
        return Err(format!(
            "`{}` was called with something that is not a JSON object. Call it again with well-formed arguments.",
            call.name
        ));
    };
    let mut tagged = args.clone();
    tagged.insert("do".into(), Value::String(call.name.clone()));
    serde_json::from_value(Value::Object(tagged)).map_err(|e| {
        format!(
            "`{}` could not be used: {e}. Nothing was done. Check its arguments and call it again.",
            call.name
        )
    })
}

/// What a look at a place offers beyond what is always there.
#[derive(Debug, Clone, Default)]
pub struct Offer {
    /// How she would take something on, if she can: what fills `{{means}}`
    /// in `undertake`.
    pub means: Option<String>,
    /// What the place and she have here, as the catalog of tools asks it:
    /// a place that takes files, work under way, a group, a place among
    /// people, a channel that reacts.
    pub here: Vec<Need>,
    /// The tools that reach beyond the conversation, offered here, as they
    /// are said here: the web, and her machine.
    pub reaching: Vec<ToolSpec>,
}

impl Offer {
    /// What she has here, as the catalog of tools asks it.
    fn has(&self, need: Need) -> bool {
        match need {
            Need::Work => self.means.is_some(),
            Need::Memory => true,
            Need::UnderWay | Need::Files | Need::Group | Need::Social | Need::React => {
                self.here.contains(&need)
            }
            // Those reach beyond the conversation, and are offered as
            // they are given.
            _ => false,
        }
    }
}

/// Arguments by name and JSON type, and which of them are required.
type Shape = (
    &'static [(&'static str, &'static str)],
    &'static [&'static str],
);

/// The arguments of each thing she does in a conversation. A tool that
/// runs the same in work is shaped there instead.
fn shape(name: &str) -> Option<Shape> {
    Some(match name {
        "say" => (&[("text", "string"), ("reply_to", "integer")], &["text"]),
        "react" => (&[("to", "integer"), ("emoji", "string")], &["to", "emoji"]),
        "later" => (&[("minutes", "number"), ("why", "string")], &["minutes"]),
        "note" => (&[("about", "string"), ("text", "string")], &["text"]),
        "feel" => (
            &[("toward", "string"), ("delta", "number"), ("why", "string")],
            &["delta"],
        ),
        "intend" => (
            &[("hours", "number"), ("what", "string"), ("about", "string")],
            &["hours", "what"],
        ),
        "canon" => (&[("fact", "string")], &["fact"]),
        "recall" => (&[("query", "string")], &["query"]),
        "reread" => (&[("memory", "integer")], &["memory"]),
        "keep" => (
            &[
                ("file", "string"),
                ("message", "integer"),
                ("image", "integer"),
                ("note", "string"),
            ],
            &["note"],
        ),
        "let_go" => (&[("thing", "integer")], &["thing"]),
        "undertake" => (&[("brief", "string"), ("for", "string")], &["brief"]),
        "steer" => (
            &[("work", "integer"), ("text", "string")],
            &["work", "text"],
        ),
        "stop" => (&[("work", "integer")], &["work"]),
        "send_file" => (
            &[
                ("file", "string"),
                ("message", "integer"),
                ("image", "integer"),
                ("text", "string"),
                ("what", "string"),
            ],
            &[],
        ),
        "quiet" => (&[("hours", "number")], &["hours"]),
        "leave" => (&[("why", "string")], &[]),
        _ => return None,
    })
}

/// What a brain that takes tools is offered while she looks at a place, in
/// the catalog's order. What she does with people in front of her is read
/// straight into an [`Action`]; what reaches beyond the conversation runs
/// as it does at work. What each is said to be is in `tools.toml`.
pub fn tools(words: &Prompts, offer: &Offer) -> Vec<ToolSpec> {
    // Each argument by name and JSON type, described if `tools.toml` says
    // something about it.
    let tool = |name: &str, arguments: &[(&str, &str)], required: &[&str]| {
        let properties: Map<String, Value> = arguments
            .iter()
            .map(|(argument, kind)| {
                let mut schema = json!({"type": kind});
                if let Some(said) = words.parameter(name, argument) {
                    schema["description"] = Value::String(said.to_owned());
                }
                ((*argument).to_owned(), schema)
            })
            .collect();
        ToolSpec {
            name: name.to_owned(),
            description: words.tool(name).to_owned(),
            parameters: json!({"type": "object", "properties": properties, "required": required}),
        }
    };
    let mut tools = Vec::new();
    for entry in catalog::TOOLS {
        if !entry
            .loops
            .contains(&ghost_core::prompts::Loop::Conversation)
        {
            continue;
        }
        if let Some(reaching) = offer.reaching.iter().find(|t| t.name == entry.name) {
            tools.push(reaching.clone());
            continue;
        }
        let Some((arguments, required)) = shape(entry.name) else {
            continue;
        };
        if !entry.needs.iter().all(|need| offer.has(*need)) {
            continue;
        }
        let mut spec = tool(entry.name, arguments, required);
        if let Some(means) = offer.means.as_deref().filter(|_| entry.name == "undertake") {
            spec.description = narrate::fill(&spec.description, &[("means", means.to_owned())]);
        }
        tools.push(spec);
    }
    // What a tool brought with it, such as the tools of a server she
    // loaded, is in no catalog: it comes after everything that is.
    tools.extend(
        offer
            .reaching
            .iter()
            .filter(|t| catalog::find(&t.name).is_none())
            .cloned(),
    );
    tools
}

/// What she can do here, as a brain that answers in JSON is told it: the
/// same list a brain that takes tools is offered, so that both are offered
/// the same things in the same place. Each is the object to write, what it
/// is, and what its fields are for.
pub fn listing(tools: &[ToolSpec]) -> String {
    let mut out = String::new();
    for tool in tools {
        let mut object = format!("{{\"do\":\"{}\"", tool.name);
        let mut fields = Vec::new();
        if let Some(properties) = tool.parameters["properties"].as_object() {
            for (field, schema) in properties {
                let kind = schema["type"].as_str().unwrap_or("string");
                let shown = if kind == "string" {
                    "\"...\"".to_owned()
                } else {
                    format!("<{kind}>")
                };
                object.push_str(&format!(",\"{field}\":{shown}"));
                if let Some(said) = schema["description"].as_str() {
                    fields.push(format!("  \"{field}\": {said}\n"));
                }
            }
        }
        object.push('}');
        out.push_str(&format!("- {object}\n  {}\n", tool.description));
        out.extend(fields);
    }
    out
}

/// Read a deliberation out of whatever a brain wrote.
pub fn parse(text: &str) -> Result<Deliberation, String> {
    let Some(value) = object(text) else {
        // No JSON at all. A short plain answer is most likely just what she
        // would say, from a model that ignored the format.
        let plain = text.trim();
        if !plain.is_empty() && plain.chars().count() <= 600 && !plain.contains('{') {
            return Ok(Deliberation {
                thought: String::new(),
                actions: vec![Action::Say {
                    text: plain.to_owned(),
                    reply_to: None,
                }],
            });
        }
        return Err(format!("no JSON object in: {}", clip(text, 200)));
    };
    let thought = value["thought"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let raw = match &value["actions"] {
        Value::Array(items) => items.clone(),
        Value::Null if value.get("do").is_some() => vec![value.clone()],
        Value::Null => Vec::new(),
        other => return Err(format!("\"actions\" is not a list: {other}")),
    };
    let actions = raw
        .into_iter()
        .filter_map(
            |item| match serde_json::from_value::<Action>(item.clone()) {
                Ok(action) => Some(action),
                Err(e) => {
                    debug!("dropping an action she cannot take: {e}: {item}");
                    None
                }
            },
        )
        .collect();
    Ok(Deliberation { thought, actions })
}

/// The first complete JSON object in `text`, if there is one.
pub fn object(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let mut start = text.find('{')?;
    loop {
        if let Some(end) = matching_brace(bytes, start)
            && let Ok(value @ Value::Object(_)) = serde_json::from_str(&text[start..=end])
        {
            return Some(value);
        }
        start += 1 + text[start + 1..].find('{')?;
    }
}

fn matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// A message reference: `12`, `"12"`, `"m12"`, `"[i3]"`.
fn reference<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    as_reference(&Value::deserialize(d)?).ok_or_else(|| serde::de::Error::custom("not a reference"))
}

fn opt_reference<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(as_reference(&Value::deserialize(d)?))
}

fn as_reference(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok(),
        _ => None,
    }
}

/// A number, however the model chose to write it.
fn number<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_start_matches('+').parse().ok(),
        _ => None,
    }
    .ok_or_else(|| serde::de::Error::custom("not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_clean_object() {
        let d = parse(
            r#"{"thought":"hm","actions":[{"do":"say","text":"hey"},{"do":"say","text":"what's up","reply_to":"m12"},{"do":"react","to":12,"emoji":"👍"}]}"#,
        )
        .unwrap();
        assert_eq!(d.thought, "hm");
        assert_eq!(d.actions.len(), 3);
        assert_eq!(
            d.actions[1],
            Action::Say {
                text: "what's up".into(),
                reply_to: Some(12)
            }
        );
    }

    #[test]
    fn digs_json_out_of_prose_and_fences() {
        let text = "Sure, here you go {not json}:\n```json\n{\"thought\": \"a } in a string\", \"actions\": [{\"do\": \"silence\"}]}\n```\nDone.";
        let d = parse(text).unwrap();
        assert_eq!(d.thought, "a } in a string");
        assert_eq!(d.actions, vec![Action::Silence { why: String::new() }]);
    }

    #[test]
    fn drops_what_it_cannot_use_and_keeps_the_rest() {
        let d = parse(
            r#"{"actions":[{"do":"dance"},{"do":"feel","delta":"+2","toward":"Haoran"},{"do":"later","minutes":"90"},{"do":"react","emoji":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(d.actions.len(), 2);
        assert!(
            matches!(&d.actions[0], Action::Feel { delta, .. } if (*delta - 2.0).abs() < f64::EPSILON)
        );
        assert!(
            matches!(&d.actions[1], Action::Later { minutes, .. } if (*minutes - 90.0).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn a_bare_action_and_an_empty_list_both_work() {
        let d = parse(r#"{"do":"say","text":"ok"}"#).unwrap();
        assert_eq!(d.actions.len(), 1);
        let d = parse(r#"{"thought":"nothing to add","actions":[]}"#).unwrap();
        assert!(d.actions.is_empty());
    }

    #[test]
    fn plain_text_is_what_she_says() {
        let d = parse("yeah, I'm around").unwrap();
        assert_eq!(
            d.actions,
            vec![Action::Say {
                text: "yeah, I'm around".into(),
                reply_to: None
            }]
        );
        assert!(parse("").is_err());
        assert!(parse("{\"thought\": \"cut off").is_err());
    }

    #[test]
    fn keeping_reads_a_message_number_and_counts_images_from_one() {
        let d = parse(
            r#"{"actions":[{"do":"keep","message":"m12","note":"for later"},{"do":"keep","message":12,"image":"2","note":"x"},{"do":"let_go","thing":"[40]"}]}"#,
        )
        .unwrap();
        assert_eq!(
            d.actions,
            vec![
                Action::Keep {
                    message: Some(12),
                    image: None,
                    file: None,
                    note: "for later".into()
                },
                Action::Keep {
                    message: Some(12),
                    image: Some(2),
                    file: None,
                    note: "x".into()
                },
                Action::LetGo { thing: 40 },
            ]
        );
    }

    #[test]
    fn handing_over_reads_a_name_a_number_or_a_message_and_its_image() {
        let d = parse(
            r##"{"actions":[{"do":"send_file","file":"#40"},{"do":"send_file","message":"m12","image":"2","text":"this"}]}"##,
        )
        .unwrap();
        assert_eq!(
            d.actions,
            vec![
                Action::SendFile {
                    file: Some("#40".into()),
                    message: None,
                    image: None,
                    text: None,
                    what: None
                },
                Action::SendFile {
                    file: None,
                    message: Some(12),
                    image: Some(2),
                    text: Some("this".into()),
                    what: None
                },
            ]
        );
    }

    #[test]
    fn work_is_named_by_its_number_however_it_is_written() {
        let d = parse(
            r##"{"actions":[{"do":"steer","work":"#41","text":"weekdays only"},{"do":"stop","work":41}]}"##,
        )
        .unwrap();
        assert_eq!(
            d.actions,
            vec![
                Action::Steer {
                    work: 41,
                    text: "weekdays only".into()
                },
                Action::Stop { work: 41 },
            ]
        );
    }

    #[test]
    fn recall_is_surfaced() {
        let d = parse(r#"{"actions":[{"do":"recall","query":"interview"}]}"#).unwrap();
        assert_eq!(d.recall(), Some("interview"));
    }
}
