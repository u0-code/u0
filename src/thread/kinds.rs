//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: an edit as a diff, a file written as its
//! first lines, a search as its results, a plan as a checklist, a recall
//! as its memories, a failed command by how it ended. All of it is text
//! in and text out, so that it can be tested without a window;
//! `thread.rs` and `drawn.rs` draw it.

use std::ops::Range;

use agent_client_protocol::v1::schema::Diff;
use similar::{ChangeTag, TextDiff};

use crate::git::{self, FileDiff, Hunk, Line};

/// How many lines of an edit the thread shows before the rest is counted.
pub const LINES_SHOWN: usize = 24;

/// An edit as git would show it: each stretch that changed, with a line of
/// context either side, numbered from `start`, the line of the file its
/// text begins on (one where that is not known).
pub fn diff(d: &Diff, start: Option<usize>) -> FileDiff {
    let old = d.old_text.as_deref().unwrap_or("");
    let at = start.unwrap_or(1);
    let text = TextDiff::from_lines(old, &d.new_text);
    let (mut added, mut gone) = (0, 0);
    let hunks = text
        .grouped_ops(1)
        .iter()
        .map(|group| {
            let (first, last) = (&group[0], &group[group.len() - 1]);
            let (o, n) = (first.old_range().start, first.new_range().start);
            let (o_len, n_len) = (last.old_range().end - o, last.new_range().end - n);
            let lines = group
                .iter()
                .flat_map(|op| text.iter_changes(op))
                .map(|c| {
                    let text = c.value().trim_end_matches(['\r', '\n']).to_owned();
                    let (old, new) = (c.old_index().map(|i| i + at), c.new_index().map(|i| i + at));
                    match (c.tag(), old, new) {
                        (ChangeTag::Delete, Some(old), _) => {
                            gone += 1;
                            Line::Gone { old, text }
                        }
                        (ChangeTag::Insert, _, Some(new)) => {
                            added += 1;
                            Line::Added { new, text }
                        }
                        (_, old, new) => Line::Same {
                            old: old.unwrap_or_default(),
                            new: new.unwrap_or_default(),
                            text,
                        },
                    }
                })
                .collect();
            Hunk {
                head: format!("@@ -{},{o_len} +{},{n_len} @@", o + at, n + at),
                lines,
            }
        })
        .collect();
    FileDiff {
        path: d.path.display().to_string(),
        from: None,
        status: if d.old_text.is_none() {
            git::Status::Added
        } else {
            git::Status::Modified
        },
        added,
        gone,
        hunks,
        binary: false,
    }
}

/// The first `n` lines of a diff's hunks, and how many were left out.
pub fn cut(mut file: FileDiff, n: usize) -> (FileDiff, usize) {
    let (mut left, mut out) = (n, 0);
    for h in &mut file.hunks {
        let keep = h.lines.len().min(left);
        out += h.lines.len() - keep;
        h.lines.truncate(keep);
        left -= keep;
    }
    file.hunks.retain(|h| !h.lines.is_empty());
    (file, out)
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

    fn edit(old: Option<&str>, new: &str) -> Diff {
        Diff::new("a.rs", new).old_text(old.map(str::to_owned))
    }

    #[test]
    fn an_edit_shows_what_went_and_what_came_with_a_line_either_side() {
        let d = edit(Some("fn a() {\n    1\n}\n"), "fn a() {\n    2\n    3\n}\n");
        let file = diff(&d, Some(10));
        assert_eq!(
            file.hunks[0].lines,
            [
                Line::Same {
                    old: 10,
                    new: 10,
                    text: "fn a() {".into()
                },
                Line::Gone {
                    old: 11,
                    text: "    1".into()
                },
                Line::Added {
                    new: 11,
                    text: "    2".into()
                },
                Line::Added {
                    new: 12,
                    text: "    3".into()
                },
                Line::Same {
                    old: 12,
                    new: 13,
                    text: "}".into()
                },
            ]
        );
        assert_eq!((file.gone, file.added), (1, 2));
    }

    #[test]
    fn a_long_write_is_counted_whole_though_the_thread_shows_its_head() {
        let new = (0..300)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat();
        let file = diff(&edit(None, &new), None);
        assert_eq!((file.gone, file.added), (0, 300));
        let (shown, more) = cut(file, LINES_SHOWN);
        assert_eq!(shown.hunks[0].lines.len(), LINES_SHOWN);
        assert_eq!(more, 300 - LINES_SHOWN);
    }

    #[test]
    fn two_changes_apart_are_two_hunks() {
        let old = (0..20)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat();
        let new = old
            .replace("line 2\n", "two\n")
            .replace("line 17\n", "seventeen\n");
        let file = diff(&edit(Some(&old), &new), Some(1));
        assert_eq!(file.hunks.len(), 2);
        assert_eq!((file.gone, file.added), (2, 2));
        assert_eq!(file.hunks[1].head, "@@ -17,3 +17,3 @@");
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
