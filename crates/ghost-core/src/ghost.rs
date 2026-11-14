//! A ghost is a directory on disk. The binary is her body; the directory is her.
//!
//! ```text
//! weiwei/
//! ├── ghost.toml   her certificate: who she is. It can be revised.
//! ├── journal.db   append-only, hash-chained; every version of ghost.toml is in it
//! └── derived/     anything that can be rebuilt from the journal
//! ```

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};

use crate::{
    Error, Result,
    identity::{self, Identity, Seal},
    journal::{Entry, Event, JOURNAL_FILE, Journal},
    memory::Memory,
};

const LOCK_FILE: &str = "derived/lock";

/// A ghost opened for living: the only handle that can write her journal.
pub struct Ghost {
    dir: PathBuf,
    identity: Identity,
    journal: Journal,
    memory: Memory,
    /// Held for as long as she is open. One body at a time.
    _lock: File,
}

/// A ghost opened for looking: verified, read-only, takes no lock.
pub struct Peek {
    pub identity: Identity,
    pub journal: Journal,
    /// `ghost.toml` has been revised since the version in her journal. The
    /// revision is written there the next time she is opened to live.
    pub revised: bool,
}

impl Ghost {
    /// Give birth to the ghost described by `dir/ghost.toml`. Happens once.
    ///
    /// Writes the `Born` entry and the certificate as it stands. Fails if she
    /// already has a journal.
    pub fn birth(dir: &Path) -> Result<Self> {
        let identity = identity::read_certificate(dir)?;
        if dir.join(JOURNAL_FILE).exists() {
            return Err(Error::AlreadyBorn(dir.to_path_buf()));
        }
        fs::create_dir_all(dir.join("derived"))?;
        let lock = lock(dir)?;
        let journal = Journal::create(&dir.join(JOURNAL_FILE))?;
        let memory = Memory::open(dir)?;
        let mut ghost = Self {
            dir: dir.to_path_buf(),
            identity,
            journal,
            memory,
            _lock: lock,
        };
        ghost.record(Event::Born {
            name: ghost.identity.name().to_owned(),
            seal: ghost.identity.seal().clone(),
        })?;
        ghost.describe()?;
        Ok(ghost)
    }

    /// Open a ghost that was already born, to live.
    ///
    /// Her journal must begin with her birth and its whole chain must hold.
    /// If `ghost.toml` has been revised since the version in her journal, the
    /// new version is written there first; from then on it is who she is.
    /// Then her derived memory is brought up to date.
    pub fn open(dir: &Path) -> Result<Self> {
        let Peek {
            identity,
            journal,
            revised,
        } = Self::peek(dir)?;
        fs::create_dir_all(dir.join("derived"))?;
        let lock = lock(dir)?;
        let mut memory = Memory::open(dir)?;
        memory.catch_up(&journal)?;
        let mut ghost = Self {
            dir: dir.to_path_buf(),
            identity,
            journal,
            memory,
            _lock: lock,
        };
        if revised {
            ghost.describe()?;
        }
        Ok(ghost)
    }

    /// Verify a ghost and open her journal without taking the lock or
    /// touching derived state. Safe while a daemon is running her.
    pub fn peek(dir: &Path) -> Result<Peek> {
        let identity = identity::read_certificate(dir)?;
        let journal = Journal::open(&dir.join(JOURNAL_FILE))
            .map_err(|_| Error::NotHer("no journal; was she ever born?".into()))?;
        match journal.first()? {
            Some(Entry {
                event: Event::Born { .. },
                ..
            }) => {}
            Some(_) => return Err(Error::NotHer("journal does not begin with birth".into())),
            None => return Err(Error::NotHer("journal is empty".into())),
        }
        journal.verify()?;
        let revised = recorded_seal(&journal)?.as_ref() != Some(identity.seal());
        Ok(Peek {
            identity,
            journal,
            revised,
        })
    }

    /// Write her certificate, as it stands, into her journal.
    fn describe(&mut self) -> Result<()> {
        let event = Event::Described {
            certificate: self.identity.text().to_owned(),
            seal: self.identity.seal().clone(),
        };
        self.record(event)?;
        Ok(())
    }

    /// Write something into her life, stamped now.
    pub fn record(&mut self, event: Event) -> Result<Entry> {
        self.record_at(Utc::now(), event)
    }

    /// Write something into her life. The journal entry and the derived
    /// memory move together; this is the only way either changes.
    pub fn record_at(&mut self, at: DateTime<Utc>, event: Event) -> Result<Entry> {
        let entry = self.journal.append_at(at, event)?;
        self.memory.apply(&entry)?;
        Ok(entry)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }
}

