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

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
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
        if let Ok(mut asked) = ASKED.lock() {
            match got {
                Ok(()) => asked.remove(&path),
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

/// Ask her core for `name` and keep it at `path`, or say why not.
async fn fetch(source: &Source, name: &str, path: &Path) -> Result<(), String> {
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
    keep(path, &bytes).map_err(|e| format!("{name} could not be kept here: {e}"))
}

/// Write `bytes` to `path` aside and move them into place, so that a file
/// under its name is always whole.
fn keep(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let aside = dir.join(format!(".{name}.partial"));
    std::fs::write(&aside, bytes)?;
    std::fs::rename(&aside, path)
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
        keep(&path, b"her picture").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"her picture");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.png"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
