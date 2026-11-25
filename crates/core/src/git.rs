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
    fmt::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use agent_client_protocol::v1::schema::Diff;
use similar::{ChangeTag, TextDiff};

/// Where the checkpoints of a repository are kept.
const REFS: &str = "refs/ghost/checkpoints";

/// Run git in `dir` with `args`; its output, or what it said went wrong.
fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        // Names as they are (架构.md), not in octal escapes.
        .args(["-c", "core.quotepath=false"])
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

/// What a new worktree starts from when no branch is chosen, as t3code
/// starts one: the remote's default branch, fetched first; where there is
/// no remote, or it cannot be reached, where the folder is.
pub fn fresh_base(dir: &Path) -> String {
    let Some(repo) = root(dir) else {
        return "HEAD".into();
    };
    if git(&repo, &["fetch", "--quiet", "origin"], None).is_err() {
        return "HEAD".into();
    }
    git(
        &repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        None,
    )
    .map(|b| b.trim().to_owned())
    .ok()
    .filter(|b| !b.is_empty())
    .unwrap_or_else(|| "HEAD".into())
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
/// under `home/worktrees/<repo>/`, on a new branch `<prefix><slug>` from
/// the commit `dir` is on. A name already taken gets a number.
pub fn worktree(
    dir: &Path,
    about: &str,
    home: &Path,
    prefix: &str,
    from: &str,
) -> Result<Worktree, String> {
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
        let branch = format!("{prefix}{slug}");
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
        git(&repo, &["worktree", "add", "-b", &branch, &at, from], None)?;
        return Ok(Worktree { repo, path, branch });
    }
    Err("no free name for a worktree".into())
}

/// Make a worktree of the repository at `dir` on the branch of the pull
/// request `number`, as `gh pr checkout` sets it up (a fork's included,
/// so that a push goes to the pull request), under
/// `home/worktrees/<repo>/pr-<number>`.
pub fn pr_worktree(dir: &Path, number: u64, home: &Path) -> Result<Worktree, String> {
    let repo = root(dir).ok_or("the folder is not in a git repository")?;
    let name = repo
        .file_name()
        .map_or_else(|| "repo".to_owned(), |n| n.to_string_lossy().into_owned());
    let path = (1..100)
        .map(|n| {
            let slug = if n == 1 {
                format!("pr-{number}")
            } else {
                format!("pr-{number}-{n}")
            };
            home.join("worktrees").join(&name).join(slug)
        })
        .find(|p| !p.exists())
        .ok_or("no free name for a worktree")?;
    let at = path.to_string_lossy().into_owned();
    git(&repo, &["worktree", "add", "--detach", &at, "HEAD"], None)?;
    let branch = yours(&path, "gh", &["pr", "checkout", &number.to_string()])
        .and_then(|_| git(&path, &["branch", "--show-current"], None));
    match branch {
        Ok(branch) if !branch.trim().is_empty() => Ok(Worktree {
            repo,
            path,
            branch: branch.trim().to_owned(),
        }),
        other => {
            let _ = git(&repo, &["worktree", "remove", "--force", &at], None);
            Err(other
                .err()
                .unwrap_or_else(|| "the pull request's branch was not checked out".into()))
        }
    }
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
pub fn uncommitted(dir: &Path, no_spacing: bool) -> Result<Vec<FileDiff>, String> {
    let now = snapshot(dir, "uncommitted")?;
    let base = git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], None)
        .unwrap_or_else(|_| EMPTY_TREE.to_owned());
    diff(dir, &base, &now, no_spacing)
}

