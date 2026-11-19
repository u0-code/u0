//! What one call leaves behind where someone watching can read it: what
//! was asked for, the head of what came back, and how long it took; and
//! the line a step of work leaves in its log.
//!
//! The step log of a piece of work and the record of a look in
//! conversation keep their calls in this one shape, so that whoever reads
//! either reads the same thing. It is a glimpse, for showing the work as it
//! goes: the whole of what came back is in the work's history, or was in
//! front of her, and is not kept again here.

use std::collections::BTreeMap;

use ghost_brain::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How much of what a call returned is kept: enough to see what happened.
const OUT_KEPT: usize = 1500;
/// How long a string among a call's arguments may be before only its head
/// is kept. A file written whole is in the file, not in the log.
const ARG_KEPT: usize = 400;

/// One call as the logs keep it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Called {
    pub name: String,
    /// Its arguments, with any long string cut to its head.
    pub args: Value,
    /// The head of what came back.
    pub out: String,
    /// Whether `out` is only the head of it.
    pub cut: bool,
    /// How many characters came back in all.
    pub chars: u64,
    /// How long it took.
    pub ms: u64,
    pub failed: bool,
    /// How many pictures came back with it.
    pub pictures: u64,
}

/// `name` called with `args`, which brought back `output` and `pictures`
/// pictures in `ms` milliseconds, as the logs keep it.
pub fn call(
    name: &str,
    args: &Value,
    output: &str,
    failed: bool,
    pictures: usize,
    ms: u128,
) -> Called {
    let chars = output.chars().count();
    Called {
        name: name.to_owned(),
        args: clipped(args),
        out: output.chars().take(OUT_KEPT).collect(),
        cut: chars > OUT_KEPT,
        chars: chars as u64,
        ms: u64::try_from(ms).unwrap_or(u64::MAX),
        failed,
        pictures: pictures as u64,
    }
}

/// One line of a piece of work's step log: one step, as the work's own
/// loop took it, or as a command-line agent handed a part told of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// When it was taken, RFC 3339.
    pub at: String,
    /// Its number in the sitting that took it.
    pub step: u64,
    /// What the model said, as written.
    pub said: String,
    pub calls: Vec<Called>,
    /// What the model was asked with and what that cost, where the work's
    /// own loop asked it. An agent's steps do not say.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub asked: Option<Asked>,
}

/// What the work's own loop put in front of the model at a step, and what
/// that cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked {
    pub usage: Usage,
    /// How long the model took to answer.
    pub model_ms: u64,
    /// What it was given, in characters, by part.
    pub context: BTreeMap<String, u64>,
    /// The model's context window in tokens, where it is known.
    pub window: Option<u64>,
    /// The size in tokens past which the transcript is folded.
    pub folds_at: Option<u64>,
}

/// The arguments as they were given, with every string longer than
/// [`ARG_KEPT`] cut to its head. The shape is kept, so that a reader can
/// still find the path a file was written to beside its contents.
fn clipped(args: &Value) -> Value {
    match args {
        Value::String(s) if s.chars().count() > ARG_KEPT => {
            Value::String(format!("{}…", s.chars().take(ARG_KEPT).collect::<String>()))
        }
        Value::Array(items) => Value::Array(items.iter().map(clipped).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), clipped(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_long_output_keeps_its_head_and_says_how_long_it_was() {
        let output = "é".repeat(OUT_KEPT + 10);
        let kept = call("shell", &json!({"command": "ls"}), &output, false, 0, 12);
        assert_eq!(kept.out.chars().count(), OUT_KEPT);
        assert!(kept.cut);
        assert_eq!(kept.chars, OUT_KEPT as u64 + 10);
        assert_eq!(kept.args, json!({"command": "ls"}));
        let short = call("read", &json!({}), "hi", false, 0, 1);
        assert_eq!((short.out.as_str(), short.cut), ("hi", false));
    }

    #[test]
    fn long_arguments_keep_their_shape_and_lose_only_their_length() {
        let content = "x".repeat(ARG_KEPT * 3);
        let kept = call(
            "write",
            &json!({"path": "notes/a.md", "content": content}),
            "ok",
            false,
            0,
            3,
        );
        assert_eq!(kept.args["path"], "notes/a.md");
        let cut = kept.args["content"].as_str().unwrap();
        assert_eq!(cut.chars().count(), ARG_KEPT + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_step_an_agent_took_reads_back_without_what_the_model_was_asked_with() {
        let line = Line {
            at: "2026-11-19T08:00:00+00:00".into(),
            step: 1,
            said: "looking".into(),
            calls: vec![call("Bash", &json!({"command": "ls"}), "a", false, 0, 3)],
            asked: None,
        };
        let text = serde_json::to_string(&line).unwrap();
        assert!(!text.contains("usage"), "{text}");
        assert_eq!(serde_json::from_str::<Line>(&text).unwrap(), line);
    }
}
