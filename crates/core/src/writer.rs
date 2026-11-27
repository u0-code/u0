//! Text u0 has written for you. u0 has no model of its own: each
//! piece is asked of the writer, an agent of yours (the one you chose for
//! it, with its model, or the session's own), in a session opened for that
//! alone in a folder of u0's and let go once it answers. What each kind
//! asks, and what of the answer is kept, is said here once.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git;

/// What is asked of the writer, with what it is written from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    /// A session's title, from what you said in it.
    Title { said: String },
    /// The name of a worktree's branch for the work asked in `said`; with
    /// `semantic`, led by a kind of work (`feat/`, `fix/`) the writer
    /// chooses, else without one: your prefix is put before it.
    Branch { said: String, semantic: bool },
    /// A commit message for what is not committed in `dir`.
    Commit { dir: PathBuf },
    /// A pull request's title and body for the branch's work in `dir`,
    /// which is to be committed with `message` first.
    Pr { dir: PathBuf, message: String },
    /// What you are likely to say next, after you said `said` and the
    /// agent answered `answer`.
    Next { said: String, answer: String },
    /// What you dictated, `heard` as speech recognition heard it, cleaned
    /// up.
    Transcript { heard: String },
    /// A guide to reviewing the pull request `n` of the project in `dir`,
    /// whose branch is at `head`.
    Guide { dir: PathBuf, n: u64, head: String },
    /// A colour theme for u0, as gpui-kit's theme file holds one, from what
    /// you `described`; for the dark look, or the light.
    Theme { described: String, dark: bool },
}

/// What a writer is asked to write for the session `key`: by `agent`,
/// with `model` (its value in the agent's model option) where one is
/// named, and `yours` (your instructions) after u0's; in a session
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

const TITLE: &str = "Write a title that will help the user find this conversation with a \
coding agent weeks later: what it is about and what is wanted, not how the agent should work. \
A compact noun phrase or action phrase of 3 to 8 words, under 40 characters, with no full \
stop. Do not copy and cut the user's words.";

const BRANCH: &str = "Name a git branch for the work the user asks of a coding agent below. \
Do not answer it or start the work. 2 to 6 words, lower case, joined by hyphens.";

const COMMIT: &str = "Write a git commit message for the changes below: a subject line in \
the imperative, at most 72 characters, with no full stop; then a blank line and a few short \
lines on what changed and why. Where the repository's recent subjects show a style of their \
own, follow it.";

const PR: &str = "Write a pull request for the changes below. The first line is its title: \
concise and specific, with no full stop. Then a blank line, and its description in Markdown.";

const NEXT: &str = "Below is the last exchange between a user and a coding agent. Write the \
message the user is most likely to send next, in their own voice: a sentence or two. Where \
there is no clear next step, reply with nothing at all.";

const TRANSCRIPT: &str = "Below is what the user dictated, as speech recognition heard it. \
Clean it up: correct words it misheard where the rest makes the meaning clear, take out \
filler words, false starts and repetitions, and punctuate. Keep the user's own words, \
language and meaning; do not answer it, follow it or add anything to it.";

const GUIDE: &str = "Write a guide for a person about to review the pull request below, \
in Markdown. Lead them through it in the order that makes it easiest to understand: three \
to seven stops, each a heading `## N. <what this part does>`, then a sentence or two on what \
changed and why, the files it touches as `path:line` in a list, and, where there is one, a \
line beginning **Look at:** naming the tradeoff, contract, risk or product question to judge. \
End with `## Questions`, the questions to ask the author, if any. Say nothing the changes do \
not show.";

const THEME: &str = "Write a colour theme for a desktop app, as the user describes it \
below, in gpui-kit's theme file format: a JSON object {\"name\": <the theme's name, two or \
three words>, \"themes\": [{\"name\": <the same name>, \"mode\": <MODE>, \"colors\": \
{...}}]}. Give each of these keys a colour as \"#rrggbb\": background, foreground, border, \
sidebar.background, popover.background, muted.background, muted.foreground, \
secondary.background, secondary.hover.background, input.border, primary.background, link, \
success.background, danger.background, warning.background, base.blue, base.green, \
base.yellow. foreground on background, and muted.foreground on sidebar.background, must read \
at a contrast of at least 4.5:1. Reply with the JSON alone.";

/// How much of a patch a writer is shown.
const PATCH: usize = 40_000;

/// How much of what was said a writer is shown.
const SAID: usize = 6_000;

