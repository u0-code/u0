//! One thing being done, and everything about it that has to outlast a
//! context window.
//!
//! A transcript is short-lived: it fills up, it is folded away, the process
//! running it can stop at any moment. An undertaking is what remains: what
//! was asked, the plan as it stands, the notes written along the way, and how
//! it ended. It is kept in a `SQLite` database, `work.db`, that holds every
//! undertaking of one ghost by id, and any sitting can be picked up from it.
//!
//! ```text
//! undertakings   brief, plan, notes, outcome, running totals
//! transcript     what the model is shown now, saved after every step
//! history        every item the transcript ever held; append-only
//! log            one line per step: what was called, what it cost
//! inbox          what reached it from outside while it ran, of what kind,
//!                and whether it has been taken
//! ```
//!
//! What goes the other way, from the work to whoever it is for while it
//! runs, is part of the state: what it told them (`told`) and what it kept
//! (`kept`), oldest first. Whoever the work is for reads those lists and
//! takes what it has not had yet; the worker only appends to them, and
//! never reads them back.
//!
//! A part of a piece of work, `<id>/<n>`, is an undertaking like any other,
//! and its parent is whoever it is for. What it says to its parent goes
//! into the parent's inbox, beside what was added to the parent's task, so
//! that a worker hears from its parts the way it hears from outside.
//!
//! Only the bookkeeping is kept there. The files the work produces are
//! wherever its machine put them. Images a tool showed the model are kept
//! beside the database, under `media/` and named by their hash; the history
//! names them and does not hold them, and the saved transcript holds the
//! name of each one still in front of the model, to be read back from there.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use chrono::{DateTime, Utc};
use ghost_brain::{Item, Picture, Usage};
use ghost_core::media;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Todo,
    Doing,
    Done,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub status: Progress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: DateTime<Utc>,
    pub text: String,
}

/// How an undertaking ended, in the worker's own judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Done, and checked.
    Finished,
    /// Something outside the worker's control is in the way.
    Blocked,
    /// Out of ideas.
    GaveUp,
    /// Ended from outside before it was done: whoever it is for no longer
    /// wants it. What it kept and told stays theirs.
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finish {
    pub outcome: Outcome,
    pub summary: String,
    /// The files that are the result, where the result is files: paths as
    /// the machine knows them.
    #[serde(default)]
    pub files: Vec<String>,
    /// What doing this taught about doing things like it, if anything.
    #[serde(default)]
    pub lesson: Option<String>,
    pub at: DateTime<Utc>,
}

/// An image the worker was shown and kept, to be had again after the work
/// is over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// Its name under `media/`, beside the database.
    pub file: String,
    /// What it is, and when it would be wanted again.
    pub note: String,
    pub at: DateTime<Utc>,
}

/// Something the worker told whoever the work is for while it ran: worth
/// their knowing before the work is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Told {
    pub text: String,
    /// Files it made that are ready to hand over: paths as the machine
    /// knows them.
    #[serde(default)]
    pub files: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// What was asked, as it was asked.
    pub brief: String,
    pub plan: Vec<PlanStep>,
    pub notes: Vec<Note>,
    /// What the worker wrote for itself the last time its context was
    /// cleared. One document, replaced each time.
    #[serde(default)]
    pub handoff: Option<String>,
    pub finish: Option<Finish>,
    /// Files written or edited most recently, newest last.
    #[serde(default)]
    pub touched: Vec<String>,
    /// Things added to the task after it began, in order. The brief itself
    /// never changes; these extend it.
    #[serde(default)]
    pub additions: Vec<String>,
    /// The session of a command-line agent that has been stepping this
    /// work, so its next sitting can pick its own context back up.
    #[serde(default)]
    pub session: Option<String>,
    /// A question the worker put to whoever the work is for. The work
    /// waits here until something is added to the task from outside.
    #[serde(default)]
    pub waiting: Option<String>,
    /// The model once refused a context of about this many tokens. The
    /// transcript is kept well under it from then on.
    #[serde(default)]
    pub refused_at: Option<u64>,
    /// What the worker kept, in the order it was kept. Keeping a file again
    /// replaces its note and moves it to the end.
    #[serde(default)]
    pub kept: Vec<Kept>,
    /// What the worker told whoever the work is for, in the order it told
    /// them. Only ever appended to.
    #[serde(default)]
    pub told: Vec<Told>,
    /// The undertaking this is a part of, for a part handed out by a
    /// worker. Helpers from before parts ran on their own have none, and
    /// are not counted as parts: each of them ended in the step that
    /// started it.
    #[serde(default)]
    pub parent: Option<String>,
    /// How many times work was picked up with a fresh context.
    pub sittings: u32,
    pub steps: u64,
    pub usage: Usage,
    pub seconds: f64,
}

impl State {
    /// What was asked, with whatever was added to it since.
    pub fn task_text(&self) -> String {
        let mut text = self.brief.trim().to_owned();
        for added in &self.additions {
            text.push_str(&format!("\n\nAdded later: {}", added.trim()));
        }
        text
    }

