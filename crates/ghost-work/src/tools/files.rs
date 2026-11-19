//! `read`, `write`, `edit`: files, without shell quoting in the way.

use std::{fmt::Write as _, sync::Arc, time::Duration};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use super::{Desk, MAX_PICTURE};
use crate::{
    machine::quote,
    prompts::{self, Loop, Store, TEXTS},
    tool::{Outcome, Tool, text},
};

const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// The most of a file that is fetched for reading.
const FETCH_BYTES: usize = 400_000;
const DEFAULT_LINES: usize = 600;
const MAX_CHARS: usize = 40_000;

/// What a directory read as a file exits with.
const A_DIRECTORY: i32 = 21;

/// The whole file as text, or why it cannot be had, in `words`.
async fn slurp(
    desk: &Desk,
    words: &Store,
    path: &str,
    limit: Option<usize>,
) -> Result<String, Outcome> {
    let q = quote(path);
    let fetch = limit.map_or_else(|| format!("cat -- {q}"), |n| format!("head -c {n} -- {q}"));
    // `cat` reports a missing file in its own words.
    let script = format!("if [ -d {q} ]; then exit {A_DIRECTORY}; fi\n{fetch}");
    let said = |key: &str, why: Option<String>| {
        let mut pairs = vec![("path", path.to_owned())];
        pairs.extend(why.map(|why| ("why", why)));
        Outcome::fail(words.fill(TEXTS, key, &pairs))
    };
    let out = desk
        .exec(&script, None, IO_TIMEOUT)
        .await
        .map_err(|e| said("unreadable", Some(e.to_string())))?;
    if out.code == Some(A_DIRECTORY) {
        return Err(said("a_directory", None));
    }
    if !out.ok() {
        return Err(Outcome::fail(out.text.trim().to_owned()));
    }
    if out.text.contains('\0') {
        return Err(said("a_binary_file", None));
    }
    if out.dropped > 0 && limit.is_none() {
        return Err(said("too_large_whole", None));
    }
    Ok(out.text)
}

async fn store(desk: &Desk, words: &Store, path: &str, content: &str) -> Result<(), Outcome> {
    if let Some(why) = desk.fenced(words, path) {
        return Err(Outcome::fail(why));
    }
    let q = quote(path);
    let script = format!("mkdir -p -- \"$(dirname -- {q})\" && cat > {q}");
    let unwritten = |why: String| {
        Outcome::fail(words.fill(
            TEXTS,
            "unwritable",
            &[("path", path.to_owned()), ("why", why)],
        ))
    };
    let out = desk
        .exec(&script, Some(content.as_bytes().to_vec()), IO_TIMEOUT)
        .await
        .map_err(|e| unwritten(e.to_string()))?;
    if out.ok() {
        Ok(())
    } else {
        Err(unwritten(out.text.trim().to_owned()))
    }
}

pub struct Read {
    desk: Arc<Desk>,
}

impl Read {
    pub fn new(desk: Arc<Desk>) -> Self {
        Self { desk }
    }

    /// The file as a picture, if its name says it is an image and its
    /// bytes agree. `None` sends it on to be read as text.
    async fn look(&self, words: &Store, path: &str) -> Option<Outcome> {
        let named = path.rsplit_once('.').is_some_and(|(_, ext)| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        });
        if !named {
            return None;
        }
        // The machine reads from its own root; the worker may have moved.
        let full = match self.desk.cwd() {
            Some(dir) if !path.starts_with('/') => format!("{}/{path}", dir.trim_end_matches('/')),
            _ => path.to_owned(),
        };
        let bytes = match self.desk.machine.read(&full, MAX_PICTURE as u64).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Some(Outcome::fail(words.fill(
                    TEXTS,
                    "image_too_large",
                    &[
                        ("path", path.to_owned()),
                        ("mb", (MAX_PICTURE / (1024 * 1024)).to_string()),
                    ],
                )));
            }
            // Machines that cannot hand files over, and missing files, are
            // left to the text reader, which says so in its own words.
            Err(_) => return None,
        };
        let picture = super::picture(bytes)?;
        Some(Outcome::seen(
            words.fill(
                TEXTS,
                "an_image",
                &[("path", path.to_owned()), ("about", super::about(&picture))],
            ),
            picture,
        ))
    }
}

