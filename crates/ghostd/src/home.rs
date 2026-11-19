//! A home: one directory that holds everything the core of a machine
//! keeps, so that whatever starts it (the desktop app, a service, a person)
//! needs to know nothing but where the directory is.
//!
//! ```text
//! <home>/
//!   ghostd.toml       its configuration: written once, then yours
//!   .env              its keys, if any: never in the configuration
//!   ghosts/solver/    Ghost, the mascot; any other ghost put in ghosts/ runs too
//!   core-id           which core this is, made the first time it runs (`host`)
//! ```
//!
//! ghostd makes a home ready by itself ([`ready`]): what is missing is
//! written, the mascot is born if no ghost has been, and nothing already
//! there is changed. The configuration, the mascot's `ghost.toml` and the
//! desk's prompts are built into the binary, so a home needs no checkout;
//! the prompts are not written into it at all, and so follow the binary
//! that runs it rather than going stale in a copy nobody asked for.

use std::{fs, path::Path};

use anyhow::Context;
use ghost_brain::presets::Preset;
use ghost_core::Ghost;
use tracing::{info, warn};

/// Its configuration.
pub const CONFIG: &str = "ghostd.toml";
/// The directory of its ghosts, every one of which it runs.
pub const GHOSTS: &str = "ghosts";
/// The mascot's directory under [`GHOSTS`].
pub const SOLVER: &str = "solver";
/// Its keys.
pub(crate) const ENV: &str = ".env";

/// Everything in the configuration but the brains.
const SETTINGS: &str = include_str!("../home/ghostd.toml");
/// The brains a new home has, by their presets: she thinks and ticks with
/// the first.
const BRAINS: [&str; 3] = ["deepseek", "claude-code", "codex"];
/// What the mascot is born from.
const PERSONA: &str = include_str!("../home/ghosts/solver/ghost.toml");

/// The desk's persona and being, built in for every ghost a home runs in
/// place of a character's.
pub const PROMPTS: &[(&str, &str)] = &[
    (
        "character/persona.md",
        include_str!("../../../prompts/system/desk/character/persona.md"),
    ),
    (
        "system/being.md",
        include_str!("../../../prompts/system/desk/system/being.md"),
    ),
];

/// What [`ready`] did to a home.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Made {
    /// The configuration was written.
    pub config: bool,
    /// The mascot was born.
    pub born: bool,
}

/// Make the home at `home` ready to run, as the module says.
pub fn ready(home: &Path) -> anyhow::Result<Made> {
    fs::create_dir_all(home).with_context(|| format!("make {}", home.display()))?;
    let ghosts = home.join(GHOSTS);
    let config = home.join(CONFIG);
    let mut made = Made {
        config: !config.is_file(),
        born: false,
    };
    if made.config {
        write_whole(&config, &format!("{SETTINGS}{}", brains()))?;
    }
    if !any_born(&ghosts) {
        let solver = ghosts.join(SOLVER);
        if !solver.join("ghost.toml").is_file() {
            fs::create_dir_all(&solver)?;
            write_whole(&solver.join("ghost.toml"), PERSONA)?;
        }
        Ghost::birth(&solver).with_context(|| format!("birth in {}", solver.display()))?;
        made.born = true;
    }
    Ok(made)
}

/// The brains of a new home's configuration, each as its preset has it.
fn brains() -> String {
    let presets: Vec<(&Preset, Vec<String>)> = BRAINS
        .iter()
        .filter_map(|id| Preset::by_id(id))
        .map(|p| (p, p.lines(None)))
        .collect();
    let brains: Vec<(&str, &str, &[String])> = presets
        .iter()
        .map(|(p, lines)| (p.id, p.name, lines.as_slice()))
        .collect();
    format!(
        "# The brains it can think with. Whoever runs it picks among these and adds\n\
         # to them over the Ghost Protocol (the desk does, from the line you type\n\
         # into), and ghostd edits this file for them, keeping your comments. A key\n\
         # is never written here, only the variable it is read from: given with a\n\
         # brain, it goes into .env.\n{}",
        crate::config::brains(&brains)
    )
}

/// Say what [`ready`] did, if it did anything.
pub fn say(home: &Path, made: &Made) {
    if made.config {
        info!("wrote {}", home.join(CONFIG).display());
    }
    if made.born {
        info!(
            "the solver is born in {}",
            home.join(GHOSTS).join(SOLVER).display()
        );
    }
}

/// Load the home's `.env`, under whatever the environment already sets.
pub fn load_env(home: &Path) {
    let path = home.join(ENV);
    if path.is_file()
        && let Err(e) = dotenvy::from_path(&path)
    {
        warn!("could not read {}: {e}", path.display());
    }
}

/// Whether any ghost in `ghosts` has been born.
fn any_born(ghosts: &Path) -> bool {
    fs::read_dir(ghosts).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.path().join(ghost_core::journal::JOURNAL_FILE).is_file())
    })
}

/// Write `text` to `path` whole, as [`ghost_core::whole`] does: a crash
/// leaves the file as it was or as it is now, never half of it.
pub(crate) fn write_whole(path: &Path, text: &str) -> anyhow::Result<()> {
    ghost_core::whole::write(path, text.as_bytes())
        .with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests;