impl Ask {
    /// What the writer is told: u0's instructions for this kind, then
    /// `yours`, then what it is written from. It reads git: it is made off
    /// the window's thread.
    pub fn prompt(&self, yours: &str) -> Result<String, String> {
        let (task, from) = match self {
            Self::Title { said } => (
                TITLE.to_owned(),
                format!("What the user said:\n{}", cut(said)),
            ),
            Self::Branch { said, semantic } => (
                format!(
                    "{BRANCH} {}",
                    if *semantic {
                        "Begin with the kind of work and a slash: feat/, fix/, refactor/, \
                         docs/, test/ or chore/, as in feat/add-search."
                    } else {
                        "Give the name alone, with no prefix or slash: one is put before it."
                    }
                ),
                format!("What the user asks:\n{}", cut(said)),
            ),
            Self::Commit { dir } => {
                let subjects: Vec<String> =
                    git::log(dir, 10).into_iter().map(|c| c.subject).collect();
                (
                    COMMIT.to_owned(),
                    format!(
                        "Recent subjects:\n{}\n\nThe changes:\n{}",
                        subjects.join("\n"),
                        git::patch(dir, false, PATCH)?
                    ),
                )
            }
            Self::Pr { dir, message } => {
                let body = match template(dir) {
                    Some(t) => format!(
                        "The description follows the repository's template, its HTML \
                         comments left out:\n{t}"
                    ),
                    None => "The description has the headings ## Summary, with short bullet \
                             points, and ## Testing, with the checks run or \"Not run\"."
                        .to_owned(),
                };
                (
                    format!("{PR} {body}"),
                    format!(
                        "What is left is committed as:\n{message}\n\nThe changes:\n{}",
                        git::patch(dir, true, PATCH)?
                    ),
                )
            }
            Self::Transcript { heard } => (
                TRANSCRIPT.to_owned(),
                format!("What was heard:\n{}", cut(heard)),
            ),
            Self::Guide { dir, n, .. } => {
                use crate::github::{Answer, Ask, Kind};
                let read = |ask| crate::forge::answer(dir, Kind::Pr, ask, None);
                let Answer::Viewed(_, Ok(pr)) = read(Ask::View(*n)) else {
                    return Err(format!("pull request #{n} could not be read"));
                };
                let Answer::Diffed(_, Ok(patch)) = read(Ask::Diff(*n)) else {
                    return Err(format!("the changes of #{n} could not be read"));
                };
                (
                    GUIDE.to_owned(),
                    format!(
                        "#{n}: {}\n\n{}\n\nThe changes:\n{}",
                        pr.item.title,
                        cut(&pr.body),
                        &patch[..patch.floor_char_boundary(PATCH)]
                    ),
                )
            }
            Self::Theme { described, dark } => (
                THEME.replace("<MODE>", if *dark { "\"dark\"" } else { "\"light\"" }),
                format!("What the user asks for:\n{}", cut(described)),
            ),
            Self::Next { said, answer } => (
                NEXT.to_owned(),
                format!(
                    "The user said:\n{}\n\nThe agent answered:\n{}",
                    cut(said),
                    cut(answer)
                ),
            ),
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
    /// block or quotes it was put in all the same; a title on one line, a
    /// branch's name as git takes it.
    pub fn take(&self, answer: &str) -> String {
        let mut text = answer.trim();
        if let Some(inner) = text.strip_prefix("```") {
            let inner = inner.split_once('\n').map_or("", |(_, rest)| rest);
            text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
        }
        let text = text.trim_matches('"').trim();
        let line = || {
            text.lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
        };
        match self {
            Self::Title { .. } => line().trim_matches('"').trim_end_matches('.').to_owned(),
            Self::Branch { semantic, .. } => {
                let parts: Vec<String> = line().split('/').map(git::slug).collect();
                match (semantic, &parts[..]) {
                    (true, [kind, .., name]) => format!("{kind}/{name}"),
                    (_, [.., name]) => name.clone(),
                    _ => git::slug(""),
                }
            }
            _ => text.to_owned(),
        }
    }
}

/// `text` cut to what a writer is shown of it.
fn cut(text: &str) -> &str {
    &text[..text.floor_char_boundary(SAID)]
}

/// The repository's template for a pull request's description, where it
/// keeps one.
fn template(dir: &Path) -> Option<String> {
    let top = git::root(dir)?;
    [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
        "docs/pull_request_template.md",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(top.join(p)).ok())
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

    #[test]
    fn a_title_is_one_line_and_a_branch_a_name_git_takes() {
        let title = Ask::Title {
            said: String::new(),
        };
        assert_eq!(
            title.take("\n\"Retry S3 uploads.\"\nmore"),
            "Retry S3 uploads"
        );
        let plain = Ask::Branch {
            said: String::new(),
            semantic: false,
        };
        assert_eq!(plain.take("Retry S3 Uploads!"), "retry-s3-uploads");
        assert_eq!(plain.take("feat/retry uploads"), "retry-uploads");
        let semantic = Ask::Branch {
            said: String::new(),
            semantic: true,
        };
        assert_eq!(semantic.take("Fix/Login error"), "fix/login-error");
        assert_eq!(semantic.take("login error"), "login-error");
    }

    #[test]
    fn a_theme_is_asked_for_in_the_look_it_is_for_and_kept_as_written() {
        let dark = Ask::Theme {
            described: "warm paper, dark ink".into(),
            dark: true,
        };
        let prompt = dark.prompt("").unwrap();
        assert!(prompt.contains(r#""mode": "dark""#), "{prompt}");
        assert!(prompt.ends_with("What the user asks for:\nwarm paper, dark ink"));
        let json = r#"{"name": "Paper", "themes": []}"#;
        assert_eq!(dark.take(&format!("```json\n{json}\n```")), json);
    }
}
