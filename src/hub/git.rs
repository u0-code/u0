//! Git off the window's thread: where a session works and what its folder
//! holds, its diffs, commits and pull request, its worktree and
//! checkpoints, and what you ask of git from the desk.

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use super::{Msg, Sender, off};
use crate::{
    acp, git,
    scripts::{self, Script},
};

/// A checkpoint, and what changed up to it.
pub struct Snapshot {
    pub commit: String,
    /// From the turn's checkpoint before to this one.
    pub turn: Option<Vec<git::FileDiff>>,
    /// From the session's first checkpoint to this one.
    pub total: Option<Vec<git::FileDiff>>,
}

/// What you ask of git from the desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAction {
    Commit(String),
    CommitPush(String),
    /// Commit what is left with this message, push, and open a pull request.
    PullRequest(String),
    RemoveWorktree(git::Worktree),
    /// Check out this branch.
    Switch(String),
    /// Rename the worktree's branch, unless it is pushed.
    RenameBranch(String),
    /// Merge the branch's pull request (squashed), as gh does.
    Merge,
    /// Make this branch, from where it is, and check it out.
    NewBranch(String),
    /// Bring in what the upstream has.
    Pull,
    /// Make the folder a repository.
    Init,
    /// Put the repository on GitHub, private.
    Publish,
}

impl GitAction {
    /// What it does, as "Couldn't …" ends.
    pub fn doing(&self) -> &'static str {
        match self {
            Self::Commit(_) => "commit",
            Self::CommitPush(_) => "commit and push",
            Self::PullRequest(_) => "open the pull request",
            Self::RemoveWorktree(_) => "remove the worktree",
            Self::Switch(_) => "switch the branch",
            Self::RenameBranch(_) => "rename the branch",
            Self::Merge => "merge the pull request",
            Self::Pull => "pull",
            Self::NewBranch(_) => "make the branch",
            Self::Init => "make it a repository",
            Self::Publish => "put it on GitHub",
        }
    }
}

/// Read the branch of `folder`, where the session `key` works, whether it
/// is in a repository, and the scripts its `ghost.json` names.
pub fn look(folder: PathBuf, key: u64, tx: Sender) {
    off(tx, move || looked(key, folder));
}

fn looked(key: u64, folder: PathBuf) -> Msg {
    let root = git::root(&folder);
    Msg::Looked {
        key,
        branch: git::branch(&folder),
        repo: root.is_some(),
        scripts: root
            .map(|r| scripts::config(&r).scripts)
            .unwrap_or_default(),
        folder,
    }
}

/// Put `script` at `at` in the `ghost.json` of the repository at `dir`, or
/// take the one at `at` away; then read the session `key`'s folder again.
pub fn save_script(dir: PathBuf, at: Option<usize>, script: Option<Script>, key: u64, tx: Sender) {
    off(tx, move || match scripts::write_script(&dir, at, script) {
        Ok(()) => looked(key, dir),
        Err(why) => Msg::Notice(Err(("Couldn't write ghost.json".into(), why))),
    });
}

/// What a project's folder holds: whether it is in a repository, and an
/// icon of its own.
#[derive(Debug, Clone, Default)]
pub struct Held {
    pub repo: bool,
    pub icon: Option<PathBuf>,
}

/// Read what the project `folder` holds.
pub fn read_project(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Held {
        held: Held {
            repo: git::root(&folder).is_some(),
            icon: icon_of(&folder),
        },
        folder,
    });
}

/// The icon a project has of its own, if it has one where icons are kept.
fn icon_of(folder: &std::path::Path) -> Option<PathBuf> {
    [
        "favicon.svg",
        "favicon.png",
        "public/favicon.svg",
        "public/favicon.png",
        "static/favicon.png",
        "assets/icon.svg",
        "assets/icon.png",
        "icon.svg",
        "icon.png",
        "logo.svg",
        "logo.png",
    ]
    .iter()
    .map(|name| folder.join(name))
    .find(|p| p.is_file())
}

/// Read what is not committed in `dir`.
pub fn uncommitted(dir: PathBuf, key: u64, no_spacing: bool, tx: Sender) {
    off(tx, move || Msg::Uncommitted {
        key,
        result: git::uncommitted(&dir, no_spacing),
    });
}

