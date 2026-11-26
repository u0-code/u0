//! A pull request's changes as a reviewer reads them, from its patch alone,
//! whichever host it is on: each file with its lines added and removed and
//! a mark of its changes (so a file viewed shows unviewed again once it
//! changes), and how much of it is code, tests, mocks, generated or docs,
//! as t3code sorts them.

use std::hash::{Hash as _, Hasher as _};

/// What a changed file is, by its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Code,
    Tests,
    Mocks,
    Generated,
    Docs,
}

impl Part {
    pub const ALL: [Self; 5] = [
        Self::Code,
        Self::Tests,
        Self::Mocks,
        Self::Generated,
        Self::Docs,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Code => "Code",
            Self::Tests => "Tests",
            Self::Mocks => "Mocks",
            Self::Generated => "Generated",
            Self::Docs => "Docs",
        }
    }

    /// What `path` is, by the names files and folders are commonly given:
    /// lock files and build output are generated, test support (mocks,
    /// fixtures, snapshots) is kept apart from tests, and prose is docs.
    pub fn of(path: &str) -> Self {
        let mut parts: Vec<&str> = path.split(['/', '\\']).collect();
        let name = parts.pop().unwrap_or_default();
        let lower = name.to_lowercase();
        let dirs: Vec<String> = parts.iter().map(|d| d.to_lowercase()).collect();
        let under = |set: &[&str]| dirs.iter().any(|d| set.contains(&d.as_str()));
        let ends = |list: &[&str]| list.iter().any(|e| lower.ends_with(e));
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        let generated = GENERATED.contains(&lower.as_str())
            || lower.contains(".generated.")
            || lower.contains(".gen.")
            || ends(&[
                ".g.dart",
                ".pb.go",
                "_pb2.py",
                "_pb2_grpc.py",
                ".pb.cc",
                ".pb.h",
                ".min.js",
                ".min.css",
                ".js.map",
                ".css.map",
            ])
            || under(&["__generated__", "_generated", "generated"]);
        if generated {
            return Self::Generated;
        }
        // A middle word: `x.mock.ts`, `x.fixtures.json`.
        let middle = lower.rsplit('.').nth(1).unwrap_or_default();
        let mock_word = [
            "mock", "mocks", "fixture", "fixtures", "stub", "stubs", "fake", "fakes",
        ];
        // Case matters after the prefix: `MockClock.ts` is a mock, `Mockup.ts` is not.
        let mock_prefix = mock_word.iter().any(|w| {
            let cap = format!("{}{}", w[..1].to_uppercase(), &w[1..]);
            [*w, cap.as_str()].iter().any(|p| {
                name.strip_prefix(p).is_some_and(|rest| {
                    rest.is_empty()
                        || rest.starts_with(['-', '_', '.'])
                        || rest.starts_with(|c: char| c.is_ascii_uppercase())
                })
            })
        });
        if ext == "snap"
            || (lower.matches('.').count() >= 2 && mock_word.contains(&middle))
            || mock_prefix
            || under(&[
                "__fixtures__",
                "__mocks__",
                "__snapshots__",
                "fakes",
                "fixture",
                "fixtures",
                "mock",
                "mocks",
                "stubs",
                "testdata",
            ])
        {
            return Self::Mocks;
        }
        let test_word = ["test", "spec", "e2e", "eval", "stories"];
        let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s);
        if (lower.matches('.').count() >= 2 && test_word.contains(&middle))
            || stem.ends_with("_test")
            || stem.ends_with("_spec")
            || (lower.starts_with("test_") && ext == "py")
            || lower == "conftest.py"
            || lower.starts_with("vitest.setup.")
            || lower.starts_with("jest.setup.")
            || class_test(name)
            || under(&[
                "__tests__",
                "__test__",
                "cypress",
                "e2e",
                "evals",
                "spec",
                "specs",
                "test",
                "tests",
            ])
        {
            return Self::Tests;
        }
        let prose = ends(&[".md", ".mdx", ".markdown", ".rst", ".adoc", ".txt"]);
        let read_by_tools = ext == "txt"
            && [
                "requirements",
                "constraints",
                "cmakelists",
                "robots",
                "llms",
            ]
            .iter()
            .any(|p| lower.starts_with(p));
        if prose && !read_by_tools {
            return Self::Docs;
        }
        Self::Code
    }
}

/// Lock files, generated whole by a tool.
const GENERATED: [&str; 20] = [
    "bun.lock",
    "bun.lockb",
    "cargo.lock",
    "composer.lock",
    "deno.lock",
    "flake.lock",
    "gemfile.lock",
    "go.sum",
    "gradle.lockfile",
    "mix.lock",
    "npm-shrinkwrap.json",
    "package-lock.json",
    "packages.lock.json",
    "pipfile.lock",
    "pnpm-lock.yaml",
    "podfile.lock",
    "poetry.lock",
    "pubspec.lock",
    "uv.lock",
    "yarn.lock",
];

