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
//! undertakings      brief, plan, notes, outcome, running totals
//! transcript_items  what the model is shown now, an item a row, saved
//!                   with every step; an item as the history has it is
//!                   kept there and only pointed to
//! history           every item the transcript ever held; append-only
//! log            one line per step: what the model said, what it called
//!                with what, the head of what came back, what it cost
//! inbox          what reached it from outside while it ran, of what kind,
//!                and whether it has been taken
//! ```
//!
//! The same database keeps what she called while she looked at a
//! conversation, which is not work: see [`crate::looks`].
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
//!
//! The record is the doing, not the account of it. Whoever the work is for
//! keeps their own account (a ghost keeps hers in her journal: that it was
//! taken on, for whom, and that it ended and how) and goes by it where the
//! two differ. What only the record has is the plan, the notes, the
//! transcript, the history, the step log, the inbox, the running totals,
//! and `finish`: the worker's own word on how it ended, or the word of
//! whoever ended it from outside, which their account then copies. So it
//! is written to before their account is, and is never behind it. Looking
//! at it from outside goes through [`Records`], which reads, and never
//! makes or changes a record. A change that cannot be written is not kept
//! anywhere: [`Undertaking::update`] returns the error.
//!
//! What the record is made of is in [`State`]; the schema, and the queries
//! everything else is built on, in `db`; the transcript, the history and
//! the step log in `transcript`; the inbox in `inbox`; and what someone
//! looking from outside is shown in `look`. This file holds the handle on
//! one undertaking, and how it is opened, read, changed and stopped.

pub(crate) mod db;
mod inbox;
mod look;
mod state;
#[cfg(test)]
mod tests;
mod transcript;

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};

use chrono::Utc;
use rusqlite::{Connection, TransactionBehavior};

use self::db::{load, other, store};
pub use self::{
    db::WORK_DB,
    inbox::{Mail, MailKind},
    look::{Glance, Logged, Look, Records, Step, StepContext, StepUsage, glance, look, steps},
    state::{AskKind, Checkout, Finish, Kept, Note, Outcome, PlanStep, Progress, State, Told},
    transcript::placeholder,
};

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
    /// The transcript as this handle last read or saved it, item by item,
    /// which is what is on disk: a save writes only the items that differ.
    /// `None` until it is known.
    saved: Mutex<Option<Vec<ghost_brain::Item>>>,
}

impl Undertaking {
    /// Start the undertaking `id` in the database at `db`, or pick up what
    /// is already there. A brief is only taken the first time: what was
    /// asked does not change.
    pub fn open(db: &Path, id: &str, brief: Option<&str>) -> io::Result<Self> {
        let mut conn = db::open(db)?;
        // Work already on record is picked up with a read: only making a
        // record takes the lock a worker writes under.
        let found = load(&conn, id).map_err(other)?;
        let state = if let Some(state) = found {
            state
        } else {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
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
            saved: Mutex::new(None),
        })
    }

    /// What whoever asked for it wrote, word for word: the lines its task
    /// quotes, and for a part, its work's too.
    pub fn their_words(&self) -> String {
        let quoted = |state: &State| {
            state
                .task_text()
                .lines()
                .filter(|l| l.trim_start().starts_with('>'))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let state = self.state();
        let mut lines = quoted(&state);
        if let Some(parent) = &state.parent
            && let Ok(Some(of)) = load(&self.conn(), parent)
        {
            lines.extend(quoted(&of));
        }
        lines.join("\n")
    }

    /// The directory its database is in: the ghost's own.
    pub fn home(&self) -> &Path {
        self.db.parent().unwrap_or_else(|| Path::new("."))
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
            // A part is done where the work it is part of is done.
            let state = State {
                brief: brief.to_owned(),
                parent: Some(self.id.clone()),
                dir: load(&tx, &self.id).map_err(other)?.and_then(|s| s.dir),
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
                let state = db::parse(&state).ok()?;
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
    /// lost.
    ///
    /// A change that could not be written is not kept, here or anywhere:
    /// the error is returned, and this handle goes on showing what is on
    /// disk. Work that goes on believing a record it does not have would
    /// part from it without anyone knowing.
    pub fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> io::Result<T> {
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(other)?;
        let mut state = match load(&tx, &self.id).map_err(other)? {
            Some(state) => state,
            None => self.cached().clone(),
        };
        let out = change(&mut state);
        store(&tx, &self.id, &state)
            .and_then(|()| tx.commit())
            .map_err(other)?;
        *self.cached() = state;
        Ok(out)
    }

    /// End the work from outside, as `dropped`, unless it has already
    /// ended. A worker stepping it sees this before its next step, and a
    /// tool server refuses every call after it. Its parts end with it.
    /// Returns whether it was still going.
    pub fn stop(&self) -> io::Result<bool> {
        self.end(Outcome::Dropped, "")
    }

    /// End the work from outside with `outcome`, unless it has already
    /// ended, and its parts with it, as `dropped`. Returns whether it was
    /// still going. How it ended first is how it ended: an end already on
    /// record is not written over.
    pub fn end(&self, outcome: Outcome, summary: &str) -> io::Result<bool> {
        let ended = self.update(|state| {
            if state.finish.is_some() {
                return false;
            }
            state.finish = Some(Finish {
                outcome,
                summary: summary.to_owned(),
                files: Vec::new(),
                lesson: None,
                at: Utc::now(),
            });
            true
        })?;
        self.stop_parts();
        Ok(ended)
    }

    /// End every part that has not ended, as `dropped`.
    pub fn stop_parts(&self) {
        for (n, state) in self.parts() {
            if state.finish.is_none()
                && let Err(e) = self.part(n).and_then(|part| part.stop())
            {
                tracing::warn!("could not stop part {n} of {}: {e}", self.id);
            }
        }
    }
}
