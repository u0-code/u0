//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: an edit as a diff (`git::edit_diff`) cut to
//! what the thread shows, a file written as its first lines, a search as
//! its results, a plan as a checklist, a recall as its memories, a failed
//! command by how it ended. All of it is text in and text out, so that it
//! can be tested without a window; `thread.rs` and `drawn.rs` draw it.

use std::ops::Range;

use crate::git::FileDiff;

/// How many lines of an edit the thread shows before the rest is counted.
pub const LINES_SHOWN: usize = 24;

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
    use agent_client_protocol::v1::schema::Diff;

    use super::*;
    use crate::git;

    fn edit(old: Option<&str>, new: &str) -> Diff {
        Diff::new("a.rs", new).old_text(old.map(str::to_owned))
    }

    #[test]
    fn a_long_write_is_counted_whole_though_the_thread_shows_its_head() {
        let new = (0..300)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat();
        let file = git::edit_diff(&edit(None, &new), None);
        assert_eq!((file.gone, file.added), (0, 300));
        let (shown, more) = cut(file, LINES_SHOWN);
        assert_eq!(shown.hunks[0].lines.len(), LINES_SHOWN);
        assert_eq!(more, 300 - LINES_SHOWN);
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
