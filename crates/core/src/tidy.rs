//! What u0 keeps on this machine, measured and tidied, as t3code's
//! storage cleanup: the records of sessions it no longer lists, the
//! worktrees no session works in (one with changes not committed is left),
//! the checkpoints of sessions gone.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use crate::git;

/// What can be tidied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tidy {
    Records,
    Worktrees,
    Checkpoints,
}

/// What the desk knows of the sessions it keeps, to tidy around them.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    /// The sessions kept, each by its agent and id: their records stay.
    pub records: Vec<(String, String)>,
    /// The worktrees sessions work in.
    pub worktrees: Vec<PathBuf>,
    /// The folders u0's worktrees are in.
    pub under: Vec<PathBuf>,
    /// The projects (those in a repository are tidied), and the checkpoint
    /// names of the sessions kept.
    pub projects: Vec<PathBuf>,
    pub uids: Vec<String>,
}

/// How much each part of the home takes, in bytes; the worktrees, those
/// in `worktrees`.
pub fn sizes(home: &Path, worktrees: &[PathBuf]) -> Vec<(String, u64)> {
    let dir = |d: &str| size(&home.join(d));
    vec![
        ("Session records".to_owned(), dir("transcripts")),
        (
            "Worktrees".to_owned(),
            worktrees.iter().map(|w| size(w)).sum(),
        ),
        ("Agents installed".to_owned(), dir("agents")),
        ("Scratch sessions".to_owned(), dir("scratch")),
    ]
}

fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        std::fs::read_dir(path).map_or(0, |all| all.flatten().map(|e| size(&e.path())).sum())
    } else {
        meta.len()
    }
}

/// Tidy `what`; what was done, in words.
pub fn tidy(home: &Path, what: Tidy, kept: &Kept) -> String {
    let repos: Vec<PathBuf> = (kept.projects.iter())
        .flat_map(|f| git::project_repos(f))
        .collect();
    match what {
        Tidy::Records => {
            let kept: Vec<PathBuf> = (kept.records.iter())
                .map(|(agent, id)| crate::store::record_path(home, agent, id))
                .collect();
            let mut n = 0;
            for agent in std::fs::read_dir(home.join("transcripts"))
                .into_iter()
                .flatten()
                .flatten()
            {
                for record in std::fs::read_dir(agent.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let path = record.path();
                    if !kept.contains(&path) && std::fs::remove_file(&path).is_ok() {
                        n += 1;
                    }
                }
            }
            format!("{n} record{} removed", if n == 1 { "" } else { "s" })
        }
        Tidy::Worktrees => {
            let (mut gone, mut left) = (0, 0);
            for repo in &repos {
                for wt in git::worktrees(repo, &kept.under) {
                    if kept.worktrees.iter().any(|k| wt.path.starts_with(k)) {
                        continue;
                    }
                    // `git worktree remove` keeps one with changes.
                    if git::remove_worktree(&wt).is_ok() {
                        gone += 1;
                    } else {
                        left += 1;
                    }
                }
            }
            let mut said = format!(
                "{gone} worktree{} removed",
                if gone == 1 { "" } else { "s" }
            );
            if left > 0 {
                let _ = write!(said, "; {left} with changes not committed left");
            }
            said
        }
        Tidy::Checkpoints => {
            let mut n = 0;
            for repo in &repos {
                for uid in git::checkpoint_owners(repo) {
                    if !kept.uids.contains(&uid) {
                        git::drop_checkpoints(repo, &uid);
                        n += 1;
                    }
                }
            }
            format!(
                "Checkpoints of {n} session{} removed",
                if n == 1 { "" } else { "s" }
            )
        }
    }
}

/// `bytes` as a person reads a size: "12 KB", "3.4 MB".
pub fn shown(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..1_024 => format!("{bytes} B"),
        1_024..1_048_576 => format!("{:.0} KB", b / 1_024.),
        1_048_576..1_073_741_824 => format!("{:.1} MB", b / 1_048_576.),
        _ => format!("{:.1} GB", b / 1_073_741_824.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_no_session_keeps_are_removed_and_sizes_are_read() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let dir = home.join("transcripts/a");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kept.jsonl"), "x").unwrap();
        std::fs::write(dir.join("gone.jsonl"), "yy").unwrap();
        assert_eq!(sizes(home, &[])[0], ("Session records".to_owned(), 3));
        let kept = Kept {
            records: vec![("a".into(), "kept".into())],
            worktrees: Vec::new(),
            under: Vec::new(),
            projects: Vec::new(),
            uids: Vec::new(),
        };
        assert_eq!(tidy(home, Tidy::Records, &kept), "1 record removed");
        assert!(dir.join("kept.jsonl").is_file());
        assert_eq!(shown(1_536), "2 KB");
    }
}
