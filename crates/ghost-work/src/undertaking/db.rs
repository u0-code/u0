//! The database underneath: its schema, and the few reads and writes
//! everything else is built on.

use std::io;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::State;

/// The database a ghost keeps her work in, beside her journal.
pub const WORK_DB: &str = "work.db";

/// The version of the schema below. Raised, with a way forward from the
/// last one, when a table changes: this is state nothing else can rebuild.
pub(super) const SCHEMA_VERSION: i64 = 2;

/// Every undertaking in one database, keyed by id. `history` is append-only
/// the way the journal is: triggers abort any change to what is written.
/// The others are bookkeeping and may be replaced; `inbox` keeps what
/// reached the work, and marks it once it has been taken.
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS undertakings (
    id    TEXT PRIMARY KEY,
    state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transcript (
    id    TEXT PRIMARY KEY,
    items TEXT NOT NULL
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
";

/// What schema 1 lacked: the kind of what is in the inbox. Everything
/// written before it was added to the task.
pub(super) const KIND_COLUMN: &str =
    "ALTER TABLE inbox ADD COLUMN kind TEXT NOT NULL DEFAULT 'added'";

pub(super) fn other(e: impl std::error::Error + Send + Sync + 'static) -> io::Error {
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
