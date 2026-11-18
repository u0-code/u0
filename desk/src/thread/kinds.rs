//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: an edit as a diff, a file written as its
//! first lines, a search as its results, a plan as a checklist, a recall
//! as its memories, a failed command by how it ended. All of it is text
//! in and text out, so that it can be tested without a window;
//! `steps.rs` draws it.

use std::ops::Range;

use serde_json::Value;

/// How many lines of a file written, or of a side of an edit, are shown
/// before the rest is counted.
pub const LINES_SHOWN: usize = 12;

/// A line of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// The same before and after: context around the change.
    Same(String),
    Gone(String),
    Added(String),
    /// Lines held back, so many of them.
    More(usize),
}

/// An edit, `old` replaced by `new`, as a diff: the lines both share at
/// the head and the tail are kept as one line of context each, and what
/// lies between is what went and what came. An edit replaces one stretch,
/// so this is the whole of what changed.
pub fn diff(old: &str, new: &str) -> Vec<Line> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = Vec::new();
    if head > 0 {
        out.push(Line::Same(old[head - 1].to_owned()));
    }
    let side = |lines: &[&str], make: fn(String) -> Line, out: &mut Vec<Line>| {
        for l in lines.iter().take(LINES_SHOWN) {
            out.push(make((*l).to_owned()));
        }
        if lines.len() > LINES_SHOWN {
            out.push(Line::More(lines.len() - LINES_SHOWN));
        }
    };
    side(&old[head..old.len() - tail], Line::Gone, &mut out);
    side(&new[head..new.len() - tail], Line::Added, &mut out);
    if tail > 0 {
        out.push(Line::Same(old[old.len() - tail].to_owned()));
    }
    out
}

/// How many lines an edit took away and put in.
pub fn counted(lines: &[Line]) -> (usize, usize) {
    lines.iter().fold((0, 0), |(gone, added), l| match l {
        Line::Gone(_) => (gone + 1, added),
        Line::Added(_) => (gone, added + 1),
        Line::Same(_) | Line::More(_) => (gone, added),
    })
}

/// The diff a call of `edit` or `write` made, from its arguments: an edit's
/// old and new, or a whole file written, every line of it added.
pub fn changed(name: &str, args: &Value) -> Option<Vec<Line>> {
    let text = |key| args.get(key).and_then(Value::as_str);
    match name {
        "edit" => Some(diff(text("old")?, text("new")?)),
        "write" => Some(diff("", text("content")?)),
        _ => None,
    }
}

/// A result of a search, as the search tool lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub about: String,
}

impl Hit {
    /// The site it is on: the host, without `www.`.
    pub fn host(&self) -> &str {
        let rest = self.url.split_once("://").map_or(&*self.url, |(_, r)| r);
        let host = rest.split('/').next().unwrap_or(rest);
        host.trim_start_matches("www.")
    }
}

/// The results a search came back with, read from what the tool said:
/// `1. Title`, then the address indented (with a date in brackets after
/// it, sometimes), then what it is about. Anything else, a refusal or
/// "No results", is no results.
pub fn hits(out: &str) -> Vec<Hit> {
    let mut found: Vec<Hit> = Vec::new();
    for line in out.lines() {
        if let Some(title) = numbered(line) {
            found.push(Hit {
                title: title.to_owned(),
                url: String::new(),
                about: String::new(),
            });
        } else if let Some(hit) = found.last_mut()
            && let Some(rest) = line.strip_prefix("   ")
        {
            if hit.url.is_empty() {
                rest.split("  (")
                    .next()
                    .unwrap_or(rest)
                    .trim()
                    .clone_into(&mut hit.url);
            } else if hit.about.is_empty() {
                rest.trim().clone_into(&mut hit.about);
            }
        }
    }
    found.retain(|h| !h.url.is_empty());
    found
}

/// The title of a line `12. Title`.
fn numbered(line: &str) -> Option<&str> {
    let (n, rest) = line.split_once(". ")?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some(rest.trim())
}

/// Where a step of a plan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Todo,
    Doing,
    Done,
    Dropped,
    Failed,
}

/// A plan as a call of `plan` set it: each step's words and where it stands.
pub fn plan(args: &Value) -> Vec<(Status, String)> {
    let Some(steps) = args.get("steps").and_then(Value::as_array) else {
        return Vec::new();
    };
    steps
        .iter()
        .filter_map(|s| {
            let text = s.get("text").and_then(Value::as_str)?.trim();
            let status = match s.get("status").and_then(Value::as_str).unwrap_or("todo") {
                "doing" => Status::Doing,
                "done" => Status::Done,
                "dropped" => Status::Dropped,
                "failed" => Status::Failed,
                _ => Status::Todo,
            };
            (!text.is_empty()).then(|| (status, text.to_owned()))
        })
        .collect()
}

