//! The tools a worker starts with.
//!
//! Few, and each one sharp. `shell` reaches everything a machine can do;
//! `read`, `write` and `edit` exist because quoting file contents through a
//! shell is where models lose the most time; `plan`, `note` and `done` are
//! how work is carried across context windows and brought to an end.

mod fence;
mod files;
mod record;
mod servers;
mod shell;
mod skills;
mod web;

use std::{io, sync::Arc, sync::Mutex, sync::atomic::AtomicU32, time::Duration};

pub use files::{Edit, Read, Write};
pub use record::{Ask, Done, Handoff, Keep, Note, Plan, Tell};
pub(crate) use record::{list, outcome_of, step_number};
pub use servers::{FindTools, Install, LoadTools, Loaded, Uninstall};
pub use shell::Shell;
pub use skills::{FindSkills, InstallSkill, UninstallSkill, UseSkill};
pub use web::{Fetch, Search, Source, WebConfig};

use crate::{
    machine::{Machine, Output, quote},
    prompts::{Store, TEXTS},
    tool::Toolbox,
    undertaking::Undertaking,
};

/// A machine, and where on it the worker currently stands. The tools that
/// act on the machine share one of these, so a `cd` in the shell moves the
/// file tools too.
pub struct Desk {
    pub machine: Arc<dyn Machine>,
    /// Where `write` and `edit` may write, for work with a folder.
    fence: Option<fence::Fence>,
    cwd: Mutex<Option<String>>,
    jobs: AtomicU32,
    /// Sets this desk's files in `/tmp` apart from every other desk's.
    tag: String,
}

impl Desk {
    pub fn new(machine: Arc<dyn Machine>) -> Arc<Self> {
        Arc::new(Self {
            machine,
            fence: None,
            cwd: Mutex::new(None),
            jobs: AtomicU32::new(0),
            tag: format!(
                "{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos())
            ),
        })
    }

    /// Why `path` is not to be written here, if it is not, in `words`: see
    /// `fence`.
    pub(crate) fn fenced(&self, words: &Store, path: &str) -> Option<String> {
        self.fence
            .as_ref()?
            .refuses(words, path, self.cwd().as_deref())
    }

