//! Looking at work from outside while it goes on. The database is opened
//! read-only, so that looking can never get in the way of the work.

use std::{io, path::Path, time::Duration};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::{
    State,
    db::{load, other},
};

/// How one undertaking stands, as another process sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glance {
    pub id: String,
    pub steps: u64,
    pub sittings: u32,
    /// When it last took a step, if it ever did.
    pub last_step: Option<DateTime<Utc>>,
    /// Whatever it put to whoever it is for, and is waiting to hear.
    pub waiting: Option<String>,
    /// Its parts that have not finished.
    pub parts_open: usize,
}

/// How each undertaking in the database at `db` stands, for a person
/// looking from outside while the work goes on. The database is opened
/// read-only, so that looking can never get in the way of the work.
/// Parts are counted under the undertaking they belong to.
pub fn glance(db: &Path) -> io::Result<Vec<Glance>> {
    let conn = read_only(db)?;
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, state FROM undertakings ORDER BY rowid")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })
        .map_err(other)?;
    let states: Vec<(String, State)> = rows
        .into_iter()
        .filter_map(|(id, text)| Some((id, serde_json::from_str(&text).ok()?)))
        .collect();
    let mut seen = Vec::new();
    for (id, state) in states.iter().filter(|(_, s)| s.parent.is_none()) {
        seen.push(Glance {
            id: id.clone(),
            steps: state.steps,
            sittings: state.sittings,
            last_step: step_at(&conn, id, Which::Last)?,
            waiting: state.waiting.clone(),
            parts_open: states
                .iter()
                .filter(|(_, part)| part.parent.as_ref() == Some(id) && part.finish.is_none())
                .count(),
        });
    }
    Ok(seen)
}

/// One undertaking as it stands, with its parts, for someone looking from
/// outside while the work goes on.
#[derive(Debug, Clone)]
pub struct Look {
    pub id: String,
    pub state: State,
    /// When it took its first step, if it has taken one. A part waiting
    /// its turn has not.
    pub first_step: Option<DateTime<Utc>>,
    /// When it took its latest step.
    pub last_step: Option<DateTime<Utc>>,
    /// Its parts, by number. A part has none: it hands out no parts.
    pub parts: Vec<(u64, Look)>,
}

/// The undertakings `ids` in the database at `db`, each with its parts, in
/// the order asked for. An id the database does not have is left out. The
/// database is opened read-only, as [`glance`] opens it.
pub fn look(db: &Path, ids: &[String]) -> io::Result<Vec<Look>> {
    let conn = read_only(db)?;
    let one = |id: &str, state: State| -> io::Result<Look> {
        Ok(Look {
            id: id.to_owned(),
            first_step: step_at(&conn, id, Which::First)?,
            last_step: step_at(&conn, id, Which::Last)?,
            state,
            parts: Vec::new(),
        })
    };
    let mut seen = Vec::new();
    for id in ids {
        let Some(state) = load(&conn, id).map_err(other)? else {
            continue;
        };
        let mut work = one(id, state)?;
        let prefix = format!("{id}/");
        let rows: Vec<(String, String)> = conn
            .prepare(
                "SELECT substr(id, length(?1) + 1), state FROM undertakings
                 WHERE substr(id, 1, length(?1)) = ?1",
            )
            .and_then(|mut stmt| {
                stmt.query_map([&prefix], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect()
            })
            .map_err(other)?;
        for (rest, text) in rows {
            let Ok(n) = rest.parse::<u64>() else { continue };
            let Ok(state) = serde_json::from_str::<State>(&text) else {
                continue;
            };
            if state.parent.as_deref() == Some(id.as_str()) {
                work.parts.push((n, one(&format!("{prefix}{n}"), state)?));
            }
        }
        work.parts.sort_by_key(|(n, _)| *n);
        seen.push(work);
    }
    Ok(seen)
}

fn read_only(db: &Path) -> io::Result<Connection> {
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(other)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(other)?;
    Ok(conn)
}

#[derive(Clone, Copy)]
enum Which {
    First,
    Last,
}

/// When undertaking `id` took its first or its latest step, by its log.
fn step_at(conn: &Connection, id: &str, which: Which) -> io::Result<Option<DateTime<Utc>>> {
    let order = match which {
        Which::First => "ASC",
        Which::Last => "DESC",
    };
    Ok(conn
        .query_row(
            &format!("SELECT line FROM log WHERE id = ?1 ORDER BY n {order} LIMIT 1"),
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(other)?
        .and_then(|line| serde_json::from_str::<Value>(&line).ok())
        .and_then(|line| line["at"].as_str().and_then(|at| at.parse().ok())))
}
