//! Where a daemon runs, as `ghost.status` tells a console: which daemon it
//! is, by an id it makes once and keeps, and which computer it is on.
//!
//! The paths a character gives (her machine's directory, the folders of her
//! work, the files it hands over) are paths on this computer. A console can
//! open them only from here, and it finds out whether it is here by reading
//! the file the id is kept in: finding the same id there is finding this
//! daemon's own file system. The host name is for saying where it is; two
//! computers can share one, so it decides nothing.

use std::{
    fs,
    hash::{BuildHasher as _, Hasher as _},
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use ghost_protocol::CoreInfo;

/// The file the id is kept in.
pub const FILE: &str = "core-id";

/// Where a daemon keeps its id: its home; run without one, the directory
/// of the configuration it read; with neither, the directory of the first
/// character it runs, which is always there.
pub(crate) fn keeps_in(home: Option<&Path>, config: Option<&Path>, first: &Path) -> PathBuf {
    home.map(Path::to_path_buf)
        .or_else(|| {
            let dir = config?.parent()?;
            Some(if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir.to_path_buf()
            })
        })
        .unwrap_or_else(|| first.to_path_buf())
}

/// This daemon as it says where it runs: its id, read from `dir` or made
/// and written there the first time, the full path of the file it is in,
/// and this computer's name.
pub(crate) fn core(dir: &Path) -> anyhow::Result<CoreInfo> {
    let file = dir.join(FILE);
    let id = match fs::read_to_string(&file) {
        Ok(kept) if is_id(kept.trim()) => kept.trim().to_owned(),
        _ => {
            let id = fresh();
            fs::write(&file, format!("{id}\n"))
                .with_context(|| format!("write {}", file.display()))?;
            id
        }
    };
    let file = file
        .canonicalize()
        .with_context(|| format!("find {}", file.display()))?;
    Ok(CoreInfo {
        id,
        id_file: file.to_string_lossy().into_owned(),
        host: host_name(),
    })
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

        // A file that holds no id is made again, not believed.
        fs::write(one.path().join(FILE), "not an id\n").unwrap();
        let remade = core(one.path()).unwrap();
        assert!(is_id(&remade.id) && remade.id != first.id);
    }

    #[test]
    fn the_id_is_kept_in_the_home_else_beside_the_configuration_else_with_her() {
        let her = Path::new("/srv/ghosts/weiwei");
        assert_eq!(
            keeps_in(
                Some(Path::new("/home/me/Ghost")),
                Some(Path::new("/etc/ghostd.toml")),
                her
            ),
            Path::new("/home/me/Ghost")
        );
        assert_eq!(
            keeps_in(None, Some(Path::new("/etc/ghost/ghostd.toml")), her),
            Path::new("/etc/ghost")
        );
        assert_eq!(
            keeps_in(None, Some(Path::new("ghostd.toml")), her),
            Path::new(".")
        );
        assert_eq!(keeps_in(None, None, her), her);
    }
}
