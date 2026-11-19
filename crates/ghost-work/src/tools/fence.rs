//! Where a piece of work may write: its own folder, and nowhere else but
//! what whoever asked named.
//!
//! What a piece of work makes is kept in its folder, so that it is all in
//! one place and can be found again, and so that nobody's files fill up
//! with what was made beside them. A model asked to look at a folder tends
//! to put what it makes there; saying not to does not stop it. So `write`
//! and `edit` refuse a path outside the folder unless whoever asked named
//! that very file in their own words (the lines quoted with `> ` in the
//! task), and say where to write instead. A folder they named is what to
//! look at: work they want done in a folder of theirs has that folder as
//! its own. Scratch files may go in the temporary directory. A part in a
//! checkout of its own writes there and not in its work's folder.
//!
//! A shell command can still write anywhere: this keeps the tools honest,
//! not the shell.

use std::path::{Component, Path, PathBuf};

use crate::prompts::{Store, TEXTS};

/// Where one worker may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fence {
    /// Its own folder: the work's, or a part's checkout.
    pub own: PathBuf,
    /// For a part in a checkout of its own, its work's folder.
    pub of: Option<PathBuf>,
    /// The files whoever asked named in their own words.
    pub named: Vec<PathBuf>,
}

impl Fence {
    /// Why `path`, written from `cwd`, is not to be written, if it is not,
    /// in `words`.
    pub(crate) fn refuses(&self, words: &Store, path: &str, cwd: Option<&str>) -> Option<String> {
        let at = real(&resolve(path, cwd.map_or(&self.own, Path::new)));
        if at.starts_with(real(&self.own)) || self.named.iter().any(|n| real(n) == at) {
            return None;
        }
        let in_theirs = self.of.as_ref().is_some_and(|of| at.starts_with(real(of)));
        if !in_theirs && scratch(&at) {
            return None;
        }
        let key = if in_theirs {
            "in_their_folder"
        } else {
            "outside_the_folder"
        };
        Some(words.fill(
            TEXTS,
            key,
            &[
                ("path", at.display().to_string()),
                ("folder", self.own.display().to_string()),
            ],
        ))
    }
}

/// `path` as an absolute path with no `.` or `..` in it: `~/` read as the
/// home directory, and a relative path read from `from`.
pub(crate) fn resolve(path: &str, from: &Path) -> PathBuf {
    let path = crate::folder::expand(path.trim());
    let full = if path.is_absolute() {
        path
    } else {
        from.join(path)
    };
    let mut out = PathBuf::new();
    for part in full.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` as it really is, through any links on the way to it (`/var` is
/// `/private/var` here): the deepest part of it that is there, made real,
/// with the rest after it.
fn real(path: &Path) -> PathBuf {
    let mut there = path.to_path_buf();
    let mut rest = Vec::new();
    while !there.exists() {
        match (there.file_name().map(ToOwned::to_owned), there.parent()) {
            (Some(name), Some(up)) => {
                rest.push(name);
                there = up.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
    let mut out = there.canonicalize().unwrap_or(there);
    out.extend(rest.into_iter().rev());
    out
}

/// Whether `path` is in a temporary directory, where scratch files go.
fn scratch(path: &Path) -> bool {
    ["/tmp", "/private/tmp", "/var/tmp", "/private/var/tmp"]
        .iter()
        .any(|t| path.starts_with(t))
}

/// Whether a character can be part of a path written in running text.
fn in_path(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            '"' | '\''
                | '`'
                | '<'
                | '>'
                | '|'
                | ','
                | ';'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '，'
                | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '（'
                | '）'
                | '《'
                | '》'
                | '「'
                | '」'
                | '“'
                | '”'
                | '‘'
                | '’'
        )
}

