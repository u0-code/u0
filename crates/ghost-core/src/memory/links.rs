//! Accounts shown to be the same person.
//!
//! When someone said they were a person she knows on another platform, she
//! asked for proof: a word she made up, to come back from the account she
//! already knew. A challenge waits until the word comes back; then the two
//! accounts are linked, and links carry across, so three accounts can be
//! one person.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, time_at};
use crate::{
    Result,
    journal::{Entry, Event, stamp},
};

/// A proof she is waiting for: `word`, from the account `claimed`.
#[derive(Debug, Clone, Serialize)]
pub struct Challenge {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub claimant: String,
    pub claimed: String,
    pub word: String,
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS challenges (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, claimant TEXT NOT NULL,
    claimed TEXT NOT NULL, word TEXT NOT NULL, met INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS links (
    seq INTEGER PRIMARY KEY, one TEXT NOT NULL, other TEXT NOT NULL
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let seq = entry.seq;
    match &entry.event {
        Event::Challenged {
            claimant,
            claimed,
            word,
        } => {
            tx.execute(
                "INSERT INTO challenges (seq, at, claimant, claimed, word) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![seq, at, claimant, claimed, word],
            )?;
        }
        Event::Linked { one, other } => {
            tx.execute(
                "INSERT INTO links (seq, one, other) VALUES (?1, ?2, ?3)",
                params![seq, one, other],
            )?;
            tx.execute(
                "UPDATE challenges SET met = 1
                 WHERE (claimant = ?1 AND claimed = ?2) OR (claimant = ?2 AND claimed = ?1)",
                params![one, other],
            )?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// The proofs she is still waiting for from the account `claimed`,
    /// asked for at or after `since`.
    pub fn challenges_to(&self, claimed: &str, since: DateTime<Utc>) -> Result<Vec<Challenge>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, claimant, claimed, word FROM challenges
             WHERE claimed = ?1 AND met = 0 AND at >= ?2 ORDER BY seq",
        )?;
        Ok(stmt
            .query_map(params![claimed, stamp(&since)], |r| {
                Ok(Challenge {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    claimant: r.get(2)?,
                    claimed: r.get(3)?,
                    word: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The other accounts shown to belong to the same person as `key`.
    pub fn linked(&self, key: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT one, other FROM links")?;
        let pairs = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Links are few; follow them until nothing new turns up.
        let mut same = vec![key.to_owned()];
        let mut grew = true;
        while grew {
            grew = false;
            for (one, other) in &pairs {
                for (here, there) in [(one, other), (other, one)] {
                    if same.contains(here) && !same.contains(there) {
                        same.push(there.clone());
                        grew = true;
                    }
                }
            }
        }
        same.remove(0);
        Ok(same)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::testing::fold;
    use crate::journal::Event;

    #[test]
    fn a_proof_is_waited_for_until_it_is_met() {
        let challenged = Event::Challenged {
            claimant: "d:wen".into(),
            claimed: "t:wen".into(),
            word: "heron-42".into(),
        };
        let (_, m) = fold(vec![challenged.clone()]);
        let since = Utc::now() - chrono::Duration::hours(1);
        assert_eq!(m.challenges_to("t:wen", since).unwrap()[0].word, "heron-42");
        assert!(m.challenges_to("d:wen", since).unwrap().is_empty());
        assert!(m.linked("t:wen").unwrap().is_empty());

        let (_, m) = fold(vec![
            challenged,
            Event::Linked {
                one: "d:wen".into(),
                other: "t:wen".into(),
            },
            Event::Linked {
                one: "t:wen".into(),
                other: "x:wen".into(),
            },
        ]);
        assert!(m.challenges_to("t:wen", since).unwrap().is_empty(), "met");
        let mut same = m.linked("d:wen").unwrap();
        same.sort();
        assert_eq!(same, ["t:wen", "x:wen"], "links carry across");
    }
}