/// The work in `dir` as git prints it, untracked files with it: what is
/// not committed, or with `branch` all since the branch left the main
/// one. Its stat, then its patch cut at `most` bytes.
pub fn patch(dir: &Path, branch: bool, most: usize) -> Result<String, String> {
    let now = snapshot(dir, "written of")?;
    let from = if branch {
        let base = default_branch(dir).ok_or("the repository has no main branch")?;
        git(dir, &["merge-base", "HEAD", &base], None)?
    } else {
        git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], None)
            .unwrap_or_else(|_| EMPTY_TREE.to_owned())
    };
    let stat = git(dir, &["diff", "--stat", &from, &now], None)?;
    if stat.is_empty() {
        return Err("there are no changes".into());
    }
    let mut patch = git(dir, &["diff", &from, &now], None)?;
    if patch.len() > most {
        patch.truncate(patch.floor_char_boundary(most));
        patch.push_str("\n[the rest is cut]");
    }
    Ok(format!("{stat}\n\n{patch}"))
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
            // HEAD read over a copy of your index keeps its stat data
            // where the two agree, so `add` hashes only files that changed
            // since, not the whole tree.
            let _ = std::fs::copy(index.with_file_name("index"), &index);
            git(dir, &["read-tree", "--reset", "HEAD"], Some(&index))?;
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
    // Only `dir` is put back: a checkpoint holds the rest of the
    // repository as it was committed, not as it is on disk.
    let added = git(
        dir,
        &[
            "diff",
            "--name-only",
            "-z",
            "--relative",
            "--no-renames",
            "--diff-filter=A",
            commit,
            &now,
            "--",
            ".",
        ],
        None,
    )?;
    for path in added.split('\0').filter(|l| !l.is_empty()) {
        let _ = std::fs::remove_file(dir.join(path));
    }
    let index = scratch_index(dir)?;
    let result = git(dir, &["read-tree", commit], Some(&index))
        .and_then(|_| git(dir, &["checkout-index", "-a", "-f"], Some(&index)));
    let _ = std::fs::remove_file(&index);
    result.map(|_| now)
}

/// How a file changed between two checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// One file's changes: what lists name and count of them, and its patch
/// as git writes it, which the desk's diff view reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileDiff {
    /// Its path from the top of the repository, as it is after.
    pub path: String,
    /// Its path before, when it was renamed.
    pub from: Option<String>,
    pub status: Status,
    pub added: usize,
    pub gone: usize,
    /// Whether git called it binary.
    pub binary: bool,
    /// Its part of the unified diff, from its `diff --git` (or `---`) line.
    pub patch: String,
}

/// An edit as git would show it: each stretch that changed, with a line of
/// context either side, numbered from `start`, the line of the file its
/// text begins on (one where that is not known).
pub fn edit_diff(d: &Diff, start: Option<usize>) -> FileDiff {
    let old = d.old_text.as_deref().unwrap_or("");
    let at = start.unwrap_or(1);
    let text = TextDiff::from_lines(old, &d.new_text);
    let named = d.path.display().to_string();
    let (mut added, mut gone) = (0, 0);
    let from = if d.old_text.is_some() {
        format!("a/{named}")
    } else {
        "/dev/null".to_owned()
    };
    let mut patch = format!("--- {from}\n+++ b/{named}\n");
    for group in text.grouped_ops(1) {
        let (first, last) = (&group[0], &group[group.len() - 1]);
        let (o, n) = (first.old_range().start, first.new_range().start);
        let (o_len, n_len) = (last.old_range().end - o, last.new_range().end - n);
        let _ = writeln!(patch, "@@ -{},{o_len} +{},{n_len} @@", o + at, n + at);
        for c in group.iter().flat_map(|op| text.iter_changes(op)) {
            let sign = match c.tag() {
                ChangeTag::Delete => {
                    gone += 1;
                    '-'
                }
                ChangeTag::Insert => {
                    added += 1;
                    '+'
                }
                ChangeTag::Equal => ' ',
            };
            let _ = writeln!(patch, "{sign}{}", c.value().trim_end_matches(['\r', '\n']));
        }
    }
    FileDiff {
        path: named,
        from: None,
        status: if d.old_text.is_none() {
            Status::Added
        } else {
            Status::Modified
        },
        added,
        gone,
        binary: false,
        patch,
    }
}

/// What changed in `dir` from the checkpoint `from` to `to`; with
/// `no_spacing` (as the settings say), a change only in spacing is left out.
pub fn diff(dir: &Path, from: &str, to: &str, no_spacing: bool) -> Result<Vec<FileDiff>, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-M", "-U3"];
    if no_spacing {
        args.push("-w");
    }
    args.extend([from, to]);
    let out = git(dir, &args, None)?;
    Ok(parse(&out))
}

/// The new name in a `diff --git a/x b/y` line: after " b/", or the
/// second name quoted where git quotes it (a tab, a quote in it).
fn new_name(rest: &str) -> String {
    if rest.ends_with('"')
        && let Some(at) = rest.rfind(" \"b/")
    {
        let named = unquote(&rest[at + 1..]);
        return named.strip_prefix("b/").unwrap_or(&named).to_owned();
    }
    rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).to_owned()
}

