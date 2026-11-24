//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: an edit as a diff, a file written as its
//! first lines, a search as its results, a plan as a checklist, a recall
//! as its memories, a failed command by how it ended. All of it is text
//! in and text out, so that it can be tested without a window;
//! `steps.rs` draws it.

use std::ops::Range;

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

/// Where a step of a plan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Todo,
    Doing,
    Done,
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

#[cfg(test)]
mod tests {

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
    fn only_whole_words_that_say_something_went_wrong_are_marked() {
        let text = "test a ... FAILED\nerror[E0425]: x\ntest_error_cases ok; errors: 0";
        let marked: Vec<&str> = alarms(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(marked, ["FAILED", "error"]);
        let panic = "thread 'main' panicked at";
        let marked: Vec<&str> = alarms(panic).into_iter().map(|r| &panic[r]).collect();
        assert_eq!(marked, ["panicked"]);
    }
}