/// A test class by its name, case and all: `FooTest.java`, not `Latest.java`.
fn class_test(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    [
        "java", "kt", "kts", "scala", "groovy", "cs", "php", "swift", "m",
    ]
    .contains(&ext)
        && ["Test", "Tests", "Spec"].iter().any(|s| stem.ends_with(s))
}

/// A file a pull request changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    /// A mark of its changes: another once they change.
    pub mark: String,
}

/// The files `patch` (as git writes it) changes, read by diffy: each named
/// as it is after, its lines counted by its hunks, its mark a hash of them.
pub fn files(patch: &str) -> Vec<File> {
    use diffy::{
        Line,
        patch_set::{FileOperation, ParseOptions, PatchSet},
    };
    let read = PatchSet::parse(patch, ParseOptions::gitdiff()).filter_map(Result::ok);
    read.map(|file| {
        let named = match file.operation().strip_prefix(1) {
            FileOperation::Delete(p) | FileOperation::Create(p) => p.into_owned(),
            FileOperation::Modify { modified: p, .. }
            | FileOperation::Rename { to: p, .. }
            | FileOperation::Copy { to: p, .. } => p.into_owned(),
        };
        let mut hasher = std::hash::DefaultHasher::new();
        let (mut added, mut removed) = (0, 0);
        let lines = (file.patch().as_text().into_iter())
            .flat_map(diffy::Patch::hunks)
            .flat_map(diffy::Hunk::lines);
        for line in lines {
            let (sign, text) = match line {
                Line::Context(t) => (' ', t),
                Line::Delete(t) => {
                    removed += 1;
                    ('-', t)
                }
                Line::Insert(t) => {
                    added += 1;
                    ('+', t)
                }
            };
            (sign, *text).hash(&mut hasher);
        }
        File {
            path: named,
            added,
            removed,
            mark: format!("{:016x}", hasher.finish()),
        }
    })
    .collect()
}

/// How many lines of each part `files` add and remove, the parts with none
/// left out.
pub fn breakdown(files: &[File]) -> Vec<(Part, usize, usize)> {
    Part::ALL
        .into_iter()
        .map(|part| {
            let of = files.iter().filter(|f| Part::of(&f.path) == part);
            (
                part,
                of.clone().map(|f| f.added).sum(),
                of.map(|f| f.removed).sum(),
            )
        })
        .filter(|(_, a, r)| a + r > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_is_sorted_by_its_path_as_t3code_sorts_it() {
        let sorted = |p| Part::of(p);
        assert_eq!(sorted("Cargo.lock"), Part::Generated);
        assert_eq!(sorted("web/app.min.js"), Part::Generated);
        assert_eq!(sorted("src/__mocks__/db.ts"), Part::Mocks);
        assert_eq!(sorted("src/MockClock.ts"), Part::Mocks);
        assert_eq!(sorted("src/Mockup.ts"), Part::Code);
        assert_eq!(sorted("src/x.fixtures.json"), Part::Mocks);
        assert_eq!(sorted("src/a.test.ts"), Part::Tests);
        assert_eq!(sorted("tests/run.rs"), Part::Tests);
        assert_eq!(sorted("FooTest.java"), Part::Tests);
        assert_eq!(sorted("Latest.java"), Part::Code);
        assert_eq!(sorted("README.md"), Part::Docs);
        assert_eq!(sorted("requirements.txt"), Part::Code);
        assert_eq!(sorted("src/main.rs"), Part::Code);
    }

    #[test]
    fn a_patch_reads_as_its_files_counted_and_marked_by_their_changes() {
        let patch = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1,2 @@\n-x\n+y\n+z\n\
                     diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\n";
        let read = files(patch);
        assert_eq!(
            read.iter()
                .map(|f| (f.path.as_str(), f.added, f.removed))
                .collect::<Vec<_>>(),
            [("src/a.rs", 2, 1), ("README.md", 1, 1)]
        );
        let changed = files(&patch.replace("+z", "+w"));
        assert_ne!(read[0].mark, changed[0].mark);
        assert_eq!(read[1].mark, changed[1].mark);
        assert_eq!(breakdown(&read), [(Part::Code, 2, 1), (Part::Docs, 1, 1)]);
    }

    #[test]
    fn a_removed_comment_line_is_counted_and_a_quoted_name_read_as_it_is() {
        let patch = "diff --git a/q.sql b/q.sql\n--- a/q.sql\n+++ b/q.sql\n@@ -1,2 +1,2 @@\n--- a comment\n+++b\n x\n\
                     diff --git \"a/my notes.md\" \"b/my notes.md\"\n--- \"a/my notes.md\"\n+++ \"b/my notes.md\"\n@@ -1 +1 @@\n-a\n+b\n";
        let read: Vec<(String, usize, usize)> = (files(patch).into_iter())
            .map(|f| (f.path, f.added, f.removed))
            .collect();
        assert_eq!(
            read,
            [("q.sql".to_owned(), 1, 1), ("my notes.md".to_owned(), 1, 1)]
        );
    }
}
