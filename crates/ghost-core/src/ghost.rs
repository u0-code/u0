//! A ghost is a directory on disk. The binary is her body; the directory is her.
//!
//! ```text
//! lin/
//! ├── ghost.toml   birth certificate, written once
//! ├── seal         BLAKE3 of ghost.toml, written by `birth`
//! ├── journal.db   append-only, hash-chained
//! └── derived/     anything that can be rebuilt from the journal
//! ```

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};

use crate::{
    Error, Result,
    identity::{self, SealedIdentity},
    journal::{Entry, Event, JOURNAL_FILE, Journal},
    memory::Memory,
};

const LOCK_FILE: &str = "derived/lock";

/// A ghost opened for living: the only handle that can write her journal.
pub struct Ghost {
    dir: PathBuf,
    identity: SealedIdentity,
    journal: Journal,
    memory: Memory,
    /// Held for as long as she is open. One body at a time.
    _lock: File,
}

/// A ghost opened for looking: verified, read-only, takes no lock.
pub struct Peek {
    pub identity: SealedIdentity,
    pub journal: Journal,
}

impl Ghost {
    /// Give birth to the ghost described by `dir/ghost.toml`. Happens once.
    ///
    /// Writes the seal file and the `Born` entry. Fails if either exists.
    pub fn birth(dir: &Path) -> Result<Self> {
        let identity = identity::read_certificate(dir)?;
        if identity::read_seal_file(dir)?.is_some() || dir.join(JOURNAL_FILE).exists() {
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
        identity::write_seal_file(dir, ghost.identity.seal())?;
        Ok(ghost)
    }

    /// Open a ghost that was already born. Verifies that she is still her.
    ///
    /// Three things must agree: the certificate on disk, the seal file, and
    /// the `Born` entry at the head of her journal. Then the whole chain is
    /// verified and her derived memory is brought up to date. There is no
    /// flag to skip any of this.
    pub fn open(dir: &Path) -> Result<Self> {
        let Peek { identity, journal } = Self::peek(dir)?;
        fs::create_dir_all(dir.join("derived"))?;
        let lock = lock(dir)?;
        let mut memory = Memory::open(dir)?;
        memory.catch_up(&journal)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            identity,
            journal,
            memory,
            _lock: lock,
        })
    }

    /// Verify a ghost and open her journal without taking the lock or
    /// touching derived state. Safe while a daemon is running her.
    pub fn peek(dir: &Path) -> Result<Peek> {
        let identity = identity::read_certificate(dir)?;
        let Some(sealed) = identity::read_seal_file(dir)? else {
            return Err(Error::NotHer("no seal; was she ever born?".into()));
        };
        if &sealed != identity.seal() {
            return Err(Error::NotHer("ghost.toml has changed since birth".into()));
        }
        let journal = Journal::open(&dir.join(JOURNAL_FILE))?;
        match journal.first()? {
            Some(entry) => match entry.event {
                Event::Born { seal, .. } if &seal == identity.seal() => {}
                Event::Born { .. } => {
                    return Err(Error::NotHer(
                        "journal was born with a different certificate".into(),
                    ));
                }
                _ => return Err(Error::NotHer("journal does not begin with birth".into())),
            },
            None => return Err(Error::NotHer("journal is empty".into())),
        }
        journal.verify()?;
        Ok(Peek { identity, journal })
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

    pub fn identity(&self) -> &SealedIdentity {
        &self.identity
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }
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
name = "Lin"
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
        assert_eq!(g.identity().name(), "Lin");
        assert_eq!(g.journal().len().unwrap(), 1);
        drop(g);
        let g = Ghost::open(d.path()).unwrap();
        assert!(matches!(
            g.journal().first().unwrap().unwrap().event,
            Event::Born { .. }
        ));
        assert_eq!(g.memory().last_seq().unwrap(), 1);
    }

    #[test]
    fn born_once() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        assert!(matches!(Ghost::birth(d.path()), Err(Error::AlreadyBorn(_))));
    }

    #[test]
    fn edited_certificate_is_not_her() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        fs::write(
            d.path().join("ghost.toml"),
            SAMPLE.replace("quiet", "cheerful"),
        )
        .unwrap();
        assert!(matches!(Ghost::open(d.path()), Err(Error::NotHer(_))));
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
        fs::remove_file(d.path().join(crate::memory::MEMORY_FILE)).unwrap();
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.memory().canon(10).unwrap().len(), 1);
    }
}
