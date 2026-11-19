//! What a step costs the disk: what is new in it, not the whole of the
//! work so far.

use std::path::Path;

use rusqlite::Connection;

use super::*;

/// What has been written to the database at `db` since a reader began
/// holding it at its first snapshot, read off its write-ahead log: the
/// bytes of the pages written, and the commits that wrote them. A reader
/// holding the first snapshot keeps every frame in the log, since none of
/// them can be copied back while it might still read the old pages.
fn written(db: &Path) -> (u64, u64) {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    let log = std::fs::read(wal).unwrap();
    let word = |at: usize| u32::from_be_bytes(log[at..at + 4].try_into().unwrap());
    let page = word(8) as usize;
    let (mut pages, mut commits) = (0, 0);
    let mut at = 32;
    while at + 24 + page <= log.len() {
        pages += 1;
        if word(at + 4) != 0 {
            commits += 1;
        }
        at += 24 + page;
    }
    (pages * page as u64, commits)
}

/// A work of `steps` steps, each reading a file of about six thousand
/// characters into the transcript, as a long piece of work does, and then
/// one to say it is done; and what its database had written to it.
async fn work_of(steps: usize) -> (u64, u64) {
    let dir = tempfile::tempdir().unwrap();
    let mut script = Vec::new();
    for n in 0..steps {
        let text = (0..120)
            .map(|line| format!("file {n} line {line:03} .................................\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(dir.path().join(format!("f{n}.txt")), text).unwrap();
        let mut step = calls("read", json!({"path": format!("f{n}.txt")}));
        step.calls[0].id = format!("c{n}");
        script.push(step);
    }
    script.push(calls(
        "done",
        json!({"outcome": "finished", "summary": "read them all"}),
    ));
    let (worker, _) = worker(dir.path(), script, Limits::default());
    let db = state_db(dir.path());
    let reader = Connection::open(&db).unwrap();
    reader
        .execute_batch("BEGIN; SELECT COUNT(*) FROM undertakings;")
        .unwrap();
    assert_eq!(worker.run().await.ended, Ended::Finished);
    written(&db)
}

#[tokio::test]
async fn a_step_writes_what_is_new_in_it_and_commits_once() {
    let (short_bytes, short_commits) = work_of(10).await;
    let (long_bytes, long_commits) = work_of(20).await;
    // The ten steps more of the longer work, each with a longer
    // transcript behind it than any of the shorter work's.
    let (bytes, commits) = (long_bytes - short_bytes, long_commits - short_commits);
    eprintln!(
        "11 steps: {short_bytes} B in {short_commits} commits; \
         21 steps: {long_bytes} B in {long_commits} commits"
    );
    assert!(
        bytes <= short_bytes,
        "ten later steps wrote {bytes} B, the first ten and the setting up {short_bytes} B"
    );
    assert_eq!(commits, 10, "one commit a step");
}
