//! What some calls hold, read so that an opened call can be drawn as what
//! it is rather than as text: a file written as its first lines, a search as
//! its results, a plan as a checklist, a recall as its memories, a failed
//! command by how it ended. All of it is text in and text out, so that it
//! can be tested without a window; `thread.rs` and `drawn.rs` draw it.

use std::ops::Range;

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
    fn only_whole_words_that_say_something_went_wrong_are_marked() {
        let text = "test a ... FAILED\nerror[E0425]: x\ntest_error_cases ok; errors: 0";
        let marked: Vec<&str> = alarms(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(marked, ["FAILED", "error"]);
        let panic = "thread 'main' panicked at";
        let marked: Vec<&str> = alarms(panic).into_iter().map(|r| &panic[r]).collect();
        assert_eq!(marked, ["panicked"]);
    }
}
