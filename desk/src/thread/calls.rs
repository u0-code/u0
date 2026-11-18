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

/// How long a path in a row may be before its head gives way: the end of
/// a path is what says which file it is.
const PATH_KEPT: usize = 48;

/// A path as a row shows it: whole when it is short, else as many of its
/// last parts as fit after "…/", and at least its file name.
fn path_tail(path: &str, most: usize) -> String {
    let path = path.trim();
    if path.chars().count() <= most {
        return path.to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut len = 2;
    for part in path.rsplit('/').filter(|p| !p.is_empty()) {
        let more = part.chars().count() + 1;
        if !kept.is_empty() && len + more > most {
            break;
        }
        kept.push(part);
        len += more;
    }
    kept.reverse();
    format!("…/{}", kept.join("/"))
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
        "read" => ("Read", path_tail(a("path"), PATH_KEPT)),
        "write" => ("Wrote", path_tail(a("path"), PATH_KEPT)),
        "edit" => ("Edited", path_tail(a("path"), PATH_KEPT)),
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

/// The whole command a call ran, when it ran one: what its opened row
/// shows in a well.
pub fn command_of(name: &str, args: &Value) -> Option<String> {
    let args = object(args);
    (name == "shell")
        .then(|| arg(&args, "command").trim().to_owned())
        .filter(|c| !c.is_empty())
}

/// What a stretch of a command is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// The program at the head of a command.
    Program,
    /// What joins commands: `&&`, `||`, `|`, `;`, a line break.
    Joint,
    Comment,
    Space,
    /// Everything else: arguments, flags, quoted strings, redirections.
    Argument,
}

/// A command cut into stretches, each with what it is, their lengths in
/// bytes covering the whole of it in order. Quotes are honoured, so that
/// a `|` inside a string is part of the string.
pub fn shell_words(command: &str) -> Vec<(usize, Word)> {
    let mut out: Vec<(usize, Word)> = Vec::new();
    let mut push = |len: usize, word: Word| match out.last_mut() {
        Some((l, w)) if *w == word && word != Word::Program && word != Word::Argument => {
            *l += len;
        }
        _ => out.push((len, word)),
    };
    let bytes = command.as_bytes();
    let mut i = 0;
    // The next word is a program: at the start, and after a joint.
    let mut head = true;
    while i < bytes.len() {
        let rest = &command[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c.is_whitespace() && c != '\n' {
            push(c.len_utf8(), Word::Space);
            i += c.len_utf8();
            continue;
        }
        let joint = ["&&", "||", "|", ";", "\n", "$(", "("]
            .iter()
            .find(|j| rest.starts_with(**j));
        if let Some(j) = joint {
            push(j.len(), Word::Joint);
            i += j.len();
            head = true;
            continue;
        }
        if c == '#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            let end = rest.find('\n').unwrap_or(rest.len());
            push(end, Word::Comment);
            i += end;
            continue;
        }
        // A word: up to the next space or joint outside quotes.
        let mut j = 0;
        let mut quote: Option<char> = None;
        for (k, ch) in rest.char_indices() {
            match quote {
                Some(q) if ch == q => quote = None,
                Some(_) => {}
                None if ch == '\'' || ch == '"' => quote = Some(ch),
                None if ch.is_whitespace() || matches!(ch, '|' | ';' | '&' | ')') => {
                    j = k;
                    break;
                }
                None => {}
            }
            j = k + ch.len_utf8();
        }
        let j = j.max(c.len_utf8());
        let word = &rest[..j];
        // An assignment before a program does not take its place.
        let program = head && !word.contains('=');
        push(
            j,
            if program {
                Word::Program
            } else {
                Word::Argument
            },
        );
        if program {
            head = false;
        }
        i += j;
    }
    out
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

pub(super) fn plural(n: usize, word: &str) -> String {
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

/// `text` on one line in at most `cols` columns, a Chinese character
/// taking two, ending in "…" when it is cut. GPUI cuts a line that is too
/// long at the edge without an ellipsis (it measures before it knows the
/// width), so a line that must end in one is cut here first.
pub fn fit(text: &str, cols: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let wide = |c: char| if c.len_utf8() >= 3 { 2 } else { 1 };
    if flat.chars().map(wide).sum::<usize>() <= cols {
        return flat;
    }
    let mut out = String::new();
    let mut used = 1;
    for c in flat.chars() {
        if used + wide(c) > cols {
            break;
        }
        used += wide(c);
        out.push(c);
    }
    out.push('…');
    out
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

/// How long a call took, as a step row says it: "0.1s", "2.1s", "34s",
/// "1m 08s"; nothing for a time not kept.
pub fn took(ms: u64) -> String {
    match ms {
        0 => String::new(),
        ms if ms < 10_000 => format!("{:.1}s", ms as f64 / 1000.),
        ms if ms < 60_000 => format!("{}s", ms / 1000),
        ms => {
            let secs = ms / 1000;
            format!("{}m {:02}s", secs / 60, secs % 60)
        }
    }
}

/// What a look's calls came to, in a line: `Ran 4 commands, read 2 files
/// · 20s`. Only the two weightiest kinds are named, edits first, then
/// commands, reads, searches, pages, memories, the rest; what is left
/// out is in the time. A call that failed is counted after.
pub fn summary(said: &[Said], failed: usize, secs: i64) -> String {
    format!("{} · {}", tally(said, failed), super::span(secs))
}

/// The weight of a verb in a summary: what changed things first.
fn rank(verb: &str) -> usize {
    [
        "Edited", "Wrote", "Ran", "Started", "Read", "Searched", "Opened", "Recalled",
    ]
    .iter()
    .position(|v| *v == verb)
    .unwrap_or(8)
}

/// A summary without its time: `Ran 4 commands, read 2 files · 1 failed`.
pub fn tally(said: &[Said], failed: usize) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for s in said {
        match counts.iter_mut().find(|(v, _)| *v == s.verb) {
            Some((_, n)) => *n += 1,
            None => counts.push((s.verb.clone(), 1)),
        }
    }
    counts.sort_by_key(|(v, _)| rank(v));
    let part = |verb: &str, n: usize| -> String {
        let noun = match verb {
            "Ran" | "Started" => Some("command"),
            "Read" | "Wrote" | "Edited" => Some("file"),
            "Opened" => Some("page"),
            "Recalled" => Some("memory"),
            "Searched" => Some("search"),
            _ => None,
        };
        match noun {
            Some("search") => match n {
                1 => "searched once".to_owned(),
                2 => "searched twice".to_owned(),
                n => format!("searched {n} times"),
            },
            Some("memory") => format!(
                "recalled {}",
                if n == 1 {
                    "1 memory".to_owned()
                } else {
                    format!("{n} memories")
                }
            ),
            Some(noun) => format!("{} {}", verb.to_lowercase(), plural(n, noun)),
            None => match n {
                1 => verb.to_lowercase(),
                n => format!("{} {n} times", verb.to_lowercase()),
            },
        }
    };
    let mut parts: Vec<String> = counts.iter().take(2).map(|(v, n)| part(v, *n)).collect();
    if let Some(first) = parts.first_mut()
        && let Some(c) = first.get_mut(0..1)
    {
        c.make_ascii_uppercase();
    }
    let line = parts.join(", ");
    if failed > 0 {
        format!("{line} · {failed} failed")
    } else {
        line
    }
}

/// The head of a group of steps while it goes on: "Working for 12s · 3
/// steps".
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

    #[test]
    fn a_line_too_long_is_cut_with_an_ellipsis_counting_chinese_as_two() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("abcdefghijkl", 6), "abcde…");
        assert_eq!(fit("一二三四五六", 7), "一二三…");
        assert_eq!(fit("一二三", 6), "一二三");
    }

    #[test]
    fn a_long_path_keeps_its_end_and_a_short_one_stays_whole() {
        assert_eq!(path_tail("src/main.rs", 48), "src/main.rs");
        let long =
            "/private/tmp/claude-501/-Users-0xhacker-Desktop-work-Ghost/e7/ghost/home/hello.py";
        assert_eq!(path_tail(long, 48), "…/e7/ghost/home/hello.py");
        assert!(path_tail(long, 48).chars().count() <= 48);
        assert_eq!(
            path_tail(&format!("/{}", "x".repeat(60)), 48),
            format!("…/{}", "x".repeat(60))
        );
    }

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
    fn a_command_is_coloured_by_what_each_word_is_and_every_byte_is_covered() {
        let command = "cd desk && cargo fmt --check | grep -E 'a|b' ; X=1 git log # done";
        let words = shell_words(command);
        assert_eq!(words.iter().map(|(l, _)| l).sum::<usize>(), command.len());
        let mut at = 0;
        let mut programs = Vec::new();
        for (len, word) in &words {
            if *word == Word::Program {
                programs.push(&command[at..at + len]);
            }
            at += len;
        }
        assert_eq!(programs, ["cd", "cargo", "grep", "git"]);
        assert_eq!(words.last().map(|w| w.1), Some(Word::Comment));
        assert_eq!(
            command_of("shell", &json!({"command": " ls "})),
            Some("ls".into())
        );
        assert_eq!(command_of("read", &json!({"path": "a"})), None);
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
        assert_eq!(took(41), "0.0s");
        assert_eq!(took(1240), "1.2s");
        assert_eq!(took(34_400), "34s");
        assert_eq!(took(68_000), "1m 08s");
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
                    said("Edited")
                ],
                0,
                12
            ),
            "Edited 1 file, ran 2 commands · 12s"
        );
        assert_eq!(
            summary(&[said("Used playwright · navigate")], 1, 3),
            "Used playwright · navigate · 1 failed · 3s"
        );
        assert_eq!(worked(75, 1, true), "Working for 1m 15s · 1 step");
    }
}
