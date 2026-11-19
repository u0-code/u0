//! A ghost is a directory on disk. The binary is her body; the directory is her.
//!
//! ```text
//! weiwei/
//! ├── ghost.toml   her persona: who she is. It can be revised.
//! ├── assets/      how she looks to others: her avatar. Optional.
//! ├── journal.db   append-only, hash-chained; every version of ghost.toml is in it
//! ├── work.db      the state of what she has taken on: plans, notes, transcripts
//! ├── media/       images she was sent and looked at, named by their hash
//! ├── skills/      the skills she installed, a folder each
//! ├── tools/       the servers of tools she installed, a folder each
//! ├── home/        where her shell works, when her machine is `local:` alone
//! └── derived/     anything that can be rebuilt from the journal
//! ```
//!
//! Everything in her directory is hers alone. How she works, the words her
//! brain is given included, is shared by every character and lives with
//! whoever runs her (see [`prompts`](crate::prompts)).

use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};

use crate::{
    Error, Result,
    identity::{self, Identity, Seal},
    journal::{Entry, Event, JOURNAL_FILE, Journal},
    memory::Memory,
};

const LOCK_FILE: &str = "derived/lock";
/// Where what she looks like to others is kept in her directory.
pub const ASSETS_DIR: &str = "assets";
/// The kinds of picture her avatar may be, in the order they are looked for.
const AVATAR_KINDS: &[&str] = &["png", "webp", "jpg", "jpeg", "gif"];

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
    /// How many entries her journal held when it was verified.
    pub entries: i64,
    /// `ghost.toml` has been revised since the version in her journal. The
    /// revision is written there the next time she is opened to live.
    pub revised: bool,
    /// Kinds of entry in her journal that this build does not know: a
    /// newer version wrote them. She can be looked at, and is not opened
    /// to live by this build.
    pub unknown: Vec<String>,
}

