//! The agent skills she has installed.
//!
//! One row per skill she has now: a `SkillInstalled` entry puts it here,
//! and replaces what was here under the same name, since installing again
//! is how she takes a newer commit of it; a `SkillUninstalled` entry takes
//! it out. They are kept apart from the servers of tools in `installed`
//! because a skill is a different thing to have: words she reads when a
//! job calls for them, not a program that runs.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};

use super::{Memory, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
};

/// A skill she has installed, as its `SkillInstalled` entry said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// The seq of the `SkillInstalled` entry.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub name: String,
    pub source: String,
    pub path: String,
    pub sha: String,
    pub hash: String,
    pub description: String,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS skills (
    name TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL, source TEXT NOT NULL,
    path TEXT NOT NULL, sha TEXT NOT NULL, hash TEXT NOT NULL, description TEXT NOT NULL
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        Event::SkillInstalled {
            name,
            source,
            path,
            sha,
            hash,
            description,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO skills
                 (name, seq, at, source, path, sha, hash, description)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![name, entry.seq, at, source, path, sha, hash, description],
            )?;
        }
        Event::SkillUninstalled { name, .. } => {
            tx.execute("DELETE FROM skills WHERE name = ?1", [name])?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// Every skill she has installed and not taken out, in the order she
    /// installed them.
    pub fn skills(&self) -> Result<Vec<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, name, source, path, sha, hash, description
             FROM skills ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Skill {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    name: r.get(2)?,
                    source: r.get(3)?,
                    path: r.get(4)?,
                    sha: r.get(5)?,
                    hash: r.get(6)?,
                    description: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Memory, testing::fold};
    use crate::journal::Event;

    fn installed(name: &str, sha: &str) -> Event {
        Event::SkillInstalled {
            name: name.into(),
            source: "someone/skills".into(),
            path: format!("skills/{name}"),
            sha: sha.into(),
            hash: "h".into(),
            description: "does a thing".into(),
        }
    }

    #[test]
    fn the_skills_she_installed_and_took_out_survive_a_rebuild_from_the_journal() {
        let (journal, m) = fold(vec![
            installed("pdf", "aaa"),
            installed("slides", "bbb"),
            Event::SkillUninstalled {
                name: "pdf".into(),
                why: "its files changed on disk".into(),
            },
            installed("slides", "ccc"),
        ]);
        let before = m.skills().unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].name, "slides");
        assert_eq!(before[0].sha, "ccc", "installing again replaces");
        assert_eq!(before[0].path, "skills/slides");

        // Memory is only a view: one folded afresh from the journal holds
        // the same.
        let mut again = Memory::in_memory().unwrap();
        again.catch_up(&journal).unwrap();
        assert_eq!(again.skills().unwrap(), before);
    }
}
