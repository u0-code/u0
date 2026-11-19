//! Where a daemon runs, as `ghost.status` tells a console: which daemon it
//! is, by an id it makes once and keeps, and which computer it is on.
//!
//! The paths a character gives (her machine's directory, the folders of her
//! work, the files it hands over) are paths on this computer. A console can
//! open them only from here, and it finds out whether it is here by reading
//! the file the id is kept in: finding the same id there is finding this
//! daemon's own file system. The host name is for saying where it is; two
//! computers can share one, so it decides nothing.
//!
//! The id is made once. A file that holds anything else is not replaced:
//! a console keeps what it was given of this core under the id, and finds
//! it is here by finding the id in the file, so a new one would leave what
//! it kept behind and, while a daemon that read the old one runs, have it
//! take this core for one somewhere else. The file is written whole, so a
//! crash cannot leave it so; whatever did is for a person to see, and the
//! daemon does not start until they have.

use std::{
    fs,
    hash::{BuildHasher as _, Hasher as _},
    io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use ghost_protocol::CoreInfo;
use tracing::warn;

/// The file the id is kept in.
pub const FILE: &str = "core-id";

/// Where a daemon keeps its id: its home; run without one, the directory
/// its first character's directory is in, beside her as a home keeps it
/// beside its `ghosts/`. What a daemon keeps goes where its characters are
/// written, never beside its configuration, which is often somewhere it
/// may only read (`/etc` under systemd) or a checkout. Two daemons running
/// characters from one directory share the id, as they share the file
/// system it stands for; her directory's name keeps each apart.
pub(crate) fn keeps_in(home: Option<&Path>, first: &Path) -> PathBuf {
    if let Some(home) = home {
        return home.to_path_buf();
    }
    let full = first.canonicalize().unwrap_or_else(|_| first.to_path_buf());
    full.parent()
        .map_or_else(|| full.clone(), Path::to_path_buf)
}

/// This daemon as it says where it runs: its id, read from `dir` or made
/// and written there the first time, the full path of the file it is in,
/// and this computer's name. Where it cannot be kept, the daemon still
/// runs, with an id of this run's that no console will find: its paths
/// are then text everywhere, which is less than it could offer and never
/// wrong. Refused if the file is there and holds no id, or cannot be read.
pub(crate) fn core(dir: &Path) -> anyhow::Result<CoreInfo> {
    let file = dir.join(FILE);
    let (id, id_file) = match kept(&file)? {
        Some(kept) => kept,
        None => (fresh(), file.to_string_lossy().into_owned()),
    };
    Ok(CoreInfo {
        id,
        id_file,
        host: host_name(),
    })
}

/// The id kept in `file`, made and written there if there is no file, and
/// the file's full path; `None` if it cannot be kept there.
fn kept(file: &Path) -> anyhow::Result<Option<(String, String)>> {
    let id = match fs::read_to_string(file) {
        Ok(kept) if is_id(kept.trim()) => kept.trim().to_owned(),
        Ok(kept) => {
            let shown: String = kept.trim().chars().take(40).collect();
            bail!(
                "{} holds {shown:?}, which is not this core's id (32 hexadecimal digits). \
                 Consoles know this core by its id, so it is not replaced here: put the id \
                 back, or remove the file to have a new one made",
                file.display()
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let id = fresh();
            if let Err(e) = ghost_core::whole::write(file, format!("{id}\n").as_bytes()) {
                warn!(
                    "keep this core's id in {}: {e}; no console will open the paths she gives",
                    file.display()
                );
                return Ok(None);
            }
            id
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "read this core's id from {}; it is not replaced while it is there",
                    file.display()
                )
            });
        }
    };
    match file.canonicalize() {
        Ok(full) => Ok(Some((id, full.to_string_lossy().into_owned()))),
        Err(e) => {
            warn!(
                "find {}: {e}; no console will open the paths she gives",
                file.display()
            );
            Ok(None)
        }
    }
}

/// 32 hexadecimal digits.
fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A new id: 128 bits from the keys the standard library seeds from the
/// system's randomness for every hash map, mixed with the time and this
/// process. It has to be unlike any other daemon's, not secret.
fn fresh() -> String {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let half = |n: u8| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u8(n);
        h.write_u128(at);
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", half(0), half(1))
}

/// This computer's name, as the system gives it, if it does: the kernel's
/// on Linux, `hostname` elsewhere.
fn host_name() -> Option<String> {
    let name = fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        })?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_keeps_the_id_it_made_and_another_makes_its_own() {
        let one = tempfile::tempdir().unwrap();
        let first = core(one.path()).unwrap();
        assert!(is_id(&first.id), "{}", first.id);
        assert_eq!(
            Path::new(&first.id_file),
            one.path().join(FILE).canonicalize().unwrap()
        );
        assert_eq!(fs::read_to_string(&first.id_file).unwrap().trim(), first.id);
        assert_eq!(core(one.path()).unwrap().id, first.id, "started again");

        let two = tempfile::tempdir().unwrap();
        assert_ne!(core(two.path()).unwrap().id, first.id);
    }

    #[test]
    fn a_file_that_holds_no_id_stops_the_daemon_and_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        for held in ["", "5f0c3a9e\n", "not an id\n"] {
            fs::write(&file, held).unwrap();
            let why = format!("{:#}", core(dir.path()).unwrap_err());
            assert!(
                why.contains(&file.display().to_string())
                    && why.contains("remove the file to have a new one made"),
                "{why}"
            );
            assert_eq!(fs::read_to_string(&file).unwrap(), held, "not replaced");
        }
        // Removed, a new one is made.
        fs::remove_file(&file).unwrap();
        let made = core(dir.path()).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), made.id);
    }

    #[test]
    fn a_daemon_that_cannot_keep_its_id_still_runs_and_is_found_by_nobody() {
        let nowhere = tempfile::tempdir().unwrap().path().join("gone");
        let core = core(&nowhere).unwrap();
        assert!(is_id(&core.id));
        assert!(
            fs::read_to_string(&core.id_file).is_err(),
            "no console finds it, so none offers to open her paths"
        );
    }

    #[test]
    fn the_id_is_kept_in_the_home_else_where_her_directory_is_never_beside_the_configuration() {
        let home = Path::new("/home/me/Ghost");
        assert_eq!(keeps_in(Some(home), Path::new("/srv/weiwei")), home);

        // As the systemd unit runs her: `ghostd run /var/lib/ghost/weiwei
        // --config /etc/ghost/ghostd.toml`, where /etc is hers only to read.
        let root = tempfile::tempdir().unwrap();
        let (etc, lib) = (root.path().join("etc"), root.path().join("lib"));
        let her = lib.join("weiwei");
        fs::create_dir_all(&her).unwrap();
        fs::create_dir_all(&etc).unwrap();
        fs::write(etc.join("ghostd.toml"), "").unwrap();
        let lib = lib.canonicalize().unwrap();
        assert_eq!(keeps_in(None, &her), lib);
        assert_eq!(keeps_in(None, &her.join(".")), lib, "named from inside");

        let made = core(&keeps_in(None, &her)).unwrap();
        assert_eq!(Path::new(&made.id_file), lib.join(FILE));
        assert_eq!(core(&keeps_in(None, &her)).unwrap().id, made.id, "again");
        assert!(!etc.join(FILE).exists() && !her.join(FILE).exists());
    }
}
