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
//!
//! A machine that ran the desk before there were homes kept its core in a
//! checkout. Given that checkout ([`ready`]'s `adopt`), a home with no ghost
//! yet takes that core in, once: her journal and her work as SQLite copies
//! them, the rest of her directory but what is rebuilt from the journal,
//! the brains and operators of its configuration, and its `.env`. Her
//! journal in the checkout is closed with an entry saying where she lives
//! now ([`ghost_core::Ghost::leave`]), written while her lock there is
//! held from before the copy, so that nothing runs her there again: not
//! this build, nor an older desk, which does not know the entry and so
//! refuses her too. The rest of the checkout is not changed. A home that
//! took her in before that entry was written writes it the next time it
//! is given the checkout.

use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use ghost_core::Ghost;
use tracing::{info, warn};

use crate::config::Config;

/// Its configuration.
pub const CONFIG: &str = "ghostd.toml";
/// The directory of its ghosts, every one of which it runs.
pub const GHOSTS: &str = "ghosts";
/// The mascot's directory under [`GHOSTS`].
pub const SOLVER: &str = "solver";
/// Its keys.
pub(crate) const ENV: &str = ".env";
/// Where a ghost taken in is put together, outside [`GHOSTS`], so that one
/// cut short is never run.
const ADOPTING: &str = ".adopting";

/// Everything in the configuration but the brains.
const SETTINGS: &str = include_str!("../home/ghostd.toml");
/// The brains a new home thinks with.
const BRAINS: &str = include_str!("../home/brains.toml");
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
    /// The old core taken in, by its directory.
    pub adopted: Option<PathBuf>,
    /// The old core this home took in earlier, by its directory, marked
    /// only now as the one she left.
    pub marked: Option<PathBuf>,
}

/// Make the home at `home` ready to run, as the module says, taking in the
/// old core in the checkout `adopt` if it has no ghost yet.
pub fn ready(home: &Path, adopt: Option<&Path>) -> anyhow::Result<Made> {
    fs::create_dir_all(home).with_context(|| format!("make {}", home.display()))?;
    let ghosts = home.join(GHOSTS);
    let config = home.join(CONFIG);
    let mut made = Made {
        config: !config.is_file(),
        ..Made::default()
    };
    if !any_born(&ghosts)
        && let Some(old) = adopt.and_then(Old::find)
    {
        take_in(&old, home)?;
        made.adopted = Some(old.ghost);
    } else if let Some(checkout) = adopt {
        made.marked = mark_left(home, checkout);
    }
    if !config.is_file() {
        write_whole(&config, &format!("{SETTINGS}{BRAINS}"))?;
    }
    if !any_born(&ghosts) {
        let solver = ghosts.join(SOLVER);
        if !solver.join("ghost.toml").is_file() {
            fs::create_dir_all(&solver)?;
            fs::write(solver.join("ghost.toml"), PERSONA)?;
        }
        Ghost::birth(&solver).with_context(|| format!("birth in {}", solver.display()))?;
        made.born = true;
    }
    Ok(made)
}

