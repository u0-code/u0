//! A part's own checkout of the folder its work is done in, so that two
//! workers never write over each other.
//!
//! When the work's folder is in a git repository, a part is given a
//! worktree of its own on a branch of its own, `ghost/<work>/part-<n>`,
//! made from the folder as it is at that moment: its uncommitted changes
//! and new files too, taken as a commit through an index of its own,
//! without touching the folder's index or what is checked out there.
//!
//! When the part has finished, what it changed is committed on its branch
//! and applied to the work's folder. `git apply` applies the whole of it or
//! nothing, so a change that no longer fits, because the folder changed in
//! the same places meanwhile, leaves the folder as it was; the branch is
//! kept, for the work to merge itself. A part that did not finish keeps
//! its branch and is not applied. A part that changed nothing leaves
//! nothing behind.

use std::{
    ffi::OsStr,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
};

use crate::undertaking::Checkout;

/// Applying to a work's folder is done one part at a time.
static APPLYING: Mutex<()> = Mutex::new(());

/// Run git in `dir` and give back what it printed, or fail with what it
/// said. Commits are made as Ghost, whatever is configured.
fn git(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &OsStr)],
    stdin: Option<&[u8]>,
) -> io::Result<String> {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "user.name=Ghost",
            "-c",
            "user.email=ghost@localhost",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command.spawn()?;
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        pipe.write_all(input)?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
    } else {
        Err(io::Error::other(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// The top of the repository `dir` is in, if it is in one.
pub fn repository(dir: &Path) -> Option<PathBuf> {
    let top = git(dir, &["rev-parse", "--show-toplevel"], &[], None).ok()?;
    Some(PathBuf::from(top))
}

/// The repository at `top` as its files are now, every one not ignored,
/// as a commit on top of what is checked out, without touching its index.
fn snapshot(top: &Path, message: &str) -> io::Result<String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let index = std::env::temp_dir().join(format!(
        "ghost-index-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let env = [("GIT_INDEX_FILE", index.as_os_str())];
    let made = (|| {
        git(top, &["add", "--all"], &env, None)?;
        let tree = git(top, &["write-tree"], &env, None)?;
        let head = git(
            top,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
            &[],
            None,
        )
        .ok();
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
        if let Some(head) = &head {
            args.extend(["-p", head.as_str()]);
        }
        git(top, &args, &[], None)
    })();
    let _ = std::fs::remove_file(&index);
    made
}

/// Give a part a checkout of its own of the folder `of`, at `at`, on the
/// branch `branch`. Returns the record of it and the folder in it that
/// stands where `of` stands in its repository. `None` when `of` is not in
/// a repository: the part then works in `of` itself.
pub fn make(of: &Path, at: &Path, branch: &str) -> io::Result<Option<(Checkout, PathBuf)>> {
    let Some(top) = repository(of) else {
        return Ok(None);
    };
    let base = snapshot(&top, &format!("Ghost: the folder as {branch} began"))?;
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let at_str = at.display().to_string();
    git(
        &top,
        &["worktree", "add", "--quiet", "-b", branch, &at_str, &base],
        &[],
        None,
    )?;
    let within = of
        .canonicalize()?
        .strip_prefix(top.canonicalize()?)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let checkout = Checkout {
        of: of.display().to_string(),
        repo: top.display().to_string(),
        at: at_str,
        branch: branch.to_owned(),
        base,
    };
    Ok(Some((checkout, at.join(within))))
}

/// What came of bringing a part's changes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Back {
    /// It changed nothing: its checkout and branch are gone.
    Unchanged,
    /// What it changed is in the work's folder now.
    Applied { files: Vec<String> },
    /// What it changed is on its branch and not in the work's folder: it
    /// did not finish, or (`clashed`) the folder changed in the same places.
    Kept { files: Vec<String>, clashed: bool },
}

/// Commit what a part changed in its checkout, and if `apply`, apply it to
/// the work's folder. The checkout is taken away either way; its branch
/// stays where its changes are not in the folder.
pub fn bring_back(checkout: &Checkout, message: &str, apply: bool) -> io::Result<Back> {
    let (top, at) = (Path::new(&checkout.repo), Path::new(&checkout.at));
    if at.is_dir() {
        git(at, &["add", "--all"], &[], None)?;
        if git(at, &["diff", "--cached", "--quiet"], &[], None).is_err() {
            git(
                at,
                &["commit", "--quiet", "--no-verify", "-m", message],
                &[],
                None,
            )?;
        }
    }
    let head = git(top, &["rev-parse", &checkout.branch], &[], None)?;
    let remove = || {
        let _ = git(
            top,
            &["worktree", "remove", "--force", &checkout.at],
            &[],
            None,
        );
        let _ = git(top, &["worktree", "prune"], &[], None);
    };
    let drop_branch = || {
        let _ = git(top, &["branch", "-D", &checkout.branch], &[], None);
    };
    if head == checkout.base {
        remove();
        drop_branch();
        return Ok(Back::Unchanged);
    }
    let files: Vec<String> = git(
        top,
        &["diff", "--name-only", &checkout.base, &head],
        &[],
        None,
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    if !apply {
        remove();
        return Ok(Back::Kept {
            files,
            clashed: false,
        });
    }
    let patch = git(top, &["diff", "--binary", &checkout.base, &head], &[], None)? + "\n";
    let applied = {
        let _one = APPLYING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        git(
            top,
            &["apply", "--whitespace=nowarn", "-"],
            &[],
            Some(patch.as_bytes()),
        )
    };
    remove();
    if applied.is_ok() {
        drop_branch();
        Ok(Back::Applied { files })
    } else {
        Ok(Back::Kept {
            files,
            clashed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"], &[], None).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git(dir.path(), &["add", "."], &[], None).unwrap();
        git(dir.path(), &["commit", "--quiet", "-m", "first"], &[], None).unwrap();
        dir
    }

    fn read(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).unwrap_or_default()
    }

    #[test]
    fn a_part_starts_from_the_folder_as_it_is_and_what_it_changes_comes_back_into_it() {
        let work = repo();
        // Not committed yet: the part still sees it.
        std::fs::write(work.path().join("new.txt"), "from the work\n").unwrap();
        let places = tempfile::tempdir().unwrap();
        let at = places.path().join("w-1");
        let (checkout, dir) = make(work.path(), &at, "ghost/w/part-1").unwrap().unwrap();
        assert_eq!(read(&dir, "new.txt"), "from the work\n");
        std::fs::write(dir.join("b.txt"), "made by the part\n").unwrap();
        // The work goes on meanwhile, elsewhere in the folder.
        std::fs::write(work.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();

        let back = bring_back(&checkout, "part 1", true).unwrap();
        assert_eq!(
            back,
            Back::Applied {
                files: vec!["b.txt".into()]
            }
        );
        assert_eq!(read(work.path(), "b.txt"), "made by the part\n");
        assert_eq!(
            read(work.path(), "a.txt"),
            "one\ntwo\nthree\nfour\n",
            "theirs kept"
        );
        assert!(!at.exists(), "its checkout is gone");
        assert!(git(work.path(), &["rev-parse", "ghost/w/part-1"], &[], None).is_err());
    }

    #[test]
    fn a_change_that_clashes_leaves_the_folder_as_it_was_and_keeps_its_branch() {
        let work = repo();
        let places = tempfile::tempdir().unwrap();
        let (checkout, dir) = make(work.path(), &places.path().join("w-2"), "ghost/w/part-2")
            .unwrap()
            .unwrap();
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(work.path().join("a.txt"), "one\n2\nthree\n").unwrap();

        let back = bring_back(&checkout, "part 2", true).unwrap();
        assert_eq!(
            back,
            Back::Kept {
                files: vec!["a.txt".into()],
                clashed: true
            }
        );
        assert_eq!(read(work.path(), "a.txt"), "one\n2\nthree\n");
        assert!(git(work.path(), &["rev-parse", "ghost/w/part-2"], &[], None).is_ok());
    }

    #[test]
    fn a_part_that_changed_nothing_leaves_nothing_and_a_folder_outside_git_is_shared() {
        let work = repo();
        let places = tempfile::tempdir().unwrap();
        let at = places.path().join("w-3");
        let (checkout, _) = make(work.path(), &at, "ghost/w/part-3").unwrap().unwrap();
        assert_eq!(
            bring_back(&checkout, "part 3", true).unwrap(),
            Back::Unchanged
        );
        assert!(!at.exists());
        assert!(git(work.path(), &["rev-parse", "ghost/w/part-3"], &[], None).is_err());

        let plain = tempfile::tempdir().unwrap();
        assert!(
            make(plain.path(), &places.path().join("x"), "b")
                .unwrap()
                .is_none()
        );
    }
}
