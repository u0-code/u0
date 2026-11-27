//! What the desk does with git around an agent's work: a worktree of its
//! own for a session, a checkpoint of the files before and after each
//! turn, the diff between two checkpoints, putting the files back as a
//! checkpoint had them, and committing, pushing and opening a pull request.
//!
//! Everything is the `git` (and `gh`) on this machine, run in the session's
//! folder. A checkpoint never touches your index or your branch: the files
//! are staged into an index of the desk's own (`GIT_INDEX_FILE`), written as
//! a commit no branch points at, and kept under `refs/u0/checkpoints/`.
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
const REFS: &str = "refs/u0/checkpoints";

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
        // A checkpoint is the desk's, not yours: it is signed by u0 and
        // needs nothing from your git configuration.
        .env("GIT_AUTHOR_NAME", "u0")
        .env("GIT_AUTHOR_EMAIL", "u0@localhost")
        .env("GIT_COMMITTER_NAME", "u0")
        .env("GIT_COMMITTER_EMAIL", "u0@localhost");
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

/// The repositories of the project `project`: its own, or each of a
/// folder of them (`repos`).
pub fn project_repos(project: &Path) -> Vec<PathBuf> {
    match root(project) {
        Some(_) => vec![project.to_path_buf()],
        None => repos(project),
    }
}

/// The repositories a folder that is not in one holds, as t3code finds
/// them: each folder in it that is a repository's top, and each in a
/// folder of it that is not one. A submodule is not counted, nor a hidden
/// folder, nor a worktree of a repository the folder holds too (so a
/// session's folder of worktrees, `spanning`, holds each of them); at most
/// 128, by name.
pub fn repos(folder: &Path) -> Vec<PathBuf> {
    fn folders(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        found.sort();
        found
    }
    let mut found = Vec::new();
    for child in folders(folder) {
        match stands(&child) {
            Some(top) => found.push((child, top)),
            None => found.extend(
                (folders(&child).into_iter()).filter_map(|p| stands(&p).map(|top| (p, top))),
            ),
        }
    }
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mains: Vec<PathBuf> = (found.iter())
        .filter(|(_, top)| matches!(top, Top::Own))
        .map(|(p, _)| real(p))
        .collect();
    found.retain(|(_, top)| !matches!(top, Top::Of(repo) if mains.contains(&real(repo))));
    found.truncate(128);
    found.into_iter().map(|(p, _)| p).collect()
}

/// The top of a repository: its own, or a worktree of the one whose top
/// it names.
enum Top {
    Own,
    Of(PathBuf),
}

/// Whether `dir` is the top of a repository, and how. A `.git` folder is a repository's; a `.git` file (a worktree's, a
/// submodule's, one made with `--separate-git-dir`) is read by git, as it
/// may name its repository relatively (`worktree.useRelativePaths`) or
/// anywhere. A submodule is no top.
fn stands(dir: &Path) -> Option<Top> {
    let dot = dir.join(".git");
    if dot.is_dir() {
        return Some(Top::Own);
    }
    if !dot.is_file() {
        return None;
    }
    let ask = [
        "rev-parse",
        "--path-format=absolute",
        "--show-superproject-working-tree",
        "--git-common-dir",
        "--git-dir",
    ];
    let said = git(dir, &ask, None).ok()?;
    // The superproject is said first, only for a submodule.
    match said.lines().collect::<Vec<_>>()[..] {
        [common, own] if common == own => Some(Top::Own),
        [common, _] => Some(Top::Of(Path::new(common).parent()?.to_path_buf())),
        _ => None,
    }
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
    /// The repository it was made from; for a folder of them, the folder.
    pub repo: PathBuf,
    /// Where it is.
    pub path: PathBuf,
    /// The branch it is on, made for it.
    pub branch: String,
    /// Made from a folder of repositories: the worktree of each, laid out
    /// in `path` as they are in the folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<Worktree>,
}

