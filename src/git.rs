//! What the desk does with git around an agent's work: a worktree of its
//! own for a session, a checkpoint of the files before and after each
//! turn, the diff between two checkpoints, putting the files back as a
//! checkpoint had them, and committing, pushing and opening a pull request.
//!
//! Everything is the `git` (and `gh`) on this machine, run in the session's
//! folder. A checkpoint never touches your index or your branch: the files
//! are staged into an index of the desk's own (`GIT_INDEX_FILE`), written as
//! a commit no branch points at, and kept under `refs/ghost/checkpoints/`.
//! Every call here blocks; the desk makes them off the window's thread.

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

/// Where the checkpoints of a repository are kept.
const REFS: &str = "refs/ghost/checkpoints";

/// Run git in `dir` with `args`; its output, or what it said went wrong.
fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        // A checkpoint is the desk's, not yours: it is signed by Ghost and
        // needs nothing from your git configuration.
        .env("GIT_AUTHOR_NAME", "Ghost")
        .env("GIT_AUTHOR_EMAIL", "ghost@localhost")
        .env("GIT_COMMITTER_NAME", "Ghost")
        .env("GIT_COMMITTER_EMAIL", "ghost@localhost");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let out = command
        .output()
        .map_err(|e| format!("git could not be started: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(said.trim().to_owned())
    }
}

/// The top of the repository `dir` is in, if it is in one.
pub fn root(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"], None)
        .ok()
        .map(PathBuf::from)
}

/// The branch `dir` is on; `None` outside a repository or on a detached head.
pub fn branch(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"], None)
        .ok()
        .filter(|b| !b.is_empty() && b != "HEAD")
}

/// A worktree made for a session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worktree {
    /// The repository it was made from.
    pub repo: PathBuf,
    /// Where it is.
    pub path: PathBuf,
    /// The branch it is on, made for it.
    pub branch: String,
}

/// The worktrees of the repository at `repo` that live under `under`
/// (Ghost's own), with the branch each is on.
pub fn worktrees(repo: &Path, under: &Path) -> Vec<Worktree> {
    let Ok(out) = git(repo, &["worktree", "list", "--porcelain"], None) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut path: Option<PathBuf> = None;
    for line in out.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(p) = path.take().filter(|p| p.starts_with(under)) {
                found.push(Worktree {
                    repo: repo.to_path_buf(),
                    path: p,
                    branch: b.to_owned(),
                });
            }
        } else if line.is_empty() {
            path = None;
        }
    }
    found
}

/// `text` as a short name for a branch: lower case, words joined by `-`.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for word in text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
    {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&word.to_ascii_lowercase());
    }
    if out.is_empty() {
        out.push_str("session");
    }
    out.truncate(40);
    out.trim_end_matches('-').to_owned()
}

/// Make a worktree of the repository at `dir` for a session about `about`,
/// under `home/worktrees/<repo>/`, on a new branch `ghost/<slug>` from the
/// commit `dir` is on. A name already taken gets a number.
pub fn worktree(dir: &Path, about: &str, home: &Path) -> Result<Worktree, String> {
    let repo = root(dir).ok_or("the folder is not in a git repository")?;
    let name = repo
        .file_name()
        .map_or_else(|| "repo".to_owned(), |n| n.to_string_lossy().into_owned());
    let base = slug(about);
    for n in 1..100 {
        let slug = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let path = home.join("worktrees").join(&name).join(&slug);
        let branch = format!("ghost/{slug}");
        let taken = path.exists()
            || git(
                &repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{branch}"),
                ],
                None,
            )
            .is_ok();
        if taken {
            continue;
        }
        let at = path.to_string_lossy().into_owned();
        git(
            &repo,
            &["worktree", "add", "-b", &branch, &at, "HEAD"],
            None,
        )?;
        return Ok(Worktree { repo, path, branch });
    }
    Err("no free name for a worktree".into())
}