/// What changed in `scope` (the branch against its base, or a commit)
/// where the session `key` works.
pub fn scope_diff(
    dir: PathBuf,
    key: u64,
    scope: git::Scope,
    base: Option<String>,
    no_spacing: bool,
    tx: Sender,
) {
    off(tx, move || {
        let result = match &scope {
            git::Scope::Commit(id) => git::commit_diff(&dir, id, no_spacing),
            _ => git::against_base(&dir, base.as_deref(), no_spacing),
        };
        Msg::Diffed { key, scope, result }
    });
}

/// The recent commits and the local branches where the session `key` works.
pub fn commits(dir: PathBuf, key: u64, tx: Sender) {
    off(tx, move || Msg::Commits {
        key,
        commits: git::log(&dir, 12),
        branches: git::branches(&dir),
        upstream: git::upstream(&dir),
    });
}

/// Read the pull request of the branch where the session `key` works.
pub fn pr(dir: PathBuf, key: u64, tx: Sender) {
    off(tx, move || Msg::Pr {
        key,
        pr: git::read_pr(&dir),
    });
}

/// Read what `gh` says of the pull request `number` of the project `dir`.
pub fn pr_text(dir: PathBuf, number: u64, tx: Sender) {
    off(tx, move || Msg::PrText {
        text: git::pull_request_text(&dir, number),
        number,
    });
}

/// Read the open pull requests and issues of `folder` with `gh`, for `@`
/// to name and the palette to begin from.
pub fn list_prs(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Prs {
        prs: git::open_on_github(&folder, "pr"),
        issues: git::open_on_github(&folder, "issue"),
        folder,
    });
}

/// Read the files of `folder` that git sees (tracked, and new ones not
/// ignored), for `@` to name.
pub fn list_files(folder: PathBuf, tx: Sender) {
    off(tx, move || Msg::Files {
        files: git::files(&folder),
        folder,
    });
}

/// Look for `words` in the files of `folder` git sees (git grep), for the
/// palette: each place as "path:line", and the line.
pub fn grep(folder: PathBuf, words: String, tx: Sender) {
    off(tx, move || Msg::Grepped {
        found: git::grep(&folder, &words),
        words,
    });
}

/// Make the new project `name` in `dir`: a folder, a repository.
pub fn create(dir: PathBuf, name: String, tx: Sender) {
    off(tx, move || Msg::Cloned {
        result: git::create(&dir, &name),
        dir,
    });
}

/// Clone `url` into `dir`, until `stop` is set.
pub fn clone(url: String, dir: PathBuf, stop: Arc<AtomicBool>, tx: Sender) {
    off(tx.clone(), move || {
        let say = |what: &str, share: Option<f32>| {
            let _ = tx.send(Msg::Cloning {
                what: what.to_owned(),
                share,
            });
        };
        let result = git::clone(&url, &dir, &say, &stop);
        Msg::Cloned { dir, result }
    });
}

/// Make a worktree for the session `key`, about `about`, on a branch
/// beginning with `prefix`.
pub fn make_worktree(
    dir: PathBuf,
    about: String,
    home: PathBuf,
    prefix: String,
    from: Option<crate::session::Begin>,
    key: u64,
    tx: Sender,
) {
    use crate::session::Begin;
    off(tx, move || {
        let result = match from {
            Some(Begin::Pr(number, _)) => git::pr_worktree(&dir, number, &home),
            Some(Begin::Branch(b)) => git::worktree(&dir, &about, &home, &prefix, &b),
            None => git::worktree(&dir, &about, &home, &prefix, "HEAD"),
        };
        let result = result.map(|wt| {
            let scripts = scripts::config(&wt.path).scripts;
            (wt, scripts)
        });
        Msg::Worktree { key, result }
    });
}