/// A name as git writes it: as it is, or in quotes with C escapes (\t,
/// \", \\, and each byte not plain ASCII as three octal digits).
fn unquote(name: &str) -> String {
    let Some(inner) = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) else {
        return name.to_owned();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match chars.next() {
            Some(b't') => bytes.push(b'\t'),
            Some(b'n') => bytes.push(b'\n'),
            Some(d @ b'0'..=b'7') => {
                let mut n = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&d @ b'0'..=b'7') = chars.peek() {
                        n = n * 8 + u32::from(d - b'0');
                        chars.next();
                    }
                }
                bytes.push(u8::try_from(n).unwrap_or(b'?'));
            }
            Some(other) => bytes.push(other),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A unified diff, cut into its files: each named and counted, with its
/// own part of the patch.
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileDiff {
                path: new_name(rest),
                from: None,
                status: Status::Modified,
                added: 0,
                gone: 0,
                binary: false,
                patch: String::new(),
            });
            in_hunk = false;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        file.patch.push_str(line);
        file.patch.push('\n');
        if line.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk {
            if line.starts_with('+') {
                file.added += 1;
            } else if line.starts_with('-') {
                file.gone += 1;
            }
        } else if line.starts_with("new file mode") {
            file.status = Status::Added;
        } else if line.starts_with("deleted file mode") {
            file.status = Status::Deleted;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.status = Status::Renamed;
            file.from = Some(unquote(from));
        } else if let Some(to) = line.strip_prefix("rename to ") {
            file.path = unquote(to);
        } else if line.starts_with("Binary files ") {
            file.binary = true;
        }
    }
    files
}

/// Commit everything in `dir` with `message`, as you, on its branch; the
/// new commit's short id.
pub fn commit(dir: &Path, message: &str, left_out: &[String]) -> Result<String, String> {
    yours(dir, "git", &["add", "-A"])?;
    // The files you left out stay as they are, out of the commit.
    if !left_out.is_empty() {
        let mut back = vec!["reset", "-q", "--"];
        back.extend(left_out.iter().map(String::as_str));
        yours(dir, "git", &back)?;
    }
    yours(dir, "git", &["commit", "-q", "-m", message])?;
    yours(dir, "git", &["rev-parse", "--short", "HEAD"])
}

/// Push the branch `dir` is on to `origin`, setting it as upstream.
pub fn push(dir: &Path) -> Result<String, String> {
    yours(dir, "git", &["push", "-u", "origin", "HEAD"])
}

/// Open a pull request for the branch `dir` is on with `gh`, with `title`
/// and `body`; its address.
pub fn pull_request(dir: &Path, title: &str, body: &str) -> Result<String, String> {
    let host = remote(dir).map(|r| r.0).unwrap_or_default();
    let last_url = |out: &str| {
        out.lines()
            .rev()
            .find(|l| l.trim_start().starts_with("https://"))
            .unwrap_or(out)
            .trim()
            .to_owned()
    };
    // GitHub through gh; GitLab through glab where it is installed; any
    // other host (Bitbucket, GitLab without glab) by its page for a new
    // one, opened in the browser.
    if host.contains("gitlab") && crate::registry::which("glab").is_some() {
        let args = [
            "mr",
            "create",
            "--title",
            title,
            "--description",
            body,
            "--yes",
        ];
        return yours(dir, "glab", &args).map(|o| last_url(&o));
    }
    if host.contains("github") || remote(dir).is_none() {
        let args = ["pr", "create", "--title", title, "--body", body];
        return yours(dir, "gh", &args).map(|o| last_url(&o));
    }
    let (host, path) = remote(dir).ok_or("the repository has no remote")?;
    let branch = branch(dir).ok_or("the folder is on no branch")?;
    Ok(if host.contains("bitbucket") {
        format!("https://{host}/{path}/pull-requests/new?source={branch}")
    } else {
        format!("https://{host}/{path}/-/merge_requests/new?merge_request[source_branch]={branch}")
    })
}

/// The host and `owner/name` of `origin`, from its address in either form
/// (`https://host/owner/name.git`, `git@host:owner/name.git`).
pub fn remote(dir: &Path) -> Option<(String, String)> {
    let url = git(dir, &["remote", "get-url", "origin"], None).ok()?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .unwrap_or_else(|| url.split_once('@').map_or(url.as_str(), |(_, r)| r));
    let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
    let (host, path) = rest.split_once(['/', ':'])?;
    Some((host.to_owned(), path.trim_end_matches(".git").to_owned()))
}

