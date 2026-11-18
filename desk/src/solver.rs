//! The solver's deployment, as the desk looks after it: the desk's own
//! core, which keeps everything in a home of its own (`ghost_link::home()`)
//! and makes it ready itself, and the daemon that runs it, which the desk
//! starts as a child of its own. The core's brains are its own: the desk
//! asks it which there are and changes them over the Ghost Protocol
//! (`link::brains`), and reads none of its files for them. A [`Provider`]
//! is one of them as the core describes it.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context as _, anyhow};
use ghost_link::protocol::BrainInfo;
use tracing::{info, warn};

use crate::brains::{self, Mark, Preset};

/// One brain the core could run on, as it describes it: a
/// `[providers.<name>]` of its configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provider {
    pub name: String,
    /// `openai`, `anthropic`, `claude-code` or `codex`.
    pub kind: String,
    pub model: String,
    /// The environment variable its key comes from, for an API, with the
    /// daemon's default where the table names none; `None` for a CLI, or
    /// an API given a placeholder key because it needs none.
    pub key_env: Option<String>,
    /// Whether the core has a key for it.
    pub key_set: bool,
    pub base_url: Option<String>,
    /// The program, for a CLI.
    pub program: Option<String>,
    /// Why the core's machine cannot run it, if it cannot: its key is not
    /// set, its CLI is not installed, nothing answers where it runs.
    pub blocker: Option<String>,
}

impl From<BrainInfo> for Provider {
    fn from(b: BrainInfo) -> Self {
        Self {
            name: b.name,
            kind: b.kind,
            model: b.model,
            key_env: b.key_env,
            key_set: b.key_set,
            base_url: b.base_url,
            program: b.program,
            blocker: b.blocker,
        }
    }
}

impl Provider {
    /// The preset it is, if the desk has one for it.
    pub fn preset(&self) -> Option<&'static Preset> {
        Preset::of(&self.kind, self.base_url.as_deref())
    }

    /// The mark that stands for it.
    pub fn mark(&self) -> Mark {
        self.preset()
            .map_or_else(|| Mark::letter(&self.name), |p| Mark::Icon(p.icon))
    }

    /// How it is named in a list: the preset's name where it is saved
    /// under the preset's own id, otherwise the name it is saved under.
    pub fn title(&self) -> String {
        match self.preset() {
            Some(p) if p.id == self.name => p.name.to_owned(),
            _ => self.name.clone(),
        }
    }

    /// What it is, in a few words, for a menu: the program it runs here,
    /// or the host its API is on; or why this machine cannot run it.
    pub fn about(&self) -> String {
        if let Some(why) = &self.blocker {
            return why.clone();
        }
        if let Some(program) = &self.program {
            return format!("runs {program} here");
        }
        let base = self
            .base_url
            .as_deref()
            .or_else(|| self.preset().map(|p| p.base_url))
            .unwrap_or_default();
        let (_, host) = brains::authority(base);
        if host.is_empty() {
            self.kind.clone()
        } else {
            host
        }
    }

    /// How it is spoken of: the CLI, or the model.
    pub fn label(&self) -> String {
        match self.kind.as_str() {
            "claude-code" => format!("Claude Code · {}", self.model),
            "codex" => format!("Codex · {}", self.model),
            _ => self.model.clone(),
        }
        .trim_end_matches(" · ")
        .to_owned()
    }
}

/// Where the desk's core listens, which is what [`crate::settings::SOLVER_URL`]
/// reaches.
const LISTEN: &str = "127.0.0.1:7438";

/// The desk's core, and the daemon if the desk started it.
///
/// The core keeps everything in a home of its own, which `ghostd` makes
/// ready by itself: the desk only names it. Its brains and their keys,
/// what its ghost can do besides talk, where it runs, the directory her
/// machine works in and her images are behind the Ghost Protocol: the
/// desk reads none of its files.
pub struct Deployment {
    /// The core's home: `ghost_link::home()`.
    pub core: PathBuf,
    /// The checkout the desk was built in, which it never reads but for a
    /// `ghostd` built there and, once, an old core kept there to be taken
    /// into the home.
    checkout: PathBuf,
    child: Option<Child>,
    /// When the desk last started it, for saying "starting" rather than
    /// "down" while it comes up.
    started_at: Option<std::time::Instant>,
}

/// How long the desk gives the core it started to come up before the
/// right column calls it down.
const STARTUP: u64 = 20;

