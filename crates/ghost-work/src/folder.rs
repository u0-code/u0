//! The folder a piece of work is done in, made ready before its first
//! sitting.
//!
//! A folder made for the work is a git repository from the start: what was
//! done in it can be read back step by step, and a part of the work can be
//! given a checkout of its own beside it, so that two workers never write
//! over each other. A folder that was there already, named by whoever
//! asked, is used as it is.

use std::{io, path::Path, process::Command};

/// Make `dir` ready to work in: there already, it is left as it is;
/// otherwise it is made, and made a git repository. Says whether it was
/// made. A machine with no git still gets the folder.
pub fn prepare(dir: &Path) -> io::Result<bool> {
    if dir.is_dir() {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let inited = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir)
        .status();
    if !inited.is_ok_and(|s| s.success()) {
        tracing::warn!("{} was made, but not as a git repository", dir.display());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_made_for_work_is_a_repository_and_one_already_there_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("2026-11-18").join("count-lines-d2a4");
        assert!(prepare(&dir).unwrap());
        assert!(dir.join(".git").is_dir(), "made as a repository");
        let theirs = root.path().join("theirs");
        std::fs::create_dir(&theirs).unwrap();
        assert!(!prepare(&theirs).unwrap());
        assert!(!theirs.join(".git").exists(), "theirs is used as it is");
    }
}