    /// The plan as the worker should see it.
    pub fn plan_text(&self) -> String {
        if self.plan.is_empty() {
            return "(no plan written yet)".to_owned();
        }
        self.plan
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let mark = match step.status {
                    Progress::Todo => "[ ]",
                    Progress::Doing => "[>]",
                    Progress::Done => "[x]",
                    Progress::Dropped => "[-]",
                };
                format!("{mark} {}. {}", i + 1, step.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Where the work stands, in a sentence or two: for whoever asks how it
    /// is going between sittings.
    pub fn progress(&self) -> String {
        let done = self
            .plan
            .iter()
            .filter(|s| s.status == Progress::Done)
            .count();
        let now = self
            .plan
            .iter()
            .find(|s| s.status == Progress::Doing)
            .or_else(|| self.plan.iter().find(|s| s.status == Progress::Todo));
        let mut out = String::new();
        if !self.plan.is_empty() {
            out = format!("{done} of {} steps done", self.plan.len());
            if let Some(step) = now {
                out.push_str(&format!("; next: {}", step.text));
            }
            out.push('.');
        }
        if let Some(note) = self.notes.last() {
            if !out.is_empty() {
                out.push(' ');
            }
            let text: String = note.text.chars().take(240).collect();
            out.push_str(&format!("Latest note: {text}"));
        }
        if out.is_empty() {
            "Started; nothing written down yet.".to_owned()
        } else {
            out
        }
    }

    /// The latest thing it has to say for itself: its report if it has
    /// ended, the question it is waiting on, what it last told, or where it
    /// stands.
    pub fn latest(&self) -> String {
        if let Some(finish) = self
            .finish
            .as_ref()
            .filter(|f| !f.summary.trim().is_empty())
        {
            return finish.summary.trim().to_owned();
        }
        if let Some(question) = &self.waiting {
            return question.trim().to_owned();
        }
        match self.told.last() {
            Some(told) => told.text.trim().to_owned(),
            None => self.progress(),
        }
    }

    /// The newest notes that fit in `budget` characters, oldest first.
    pub fn notes_text(&self, budget: usize) -> String {
        if self.notes.is_empty() {
            return "(no notes yet)".to_owned();
        }
        let mut kept = Vec::new();
        let mut used = 0;
        for note in self.notes.iter().rev() {
            let line = format!("- {}", note.text.trim());
            if !kept.is_empty() && used + line.len() > budget {
                break;
            }
            used += line.len();
            kept.push(line);
        }
        let left_out = self.notes.len() - kept.len();
        kept.reverse();
        if left_out > 0 {
            kept.insert(0, format!("({left_out} older notes are not shown)"));
        }
        kept.join("\n")
    }
}

/// The database a ghost keeps her work in, beside her journal.
pub const WORK_DB: &str = "work.db";

/// The version of the schema below. Raised, with a way forward from the
/// last one, when a table changes: this is state nothing else can rebuild.
const SCHEMA_VERSION: i64 = 2;

/// Every undertaking in one database, keyed by id. `history` is append-only
/// the way the journal is: triggers abort any change to what is written.
/// The others are bookkeeping and may be replaced; `inbox` keeps what
/// reached the work, and marks it once it has been taken.
const SCHEMA: &str = "
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
const KIND_COLUMN: &str = "ALTER TABLE inbox ADD COLUMN kind TEXT NOT NULL DEFAULT 'added'";

/// What reached a piece of work from outside while it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailKind {
    /// Added to its task, by whoever it is for.
    Added,
    /// Something one of its parts told it.
    Told,
    /// A question one of its parts is waiting on.
    Asked,
    /// One of its parts ended, and this is its report.
    Ended,
}

impl MailKind {
    fn word(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Told => "told",
            Self::Asked => "asked",
            Self::Ended => "ended",
        }
    }

    fn from_word(word: &str) -> Self {
        match word {
            "told" => Self::Told,
            "asked" => Self::Asked,
            "ended" => Self::Ended,
            _ => Self::Added,
        }
    }
}

/// One thing from the inbox. What came from a part is written as the
/// worker is to read it: who it is from and what kind of thing it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub kind: MailKind,
    pub text: String,
}

/// One undertaking in a work database, with a connection of its own.
///
/// Others may hold the same undertaking at the same time: a helper's parent,
/// the mind adding to it, or the tool server a command-line agent starts in
/// another process. So every change is read, made and written inside one
/// write transaction, and [`Undertaking::state`] reads what is in the
/// database, not what this handle last wrote.
pub struct Undertaking {
    db: PathBuf,
    id: String,
    conn: Mutex<Connection>,
    /// The state as last read or written: what is shown if the database
    /// cannot be read, so that work goes on without its record.
    state: Mutex<State>,
}

/// The line that stands for a picture wherever its bytes are not kept.
pub fn placeholder(file: &str) -> String {
    format!("[image: {file}]")
}

/// An item as the history keeps it: a result's pictures become the lines
/// that name their files, where those are not already in its output.
fn without_pictures(item: &Item) -> std::borrow::Cow<'_, Item> {
    let Item::Result {
        id,
        name,
        output,
        failed,
        pictures,
    } = item
    else {
        return std::borrow::Cow::Borrowed(item);
    };
    if pictures.is_empty() {
        return std::borrow::Cow::Borrowed(item);
    }
    let mut output = output.clone();
    for p in pictures {
        let line = placeholder(&media::name(&p.bytes, &p.mime));
        if !output.contains(&line) {
            output.push('\n');
            output.push_str(&line);
        }
    }
    std::borrow::Cow::Owned(Item::result(id, name, output, *failed))
}