/// Take the worktree away. A worktree with changes not committed is
/// refused, and git says so; its branch is kept.
pub fn remove_worktree(wt: &Worktree) -> Result<(), String> {
    let at = wt.path.to_string_lossy().into_owned();
    git(&wt.repo, &["worktree", "remove", &at], None).map(|_| ())
}

/// A private index file, its own for each call.
fn scratch_index(dir: &Path) -> Result<PathBuf, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let git_dir = git(dir, &["rev-parse", "--absolute-git-dir"], None)?;
    Ok(PathBuf::from(git_dir).join(format!(
        "ghost-index-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

/// The files of `dir` as they are now, ignored files left out, written as a
/// commit kept under `refs/ghost/checkpoints/<name>`; its id.
pub fn checkpoint(dir: &Path, name: &str) -> Result<String, String> {
    let commit = snapshot(dir, name)?;
    git(
        dir,
        &["update-ref", &format!("{REFS}/{name}"), &commit],
        None,
    )?;
    Ok(commit)
}

/// What is not committed in `dir`, untracked files with it, against the
/// commit it is on: read through a commit no ref keeps, so your index is
/// left alone.
pub fn uncommitted(dir: &Path) -> Result<Vec<FileDiff>, String> {
    let now = snapshot(dir, "uncommitted")?;
    let base = git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], None)
        .unwrap_or_else(|_| EMPTY_TREE.to_owned());
    diff(dir, &base, &now)
}

/// The tree git gives a repository with nothing in it.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The files of `dir` as they are now, ignored files left out, written as a
/// commit on the one it is on; its id.
fn snapshot(dir: &Path, name: &str) -> Result<String, String> {
    let index = scratch_index(dir)?;
    let result = (|| {
        let head = git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], None).ok();
        if head.is_some() {
            git(dir, &["read-tree", "HEAD"], Some(&index))?;
        }
        git(dir, &["add", "-A", "--", "."], Some(&index))?;
        let tree = git(dir, &["write-tree"], Some(&index))?;
        let mut args = vec!["commit-tree", tree.as_str(), "-m", name];
        if let Some(h) = &head {
            args.extend(["-p", h.as_str()]);
        }
        git(dir, &args, None)
    })();
    let _ = std::fs::remove_file(&index);
    result
}

/// Put the files of `dir` back as the checkpoint `commit` had them: files
/// it did not have are removed, the rest written as they were. Your index
/// and branch are left alone. What was there just before is kept as a
/// checkpoint first, and its id is returned, so that this can be undone.
pub fn restore(dir: &Path, commit: &str, name: &str) -> Result<String, String> {
    let now = checkpoint(dir, name)?;
    let added = git(
        dir,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=A",
            commit,
            &now,
        ],
        None,
    )?;
    let top = root(dir).ok_or("the folder is not in a git repository")?;
    for path in added.lines().filter(|l| !l.is_empty()) {
        let _ = std::fs::remove_file(top.join(path));
    }
    let index = scratch_index(dir)?;
    let result = git(dir, &["read-tree", commit], Some(&index))
        .and_then(|_| git(&top, &["checkout-index", "-a", "-f"], Some(&index)));
    let _ = std::fs::remove_file(&index);
    result.map(|_| now)
}

/// How a file changed between two checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// One line of a hunk, with its number on either side where it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Same {
        old: usize,
        new: usize,
        text: String,
    },
    Gone {
        old: usize,
        text: String,
    },
    Added {
        new: usize,
        text: String,
    },
}

/// A stretch of a file that changed, with the lines around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// Git's own head for it: "@@ -58,9 +58,41 @@ fn send".
    pub head: String,
    pub lines: Vec<Line>,
}

/// One file's changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Its path from the top of the repository, as it is after.
    pub path: String,
    /// Its path before, when it was renamed.
    pub from: Option<String>,
    pub status: Status,
    pub added: usize,
    pub gone: usize,
    pub hunks: Vec<Hunk>,
    /// Whether git called it binary.
    pub binary: bool,
}

