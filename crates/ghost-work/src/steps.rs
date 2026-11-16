//! What one call leaves behind where someone watching can read it: what
//! was asked for, the head of what came back, and how long it took.
//!
//! The step log of a piece of work and the record of a look in
//! conversation keep their calls in this one shape, so that whoever reads
//! either reads the same thing. It is a glimpse, for showing the work as it
//! goes: the whole of what came back is in the work's history, or was in
//! front of her, and is not kept again here.

use serde_json::{Value, json};

/// How much of what a call returned is kept: enough to see what happened.
pub const OUT_KEPT: usize = 1500;
/// How long a string among a call's arguments may be before only its head
/// is kept. A file written whole is in the file, not in the log.
pub const ARG_KEPT: usize = 400;

/// One call as the logs keep it: its `name`, its `args` with any long
/// string cut to its head, the head of its output as `out`, whether that
/// was `cut` and how many `chars` the whole of it had, how long it took in
/// `ms`, whether it `failed`, and how many `pictures` came back with it.
pub fn call(
    name: &str,
    args: &Value,
    output: &str,
    failed: bool,
    pictures: usize,
    ms: u128,
) -> Value {
    let chars = output.chars().count();
    json!({
        "name": name,
        "args": clipped(args),
        "out": output.chars().take(OUT_KEPT).collect::<String>(),
        "cut": chars > OUT_KEPT,
        "chars": chars,
        "ms": ms,
        "failed": failed,
        "pictures": pictures,
    })
}

/// A call as it is shown to a reader, from what a log kept: a line written
/// before calls kept their output, or by a server of tools, reads with
/// what it has and the rest left empty.
pub fn shown(kept: &Value) -> Value {
    json!({
        "name": kept["name"].as_str().unwrap_or_default(),
        "args": kept.get("args").cloned().unwrap_or_else(|| json!({})),
        "out": kept["out"].as_str().unwrap_or_default(),
        "cut": kept["cut"].as_bool().unwrap_or(false),
        "chars": kept["chars"].as_u64().unwrap_or(0),
        "ms": kept["ms"].as_u64().unwrap_or(0),
        "failed": kept["failed"].as_bool().unwrap_or(false),
        "pictures": kept["pictures"].as_u64().unwrap_or(0),
    })
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

    #[test]
    fn a_long_output_keeps_its_head_and_says_how_long_it_was() {
        let output = "é".repeat(OUT_KEPT + 10);
        let kept = call("shell", &json!({"command": "ls"}), &output, false, 0, 12);
        assert_eq!(kept["out"].as_str().unwrap().chars().count(), OUT_KEPT);
        assert_eq!(kept["cut"], true);
        assert_eq!(kept["chars"], OUT_KEPT + 10);
        assert_eq!(kept["args"], json!({"command": "ls"}));
        let short = call("read", &json!({}), "hi", false, 0, 1);
        assert_eq!(
            (short["out"].as_str(), short["cut"].as_bool()),
            (Some("hi"), Some(false))
        );
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
        assert_eq!(kept["args"]["path"], "notes/a.md");
        let cut = kept["args"]["content"].as_str().unwrap();
        assert_eq!(cut.chars().count(), ARG_KEPT + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_call_logged_before_outputs_were_kept_still_reads() {
        let old = json!({"name": "shell", "args": "{\"command\":\"ls\"}", "ms": 4, "failed": false, "chars": 9, "pictures": 0});
        let shown = shown(&old);
        assert_eq!(shown["out"], "");
        assert_eq!(shown["cut"], false);
        assert_eq!(shown["args"], "{\"command\":\"ls\"}");
        assert_eq!(shown["chars"], 9);
    }
}
