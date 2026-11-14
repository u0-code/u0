//! `read`, `write`, `edit`: files, without shell quoting in the way.

use std::{fmt::Write as _, sync::Arc, time::Duration};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use super::{Desk, MAX_PICTURE};
use crate::{
    machine::quote,
    tool::{Outcome, Tool, spec, text},
};

const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// The most of a file that is fetched for reading.
const FETCH_BYTES: usize = 400_000;
const DEFAULT_LINES: usize = 600;
const MAX_CHARS: usize = 40_000;

/// The whole file as text, or why it cannot be had.
async fn slurp(desk: &Desk, path: &str, limit: Option<usize>) -> Result<String, Outcome> {
    let q = quote(path);
    let fetch = limit.map_or_else(|| format!("cat -- {q}"), |n| format!("head -c {n} -- {q}"));
    // `cat` reports a missing file or a directory in its own words.
    let script = format!("if [ -d {q} ]; then echo {q} is a directory; exit 21; fi\n{fetch}");
    let out = desk
        .exec(&script, None, IO_TIMEOUT)
        .await
        .map_err(|e| Outcome::fail(format!("could not read {path}: {e}")))?;
    if !out.ok() {
        return Err(Outcome::fail(out.text.trim().to_owned()));
    }
    if out.text.contains('\0') {
        return Err(Outcome::fail(format!(
            "{path} is a binary file. Inspect it with shell tools (file, xxd, strings)."
        )));
    }
    if out.dropped > 0 && limit.is_none() {
        return Err(Outcome::fail(format!(
            "{path} is too large to handle whole. Work on it with shell tools (sed, awk, python)."
        )));
    }
    Ok(out.text)
}

async fn store(desk: &Desk, path: &str, content: &str) -> Result<(), Outcome> {
    let q = quote(path);
    let script = format!("mkdir -p -- \"$(dirname -- {q})\" && cat > {q}");
    let out = desk
        .exec(&script, Some(content.as_bytes().to_vec()), IO_TIMEOUT)
        .await
        .map_err(|e| Outcome::fail(format!("could not write {path}: {e}")))?;
    if out.ok() {
        Ok(())
    } else {
        Err(Outcome::fail(format!(
            "could not write {path}: {}",
            out.text.trim()
        )))
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
    async fn look(&self, path: &str) -> Option<Outcome> {
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
                return Some(Outcome::fail(format!(
                    "{path} is an image larger than {} MB, too large to be shown. Make a smaller copy with shell tools and read that.",
                    MAX_PICTURE / (1024 * 1024)
                )));
            }
            // Machines that cannot hand files over, and missing files, are
            // left to the text reader, which says so in its own words.
            Err(_) => return None,
        };
        let picture = super::picture(bytes)?;
        Some(Outcome::seen(
            format!("{path}: an image ({})", super::about(&picture)),
            picture,
        ))
    }
}

