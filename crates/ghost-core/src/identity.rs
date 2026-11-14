//! The persona.
//!
//! Who she is is written in `ghost.toml`. It can be revised: when the file no
//! longer matches the version last written into her journal, the new text is
//! written there too (see [`Ghost::open`](crate::Ghost::open)), so every
//! version of her stays on record. An [`Identity`] is one version, read-only
//! while she runs; a revision takes effect the next time she is opened.

use std::{fmt, fs, path::Path};

use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// File name of the persona inside a ghost directory.
pub const SEED_FILE: &str = "ghost.toml";

/// The persona as written by whoever gives birth to her.
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
    /// Her own account of where she came from, including who made her. Kept
    /// short: it is in front of her in every conversation.
    pub origin: String,
    /// How she sounds, shown rather than described: a few short exchanges.
    /// They teach her voice; they are never lines to reuse.
    #[serde(default)]
    pub examples: String,
    /// Pieces of her past that come up in a conversation only when
    /// something said there touches them.
    #[serde(default)]
    pub past: Vec<Past>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub cares_about: Vec<String>,
    #[serde(default)]
    pub cannot_stand: Vec<String>,
}

/// One piece of her past, and the words that bring it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Past {
    /// Words that, said in a conversation, bring this to mind. Matched as
    /// plain text, ignoring case.
    pub keys: Vec<String>,
    pub text: String,
}

impl Past {
    /// Whether any of `texts` touches this.
    pub fn touched_by(&self, texts: &[&str]) -> bool {
        self.keys
            .iter()
            .map(|k| k.trim().to_lowercase())
            .any(|key| !key.is_empty() && texts.iter().any(|t| t.to_lowercase().contains(&key)))
    }
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
    /// Parse a persona and check that every field she depends on is sane.
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

/// The BLAKE3 hash of the persona text: which version of it this is.
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

/// One version of her persona: what it says, its text, and its seal.
#[derive(Debug, Clone)]
pub struct Identity {
    seed: Seed,
    text: String,
    seal: Seal,
}

impl Identity {
    /// Read a persona from its text.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self {
            seed: Seed::parse(text)?,
            text: text.to_owned(),
            seal: Seal::of(text.as_bytes()),
        })
    }

    /// The persona exactly as written.
    pub fn text(&self) -> &str {
        &self.text
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
    /// Never in practice: the zone was validated when the persona was parsed.
    pub fn tz(&self) -> Tz {
        self.seed.home.tz().expect("validated at parse")
    }
}

/// Read the persona in `dir`.
pub fn read_persona(dir: &Path) -> Result<Identity> {
    let path = dir.join(SEED_FILE);
    let bytes = fs::read(&path).map_err(|_| Error::NoSeed(path.clone()))?;
    let text = String::from_utf8(bytes).map_err(|e| Error::BadSeed(format!("not utf-8: {e}")))?;
    Identity::parse(&text)
}
#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"
name = "Weiwei"
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
        assert_eq!(seed.name, "Weiwei");
        assert_eq!(seed.home.tz().unwrap(), chrono_tz::Asia::Shanghai);
    }

    #[test]
    fn a_persona_may_show_her_voice_and_key_her_past() {
        // Plain keys go before the first table; `[[past]]` entries after it.
        let text = format!(
            "examples = \"them: hi\\nme: hey\"\n{SAMPLE}\n[[past]]\nkeys = [\"Clinic\", \"诊所\"]\ntext = \"She runs a clinic.\"\n"
        );
        let seed = Seed::parse(&text).unwrap();
        assert_eq!(seed.examples, "them: hi\nme: hey");
        let past = &seed.past[0];
        assert!(past.touched_by(&["is the clinic open?"]));
        assert!(past.touched_by(&["你的诊所在哪"]));
        assert!(!past.touched_by(&["hello", "how are you"]));
        // Personas written before these fields still read.
        assert!(Seed::parse(SAMPLE).unwrap().past.is_empty());
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