/// Say what [`ready`] did, if it did anything.
pub fn say(home: &Path, made: &Made) {
    if let Some(old) = &made.adopted {
        info!(
            "{} took in the core from {}; that one is marked as the one she left, \
             to be read and never run again",
            home.display(),
            old.display()
        );
    }
    if let Some(old) = &made.marked {
        info!(
            "{} is the core {} took in earlier; it is marked now as the one she left, \
             to be read and never run again",
            old.display(),
            home.display()
        );
    }
    if made.config && made.adopted.is_none() {
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

/// The desk's core as a checkout kept it before there were homes.
#[derive(Debug)]
struct Old {
    ghost: PathBuf,
    config: Option<PathBuf>,
    env: Option<PathBuf>,
}

impl Old {
    /// The core in `checkout`, if one was born there: under `desk/core/`,
    /// where it was kept last, or where it was kept before that.
    fn find(checkout: &Path) -> Option<Self> {
        let first = |paths: &[&str]| paths.iter().map(|p| checkout.join(p)).find(|p| p.is_file());
        let ghost = OLD_PLACES
            .iter()
            .map(|p| checkout.join(p))
            .find(|p| unmoved(p))?;
        Some(Self {
            ghost,
            config: first(&["desk/core/ghostd.toml", "solver/ghostd.toml"]),
            env: first(&[ENV]),
        })
    }
}

/// Where a checkout kept its core, the last place first.
const OLD_PLACES: [&str; 2] = ["desk/core/ghosts/solver", "ghosts/solver"];

/// Whether a ghost was born in `dir` and has not moved out of it.
fn unmoved(dir: &Path) -> bool {
    dir.join(ghost_core::journal::JOURNAL_FILE).is_file()
        && matches!(ghost_core::ghost::moved(dir), Ok(None))
}

/// Her birth in `dir`, by the hash of its entry: the same in every copy of
/// her journal, and in no other.
fn birth(dir: &Path) -> Option<String> {
    let journal = ghost_core::journal::Journal::open(&dir.join(ghost_core::journal::JOURNAL_FILE));
    Some(journal.ok()?.first().ok()??.hash)
}

/// Mark the core in `checkout` as the one she left, if this home took it
/// in before taking one in marked what it left: the mascot here was born
/// in the same entry as a core there that has not moved out. Until it is,
/// whatever runs it (an older desk, `ghostd run` on the checkout) goes on
/// as a second her. Its directory, if it was marked.
fn mark_left(home: &Path, checkout: &Path) -> Option<PathBuf> {
    let solver = home.join(GHOSTS).join(SOLVER);
    let born = birth(&solver)?;
    let old = OLD_PLACES
        .iter()
        .map(|p| checkout.join(p))
        .find(|p| unmoved(p) && birth(p).as_ref() == Some(&born))?;
    let marked = std::path::absolute(&solver)
        .map_err(ghost_core::Error::from)
        .and_then(|to| Ghost::leave(&old)?.to(&to));
    match marked {
        Ok(_) => Some(old),
        Err(e) => {
            warn!(
                "{} is the core this home took in, and could not be marked as the one she \
                 left: {e}; run there, she would go on as a second her",
                old.display()
            );
            None
        }
    }
}

/// Take `old` into `home`, which has no ghost born.
fn take_in(old: &Old, home: &Path) -> anyhow::Result<()> {
    // Held from before she is copied until after: nothing is written to
    // her there in between, and then what is written is that she moved.
    let leaving = Ghost::leave(&old.ghost)
        .with_context(|| format!("take in the core in {}", old.ghost.display()))?;
    let staging = home.join(ADOPTING);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    copy_ghost(&old.ghost, &staging)?;
    // Hers whole before she is put where she runs.
    Ghost::peek(&staging)
        .with_context(|| format!("the copy of {} does not hold together", old.ghost.display()))?;
    let solver = home.join(GHOSTS).join(SOLVER);
    if solver.exists() {
        // Never born, or she would have been found: a template at most.
        fs::remove_dir_all(&solver)?;
    }
    fs::create_dir_all(home.join(GHOSTS))?;
    fs::rename(&staging, &solver).with_context(|| format!("move into {}", solver.display()))?;

    let config = home.join(CONFIG);
    if let Some(from) = old.config.as_deref().filter(|_| !config.is_file()) {
        match carried(from) {
            Ok(brains) => write_whole(&config, &format!("{SETTINGS}{brains}"))?,
            Err(e) => warn!(
                "kept the built-in brains, not those in {}: {e:#}",
                from.display()
            ),
        }
    }
    let env = home.join(ENV);
    if let Some(from) = old.env.as_deref().filter(|_| !env.exists()) {
        let keys = fs::read(from).with_context(|| format!("read {}", from.display()))?;
        private_file(&env)?.write_all(&keys)?;
    }
    // Last, since she is hers here whether or not it is written: if it is
    // not, the next time this home is given the checkout writes it.
    if let Err(e) = std::path::absolute(&solver)
        .map_err(ghost_core::Error::from)
        .and_then(|to| leaving.to(&to))
    {
        warn!(
            "{} could not be marked as the core she left: {e}; run there, she would go on \
             as a second her",
            old.ghost.display()
        );
    }
    Ok(())
}

/// The brains and operators of the configuration at `path`, as a part of a
/// configuration: `[brain]`, `[providers]` and `[operators]`, and nothing
/// else of it, since the rest is how the core lives, which is the binary's
/// to say. Refused if they would not run.
fn carried(path: &Path) -> anyhow::Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let old: toml::Table =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let mut kept = toml::Table::new();
    for key in ["brain", "providers", "operators"] {
        if let Some(value) = old.get(key) {
            kept.insert(key.to_owned(), value.clone());
        }
    }
    let part = format!(
        "# Carried over from {} when this home took in the core kept there.\n{}",
        path.display(),
        toml::to_string(&kept)?
    );
    let cfg = Config::parse(&format!("{SETTINGS}{part}"))?;
    for tier in [&cfg.brain.think, &cfg.brain.tick] {
        if !cfg.providers.contains_key(tier) {
            bail!("[brain] names {tier:?}, which is not among its providers");
        }
    }
    Ok(part)
}

/// Copy the ghost directory `from` to `to`: each SQLite database through
/// SQLite, which gives a whole copy even while a daemon writes to it; what
/// is rebuilt from the journal, and the files SQLite keeps beside a
/// database, not at all; the rest as it is.
fn copy_ghost(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let (source, target) = (entry.path(), to.join(&*name));
        if name == "derived"
            || name == ".DS_Store"
            || name.ends_with(".db-wal")
            || name.ends_with(".db-shm")
        {
            continue;
        }
        if name.ends_with(".db") && entry.file_type()?.is_file() {
            copy_database(&source, &target)?;
        } else {
            copy_tree(&source, &target)?;
        }
    }
    Ok(())
}

