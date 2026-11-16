//! What reaches a piece of work while it runs: what is added to its task
//! from outside, and what its parts say to it. Each is put in the inbox and
//! taken, once, before the worker's next step.

use std::io;

use rusqlite::{TransactionBehavior, params};

use super::{
    State, Undertaking,
    db::{load, other, store},
};

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

impl Undertaking {
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
}