/// Take a checkpoint; after a turn, read what changed in it and in all.
///
/// `after` says it is taken after the turn `turn`, and `before` and `first`
/// are the turn's checkpoint before it and the session's first; `then` is
/// what is said once it is taken.
pub fn checkpoint(
    dir: PathBuf,
    name: String,
    key: u64,
    turn: usize,
    after: bool,
    before: Option<String>,
    first: Option<String>,
    then: Option<String>,
    no_spacing: bool,
    tx: Sender,
) {
    off(tx, move || {
        let result = git::checkpoint(&dir, &name).map(|commit| {
            let diff = |from: &Option<String>| {
                from.as_deref()
                    .filter(|_| after)
                    .and_then(|f| git::diff(&dir, f, &commit, no_spacing).ok())
            };
            Snapshot {
                turn: diff(&before),
                total: diff(&first),
                commit,
            }
        });
        Msg::Checkpoint {
            key,
            turn,
            after,
            result,
            then,
        }
    });
}

/// Put the session's files back as `target` had them.
pub fn restore(
    dir: PathBuf,
    target: String,
    name: String,
    first: Option<String>,
    key: u64,
    turn: usize,
    undo: bool,
    no_spacing: bool,
    tx: Sender,
) {
    off(tx, move || {
        let result = git::restore(&dir, &target, &name).map(|back| {
            let total = first.and_then(|f| {
                let now = git::checkpoint(&dir, &format!("{name}-now")).ok()?;
                git::diff(&dir, &f, &now, no_spacing).ok()
            });
            (back, total)
        });
        Msg::Restored {
            key,
            turn,
            undo,
            result,
        }
    });
}

/// Do `action` in `dir`.
pub fn git_action(
    dir: PathBuf,
    action: GitAction,
    prefix: String,
    left_out: Vec<String>,
    key: u64,
    tx: Sender,
) {
    off(tx.clone(), move || {
        let step = |text: &str| {
            let _ = tx.send(Msg::GitStep {
                key,
                text: text.to_owned(),
            });
        };
        // Work on the main branch goes on a branch of its own before a pull
        // request is opened for it: named for what the commit says. A push
        // alone goes where you are.
        let own_branch = |m: &str| -> Result<(), String> {
            let main = git::default_branch(&dir);
            let now = git::branch(&dir);
            if now.is_some()
                && main.as_deref().map(|b| b.trim_start_matches("origin/")) == now.as_deref()
            {
                step("Making a branch");
                git::branch_off(&dir, &format!("{prefix}{}", git::slug(m)))?;
            }
            Ok(())
        };
        let result = match &action {
            GitAction::Commit(m) => {
                git::commit(&dir, m, &left_out).map(|id| format!("Committed {id}"))
            }
            GitAction::CommitPush(m) => {
                step("Committing");
                git::commit(&dir, m, &left_out).and_then(|id| {
                    step("Pushing");
                    git::push(&dir).map(|_| format!("Committed {id} and pushed"))
                })
            }
            GitAction::PullRequest(m) => own_branch(m).and_then(|()| {
                step("Committing");
                // Nothing left to commit is no reason to stop.
                let _ = git::commit(&dir, m, &left_out);
                step("Pushing");
                git::push(&dir).and_then(|_| {
                    step("Opening the pull request");
                    git::pull_request(&dir)
                })
            }),
            GitAction::Switch(b) => git::switch(&dir, b).map(|()| format!("On {b}")),
            GitAction::Merge => git::merge_pr(&dir).map(|_| "The pull request is merged".into()),
            GitAction::Pull => git::pull(&dir).map(|_| "Pulled".into()),
            GitAction::NewBranch(b) => git::branch_off(&dir, b).map(|()| format!("On {b}, new")),
            GitAction::Init => {
                git::init_here(&dir).map(|_| "It is a repository now, all of it committed".into())
            }
            GitAction::Publish => git::publish(&dir).map(|_| "It is on GitHub now, private".into()),
            GitAction::RenameBranch(b) => git::rename_branch(&dir, b).map(|()| b.clone()),
            GitAction::RemoveWorktree(wt) => git::remove_worktree(wt)
                .map(|()| format!("Worktree removed; the branch {} is kept", wt.branch)),
        };
        Msg::Git {
            key,
            action,
            result,
        }
    });
}

/// Drop the checkpoints of the session `uid` in the repository at `dir`.
pub fn drop_checkpoints(dir: PathBuf, uid: String) {
    acp::runtime().spawn_blocking(move || git::drop_checkpoints(&dir, &uid));
}