/// An item from a saved transcript. A picture is either a reference to a
/// file under `root/media/`, which is read back, or, as this was first
/// written, its bytes in base64. A reference whose file is gone is dropped,
/// and the result says so.
fn from_disk(mut saved: Value, root: &Path) -> Option<Item> {
    let written = saved
        .as_object_mut()
        .and_then(|item| item.remove("pictures"));
    let mut item: Item = serde_json::from_value(saved).ok()?;
    let (
        Some(Value::Array(written)),
        Item::Result {
            output, pictures, ..
        },
    ) = (written, &mut item)
    else {
        return Some(item);
    };
    for picture in written {
        let Some(file) = picture["file"].as_str() else {
            pictures.push(serde_json::from_value(picture).ok()?);
            continue;
        };
        // Only a bare name is looked for, and only under `media/`.
        let bytes = Path::new(file)
            .file_name()
            .filter(|bare| *bare == file)
            .and_then(|_| fs::read(root.join(media::MEDIA_DIR).join(file)).ok());
        match bytes {
            Some(bytes) => pictures.push(Picture {
                mime: picture["mime"].as_str().unwrap_or_default().to_owned(),
                bytes,
            }),
            None => output.push_str(&format!(
                "\n[The image {file} is no longer on disk, so it cannot be shown.]"
            )),
        }
    }
    Some(item)
}

fn other(e: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(e)
}