    fn cwd(&self) -> Option<String> {
        self.cwd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn moved_to(&self, dir: String) -> bool {
        let mut cwd = self
            .cwd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = cwd.as_deref().is_some_and(|was| was != dir);
        *cwd = Some(dir);
        changed
    }

    /// The line that puts a script where the worker stands.
    fn prelude(&self) -> String {
        self.cwd()
            .map(|dir| format!("cd -- {} 2>/dev/null\n", quote(&dir)))
            .unwrap_or_default()
    }

    /// Run a script from where the worker stands.
    async fn exec(
        &self,
        script: &str,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> io::Result<Output> {
        self.machine
            .exec(&format!("{}{script}", self.prelude()), stdin, timeout)
            .await
    }
}

/// The largest image a tool shows the model: 5 MB, the most Anthropic
/// takes in one image. Others take more; none takes less.
pub(crate) const MAX_PICTURE: usize = 5 * 1024 * 1024;

/// An image the model can be shown, if `bytes` is one: PNG, JPEG, GIF or
/// WebP, known by its first bytes and not by what anything says it is.
pub(crate) fn picture(bytes: Vec<u8>) -> Option<ghost_brain::Picture> {
    let mime = ghost_core::media::sniff(&bytes)?;
    Some(ghost_brain::Picture {
        mime: mime.to_owned(),
        bytes,
    })
}

/// A line about an image: its type, its size, and how large it is drawn
/// where its header says.
pub(crate) fn about(picture: &ghost_brain::Picture) -> String {
    const MB: usize = 1024 * 1024;
    let size = match picture.bytes.len() {
        n if n >= MB => format!("{}.{} MB", n / MB, n % MB * 10 / MB),
        n if n >= 1024 => format!("{} KB", n / 1024),
        n => format!("{n} bytes"),
    };
    match ghost_core::media::dimensions(&picture.bytes) {
        Some((w, h)) => format!("{}, {size}, {w}×{h}", picture.mime),
        None => format!("{}, {size}", picture.mime),
    }
}

/// Keep the beginning and the end of long text, and say in `words` what
/// was cut.
pub fn clip(words: &Store, text: &str, head: usize, tail: usize) -> String {
    let total = text.chars().count();
    if total <= head + tail {
        return text.to_owned();
    }
    words.fill(
        TEXTS,
        "cut",
        &[
            ("head", text.chars().take(head).collect()),
            ("count", (total - head - tail).to_string()),
            ("tail", text.chars().skip(total - tail).collect()),
        ],
    )
}

/// The standard set: a shell and files on `machine`, and the record of
/// `undertaking`.
pub fn standard(machine: Arc<dyn Machine>, undertaking: &Arc<Undertaking>) -> Toolbox {
    let mut desk = Desk::new(machine);
    let state = undertaking.state();
    if let Some(own) = state.dir
        && let Some(d) = Arc::get_mut(&mut desk)
    {
        let own = std::path::PathBuf::from(own);
        d.fence = Some(fence::Fence {
            named: fence::named_files(&undertaking.their_words(), &own),
            of: state.checkout.map(|c| c.of.into()),
            own,
        });
    }
    let tools = Toolbox::new()
        .with(Shell::new(desk.clone()))
        .with(Read::new(desk.clone()))
        .with(Write::new(desk.clone()))
        .with(Edit::new(desk));
    record(tools, undertaking)
}

/// The record of `undertaking` and nothing else: for work that needs no
/// machine, such as finding something out.
pub fn desk(undertaking: &Arc<Undertaking>) -> Toolbox {
    record(Toolbox::new(), undertaking)
}

fn record(tools: Toolbox, undertaking: &Arc<Undertaking>) -> Toolbox {
    tools
        .with(Plan::new(undertaking.clone()))
        .with(Note::new(undertaking.clone()))
        .with(Done::new(undertaking.clone()))
}

/// Add the web to a toolbox: `search` if there is somewhere to search, and
/// `fetch` always.
#[must_use]
pub fn with_web(tools: Toolbox, config: &WebConfig) -> Toolbox {
    let tools = match &config.search {
        Some(source) => tools.with(Search::new(source.clone())),
        None => tools,
    };
    tools.with(Fetch::new(config.reader_key.clone()))
}

/// Add servers of tools to a toolbox. At work, with `installing`, that is
/// `find_tools`, `install` and `uninstall`; anywhere, `load_tools`, which
/// brings the tools of whatever is loaded in `loaded`. In a conversation
/// `load_tools` is only there when she has a server to load.
#[must_use]
pub fn with_servers(
    tools: Toolbox,
    servers: &Arc<crate::servers::Servers>,
    loaded: &Loaded,
    installing: bool,
) -> Toolbox {
    let tools = if installing && servers.installs() {
        tools
            .with(FindTools::new(servers.clone()))
            .with(Install::new(servers.clone()))
            .with(Uninstall::new(servers.clone()))
    } else {
        tools
    };
    if installing || servers.any() {
        tools.with(LoadTools::new(servers.clone(), loaded.clone()))
    } else {
        tools
    }
}

/// Add skills to a toolbox. At work, with `installing`, that is
/// `find_skills`, `install_skill` and `uninstall_skill`; anywhere,
/// `use_skill`. In a conversation `use_skill` is only there when she has
/// a skill to use; at work it is there from the start, so that a skill
/// installed in a sitting can be read in the same sitting.
#[must_use]
pub fn with_skills(
    tools: Toolbox,
    skills: &Arc<crate::skills::Skills>,
    installing: bool,
) -> Toolbox {
    let installing = installing && skills.installs();
    let tools = if installing {
        tools
            .with(FindSkills::new(skills.clone()))
            .with(InstallSkill::new(skills.clone()))
            .with(UninstallSkill::new(skills.clone()))
    } else {
        tools
    };
    if installing || skills.any() {
        tools.with(UseSkill::new(skills.clone()))
    } else {
        tools
    }
}

/// The tools that reach beyond a conversation, for a conversation: the web
/// if `web`. They are the same tools work has; only the loop around them
/// differs. A machine is never among them: a conversation looks things up
/// and answers, and anything that runs or changes something is work.
pub fn reaching(web: Option<&WebConfig>) -> Toolbox {
    match web {
        Some(web) => with_web(Toolbox::new(), web),
        None => Toolbox::new(),
    }
}

/// What tests of the tools that reach out need, with no network.
#[cfg(test)]
pub(crate) mod testing {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// The smallest PNG there is: one transparent pixel.
    pub const PIXEL: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";

    /// A web server on this machine that answers each path in `routes`
    /// with its type and body, and anything else with 404. Returns the
    /// address it is at, without a trailing slash.
    pub async fn serve(routes: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let n = socket.read(&mut request).await.unwrap_or(0);
                let line = String::from_utf8_lossy(&request[..n]);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (status, kind, body) = routes.iter().find(|(p, _, _)| *p == path).map_or(
                    ("404 Not Found", "text/plain", b"no".to_vec()),
                    |(_, k, b)| ("200 OK", *k, b.clone()),
                );
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_both_ends() {
        let words = crate::Prompts::shipped();
        let text = "a".repeat(50) + &"b".repeat(50);
        let cut = clip(&words, &text, 10, 10);
        assert!(cut.starts_with("aaaaaaaaaa\n[... 80 characters cut"));
        assert!(cut.ends_with("bbbbbbbbbb"));
        assert_eq!(clip(&words, "short", 10, 10), "short");
    }
}
