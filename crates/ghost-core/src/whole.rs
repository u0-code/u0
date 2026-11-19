//! Small files written whole: whoever reads one, and the disk after a
//! crash or a power cut, finds either what was there before or all of what
//! was written, never a part of it.
//!
//! The bytes go to a new file beside the old one and are synced to the
//! disk, that file is renamed over the old, and the directory is synced so
//! that the rename is kept too. On macOS `File::sync_all` is `F_FULLFSYNC`,
//! which a plain `fsync` there is not.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Write `bytes` to `path` whole. A file already there keeps its
/// permissions.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_as(path, bytes, false)
}

/// Write `bytes` to `path` whole, as a file only its owner may read or
/// write, whatever it was before: for keys.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_as(path, bytes, true)
}

fn write_as(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let aside = aside(path, bytes, private)?;
    if let Err(e) = fs::rename(&aside, path) {
        let _ = fs::remove_file(&aside);
        return Err(e);
    }
    // The file under its name is whole either way; this only makes the
    // rename outlive a power cut, so a directory that cannot be synced
    // (not every system can) does not make the write a failure.
    #[cfg(unix)]
    let _ = fs::File::open(dir_of(path)).and_then(|d| d.sync_all());
    Ok(())
}

/// `bytes` written and synced in a new file beside `path`, under a hidden
/// name of its own, so that two writers never write into one. A crash
/// before the rename leaves this file behind and `path` as it was.
fn aside(path: &Path, bytes: &[u8], private: bool) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} names no file", path.display()),
        )
    })?;
    let aside = dir_of(path).join(format!(
        ".{}.{}-{}.partial",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let was = fs::metadata(path).ok().map(|m| m.permissions());
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    let written = options.open(&aside).and_then(|mut file| {
        if !private && let Some(was) = was {
            file.set_permissions(was)?;
        }
        io::Write::write_all(&mut file, bytes)?;
        file.sync_all()
    });
    match written {
        Ok(()) => Ok(aside),
        Err(e) => {
            let _ = fs::remove_file(&aside);
            Err(e)
        }
    }
}

fn dir_of(path: &Path) -> &Path {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left_aside(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".partial"))
            .collect()
    }

    #[test]
    fn a_write_cut_short_before_the_rename_leaves_the_old_file_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desk.json");
        write(&path, b"{\"old\": true}").unwrap();

        // Everything up to the rename, and then the crash.
        let cut = aside(&path, b"{\"new\": tr", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"old\": true}");
        assert_eq!(left_aside(dir.path()).len(), 1, "what the crash left");
        assert!(cut.file_name().unwrap().to_string_lossy().starts_with('.'));

        // The next write is whole, and leaves nothing of its own behind.
        write(&path, b"{\"new\": true}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"new\": true}");
        assert_eq!(left_aside(dir.path()).len(), 1);
    }

    #[test]
    fn a_write_that_fails_leaves_the_old_file_and_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("taken");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inside"), b"kept").unwrap();
        assert!(write(&path, b"over a directory").is_err());
        assert_eq!(fs::read(path.join("inside")).unwrap(), b"kept");
        assert!(left_aside(dir.path()).is_empty());
        assert!(write(Path::new("/"), b"x").is_err(), "no file named");
    }

    #[cfg(unix)]
    #[test]
    fn keys_stay_the_owners_alone_and_other_files_keep_their_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();

        let env = dir.path().join(".env");
        fs::write(&env, b"OLD=1\n").unwrap();
        fs::set_permissions(&env, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&env, b"KEY=v\n").unwrap();
        assert_eq!(mode(&env), 0o600, "made private when rewritten");
        write_private(&dir.path().join(".env.new"), b"KEY=v\n").unwrap();
        assert_eq!(mode(&dir.path().join(".env.new")), 0o600, "and when new");

        let config = dir.path().join("ghostd.toml");
        fs::write(&config, b"a = 1\n").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        write(&config, b"a = 2\n").unwrap();
        assert_eq!(mode(&config), 0o640);
        assert_eq!(fs::read(&config).unwrap(), b"a = 2\n");
    }
}