/// The files named in `words`, someone's own: every path in them that
/// names a file, as `~/notes/a.md` or `/srv/out.txt` does, read from
/// `from`. A path that runs on into the words after it (`a.md里`) is cut
/// back to where it names a file.
pub(crate) fn named_files(words: &str, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let chars: Vec<char> = words.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let starts = (chars[i] == '/' || (chars[i] == '~' && chars.get(i + 1) == Some(&'/')))
            && (i == 0 || !in_path(chars[i - 1]) || !chars[i - 1].is_ascii());
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && in_path(chars[j]) {
            j += 1;
        }
        let mut candidate: String = chars[i..j].iter().collect();
        i = j;
        // Words run on after a path in a script written without spaces.
        loop {
            let trimmed = candidate.trim_end_matches(['.', ':', '!', '?']);
            let path = resolve(trimmed, home);
            if is_file(&path) {
                found.push(path);
                break;
            }
            match candidate.chars().last() {
                Some(c) if !c.is_ascii() => {
                    candidate.pop();
                }
                _ => break,
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Whether `path` names a file: one there now, or one with an extension.
fn is_file(path: &Path) -> bool {
    path.is_file()
        || (!path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.rsplit_once('.'))
                .is_some_and(|(stem, ext)| {
                    !stem.is_empty() && !ext.is_empty() && ext.chars().all(char::is_alphanumeric)
                }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fence(own: &str) -> Fence {
        Fence {
            own: PathBuf::from(own),
            of: None,
            named: vec![PathBuf::from("/srv/notes/summary.md")],
        }
    }

    #[test]
    fn the_work_writes_in_its_folder_in_scratch_and_in_files_named_for_it_only() {
        let words = crate::Prompts::shipped();
        let f = fence("/home/her/Ghost/2026-11-18/count-d2a4");
        assert!(
            f.refuses(&words, "counts.md", None).is_none(),
            "in its folder"
        );
        assert!(
            f.refuses(
                &words,
                "/home/her/Ghost/2026-11-18/count-d2a4/a/b.txt",
                None
            )
            .is_none()
        );
        assert!(f.refuses(&words, "/tmp/scratch.txt", None).is_none());
        assert!(
            f.refuses(&words, "/srv/notes/summary.md", None).is_none(),
            "named"
        );
        let why = f
            .refuses(&words, "/home/me/repo/prompts/counts.md", None)
            .unwrap();
        assert!(why.contains("outside the work's folder"), "{why}");
        // Having moved elsewhere, a relative path is read from there.
        assert!(
            f.refuses(&words, "core.md", Some("/home/me/repo"))
                .is_some()
        );
        assert!(
            f.refuses(&words, "../../../../me/x.md", None).is_some(),
            "no way out by .."
        );
    }

    #[test]
    fn a_part_in_its_own_checkout_is_told_to_write_there_and_not_in_its_works_folder() {
        let words = crate::Prompts::shipped();
        let f = Fence {
            own: PathBuf::from("/home/her/ghost/worktrees/w-1"),
            of: Some(PathBuf::from("/home/her/Ghost/w")),
            named: Vec::new(),
        };
        assert!(f.refuses(&words, "b.txt", None).is_none());
        let why = f.refuses(&words, "/home/her/Ghost/w/b.txt", None).unwrap();
        assert!(why.contains("its own checkout"), "{why}");
    }

    #[test]
    fn files_named_in_their_words_are_found_and_folders_named_are_not() {
        let home = Path::new("/home/me");
        let named = named_files(
            "> 数一下 /home/me/repo/prompts 下的文件，写到 /home/me/notes/counts.md。\n\
             > and put the log in ~/logs/run.txt, or /etc",
            home,
        );
        let mut want = vec![
            PathBuf::from("/home/me/notes/counts.md"),
            PathBuf::from(std::env::var("HOME").unwrap()).join("logs/run.txt"),
        ];
        want.sort();
        assert_eq!(named, want);
        assert!(named_files("> 看看 /home/me/repo/crates下每个", home).is_empty());
    }
}
