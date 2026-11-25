//! What the server is asked of a project's folder and its git, off the
//! window's thread: where a session works and what its folder holds, its
//! diffs, commits and pull request, its worktree and checkpoints, what you
//! ask of git from the desk, and finding things in it. Each is one
//! [`Call`](crate::proto::Call), answered as one [`Event`].

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use agent_client_protocol::v1::schema::ToolCallId;
use serde::{Deserialize, Serialize};

use crate::{
    git,
    proto::Event,
    scripts::{self, Script},
    session::Begin,
};

/// A checkpoint, and what changed up to it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub commit: String,
    /// From the turn's checkpoint before to this one.
    pub turn: Option<Vec<git::FileDiff>>,
    /// From the session's first checkpoint to this one.
    pub total: Option<Vec<git::FileDiff>>,
}

/// What you ask of git from the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// What a project's folder holds: whether it is in a repository, and an
/// icon of its own.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Held {
    pub repo: bool,
    pub icon: Option<PathBuf>,
}

/// The branch of `folder`, where the session `key` works, whether it is in
/// a repository, and the scripts its `ghost.json` names.
pub fn look(key: u64, folder: PathBuf) -> Event {
    let root = git::root(&folder);
    Event::Looked {
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
/// take the one at `at` away; then the session `key`'s folder, read again.
pub fn save_script(dir: PathBuf, at: Option<usize>, script: Option<Script>, key: u64) -> Event {
    match scripts::write_script(&dir, at, script) {
        Ok(()) => look(key, dir),
        Err(why) => Event::Notice(Err(("Couldn't write ghost.json".into(), why))),
    }
}

/// The folders in `dir` that are not hidden, by name, in order; none
/// where it cannot be read.
pub fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// What the project `folder` holds.
pub fn read_project(folder: PathBuf) -> Event {
    Event::Held {
        held: Held {
            repo: git::root(&folder).is_some(),
            icon: icon_of(&folder),
        },
        folder,
    }
}

/// The icon a project has of its own, if it has one where icons are kept.
fn icon_of(folder: &Path) -> Option<PathBuf> {
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

/// What changed in `scope` (the branch against its base, or a commit)
/// where the session `key` works.
pub fn scope_diff(
    dir: &Path,
    key: u64,
    scope: git::Scope,
    base: Option<&str>,
    no_spacing: bool,
) -> Event {
    let result = match &scope {
        git::Scope::Commit(id) => git::commit_diff(dir, id, no_spacing),
        _ => git::against_base(dir, base, no_spacing),
    };
    Event::Diffed { key, scope, result }
}

/// Make a worktree in Ghost's `home` for the session `key`, about `about`,
/// on a branch beginning with `prefix`.
pub fn make_worktree(
    dir: &Path,
    about: &str,
    home: &Path,
    prefix: &str,
    from: Option<Begin>,
    key: u64,
) -> Event {
    let result = match from {
        Some(Begin::Pr(number, _)) => git::pr_worktree(dir, number, home),
        Some(Begin::Branch(b)) => git::worktree(dir, about, home, prefix, &b),
        None => git::worktree(dir, about, home, prefix, "HEAD"),
    };
    let result = result.map(|wt| {
        let scripts = scripts::config(&wt.path).scripts;
        (wt, scripts)
    });
    Event::Worktree { key, result }
}

/// Take a checkpoint; after a turn, read what changed in it and in all.
///
/// `after` says it is taken after the turn `turn`, and `before` and `first`
/// are the turn's checkpoint before it and the session's first; `then` is
/// what is said once it is taken.
pub fn checkpoint(
    dir: &Path,
    name: &str,
    key: u64,
    turn: usize,
    after: bool,
    (before, first): (Option<String>, Option<String>),
    then: Option<String>,
    no_spacing: bool,
) -> Event {
    let result = git::checkpoint(dir, name).map(|commit| {
        let diff = |from: &Option<String>| {
            from.as_deref()
                .filter(|_| after)
                .and_then(|f| git::diff(dir, f, &commit, no_spacing).ok())
        };
        Snapshot {
            turn: diff(&before),
            total: diff(&first),
            commit,
        }
    });
    Event::Checkpoint {
        key,
        turn,
        after,
        result,
        then,
    }
}

/// Put the session's files back as `target` had them.
pub fn restore(
    dir: &Path,
    target: &str,
    name: &str,
    first: Option<String>,
    key: u64,
    turn: usize,
    undo: bool,
    no_spacing: bool,
) -> Event {
    let result = git::restore(dir, target, name).map(|back| {
        let total = first.and_then(|f| {
            let now = git::checkpoint(dir, &format!("{name}-now")).ok()?;
            git::diff(dir, &f, &now, no_spacing).ok()
        });
        (back, total)
    });
    Event::Restored {
        key,
        turn,
        undo,
        result,
    }
}

/// Do `action` in `dir`, saying each step on its way.
pub fn git_action(
    dir: &Path,
    action: GitAction,
    prefix: &str,
    left_out: &[String],
    key: u64,
    say: &dyn Fn(Event),
) -> Event {
    let step = |text: &str| {
        say(Event::GitStep {
            key,
            text: text.to_owned(),
        });
    };
    // Work on the main branch goes on a branch of its own before a pull
    // request is opened for it: named for what the commit says. A push
    // alone goes where you are.
    let own_branch = |m: &str| -> Result<(), String> {
        let main = git::default_branch(dir);
        let now = git::branch(dir);
        if now.is_some()
            && main.as_deref().map(|b| b.trim_start_matches("origin/")) == now.as_deref()
        {
            step("Making a branch");
            git::branch_off(dir, &format!("{prefix}{}", git::slug(m)))?;
        }
        Ok(())
    };
    let result = match &action {
        GitAction::Commit(m) => git::commit(dir, m, left_out).map(|id| format!("Committed {id}")),
        GitAction::CommitPush(m) => {
            step("Committing");
            git::commit(dir, m, left_out).and_then(|id| {
                step("Pushing");
                git::push(dir).map(|_| format!("Committed {id} and pushed"))
            })
        }
        GitAction::PullRequest(m) => own_branch(m).and_then(|()| {
            step("Committing");
            // Nothing left to commit is no reason to stop.
            let _ = git::commit(dir, m, left_out);
            step("Pushing");
            git::push(dir).and_then(|_| {
                step("Opening the pull request");
                git::pull_request(dir)
            })
        }),
        GitAction::Switch(b) => git::switch(dir, b).map(|()| format!("On {b}")),
        GitAction::Merge => git::merge_pr(dir).map(|_| "The pull request is merged".into()),
        GitAction::Pull => git::pull(dir).map(|_| "Pulled".into()),
        GitAction::NewBranch(b) => git::branch_off(dir, b).map(|()| format!("On {b}, new")),
        GitAction::Init => {
            git::init_here(dir).map(|_| "It is a repository now, all of it committed".into())
        }
        GitAction::Publish => git::publish(dir).map(|_| "It is on GitHub now, private".into()),
        GitAction::RenameBranch(b) => git::rename_branch(dir, b).map(|()| b.clone()),
        GitAction::RemoveWorktree(wt) => git::remove_worktree(wt)
            .map(|()| format!("Worktree removed; the branch {} is kept", wt.branch)),
    };
    Event::Git {
        key,
        action,
        result,
    }
}

/// Clone `url` into `dir`, saying how it goes, until `stop` is set.
pub fn clone(url: &str, dir: PathBuf, stop: &AtomicBool, say: &dyn Fn(Event)) -> Event {
    let cloning = |what: &str, share: Option<f32>| {
        say(Event::Cloning {
            what: what.to_owned(),
            share,
        });
    };
    let result = git::clone(url, &dir, &cloning, stop);
    Event::Cloned { dir, result }
}

/// Find the line each diff of the calls `calls` of the session `key` starts
/// on in its file.
pub fn place(key: u64, calls: Vec<(ToolCallId, Vec<(PathBuf, String)>)>) -> Event {
    Event::Placed {
        key,
        starts: calls
            .into_iter()
            .map(|(id, diffs)| (id, diffs.iter().map(|(f, t)| line_of(f, t)).collect()))
            .collect(),
    }
}

/// The line of `file` that `text` starts on, counting from one.
fn line_of(file: &Path, text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let whole = std::fs::read_to_string(file).ok()?;
    let at = whole.find(text)?;
    Some(whole[..at].matches('\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn a_diffs_place_is_found_in_its_file_once_the_call_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
        let mut s = Session::new(1, "x", dir.path().to_path_buf());
        let json = format!(
            r#"{{"sessionUpdate":"tool_call","toolCallId":"e","title":"Edit","kind":"edit","status":"completed","content":[{{"type":"diff","path":"{}","oldText":"3\n","newText":"three\nfour\n"}}]}}"#,
            file.display()
        );
        s.apply(serde_json::from_str(&json).unwrap());
        assert!(s.tools().next().unwrap().starts.is_empty());
        let Event::Placed { starts, .. } = place(1, std::mem::take(&mut s.placing)) else {
            panic!("the diffs were not placed");
        };
        for (id, starts) in starts {
            s.placed(&id, starts);
        }
        assert_eq!(s.tools().next().unwrap().starts, vec![Some(3)]);
    }
}
