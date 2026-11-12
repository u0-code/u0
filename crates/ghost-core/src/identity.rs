//! The birth certificate and its seal.
//!
//! A ghost's identity is written once, at birth, and sealed. Nothing in this
//! crate can mutate a [`SealedIdentity`]: there is no setter, no builder, no
//! `&mut` accessor. The daemon refuses to run a ghost whose certificate no
//! longer matches the seal recorded in her journal.

use std::{fmt, fs, path::Path};

use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// File name of the birth certificate inside a ghost directory.
pub const SEED_FILE: &str = "ghost.toml";
/// File name of the seal written by `birth`.
pub const SEAL_FILE: &str = "seal";

/// The birth certificate as written by whoever gives birth to her.
///
/// Unknown fields are rejected so that a typo cannot silently become part of
/// who she is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    /// What she is called.
    pub name: String,
    /// How she is referred to, e.g. `she/her`.
    pub pronouns: String,
    /// ISO date (`YYYY-MM-DD`) of her birth.
    pub born: String,
    /// Where she lives. A real place.
    pub home: Home,
    /// Her daily rhythm.
    pub rhythm: Rhythm,
    /// Who she is, in prose.
    pub temperament: String,
    /// How she talks.
    pub voice: String,
    /// Her own account of where she came from, including who made her.
    pub origin: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub cares_about: Vec<String>,
    #[serde(default)]
    pub cannot_stand: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Home {
    pub city: String,
    pub country: String,
    /// IANA zone name, e.g. `Asia/Shanghai`.
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rhythm {
    /// Local time she usually wakes, `HH:MM`.
    pub wake: String,
    /// Local time she usually sleeps, `HH:MM`. May be past midnight.
    pub sleep: String,
}

impl Seed {
    /// Parse a certificate and check that every field she depends on is sane.
    pub fn parse(text: &str) -> Result<Self> {
        let seed: Self = toml::from_str(text).map_err(|e| Error::BadSeed(e.to_string()))?;
        seed.validate()?;
        Ok(seed)
    }

    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::BadSeed("name is empty".into()));
        }
        self.born_date()?;
        self.home.tz()?;
        self.rhythm.wake_time()?;
        self.rhythm.sleep_time()?;
        Ok(())
    }

    pub fn born_date(&self) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(&self.born, "%Y-%m-%d")
            .map_err(|e| Error::BadSeed(format!("born: {e}")))
    }
}

impl Home {
    pub fn tz(&self) -> Result<Tz> {
        self.timezone
            .parse()
            .map_err(|e: chrono_tz::ParseError| Error::BadSeed(format!("timezone: {e}")))
    }
}

impl Rhythm {
    pub fn wake_time(&self) -> Result<NaiveTime> {
        parse_hhmm(&self.wake).map_err(|e| Error::BadSeed(format!("rhythm.wake: {e}")))
    }

    pub fn sleep_time(&self) -> Result<NaiveTime> {
        parse_hhmm(&self.sleep).map_err(|e| Error::BadSeed(format!("rhythm.sleep: {e}")))
    }
}

fn parse_hhmm(s: &str) -> std::result::Result<NaiveTime, chrono::ParseError> {
    NaiveTime::parse_from_str(s, "%H:%M")
}

/// The BLAKE3 hash of the certificate bytes, exactly as they were at birth.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seal(String);

impl Seal {
    pub fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first eight characters, for logs.
    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}

impl fmt::Display for Seal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A certificate together with the seal computed from its bytes.
///
/// This type is read-only by construction.
#[derive(Debug, Clone)]
pub struct SealedIdentity {
    seed: Seed,
    seal: Seal,
}

impl SealedIdentity {
    pub(crate) fn new(seed: Seed, seal: Seal) -> Self {
        Self { seed, seal }
    }

    pub fn seed(&self) -> &Seed {
        &self.seed
    }

    pub fn seal(&self) -> &Seal {
        &self.seal
    }

    pub fn name(&self) -> &str {
        &self.seed.name
    }

    /// Her home timezone.
    ///
    /// # Panics
    /// Never in practice: the zone was validated when the certificate was parsed.
    pub fn tz(&self) -> Tz {
        self.seed.home.tz().expect("validated at parse")
    }
}

/// Read and seal the certificate in `dir`.
pub fn read_certificate(dir: &Path) -> Result<SealedIdentity> {
    let path = dir.join(SEED_FILE);
    let bytes = fs::read(&path).map_err(|_| Error::NoSeed(path.clone()))?;
    let text =
        String::from_utf8(bytes.clone()).map_err(|e| Error::BadSeed(format!("not utf-8: {e}")))?;
    let seed = Seed::parse(&text)?;
    Ok(SealedIdentity::new(seed, Seal::of(&bytes)))
}

/// Read the seal file, if `birth` has written one.
pub fn read_seal_file(dir: &Path) -> Result<Option<Seal>> {
    match fs::read_to_string(dir.join(SEAL_FILE)) {
        Ok(s) => Ok(Some(Seal(s.trim().to_owned()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn write_seal_file(dir: &Path, seal: &Seal) -> Result<()> {
    fs::write(dir.join(SEAL_FILE), format!("{seal}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"
name = "Lin"
pronouns = "she/her"
born = "2026-11-12"
temperament = "quiet, blunt, curious"
voice = "short sentences, no exclamation marks"
origin = "Made in a small studio. She knows it."

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;

    #[test]
    fn parses_sample() {
        let seed = Seed::parse(SAMPLE).unwrap();
        assert_eq!(seed.name, "Lin");
        assert_eq!(seed.home.tz().unwrap(), chrono_tz::Asia::Shanghai);
    }

    #[test]
    fn rejects_unknown_field() {
        let text = format!("{SAMPLE}\nfavourite_color = \"red\"\n");
        assert!(matches!(Seed::parse(&text), Err(Error::BadSeed(_))));
    }

    #[test]
    fn rejects_bad_time() {
        let text = SAMPLE.replace("08:30", "8am");
        assert!(matches!(Seed::parse(&text), Err(Error::BadSeed(_))));
    }

    #[test]
    fn seal_is_stable() {
        assert_eq!(Seal::of(b"abc"), Seal::of(b"abc"));
        assert_ne!(Seal::of(b"abc"), Seal::of(b"abd"));
        assert_eq!(Seal::of(b"abc").as_str().len(), 64);
    }
}