/// The branch the repository's work goes into: `origin`'s HEAD, else
/// `main`, else `master`.
pub fn default_branch(dir: &Path) -> Option<String> {
    if let Ok(head) = git(
        dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        None,
    ) {
        return Some(head);
    }
    ["main", "master"]
        .into_iter()
        .find(|b| git(dir, &["rev-parse", "--verify", "--quiet", b], None).is_ok())
        .map(str::to_owned)
}

/// What changed from where the branch left the default one to the files
/// as they are now, not committed included: the branch's whole work.
pub fn against_base(
    dir: &Path,
    base: Option<&str>,
    no_spacing: bool,
) -> Result<Vec<FileDiff>, String> {
    let base = match base {
        Some(b) => b.to_owned(),
        None => default_branch(dir).ok_or("the repository has no main branch")?,
    };
    let fork = git(dir, &["merge-base", "HEAD", &base], None)?;
    let now = snapshot(dir, "against base")?;
    diff(dir, &fork, &now, no_spacing)
}

/// The last `n` commits: short id, subject, how long ago.
pub fn log(dir: &Path, n: usize) -> Vec<(String, String, String)> {
    git(
        dir,
        &["log", &format!("-{n}"), "--format=%h%x09%s%x09%ct"],
        None,
    )
    .map(|t| {
        // The time is said as the desk says times, not in git's language.
        let now = std::time::SystemTime::now();
        t.lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let (id, subject) = (f.next()?, f.next()?);
                let secs = f.next()?.trim().parse().ok()?;
                let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
                Some((id.to_owned(), subject.to_owned(), crate::when::ago(at, now)))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// What the commit `id` changed.
pub fn commit_diff(dir: &Path, id: &str, no_spacing: bool) -> Result<Vec<FileDiff>, String> {
    let parent = git(
        dir,
        &["rev-parse", "--verify", "--quiet", &format!("{id}^")],
        None,
    )
    .unwrap_or_else(|_| EMPTY_TREE.to_owned());
    diff(dir, &parent, id, no_spacing)
}

/// The local branches, the one checked out first.
pub fn branches(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(dir, &["branch", "--format=%(refname:short)"], None)
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(now) = branch(dir) {
        out.retain(|b| b != &now);
        out.insert(0, now);
    }
    out
}

/// Check out `name` in `dir`, as you would.
pub fn switch(dir: &Path, name: &str) -> Result<(), String> {
    yours(dir, "git", &["switch", name]).map(|_| ())
}

/// Make the branch `name` at what `dir` is on, and check it out.
pub fn branch_off(dir: &Path, name: &str) -> Result<(), String> {
    yours(dir, "git", &["switch", "-c", name]).map(|_| ())
}

/// Rename the branch `dir` is on to `name`, unless it has been pushed.
pub fn rename_branch(dir: &Path, name: &str) -> Result<(), String> {
    if git(dir, &["rev-parse", "--abbrev-ref", "@{u}"], None).is_ok() {
        return Err("it is pushed already".into());
    }
    git(dir, &["branch", "-m", name], None).map(|_| ())
}

/// The sessions that have checkpoints in the repository at `dir`, by name.
pub fn checkpoint_owners(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(dir, &["for-each-ref", "--format=%(refname)", REFS], None)
        .map(|t| {
            t.lines()
                .filter_map(|r| r.strip_prefix(&format!("{REFS}/"))?.split('/').next())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// Take away the checkpoints Ghost kept for the session `uid` in the
/// repository at `dir`.
pub fn drop_checkpoints(dir: &Path, uid: &str) {
    let prefix = format!("{REFS}/{uid}/");
    if let Ok(refs) = git(dir, &["for-each-ref", "--format=%(refname)", &prefix], None) {
        for r in refs.lines() {
            let _ = git(dir, &["update-ref", "-d", r], None);
        }
    }
}

/// Make a new project at `dir`: the folder, a repository on `main`, a
/// README with its name, committed as you (your git identity).
pub fn create(dir: &Path, name: &str) -> Result<(), String> {
    if dir.exists() {
        return Err(format!("{} is there already", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("README.md"), format!("# {name}\n")).map_err(|e| e.to_string())?;
    init_here(dir).map(|_| ())
}

/// How far the branch `dir` is on is behind its upstream (0 with none),
/// and whether the repository has a remote called origin.
pub fn upstream(dir: &Path) -> (usize, bool) {
    let behind = git(dir, &["rev-list", "--count", "HEAD..@{u}"], None)
        .ok()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    let origin = git(dir, &["remote", "get-url", "origin"], None).is_ok();
    (behind, origin)
}

/// Bring in what the upstream has, only where it moves straight on.
pub fn pull(dir: &Path) -> Result<String, String> {
    yours(dir, "git", &["pull", "--ff-only", "-q"])
}

/// Make the folder `dir` a repository on main, all it holds committed.
pub fn init_here(dir: &Path) -> Result<String, String> {
    yours(dir, "git", &["init", "-q", "-b", "main"])?;
    yours(dir, "git", &["add", "-A"])?;
    yours(dir, "git", &["commit", "-q", "-m", "First commit"])
}

/// Put the repository at `dir` on GitHub, private, as gh does, and push.
pub fn publish(dir: &Path) -> Result<String, String> {
    yours(
        dir,
        "gh",
        &["repo", "create", "--private", "--source", ".", "--push"],
    )
}

/// Clone `url` into `dir`, saying how far it is from what git prints
/// ("Receiving objects:  45% ..."), until it is done or `stop` is set; a
/// clone stopped leaves nothing.
pub fn clone(
    url: &str,
    dir: &Path,
    say: &dyn Fn(&str, Option<f32>),
    stop: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    use std::io::Read as _;
    if dir.exists() {
        return Err(format!("{} is there already", dir.display()));
    }
    let mut child = Command::new("git")
        // A "URL" that begins with a dash is not taken as an option
        // (--upload-pack= runs a command).
        .args(["clone", "--progress", "--", url])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git could not be started: {e}"))?;
    let mut err = child.stderr.take().expect("piped");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut line = String::new();
        while let Ok(n) = err.read(&mut buf) {
            if n == 0 {
                break;
            }
            for c in String::from_utf8_lossy(&buf[..n]).chars() {
                if c == '\r' || c == '\n' {
                    if !line.trim().is_empty() {
                        let _ = tx.send(std::mem::take(&mut line));
                    }
                    line.clear();
                } else {
                    line.push(c);
                }
            }
        }
    });
    let mut last = String::new();
    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(dir);
            return Err(crate::registry::CANCELLED.into());
        }
        while let Ok(l) = rx.try_recv() {
            let share = l
                .split_once('%')
                .and_then(|(a, _)| a.rsplit(' ').next()?.trim().parse::<f32>().ok())
                .map(|p| p / 100.);
            say(l.trim(), share);
            last = l;
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                let _ = std::fs::remove_dir_all(dir);
                Err(format!("git clone failed: {}", last.trim()))
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
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

/// The pull request of the branch `dir` is on, as `gh` says.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pr {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// open, merged, closed; draft.
    pub state: String,
    pub draft: bool,
    /// It cannot be merged as it is: it conflicts with its base.
    pub conflicts: bool,
    /// Its checks: how many passed, failed, and are still going.
    pub passed: usize,
    pub failed: usize,
    pub going: usize,
    /// approved, changes requested, review required; empty for none.
    pub review: String,
}

/// The pull request of the branch `dir` is on, as `gh` says.
pub fn read_pr(dir: &Path) -> Option<Pr> {
    let out = yours(
        dir,
        "gh",
        &[
            "pr",
            "view",
            "--json",
            "number,title,url,state,isDraft,reviewDecision,statusCheckRollup,mergeable",
        ],
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&out).ok()?;
    let checks = v["statusCheckRollup"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let count = |f: &dyn Fn(&str) -> bool| {
        checks
            .iter()
            .filter(|c| {
                let word = c["conclusion"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| c["state"].as_str())
                    .unwrap_or("");
                f(&word.to_ascii_uppercase())
            })
            .count()
    };
    Some(Pr {
        number: v["number"].as_u64()?,
        title: v["title"].as_str().unwrap_or_default().to_owned(),
        url: v["url"].as_str().unwrap_or_default().to_owned(),
        state: v["state"].as_str().unwrap_or_default().to_lowercase(),
        draft: v["isDraft"].as_bool().unwrap_or(false),
        conflicts: v["mergeable"].as_str() == Some("CONFLICTING"),
        passed: count(&|w| matches!(w, "SUCCESS" | "NEUTRAL" | "SKIPPED")),
        failed: count(&|w| {
            matches!(
                w,
                "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED"
            )
        }),
        going: count(&|w| matches!(w, "PENDING" | "QUEUED" | "IN_PROGRESS" | "EXPECTED" | "")),
        review: v["reviewDecision"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .replace('_', " "),
    })
}

/// The open pull requests (`what` "pr") or issues ("issue") of the
/// repository at `dir`, by number and title, as `gh` lists them; none
/// where it is not installed or signed in.
pub fn open_on_github(dir: &Path, what: &str) -> Vec<(u64, String)> {
    yours(
        dir,
        "gh",
        &[what, "list", "--limit", "30", "--json", "number,title"],
    )
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
    yours(dir, "gh", &["pr", "view", &number.to_string()]).ok()
}

/// Where `words` are in the files git sees in `dir`: "path:line" and the
/// line, fifty at most.
pub fn grep(dir: &Path, words: &str, how: Match) -> Vec<(String, String)> {
    let mut args = vec!["grep", "-n", "-I", "--max-count=5"];
    args.push(if how.regex { "-E" } else { "-F" });
    if !how.case {
        args.push("-i");
    }
    if how.word {
        args.push("-w");
    }
    args.extend(["-e", words]);
    git(dir, &args, None)
        .unwrap_or_default()
        .lines()
        .take(50)
        .filter_map(|l| {
            let (path, rest) = l.split_once(':')?;
            let (n, text) = rest.split_once(':')?;
            Some((format!("{path}:{n}"), text.to_owned()))
        })
        .collect()
}

/// How words are looked for in a project's files: in their case, as whole
/// words, as a regular expression.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Match {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

/// Merge the pull request of the branch `dir` is on, squashed, as gh does.
pub fn merge_pr(dir: &Path) -> Result<String, String> {
    yours(dir, "gh", &["pr", "merge", "--squash"])
}

/// Run `program` (git, gh, glab) in `dir` with `args` as you, with your
/// git and gh configuration; its output, or what it said went wrong.
fn yours(dir: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .current_dir(dir)
        .args(args)
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

    fn edit(old: Option<&str>, new: &str) -> Diff {
        Diff::new("a.rs", new).old_text(old.map(str::to_owned))
    }

    #[test]
    fn an_edit_shows_what_went_and_what_came_with_a_line_either_side() {
        let d = edit(Some("fn a() {\n    1\n}\n"), "fn a() {\n    2\n    3\n}\n");
        let file = edit_diff(&d, Some(10));
        assert_eq!(
            file.patch,
            "--- a/a.rs\n+++ b/a.rs\n@@ -10,3 +10,4 @@\n fn a() {\n-    1\n+    2\n+    3\n }\n"
        );
        assert_eq!((file.gone, file.added), (1, 2));
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
        let file = edit_diff(&edit(Some(&old), &new), Some(1));
        assert_eq!(file.patch.matches("\n@@ ").count(), 2);
        assert_eq!((file.gone, file.added), (2, 2));
        assert!(file.patch.contains("\n@@ -17,3 +17,3 @@\n"));
    }

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
    fn a_name_git_quotes_reads_as_it_is() {
        let d = parse(
            "diff --git \"a/reports/JEPA \\346\\236\\266.md\" \"b/reports/JEPA \\346\\236\\266.md\"\nnew file mode 100644\n@@ -0,0 +1 @@\n+x\n",
        );
        assert_eq!(d[0].path, "reports/JEPA 架.md");
        let d = parse("diff --git a/报告 一.md b/报告 一.md\n@@ -1 +1 @@\n-a\n+b\n");
        assert_eq!(d[0].path, "报告 一.md");
        assert_eq!(unquote("\"tab\\there\""), "tab\there");
    }

    #[test]
    fn a_folder_made_a_repository_has_all_it_held_committed_and_no_upstream() {
        let dir = std::env::temp_dir().join(format!("ghost-git-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "hi\n").unwrap();
        init_here(&dir).unwrap();
        assert_eq!(git(&dir, &["status", "--porcelain"], None).unwrap(), "");
        assert_eq!(upstream(&dir), (0, false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn words_are_found_with_their_file_and_line_in_case_whole_or_as_a_pattern() {
        let dir = repo("grep");
        let any = Match::default();
        assert_eq!(
            grep(&dir, "TWO", any),
            vec![("a.txt:2".to_owned(), "two".to_owned())]
        );
        assert!(grep(&dir, "nowhere", any).is_empty());
        let case = Match { case: true, ..any };
        assert!(grep(&dir, "TWO", case).is_empty());
        let word = Match { word: true, ..any };
        assert!(grep(&dir, "tw", word).is_empty());
        let regex = Match { regex: true, ..any };
        assert_eq!(grep(&dir, "^t[wh]", regex).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_left_out_of_a_commit_stays_as_it_was() {
        let dir = repo("left-out");
        git(&dir, &["config", "user.email", "t@localhost"], None).unwrap();
        git(&dir, &["config", "user.name", "t"], None).unwrap();
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        commit(&dir, "only a", &["b.txt".to_owned()]).unwrap();
        let left = git(&dir, &["status", "--porcelain"], None).unwrap();
        assert_eq!(left.trim(), "?? b.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_project_is_a_repository_with_its_readme_and_a_clone_says_how_far_it_is() {
        let base = std::env::temp_dir().join(format!("ghost-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let made = base.join("made");
        if create(&made, "made").is_ok() {
            assert_eq!(
                std::fs::read_to_string(made.join("README.md")).unwrap(),
                "# made\n"
            );
            assert!(branch(&made).is_some());
        }
        assert!(create(&made, "again").is_err() || !made.exists());

        let from = repo("clone-from");
        let to = base.join("cloned");
        let said = std::cell::RefCell::new(Vec::new());
        let stop = std::sync::atomic::AtomicBool::new(false);
        clone(
            &format!("file://{}", from.display()),
            &to,
            &|what, _| said.borrow_mut().push(what.to_owned()),
            &stop,
        )
        .unwrap();
        assert!(to.join("a.txt").is_file());
        assert!(!said.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&from);
    }

    #[test]
    fn a_branch_is_read_against_its_base_its_commits_and_what_each_changed() {
        let dir = repo("base");
        git(
            &dir,
            &["remote", "add", "origin", "git@github.com:owner/name.git"],
            None,
        )
        .unwrap();
        assert_eq!(
            remote(&dir),
            Some(("github.com".into(), "owner/name".into()))
        );
        assert_eq!(default_branch(&dir).as_deref(), Some("main"));

        branch_off(&dir, "ghost/work").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        git(&dir, &["add", "b.txt"], None).unwrap();
        git(&dir, &["commit", "-q", "-m", "second"], None).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();

        let mut whole: Vec<String> = against_base(&dir, None, false)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        whole.sort();
        assert_eq!(whole, ["a.txt", "b.txt"]);
        let commits = log(&dir, 5);
        assert_eq!(commits[0].1, "second");
        let changed = commit_diff(&dir, &commits[0].0, false).unwrap();
        assert_eq!(changed[0].path, "b.txt");
        assert_eq!(
            commit_diff(&dir, &commits[1].0, false).unwrap()[0].path,
            "a.txt"
        );
        assert_eq!(branches(&dir), ["ghost/work", "main"]);

        rename_branch(&dir, "ghost/better").unwrap();
        assert_eq!(branch(&dir).as_deref(), Some("ghost/better"));

        checkpoint(&dir, "s9/0-before").unwrap();
        drop_checkpoints(&dir, "s9");
        assert_eq!(git(&dir, &["for-each-ref", REFS], None).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_committed_is_read_with_new_files_and_no_ref_or_index_touched() {
        let dir = repo("uncommitted");
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let files = uncommitted(&dir, false).unwrap();
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
        let files = diff(&dir, &before, &c, false).unwrap();
        assert_eq!(files.len(), 2);
        let a = files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!((a.added, a.gone, a.status), (1, 1, Status::Modified));
        assert!(a.patch.starts_with("diff --git a/a.txt b/a.txt\n"));
        assert!(a.patch.contains("\n one\n-two\n+2\n three\n"));
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
    fn undo_in_a_subfolder_leaves_the_rest_of_the_repository_alone() {
        let top = repo("subfolder");
        std::fs::create_dir_all(top.join("app")).unwrap();
        std::fs::create_dir_all(top.join("lib")).unwrap();
        std::fs::write(top.join("app/x"), "x\n").unwrap();
        std::fs::write(top.join("lib/o"), "o\n").unwrap();
        git(&top, &["add", "-A"], None).unwrap();
        git(&top, &["commit", "-q", "-m", "second"], None).unwrap();
        let app = top.join("app");
        let start = checkpoint(&app, "s1/1-before").unwrap();
        std::fs::write(top.join("lib/o"), "mine\n").unwrap();
        std::fs::write(top.join("lib/new"), "new\n").unwrap();
        git(&top, &["add", "lib/new"], None).unwrap();
        git(&top, &["commit", "-q", "-m", "third"], None).unwrap();
        std::fs::write(app.join("x"), "changed\n").unwrap();
        std::fs::write(app.join("tab\there"), "new\n").unwrap();
        restore(&app, &start, "s1/undo").unwrap();
        assert_eq!(std::fs::read_to_string(app.join("x")).unwrap(), "x\n");
        assert!(!app.join("tab\there").exists());
        assert_eq!(
            std::fs::read_to_string(top.join("lib/o")).unwrap(),
            "mine\n"
        );
        assert!(top.join("lib/new").exists());
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_checkpoint_holds_the_folder_as_on_disk_and_the_rest_as_committed_whatever_is_staged() {
        let top = repo("staged");
        std::fs::create_dir_all(top.join("app")).unwrap();
        std::fs::create_dir_all(top.join("lib")).unwrap();
        std::fs::write(top.join("app/x"), "x\n").unwrap();
        std::fs::write(top.join("lib/o"), "o\n").unwrap();
        git(&top, &["add", "-A"], None).unwrap();
        git(&top, &["commit", "-q", "-m", "second"], None).unwrap();
        std::fs::write(top.join("lib/o"), "staged\n").unwrap();
        std::fs::write(top.join("lib/s"), "new\n").unwrap();
        // Staged, then changed again at once, to the same size: only its
        // contents tell the two apart.
        std::fs::write(top.join("app/x"), "stg\n").unwrap();
        git(&top, &["add", "-A"], None).unwrap();
        std::fs::write(top.join("app/x"), "dsk\n").unwrap();
        let c = checkpoint(&top.join("app"), "s1/1-before").unwrap();
        let show = |p: &str| git(&top, &["show", &format!("{c}:{p}")], None);
        assert_eq!(show("app/x").unwrap(), "dsk");
        assert_eq!(show("lib/o").unwrap(), "o");
        assert!(show("lib/s").is_err());
        assert_eq!(
            git(&top, &["diff", "--cached", "--name-only"], None).unwrap(),
            "app/x\nlib/o\nlib/s"
        );
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_worktree_gets_a_branch_of_its_own_and_a_second_one_a_number() {
        let dir = repo("worktree");
        let home = dir.join(".home");
        let a = worktree(&dir, "Retry S3 uploads!", &home, "ghost/", "HEAD").unwrap();
        let b = worktree(&dir, "Retry S3 uploads!", &home, "ghost/", "HEAD").unwrap();
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

/// Which changes the right column shows: one turn's, all of the
/// session's, the branch's against where it left the main one, one
/// commit's, or the edits of one call (by its place in the thread).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    Turn(usize),
    All,
    Branch,
    Commit(String),
    Call(usize),
}

/// What git says where a session works, read while it is on screen, and
/// what you chose of it for the next commit.
#[derive(Debug, Clone)]
pub struct View {
    /// The changes in a scope git was asked for (the branch, a commit).
    pub scoped: Option<(Scope, Vec<FileDiff>)>,
    /// The recent commits: id, subject, when.
    pub commits: Vec<(String, String, String)>,
    /// The local branches.
    pub branches: Vec<String>,
    /// The pull request of its branch.
    pub pr: Option<Pr>,
    /// The step a git action is at.
    pub step: Option<String>,
    /// How far its branch is behind its upstream, and whether the
    /// repository has an origin.
    pub upstream: (usize, bool),
    /// The branch its branch is compared with; none, the main one.
    pub compare_to: Option<String>,
    /// The files (as git names them) you left out of the next commit.
    pub left_out: std::collections::BTreeSet<String>,
    /// The agent was asked to write a commit message: its answer goes in
    /// the commit form.
    pub writing_message: bool,
}

impl Default for View {
    /// Nothing read yet: taken to have an origin until git says otherwise.
    fn default() -> Self {
        Self {
            scoped: None,
            commits: Vec::new(),
            branches: Vec::new(),
            pr: None,
            step: None,
            upstream: (0, true),
            compare_to: None,
            left_out: std::collections::BTreeSet::new(),
            writing_message: false,
        }
    }
}