/// What changed in `dir` from the checkpoint `from` to `to`.
pub fn diff(dir: &Path, from: &str, to: &str) -> Result<Vec<FileDiff>, String> {
    let out = git(
        dir,
        &["diff", "--no-color", "--no-ext-diff", "-M", "-U3", from, to],
        None,
    )?;
    Ok(parse(&out))
}

/// A unified diff, read.
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old, mut new) = (0usize, 0usize);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).to_owned();
            files.push(FileDiff {
                path,
                from: None,
                status: Status::Modified,
                added: 0,
                gone: 0,
                hunks: Vec::new(),
                binary: false,
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("new file mode") {
            file.status = Status::Added;
        } else if line.starts_with("deleted file mode") {
            file.status = Status::Deleted;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.status = Status::Renamed;
            file.from = Some(from.to_owned());
        } else if let Some(to) = line.strip_prefix("rename to ") {
            to.clone_into(&mut file.path);
        } else if line.starts_with("Binary files ") {
            file.binary = true;
        } else if line.starts_with("@@") {
            let nums = |prefix: char| {
                line.split_whitespace()
                    .find_map(|w| w.strip_prefix(prefix))
                    .and_then(|w| w.split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0)
            };
            old = nums('-');
            new = nums('+');
            file.hunks.push(Hunk {
                head: line.to_owned(),
                lines: Vec::new(),
            });
        } else if let Some(hunk) = file.hunks.last_mut() {
            if let Some(t) = line.strip_prefix('+') {
                hunk.lines.push(Line::Added {
                    new,
                    text: t.to_owned(),
                });
                new += 1;
                file.added += 1;
            } else if let Some(t) = line.strip_prefix('-') {
                hunk.lines.push(Line::Gone {
                    old,
                    text: t.to_owned(),
                });
                old += 1;
                file.gone += 1;
            } else if let Some(t) = line.strip_prefix(' ') {
                hunk.lines.push(Line::Same {
                    old,
                    new,
                    text: t.to_owned(),
                });
                old += 1;
                new += 1;
            }
        }
    }
    files
}

/// Commit everything in `dir` with `message`, as you, on its branch; the
/// new commit's short id.
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    let mut add = Command::new("git");
    add.arg("-C").arg(dir).args(["add", "-A"]);
    run(&mut add)?;
    let mut commit = Command::new("git");
    commit
        .arg("-C")
        .arg(dir)
        .args(["commit", "-q", "-m", message]);
    run(&mut commit)?;
    let mut id = Command::new("git");
    id.arg("-C").arg(dir).args(["rev-parse", "--short", "HEAD"]);
    run(&mut id)
}

/// Push the branch `dir` is on to `origin`, setting it as upstream.
pub fn push(dir: &Path) -> Result<String, String> {
    let mut push = Command::new("git");
    push.arg("-C")
        .arg(dir)
        .args(["push", "-u", "origin", "HEAD"]);
    run(&mut push)
}

/// Open a pull request for the branch `dir` is on with `gh`, titled and
/// described from its commits; its address.
pub fn pull_request(dir: &Path) -> Result<String, String> {
    let mut pr = Command::new("gh");
    pr.current_dir(dir).args(["pr", "create", "--fill"]);
    let out = run(&mut pr)?;
    Ok(out
        .lines()
        .rev()
        .find(|l| l.starts_with("https://"))
        .unwrap_or(&out)
        .to_owned())
}