#[async_trait]
impl Tool for Read {
    fn spec(&self) -> ToolSpec {
        spec(
            "read",
            "Read a text file, with line numbers. Returns up to 600 lines; use `from` and `lines` to read a different part of a long file. An image file (PNG, JPEG, GIF or WebP, up to 5 MB) comes back as the image itself, so you can look at it.",
            json!({
                "path": {"type": "string", "description": "The file. Relative paths are taken from the shell's working directory."},
                "from": {"type": "integer", "description": "First line to return, counting from 1."},
                "lines": {"type": "integer", "description": "How many lines to return."},
            }),
            &["path"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let path = match text(args, "path") {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        if let Some(seen) = self.look(path).await {
            return seen;
        }
        let content = match slurp(&self.desk, path, Some(FETCH_BYTES)).await {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };
        if content.is_empty() {
            return Outcome::ok("(the file is empty)");
        }
        let from = usize::try_from(args["from"].as_u64().unwrap_or(1))
            .unwrap_or(1)
            .max(1);
        let want = usize::try_from(args["lines"].as_u64().unwrap_or(DEFAULT_LINES as u64))
            .unwrap_or(DEFAULT_LINES)
            .max(1);
        let total = content.lines().count();
        if from > total {
            return Outcome::fail(format!("{path} has only {total} lines"));
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
            let _ = write!(
                out,
                "[lines {from}-{last} of {}{total}; pass `from` to read on]",
                if cut_short { "at least " } else { "" }
            );
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
    fn spec(&self) -> ToolSpec {
        spec(
            "write",
            "Create a file or replace its whole content. Missing directories are created. The content is written exactly as given.",
            json!({
                "path": {"type": "string"},
                "content": {"type": "string", "description": "The complete content of the file."},
            }),
            &["path", "content"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (path, content) = match (text(args, "path"), text(args, "content")) {
            (Ok(p), Ok(c)) => (p, c),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        match store(&self.desk, path, content).await {
            Ok(()) => Outcome::ok(format!(
                "wrote {path} ({} lines, {} bytes)",
                content.lines().count(),
                content.len()
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
    fn spec(&self) -> ToolSpec {
        spec(
            "edit",
            "Replace an exact piece of text in a file. `old` must match the file character for character, including indentation, and must be unique unless `all` is true. To insert, replace a nearby line with itself plus the new text.",
            json!({
                "path": {"type": "string"},
                "old": {"type": "string", "description": "The text to replace, exactly as it is in the file."},
                "new": {"type": "string", "description": "The text to put in its place."},
                "all": {"type": "boolean", "description": "Replace every occurrence instead of requiring exactly one."},
            }),
            &["path", "old", "new"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (path, old, new) = match (text(args, "path"), text(args, "old"), text(args, "new")) {
            (Ok(p), Ok(o), Ok(n)) => (p, o, n),
            (Err(refusal), _, _) | (_, Err(refusal), _) | (_, _, Err(refusal)) => return refusal,
        };
        if old.is_empty() {
            return Outcome::fail("`old` is empty. To create a file use `write`.");
        }
        if old == new {
            return Outcome::fail("`old` and `new` are the same; nothing would change.");
        }
        let content = match slurp(&self.desk, path, None).await {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };
        if content.contains('\u{fffd}') {
            return Outcome::fail(format!(
                "{path} is not valid UTF-8, so editing it here could corrupt it. Use shell tools."
            ));
        }
        let found = content.matches(old).count();
        if found == 0
            && let Some(updated) = replace_loosely(&content, old, new)
        {
            // The text is there but for whitespace at the ends of lines,
            // which nobody can see. Take it.
            return match store(&self.desk, path, &updated).await {
                Ok(()) => Outcome::ok(format!(
                    "edited {path} (1 replacement; trailing whitespace differed from `old`)"
                )),
                Err(refusal) => refusal,
            };
        }
        if found == 0 {
            // The commonest miss is indentation. Say so when that is all it is.
            let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            let hint = if squash(&content).contains(&squash(old)) {
                "The words are in the file but the whitespace differs. Read the lines again and copy them exactly."
            } else {
                "Read the file again: it may have changed, or the text may differ from what you remember."
            };
            return Outcome::fail(format!("`old` was not found in {path}. {hint}"));
        }
        let every = args["all"].as_bool() == Some(true);
        if found > 1 && !every {
            return Outcome::fail(format!(
                "`old` matches {found} places in {path}. Include more of the surrounding lines so it matches one, or pass `all`: true."
            ));
        }
        let updated = if every {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        match store(&self.desk, path, &updated).await {
            Ok(()) => Outcome::ok(format!(
                "edited {path} ({found} replacement{})",
                if found == 1 { "" } else { "s" }
            )),
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
            .call(&json!({"path": "deep/dir/f.txt", "content": content}))
            .await;
        assert!(!wrote.failed, "{}", wrote.output);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("deep/dir/f.txt")).unwrap(),
            content
        );

        let read = Read::new(d.clone())
            .call(&json!({"path": "deep/dir/f.txt"}))
            .await;
        assert_eq!(
            read.output,
            "    1  line 'one'\n    2  \t$HOME \"two\"\n    3  three\\n"
        );

        let part = Read::new(d)
            .call(&json!({"path": "deep/dir/f.txt", "from": 2, "lines": 1}))
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
            .call(&json!({"path": "a.py", "old": "    return 1", "new": "    return 2"}))
            .await;
        assert!(
            twice.failed && twice.output.contains("matches 2 places"),
            "{}",
            twice.output
        );

        let spaced = edit
            .call(&json!({"path": "a.py", "old": "def g():\n  return 1", "new": "x"}))
            .await;
        assert!(
            spaced.output.contains("whitespace differs"),
            "{}",
            spaced.output
        );

        let once = edit
            .call(&json!({"path": "a.py", "old": "def g():\n    return 1", "new": "def g():\n    return 2"}))
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

        let seen = read.call(&json!({"path": "dot.png"})).await;
        assert!(!seen.failed, "{}", seen.output);
        assert_eq!(seen.pictures.len(), 1);
        assert_eq!(seen.pictures[0].bytes, crate::tools::testing::PIXEL);
        assert_eq!(seen.output, "dot.png: an image (image/png, 67 bytes, 1×1)");
        // A name is not enough: what is not an image is read as text.
        let text = read.call(&json!({"path": "notes.png"})).await;
        assert!(text.pictures.is_empty());
        assert_eq!(text.output, "    1  only text");
    }

    #[tokio::test]
    async fn a_missing_file_is_said_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let read = Read::new(desk(dir.path()))
            .call(&json!({"path": "nope.txt"}))
            .await;
        assert!(read.failed);
        assert!(read.output.contains("No such file"), "{}", read.output);
    }
}
