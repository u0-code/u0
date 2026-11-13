//! What a brain hands back: not a reply, a set of things she does.
//!
//! The format is plain JSON in the text of a completion, so it works on any
//! provider, including the CLI relays. Parsing is deliberately forgiving:
//! models wrap JSON in prose and code fences, write numbers as strings, and
//! invent fields. Anything unusable is dropped rather than failing the whole
//! deliberation.

use ghost_brain::{Call, ToolSpec};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use tracing::debug;

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
    /// Look something up on the web, then think again. Only where what
    /// she does is tool calls.
    LookUp { query: String },
    /// End a relationship.
    Leave {
        #[serde(default)]
        why: String,
    },
    /// Post publicly, to no one in particular.
    Post { text: String },
    /// Hand over a file she made. Only where what she does is tool calls.
    SendFile {
        file: String,
        #[serde(default)]
        text: Option<String>,
    },
    /// Take on something that needs real work, to be done in her own time.
    Undertake {
        brief: String,
        /// Who it is for. Absent when it is for herself.
        #[serde(default, rename = "for")]
        for_whom: Option<String>,
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

/// What a brain that takes tools is offered while she looks at a place.
/// The names and arguments are those of [`Action`], so a call reads straight
/// into one.
pub fn tools(work: Option<&str>, look_up: bool, files: bool) -> Vec<ToolSpec> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| ToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        parameters: json!({"type": "object", "properties": properties, "required": required}),
    };
    let text = |description: &str| json!({"type": "string", "description": description});
    let mut tools = vec![
        tool(
            "say",
            "Send one message bubble. Call it several times for several bubbles.",
            json!({
                "text": text("The message, as you would type it."),
                "reply_to": {"type": "integer", "description": "The m number of an earlier message, only when you are quoting that specific message."},
            }),
            &["text"],
        ),
        tool(
            "react",
            "React to a message, instead of or as well as replying.",
            json!({
                "to": {"type": "integer", "description": "The m number of the message."},
                "emoji": text("One emoji."),
            }),
            &["to", "emoji"],
        ),
        tool(
            "later",
            "Not now. You will come back to this conversation yourself in about this many minutes.",
            json!({"minutes": {"type": "number"}, "why": text("Why not now.")}),
            &["minutes"],
        ),
        tool(
            "note",
            "Keep something about a person for a long time. Facts and impressions, not a summary of the chat.",
            json!({"about": text("Their name."), "text": text("What to remember.")}),
            &["text"],
        ),
        tool(
            "feel",
            "This moment shifted how you feel about someone. Most moments shift nothing and need no such call.",
            json!({
                "toward": text("Their name."),
                "delta": {"type": "number", "description": "From -3 to 3."},
                "why": text("What did it."),
            }),
            &["delta"],
        ),
        tool(
            "intend",
            "Something you mean to do or ask later, such as finding out how their interview went.",
            json!({
                "hours": {"type": "number", "description": "How many hours from now."},
                "what": text("What you mean to do."),
                "about": text("Whom it concerns."),
            }),
            &["hours", "what"],
        ),
        tool(
            "canon",
            "Required whenever a message of yours states something about your own life, habits or history that is not already listed as true: a place you go, a thing you own, something you did. One plain sentence, so that you say the same thing next month.",
            json!({"fact": text("The fact.")}),
            &["fact"],
        ),
        tool(
            "recall",
            "Search your memory for something older before answering. You get what you find, and another turn.",
            json!({"query": text("A few words.")}),
            &["query"],
        ),
    ];
    if look_up {
        tools.push(tool(
            "look_up",
            "Look something up on the web, the way you would glance at a search on your phone in the middle of a conversation: a fact, a date, what something is. You get what comes back, and another turn. For anything that takes real work, take it on instead.",
            json!({"query": text("What to search for.")}),
            &["query"],
        ));
    }
    if let Some(means) = work {
        tools.push(tool(
            "undertake",
            &format!(
                "Take on something that needs real work {means}: finding something out properly, making something, checking something. \
                 It is not done in this reply. You will work on it in your own time and come back with what you found. \
                 If you have already taken something on for them and they add to it or change it, call this again with just what is new: it is added to what you are doing. \
                 Only for things you want to do or have agreed to do: nobody is owed your afternoon. Say something as well, the way you would."
            ),
            json!({
                "brief": text("A complete instruction to yourself, with everything needed to do it without this conversation in front of you."),
                "for": text("Who it is for. Leave out when it is for yourself."),
            }),
            &["brief"],
        ));
    }
    if files {
        tools.push(tool(
            "send_file",
            "Hand over a file you made, in this conversation. It arrives as a file, not as text.",
            json!({
                "file": text("The file, as it was named to you."),
                "text": text("A line to go with it, if you have one."),
            }),
            &["file"],
        ));
    }
    tools.push(tool(
        "leave",
        "End this relationship for good. Rare and grave, and only after things have already gone cold.",
        json!({"why": text("Why.")}),
        &[],
    ));
    tools
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
    fn recall_is_surfaced() {
        let d = parse(r#"{"actions":[{"do":"recall","query":"interview"}]}"#).unwrap();
        assert_eq!(d.recall(), Some("interview"));
    }
}
