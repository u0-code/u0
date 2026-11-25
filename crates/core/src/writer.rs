//! Text Ghost has written for you. Ghost has no model of its own: each
//! piece is asked of the writer, an agent of yours (the one you chose for
//! it, with its model, or the session's own), in a session opened for that
//! alone in a folder of Ghost's and let go once it answers. What each kind
//! asks, and what of the answer is kept, is said here once.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git;

/// What is asked of the writer, with what it is written from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    /// A commit message for what is not committed in `dir`.
    Commit { dir: PathBuf },
}

/// What a writer is asked to write for the session `key`: by `agent`,
/// with `model` (its value in the agent's model option) where one is
/// named, and `yours` (your instructions) after Ghost's; in a session
/// opened in `dir` for that alone, deleted (or closed) once it answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub agent: String,
    pub model: Option<String>,
    pub key: u64,
    pub ask: Ask,
    pub yours: String,
    pub dir: PathBuf,
    pub delete: bool,
    pub close: bool,
}

/// What every writer is told first.
const RULES: &str = "You write one piece of text for an app. It is a writing task alone: \
do not use tools, read files or run commands; all you need is below. Reply with the text \
only: no preamble, no quotes around it, no code block.";

const COMMIT: &str = "Write a git commit message for the changes below: a subject line in \
the imperative, at most 72 characters, with no full stop; then a blank line and a few short \
lines on what changed and why. Where the repository's recent subjects show a style of their \
own, follow it.";

/// How much of a patch a writer is shown.
const PATCH: usize = 40_000;

impl Ask {
    /// What the writer is told: Ghost's instructions for this kind, then
    /// `yours`, then what it is written from. It reads git: it is made off
    /// the window's thread.
    pub fn prompt(&self, yours: &str) -> Result<String, String> {
        let (task, from) = match self {
            Self::Commit { dir } => {
                let subjects: Vec<String> = git::log(dir, 10).into_iter().map(|c| c.1).collect();
                let patch = git::patch(dir, false, PATCH)?;
                (
                    COMMIT,
                    format!(
                        "Recent subjects:\n{}\n\nThe changes:\n{patch}",
                        subjects.join("\n")
                    ),
                )
            }
        };
        let mut prompt = format!("{RULES}\n\n{task}");
        if !yours.trim().is_empty() {
            prompt.push_str("\n\nYour instructions, which win over the above:\n");
            prompt.push_str(yours.trim());
        }
        prompt.push_str("\n\n");
        prompt.push_str(&from);
        Ok(prompt)
    }

    /// What of the writer's `answer` is kept: the text, without a code
    /// block or quotes it was put in all the same.
    pub fn take(&self, answer: &str) -> String {
        let mut text = answer.trim();
        if let Some(inner) = text.strip_prefix("```") {
            let inner = inner.split_once('\n').map_or("", |(_, rest)| rest);
            text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
        }
        text.trim_matches('"').trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_put_in_a_code_block_or_quotes_is_kept_without_them() {
        let ask = Ask::Commit { dir: "/".into() };
        assert_eq!(ask.take("```text\nAdd it\n\nWhy.\n```\n"), "Add it\n\nWhy.");
        assert_eq!(ask.take("  \"Add it\" "), "Add it");
        assert_eq!(ask.take("Add it"), "Add it");
    }
}
