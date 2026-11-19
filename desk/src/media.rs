//! Her images, as her core hands them out.
//!
//! What she sees, sends, opens and keeps is kept in her `media/` on her
//! core's computer, named by the hash of its bytes, and her journal names
//! it so. The desk asks her core for one with `media.get` the first time a
//! thread or a list shows it, off the main thread, and keeps it under
//! `~/.config/ghost-desk/cache/media/<core id>/<name>`: a name means the
//! same bytes forever, so what was fetched once is drawn from there after,
//! and shows while her core cannot be reached. It is the same for the
//! desk's own core and for every connection; the desk reads nothing in a
//! core's home.
//!
//! What is kept here is a copy, and her core keeps every file for good, so
//! the copies are let go when there are too many: past [`KEPT_MOST`]
//! bytes, the files shown least lately go first, and come back from her
//! core the next time they are shown.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use ghost_link::{Console, protocol::code};
use tokio::sync::Semaphore;

use crate::{cache, link};

/// Where to ask for her files: her core, by its id, and how to reach her
/// on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub core: String,
    pub url: String,
    pub token: Option<String>,
    /// Which of the core's characters, by her id: `None` when it runs
    /// only one.
    pub ghost: Option<String>,
}

/// What can be drawn of a file of hers now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// It is on its way from her core.
    Loading,
    /// It is kept on this machine, here.
    Here(PathBuf),
    /// It could not be had, and why, in a line.
    Failed(String),
}

/// How long a file that could not be had is left before it is asked for
/// again: her core may only have been out of reach.
const AGAIN: Duration = Duration::from_secs(30);

/// How many files are asked for at once, each on a connection of its own:
/// a thread full of images does not open a socket for every one.
static AT_ONCE: Semaphore = Semaphore::const_new(4);

/// A file being asked for, or that could not be had and since when.
enum Asked {
    Going,
    Failed(String, Instant),
}

static ASKED: LazyLock<Mutex<HashMap<PathBuf, Asked>>> = LazyLock::new(Mutex::default);

/// Set when a file arrives or fails, for the desk to draw again.
static ARRIVED: AtomicBool = AtomicBool::new(false);

/// The most the desk keeps of her images on this machine, every core's
/// together. A picture in a chat is a few hundred KB to a few MB, so this
/// holds thousands of them: every image in months of threads, at a small
/// part of a disk. Past it nothing is lost, only fetched again.
const KEPT_MOST: u64 = 512 * 1024 * 1024;
/// What is left once the least lately shown have gone: room for many more
/// before the files are counted through again.
const KEPT_AFTER: u64 = KEPT_MOST / 4 * 3;

/// How many bytes the desk keeps, once it has counted them this run.
static KEPT: Mutex<Option<u64>> = Mutex::new(None);

/// The kept files shown this run, each marked as used the first time:
/// when the files are let go, the least lately used go first.
static USED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Mutex::default);

/// Where the file `name` from the core `core` is kept under `root`, if
/// both are names the desk keeps anything under: a core's id is letters,
/// digits and dashes, and a file's name is a bare name, never a path.
fn under(root: &Path, core: &str, name: &str) -> Option<PathBuf> {
    let core_ok = !core.is_empty()
        && core
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let name_ok =
        !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0', ':']);
    (core_ok && name_ok).then(|| root.join(core).join(name))
}

/// Where the file `name` from the core `core` is kept on this machine.
fn path(core: &str, name: &str) -> Option<PathBuf> {
    under(&cache::dir()?.join("media"), core, name)
}

/// What can be drawn of the file `name` from `source` now. The first time
/// it is asked for and is not kept, it is fetched, and comes back
/// [`Loading`](Shown::Loading) until it is here.
pub fn shown(source: &Source, name: &str) -> Shown {
    let Some(path) = path(&source.core, name) else {
        return Shown::Failed(format!("{name} is not a name her core keeps a file by"));
    };
    if path.is_file() {
        if USED.lock().is_ok_and(|mut used| used.insert(path.clone())) {
            used_now(&path);
        }
        return Shown::Here(path);
    }
    let Ok(mut asked) = ASKED.lock() else {
        return Shown::Loading;
    };
    match asked.get(&path) {
        Some(Asked::Going) => return Shown::Loading,
        Some(Asked::Failed(why, at)) if at.elapsed() < AGAIN => {
            return Shown::Failed(why.clone());
        }
        _ => {}
    }
    asked.insert(path.clone(), Asked::Going);
    let (source, name) = (source.clone(), name.to_owned());
    link::runtime().spawn(async move {
        let got = match AT_ONCE.acquire().await {
            Ok(_turn) => fetch(&source, &name, &path).await,
            Err(e) => Err(e.to_string()),
        };
        if let (Ok(size), Some(root)) = (&got, path.parent().and_then(Path::parent)) {
            let (size, root) = (*size, root.to_path_buf());
            let _ = tokio::task::spawn_blocking(move || {
                if let Ok(mut kept) = KEPT.lock() {
                    *kept = Some(keep_within(&root, *kept, size, KEPT_MOST, KEPT_AFTER));
                }
            })
            .await;
        }
        if let Ok(mut asked) = ASKED.lock() {
            match got {
                Ok(_) => asked.remove(&path),
                Err(why) => asked.insert(path, Asked::Failed(why, Instant::now())),
            };
        }
        ARRIVED.store(true, Ordering::Relaxed);
    });
    Shown::Loading
}

