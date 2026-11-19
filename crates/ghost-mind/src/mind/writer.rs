//! Her work database, written off her mind.
//!
//! What her mind changes in `work.db` (a round of a look and where the look
//! ended, a word passed on to work, a stop, an end, the record a sitting is
//! opened on) is a write that waits, for as long as the database's busy
//! timeout, on a sitting holding its lock. So none is made on her mind:
//! each is handed to one thread of her own, which makes them in the order
//! they were handed over, and what she has to know of how one went comes
//! back to her as an `Outcome`, as any job's does. Being in order is what
//! keeps a look's rounds after its beginning and its end after its rounds,
//! and a stop after the word passed on before it, without her waiting on
//! any of them.
//!
//! A look is kept by its place: there is one look at a place at a time,
//! and the writer numbers it when its first round comes, so that her mind
//! never needs its number. It knows the look by when it began keeping
//! rounds, and says that for a console, which finds the look by it.

use std::{collections::HashMap, io, path::PathBuf, sync::mpsc};

use chrono::{DateTime, Utc};
use ghost_work::{Undertaking, looks::Looks};
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::{Span, warn};

use super::{Mind, Outcome, Purpose};

/// One write, as the writer is handed it.
type Write = Box<dyn FnOnce(&mut WorkDb) + Send>;

/// The way to her work database's writer.
pub(super) struct Writer {
    tx: mpsc::Sender<Write>,
}

impl Writer {
    /// Start the writer of the work database at `db`. It stops once this
    /// is dropped and what was handed to it is written.
    pub fn new(db: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Write>();
        std::thread::spawn(move || {
            let mut work = WorkDb {
                db,
                looks: None,
                kept: HashMap::new(),
            };
            for write in rx {
                write(&mut work);
            }
        });
        Self { tx }
    }

    /// Hand `write` to the writer, to be made after everything handed to
    /// it before, in the span it was handed over in. What it returns comes
    /// back on the receiver; a writer that is gone drops it.
    pub fn write<T: Send + 'static>(
        &self,
        write: impl FnOnce(&mut WorkDb) -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
        let (tx, rx) = oneshot::channel();
        let span = Span::current();
        let _ = self.tx.send(Box::new(move |work: &mut WorkDb| {
            let _ = tx.send(span.in_scope(|| write(work)));
        }));
        rx
    }
}

impl Mind {
    /// Change her work database with `write`, off her mind; `then` makes
    /// what it returns into what comes back to her. A write to the record
    /// of the work `about` holds that work while it is on its way: nothing
    /// else is begun on it, or settled from its record, until it is in.
    pub(super) fn write<T: Send + 'static>(
        &mut self,
        about: Option<String>,
        write: impl FnOnce(&mut WorkDb) -> T + Send + 'static,
        then: impl FnOnce(T) -> Outcome + Send + 'static,
    ) {
        let written = self.writer.write(write);
        let job = async move {
            if let Ok(done) = written.await {
                then(done)
            } else {
                warn!("her work database's writer is gone: a write was not made");
                Outcome::Done
            }
        };
        match about {
            Some(id) => self.jobs.spawn_for(Purpose::Record(id), job),
            None => self.jobs.spawn(job),
        }
    }
}

/// The work database, as the writer holds it.
pub(super) struct WorkDb {
    db: PathBuf,
    /// The record of her looks, opened the first time a look keeps a round.
    looks: Option<Looks>,
    /// The look being kept at each place: when it began, and its number.
    kept: HashMap<String, (DateTime<Utc>, i64)>,
}

impl WorkDb {
    /// The undertaking `id`, to write to, started from `brief` if it has
    /// not been opened before.
    pub fn undertaking(&self, id: &str, brief: &str) -> io::Result<Undertaking> {
        Undertaking::open(&self.db, id, Some(brief))
    }

    /// Add `round` to the look at `place` that began keeping rounds
    /// `since`, beginning it if this is its first.
    pub fn round(&mut self, place: &str, since: DateTime<Utc>, round: &Value) -> io::Result<()> {
        let looks = match &mut self.looks {
            Some(looks) => looks,
            none => none.insert(Looks::open(&self.db)?),
        };
        let look = match self.kept.get(place) {
            Some(&(began, look)) if began == since => look,
            // A look tried again is a new look: the one before it is left
            // without an end.
            _ => {
                let look = looks.begin(place, since)?;
                self.kept.insert(place.to_owned(), (since, look));
                look
            }
        };
        looks.round(look, round)
    }

    /// The look at `place` that began keeping rounds `since` ended with
    /// the journal entry `seq`. One that kept none has nothing to end.
    pub fn end_look(&mut self, place: &str, since: DateTime<Utc>, seq: i64) -> io::Result<()> {
        match (&self.looks, self.kept.get(place)) {
            (Some(looks), Some(&(began, look))) if began == since => {
                self.kept.remove(place);
                looks.end(look, seq)
            }
            _ => Ok(()),
        }
    }
}