/// How a command that failed ended, as its shell tool wrote it at the foot
/// of what it printed: "exit 101" for `[exit 101]` or a background job
/// that `finished with exit 101`, "killed" for one killed by a signal,
/// "timed out" for one stopped for taking too long. None when the foot is
/// not in what was kept, as with an output cut to its head.
pub fn exit(out: &str) -> Option<String> {
    out.lines().rev().find_map(|line| {
        let note = line.trim().strip_prefix('[')?.strip_suffix(']')?;
        if let Some(code) = note
            .strip_prefix("exit ")
            .or_else(|| note.split_once(" finished with exit ").map(|(_, c)| c))
        {
            return code.parse::<i64>().ok().map(|c| format!("exit {c}"));
        }
        if note == "killed by a signal" {
            return Some("killed".to_owned());
        }
        note.starts_with("stopped: still running after")
            .then(|| "timed out".to_owned())
    })
}

/// The words in what a command printed that say something went wrong, as
/// byte ranges: a test runner's `FAILED`, a compiler's `error`, a
/// `panicked`. Whole words only, so that `test_error_cases` or `errors: 0`
/// stays as it is.
pub fn alarms(text: &str) -> Vec<Range<usize>> {
    const WORDS: [&str; 6] = ["FAILED", "FAIL", "ERROR", "Error", "error", "panicked"];
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (start, word(c)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                if WORDS.contains(&&text[s..i]) {
                    found.push(s..i);
                }
                start = None;
            }
            _ => {}
        }
    }
    found
}

/// The number of a line as the read tool numbers it, `   16  text`, and
/// the text after it.
fn numbered_line(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    let words = trimmed[digits..].strip_prefix("  ")?;
    Some((trimmed[..digits].parse().ok()?, words))
}

/// Which lines of a file a read brought back, as its box says it: "lines
/// 16–24". The read tool says so itself at the foot of a part of a file
/// (`[lines 16-24 of 200; …]`); a file read to its end says it by the
/// numbers of its first and last lines. When what was kept is only the
/// head of it, only where it began is known.
pub fn read_span(out: &str, cut: bool) -> Option<String> {
    let said = out.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("[lines ")?;
        let (from, rest) = rest.split_once('-')?;
        let to: String = rest.chars().take_while(char::is_ascii_digit).collect();
        Some((from.parse::<usize>().ok()?, to.parse::<usize>().ok()?))
    });
    let mut numbers = out.lines().filter_map(|l| numbered_line(l).map(|(n, _)| n));
    let first = numbers.next();
    let (from, to) = match (said, first) {
        (Some(span), _) => span,
        (None, Some(first)) if cut => return Some(format!("from line {first}")),
        (None, Some(first)) => (first, numbers.next_back().unwrap_or(first)),
        (None, None) => return None,
    };
    Some(if from == to {
        format!("line {from}")
    } else {
        format!("lines {from}\u{2013}{to}")
    })
}

/// A memory a recall brought back: her memory's number for it, and what
/// it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub n: u64,
    pub text: String,
}

/// What a recall found, read from what she was told: a line `- [#412]
/// Yesterday: what it says (here)` for each memory, a memory of more than
/// one line going on in the lines after it. "You searched your memory for
/// … and found nothing" is no memories.
pub fn memories(out: &str) -> Vec<Memory> {
    let mut found: Vec<Memory> = Vec::new();
    for line in out.lines() {
        let head = line
            .strip_prefix("- [#")
            .and_then(|rest| rest.split_once(']'))
            .and_then(|(n, text)| Some((n.parse().ok()?, text.trim())));
        if let Some((n, text)) = head {
            found.push(Memory {
                n,
                text: text.to_owned(),
            });
        } else if let Some(memory) = found.last_mut()
            && !line.trim().is_empty()
        {
            memory.text.push(' ');
            memory.text.push_str(line.trim());
        }
    }
    found
}

