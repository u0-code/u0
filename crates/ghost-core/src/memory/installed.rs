//! The servers of tools she has installed.
//!
//! One row per server she has now: an `Installed` entry puts it here, and
//! replaces what was here under the same name, since installing again is
//! how she takes a newer version or takes back one she stopped using; an
//! `Uninstalled` entry takes it out. Servers whoever runs her body gave
//! her in its configuration are not hers in this sense and are not here.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};

use super::{Memory, time_at};
use crate::{
    Result,
    journal::{Entry, Event, Launch},
};

/// A server she has installed, as its `Installed` entry said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The seq of the `Installed` entry.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: String,
    pub owner: Option<String>,
    pub how: Launch,
    pub tools: Vec<String>,
    pub hash: String,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS installed (
    name TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL, version TEXT NOT NULL,
    description TEXT NOT NULL, source TEXT NOT NULL, owner TEXT, how TEXT NOT NULL,
    tools TEXT NOT NULL, hash TEXT NOT NULL
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    match &entry.event {
        Event::Installed {
            name,
            version,
            description,
            source,
            owner,
            how,
            tools,
            hash,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO installed
                 (name, seq, at, version, description, source, owner, how, tools, hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    name,
                    entry.seq,
                    at,
                    version,
                    description,
                    source,
                    owner,
                    serde_json::to_string(how)?,
                    serde_json::to_string(tools)?,
                    hash
                ],
            )?;
        }
        Event::Uninstalled { name, .. } => {
            tx.execute("DELETE FROM installed WHERE name = ?1", [name])?;
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// Every server of tools she has installed and not taken out, in the
    /// order she installed them.
    pub fn installed(&self) -> Result<Vec<Installed>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, name, version, description, source, owner, how, tools, hash
             FROM installed ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    Installed {
                        seq: r.get(0)?,
                        at: time_at(r, 1)?,
                        name: r.get(2)?,
                        version: r.get(3)?,
                        description: r.get(4)?,
                        source: r.get(5)?,
                        owner: r.get(6)?,
                        how: Launch::default(),
                        tools: Vec::new(),
                        hash: r.get(9)?,
                    },
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(row, how, tools)| {
                Ok(Installed {
                    how: serde_json::from_str(&how)?,
                    tools: serde_json::from_str(&tools)?,
                    ..row
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Memory, testing::fold};
    use crate::journal::{Event, Launch};

    fn installed(name: &str, version: &str) -> Event {
        Event::Installed {
            name: name.into(),
            version: version.into(),
            description: "does a thing".into(),
            source: format!("npm:{name}@{version}"),
            owner: Some("github.com/someone".into()),
            how: Launch {
                command: vec!["npx".into(), format!("{name}@{version}")],
                ..Launch::default()
            },
            tools: vec!["look".into()],
            hash: "h".into(),
        }
    }

    #[test]
    fn what_she_installed_and_took_out_survives_a_rebuild_from_the_journal() {
        let (journal, m) = fold(vec![
            installed("io.github.someone/a", "1.0.0"),
            installed("io.github.someone/b", "2.0.0"),
            Event::Uninstalled {
                name: "io.github.someone/a".into(),
                why: "it stopped starting".into(),
            },
            installed("io.github.someone/b", "2.1.0"),
        ]);
        let before = m.installed().unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].name, "io.github.someone/b");
        assert_eq!(before[0].version, "2.1.0", "installing again replaces");
        assert_eq!(before[0].how.command[1], "io.github.someone/b@2.1.0");

        // Memory is only a view: one folded afresh from the journal holds
        // the same.
        let mut again = Memory::in_memory().unwrap();
        again.catch_up(&journal).unwrap();
        assert_eq!(again.installed().unwrap(), before);
    }
}
