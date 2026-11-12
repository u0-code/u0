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
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Error, Result,
    identity::{self, SealedIdentity},
    journal::{Event, JOURNAL_FILE, Journal},
};

pub struct Ghost {
    dir: PathBuf,
    identity: SealedIdentity,
    journal: Journal,
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
        let mut journal = Journal::create(&dir.join(JOURNAL_FILE))?;
        journal.append(Event::Born {
            name: identity.name().to_owned(),
            seal: identity.seal().clone(),
        })?;
        identity::write_seal_file(dir, identity.seal())?;
        Ok(Self {
            dir: dir.to_path_buf(),
            identity,
            journal,
        })
    }

    /// Open a ghost that was already born. Verifies that she is still her.
    ///
    /// Three things must agree: the certificate on disk, the seal file, and
    /// the `Born` entry at the head of her journal. Then the whole chain is
    /// verified. There is no flag to skip any of this.
    pub fn open(dir: &Path) -> Result<Self> {
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
        Ok(Self {
            dir: dir.to_path_buf(),
            identity,
            journal,
        })
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

    pub fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
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
    }

    #[test]
    fn born_once() {
        let d = dir_with_seed();
        Ghost::birth(d.path()).unwrap();
        assert!(matches!(Ghost::birth(d.path()), Err(Error::AlreadyBorn(_))));
    }

    #[test]
    fn edited_certificate_is_not_her() {
        let d = dir_with_seed();
        Ghost::birth(d.path()).unwrap();
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
}