/// What an opened call's box says after what it was about, quietly: the
/// lines a read brought back, how many memories a recall found.
pub fn noted(name: &str, out: &str, cut: bool) -> Option<String> {
    match name {
        "read" => read_span(out, cut),
        "recall" => match memories(out).len() {
            0 => None,
            1 => Some("1 memory".to_owned()),
            n => Some(format!("{n} memories")),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_edit_shows_what_went_and_what_came_with_a_line_either_side() {
        let old = "fn a() {\n    1\n}\n";
        let new = "fn a() {\n    2\n    3\n}\n";
        assert_eq!(
            diff(old, new),
            [
                Line::Same("fn a() {".into()),
                Line::Gone("    1".into()),
                Line::Added("    2".into()),
                Line::Added("    3".into()),
                Line::Same("}".into()),
            ]
        );
        assert_eq!(counted(&diff(old, new)), (1, 2));
    }

    #[test]
    fn a_file_written_is_its_lines_added_and_the_rest_counted() {
        let content = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = changed("write", &json!({"path": "a", "content": content})).unwrap();
        assert_eq!(lines.len(), LINES_SHOWN + 1);
        assert_eq!(lines[0], Line::Added("line 0".into()));
        assert_eq!(lines[LINES_SHOWN], Line::More(8));
    }

    #[test]
    fn a_search_is_read_back_into_its_results() {
        let out = "1. Font fallback drops weight\n   https://github.com/zed/issues/1  (2026-06-01)\n   when a run falls back\n\n2. Second\n   https://www.example.com/b";
        let found = hits(out);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Font fallback drops weight");
        assert_eq!(found[0].url, "https://github.com/zed/issues/1");
        assert_eq!(found[0].about, "when a run falls back");
        assert_eq!(found[1].host(), "example.com");
        assert!(hits("No results. Try other words.").is_empty());
    }

    #[test]
    fn a_plan_is_its_steps_and_where_each_stands() {
        let args = json!({"steps": [
            {"text": "list them", "status": "done"},
            {"text": "count them", "status": "doing"},
            {"text": "", "status": "todo"},
            {"text": "write it up"}
        ]});
        assert_eq!(
            plan(&args),
            [
                (Status::Done, "list them".into()),
                (Status::Doing, "count them".into()),
                (Status::Todo, "write it up".into()),
            ]
        );
    }

    #[test]
    fn a_failed_command_says_how_it_ended_from_the_foot_of_its_output() {
        assert_eq!(exit("sub\n[exit 1]").as_deref(), Some("exit 1"));
        assert_eq!(
            exit("test result: FAILED\n[exit 101]\n[now in /tmp/desk]").as_deref(),
            Some("exit 101")
        );
        assert_eq!(
            exit("two\n[job 3 finished with exit 4]").as_deref(),
            Some("exit 4")
        );
        assert_eq!(exit("[killed by a signal]").as_deref(), Some("killed"));
        assert_eq!(
            exit("x\n[stopped: still running after 60s. Raise `timeout`.]").as_deref(),
            Some("timed out")
        );
        assert_eq!(exit("the head of a long output, cut"), None);
    }

    #[test]
    fn only_whole_words_that_say_something_went_wrong_are_marked() {
        let text = "test a ... FAILED\nerror[E0425]: x\ntest_error_cases ok; errors: 0";
        let marked: Vec<&str> = alarms(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(marked, ["FAILED", "error"]);
        let panic = "thread 'main' panicked at";
        let marked: Vec<&str> = alarms(panic).into_iter().map(|r| &panic[r]).collect();
        assert_eq!(marked, ["panicked"]);
    }

    #[test]
    fn a_read_says_which_lines_it_brought_back() {
        let part = "   16  /// Body text\n   17  pub const BODY: f32 = 15.;\n[lines 16-24 of 84; pass `from` to read on]";
        assert_eq!(
            read_span(part, false).as_deref(),
            Some("lines 16\u{2013}24")
        );
        let whole = "    1  a\n    2  b\n    3  c";
        assert_eq!(read_span(whole, false).as_deref(), Some("lines 1\u{2013}3"));
        assert_eq!(read_span("    7  only", false).as_deref(), Some("line 7"));
        assert_eq!(
            read_span("   40  the head of a long read", true).as_deref(),
            Some("from line 40")
        );
        assert_eq!(read_span("(the file is empty)", false), None);
    }

    #[test]
    fn a_recall_is_read_back_into_its_memories_by_number() {
        let out = "- [#412] Yesterday: 群里那张图是阿鹿做的 (here)\n- [#388] Last Tuesday: 她存了三张表情\n放在 media/";
        assert_eq!(
            memories(out),
            [
                Memory {
                    n: 412,
                    text: "Yesterday: 群里那张图是阿鹿做的 (here)".into()
                },
                Memory {
                    n: 388,
                    text: "Last Tuesday: 她存了三张表情 放在 media/".into()
                },
            ]
        );
        assert_eq!(noted("recall", out, false).as_deref(), Some("2 memories"));
        let none = "You searched your memory for \"memes\" and found nothing.";
        assert!(memories(none).is_empty());
        assert_eq!(noted("recall", none, false), None);
    }
}