#[async_trait]
impl Tool for Read {
    fn name(&self) -> &'static str {
        "read"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "read",
            json!({
                "path": {"type": "string"},
                "from": {"type": "integer"},
                "lines": {"type": "integer"},
            }),
            &["path"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let path = match text(words, args, "path") {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        if let Some(seen) = self.look(words, path).await {
            return seen;
        }
        let content = match slurp(&self.desk, words, path, Some(FETCH_BYTES)).await {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };
        if content.is_empty() {
            return Outcome::ok(words.text(TEXTS, "file_empty"));
        }
        let from = usize::try_from(args["from"].as_u64().unwrap_or(1))
            .unwrap_or(1)
            .max(1);
        let want = usize::try_from(args["lines"].as_u64().unwrap_or(DEFAULT_LINES as u64))
            .unwrap_or(DEFAULT_LINES)
            .max(1);
        let total = content.lines().count();
        if from > total {
            return Outcome::fail(words.fill(
                TEXTS,
                "too_few_lines",
                &[("path", path.to_owned()), ("lines", total.to_string())],
            ));
        }
        let mut out = String::new();
        let mut last = from - 1;
        for (i, line) in content.lines().enumerate().skip(from - 1).take(want) {
            if out.len() + line.len() > MAX_CHARS {
                break;
            }
            let _ = writeln!(out, "{:>5}  {line}", i + 1);
            last = i + 1;
        }
        let cut_short = content.len() >= FETCH_BYTES;
        if last < total || cut_short {
            let total = if cut_short {
                words.fill(TEXTS, "at_least", &[("lines", total.to_string())])
            } else {
                total.to_string()
            };
            out.push_str(&words.fill(
                TEXTS,
                "lines_shown",
                &[
                    ("from", from.to_string()),
                    ("last", last.to_string()),
                    ("total", total),
                ],
            ));
        }
        Outcome::ok(out.trim_end().to_owned())
    }
}

pub struct Write {
    desk: Arc<Desk>,
}

impl Write {
    pub fn new(desk: Arc<Desk>) -> Self {
        Self { desk }
    }
}

#[async_trait]
impl Tool for Write {
    fn name(&self) -> &'static str {
        "write"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "write",
            json!({"path": {"type": "string"}, "content": {"type": "string"}}),
            &["path", "content"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let (path, content) = match (text(words, args, "path"), text(words, args, "content")) {
            (Ok(p), Ok(c)) => (p, c),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        match store(&self.desk, words, path, content).await {
            Ok(()) => Outcome::ok(words.fill(
                TEXTS,
                "wrote",
                &[
                    ("path", path.to_owned()),
                    ("lines", content.lines().count().to_string()),
                    ("bytes", content.len().to_string()),
                ],
            )),
            Err(refusal) => refusal,
        }
    }
}

pub struct Edit {
    desk: Arc<Desk>,
}

impl Edit {
    pub fn new(desk: Arc<Desk>) -> Self {
        Self { desk }
    }
}

#[async_trait]
impl Tool for Edit {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "edit",
            json!({
                "path": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "all": {"type": "boolean"},
            }),
            &["path", "old", "new"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let (path, old, new) = match (
            text(words, args, "path"),
            text(words, args, "old"),
            text(words, args, "new"),
        ) {
            (Ok(p), Ok(o), Ok(n)) => (p, o, n),
            (Err(refusal), _, _) | (_, Err(refusal), _) | (_, _, Err(refusal)) => return refusal,
        };
        if old.is_empty() {
            return Outcome::fail(words.text(TEXTS, "old_empty"));
        }
        if old == new {
            return Outcome::fail(words.text(TEXTS, "old_is_new"));
        }
        let content = match slurp(&self.desk, words, path, None).await {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };
        let at = || ("path", path.to_owned());
        if content.contains('\u{fffd}') {
            return Outcome::fail(words.fill(TEXTS, "not_utf8", &[at()]));
        }
        let found = content.matches(old).count();
        if found == 0
            && let Some(updated) = replace_loosely(&content, old, new)
        {
            // The text is there but for whitespace at the ends of lines,
            // which nobody can see. Take it.
            return match store(&self.desk, words, path, &updated).await {
                Ok(()) => Outcome::ok(words.fill(TEXTS, "edited_loosely", &[at()])),
                Err(refusal) => refusal,
            };
        }
        if found == 0 {
            // The commonest miss is indentation. Say so when that is all it is.
            let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            let hint = if squash(&content).contains(&squash(old)) {
                "old_whitespace_differs"
            } else {
                "old_read_again"
            };
            return Outcome::fail(words.fill(
                TEXTS,
                "old_not_found",
                &[at(), ("hint", words.text(TEXTS, hint).to_owned())],
            ));
        }
        let every = args["all"].as_bool() == Some(true);
        let count = ("count", found.to_string());
        if found > 1 && !every {
            return Outcome::fail(words.fill(TEXTS, "old_ambiguous", &[at(), count]));
        }
        let updated = if every {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        match store(&self.desk, words, path, &updated).await {
            Ok(()) if found == 1 => Outcome::ok(words.fill(TEXTS, "edited_once", &[at()])),
            Ok(()) => Outcome::ok(words.fill(TEXTS, "edited", &[at(), count])),
            Err(refusal) => refusal,
        }
    }
}

/// Replace `old` with `new` where `old` matches a run of whole lines except
/// for whitespace at the ends of lines. Only when there is exactly one such
/// place.
fn replace_loosely(content: &str, old: &str, new: &str) -> Option<String> {
    let want: Vec<&str> = old
        .trim_end_matches('\n')
        .lines()
        .map(str::trim_end)
        .collect();
    if want.is_empty() || want.iter().all(|l| l.is_empty()) {
        return None;
    }
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let matches: Vec<usize> = (0..=lines.len().saturating_sub(want.len()))
        .filter(|&at| {
            lines[at..at + want.len()]
                .iter()
                .zip(&want)
                .all(|(have, want)| have.trim_end() == *want)
        })
        .collect();
    let [at] = matches.as_slice() else {
        return None;
    };
    let mut out: String = lines[..*at].concat();
    out.push_str(new);
    // Keep the line break the replaced run ended with.
    if lines[at + want.len() - 1].ends_with('\n') && !new.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&lines[at + want.len()..].concat());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Local;

