//! The database underneath: its schema, and the few reads and writes
//! everything else is built on.

use std::{fs, io, path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::State;

/// The database a ghost keeps her work in, beside her journal.
pub const WORK_DB: &str = "work.db";

/// The version of the schema below, raised when a table changes. This is
/// state nothing else can rebuild, and a database of another version is
/// not read: it is refused, and nothing in it is changed.
pub(super) const SCHEMA_VERSION: i64 = 4;

/// Every undertaking in one database, keyed by id. `history` is append-only
/// the way the journal is: triggers abort any change to what is written.
/// A row of `transcript_items` holds its item, or, when the item is as the
/// history keeps it, the number of its row there: a step writes what is
/// new in it, not the transcript again.
/// The others are bookkeeping and may be replaced; `inbox` keeps what
/// reached the work, and marks it once it has been taken.
///
/// `looks` and `look_rounds` are not work: they are what she called while
/// she looked at a conversation, kept for a fortnight (see
/// [`crate::looks`]). They are here because this is the database whose
/// state is working state, neither forever nor rebuilt.
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS undertakings (
    id    TEXT PRIMARY KEY,
    state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transcript_items (
    id      TEXT    NOT NULL,
    n       INTEGER NOT NULL,
    item    TEXT,
    history INTEGER,
    PRIMARY KEY (id, n)
);
CREATE TABLE IF NOT EXISTS history (
    id   TEXT    NOT NULL,
    n    INTEGER NOT NULL,
    item TEXT    NOT NULL,
    PRIMARY KEY (id, n)
);
CREATE TRIGGER IF NOT EXISTS history_no_update BEFORE UPDATE ON history
BEGIN SELECT RAISE(ABORT, 'history is append-only'); END;
CREATE TRIGGER IF NOT EXISTS history_no_delete BEFORE DELETE ON history
BEGIN SELECT RAISE(ABORT, 'history is append-only'); END;
CREATE TABLE IF NOT EXISTS log (
    id   TEXT    NOT NULL,
    n    INTEGER NOT NULL,
    line TEXT    NOT NULL,
    PRIMARY KEY (id, n)
);
CREATE TABLE IF NOT EXISTS inbox (
    id    TEXT    NOT NULL,
    n     INTEGER NOT NULL,
    text  TEXT    NOT NULL,
    taken INTEGER NOT NULL DEFAULT 0,
    kind  TEXT    NOT NULL DEFAULT 'added',
    PRIMARY KEY (id, n)
);
CREATE TABLE IF NOT EXISTS looks (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    place TEXT    NOT NULL,
    at    INTEGER NOT NULL,
    seq   INTEGER
);
CREATE INDEX IF NOT EXISTS looks_by_place ON looks (place, id);
CREATE TABLE IF NOT EXISTS look_rounds (
    look  INTEGER NOT NULL,
    n     INTEGER NOT NULL,
    round TEXT    NOT NULL,
    PRIMARY KEY (look, n)
);
";

/// Open the work database at `db` for writing, making it first if it is
/// new. Several handles write here at once, from more than one process,
/// so a writer waits its turn rather than failing.
pub(crate) fn open(db: &Path) -> io::Result<Connection> {
    if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(db).map_err(other)?;
    conn.busy_timeout(Duration::from_secs(10)).map_err(other)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(other)?;
    // A database already at this schema is opened without a write: every
    // sitting and every word passed on to work opens one, and a write here
    // would wait on the worker for the lock to change nothing.
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(other)?;
    if version == SCHEMA_VERSION {
        return Ok(conn);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(other)?;
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(other)?;
    if version == SCHEMA_VERSION {
        // Another handle made it while this one waited.
        drop(tx);
        return Ok(conn);
    }
    let tables: i64 = tx
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
        .map_err(other)?;
    if version != 0 || tables != 0 {
        return Err(io::Error::other(format!(
            "{} is work kept in schema {version}, and this ghost-work reads only schema \
             {SCHEMA_VERSION}; move it aside to start her work afresh",
            db.display()
        )));
    }
    tx.execute_batch(SCHEMA).map_err(other)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(other)?;
    tx.commit().map_err(other)?;
    Ok(conn)
}

/// Open the work database at `db` to look, never to write, so that
/// looking can never get in the way of the work.
pub(crate) fn read_only(db: &Path) -> io::Result<Connection> {
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(other)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(other)?;
    Ok(conn)
}

pub(crate) fn other(e: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(e)
}

pub(super) fn load(conn: &Connection, id: &str) -> rusqlite::Result<Option<State>> {
    conn.query_row(
        "SELECT state FROM undertakings WHERE id = ?1",
        [id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|text| {
        serde_json::from_str(&text).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

pub(super) fn store(conn: &Connection, id: &str, state: &State) -> rusqlite::Result<()> {
    let text = serde_json::to_string(state)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO undertakings (id, state) VALUES (?1, ?2)
         ON CONFLICT (id) DO UPDATE SET state = excluded.state",
        params![id, text],
    )?;
    Ok(())
}

/// Append `rows` to `table` for `id`, numbered on from the last one. The
/// caller holds a write transaction, so two writers cannot take one number.
pub(super) fn append(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    id: &str,
    rows: impl IntoIterator<Item = String>,
) -> rusqlite::Result<()> {
    let last: i64 = tx.query_row(
        &format!("SELECT COALESCE(MAX(n), 0) FROM {table} WHERE id = ?1"),
        [id],
        |row| row.get(0),
    )?;
    let mut insert = tx.prepare(&format!(
        "INSERT INTO {table} (id, n, {column}) VALUES (?1, ?2, ?3)"
    ))?;
    for (n, row) in (last + 1..).zip(rows) {
        insert.execute(params![id, n, row])?;
    }
    Ok(())
}