/// Whether a file arrived, or could not be had, since this was last asked.
pub fn arrived() -> bool {
    ARRIVED.swap(false, Ordering::Relaxed)
}

/// Mark the kept file at `path` as used now, by its time: what was shown
/// lately is what is kept longest. A file that cannot be marked is only
/// let go sooner.
fn used_now(path: &Path) {
    let _ = fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()));
}

/// What is kept under `root` once `added` more bytes have been: `kept` is
/// what was there before, if it has been counted. Past `most`, the files
/// used least lately go until no more than `after` is left.
fn keep_within(root: &Path, kept: Option<u64>, added: u64, most: u64, after: u64) -> u64 {
    let total = match kept {
        Some(kept) => kept + added,
        None => files(root).iter().map(|(_, len, _)| len).sum(),
    };
    if total <= most {
        return total;
    }
    let mut files = files(root);
    files.sort_by_key(|(used, _, _)| *used);
    let mut left: u64 = files.iter().map(|(_, len, _)| len).sum();
    for (_, len, path) in files {
        if left <= after {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            left -= len;
        }
    }
    left
}

/// Every file kept under `root`, one directory a core: when it was last
/// used, its size, and where it is.
fn files(root: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    let Ok(cores) = fs::read_dir(root) else {
        return Vec::new();
    };
    cores
        .flatten()
        .filter_map(|core| fs::read_dir(core.path()).ok())
        .flat_map(Iterator::flatten)
        .filter_map(|file| {
            let meta = file.metadata().ok().filter(fs::Metadata::is_file)?;
            Some((
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
                file.path(),
            ))
        })
        .collect()
}

/// Ask her core for `name` and keep it at `path`, or say why not. Returns
/// how many bytes were kept.
async fn fetch(source: &Source, name: &str, path: &Path) -> Result<u64, String> {
    let asked = async {
        let (mut console, _) =
            Console::connect(&source.url, source.token.clone(), source.ghost.clone()).await?;
        console.media(name).await
    };
    let (_, bytes) =
        asked.await.map_err(
            |e| match e.downcast_ref::<ghost_link::protocol::RpcError>() {
                Some(rpc) if rpc.code == code::NO_SUCH_MEDIA => {
                    format!("{name} is no longer kept by her core")
                }
                _ => {
                    let why = format!("{e:#}");
                    let first = why.lines().next().unwrap_or_default();
                    format!("{name} could not be had from her core: {first}")
                }
            },
        )?;
    crate::whole::write(path, &bytes).map_err(|e| format!("{name} could not be kept here: {e}"))?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_kept_by_its_cores_id_and_its_own_name() {
        let root = Path::new("/cache/media");
        let name = "9f2c0e1a.png";
        assert_eq!(
            under(root, "5f0c3a9e", name),
            Some(root.join("5f0c3a9e").join(name))
        );
        assert_ne!(
            under(root, "5f0c3a9e", name),
            under(root, "77aa01bc", name),
            "two cores keep apart"
        );
    }

    #[test]
    fn nothing_is_kept_outside_the_cache_whatever_a_core_says() {
        let root = Path::new("/cache/media");
        for core in ["", "..", "../x", "a/b", "/abs", "id\0"] {
            assert_eq!(under(root, core, "a.png"), None, "core {core:?}");
        }
        for name in [
            "",
            ".",
            "..",
            "../journal.db",
            "a/b.png",
            "a\\b.png",
            "/etc/passwd",
            ".a.png.partial",
            "C:x.png",
        ] {
            assert_eq!(under(root, "5f0c", name), None, "name {name:?}");
        }
    }

    #[test]
    fn a_kept_file_is_whole_under_its_name_and_nothing_is_left_aside() {
        let dir = std::env::temp_dir().join(format!("ghost-desk-media-{}", std::process::id()));
        let path = dir.join("5f0c").join("a.png");
        crate::whole::write(&path, b"her picture").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"her picture");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.png"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn past_the_most_kept_the_files_used_least_lately_go_first_on_every_core() {
        let root = std::env::temp_dir().join(format!("ghost-desk-kept-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let day = Duration::from_secs(86_400);
        let long_ago = SystemTime::now() - day * 10;
        // Five files of 100 bytes on two cores, each used a day after the last.
        let kept: Vec<PathBuf> = (0..5u32)
            .map(|n| {
                let path = root
                    .join(["5f0c", "77aa"][n as usize % 2])
                    .join(format!("{n}.png"));
                crate::whole::write(&path, &[0; 100]).unwrap();
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(long_ago + day * n)
                    .unwrap();
                path
            })
            .collect();
        // The oldest is shown again, and so is used now.
        used_now(&kept[0]);

        assert_eq!(keep_within(&root, None, 0, 500, 300), 500, "not past it");
        let left = keep_within(&root, Some(500), 100, 500, 300);
        assert_eq!(left, 300);
        let there: Vec<bool> = kept.iter().map(|p| p.is_file()).collect();
        assert_eq!(there, [true, false, false, true, true]);
        fs::remove_dir_all(&root).unwrap();
    }
}