/// The files git sees in `dir`: tracked, and new ones not ignored; paths
/// from `dir`. At most a few thousand, which is enough to name one.
pub fn files(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(
        dir,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
        None,
    )
    .map(|t| t.lines().take(20_000).map(str::to_owned).collect())
    .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// The open pull requests of the repository at `dir`, by number and title,
/// as `gh` lists them; none where it is not installed or signed in.
pub fn pull_requests(dir: &Path) -> Vec<(u64, String)> {
    let mut gh = Command::new("gh");
    gh.current_dir(dir)
        .args(["pr", "list", "--limit", "30", "--json", "number,title"]);
    run(&mut gh)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok())
        .map(|list| {
            list.iter()
                .filter_map(|p| Some((p["number"].as_u64()?, p["title"].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// What `gh` says of the pull request `number`: its title, body and state.
pub fn pull_request_text(dir: &Path, number: u64) -> Option<String> {
    let mut gh = Command::new("gh");
    gh.current_dir(dir)
        .args(["pr", "view", &number.to_string()]);
    run(&mut gh).ok()
}

/// Run a command as you, with your git and gh configuration; its output.
fn run(command: &mut Command) -> Result<String, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program} could not be started: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let mut said = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        if said.is_empty() {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .clone_into(&mut said);
        }
        Err(said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository in a temporary folder with one committed file.
    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ghost-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"], None).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git(&dir, &["add", "a.txt"], None).unwrap();
        git(&dir, &["commit", "-q", "-m", "first"], None).unwrap();
        dir
    }

    #[test]
    fn what_is_not_committed_is_read_with_new_files_and_no_ref_or_index_touched() {
        let dir = repo("uncommitted");
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let files = uncommitted(&dir).unwrap();
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["a.txt", "b.txt"]);
        assert_eq!(
            git(&dir, &["diff", "--cached", "--name-only"], None).unwrap(),
            ""
        );
        assert_eq!(git(&dir, &["for-each-ref", REFS], None).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checkpoint_keeps_the_files_without_touching_the_index_or_branch() {
        let dir = repo("checkpoint");
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let before = git(&dir, &["rev-parse", "HEAD"], None).unwrap();
        let c = checkpoint(&dir, "s1/1-after").unwrap();
        assert_eq!(git(&dir, &["rev-parse", "HEAD"], None).unwrap(), before);
        assert_eq!(
            git(&dir, &["diff", "--cached", "--name-only"], None).unwrap(),
            ""
        );
        let files = diff(&dir, &before, &c).unwrap();
        assert_eq!(files.len(), 2);
        let a = files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!((a.added, a.gone, a.status), (1, 1, Status::Modified));
        assert!(matches!(a.hunks[0].lines[1], Line::Gone { old: 2, .. }));
        assert!(matches!(a.hunks[0].lines[2], Line::Added { new: 2, .. }));
        let b = files.iter().find(|f| f.path == "b.txt").unwrap();
        assert_eq!(b.status, Status::Added);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_puts_the_files_back_and_can_itself_be_undone() {
        let dir = repo("restore");
        let start = checkpoint(&dir, "s1/1-before").unwrap();
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let after = restore(&dir, &start, "s1/undo").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(!dir.join("b.txt").exists());
        restore(&dir, &after, "s1/redo").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "changed\n"
        );
        assert!(dir.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_worktree_gets_a_branch_of_its_own_and_a_second_one_a_number() {
        let dir = repo("worktree");
        let home = dir.join(".home");
        let a = worktree(&dir, "Retry S3 uploads!", &home).unwrap();
        let b = worktree(&dir, "Retry S3 uploads!", &home).unwrap();
        assert_eq!(a.branch, "ghost/retry-s3-uploads");
        assert_eq!(b.branch, "ghost/retry-s3-uploads-2");
        assert!(a.path.join("a.txt").is_file());
        assert_eq!(branch(&a.path).as_deref(), Some("ghost/retry-s3-uploads"));
        remove_worktree(&a).unwrap();
        assert!(!a.path.exists());
        let _ = remove_worktree(&b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_is_read_with_both_its_names() {
        let files = parse(
            "diff --git a/x.rs b/y.rs\nsimilarity index 90%\nrename from x.rs\nrename to y.rs\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n",
        );
        assert_eq!(files[0].status, Status::Renamed);
        assert_eq!(files[0].from.as_deref(), Some("x.rs"));
        assert_eq!(files[0].path, "y.rs");
        assert_eq!((files[0].added, files[0].gone), (1, 1));
    }
}