impl Ghost {
    /// Give birth to the ghost described by `dir/ghost.toml`. Happens once.
    ///
    /// Writes the `Born` entry and the persona as it stands. Fails if she
    /// already has a journal.
    pub fn birth(dir: &Path) -> Result<Self> {
        let identity = identity::read_persona(dir)?;
        if dir.join(JOURNAL_FILE).exists() {
            return Err(Error::AlreadyBorn(dir.to_path_buf()));
        }
        fs::create_dir_all(dir.join("derived"))?;
        let lock = lock(dir)?;
        let journal = Journal::create(&dir.join(JOURNAL_FILE))?;
        let memory = Memory::open(dir, &journal)?;
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
    ///
    /// Refused, before anything is written, when her journal has entries
    /// of a kind this build does not know.
    /// If another body has her, it is given a moment to let her go.
    pub fn open(dir: &Path) -> Result<Self> {
        let (
            Peek {
                identity,
                journal,
                revised,
                ..
            },
            lock,
        ) = hold(dir)?;
        let mut memory = Memory::open(dir, &journal)?;
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
        let identity = identity::read_persona(dir)?;
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
        let entries = journal.verify()?;
        let revised = recorded_seal(&journal)?.as_ref() != Some(identity.seal());
        let unknown = journal.unknown_kinds()?;
        Ok(Peek {
            identity,
            journal,
            entries,
            revised,
            unknown,
        })
    }

    /// Write her persona, as it stands, into her journal.
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
    ///
    /// Memory folds from where it stopped, so an entry an earlier write
    /// could not fold is folded first. If folding fails, the entry is still
    /// in her journal, and the error says which entry memory stopped at.
    pub fn record_at(&mut self, at: DateTime<Utc>, event: Event) -> Result<Entry> {
        let entry = self.journal.append_at(at, event)?;
        self.memory.catch_up(&self.journal)?;
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

    /// Her picture, for the profile of an account she has on a platform:
    /// `assets/avatar.png`, or a `.webp`, `.jpg` or `.gif`, if there is
    /// one. It is not in her journal; like the rest of `assets/`, it is how
    /// others see her, not something she lived.
    pub fn avatar(&self) -> Option<PathBuf> {
        avatar(&self.dir)
    }
}

/// The avatar in the ghost directory `dir`, if there is one.
pub fn avatar(dir: &Path) -> Option<PathBuf> {
    let assets = dir.join(ASSETS_DIR);
    AVATAR_KINDS
        .iter()
        .map(|kind| assets.join(format!("avatar.{kind}")))
        .find(|path| path.is_file())
}

/// The seal of the persona her journal last recorded: the latest
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

/// Her lock in `dir`, and her as [`Ghost::peek`] finds her, if she may be
/// opened to live there by this build.
fn hold(dir: &Path) -> Result<(Peek, File)> {
    let mut peek = Ghost::peek(dir)?;
    livable(peek.unknown.clone())?;
    fs::create_dir_all(dir.join("derived"))?;
    let lock = lock(dir)?;
    // Read again with her lock held: the body that let her go may have
    // written since she was looked at.
    livable(peek.journal.unknown_kinds()?)?;
    peek.revised = recorded_seal(&peek.journal)?.as_ref() != Some(peek.identity.seal());
    Ok((peek, lock))
}

/// Refused if her journal has kinds this build does not know.
fn livable(unknown: Vec<String>) -> Result<()> {
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(Error::Newer(unknown))
    }
}

/// How long opening her waits for another body to let her go before she
/// is taken to be awake in it: one that is stopping, as a body started
/// again stops the one before, takes a moment to close her.
#[cfg(not(test))]
const LOCK_WAIT: Duration = Duration::from_secs(5);
#[cfg(test)]
const LOCK_WAIT: Duration = Duration::from_secs(1);

fn lock(dir: &Path) -> Result<File> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))?;
    let until = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(TryLockError::WouldBlock) => return Err(Error::Busy(dir.to_path_buf())),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
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
    fn her_avatar_and_bio_are_hers_and_optional() {
        let d = dir_with_seed();
        let g = Ghost::birth(d.path()).unwrap();
        assert_eq!(g.avatar(), None);
        assert_eq!(g.identity().seed().bio, None);
        drop(g);

        fs::create_dir_all(d.path().join(ASSETS_DIR)).unwrap();
        fs::write(d.path().join("assets/avatar.webp"), b"RIFF").unwrap();
        fs::write(
            d.path().join("ghost.toml"),
            SAMPLE.replace("origin = \"made\"", "origin = \"made\"\nbio = \"Quiet.\""),
        )
        .unwrap();
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(g.avatar(), Some(d.path().join("assets/avatar.webp")));
        assert_eq!(g.identity().seed().bio.as_deref(), Some("Quiet."));
    }

    #[test]
    fn born_once() {
        let d = dir_with_seed();
        drop(Ghost::birth(d.path()).unwrap());
        assert!(matches!(Ghost::birth(d.path()), Err(Error::AlreadyBorn(_))));
    }

    #[test]
    fn a_revised_persona_is_who_she_is_from_then_on() {
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
    fn a_journal_from_before_revisions_learns_her_persona_only_when_it_changes() {
        let d = dir_with_seed();
        // As earlier versions wrote her: birth alone.
        {
            fs::create_dir_all(d.path().join("derived")).unwrap();
            let mut journal = Journal::create(&d.path().join(JOURNAL_FILE)).unwrap();
            let id = identity::read_persona(d.path()).unwrap();
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
    fn a_persona_that_does_not_parse_is_refused() {
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
        let asked = std::time::Instant::now();
        assert!(matches!(Ghost::open(d.path()), Err(Error::Busy(_))));
        assert!(
            asked.elapsed() >= LOCK_WAIT,
            "given a moment to let go first"
        );
        // Looking does not need the lock.
        assert!(Ghost::peek(d.path()).is_ok());
        drop(first);
        assert!(Ghost::open(d.path()).is_ok());
    }

    #[test]
    fn a_body_letting_her_go_is_waited_for_by_the_next() {
        let d = dir_with_seed();
        let first = Ghost::birth(d.path()).unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(LOCK_WAIT / 10);
            drop(first);
        });
        assert!(Ghost::open(d.path()).is_ok());
        letting_go.join().unwrap();
    }

    fn canon(fact: &str) -> Event {
        Event::Canon {
            fact: fact.into(),
            source: "life".into(),
        }
    }

    fn facts(g: &Ghost) -> Vec<String> {
        let mut facts: Vec<String> = g
            .memory()
            .canon(10)
            .unwrap()
            .into_iter()
            .map(|f| f.fact)
            .collect();
        facts.sort();
        facts
    }

    /// Another connection to her memory that makes folding `fact` fail, the
    /// way a lock held too long or a full disk would.
    fn refuse(d: &Path, fact: &str) -> rusqlite::Connection {
        let other = rusqlite::Connection::open(d.join(crate::memory::MEMORY_FILE)).unwrap();
        other
            .execute_batch(&format!(
                "CREATE TRIGGER refuse BEFORE INSERT ON canon WHEN NEW.fact = '{fact}'
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;"
            ))
            .unwrap();
        other
    }

    #[test]
    fn an_entry_memory_could_not_fold_is_folded_by_her_next_write() {
        let d = dir_with_seed();
        let mut g = Ghost::birth(d.path()).unwrap();
        g.record(canon("has a cat")).unwrap();
        let other = refuse(d.path(), "lives by the river");

        let err = g.record(canon("lives by the river")).unwrap_err();
        let missed = g.journal().last_seq().unwrap();
        assert!(
            matches!(err, Error::Unfolded { seq, kind: "canon", .. } if seq == missed),
            "{err}"
        );
        // It is in her journal all the same.
        assert_eq!(g.memory().last_seq().unwrap(), missed - 1);

        other.execute_batch("DROP TRIGGER refuse").unwrap();
        g.record(canon("drinks tea")).unwrap();
        assert_eq!(facts(&g), ["drinks tea", "has a cat", "lives by the river"]);
        assert_eq!(
            g.memory().last_seq().unwrap(),
            g.journal().last_seq().unwrap()
        );
    }

    #[test]
    fn a_fold_that_keeps_failing_stops_her_at_that_entry() {
        let d = dir_with_seed();
        let mut g = Ghost::birth(d.path()).unwrap();
        let other = refuse(d.path(), "lives by the river");
        g.record(canon("lives by the river")).unwrap_err();
        let stuck = g.journal().last_seq().unwrap();

        // Later writes reach her journal, and memory goes no further.
        let err = g.record(canon("drinks tea")).unwrap_err();
        assert!(
            matches!(err, Error::Unfolded { seq, .. } if seq == stuck),
            "{err}"
        );
        drop(g);
        let Err(err) = Ghost::open(d.path()) else {
            panic!("she opened with an entry her memory could not fold");
        };
        assert!(
            matches!(err, Error::Unfolded { seq, .. } if seq == stuck),
            "{err}"
        );

        other.execute_batch("DROP TRIGGER refuse").unwrap();
        let g = Ghost::open(d.path()).unwrap();
        assert_eq!(facts(&g), ["drinks tea", "lives by the river"]);
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