impl Deployment {
    /// The core in its home, and the checkout: `GHOST_ROOT`, else the one
    /// this crate was built in. The checkout is worked out from the path
    /// alone, without asking the file system: one under the Desktop could
    /// not be looked at before macOS has asked whether it may.
    pub fn find() -> Self {
        let checkout = std::env::var_os("GHOST_ROOT").map_or_else(
            || {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            },
            PathBuf::from,
        );
        Self::at(ghost_link::home(), checkout)
    }

    /// The core at `core`, with nothing started.
    pub fn at(core: PathBuf, checkout: PathBuf) -> Self {
        Self {
            core,
            checkout,
            child: None,
            started_at: None,
        }
    }

    /// The daemon binary. See [`pick_ghostd`].
    pub fn ghostd(&self) -> PathBuf {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("ghostd")));
        pick_ghostd(std::env::var_os("GHOSTD_BIN"), beside, &self.checkout)
    }

    /// Whether the desk started a daemon it has not yet seen stop. Unlike
    /// [`Self::owns_running`] it does not ask the system, so it can be
    /// asked while drawing.
    pub fn started(&self) -> bool {
        self.child.is_some()
    }

    /// Whether the core the desk started is still coming up: a daemon
    /// takes a few seconds to listen, and that is not a failure.
    pub fn starting(&self) -> bool {
        self.child.is_some()
            && self
                .started_at
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(STARTUP))
    }

    /// Whether the daemon the desk started is still running.
    pub fn owns_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => {
                if let Ok(None) = child.try_wait() {
                    true
                } else {
                    self.child = None;
                    false
                }
            }
            None => false,
        }
    }

    /// Have `ghostd` make the core's home ready, and wait for it: the core
    /// it starts there must have a ghost to run. A home already
    /// set up is left as it is; one with no ghost yet takes in the core a
    /// checkout kept before there were homes, if there is one.
    pub fn ready(&self) -> anyhow::Result<()> {
        let out = Command::new(self.ghostd())
            .arg("init")
            .arg(&self.core)
            .arg("--adopt")
            .arg(&self.checkout)
            .output()
            .context("run ghostd init")?;
        if !out.status.success() {
            return Err(anyhow!(
                "ghostd init: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    /// Start the daemon as a child of the desk, on its home. Its output
    /// goes to the desk's stderr.
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.owns_running() {
            return Ok(());
        }
        self.ready()?;
        let child = Command::new(self.ghostd())
            .args(run_args(&self.core))
            // Her lifeline: the desk holds this pipe open, and when the
            // desk goes, however it goes, the system closes it and she
            // stops in order. The desk's own ghost never outlives it.
            .arg("--with-parent")
            .current_dir(&self.core)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("start {}", self.ghostd().display()))?;
        info!("started the solver's daemon, pid {}", child.id());
        self.child = Some(child);
        self.started_at = Some(std::time::Instant::now());
        Ok(())
    }

    /// Stop the daemon the desk started, if any, and start it again.
    pub fn restart(&mut self) -> anyhow::Result<()> {
        self.stop();
        self.start()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Letting go of her lifeline stops her in order; SIGTERM as
            // well, for a ghostd built before it read one.
            drop(child.stdin.take());
            let pid = child.id();
            let _ = Command::new("kill").arg(pid.to_string()).status();
            for _ in 0..40 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            warn!("the solver's daemon did not stop in time; killing it");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Deployment {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Which `ghostd` runs the core: `named` (`GHOSTD_BIN`) if set; else the
/// one `beside` the desk's own binary, which is where `bundle.sh` puts the
/// release build it makes with the app, so that Ghost.app runs the daemon
/// it was built with; else, for a `cargo run` of the desk, the checkout's
/// release build, then its debug build; else `ghostd` on PATH. A release
/// build is taken over a newer debug one: a debug build is what running
/// the tests leaves behind, not a daemon anyone chose to live on.
fn pick_ghostd(named: Option<OsString>, beside: Option<PathBuf>, root: &Path) -> PathBuf {
    if let Some(bin) = named {
        return PathBuf::from(bin);
    }
    beside
        .into_iter()
        .chain(["target/release/ghostd", "target/debug/ghostd"].map(|p| root.join(p)))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("ghostd"))
}

/// What `ghostd` is told to run the core in its home `core` with.
fn run_args(core: &Path) -> Vec<OsString> {
    vec![
        "run".into(),
        "--home".into(),
        core.into(),
        "--listen".into(),
        LISTEN.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A core's home and a checkout, both empty, each its own directory
    /// under one for the test.
    fn checkout(name: &str) -> Deployment {
        let root = std::env::temp_dir().join(format!("ghost-desk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (core, checkout) = (root.join("core"), root.join("checkout"));
        std::fs::create_dir_all(&core).unwrap();
        std::fs::create_dir_all(&checkout).unwrap();
        Deployment::at(core, checkout)
    }

    #[test]
    fn the_ghostd_beside_the_app_runs_before_any_in_the_checkout() {
        let d = checkout("ghostd-beside");
        let bundle = d.checkout.join("Ghost.app/Contents/MacOS");
        for dir in [bundle.clone(), d.checkout.join("target/release")] {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("ghostd"), "").unwrap();
        }
        let beside = Some(bundle.join("ghostd"));
        assert_eq!(
            pick_ghostd(None, beside.clone(), &d.checkout),
            bundle.join("ghostd")
        );
        assert_eq!(
            pick_ghostd(Some("/opt/ghostd".into()), beside, &d.checkout),
            PathBuf::from("/opt/ghostd"),
            "GHOSTD_BIN is the one asked for"
        );
    }

    #[test]
    fn a_cargo_run_desk_takes_the_release_ghostd_over_a_newer_debug_one() {
        let d = checkout("ghostd-release");
        let beside = Some(d.checkout.join("desk/target/debug/ghostd"));
        assert_eq!(
            pick_ghostd(None, beside.clone(), &d.checkout),
            PathBuf::from("ghostd"),
            "with none built, the one on PATH"
        );
        for build in ["release", "debug"] {
            let dir = d.checkout.join("target").join(build);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("ghostd"), "").unwrap();
        }
        assert_eq!(
            pick_ghostd(None, beside.clone(), &d.checkout),
            d.checkout.join("target/release/ghostd")
        );
        std::fs::remove_file(d.checkout.join("target/release/ghostd")).unwrap();
        assert_eq!(
            pick_ghostd(None, beside, &d.checkout),
            d.checkout.join("target/debug/ghostd")
        );
    }

    #[test]
    fn the_core_is_run_from_its_home_where_the_desk_reaches_it() {
        let d = checkout("run");
        let args = run_args(&d.core);
        assert_eq!(
            args[..3],
            [
                "run".into(),
                "--home".into(),
                d.core.clone().into_os_string()
            ]
        );
        assert_eq!(
            crate::settings::SOLVER_URL,
            format!("ws://{LISTEN}"),
            "it listens where the desk looks for it"
        );
        assert!(args.contains(&OsString::from(LISTEN)));
    }

    /// One brain as a core describes it, read the way the desk reads it.
    fn described(json: &str) -> Provider {
        Provider::from(serde_json::from_str::<BrainInfo>(json).unwrap())
    }

    #[test]
    fn a_brain_the_core_describes_is_named_and_marked_as_its_preset() {
        let deepseek = described(
            r#"{"name":"main","kind":"openai","model":"deepseek-flash",
                "base_url":"https://api.deepseek.com/v1","key_env":"DEEPSEEK_API_KEY","key_set":true}"#,
        );
        assert_eq!(deepseek.preset().map(|p| p.id), Some("deepseek"));
        assert_eq!(deepseek.mark(), Mark::Icon("deepseek"));
        assert_eq!(deepseek.title(), "main", "saved under a name of its own");
        assert_eq!(deepseek.about(), "api.deepseek.com");
        assert_eq!(deepseek.label(), "deepseek-flash");
        assert!(deepseek.key_set);

        let claude = described(
            r#"{"name":"claude-code","kind":"claude-code","model":"sonnet","program":"claude"}"#,
        );
        assert_eq!(
            (
                claude.mark(),
                claude.title().as_str(),
                claude.label().as_str(),
                claude.about().as_str()
            ),
            (
                Mark::Icon("claude"),
                "Claude Code",
                "Claude Code · sonnet",
                "runs claude here"
            )
        );
        let codex = described(r#"{"name":"codex","kind":"codex","program":"codex"}"#);
        assert_eq!(codex.label(), "Codex", "no model is its own default");
    }

    #[test]
    fn a_brain_the_core_cannot_run_says_why_in_place_of_what_it_is() {
        let blocked = described(
            r#"{"name":"main","kind":"openai","model":"m","key_env":"K","key_set":false,
                "blocker":"K is not set"}"#,
        );
        assert_eq!(blocked.about(), "K is not set");
        let unknown = described(
            r#"{"name":"local-llama","kind":"openai","model":"m",
                "base_url":"http://localhost:8000/v1"}"#,
        );
        assert_eq!(unknown.preset(), None);
        assert_eq!(unknown.mark(), Mark::Letter('L'));
        assert_eq!(unknown.about(), "localhost");
    }
}