/// The worktrees of the repository at `repo` that live in one of `under`
/// (u0's own), with the branch each is on.
pub fn worktrees(repo: &Path, under: &[PathBuf]) -> Vec<Worktree> {
    let Ok(out) = git(repo, &["worktree", "list", "--porcelain"], None) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut path: Option<PathBuf> = None;
    for line in out.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(p) = path
                .take()
                .filter(|p| under.iter().any(|u| p.starts_with(u)))
            {
                found.push(Worktree {
                    repo: repo.to_path_buf(),
                    path: p,
                    branch: b.to_owned(),
                    repos: Vec::new(),
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

/// `text` as a short name, for a branch, a file or an id: its first six
/// words in lower-case ASCII, joined by `-`, other scripts spelt in Latin
/// letters (`slug`'s deunicode); "session" where it has none.
pub fn slug(text: &str) -> String {
    let words = slug::slugify(text);
    let mut out = words.split('-').take(6).collect::<Vec<_>>().join("-");
    out.truncate(40);
    let out = out.trim_end_matches('-');
    if out.is_empty() { "session" } else { out }.to_owned()
}

/// Make a worktree of the repository at `dir` for a session about `about`,
/// under `under/<repo>/`, on a new branch `<prefix><slug>` from `from`. A
/// name already taken gets a number. With `sparse`, git's sparse-checkout
/// patterns (non-cone, a line each), only the files they match are ever
/// written in it. A folder that is not in a repository but holds some gets
/// one of each (`spanning`), each from its own fresh base.
pub fn worktree(
    dir: &Path,
    about: &str,
    under: &Path,
    prefix: &str,
    from: &str,
    sparse: Option<&str>,
) -> Result<Worktree, String> {
    let Some(repo) = root(dir) else {
        return spanning(dir, about, under, prefix);
    };
    let name = repo
        .file_name()
        .map_or_else(|| "repo".to_owned(), |n| n.to_string_lossy().into_owned());
    let (path, branch) = free(about, &under.join(name), prefix, |b| has_branch(&repo, b))?;
    let at = path.to_string_lossy().into_owned();
    let wt = Worktree {
        repo,
        path,
        branch,
        repos: Vec::new(),
    };
    let add = |more: &[&str]| {
        let mut args = vec!["worktree", "add"];
        args.extend(more);
        args.extend(["-b", &wt.branch, &at, from]);
        git(&wt.repo, &args, None)
    };
    let Some(patterns) = sparse else {
        return add(&[]).map(|_| wt);
    };
    add(&["--no-checkout"])?;
    let mut set = vec!["sparse-checkout", "set", "--no-cone"];
    set.extend(
        patterns
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#')),
    );
    match git(&wt.path, &set, None).and_then(|_| git(&wt.path, &["read-tree", "-mu", "HEAD"], None))
    {
        Ok(_) => Ok(wt),
        Err(e) => {
            let _ = git(&wt.repo, &["worktree", "remove", "--force", &at], None);
            Err(e)
        }
    }
}

/// The first of `<slug>`, `<slug>-2`, … of `about` whose folder in `at`
/// is free and whose branch `<prefix>…` is not `taken`.
fn free(
    about: &str,
    at: &Path,
    prefix: &str,
    taken: impl Fn(&str) -> bool,
) -> Result<(PathBuf, String), String> {
    let base = slug(about);
    (1..100)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .map(|s| (at.join(&s), format!("{prefix}{s}")))
        .find(|(path, branch)| !path.exists() && !taken(branch))
        .ok_or_else(|| "no free name for a worktree".into())
}

/// Whether the repository at `repo` has the branch `branch`.
fn has_branch(repo: &Path, branch: &str) -> bool {
    let name = format!("refs/heads/{branch}");
    git(repo, &["rev-parse", "--verify", "--quiet", &name], None).is_ok()
}

/// A worktree of each repository the folder `dir` holds, for a session
/// about `about`, as t3code makes one: in one folder `under/<folder>/<slug>`
/// laid out as `dir` is, each on the new branch `<prefix><slug>` from its
/// remote's default branch; the rest of `dir` linked there, not copied.
fn spanning(dir: &Path, about: &str, under: &Path, prefix: &str) -> Result<Worktree, String> {
    let held = repos(dir);
    if held.is_empty() {
        return Err("the folder is not in a git repository, nor holds one".into());
    }
    let name = dir
        .file_name()
        .map_or_else(|| "folder".to_owned(), |n| n.to_string_lossy().into_owned());
    let (path, branch) = free(about, &under.join(name), prefix, |b| {
        held.iter().any(|r| has_branch(r, b))
    })?;
    let wt = Worktree {
        repos: (held.iter())
            .map(|r| Worktree {
                repo: r.clone(),
                path: path.join(r.strip_prefix(dir).unwrap_or(r)),
                branch: branch.clone(),
                repos: Vec::new(),
            })
            .collect(),
        repo: dir.to_path_buf(),
        path,
        branch,
    };
    match lay_out(&wt, true) {
        Ok(()) => Ok(wt),
        Err(why) => {
            let _ = remove_worktree(&wt);
            Err(why)
        }
    }
}

/// Lay the worktree `wt` of a folder of repositories out: each
/// repository's checked out (on its new branch from its fresh base where
/// `new`, else on its branch again), and the rest of the folder linked.
fn lay_out(wt: &Worktree, new: bool) -> Result<(), String> {
    for each in &wt.repos {
        let at = each.path.to_string_lossy();
        if new {
            let from = fresh_base(&each.repo);
            let args = ["worktree", "add", "-b", &each.branch, &at, &from];
            git(&each.repo, &args, None)?;
        } else {
            git(&each.repo, &["worktree", "add", &at, &each.branch], None)?;
        }
    }
    link(&wt.repo, &wt.path, &wt.repos)
}

/// Link each thing in `from` from the same place in `to`, but the
/// repositories of `repos` (checked out there) and the folders holding
/// them, whose things are linked in turn.
fn link(from: &Path, to: &Path, repos: &[Worktree]) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let (theirs, ours) = (entry.path(), to.join(entry.file_name()));
        if repos.iter().any(|r| r.repo == theirs) {
            continue;
        }
        if repos.iter().any(|r| r.repo.starts_with(&theirs)) {
            link(&theirs, &ours, repos)?;
        } else if ours.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(&theirs, &ours).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Take away the links and the empty folders left in `dir`, `dir` too.
fn unlink(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        match entry.file_type() {
            Ok(t) if t.is_symlink() => drop(std::fs::remove_file(entry.path())),
            Ok(t) if t.is_dir() => unlink(&entry.path()),
            _ => {}
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// The file `path` as the commit `rev` of the repository at `dir` has it.
pub fn show(dir: &Path, rev: &str, path: &str) -> Result<String, String> {
    git(dir, &["show", &format!("{rev}:{path}")], None)
}

/// Check out the submodules of the worktree `wt` (of each, for a folder of
/// repositories) as `how` says. What cannot be fetched is said, the
/// worktree kept.
pub fn submodules(wt: &Worktree, how: crate::settings::Submodules) {
    use crate::settings::Submodules;
    if !wt.repos.is_empty() {
        return wt.repos.iter().for_each(|w| submodules(w, how));
    }
    let dir = &wt.path;
    let args: &[&str] = match how {
        _ if !dir.join(".gitmodules").is_file() => return,
        Submodules::Skip => return,
        Submodules::Top => &["submodule", "update", "--init"],
        Submodules::Recursive => &["submodule", "update", "--init", "--recursive"],
    };
    if let Err(e) = git(dir, args, None) {
        tracing::warn!(
            "the submodules of {} were not checked out: {e}",
            dir.display()
        );
    }
}

/// Make a worktree of the repository at `dir` on the branch of the pull
/// request `number`, as `gh pr checkout` sets it up (a fork's included,
/// so that a push goes to the pull request), under
/// `under/<repo>/pr-<number>`.
pub fn pr_worktree(dir: &Path, number: u64, under: &Path) -> Result<Worktree, String> {
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
            under.join(&name).join(slug)
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
            repos: Vec::new(),
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
/// refused, and git says so; its branch is kept. One of a folder of
/// repositories goes each, then its links; what is not a link stays.
pub fn remove_worktree(wt: &Worktree) -> Result<(), String> {
    if wt.repos.is_empty() {
        let at = wt.path.to_string_lossy().into_owned();
        return git(&wt.repo, &["worktree", "remove", &at], None).map(|_| ());
    }
    let mut done = Ok(());
    for each in wt.repos.iter().filter(|w| w.path.exists()) {
        done = done.and(remove_worktree(each));
    }
    unlink(&wt.path);
    done
}

/// Check the worktree `wt` out again where it was, on its branch, after it
/// was taken away.
pub fn restore_worktree(wt: &Worktree) -> Result<(), String> {
    if !wt.repos.is_empty() {
        return lay_out(wt, false);
    }
    let at = wt.path.to_string_lossy().into_owned();
    git(&wt.repo, &["worktree", "add", &at, &wt.branch], None).map(|_| ())
}

/// A private index file, its own for each call.
fn scratch_index(dir: &Path) -> Result<PathBuf, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let git_dir = git(dir, &["rev-parse", "--absolute-git-dir"], None)?;
    Ok(PathBuf::from(git_dir).join(format!(
        "u0-index-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

/// The files of `dir` as they are now, ignored files left out, written as a
/// commit kept under `refs/u0/checkpoints/<name>`; its id. A folder of
/// repositories is checkpointed in each: a line `<commit> <its place in the
/// folder>` each (`points`).
pub fn checkpoint(dir: &Path, name: &str) -> Result<String, String> {
    if root(dir).is_none() {
        let held = repos(dir);
        if held.is_empty() {
            return Err("the folder holds no git repository".into());
        }
        let each = held.iter().map(|repo| {
            let place = repo.strip_prefix(dir).unwrap_or(repo).display();
            checkpoint(repo, name).map(|commit| format!("{commit} {place}"))
        });
        return each.collect::<Result<Vec<_>, _>>().map(|l| l.join("\n"));
    }
    let commit = snapshot(dir, name)?;
    git(
        dir,
        &["update-ref", &format!("{REFS}/{name}"), &commit],
        None,
    )?;
    Ok(commit)
}

/// What is not committed in `dir`, untracked files with it, against the
/// commit it is on, or against the checkpoint `since`: read through a
/// commit no ref keeps, so your index is left alone.
pub fn uncommitted(
    dir: &Path,
    since: Option<&str>,
    no_spacing: bool,
) -> Result<Vec<FileDiff>, String> {
    let now = snapshot(dir, "uncommitted")?;
    let base = match since {
        Some(c) => c.to_owned(),
        None => git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], None)
            .unwrap_or_else(|_| EMPTY_TREE.to_owned()),
    };
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
/// A folder of repositories is put back in each.
pub fn restore(dir: &Path, commit: &str, name: &str) -> Result<String, String> {
    if root(dir).is_none() {
        let each = points(commit).map(|(commit, place)| {
            restore(&dir.join(place), commit, name).map(|now| format!("{now} {place}"))
        });
        return each.collect::<Result<Vec<_>, _>>().map(|l| l.join("\n"));
    }
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

/// One file's changes: what lists name and count of them, and its patch
/// as git writes it, which the desk's diff view reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileDiff {
    /// Its path from the top of the repository, as it is after.
    pub path: String,
    pub added: usize,
    pub gone: usize,
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
        added,
        gone,
        patch,
    }
}

/// What changed in `dir` from the checkpoint `from` to `to`; with
/// `no_spacing` (as the settings say), a change only in spacing is left out.
/// In a folder of repositories, each one's, its paths from the folder.
pub fn diff(dir: &Path, from: &str, to: &str, no_spacing: bool) -> Result<Vec<FileDiff>, String> {
    if root(dir).is_some() {
        return diff_in(dir, from, to, no_spacing, "");
    }
    let mut all = Vec::new();
    for (commit, place) in points(to) {
        let before = points(from).find(|(_, p)| *p == place);
        let before = before.map_or(EMPTY_TREE, |(c, _)| c);
        all.extend(diff_in(
            &dir.join(place),
            before,
            commit,
            no_spacing,
            place,
        )?);
    }
    Ok(all)
}

/// Each repository's commit in a checkpoint of a folder of them, with its
/// place in the folder.
fn points(point: &str) -> impl Iterator<Item = (&str, &str)> {
    point.lines().filter_map(|l| l.split_once(' '))
}

/// What changed in the repository `dir` from `from` to `to`, its paths
/// led by `place` (its place in a folder of repositories) where there is
/// one.
fn diff_in(
    dir: &Path,
    from: &str,
    to: &str,
    no_spacing: bool,
    place: &str,
) -> Result<Vec<FileDiff>, String> {
    let prefixes = [
        format!("--src-prefix=a/{place}/"),
        format!("--dst-prefix=b/{place}/"),
    ];
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-M", "-U3"];
    if !place.is_empty() {
        args.extend(prefixes.iter().map(String::as_str));
    }
    if no_spacing {
        args.push("-w");
    }
    args.extend([from, to]);
    let patch = git(dir, &args, None)?;
    args.splice(1..1, ["--numstat", "-z"]);
    let counts = git(dir, &args, None)?;
    let mut files = cut(&counts, &patch);
    if !place.is_empty() {
        for f in &mut files {
            f.path = format!("{place}/{}", f.path);
        }
    }
    Ok(files)
}

/// The files of a diff: named and counted by git itself (`--numstat -z`,
/// `counts`, whose names are never quoted), each with its part of
/// `text`, the same diff written out, its files in the same order.
fn cut(counts: &str, text: &str) -> Vec<FileDiff> {
    let mut parts: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with("diff --git ") || parts.is_empty() {
            parts.push(String::new());
        }
        let part = parts.last_mut().expect("one at least");
        part.push_str(line);
        part.push('\n');
    }
    let mut parts = parts.into_iter();
    let mut fields = counts.split('\0');
    let mut out = Vec::new();
    while let Some(head) = fields.next().filter(|h| !h.is_empty()) {
        let mut head = head.splitn(3, '\t');
        // A binary file is counted "-": nought lines.
        let mut count = || head.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let (added, gone) = (count(), count());
        let mut path = head.next().unwrap_or_default();
        // A rename's names follow, before and after.
        if path.is_empty() {
            fields.next();
            path = fields.next().unwrap_or_default();
        }
        out.push(FileDiff {
            path: path.to_owned(),
            added,
            gone,
            patch: parts.next().unwrap_or_default(),
        });
    }
    out
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
    if host.contains("gitlab") && which::which("glab").is_ok() {
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

/// One commit on a branch: its short id, subject, author, relative age,
/// ref names pointing at it (`HEAD`, `origin/...`, `main`), line counts,
/// and whether it has not been pushed yet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Commit {
    pub id: String,
    pub subject: String,
    pub author: String,
    pub ago: String,
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub added: usize,
    #[serde(default)]
    pub gone: usize,
    #[serde(default)]
    pub unpushed: bool,
}

/// A git stash entry: its index (`stash@{index}`), the branch it was made
/// on, its message, how many files it touches, and how long ago it was made.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stash {
    pub index: usize,
    pub branch: String,
    pub message: String,
    pub files: usize,
    pub ago: String,
}

/// A summary of one repository inside a multi-repository folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepoSummary {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub files: usize,
    pub added: usize,
    pub gone: usize,
    pub behind: usize,
}

/// The short ids of commits on `HEAD` not yet on its upstream (or base).
fn unpushed_ids(dir: &Path) -> std::collections::HashSet<String> {
    let range = if git(dir, &["rev-parse", "--verify", "--quiet", "@{u}"], None).is_ok() {
        Some("@{u}..HEAD".to_owned())
    } else {
        default_branch(dir).map(|base| format!("{base}..HEAD"))
    };
    let Some(range) = range else {
        return std::collections::HashSet::new();
    };
    git(dir, &["rev-list", "--abbrev-commit", &range], None)
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The last `n` commits, with author, decorations, line stats, and push state.
pub fn log(dir: &Path, n: usize) -> Vec<Commit> {
    let unpushed = unpushed_ids(dir);
    git(
        dir,
        &[
            "log",
            &format!("-{n}"),
            "--numstat",
            "--format=%x1e%h%x09%s%x09%an%x09%ct%x09%D",
        ],
        None,
    )
    .map(|t| {
        // The time is said as the desk says times, not in git's language.
        let now = std::time::SystemTime::now();
        t.split('\x1e')
            .filter(|chunk| !chunk.trim().is_empty())
            .filter_map(|chunk| {
                let mut lines = chunk.lines();
                let header = lines.next()?;
                let mut f = header.splitn(5, '\t');
                let (id, subject, author) = (f.next()?, f.next()?, f.next()?);
                let secs: u64 = f.next()?.trim().parse().ok()?;
                let raw_refs = f.next().unwrap_or_default();
                let refs = raw_refs
                    .split(", ")
                    .map(str::trim)
                    .filter(|r| !r.is_empty() && *r != "origin/HEAD")
                    .map(|r| {
                        if r.starts_with("HEAD -> ") {
                            "HEAD".to_owned()
                        } else {
                            r.to_owned()
                        }
                    })
                    .collect();
                let (mut added, mut gone) = (0usize, 0usize);
                for stat in lines.filter(|l| !l.trim().is_empty()) {
                    let mut cols = stat.splitn(3, '\t');
                    added += cols
                        .next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    gone += cols
                        .next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                }
                let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
                Some(Commit {
                    unpushed: unpushed.contains(id),
                    id: id.to_owned(),
                    subject: subject.to_owned(),
                    author: author.to_owned(),
                    ago: crate::when::ago(at, now),
                    refs,
                    added,
                    gone,
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The stashes kept in the repository at `dir`, newest first.
pub fn stashes(dir: &Path) -> Vec<Stash> {
    let Ok(text) = git(dir, &["stash", "list", "--format=%gd%x09%gs%x09%ct"], None) else {
        return Vec::new();
    };
    let now = std::time::SystemTime::now();
    text.lines()
        .filter_map(|line| {
            let mut f = line.splitn(3, '\t');
            let gd = f.next()?;
            let gs = f.next()?;
            let secs: u64 = f.next()?.trim().parse().ok()?;
            let index: usize = gd
                .strip_prefix("stash@{")?
                .strip_suffix('}')?
                .parse()
                .ok()?;
            let (branch, message) = gs
                .strip_prefix("WIP on ")
                .or_else(|| gs.strip_prefix("On "))
                .and_then(|rest| rest.split_once(": "))
                .map_or_else(
                    || (String::new(), gs.to_owned()),
                    |(b, m)| (b.to_owned(), m.to_owned()),
                );
            let files = git(dir, &["stash", "show", "--name-only", gd], None)
                .map_or(0, |names| names.lines().filter(|l| !l.is_empty()).count());
            let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            Some(Stash {
                index,
                branch,
                message,
                files,
                ago: crate::when::ago(at, now),
            })
        })
        .collect()
}

/// Save uncommitted changes (including untracked files) in `dir` onto the stash stack.
pub fn stash_push(dir: &Path, message: Option<&str>) -> Result<String, String> {
    let mut args = vec!["stash", "push", "-u"];
    if let Some(m) = message.map(str::trim).filter(|m| !m.is_empty()) {
        args.extend(["-m", m]);
    }
    yours(dir, "git", &args)
}

/// Apply (or pop when `pop` is true) `stash@{index}` in `dir`.
pub fn stash_apply(dir: &Path, index: usize, pop: bool) -> Result<String, String> {
    let verb = if pop { "pop" } else { "apply" };
    yours(dir, "git", &["stash", verb, &format!("stash@{{{index}}}")])
}

/// Drop `stash@{index}` from the stash list in `dir`.
pub fn stash_drop(dir: &Path, index: usize) -> Result<String, String> {
    yours(
        dir,
        "git",
        &["stash", "drop", &format!("stash@{{{index}}}")],
    )
}

/// Per-repository branch, uncommitted change counts, and upstream behind count.
pub fn repo_summaries(repos: &[PathBuf]) -> Vec<RepoSummary> {
    repos
        .iter()
        .map(|path| {
            let diffs = uncommitted(path, None, false).unwrap_or_default();
            let added = diffs.iter().map(|d| d.added).sum();
            let gone = diffs.iter().map(|d| d.gone).sum();
            RepoSummary {
                branch: branch(path),
                files: diffs.len(),
                added,
                gone,
                behind: upstream(path).1,
                path: path.clone(),
            }
        })
        .collect()
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

/// Take away the checkpoints u0 kept for the session `uid` in the
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

/// How far the branch `dir` is on is ahead of and behind its upstream
/// (ahead falls back to the repository's default branch before first push),
/// and whether the repository has a remote called origin.
pub fn upstream(dir: &Path) -> (usize, usize, bool) {
    let count = |range: &str| {
        git(dir, &["rev-list", "--count", range], None)
            .ok()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0)
    };
    let has_tracking = git(dir, &["rev-parse", "--verify", "--quiet", "@{u}"], None).is_ok();
    let behind = if has_tracking { count("HEAD..@{u}") } else { 0 };
    let ahead = if has_tracking {
        count("@{u}..HEAD")
    } else if let Some(base) = default_branch(dir) {
        count(&format!("{base}..HEAD"))
    } else {
        0
    };
    let origin = git(dir, &["remote", "get-url", "origin"], None).is_ok();
    (ahead, behind, origin)
}

/// Bring in what the upstream has, only where it moves straight on.
pub fn pull(dir: &Path) -> Result<String, String> {
    yours(dir, "git", &["pull", "--ff-only", "-q"])
}

/// Fetch what `origin` has for the repository at `dir`; with `pull`, and
/// the folder on its default branch with nothing changed, move it on to
/// where `origin` has it, only where it moves straight on (t3code's auto
/// pull). Nothing to fetch from is no failure.
pub fn refresh(dir: &Path, pull: bool) -> Result<(), String> {
    if git(dir, &["remote", "get-url", "origin"], None).is_err() {
        return Ok(());
    }
    git(dir, &["fetch", "--quiet", "origin"], None)?;
    let on_default = default_branch(dir)
        .is_some_and(|d| branch(dir).is_some_and(|b| d.strip_prefix("origin/").unwrap_or(&d) == b));
    let clean = git(dir, &["status", "--porcelain"], None).is_ok_and(|s| s.is_empty());
    if pull && on_default && clean {
        git(dir, &["merge", "--ff-only", "--quiet", "@{upstream}"], None)?;
    }
    Ok(())
}

/// Make the folder `dir` a repository on main, all it holds committed.
pub fn init_here(dir: &Path) -> Result<String, String> {
    yours(dir, "git", &["init", "-q", "-b", "main"])?;
    yours(dir, "git", &["add", "-A"])?;
    yours(dir, "git", &["commit", "-q", "-m", "First commit"])
}

/// Put the repository at `dir` on GitHub, private, as gh does, and push.
pub fn publish(dir: &Path, public: bool) -> Result<String, String> {
    let seen = if public { "--public" } else { "--private" };
    yours(
        dir,
        "gh",
        &["repo", "create", seen, "--source", ".", "--push"],
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
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pr {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// open, merged, closed; draft.
    pub state: String,
    pub draft: bool,
    /// It cannot be merged as it is: it conflicts with its base.
    pub conflicts: bool,
    /// Its base has moved on since it branched: it can be brought up to
    /// date (`update_branch`).
    pub behind: bool,
    /// Its checks: how many passed, failed, and are still going.
    pub passed: usize,
    pub failed: usize,
    pub going: usize,
    /// approved, changes requested, review required; empty for none.
    pub review: String,
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub checks: Vec<crate::github::Check>,
    /// Latest reviewer note: `(reviewer, when, body)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_note: Option<(String, String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
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
            "number,title,url,state,isDraft,headRefName,baseRefName,reviewDecision,statusCheckRollup,mergeable,mergeStateStatus,reviews,mergedAt",
        ],
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&out).ok()?;
    parse_pr(&v)
}

fn parse_pr(v: &serde_json::Value) -> Option<Pr> {
    let raw_checks = v["statusCheckRollup"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let count = |f: &dyn Fn(&str) -> bool| {
        raw_checks
            .iter()
            .filter(|c| {
                let word = c["conclusion"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| c["state"].as_str())
                    .or_else(|| c["status"].as_str())
                    .unwrap_or("");
                f(&word.to_ascii_uppercase())
            })
            .count()
    };
    let checks = raw_checks
        .iter()
        .map(|c| {
            let name = c["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| c["context"].as_str())
                .unwrap_or_default()
                .to_owned();
            let state = c["conclusion"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| c["state"].as_str())
                .or_else(|| c["status"].as_str())
                .unwrap_or_default()
                .to_lowercase();
            let url = c["detailsUrl"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| c["targetUrl"].as_str())
                .unwrap_or_default()
                .to_owned();
            let duration = crate::github::check_duration(
                c["startedAt"].as_str().unwrap_or_default(),
                c["completedAt"].as_str().unwrap_or_default(),
            );
            crate::github::Check {
                name,
                state,
                url,
                duration,
            }
        })
        .collect();
    let review_note = v["reviews"]
        .as_array()
        .and_then(|reviews| {
            reviews
                .iter()
                .rev()
                .find(|r| !r["body"].as_str().unwrap_or_default().trim().is_empty())
        })
        .map(|r| {
            let who = r
                .pointer("/author/login")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let when = r["submittedAt"]
                .as_str()
                .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                .and_then(|ts| std::time::SystemTime::try_from(ts).ok())
                .map(|at| crate::when::ago(at, std::time::SystemTime::now()))
                .unwrap_or_default();
            let body = r["body"].as_str().unwrap_or_default().trim().to_owned();
            (who, when, body)
        });
    let merged_at = v["mergedAt"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Some(Pr {
        number: v["number"].as_u64()?,
        title: v["title"].as_str().unwrap_or_default().to_owned(),
        url: v["url"].as_str().unwrap_or_default().to_owned(),
        state: v["state"].as_str().unwrap_or_default().to_lowercase(),
        draft: v["isDraft"].as_bool().unwrap_or(false),
        conflicts: v["mergeable"].as_str() == Some("CONFLICTING"),
        behind: v["mergeStateStatus"].as_str() == Some("BEHIND"),
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
        head: v["headRefName"].as_str().unwrap_or_default().to_owned(),
        base: v["baseRefName"].as_str().unwrap_or_default().to_owned(),
        checks,
        review_note,
        merged_at,
    })
}

/// Bring the pull request of the branch `dir` is on up to date with its
/// base, on the host, as GitHub's Update branch does: a merge of the base
/// into it, or with `rebase` its commits put on the base again.
pub fn update_branch(dir: &Path, rebase: bool) -> Result<String, String> {
    let mut args = vec!["pr", "update-branch"];
    if rebase {
        args.push("--rebase");
    }
    yours(dir, "gh", &args)
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

/// How a pull request is merged.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Its commits squashed into one.
    #[default]
    Squash,
    /// With a merge commit.
    Merge,
    /// Its commits put on the base one by one.
    Rebase,
}

impl Method {
    /// What gh is told: `squash`, `merge` or `rebase`.
    pub fn gh(self) -> &'static str {
        match self {
            Self::Squash => "squash",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }
}

/// Merge the pull request of the branch `dir` is on, as `how` says, as gh
/// does; a squash's message with the agents' credits taken out where
/// `uncredit`.
pub fn merge_pr(dir: &Path, how: Method, uncredit: bool) -> Result<String, String> {
    let mut args = vec![
        "pr".to_owned(),
        "merge".to_owned(),
        format!("--{}", how.gh()),
    ];
    if uncredit && how == Method::Squash {
        args.extend(["--body".into(), crate::github::squash_message(dir, None)?]);
    }
    yours(
        dir,
        "gh",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

/// Run `program` (git, gh, glab) in `dir` with `args` as you, with your
/// git and gh configuration; its output, or what it said went wrong.
pub(crate) fn yours(dir: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(program);
    // A token you gave for the host whose CLI this is, over its own sign-in.
    if let Some((var, token)) = crate::forge::Host::token_for(program) {
        command.env(var, token);
    }
    let out = command
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

    #[test]
    fn a_folder_of_repositories_holds_those_one_or_two_levels_down_not_their_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let away = tempfile::tempdir().unwrap();
        for d in ["packages/web/.git", "packages/notes", ".hidden/.git"] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        let git = |at: &Path, args: &[&str]| {
            let named = ["-c", "user.name=u0", "-c", "user.email=u0@localhost"];
            let file = ["-c", "protocol.file.allow=always"];
            yours(at, "git", &[&named[..], &file[..], args].concat()).unwrap();
        };
        let repo = |at: &Path, more: &[&str]| {
            std::fs::create_dir_all(at).unwrap();
            git(at, &[&["init", "-q", "-b", "main"], more].concat());
            git(at, &["commit", "-q", "--allow-empty", "-m", "first"]);
        };
        let (api, other) = (dir.path().join("api"), away.path().join("other"));
        repo(&api, &[]);
        repo(&other, &[]);
        // A worktree of a repository held, named relatively (git 2.48's
        // `worktree.useRelativePaths`); one of a repository elsewhere.
        let api_wt = dir.path().join("api-wt").display().to_string();
        let relative = ["-c", "worktree.useRelativePaths=true"];
        git(
            &api,
            &[&relative[..], &["worktree", "add", "-q", &api_wt]].concat(),
        );
        let other_wt = dir.path().join("other-wt").display().to_string();
        git(&other, &["worktree", "add", "-q", &other_wt]);
        // A repository whose `.git` is a file naming its folder elsewhere.
        let kept = away.path().join("kept.git").display().to_string();
        repo(&dir.path().join("apart"), &["--separate-git-dir", &kept]);
        // A submodule's `.git` is a file, as a worktree's is.
        let other_url = other.display().to_string();
        git(&api, &["submodule", "add", "-q", &other_url, "sub"]);
        assert_eq!(
            repos(dir.path()),
            vec![
                dir.path().join("apart"),
                dir.path().join("api"),
                dir.path().join("other-wt"),
                dir.path().join("packages/web")
            ]
        );
    }

    #[test]
    fn a_title_in_any_script_makes_a_short_ascii_name() {
        assert_eq!(slug("Fix the login page!"), "fix-the-login-page");
        assert_eq!(slug("修复登录"), "xiu-fu-deng-lu");
        assert_eq!(slug("Ünïcode  café"), "unicode-cafe");
        assert_eq!(
            slug("one two three four five six seven"),
            "one-two-three-four-five-six"
        );
        assert_eq!(slug("!!"), "session");
    }

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
        let dir = std::env::temp_dir().join(format!("u0-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"], None).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git(&dir, &["add", "a.txt"], None).unwrap();
        git(&dir, &["commit", "-q", "-m", "first"], None).unwrap();
        dir
    }

    #[test]
    fn a_refresh_fetches_and_pulls_the_default_branch_only_when_asked() {
        let origin = repo("refresh-origin");
        let here = std::env::temp_dir().join(format!("u0-git-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&here);
        let at = here.to_string_lossy().into_owned();
        let from = origin.to_string_lossy().into_owned();
        git(&origin, &["clone", "-q", &from, &at], None).unwrap();
        std::fs::write(origin.join("b.txt"), "new\n").unwrap();
        git(&origin, &["add", "b.txt"], None).unwrap();
        git(&origin, &["commit", "-q", "-m", "second"], None).unwrap();
        refresh(&here, false).unwrap();
        assert!(!here.join("b.txt").exists());
        assert_eq!(upstream(&here).1, 1);
        refresh(&here, true).unwrap();
        assert!(here.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&here);
        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn a_folder_of_repositories_gets_a_worktree_of_each_laid_out_and_checkpointed_in_each() {
        let top = std::env::temp_dir().join(format!("u0-git-span-{}", std::process::id()));
        let (shop, under) = (top.join("shop"), top.join("worktrees"));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(shop.join("libs")).unwrap();
        for (made, place) in [(repo("span-api"), "api"), (repo("span-web"), "libs/web")] {
            std::fs::rename(made, shop.join(place)).unwrap();
        }
        std::fs::write(shop.join("README.md"), "the shop\n").unwrap();
        let wt = worktree(&shop, "Fix checkout", &under, "u0/", "HEAD", None).unwrap();
        assert_eq!(
            (wt.path.clone(), wt.branch.as_str()),
            (under.join("shop/fix-checkout"), "u0/fix-checkout")
        );
        let (api, web) = (wt.path.join("api"), wt.path.join("libs/web"));
        assert_eq!(branch(&api).as_deref(), Some("u0/fix-checkout"));
        assert_eq!(branch(&web).as_deref(), Some("u0/fix-checkout"));
        // What is not a repository is the folder's own, linked.
        assert!(
            wt.path
                .join("README.md")
                .symlink_metadata()
                .unwrap()
                .is_symlink()
        );
        // The folder of worktrees holds each of them, as the folder did.
        assert_eq!(repos(&wt.path), vec![api.clone(), web.clone()]);
        let before = checkpoint(&wt.path, "s/0-before").unwrap();
        std::fs::write(api.join("a.txt"), "one\n").unwrap();
        std::fs::write(web.join("new.txt"), "new\n").unwrap();
        let after = checkpoint(&wt.path, "s/0-after").unwrap();
        let mut changed: Vec<(String, usize, usize)> = diff(&wt.path, &before, &after, false)
            .unwrap()
            .into_iter()
            .map(|f| (f.path, f.added, f.gone))
            .collect();
        changed.sort();
        assert_eq!(
            changed,
            [
                ("api/a.txt".to_owned(), 0, 2),
                ("libs/web/new.txt".to_owned(), 1, 0)
            ]
        );
        restore(&wt.path, &before, "s/0-undo").unwrap();
        assert!(!web.join("new.txt").exists());
        assert_eq!(
            std::fs::read_to_string(api.join("a.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        remove_worktree(&wt).unwrap();
        assert!(!wt.path.exists());
        assert!(shop.join("README.md").exists());
        restore_worktree(&wt).unwrap();
        assert!(api.join("a.txt").exists() && wt.path.join("README.md").exists());
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_name_git_quotes_reads_as_it_is() {
        let dir = repo("quoted");
        let before = git(&dir, &["rev-parse", "HEAD"], None).unwrap();
        std::fs::write(dir.join("报告 \"一\".md"), "x\n").unwrap();
        std::fs::write(dir.join("tab\there.md"), "y\n").unwrap();
        let c = checkpoint(&dir, "s1/1-after").unwrap();
        let mut names: Vec<String> = diff(&dir, &before, &c, false)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        names.sort();
        assert_eq!(names, ["tab\there.md", "报告 \"一\".md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_made_a_repository_has_all_it_held_committed_and_no_upstream() {
        let dir = std::env::temp_dir().join(format!("u0-git-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "hi\n").unwrap();
        init_here(&dir).unwrap();
        assert_eq!(git(&dir, &["status", "--porcelain"], None).unwrap(), "");
        assert_eq!(upstream(&dir), (0, 0, false));
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
        let base = std::env::temp_dir().join(format!("u0-new-{}", std::process::id()));
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

        branch_off(&dir, "u0/work").unwrap();
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
        assert_eq!(commits[0].subject, "second");
        assert_eq!(
            (commits[0].added, commits[0].gone, commits[0].unpushed),
            (1, 0, true)
        );
        assert!(commits[0].refs.iter().any(|r| r == "HEAD"));
        assert!(commits[1].refs.iter().any(|r| r == "main"));
        let changed = commit_diff(&dir, &commits[0].id, false).unwrap();
        assert_eq!(changed[0].path, "b.txt");
        assert_eq!(
            commit_diff(&dir, &commits[1].id, false).unwrap()[0].path,
            "a.txt"
        );
        assert_eq!(branches(&dir), ["u0/work", "main"]);
        assert_eq!(upstream(&dir), (1, 0, true));

        rename_branch(&dir, "u0/better").unwrap();
        assert_eq!(branch(&dir).as_deref(), Some("u0/better"));

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
        let files = uncommitted(&dir, None, false).unwrap();
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
    fn what_changed_since_a_checkpoint_leaves_out_what_was_there_before_it() {
        let dir = repo("since");
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        let first = checkpoint(&dir, "s2/0-before").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let files = uncommitted(&dir, Some(&first), false).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["b.txt"]);
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
        assert_eq!((a.added, a.gone), (1, 1));
        assert!(a.patch.starts_with("diff --git a/a.txt b/a.txt\n"));
        assert!(a.patch.contains("\n one\n-two\n+2\n three\n"));
        let b = files.iter().find(|f| f.path == "b.txt").unwrap();
        assert!(b.patch.contains("\nnew file mode "));
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
        let a = worktree(&dir, "Retry S3 uploads!", &home, "u0/", "HEAD", None).unwrap();
        let b = worktree(&dir, "Retry S3 uploads!", &home, "u0/", "HEAD", None).unwrap();
        assert_eq!(a.branch, "u0/retry-s3-uploads");
        assert_eq!(b.branch, "u0/retry-s3-uploads-2");
        assert!(a.path.join("a.txt").is_file());
        assert_eq!(branch(&a.path).as_deref(), Some("u0/retry-s3-uploads"));
        remove_worktree(&a).unwrap();
        assert!(!a.path.exists());
        // Taken away, it is checked out again on its branch.
        restore_worktree(&a).unwrap();
        assert_eq!(branch(&a.path).as_deref(), Some("u0/retry-s3-uploads"));
        let _ = remove_worktree(&a);
        let _ = remove_worktree(&b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sparse_worktree_writes_only_the_files_its_patterns_match() {
        let dir = repo("sparse");
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/big.md"), "big\n").unwrap();
        git(&dir, &["add", "docs"], None).unwrap();
        git(&dir, &["commit", "-q", "-m", "docs"], None).unwrap();
        let under = dir.join(".worktrees");
        let wt = worktree(
            &dir,
            "lean",
            &under,
            "g/",
            "HEAD",
            Some("/*\n# not docs\n!/docs/\n"),
        )
        .unwrap();
        assert!(wt.path.join("a.txt").is_file());
        assert!(!wt.path.join("docs").exists());
        // The repository's own checkout is left whole.
        assert!(dir.join("docs/big.md").is_file());
        let status = git(&dir, &["status", "--porcelain", "-uno"], None).unwrap();
        assert!(status.is_empty(), "{status}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_is_read_by_its_new_name_with_its_counts_and_patch() {
        let dir = repo("rename");
        let before = git(&dir, &["rev-parse", "HEAD"], None).unwrap();
        let text = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        std::fs::remove_file(dir.join("a.txt")).unwrap();
        std::fs::write(dir.join("z.txt"), format!("{text}four\n")).unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let c = checkpoint(&dir, "s1/1-after").unwrap();
        let files = diff(&dir, &before, &c, false).unwrap();
        let z = files.iter().find(|f| f.path == "z.txt").unwrap();
        assert_eq!((z.added, z.gone), (1, 0));
        assert!(z.patch.contains("\nrename from a.txt\nrename to z.txt\n"));
        let b = files.iter().find(|f| f.path == "b.txt").unwrap();
        assert!(b.patch.starts_with("diff --git a/b.txt b/b.txt\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stashes_repo_summaries_and_branch_pr_details_read_cleanly() {
        let dir = repo("stash");
        git(&dir, &["config", "user.email", "t@localhost"], None).unwrap();
        git(&dir, &["config", "user.name", "Tester"], None).unwrap();
        std::fs::write(dir.join("a.txt"), "one\nchanged\n").unwrap();
        std::fs::write(dir.join("extra.txt"), "untracked\n").unwrap();

        let summaries = repo_summaries(std::slice::from_ref(&dir));
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            (
                summaries[0].branch.as_deref(),
                summaries[0].files,
                summaries[0].added,
                summaries[0].gone
            ),
            (Some("main"), 2, 2, 2)
        );

        stash_push(&dir, Some("WIP: carbon tokens")).unwrap();
        assert!(!dir.join("extra.txt").exists());
        let listed = stashes(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (
                listed[0].index,
                listed[0].branch.as_str(),
                listed[0].message.as_str()
            ),
            (0, "main", "WIP: carbon tokens")
        );
        stash_apply(&dir, 0, false).unwrap();
        assert!(dir.join("extra.txt").is_file());
        stash_drop(&dir, 0).unwrap();
        assert!(stashes(&dir).is_empty());

        let pr_json = serde_json::json!({
            "number": 491,
            "title": "Carbon theme",
            "url": "https://github.com/acme/carbon-ui/pull/491",
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "feat/carbon",
            "baseRefName": "main",
            "reviewDecision": "CHANGES_REQUESTED",
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "BLOCKED",
            "statusCheckRollup": [{
                "name": "unit-tests",
                "conclusion": "FAILURE",
                "detailsUrl": "https://github.com/acme/carbon-ui/actions/runs/9/job/1",
                "startedAt": "2026-11-27T10:00:00Z",
                "completedAt": "2026-11-27T10:01:42Z"
            }],
            "reviews": [{
                "author": {"login": "maya"},
                "body": "Keep the 1px border.",
                "submittedAt": "2026-11-27T10:02:00Z"
            }]
        });
        let pr = parse_pr(&pr_json).unwrap();
        assert_eq!(
            (pr.head.as_str(), pr.base.as_str()),
            ("feat/carbon", "main")
        );
        assert_eq!(pr.checks[0].duration.as_deref(), Some("1m 42s"));
        assert_eq!(
            pr.review_note
                .as_ref()
                .map(|(w, _, b)| (w.as_str(), b.as_str())),
            Some(("maya", "Keep the 1px border."))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Which changes the right column shows: one turn's, all of the
/// session's, the branch's against where it left the main one, one
/// commit's, or the edits of one call (by its place in the thread).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    Turn(usize),
    All,
    Uncommitted,
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
    /// The recent commits on the branch.
    pub commits: Vec<Commit>,
    /// The stashes in the repository.
    pub stashes: Vec<Stash>,
    /// The local branches.
    pub branches: Vec<String>,
    /// The pull request of its branch.
    pub pr: Option<Pr>,
    /// The step a git action is at.
    pub step: Option<String>,
    /// How far its branch is ahead of and behind its upstream, and whether
    /// the repository has an origin.
    pub upstream: (usize, usize, bool),
    /// The branch its branch is compared with; none, the main one.
    pub compare_to: Option<String>,
    /// The files (as git names them) you left out of the next commit.
    pub left_out: std::collections::BTreeSet<String>,
    /// The files you marked viewed in this session's review.
    pub viewed: std::collections::BTreeSet<String>,
    /// The agent was asked to write a commit message: its answer goes in
    /// the commit form.
    pub writing_message: bool,
    /// The repositories its folder holds, where it is not in one itself
    /// (`repos`), their summaries, and the one git is asked about of them.
    pub repos: Vec<PathBuf>,
    pub summaries: Vec<RepoSummary>,
    pub repo: Option<PathBuf>,
}

impl Default for View {
    /// Nothing read yet: taken to have an origin until git says otherwise.
    fn default() -> Self {
        Self {
            scoped: None,
            commits: Vec::new(),
            stashes: Vec::new(),
            branches: Vec::new(),
            pr: None,
            step: None,
            upstream: (0, 0, true),
            compare_to: None,
            left_out: std::collections::BTreeSet::new(),
            viewed: std::collections::BTreeSet::new(),
            writing_message: false,
            repos: Vec::new(),
            summaries: Vec::new(),
            repo: None,
        }
    }
}