/// The seal of the certificate her journal last recorded: the latest
/// `Described`, or, in a journal from before those were written, her birth.
fn recorded_seal(journal: &Journal) -> Result<Option<Seal>> {
    if let Some(entry) = journal.last_of(&["described"])?
        && let Event::Described { seal, .. } = entry.event
    {
        return Ok(Some(seal));
    }
    Ok(match journal.first()?.map(|e| e.event) {
        Some(Event::Born { seal, .. }) => Some(seal),
        _ => None,
    })
}

fn lock(dir: &Path) -> Result<File> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))?;
    file.try_lock()
        .map_err(|_| Error::Busy(dir.to_path_buf()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"
name = "Weiwei"
pronouns = "she/her"
born = "2026-11-12"
temperament = "quiet"
voice = "short"
origin = "made"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;

    fn dir_with_seed() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("ghost.toml"), SAMPLE).unwrap();
        d
    }

    #[test]
    fn birth_then_open() {
        let d = dir_with_seed();
        let g = Ghost::birth(d.path()).unwrap();
        assert_eq!(g.identity().name(), "Weiwei");
        // Her birth, and who she is.
        assert_eq!(g.journal().len().unwrap(), 2);
        drop(g);
        let g = Ghost::open(d.path()).unwrap();
        assert!(matches!(
            g.journal().first().unwrap().unwrap().event,
            Event::Born { .. }
        ));
        assert_eq!(g.memory().last_seq().unwrap(), 2);
    }

    #[test]
    fn born_once() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        assert!(matches!(Ghost::birth(d.path()), Err(Error::AlreadyBorn(_))));
    }

    #[test]
    fn a_revised_certificate_is_who_she_is_from_then_on() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        let revised = SAMPLE.replace("quiet", "cheerful").replace("Weiwei", "Wei");
        fs::write(d.path().join("ghost.toml"), &revised).unwrap();

        // Looking sees the revision but writes nothing.
        let peek = Ghost::peek(d.path()).unwrap();
        assert!(peek.revised);
        assert_eq!(peek.journal.len().unwrap(), 2);

        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.identity().name(), "Wei");
        assert_eq!(g.identity().seed().temperament, "cheerful");
        // Both versions are in her journal, in order.
        let described: Vec<String> = g
            .journal()
            .of_kinds_after(&["described"], 0)
            .unwrap()
            .into_iter()
            .map(|e| match e.event {
                Event::Described { certificate, .. } => certificate,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(described, [SAMPLE.to_owned(), revised]);
        drop(g);
        // Opening again with nothing changed writes nothing.
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.journal().len().unwrap(), 3);
    }

    #[test]
    fn a_journal_from_before_revisions_learns_her_certificate_only_when_it_changes() {
        let d = dir_with_seed();
        // As earlier versions wrote her: birth alone.
        {
            fs::create_dir_all(d.path().join("derived")).unwrap();
            let mut journal = Journal::create(&d.path().join(JOURNAL_FILE)).unwrap();
            let id = identity::read_certificate(d.path()).unwrap();
            journal
                .append(Event::Born {
                    name: "Weiwei".into(),
                    seal: id.seal().clone(),
                })
                .unwrap();
        }
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.journal().len().unwrap(), 1, "unchanged since birth");
        drop(g);
        fs::write(d.path().join("ghost.toml"), SAMPLE.replace("quiet", "dry")).unwrap();
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.journal().len().unwrap(), 2);
    }

    #[test]
    fn a_certificate_that_does_not_parse_is_refused() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        fs::write(d.path().join("ghost.toml"), SAMPLE.replace("08:30", "8am")).unwrap();
        assert!(matches!(Ghost::open(d.path()), Err(Error::BadSeed(_))));
    }

    #[test]
    fn unborn_is_not_her() {
        let d = dir_with_seed();
        assert!(matches!(Ghost::open(d.path()), Err(Error::NotHer(_))));
    }

    #[test]
    fn one_body_at_a_time() {
        let d = dir_with_seed();
        let first = Ghost::birth(d.path()).unwrap();
        assert!(matches!(Ghost::open(d.path()), Err(Error::Busy(_))));
        // Looking does not need the lock.
        assert!(Ghost::peek(d.path()).is_ok());
        drop(first);
        assert!(Ghost::open(d.path()).is_ok());
    }

    #[test]
    fn memory_is_only_a_view() {
        let d = dir_with_seed();
        let mut g = Ghost::birth(d.path()).unwrap();
        g.record(Event::Canon {
            fact: "has a cat".into(),
            source: "life".into(),
        })
        .unwrap();
        drop(g);
        fs::remove_dir_all(d.path().join(crate::memory::MEMORY_DIR)).unwrap();
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.memory().canon(10).unwrap().len(), 1);
    }
}
