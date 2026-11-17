//! A call she made, said the way a coding agent's app says it: a verb in
//! the past tense and the one argument that tells what it was about.
//! `shell` with `du -sh ~` is "Ran du -sh ~"; `read` is "Read src/main.rs";
//! a tool of a server she installed is "Used" with its server and its
//! name. Everything here is text, so that it can be tested without a
//! window; `steps.rs` draws it.

use serde_json::Value;

/// The calls that are her deciding what to do in the conversation: what
/// they did is in the thread itself, as her reply or her reaction, so a
/// row for them would only say it twice.
const DECISIONS: [&str; 2] = ["say", "react"];

/// Whether a call is worth a row of its own.
pub fn shown(name: &str) -> bool {
    !DECISIONS.contains(&name)
}

/// How many characters a key argument keeps: the row trails off well
/// before this, and a whole file written is no argument to read in a row.
const KEY_KEPT: usize = 240;

/// One call, as a row says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// What was done: "Ran", "Read", "Used playwright · navigate".
    pub verb: String,
    /// What it was done to, on one line: a command, a path, a query in
    /// quotes. Empty when nothing says it better than the verb.
    pub what: String,
}

/// The arguments as an object: a call logged before calls kept their
/// arguments as JSON has them as a string of JSON.
fn object(args: &Value) -> Value {
    match args {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

fn arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// A number argument, given as a number or as its digits.
fn number(args: &Value, key: &str) -> Option<i64> {
    match args.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().trim_start_matches('#').parse().ok(),
        _ => None,
    }
}

/// The first string among the arguments, for a tool the desk does not know.
fn first_string(args: &Value) -> &str {
    args.as_object()
        .and_then(|o| o.values().find_map(Value::as_str))
        .unwrap_or_default()
}

fn quoted(s: &str) -> String {
    if s.trim().is_empty() {
        String::new()
    } else {
        format!("\u{201c}{}\u{201d}", s.trim())
    }
}

/// An address without its scheme: the host and the path are what a person
/// recognises.
fn address(url: &str) -> &str {
    url.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
}

/// Say the call `name` made with `args`.
pub fn say(name: &str, args: &Value) -> Said {
    let args = object(args);
    let a = |key| arg(&args, key);
    let (verb, what): (&str, String) = match name {
        "shell" => match number(&args, "job") {
            Some(job) => ("Checked", format!("job {job}")),
            None if args.get("background").and_then(Value::as_bool) == Some(true) => {
                ("Started", a("command").to_owned())
            }
            None => ("Ran", command(a("command")).to_owned()),
        },
        "read" => ("Read", a("path").to_owned()),
        "write" => ("Wrote", a("path").to_owned()),
        "edit" => ("Edited", a("path").to_owned()),
        "search" => ("Searched", quoted(a("query"))),
        "fetch" => ("Opened", address(a("url")).to_owned()),
        "recall" => ("Recalled", quoted(a("query"))),
        "reread" => (
            "Reread",
            number(&args, "memory").map_or_else(String::new, |m| format!("memory {m}")),
        ),
        "note" => ("Noted", a("text").to_owned()),
        "plan" => {
            let n = args
                .get("steps")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            ("Planned", plural(n, "step"))
        }
        "done" => ("Ended the work", a("outcome").replace('_', " ")),
        "handoff" => ("Wrote a handoff", String::new()),
        "ask" => ("Asked", a("question").to_owned()),
        "tell" => ("Told", a("text").to_owned()),
        "delegate" => ("Handed out", a("task").to_owned()),
        "wait" => (
            "Waited for parts",
            number(&args, "seconds").map_or_else(String::new, |s| format!("up to {s}s")),
        ),
        "parts" => ("Looked over the parts", String::new()),
        "steer" | "stop" => {
            let verb = if name == "steer" {
                "Steered"
            } else {
                "Stopped"
            };
            let whom = number(&args, "part")
                .map(|p| format!("part {p}"))
                .or_else(|| number(&args, "work").map(|w| format!("work #{w}")))
                .unwrap_or_default();
            (verb, whom)
        }
        "undertake" => ("Took on", a("brief").to_owned()),
        "keep" => ("Kept", first_of(&args, &["file", "note"])),
        "let_go" => ("Let go of", first_of(&args, &["thing"])),
        "send_file" => ("Sent", first_of(&args, &["file", "what"])),
        "load_tools" => ("Loaded the tools of", a("server").to_owned()),
        "find_tools" => ("Looked for tools", quoted(a("query"))),
        "install" => ("Installed", a("name").to_owned()),
        "uninstall" => ("Uninstalled", a("name").to_owned()),
        "find_skills" => ("Looked for skills", quoted(a("query"))),
        "install_skill" => ("Installed the skill", a("id").to_owned()),
        "uninstall_skill" => ("Uninstalled the skill", a("name").to_owned()),
        "use_skill" => ("Read the skill", a("name").to_owned()),
        "feel" => ("Felt", a("toward").to_owned()),
        "intend" => ("Meant to", a("what").to_owned()),
        "canon" => ("Settled", a("fact").to_owned()),
        "later" => ("Put it off", a("why").to_owned()),
        "quiet" => ("Went quiet", String::new()),
        "leave" => ("Left", String::new()),
        "say" => ("Said", a("text").to_owned()),
        "react" => ("Reacted", a("emoji").to_owned()),
        _ => {
            // A tool of a server she has is `server__tool`.
            let used = match name.split_once("__") {
                Some((server, tool)) => format!("Used {server} · {tool}"),
                None => format!("Used {name}"),
            };
            return Said {
                verb: used,
                what: one_line(first_string(&args), KEY_KEPT),
            };
        }
    };
    Said {
        verb: verb.to_owned(),
        what: one_line(&what, KEY_KEPT),
    }
}

/// A command as a row says it: without the `cd somewhere &&` it began
/// with, which is the same for every command of a piece of work and would
/// leave rows that look alike once cut to the width of the column. Where it
/// ran is in what it printed, a click away.
fn command(text: &str) -> &str {
    let t = text.trim_start();
    if let Some(rest) = t.strip_prefix("cd ")
        && let Some((_, after)) = rest.split_once("&&")
        && !after.trim().is_empty()
    {
        return after.trim_start();
    }
    t
}

fn first_of(args: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| match args.get(*k)? {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// `text` on one line, its runs of space and its line breaks each made one
/// space, and at most `most` characters, ending in `…` when it was longer.
pub fn one_line(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= most {
        flat
    } else {
        let mut cut: String = flat.chars().take(most.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

/// What came back, as the row opens to it: the head that was kept, and a
/// line saying how much more there was, if it was cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub text: String,
    /// "… 3,620 more characters", when what came back was longer.
    pub more: Option<String>,
}

/// The output of a call, from what its log kept.
pub fn output(out: &str, cut: bool, chars: u64, pictures: u32) -> Output {
    let text = out.trim_end().to_owned();
    let kept = out.chars().count() as u64;
    let mut more = cut.then(|| {
        let rest = chars.saturating_sub(kept);
        format!("… {} more characters", grouped(rest))
    });
    if pictures > 0 {
        let pictures = if pictures == 1 {
            "1 picture".to_owned()
        } else {
            format!("{pictures} pictures")
        };
        more = Some(match more {
            Some(m) => format!("{m} · {pictures}"),
            None => format!("and {pictures}"),
        });
    }
    Output { text, more }
}

/// A count with its thousands set apart: 3620 is "3,620".
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// How long a call took, briefly: "41ms", "1.2s", "2m 5s"; nothing for a
/// time not kept.
pub fn took(ms: u64) -> String {
    match ms {
        0 => String::new(),
        ms if ms < 1000 => format!("{ms}ms"),
        ms if ms < 10_000 => format!("{:.1}s", ms as f64 / 1000.),
        ms => super::span(i64::try_from(ms / 1000).unwrap_or(i64::MAX)),
    }
}

/// The head of a group of steps: "Worked for 12s · 3 steps", or while it
/// goes on, "Working for 12s · 3 steps".
/// What a look's calls came to, in a line: `Ran 4 commands, read 2 files,
/// searched twice`, by the verbs the rows say, in the order they first
/// came, with how long it all took after.
pub fn summary(said: &[Said], secs: i64) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for s in said {
        match counts.iter_mut().find(|(v, _)| *v == s.verb) {
            Some((_, n)) => *n += 1,
            None => counts.push((s.verb.clone(), 1)),
        }
    }
    let part = |verb: &str, n: usize| -> String {
        let noun = match verb {
            "Ran" | "Started" => Some("command"),
            "Read" | "Wrote" | "Edited" => Some("file"),
            "Opened" => Some("page"),
            "Searched" => Some("search"),
            _ => None,
        };
        match noun {
            Some("search") => match n {
                1 => "searched once".to_owned(),
                2 => "searched twice".to_owned(),
                n => format!("searched {n} times"),
            },
            Some(noun) => format!("{} {}", verb.to_lowercase(), plural(n, noun)),
            None => match n {
                1 => verb.to_lowercase(),
                n => format!("{} {n} times", verb.to_lowercase()),
            },
        }
    };
    let mut parts: Vec<String> = counts.iter().map(|(v, n)| part(v, *n)).collect();
    if let Some(first) = parts.first_mut()
        && let Some(c) = first.get_mut(0..1)
    {
        c.make_ascii_uppercase();
    }
    format!("{} · {}", parts.join(", "), super::span(secs))
}

pub fn worked(secs: i64, steps: usize, running: bool) -> String {
    let verb = if running { "Working" } else { "Worked" };
    format!(
        "{verb} for {} · {}",
        super::span(secs),
        plural(steps, "step")
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[allow(
        clippy::needless_pass_by_value,
        reason = "each case reads best as json!(…)"
    )]
    fn said(name: &str, args: Value) -> (String, String) {
        let s = say(name, &args);
        (s.verb, s.what)
    }

    #[test]
    fn her_own_tools_are_said_with_a_verb_and_the_argument_that_says_what() {
        assert_eq!(
            said("shell", json!({"command": "du -sh ~/Library"})),
            ("Ran".into(), "du -sh ~/Library".into())
        );
        assert_eq!(
            said(
                "shell",
                json!({"command": "cd /Users/me/Ghost/crates && for d in */; do wc -l $d; done"})
            ),
            ("Ran".into(), "for d in */; do wc -l $d; done".into()),
            "the cd every command of the work starts with is left out"
        );
        assert_eq!(
            said("shell", json!({"command": "cd /tmp"})),
            ("Ran".into(), "cd /tmp".into())
        );
        assert_eq!(
            said("read", json!({"path": "src/main.rs", "from": 3})),
            ("Read".into(), "src/main.rs".into())
        );
        assert_eq!(
            said("write", json!({"path": "notes.md", "content": "…"})),
            ("Wrote".into(), "notes.md".into())
        );
        assert_eq!(
            said("search", json!({"query": "heming tea house hours"})),
            (
                "Searched".into(),
                "\u{201c}heming tea house hours\u{201d}".into()
            )
        );
        assert_eq!(
            said("fetch", json!({"url": "https://www.example.com/a?b"})),
            ("Opened".into(), "example.com/a?b".into())
        );
        assert_eq!(
            said("shell", json!({"job": 2, "timeout": 30})),
            ("Checked".into(), "job 2".into())
        );
        assert_eq!(
            said("plan", json!({"steps": [{"text": "a"}, {"text": "b"}]})),
            ("Planned".into(), "2 steps".into())
        );
        assert_eq!(
            said("steer", json!({"part": "3", "text": "faster"})),
            ("Steered".into(), "part 3".into())
        );
    }

    #[test]
    fn a_tool_of_a_server_is_used_and_one_nobody_knows_is_used_by_name() {
        assert_eq!(
            said(
                "playwright__browser_navigate",
                json!({"url": "https://x.org"})
            ),
            (
                "Used playwright · browser_navigate".into(),
                "https://x.org".into()
            )
        );
        assert_eq!(
            said("summon", json!({"count": 2})),
            ("Used summon".into(), String::new())
        );
    }

    #[test]
    fn arguments_logged_as_a_string_of_json_are_read_as_well() {
        assert_eq!(
            said("shell", json!("{\"command\":\"ls -la\"}")),
            ("Ran".into(), "ls -la".into())
        );
        assert_eq!(
            said("shell", json!("not json")),
            ("Ran".into(), String::new())
        );
    }

    #[test]
    fn a_key_argument_is_one_line_and_trails_off_when_long() {
        let command = format!("ls /tmp &&\n  {}", "x".repeat(400));
        let what = say("shell", &json!({"command": command})).what;
        assert!(what.starts_with("ls /tmp && xxx"));
        assert_eq!(what.chars().count(), KEY_KEPT);
        assert!(what.ends_with('…'));
        assert_eq!(one_line("短 的\n句子", 10), "短 的 句子");
    }

    #[test]
    fn her_decisions_in_a_conversation_get_no_row_of_their_own() {
        assert!(!shown("say") && !shown("react"));
        assert!(shown("shell") && shown("note") && shown("ghost__tool"));
    }

    #[test]
    fn a_cut_output_says_how_much_more_there_was() {
        let out = output("total 8\n", true, 3628, 0);
        assert_eq!(out.text, "total 8");
        assert_eq!(out.more.as_deref(), Some("… 3,620 more characters"));
        let whole = output("ok", false, 2, 0);
        assert_eq!(whole.more, None);
        let seen = output("", false, 0, 2);
        assert_eq!(seen.more.as_deref(), Some("and 2 pictures"));
    }

    #[test]
    fn times_are_said_briefly_and_a_group_says_how_long_and_how_many() {
        assert_eq!(took(0), "");
        assert_eq!(took(41), "41ms");
        assert_eq!(took(1240), "1.2s");
        assert_eq!(took(125_000), "2m 5s");
        assert_eq!(worked(12, 3, false), "Worked for 12s · 3 steps");
        let said = |v: &str| Said {
            verb: v.into(),
            what: String::new(),
        };
        assert_eq!(
            summary(
                &[
                    said("Ran"),
                    said("Read"),
                    said("Ran"),
                    said("Searched"),
                    said("Opened")
                ],
                12
            ),
            "Ran 2 commands, read 1 file, searched once, opened 1 page · 12s"
        );
        assert_eq!(
            summary(&[said("Used playwright · navigate")], 3),
            "Used playwright · navigate · 3s"
        );
        assert_eq!(worked(75, 1, true), "Working for 1m 15s · 1 step");
    }
}
