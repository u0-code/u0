//! Looking at work from outside while it goes on. The database is opened
//! read-only, so that looking can never get in the way of the work, and
//! never makes or changes a record: a look at work that has no record
//! finds none.

use std::{collections::BTreeMap, io, path::Path};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::{
    State,
    db::{load, other, read_only},
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

/// A work database opened to read and never to write, for whoever looks
/// at work from outside while it goes on and looks often: one connection,
/// held as long as they like, that takes no lock a writer waits on.
pub struct Records {
    conn: Connection,
}

impl Records {
    /// The work database at `db`, read-only. It has to be there: a reader
    /// does not make one.
    pub fn open(db: &Path) -> io::Result<Self> {
        Ok(Self {
            conn: read_only(db)?,
        })
    }

    /// See [`glance`].
    pub fn glance(&self) -> io::Result<Vec<Glance>> {
        glance_in(&self.conn)
    }

    /// See [`look`].
    pub fn look(&self, ids: &[String]) -> io::Result<Vec<Look>> {
        look_in(&self.conn, ids)
    }

    /// See [`steps`].
    pub fn steps(&self, id: &str, after: u64, limit: usize) -> io::Result<Option<Logged>> {
        steps_in(&self.conn, id, after, limit)
    }
}

/// How each undertaking in the database at `db` stands, for a person
/// looking from outside while the work goes on. The database is opened
/// read-only, so that looking can never get in the way of the work.
/// Parts are counted under the undertaking they belong to.
pub fn glance(db: &Path) -> io::Result<Vec<Glance>> {
    Records::open(db)?.glance()
}

fn glance_in(conn: &Connection) -> io::Result<Vec<Glance>> {
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
            last_step: step_at(conn, id, Which::Last)?,
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
    Records::open(db)?.look(ids)
}

fn look_in(conn: &Connection, ids: &[String]) -> io::Result<Vec<Look>> {
    let one = |id: &str, state: State| -> io::Result<Look> {
        Ok(Look {
            id: id.to_owned(),
            first_step: step_at(conn, id, Which::First)?,
            last_step: step_at(conn, id, Which::Last)?,
            state,
            parts: Vec::new(),
        })
    };
    let mut seen = Vec::new();
    for id in ids {
        let Some(state) = load(conn, id).map_err(other)? else {
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

/// One step of a piece of work as its log has it, for someone following
/// it from outside.
#[derive(Debug, Clone, Serialize)]
pub struct Step {
    /// Its place in the log, from 1: steps over every sitting, in order.
    pub n: u64,
    /// When it was taken, RFC 3339.
    pub at: Option<String>,
    /// What the model said at that step, if anything. A command-line agent
    /// stepping the work through a server of tools says nothing here.
    pub said: String,
    /// How long the model took to answer, where the work's own loop asked
    /// it.
    pub ms: Option<u64>,
    /// What it called, in order, each as [`crate::steps::shown`] has it.
    pub calls: Vec<Value>,
}

/// Steps of one piece of work, whether there are more after them, and what
/// its newest step put in front of the model.
#[derive(Debug, Clone)]
pub struct Logged {
    pub steps: Vec<Step>,
    pub more: bool,
    pub context: Option<StepContext>,
}

/// What one step put in front of the model, and the room it had, as the
/// work's own loop logged it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepContext {
    /// The step, as [`Step::n`] has it.
    pub n: u64,
    pub at: Option<String>,
    /// Tokens: what the model read (`input`), how much of that came from
    /// the provider's cache (`cached`), and what it wrote (`output`).
    pub usage: StepUsage,
    /// What it was given, in characters, by part.
    pub parts: BTreeMap<String, u64>,
    /// The model's context window in tokens, where it is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// The size in tokens past which the transcript is folded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folds_at: Option<u64>,
}

/// Tokens one step cost, as the provider counted them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StepUsage {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

/// How many of the newest lines are looked through for the newest step
/// that says what it was given: calls a server of tools logged come
/// between, and say nothing of it.
const CONTEXT_LOOKED: i64 = 50;

/// The steps of the undertaking `id` in the database at `db` after the
/// `after`th, at most `limit` of them, oldest first; `None` if there is no
/// undertaking by that id. Opened read-only, as [`glance`] opens it.
///
/// A line the work's own loop wrote is one step. A line a server of tools
/// wrote is one call a command-line agent made, and is shown as a step of
/// its own with that one call.
pub fn steps(db: &Path, id: &str, after: u64, limit: usize) -> io::Result<Option<Logged>> {
    Records::open(db)?.steps(id, after, limit)
}

fn steps_in(conn: &Connection, id: &str, after: u64, limit: usize) -> io::Result<Option<Logged>> {
    if load(conn, id).map_err(other)?.is_none() {
        return Ok(None);
    }
    let after = i64::try_from(after).unwrap_or(i64::MAX);
    let wanted = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT n, line FROM log WHERE id = ?1 AND n > ?2 ORDER BY n LIMIT ?3")
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![id, after, wanted], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect()
        })
        .map_err(other)?;
    let more = rows.len() > limit;
    let steps = rows
        .into_iter()
        .take(limit)
        .map(|(n, line)| {
            let line: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
            let calls = match line["calls"].as_array() {
                Some(calls) => calls.iter().map(crate::steps::shown).collect(),
                None if line.get("name").is_some() => vec![crate::steps::shown(&line)],
                None => Vec::new(),
            };
            Step {
                n: u64::try_from(n).unwrap_or_default(),
                at: line["at"].as_str().map(str::to_owned),
                said: line["said"].as_str().unwrap_or_default().to_owned(),
                ms: line["model_ms"].as_u64(),
                calls,
            }
        })
        .collect();
    let context = newest_context(conn, id)?;
    Ok(Some(Logged {
        steps,
        more,
        context,
    }))
}

/// What the newest step of undertaking `id` that says so put in front of
/// the model. A step logged before steps kept what they were given says
/// only how many tokens were read: that is still what the window held,
/// told without its parts or the window's size.
fn newest_context(conn: &Connection, id: &str) -> io::Result<Option<StepContext>> {
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT n, line FROM log WHERE id = ?1 ORDER BY n DESC LIMIT ?2")
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![id, CONTEXT_LOOKED], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect()
        })
        .map_err(other)?;
    Ok(rows.into_iter().find_map(|(n, line)| {
        let line: Value = serde_json::from_str(&line).ok()?;
        let parts = if let Some(context) = line.get("context") {
            serde_json::from_value(context.clone()).ok()?
        } else {
            line.get("read")?.as_u64()?;
            BTreeMap::new()
        };
        Some(StepContext {
            n: u64::try_from(n).unwrap_or_default(),
            at: line["at"].as_str().map(str::to_owned),
            usage: StepUsage {
                input: line["read"].as_u64().unwrap_or(0),
                cached: line["cached"].as_u64().unwrap_or(0),
                output: line["wrote"].as_u64().unwrap_or(0),
            },
            parts,
            window: line["window"].as_u64(),
            folds_at: line["folds_at"].as_u64(),
        })
    }))
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
