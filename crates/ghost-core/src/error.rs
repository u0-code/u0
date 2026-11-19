use std::path::PathBuf;

use thiserror::Error;

/// Everything that can go wrong inside the core.
#[derive(Debug, Error)]
pub enum Error {
    /// The persona on disk no longer matches the seal she was born with.
    /// There is no flag to override this.
    #[error("this is not her: {0}")]
    NotHer(String),

    #[error("{0} already holds a ghost; a ghost is born once")]
    AlreadyBorn(PathBuf),

    /// Another process is running this ghost. One body at a time.
    #[error("{0} is already awake in another process")]
    Busy(PathBuf),

    #[error("no persona at {0}")]
    NoSeed(PathBuf),

    #[error("persona is malformed: {0}")]
    BadSeed(String),

    #[error("journal chain broken at seq {seq}: {reason}")]
    BrokenChain { seq: i64, reason: String },

    /// An entry is in her journal but her derived memory could not fold it
    /// in. It is still hers. Memory folds nothing after it until it is, and
    /// tries again at her next write and whenever she is opened.
    #[error("her memory could not fold entry {seq} ({kind}) of her journal: {reason}")]
    Unfolded {
        seq: i64,
        kind: &'static str,
        reason: String,
    },

    #[error("journal at {0} does not exist")]
    NoJournal(PathBuf),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Db(#[from] rusqlite::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