/// A whole copy of the database at `from`, read without writing to it.
/// With no write-ahead log beside it, nobody has it open to write, and it
/// is read as a file that does not change: read otherwise, SQLite would
/// leave a log and an index of it beside a database that had none.
fn copy_database(from: &Path, to: &Path) -> anyhow::Result<()> {
    use rusqlite::{Connection, OpenFlags};
    let mut wal = from.as_os_str().to_owned();
    wal.push("-wal");
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let source = if Path::new(&wal).exists() {
        Connection::open_with_flags(from, flags)
    } else {
        Connection::open_with_flags(
            format!("file:{}?immutable=1", uri_path(from)),
            flags | OpenFlags::SQLITE_OPEN_URI,
        )
    }
    .with_context(|| format!("open {}", from.display()))?;
    source
        .backup(
            rusqlite::MAIN_DB,
            to,
            None::<fn(rusqlite::backup::Progress)>,
        )
        .with_context(|| format!("copy {} to {}", from.display(), to.display()))
}

/// `path` as the path of a `file:` URI: every byte but those that stand
/// for themselves written as `%XX`.
fn uri_path(path: &Path) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Copy a file, a link or a directory and everything in it.
fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    let kind = fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        let points = fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&points, to)?;
        #[cfg(not(unix))]
        warn!("left out the link {}", from.display());
    } else if kind.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        fs::copy(from, to).with_context(|| format!("copy {}", from.display()))?;
    }
    Ok(())
}

/// Write `text` to `path` whole: beside it first, then renamed, so that a
/// write cut short never leaves half a file under its name.
pub(crate) fn write_whole(path: &Path, text: &str) -> anyhow::Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, text).with_context(|| format!("write {}", part.display()))?;
    fs::rename(&part, path).with_context(|| format!("write {}", path.display()))
}

/// A new file only its owner may read.
fn private_file(path: &Path) -> anyhow::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .with_context(|| format!("create {}", path.display()))
}

#[cfg(test)]
mod tests;