    #[test]
    fn an_edit_forgives_whitespace_nobody_can_see() {
        let content = "def f():   \n    return 1\t\n\ndef g():\n    return 1\n";
        let updated =
            replace_loosely(content, "def f():\n    return 1", "def f():\n    return 2").unwrap();
        assert_eq!(
            updated,
            "def f():\n    return 2\n\ndef g():\n    return 1\n"
        );
        // Still refuses when it would have to guess which place was meant.
        assert!(replace_loosely(content, "    return 1", "    return 3").is_none());
    }

    fn desk(dir: &std::path::Path) -> Arc<Desk> {
        Desk::new(Arc::new(Local::new(dir)))
    }

    #[tokio::test]
    async fn written_content_is_exact_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let d = desk(dir.path());
        let content = "line 'one'\n\t$HOME \"two\"\nthree\\n\n";
        let wrote = Write::new(d.clone())
            .call(
                &json!({"path": "deep/dir/f.txt", "content": content}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(!wrote.failed, "{}", wrote.output);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("deep/dir/f.txt")).unwrap(),
            content
        );

        let read = Read::new(d.clone())
            .call(
                &json!({"path": "deep/dir/f.txt"}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert_eq!(
            read.output,
            "    1  line 'one'\n    2  \t$HOME \"two\"\n    3  three\\n"
        );

        let part = Read::new(d)
            .call(
                &json!({"path": "deep/dir/f.txt", "from": 2, "lines": 1}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(part.output.starts_with("    2  \t$HOME"), "{}", part.output);
        assert!(
            part.output
                .ends_with("[lines 2-2 of 3; pass `from` to read on]"),
            "{}",
            part.output
        );
    }

    #[tokio::test]
    async fn an_edit_must_match_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.py"),
            "def f():\n    return 1\n\ndef g():\n    return 1\n",
        )
        .unwrap();
        let edit = Edit::new(desk(dir.path()));

        let twice = edit
            .call(
                &json!({"path": "a.py", "old": "    return 1", "new": "    return 2"}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(
            twice.failed && twice.output.contains("matches 2 places"),
            "{}",
            twice.output
        );

        let spaced = edit
            .call(
                &json!({"path": "a.py", "old": "def g():\n  return 1", "new": "x"}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(
            spaced.output.contains("whitespace differs"),
            "{}",
            spaced.output
        );

        let once = edit
            .call(&json!({"path": "a.py", "old": "def g():\n    return 1", "new": "def g():\n    return 2"}), &crate::Prompts::shipped())
            .await;
        assert!(!once.failed, "{}", once.output);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            "def f():\n    return 1\n\ndef g():\n    return 2\n"
        );
    }

    #[tokio::test]
    async fn reading_an_image_file_shows_the_image_from_where_the_shell_stands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("pics")).unwrap();
        std::fs::write(
            dir.path().join("pics/dot.png"),
            crate::tools::testing::PIXEL,
        )
        .unwrap();
        std::fs::write(dir.path().join("pics/notes.png"), "only text").unwrap();
        let d = desk(dir.path());
        d.moved_to(dir.path().join("pics").display().to_string());
        let read = Read::new(d.clone());

        let seen = read
            .call(&json!({"path": "dot.png"}), &crate::Prompts::shipped())
            .await;
        assert!(!seen.failed, "{}", seen.output);
        assert_eq!(seen.pictures.len(), 1);
        assert_eq!(seen.pictures[0].bytes, crate::tools::testing::PIXEL);
        assert_eq!(seen.output, "dot.png: an image (image/png, 67 bytes, 1×1)");
        // A name is not enough: what is not an image is read as text.
        let text = read
            .call(&json!({"path": "notes.png"}), &crate::Prompts::shipped())
            .await;
        assert!(text.pictures.is_empty());
        assert_eq!(text.output, "    1  only text");
    }

    #[tokio::test]
    async fn a_missing_file_is_said_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let read = Read::new(desk(dir.path()))
            .call(&json!({"path": "nope.txt"}), &crate::Prompts::shipped())
            .await;
        assert!(read.failed);
        assert!(read.output.contains("No such file"), "{}", read.output);
    }
}
