//! Files the desk rewrites, written whole: read back, and after a crash or
//! a power cut, each is what was there before or all of what was written,
//! never a part of it.
//!
//! The bytes go to a new file beside the old one and are synced to the
//! disk, that file is renamed over the old, and the directory is synced so
//! that the rename is kept too. On macOS `File::sync_all` is `F_FULLFSYNC`:
//! about 10 ms a write on an Apple SSD, which is why the cache is written
//! off the window's thread. (The core has its own, `ghost_core::whole`; the
//! desk does not build on `ghost-core`.)

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Write `bytes` to `path` whole, making its directory if need be. A file
/// already there keeps its permissions.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir_of(path))?;
    let aside = aside(path, bytes)?;
    if let Err(e) = fs::rename(&aside, path) {
        let _ = fs::remove_file(&aside);
        return Err(e);
    }
    // The file under its name is whole either way; this only makes the
    // rename outlive a power cut, so a directory that cannot be synced
    // does not make the write a failure.
    #[cfg(unix)]
    let _ = fs::File::open(dir_of(path)).and_then(|d| d.sync_all());
    Ok(())
}

/// `bytes` written and synced in a new file beside `path`, under a hidden
/// name of its own, so that two writers never write into one. A crash
/// before the rename leaves this file behind and `path` as it was.
fn aside(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
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
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&aside)
        .and_then(|mut file| {
            if let Some(was) = was {
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

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ghost-desk-whole-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn left_aside(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".partial")
            })
            .count()
    }

    #[test]
    fn a_write_cut_short_before_the_rename_leaves_the_old_file_as_it_was() {
        let dir = scratch("cut");
        let path = dir.join("desk.json");
        write(&path, b"{\"connections\": [1]}").unwrap();

        // Everything up to the rename, and then the crash.
        aside(&path, b"{\"connections\": [1, 2").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"connections\": [1]}");
        assert_eq!(left_aside(&dir), 1, "what the crash left");

        // The next write is whole, and leaves nothing of its own behind.
        write(&path, b"{\"connections\": [1, 2]}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"connections\": [1, 2]}");
        assert_eq!(left_aside(&dir), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_write_makes_its_directory_and_one_that_fails_leaves_nothing_beside() {
        let dir = scratch("dirs");
        write(&dir.join("cache/a.json"), b"{}").unwrap();
        assert_eq!(fs::read(dir.join("cache/a.json")).unwrap(), b"{}");

        // A directory where the file would go is not written over.
        fs::create_dir(dir.join("taken")).unwrap();
        assert!(write(&dir.join("taken"), b"{}").is_err());
        assert!(dir.join("taken").is_dir());
        assert_eq!(left_aside(&dir), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_rewritten_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("mode");
        let path = dir.join("desk.json");
        fs::write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write(&path, b"{\"who\": \"me\"}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
