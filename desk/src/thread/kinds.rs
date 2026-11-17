//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: an edit as a diff, a file written as its
//! first lines, a search as its results, a plan as a checklist. All of it
//! is text in and text out, so that it can be tested without a window;
//! `steps.rs` draws it.

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
}
