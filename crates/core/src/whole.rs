//! Files the desk rewrites, written whole: read back, and after a crash or
//! a power cut, each is what was there before or all of what was written,
//! never a part of it.
//!
//! The bytes go to a new file beside the old one and are synced to the
//! disk, that file is renamed over the old, and the directory is synced so
//! that the rename is kept too. On macOS `File::sync_all` is `F_FULLFSYNC`:
//! about 10 ms a write on an Apple SSD, which is why the cache is written
//! off the window's thread.
//!
//! Read back, a JSON file that is not there is the default, and one that
//! does not read is refused with why, so that nothing is written over it.

use std::{fs, io, path::Path};

use tempfile::NamedTempFile;

/// The JSON at `path`; the default where there is no file, and why where
/// it does not read.
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        text => text
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            .map_err(|e| format!("{} does not read ({e})", path.display())),
    }
}

/// Write `bytes` to `path` whole, making its directory if need be; it is
/// written readable and writable by you alone (0600).
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir_of(path))?;
    aside(path, bytes)?.persist(path).map_err(|e| e.error)?;
    // The file under its name is whole either way; this only makes the
    // rename outlive a power cut, so a directory that cannot be synced
    // does not make the write a failure.
    #[cfg(unix)]
    let _ = fs::File::open(dir_of(path)).and_then(|d| d.sync_all());
    Ok(())
}

/// `bytes` written and synced in a new file beside `path`, under a hidden
/// name of its own, so that two writers never write into one. It is
/// removed if it is dropped; a crash before the rename leaves it behind
/// and `path` as it was.
fn aside(path: &Path, bytes: &[u8]) -> io::Result<NamedTempFile> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} names no file", path.display()),
        )
    })?;
    let prefix = format!(".{}.", name.to_string_lossy());
    let mut made = tempfile::Builder::new();
    made.prefix(&prefix).suffix(".partial");
    // Readable and writable by you alone, whatever it was: u0's files
    // hold tokens (a named tunnel's, a webhook's address) and what you
    // said.
    #[cfg(unix)]
    made.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600));
    let mut file = made.tempfile_in(dir_of(path))?;
    io::Write::write_all(&mut file, bytes)?;
    file.as_file().sync_all()?;
    Ok(file)
}

fn dir_of(path: &Path) -> &Path {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_file_written_is_yours_alone_whatever_it_was() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write(&path, b"{\"devices\": []}").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let new = dir.path().join("tasks.json");
        write(&new, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&new).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_write_cut_short_before_the_rename_leaves_the_old_file_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desk.json");
        write(&path, b"{\"connections\": [1]}").unwrap();

        // Everything up to the rename, and then the crash: nothing after
        // it runs, so nothing removes the file set aside.
        std::mem::forget(aside(&path, b"{\"connections\": [1, 2").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"{\"connections\": [1]}");
        assert_eq!(left_aside(dir.path()), 1, "what the crash left");

        // The next write is whole, and leaves nothing of its own behind.
        write(&path, b"{\"connections\": [1, 2]}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"connections\": [1, 2]}");
        assert_eq!(left_aside(dir.path()), 1);
    }

    #[test]
    fn a_write_makes_its_directory_and_one_that_fails_leaves_nothing_beside() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        write(&dir.join("cache/a.json"), b"{}").unwrap();
        assert_eq!(fs::read(dir.join("cache/a.json")).unwrap(), b"{}");

        // A directory where the file would go is not written over.
        fs::create_dir(dir.join("taken")).unwrap();
        assert!(write(&dir.join("taken"), b"{}").is_err());
        assert!(dir.join("taken").is_dir());
        assert_eq!(left_aside(dir), 0);
    }
}
