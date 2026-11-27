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
    Snapshots,
}

/// What the desk knows of the sessions it keeps, to tidy around them.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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

/// One category of what u0 keeps on disk, with its size and what tidying it
/// can reclaim.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageBucket {
    pub label: String,
    pub detail: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanable: Option<u64>,
}

/// Disk usage of u0's home and worktrees, plus free disk space on the volume.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageReport {
    pub buckets: Vec<StorageBucket>,
    pub free_bytes: u64,
}

/// Snapshots older than 30 days are eligible to be tidied.
const SNAPSHOT_KEEP_SECS: u64 = 30 * 86_400;

/// Free bytes available to the user on the filesystem holding `path`.
fn disk_free(path: &Path) -> u64 {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    let target = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(Path::new("/"))
    };
    let Ok(c_path) = CString::new(target.as_os_str().as_bytes()) else {
        return 0;
    };
    // SAFETY: `statvfs` writes into `stat` for a valid NUL-terminated path.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &raw mut stat) == 0 {
            #[allow(clippy::useless_conversion, reason = "f_bavail width varies by target")]
            let avail = u64::from(stat.f_bavail);
            avail.saturating_mul(stat.f_frsize)
        } else {
            0
        }
    }
}

/// Measure each part of `home` and `kept`, what tidying it can reclaim, and
/// how much free space remains on the disk.
pub fn report(home: &Path, kept: &Kept) -> StorageReport {
    let repos: Vec<PathBuf> = (kept.projects.iter())
        .flat_map(|f| git::project_repos(f))
        .collect();
    let mut wt_total = 0u64;
    let mut wt_clean = 0u64;
    let mut wt_count = 0usize;
    for repo in &repos {
        for wt in git::worktrees(repo, &kept.under) {
            wt_count += 1;
            let bytes = size(&wt.path);
            wt_total += bytes;
            if !kept.worktrees.iter().any(|k| wt.path.starts_with(k)) {
                wt_clean += bytes;
            }
        }
    }
    if wt_total == 0 {
        wt_total = kept.under.iter().map(|w| size(w)).sum();
    }

    let kept_records: Vec<PathBuf> = (kept.records.iter())
        .map(|(agent, id)| crate::store::record_path(home, agent, id))
        .collect();
    let mut rec_bytes = size(&home.join("sessions.json"));
    let mut rec_clean = 0u64;
    let mut rec_count = 0usize;
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
            let p = record.path();
            let b = size(&p);
            rec_bytes += b;
            rec_count += 1;
            if !kept_records.contains(&p) {
                rec_clean += b;
            }
        }
    }

    let mut cp_bytes = 0u64;
    let mut cp_clean = 0u64;
    for repo in &repos {
        let refs_dir = repo.join(".git/refs/u0/checkpoints");
        for uid in git::checkpoint_owners(repo) {
            let b = size(&refs_dir.join(&uid));
            cp_bytes += b;
            if !kept.uids.contains(&uid) {
                cp_clean += b;
            }
        }
    }

    let snap_dir = home.join("snapshots");
    let now = std::time::SystemTime::now();
    let mut snap_bytes = 0u64;
    let mut snap_clean = 0u64;
    for entry in std::fs::read_dir(&snap_dir).into_iter().flatten().flatten() {
        let p = entry.path();
        let b = size(&p);
        snap_bytes += b;
        let old = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age.as_secs() >= SNAPSHOT_KEEP_SECS);
        if old {
            snap_clean += b;
        }
    }

    let agents_dir = home.join("agents");
    let agents_bytes = size(&agents_dir);
    let agents_count = std::fs::read_dir(&agents_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .count();

    let n_projects = kept.projects.len();
    StorageReport {
        buckets: vec![
            StorageBucket {
                label: "Worktrees".into(),
                detail: format!("{wt_count} worktrees across {n_projects} projects"),
                bytes: wt_total,
                cleanable: (wt_clean > 0).then_some(wt_clean),
            },
            StorageBucket {
                label: "Thread database".into(),
                detail: format!("sessions.json + {rec_count} threads, transcripts"),
                bytes: rec_bytes,
                cleanable: (rec_clean > 0).then_some(rec_clean),
            },
            StorageBucket {
                label: "Checkpoints".into(),
                detail: format!("Turn checkpoints across {n_projects} projects"),
                bytes: cp_bytes,
                cleanable: (cp_clean > 0).then_some(cp_clean),
            },
            StorageBucket {
                label: "Snapshots and attachments".into(),
                detail: "Window captures and pasted images".into(),
                bytes: snap_bytes,
                cleanable: (snap_clean > 0).then_some(snap_clean),
            },
            StorageBucket {
                label: "Agents installed by u0".into(),
                detail: format!("{agents_count} adapters in agents/"),
                bytes: agents_bytes,
                cleanable: None,
            },
        ],
        free_bytes: disk_free(home),
    }
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
                "Checkpoints of {n} thread{} removed",
                if n == 1 { "" } else { "s" }
            )
        }
        Tidy::Snapshots => {
            let now = std::time::SystemTime::now();
            let mut n = 0;
            for entry in std::fs::read_dir(home.join("snapshots"))
                .into_iter()
                .flatten()
                .flatten()
            {
                let old = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|age| age.as_secs() >= SNAPSHOT_KEEP_SECS);
                if old && std::fs::remove_file(entry.path()).is_ok() {
                    n += 1;
                }
            }
            format!("{n} snapshot{} removed", if n == 1 { "" } else { "s" })
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
        let kept = Kept {
            records: vec![("a".into(), "kept".into())],
            worktrees: Vec::new(),
            under: Vec::new(),
            projects: Vec::new(),
            uids: Vec::new(),
        };
        let r = report(home, &kept);
        assert_eq!(r.buckets[1].label, "Thread database");
        assert_eq!((r.buckets[1].bytes, r.buckets[1].cleanable), (3, Some(2)));
        assert!(r.free_bytes > 0);
        assert_eq!(tidy(home, Tidy::Records, &kept), "1 record removed");
        assert!(dir.join("kept.jsonl").is_file());
        assert_eq!(shown(1_536), "2 KB");
    }
}