fn load(conn: &Connection, id: &str) -> rusqlite::Result<Option<State>> {
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

fn store(conn: &Connection, id: &str, state: &State) -> rusqlite::Result<()> {
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
fn append(
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

impl Undertaking {
    /// Start the undertaking `id` in the database at `db`, or pick up what
    /// is already there. A brief is only taken the first time: what was
    /// asked does not change.
    pub fn open(db: &Path, id: &str, brief: Option<&str>) -> io::Result<Self> {
        if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(db).map_err(other)?;
        // Several handles write here at once, from more than one process.
        // A writer waits its turn rather than failing.
        conn.busy_timeout(Duration::from_secs(10)).map_err(other)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(other)?;
        let state = {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(other)?;
            let version: i64 = tx
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(other)?;
            if version > SCHEMA_VERSION {
                return Err(io::Error::other(format!(
                    "{} was written by a newer ghost-work (schema {version})",
                    db.display()
                )));
            }
            tx.execute_batch(SCHEMA).map_err(other)?;
            let has_kind: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('inbox') WHERE name = 'kind'",
                    [],
                    |row| row.get(0),
                )
                .map_err(other)?;
            if has_kind == 0 {
                tx.execute_batch(KIND_COLUMN).map_err(other)?;
            }
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(other)?;
            let state = if let Some(state) = load(&tx, id).map_err(other)? {
                state
            } else {
                let brief = brief
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "a new undertaking needs a brief",
                        )
                    })?;
                let state = State {
                    brief: brief.to_owned(),
                    ..State::default()
                };
                store(&tx, id, &state).map_err(other)?;
                state
            };
            tx.commit().map_err(other)?;
            state
        };
        Ok(Self {
            db: db.to_path_buf(),
            id: id.to_owned(),
            conn: Mutex::new(conn),
            state: Mutex::new(state),
        })
    }

    /// Start a new undertaking that is part of this one, as `<id>/<n>`,
    /// with `n` one past the highest part already in the database.
    ///
    /// The number comes from what is on record, not from a count kept in
    /// memory: a count starts again with every sitting, and the second
    /// sitting's first part would reopen the first sitting's, finished
    /// already, and hand back its old report for new work. The id is taken
    /// in the same write transaction that finds it, so parts started side
    /// by side, or from another process, each get one of their own.
    pub fn open_part(&self, brief: &str) -> io::Result<Self> {
        let brief = brief.trim();
        if brief.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a new undertaking needs a brief",
            ));
        }
        let id = {
            let mut conn = self.conn();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(other)?;
            let prefix = format!("{}/", self.id);
            let last = {
                let mut query = tx
                    .prepare(
                        "SELECT substr(id, length(?1) + 1) FROM undertakings
                         WHERE substr(id, 1, length(?1)) = ?1",
                    )
                    .map_err(other)?;
                query
                    .query_map([&prefix], |row| row.get::<_, String>(0))
                    .map_err(other)?
                    .filter_map(|rest| rest.ok()?.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0)
            };
            let id = format!("{prefix}{}", last + 1);
            let state = State {
                brief: brief.to_owned(),
                parent: Some(self.id.clone()),
                ..State::default()
            };
            store(&tx, &id, &state).map_err(other)?;
            tx.commit().map_err(other)?;
            id
        };
        Self::open(&self.db, &id, Some(brief))
    }

    /// The database this undertaking is kept in.
    pub fn db(&self) -> &Path {
        &self.db
    }

    /// Its key in that database.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its parts, by number: every undertaking started with
    /// [`Undertaking::open_part`] as part of this one, as they are now.
    pub fn parts(&self) -> Vec<(u64, State)> {
        let prefix = format!("{}/", self.id);
        let conn = self.conn();
        let read = conn
            .prepare(
                "SELECT substr(id, length(?1) + 1), state FROM undertakings
                 WHERE substr(id, 1, length(?1)) = ?1",
            )
            .and_then(|mut query| {
                query
                    .query_map([&prefix], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            });
        let rows = read.unwrap_or_else(|e| {
            tracing::warn!("could not read the parts of {}: {e}", self.id);
            Vec::new()
        });
        let mut parts: Vec<(u64, State)> = rows
            .into_iter()
            .filter_map(|(rest, state)| {
                let n = rest.parse::<u64>().ok()?;
                let state: State = serde_json::from_str(&state).ok()?;
                (state.parent.as_deref() == Some(self.id.as_str())).then_some((n, state))
            })
            .collect();
        parts.sort_by_key(|(n, _)| *n);
        parts
    }

    /// Its part numbered `n`, with a handle of its own.
    pub fn part(&self, n: u64) -> io::Result<Self> {
        let part = Self::open(&self.db, &format!("{}/{n}", self.id), None)?;
        if part.state().parent.as_deref() != Some(self.id.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} has no part {n}", self.id),
            ));
        }
        Ok(part)
    }

    /// The number this undertaking has among its parent's parts, if it is
    /// a part.
    pub fn number(&self) -> Option<u64> {
        self.id.rsplit_once('/')?.1.parse().ok()
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn cached(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The state as it is in the database now, whoever wrote it.
    pub fn state(&self) -> State {
        let read = load(&self.conn(), &self.id);
        match read {
            Ok(Some(state)) => {
                *self.cached() = state.clone();
                state
            }
            Ok(None) => self.cached().clone(),
            Err(e) => {
                tracing::warn!("could not read the undertaking {}: {e}", self.id);
                self.cached().clone()
            }
        }
    }

    /// Change the state and write it down, in one transaction, so that a
    /// change made meanwhile through another handle is built on and not
    /// lost. A failed write is logged, not fatal: the work in hand matters
    /// more than the record of it.
    pub fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> T {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate);
        let base = match &tx {
            Ok(tx) => load(tx, &self.id),
            Err(_) => Ok(None),
        };
        let readable = base.is_ok();
        let mut state = match base {
            Ok(Some(state)) => state,
            Ok(None) => self.cached().clone(),
            Err(e) => {
                tracing::warn!("could not read the undertaking {}: {e}", self.id);
                self.cached().clone()
            }
        };
        let out = change(&mut state);
        let saved = match tx {
            // What could not be read is not written over.
            Ok(tx) if readable => store(&tx, &self.id, &state).and_then(|()| tx.commit()),
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = saved {
            tracing::warn!("could not save the undertaking {}: {e}", self.id);
        }
        *self.cached() = state;
        out
    }

    /// Where pictures are kept: beside the database.
    fn media_root(&self) -> &Path {
        self.db.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Where the file `name` is kept under `media/`, if it is there. Only
    /// a bare name is looked for: a path is never followed.
    pub fn media(&self, name: &str) -> Option<PathBuf> {
        let bare = !name.starts_with('.')
            && Path::new(name)
                .file_name()
                .is_some_and(|n| n.to_str() == Some(name));
        let path = self.media_root().join(media::MEDIA_DIR).join(name);
        (bare && path.is_file()).then_some(path)
    }

    /// The sitting in progress, if one was left unfinished. Pictures saved
    /// as references are read back from `media/`.
    pub fn transcript(&self) -> Option<Vec<Item>> {
        let text: String = self
            .conn()
            .query_row(
                "SELECT items FROM transcript WHERE id = ?1",
                [&self.id],
                |row| row.get(0),
            )
            .optional()
            .ok()??;
        let saved: Vec<Value> = serde_json::from_str(&text).ok()?;
        saved
            .into_iter()
            .map(|item| from_disk(item, self.media_root()))
            .collect()
    }

    /// Replace the transcript with `items`. Done after every step, so that
    /// a process stopped anywhere can be picked up from the last one.
    ///
    /// It is rewritten whole every time, so it holds no image bytes: each
    /// picture is kept under `media/` first, and saved as the name of its
    /// file.
    pub fn save_transcript(&self, items: &[Item]) {
        let saved = items
            .iter()
            .map(|item| self.to_disk(item))
            .collect::<Result<Vec<Value>, _>>()
            .and_then(|items| serde_json::to_string(&items))
            .map_err(other)
            .and_then(|text| {
                self.conn()
                    .execute(
                        "INSERT INTO transcript (id, items) VALUES (?1, ?2)
                     ON CONFLICT (id) DO UPDATE SET items = excluded.items",
                        params![self.id, text],
                    )
                    .map_err(other)
            });
        if let Err(e) = saved {
            tracing::warn!("could not save the transcript: {e}");
        }
    }

    /// Keep images a tool returned beside this database, under `media/`,
    /// the way a ghost keeps what she is shown. Returns the name each one
    /// is kept under, which names the same bytes whether or not keeping
    /// them worked.
    pub fn keep(&self, pictures: &[Picture]) -> Vec<String> {
        pictures
            .iter()
            .map(
                |p| match media::store(self.media_root(), &p.bytes, &p.mime) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!("could not keep an image: {e}");
                        media::name(&p.bytes, &p.mime)
                    }
                },
            )
            .collect()
    }

    /// An item as the saved transcript holds it. A picture is kept before
    /// it is named, so a name on disk always has its file; one that cannot
    /// be kept is left out, and its result says so.
    fn to_disk(&self, item: &Item) -> serde_json::Result<Value> {
        let Item::Result {
            id,
            name,
            output,
            failed,
            pictures,
        } = item
        else {
            return serde_json::to_value(item);
        };
        if pictures.is_empty() {
            return serde_json::to_value(item);
        }
        let mut output = output.clone();
        let mut refs = Vec::new();
        for p in pictures {
            match media::store(self.media_root(), &p.bytes, &p.mime) {
                Ok(file) => refs.push(json!({"mime": p.mime, "file": file})),
                Err(e) => {
                    tracing::warn!("could not keep an image: {e}");
                    output.push_str("\n[An image could not be kept on disk, and was dropped.]");
                }
            }
        }
        let mut v = serde_json::to_value(Item::result(id, name, output, *failed))?;
        if !refs.is_empty() {
            v["pictures"] = Value::Array(refs);
        }
        Ok(v)
    }

    /// Add items to the history, in the order they entered the transcript.
    /// The transcript is trimmed and cleared as it fills; this is the copy
    /// nothing is ever taken out of.
    ///
    /// Pictures are not written here: the bytes are under `media/`, and the
    /// history says which file each one is with a line `[image: <file>]`.
    pub fn remember(&self, items: &[Item]) {
        if items.is_empty() {
            return;
        }
        let rows: Result<Vec<String>, _> = items
            .iter()
            .map(|item| serde_json::to_string(&without_pictures(item)))
            .collect();
        let written = rows
            .map_err(other)
            .and_then(|rows| self.appending("history", "item", rows).map_err(other));
        if let Err(e) = written {
            tracing::warn!("could not add to the history: {e}");
        }
    }

    fn appending(
        &self,
        table: &str,
        column: &str,
        rows: impl IntoIterator<Item = String>,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        append(&tx, table, column, &self.id, rows)?;
        tx.commit()
    }

    /// Everything the transcript ever held, in order.
    pub fn history(&self) -> Vec<Item> {
        self.column("SELECT item FROM history WHERE id = ?1 ORDER BY n")
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }

    fn column(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let read = conn.prepare(sql).and_then(|mut query| {
            query
                .query_map([&self.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()
        });
        read.unwrap_or_else(|e| {
            tracing::warn!("could not read the undertaking {}: {e}", self.id);
            Vec::new()
        })
    }

    /// Add something to the task from outside, while it may be under way.
    /// Whoever is working on this may hold another handle, in another
    /// process; the worker picks it up before its next step.
    pub fn add(&self, text: &str) -> io::Result<()> {
        self.post(MailKind::Added, text)
    }

    /// Put something in the inbox, to be taken before the next step of
    /// whoever works on this.
    pub fn post(&self, kind: MailKind, text: &str) -> io::Result<()> {
        let posted = (|| -> rusqlite::Result<()> {
            let mut conn = self.conn();
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let last: i64 = tx.query_row(
                "SELECT COALESCE(MAX(n), 0) FROM inbox WHERE id = ?1",
                [&self.id],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO inbox (id, n, text, kind) VALUES (?1, ?2, ?3, ?4)",
                params![self.id, last + 1, text.trim(), kind.word()],
            )?;
            tx.commit()
        })();
        posted.map_err(other)
    }

    /// How many things in the inbox have not been taken yet.
    pub fn pending(&self) -> usize {
        self.conn()
            .query_row(
                "SELECT COUNT(*) FROM inbox WHERE id = ?1 AND taken = 0",
                [&self.id],
                |row| row.get::<_, i64>(0),
            )
            .map_or(0, |n| usize::try_from(n).unwrap_or(0))
    }

    /// Take what has reached the work since the last look, oldest first.
    /// What was added becomes part of the task. Taking it and adding it to
    /// the task are one transaction: whichever handle takes a line takes it
    /// alone, and only once.
    pub fn take_inbox(&self) -> Vec<Mail> {
        let mut conn = self.conn();
        let taken = (|| -> rusqlite::Result<(Vec<Mail>, Option<State>)> {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let lines: Vec<Mail> = {
                let mut query = tx.prepare(
                    "SELECT text, kind FROM inbox WHERE id = ?1 AND taken = 0 ORDER BY n",
                )?;
                query
                    .query_map([&self.id], |row| {
                        Ok(Mail {
                            text: row.get(0)?,
                            kind: MailKind::from_word(&row.get::<_, String>(1)?),
                        })
                    })?
                    .collect::<rusqlite::Result<_>>()?
            };
            if lines.is_empty() {
                return Ok((lines, None));
            }
            tx.execute(
                "UPDATE inbox SET taken = 1 WHERE id = ?1 AND taken = 0",
                [&self.id],
            )?;
            let mail: Vec<Mail> = lines.into_iter().filter(|m| !m.text.is_empty()).collect();
            let mut state = load(&tx, &self.id)?.unwrap_or_else(|| self.cached().clone());
            state.additions.extend(
                mail.iter()
                    .filter(|m| m.kind == MailKind::Added)
                    .map(|m| m.text.clone()),
            );
            store(&tx, &self.id, &state)?;
            tx.commit()?;
            Ok((mail, Some(state)))
        })();
        match taken {
            Ok((mail, state)) => {
                if let Some(state) = state {
                    *self.cached() = state;
                }
                mail
            }
            Err(e) => {
                tracing::warn!("could not take what reached the work: {e}");
                Vec::new()
            }
        }
    }

    /// End the work from outside, as `dropped`, unless it has already
    /// ended. A worker stepping it sees this before its next step, and a
    /// tool server refuses every call after it. Its parts end with it.
    /// Returns whether it was still going.
    pub fn stop(&self) -> bool {
        let stopped = self.update(|state| {
            if state.finish.is_some() {
                return false;
            }
            state.finish = Some(Finish {
                outcome: Outcome::Dropped,
                summary: String::new(),
                files: Vec::new(),
                lesson: None,
                at: Utc::now(),
            });
            true
        });
        self.stop_parts();
        stopped
    }

    /// End every part that has not ended, as `dropped`.
    pub fn stop_parts(&self) {
        for (n, state) in self.parts() {
            if state.finish.is_none() {
                match self.part(n) {
                    Ok(part) => {
                        part.stop();
                    }
                    Err(e) => tracing::warn!("could not stop part {n} of {}: {e}", self.id),
                }
            }
        }
    }

    /// Append one line to the step log.
    pub fn log(&self, line: &serde_json::Value) {
        if let Err(e) = self.appending("log", "line", [line.to_string()]) {
            tracing::warn!("could not write the step log: {e}");
        }
    }

    /// The step log, oldest line first.
    pub fn logged(&self) -> Vec<serde_json::Value> {
        self.column("SELECT line FROM log WHERE id = ?1 ORDER BY n")
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Undertaking {
        /// The words of what was taken from the inbox, whatever their kind.
        fn taken(&self) -> Vec<String> {
            self.take_inbox().into_iter().map(|m| m.text).collect()
        }
    }

    fn db(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(WORK_DB)
    }

    #[test]
    fn an_undertaking_is_picked_up_from_the_database_after_its_handle_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
        first.update(|s| {
            s.plan.push(PlanStep {
                text: "look around".into(),
                status: Progress::Done,
            });
            s.notes.push(Note {
                at: Utc::now(),
                text: "python is 3.12".into(),
            });
        });
        first.save_transcript(&[Item::user("build the thing")]);
        first.remember(&[Item::user("build the thing"), Item::user("and test it")]);
        first.log(&serde_json::json!({"step": 1}));
        drop(first);

        let again = Undertaking::open(&db(&dir), "u1", Some("something else")).unwrap();
        let state = again.state();
        assert_eq!(state.brief, "build the thing", "the brief does not change");
        assert_eq!(state.plan_text(), "[x] 1. look around");
        assert_eq!(state.notes_text(1000), "- python is 3.12");
        assert_eq!(again.transcript().unwrap().len(), 1);
        assert_eq!(again.history().len(), 2);
        assert_eq!(again.logged(), [serde_json::json!({"step": 1})]);
    }

    #[test]
    fn a_part_is_numbered_on_from_the_parts_on_record_and_only_its_own_parent_counts() {
        let dir = tempfile::tempdir().unwrap();
        let parent = Undertaking::open(&db(&dir), "main", Some("the whole")).unwrap();
        assert_eq!(parent.open_part("first").unwrap().id(), "main/1");
        // A sibling whose id merely begins the same way is not a part.
        let other = Undertaking::open(&db(&dir), "mainline", Some("other")).unwrap();
        other.open_part("elsewhere").unwrap();
        drop(parent);

        // A new handle, as in a later sitting, counts from the database.
        let parent = Undertaking::open(&db(&dir), "main", None).unwrap();
        let second = parent.open_part("second").unwrap();
        assert_eq!(second.id(), "main/2");
        assert_eq!(second.state().brief, "second");
        assert!(parent.open_part("  ").is_err(), "a part needs a brief");
    }

    #[test]
    fn a_transcript_saved_before_tools_returned_pictures_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        // Exactly as an earlier version wrote it.
        let old = r#"[{"is":"user","text":"list the files"},{"is":"assistant","text":"","calls":[{"id":"c1","name":"shell","args":{"command":"ls"}}]},{"is":"result","id":"c1","name":"shell","output":"a.txt","failed":false}]"#;
        Connection::open(db(&dir))
            .unwrap()
            .execute(
                "INSERT INTO transcript (id, items) VALUES ('u1', ?1)",
                [old],
            )
            .unwrap();
        let items = u.transcript().expect("it loads");
        assert_eq!(items.len(), 3);
        assert_eq!(items[2], Item::result("c1", "shell", "a.txt", false));
    }

    fn seen(bytes: &[u8]) -> Item {
        Item::Result {
            id: "c1".into(),
            name: "fetch".into(),
            output: "An image.".into(),
            failed: false,
            pictures: vec![Picture {
                mime: "image/png".into(),
                bytes: bytes.to_vec(),
            }],
        }
    }

    fn saved_row(dir: &tempfile::TempDir) -> String {
        Connection::open(db(dir))
            .unwrap()
            .query_row("SELECT items FROM transcript", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_saved_transcript_names_a_picture_by_its_file_and_holds_no_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        let bytes = b"\x89PNG\r\n\x1a\n and the rest";
        // Never handed to `keep`: saving keeps it first.
        u.save_transcript(&[Item::user("look"), seen(bytes)]);
        let file = media::name(bytes, "image/png");
        let row = saved_row(&dir);
        let written: Value = serde_json::from_str(&row).unwrap();
        assert_eq!(
            written[1]["pictures"],
            json!([{"mime": "image/png", "file": file}])
        );
        assert!(!row.contains("\"data\""), "{row}");
        assert!(dir.path().join(media::MEDIA_DIR).join(&file).exists());
        // And it comes back whole.
        assert_eq!(u.transcript().unwrap()[1], seen(bytes));
    }

    #[test]
    fn a_picture_whose_file_is_gone_comes_back_as_a_line_of_text() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        let bytes = b"GIF89a gone";
        u.save_transcript(&[seen(bytes)]);
        let file = media::name(bytes, "image/png");
        fs::remove_file(dir.path().join(media::MEDIA_DIR).join(&file)).unwrap();
        let items = u.transcript().expect("the rest still loads");
        assert_eq!(
            items,
            [Item::result(
                "c1",
                "fetch",
                format!(
                    "An image.\n[The image {file} is no longer on disk, so it cannot be shown.]"
                ),
                false
            )]
        );
    }

    #[test]
    fn a_picture_saved_as_base64_still_loads_and_a_path_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        fs::write(dir.path().join("outside.png"), "not to be read").unwrap();
        let row = r#"[{"is":"result","id":"c1","name":"fetch","output":"An image.","failed":false,"pictures":[{"mime":"image/png","data":"AQID"}]},
            {"is":"result","id":"c2","name":"fetch","output":"Another.","failed":false,"pictures":[{"mime":"image/png","file":"../outside.png"}]}]"#;
        Connection::open(db(&dir))
            .unwrap()
            .execute(
                "INSERT INTO transcript (id, items) VALUES ('u1', ?1)",
                [row],
            )
            .unwrap();
        let items = u.transcript().unwrap();
        assert_eq!(items[0], seen(&[1, 2, 3]));
        assert!(matches!(&items[1], Item::Result { pictures, output, .. }
            if pictures.is_empty() && output.contains("no longer on disk")));
    }

    #[test]
    fn the_history_names_a_picture_and_does_not_hold_it() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        let picture = Picture {
            mime: "image/png".into(),
            bytes: b"\x89PNG not really".to_vec(),
        };
        let kept = u.keep(std::slice::from_ref(&picture));
        let file = media::name(&picture.bytes, &picture.mime);
        assert_eq!(kept, std::slice::from_ref(&file));
        assert!(dir.path().join(media::MEDIA_DIR).join(&file).exists());
        u.remember(&[Item::Result {
            id: "c1".into(),
            name: "fetch".into(),
            output: "An image.".into(),
            failed: false,
            pictures: vec![picture],
        }]);
        let conn = Connection::open(db(&dir)).unwrap();
        let written: String = conn
            .query_row("SELECT item FROM history", [], |row| row.get(0))
            .unwrap();
        assert!(!written.contains("pictures"), "{written}");
        assert_eq!(
            u.history(),
            [Item::result(
                "c1",
                "fetch",
                format!("An image.\n[image: {file}]"),
                false
            )]
        );
    }

    #[test]
    fn history_refuses_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
        u.remember(&[Item::user("first"), Item::user("second")]);
        let conn = Connection::open(db(&dir)).unwrap();
        assert!(
            conn.execute("UPDATE history SET item = '\"changed\"'", [])
                .is_err()
        );
        assert!(conn.execute("DELETE FROM history", []).is_err());
        u.remember(&[Item::user("third")]);
        assert_eq!(u.history().len(), 3, "and it is still added to");
    }

    #[test]
    fn two_undertakings_in_one_database_do_not_see_each_others_state() {
        let dir = tempfile::tempdir().unwrap();
        let a = Undertaking::open(&db(&dir), "a", Some("paint the fence")).unwrap();
        let b = Undertaking::open(&db(&dir), "b", Some("mend the gate")).unwrap();
        a.update(|s| s.steps = 7);
        a.save_transcript(&[Item::user("fence")]);
        a.remember(&[Item::user("fence")]);
        a.log(&serde_json::json!({"step": 1}));
        a.add("white").unwrap();

        assert_eq!(b.state().brief, "mend the gate");
        assert_eq!(b.state().steps, 0);
        assert!(b.transcript().is_none());
        assert!(b.history().is_empty());
        assert!(b.logged().is_empty());
        assert!(b.taken().is_empty(), "what was added to a is not b's");
        assert_eq!(a.taken(), ["white"]);
    }

    #[test]
    fn changes_made_through_different_handles_at_once_are_all_kept() {
        let dir = tempfile::tempdir().unwrap();
        Undertaking::open(&db(&dir), "u1", Some("count")).unwrap();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let path = db(&dir);
                std::thread::spawn(move || {
                    let u = Undertaking::open(&path, "u1", None).unwrap();
                    for _ in 0..25 {
                        u.update(|s| s.steps += 1);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let u = Undertaking::open(&db(&dir), "u1", None).unwrap();
        assert_eq!(u.state().steps, 100);
    }

    #[test]
    fn items_added_from_another_connection_are_taken_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        Undertaking::open(&db(&dir), "u1", Some("find when it closes")).unwrap();
        let adders: Vec<_> = (0..3)
            .map(|who| {
                let path = db(&dir);
                std::thread::spawn(move || {
                    let u = Undertaking::open(&path, "u1", None).unwrap();
                    for i in 0..20 {
                        u.add(&format!("{who}-{i}")).unwrap();
                    }
                })
            })
            .collect();
        // Two takers racing the adders and each other.
        let takers: Vec<_> = (0..2)
            .map(|_| {
                let path = db(&dir);
                std::thread::spawn(move || {
                    let u = Undertaking::open(&path, "u1", None).unwrap();
                    let mut got = Vec::new();
                    for _ in 0..200 {
                        got.extend(u.taken());
                    }
                    got
                })
            })
            .collect();
        for t in adders {
            t.join().unwrap();
        }
        let mut taken: Vec<String> = takers.into_iter().flat_map(|t| t.join().unwrap()).collect();
        let last = Undertaking::open(&db(&dir), "u1", None).unwrap();
        taken.extend(last.taken());

        let mut sorted = taken.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), taken.len(), "nothing was taken twice");
        assert_eq!(taken.len(), 60, "nothing was lost");
        let mut additions = last.state().additions;
        additions.sort();
        assert_eq!(additions, sorted, "each became part of the task once");
    }

    #[test]
    fn only_the_newest_notes_are_shown_when_there_are_many() {
        let mut state = State::default();
        for i in 0..10 {
            state.notes.push(Note {
                at: Utc::now(),
                text: format!("fact number {i}"),
            });
        }
        let shown = state.notes_text(40);
        assert_eq!(
            shown,
            "(8 older notes are not shown)\n- fact number 8\n- fact number 9"
        );
    }

    #[test]
    fn what_is_added_from_outside_becomes_part_of_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let worker = Undertaking::open(&db(&dir), "u1", Some("find when it closes")).unwrap();
        // Someone else, with their own handle on the same database.
        let outside = Undertaking::open(&db(&dir), "u1", None).unwrap();
        outside.add("weekends too").unwrap();
        outside.add("and whether it takes bookings").unwrap();
        assert_eq!(
            worker.taken(),
            ["weekends too", "and whether it takes bookings"]
        );
        assert!(worker.taken().is_empty(), "taken once");
        assert_eq!(
            outside.state().task_text(),
            "find when it closes\n\nAdded later: weekends too\n\nAdded later: and whether it takes bookings"
        );
    }

    #[test]
    fn an_inbox_written_by_schema_one_is_kept_and_what_is_in_it_still_joins_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let state = serde_json::to_string(&State {
            brief: "find when it closes".into(),
            ..State::default()
        })
        .unwrap();
        // As schema 1 left it, with a line not yet taken.
        let conn = Connection::open(db(&dir)).unwrap();
        conn.execute_batch(
            "CREATE TABLE undertakings (id TEXT PRIMARY KEY, state TEXT NOT NULL);
             CREATE TABLE inbox (
                 id TEXT NOT NULL, n INTEGER NOT NULL, text TEXT NOT NULL,
                 taken INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (id, n));
             INSERT INTO inbox (id, n, text) VALUES ('u1', 1, 'weekends too');
             PRAGMA user_version = 1;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO undertakings (id, state) VALUES ('u1', ?1)",
            [&state],
        )
        .unwrap();
        drop(conn);

        let u = Undertaking::open(&db(&dir), "u1", None).unwrap();
        assert_eq!(
            u.take_inbox(),
            [Mail {
                kind: MailKind::Added,
                text: "weekends too".into()
            }]
        );
        assert_eq!(u.state().additions, ["weekends too"]);
        u.post(MailKind::Told, "from a part").unwrap();
        assert_eq!(u.take_inbox()[0].kind, MailKind::Told);
    }

    #[test]
    fn what_a_part_says_to_its_parent_does_not_join_the_parents_task() {
        let dir = tempfile::tempdir().unwrap();
        let parent = Undertaking::open(&db(&dir), "main", Some("find when it closes")).unwrap();
        parent.add("weekends too").unwrap();
        parent
            .post(MailKind::Told, "part 1 says two sources agree")
            .unwrap();
        parent
            .post(MailKind::Asked, "part 1 asks which year")
            .unwrap();
        parent.post(MailKind::Ended, "part 1 finished").unwrap();
        assert_eq!(parent.pending(), 4);
        let kinds: Vec<MailKind> = parent.take_inbox().iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            [
                MailKind::Added,
                MailKind::Told,
                MailKind::Asked,
                MailKind::Ended
            ]
        );
        assert_eq!(parent.pending(), 0);
        assert_eq!(parent.state().additions, ["weekends too"]);
    }

    #[test]
    fn the_parts_of_a_work_are_listed_by_number_and_end_when_it_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let parent = Undertaking::open(&db(&dir), "main", Some("the whole")).unwrap();
        let first = parent.open_part("first").unwrap();
        let second = parent.open_part("second").unwrap();
        second.update(|s| {
            s.finish = Some(Finish {
                outcome: Outcome::Finished,
                summary: "done".into(),
                files: vec![],
                lesson: None,
                at: Utc::now(),
            });
        });
        // A helper from before parts ran on their own is not one of them.
        Undertaking::open(&db(&dir), "main/3", Some("an old helper")).unwrap();
        let listed: Vec<(u64, String)> = parent
            .parts()
            .into_iter()
            .map(|(n, s)| (n, s.brief))
            .collect();
        assert_eq!(listed, [(1, "first".into()), (2, "second".into())]);
        assert_eq!(parent.part(2).unwrap().number(), Some(2));
        assert!(parent.part(3).is_err());

        assert!(parent.stop());
        assert_eq!(first.state().finish.unwrap().outcome, Outcome::Dropped);
        assert_eq!(
            second.state().finish.unwrap().outcome,
            Outcome::Finished,
            "what had ended stays as it ended"
        );
    }

    #[test]
    fn nothing_starts_without_a_brief() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Undertaking::open(&db(&dir), "u1", None).is_err());
    }

    #[test]
    fn a_database_from_a_newer_version_is_not_written_to() {
        let dir = tempfile::tempdir().unwrap();
        Connection::open(db(&dir))
            .unwrap()
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(Undertaking::open(&db(&dir), "u1", Some("anything")).is_err());
    }
}
