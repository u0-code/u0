//! The solver's deployment, as the desk looks after it: the desk's own
//! core, which keeps everything in a home of its own (`ghost_link::home()`)
//! and makes it ready itself, and the daemon that runs it. The desk can
//! read which brains the core knows, pick one, and start or restart the
//! daemon as a child of its own. It never edits a ghost's own files; the
//! core's configuration is what a person would edit by hand, and it is
//! edited the way a person would, keeping every comment.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context as _, anyhow};
use toml_edit::{DocumentMut, Item, Table, Value};
use tracing::{info, warn};

use crate::brains::{self, Mark, Preset, Wire};

/// One brain the deployment could run on: a `[providers.<name>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub name: String,
    /// `openai`, `anthropic`, `claude-code` or `codex`.
    pub kind: String,
    pub model: String,
    /// The environment variable its key comes from, for an API, with the
    /// daemon's default where the table names none; `None` for a CLI, or
    /// an API given a placeholder key because it needs none.
    pub key_env: Option<String>,
    pub base_url: Option<String>,
    /// The program, for a CLI.
    pub program: Option<String>,
    /// Why this machine cannot run it, if it cannot: its key is not set,
    /// its CLI is not installed, nothing answers where it runs.
    pub blocker: Option<String>,
}

impl Provider {
    /// Read `[providers.<name>]`. Whether it can run here is left for
    /// [`Provider::check`].
    fn read(name: &str, table: &Item) -> Self {
        let s = |k: &str| table.get(k).and_then(Item::as_str).map(str::to_owned);
        let kind = s("kind").unwrap_or_default();
        let key_env = match Wire::of(&kind) {
            Wire::Cli => None,
            _ if table.get("api_key").is_some() => None,
            Wire::Anthropic => Some(s("api_key_env").unwrap_or_else(|| "ANTHROPIC_API_KEY".into())),
            Wire::OpenAi => Some(s("api_key_env").unwrap_or_else(|| "OPENAI_API_KEY".into())),
        };
        let program = (Wire::of(&kind) == Wire::Cli).then(|| {
            s("program")
                .unwrap_or_else(|| if kind == "codex" { "codex" } else { "claude" }.to_owned())
        });
        Self {
            name: name.to_owned(),
            model: s("model").unwrap_or_default(),
            key_env,
            base_url: s("base_url"),
            program,
            kind,
            blocker: None,
        }
    }

    /// Find out whether this machine can run it, for real: the key is in
    /// `env` or the core's `.env`, the CLI is installed, or something
    /// answers at the address of an API on this machine.
    fn check(mut self, env: &dyn Fn(&str) -> Option<String>, dotenv: Option<&str>) -> Self {
        self.blocker = if let Some(program) = &self.program {
            (!on_path(program)).then(|| format!("`{program}` was not found on this machine"))
        } else if let Some(var) = &self.key_env {
            (!brains::key_status(var, env, dotenv).found()).then(|| format!("{var} is not set"))
        } else {
            let base = self.base_url.clone().unwrap_or_default();
            let (authority, host) = brains::authority(&base);
            let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
            (local && !answers(&authority)).then(|| format!("nothing answers at {authority}"))
        };
        self
    }

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

/// What the solver can do besides talk, as `[mind.work]` sets it: the
/// empty page says so, and offers only what it can do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Abilities {
    /// Work is on at all.
    pub work: bool,
    /// The machine its work runs on, as written: `local:` is this one.
    pub machine: Option<String>,
    /// It may search and read the web.
    pub web: bool,
}

impl Abilities {
    /// Whether it has a shell on this machine, as you.
    pub fn local(&self) -> bool {
        self.work && self.machine.as_deref() == Some("local:")
    }
}

/// Where the desk's core listens, which is what [`crate::settings::SOLVER_URL`]
/// reaches.
const LISTEN: &str = "127.0.0.1:7438";
/// The core's configuration, in its home.
const CONFIG: &str = "ghostd.toml";
/// The mascot, in its home.
const GHOST: &str = "ghosts/solver";

/// The desk's core, and the daemon if the desk started it.
///
/// The core keeps everything in a home of its own, which `ghostd` makes
/// ready by itself: the desk only names it. What the desk still reads and
/// writes there (the brains in its configuration, what it may do, the keys
/// in its `.env`, the media and the machine directory of its ghost) are
/// the next steps' to move behind the Ghost Protocol, after which the desk
/// will read none of its files.
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

    /// The configuration the daemon runs with, in its home.
    pub fn config(&self) -> PathBuf {
        self.core.join(CONFIG)
    }

    /// Where the solver lives.
    pub fn ghost_dir(&self) -> PathBuf {
        self.core.join(GHOST)
    }

    /// The daemon binary. See [`pick_ghostd`].
    pub fn ghostd(&self) -> PathBuf {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("ghostd")));
        pick_ghostd(std::env::var_os("GHOSTD_BIN"), beside, &self.checkout)
    }

    /// Every brain the deployment names, with whether it can run here.
    pub fn providers(&self) -> Vec<Provider> {
        let Ok(text) = std::fs::read_to_string(self.config()) else {
            return Vec::new();
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            return Vec::new();
        };
        let Some(table) = doc.get("providers").and_then(|p| p.as_table()) else {
            return Vec::new();
        };
        let dotenv = self.dotenv();
        table
            .iter()
            .map(|(name, p)| Provider::read(name, p).check(&env, dotenv.as_deref()))
            .collect()
    }

    /// The core's `.env` as written, which its daemon loads. Read for
    /// whether a key is in it, never shown.
    pub fn dotenv(&self) -> Option<String> {
        std::fs::read_to_string(self.core.join(".env")).ok()
    }

    /// How a request the desk makes leaves the machine: `[network]` in the
    /// deployment, under the environment's `GHOST_PROXY` and
    /// `GHOST_DIRECT`, exactly as the daemon would send it.
    pub fn network(&self) -> Result<ghost_net::Network, String> {
        let doc = std::fs::read_to_string(self.config())
            .ok()
            .and_then(|t| t.parse::<DocumentMut>().ok());
        let network = doc.as_ref().and_then(|d| d.get("network"));
        let setting = ghost_net::Setting {
            proxy: network
                .and_then(|n| n.get("proxy"))
                .and_then(Item::as_str)
                .map(str::to_owned),
            direct: network
                .and_then(|n| n.get("direct"))
                .and_then(Item::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                }),
        };
        let dotenv = self.dotenv();
        ghost_net::Network::new(&setting, |var| {
            env(var).or_else(|| brains::key_value(var, &|_| None, dotenv.as_deref()))
        })
        .map_err(|e| e.to_string())
    }

    /// What `[mind.work]` lets it do. Work is on unless it says otherwise.
    pub fn abilities(&self) -> Abilities {
        let Some(doc) = std::fs::read_to_string(self.config())
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
        else {
            return Abilities::default();
        };
        let work = doc.get("mind").and_then(|m| m.get("work"));
        let field = |k: &str| work.and_then(|w| w.get(k));
        Abilities {
            work: work.is_some()
                && field("enabled")
                    .and_then(toml_edit::Item::as_bool)
                    .unwrap_or(true),
            machine: field("machine")
                .and_then(toml_edit::Item::as_str)
                .map(str::to_owned),
            web: field("web")
                .and_then(toml_edit::Item::as_bool)
                .unwrap_or(false),
        }
    }

    /// The directory its machine works in: `machine = "local:"` runs as
    /// you, under the ghost's own `home`.
    pub fn home(&self) -> PathBuf {
        self.ghost_dir().join("home")
    }

    /// Which brain `[brain] think` names.
    pub fn brain(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config()).ok()?;
        let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        doc.get("brain")?.get("think")?.as_str().map(str::to_owned)
    }

    /// Make `name` the brain it thinks and ticks with, in the file, keeping
    /// everything else as it is.
    pub fn choose(&self, name: &str) -> anyhow::Result<()> {
        let path = self.config();
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?;
        if !doc.contains_key("brain") {
            doc["brain"] = toml_edit::table();
        }
        doc["brain"]["think"] = toml_edit::value(name);
        doc["brain"]["tick"] = toml_edit::value(name);
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("write {}", path.display()))?;
        info!("the solver thinks with {name} from its next start");
        Ok(())
    }

    /// Think with `model` on the brain `name` from its next start. An empty
    /// model names none, for a CLI to use its own default.
    pub fn set_model(&self, name: &str, model: &str) -> anyhow::Result<()> {
        self.edit(|doc| {
            let table = doc
                .get_mut("providers")
                .and_then(|p| p.get_mut(name))
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(|| anyhow!("no brain called {name}"))?;
            if model.is_empty() {
                table.remove("model");
            } else {
                table.insert("model", toml_edit::value(model));
            }
            Ok(())
        })?;
        info!(
            "{name} thinks with {} from its next start",
            if model.is_empty() {
                "its default"
            } else {
                model
            }
        );
        Ok(())
    }

    /// Change the deployment's file with `edit`, keeping every comment and
    /// everything `edit` does not touch.
    fn edit(
        &self,
        edit: impl FnOnce(&mut DocumentMut) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let path = self.config();
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut doc = text
            .parse::<DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?;
        edit(&mut doc)?;
        std::fs::write(&path, doc.to_string()).with_context(|| format!("write {}", path.display()))
    }

    /// Write a brain as `[providers.<name>]`: a new one, or the one saved
    /// as `old`, renamed if `name` is another. See [`write_provider`].
    pub fn save_provider(
        &self,
        old: Option<&str>,
        name: &str,
        set: &[(&str, Value)],
        clear: &[&str],
    ) -> anyhow::Result<()> {
        self.edit(|doc| write_provider(doc, old, name, set, clear))?;
        info!("the solver can think with {name}");
        Ok(())
    }

    /// Take the brain `name` out of the deployment. See [`drop_provider`].
    pub fn remove_provider(&self, name: &str) -> anyhow::Result<()> {
        self.edit(|doc| drop_provider(doc, name))?;
        info!("the solver no longer knows {name}");
        Ok(())
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

    /// Have `ghostd` make the core's home ready, and wait for it: what the
    /// desk reads there must be there before it is read. A home already
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

/// Write a brain as `[providers.<name>]`, setting the keys in `set` and
/// taking out those in `clear`. With `old`, the table saved under that name
/// is changed in place, every other key and comment in it kept, and moved
/// to `name` if that is another, with `[brain]` following it. A new brain
/// may not take a name already used. A key is never among what is set:
/// only the name of the variable it is in.
pub fn write_provider(
    doc: &mut DocumentMut,
    old: Option<&str>,
    name: &str,
    set: &[(&str, Value)],
    clear: &[&str],
) -> anyhow::Result<()> {
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("a brain's name is letters, digits, - and _"));
    }
    if !doc.contains_key("providers") {
        let mut providers = Table::new();
        providers.set_implicit(true);
        doc["providers"] = Item::Table(providers);
    }
    let providers = doc["providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("[providers] is not a table"))?;
    if old != Some(name) && providers.contains_key(name) {
        return Err(anyhow!("{name} is taken"));
    }
    let mut table = match old {
        Some(old) => match providers.remove(old) {
            Some(Item::Table(t)) => t,
            _ => return Err(anyhow!("no brain is saved as {old}")),
        },
        None => Table::new(),
    };
    for (k, v) in set {
        match table.get_mut(k).and_then(Item::as_value_mut) {
            // Keep the comment beside a value that is changed.
            Some(was) => {
                let decor = was.decor().clone();
                *was = v.clone();
                *was.decor_mut() = decor;
            }
            None => table[k] = Item::Value(v.clone()),
        }
    }
    for k in clear {
        table.remove(k);
    }
    providers.insert(name, Item::Table(table));
    if let Some(old) = old.filter(|o| *o != name)
        && let Some(brain) = doc.get_mut("brain").and_then(Item::as_table_like_mut)
    {
        for role in ["think", "tick"] {
            if brain.get(role).and_then(Item::as_str) == Some(old) {
                brain.insert(role, toml_edit::value(name));
            }
        }
    }
    Ok(())
}

/// Take `[providers.<name>]` out. The brain `[brain]` thinks or ticks with
/// stays: without it the daemon would not start.
pub fn drop_provider(doc: &mut DocumentMut, name: &str) -> anyhow::Result<()> {
    let used = ["think", "tick"].iter().any(|role| {
        doc.get("brain")
            .and_then(|b| b.get(role))
            .and_then(Item::as_str)
            == Some(name)
    });
    if used {
        return Err(anyhow!("in use: choose another brain first"));
    }
    doc.get_mut("providers")
        .and_then(Item::as_table_like_mut)
        .and_then(|p| p.remove(name))
        .map(|_| ())
        .ok_or_else(|| anyhow!("no brain is saved as {name}"))
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

/// The process's environment.
pub fn env(var: &str) -> Option<String> {
    std::env::var(var).ok()
}

/// Whether a program is installed: a path to it, or its name on PATH.
pub fn on_path(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// Whether something listens at `host:port` on this machine, asked quickly.
pub fn answers(authority: &str) -> bool {
    use std::net::ToSocketAddrs as _;
    authority.to_socket_addrs().is_ok_and(|mut addrs| {
        addrs.any(|a| {
            std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(250)).is_ok()
        })
    })
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

    /// A core's home with only its configuration in it, and a checkout
    /// with nothing in it, each its own directory under one for the test.
    fn checkout(name: &str) -> Deployment {
        let root = std::env::temp_dir().join(format!("ghost-desk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (core, checkout) = (root.join("core"), root.join("checkout"));
        std::fs::create_dir_all(&core).unwrap();
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::write(
            core.join(CONFIG),
            "# how it thinks\n[brain]\nthink = \"main\"\ntick = \"main\"\n\n[mind.work]\nenabled = true\nmachine = \"local:\"\nweb = true\n\n[operators]\n\n# the API\n[providers.main]\nkind = \"openai\"\nmodel = \"m\"\napi_key_env = \"NO_SUCH_KEY_FOR_A_TEST\"\n",
        )
        .unwrap();
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
    fn the_abilities_are_what_the_deployment_allows() {
        let d = checkout("abilities");
        let a = d.abilities();
        assert!(a.local() && a.web);
        let none = Abilities::default();
        assert!(!none.local());
        let off = Abilities { work: false, ..a };
        assert!(!off.local(), "a machine is nothing with work turned off");
    }

    #[test]
    fn what_the_desk_reads_of_its_core_is_in_the_cores_home_and_never_in_the_checkout() {
        let d = checkout("home");
        assert_eq!(d.config(), d.core.join("ghostd.toml"));
        assert_eq!(d.brain().as_deref(), Some("main"));
        assert_eq!(d.ghost_dir(), d.core.join("ghosts/solver"));
        assert_eq!(d.home(), d.core.join("ghosts/solver/home"));
        std::fs::write(d.checkout.join(".env"), "A_KEY=checkout\n").unwrap();
        assert_eq!(d.dotenv(), None, "a key in the checkout is not the core's");
        std::fs::write(d.core.join(".env"), "A_KEY=core\n").unwrap();
        assert_eq!(d.dotenv().as_deref(), Some("A_KEY=core\n"));
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

    /// What a new brain from `preset` writes, as the sheet would save it.
    fn fields(id: &str) -> Vec<(&'static str, Value)> {
        let p = Preset::by_id(id).unwrap();
        p.fields(&brains::Draft {
            model: p.model.to_owned(),
            key_env: p.key_env.unwrap_or_default().to_owned(),
            base_url: p.base_url.to_owned(),
        })
    }

    #[test]
    fn a_brain_added_and_chosen_keeps_the_rest_of_the_file() {
        let d = checkout("brain");
        d.save_provider(None, "claude-code", &fields("claude-code"), &[])
            .unwrap();
        d.choose("claude-code").unwrap();
        let text = std::fs::read_to_string(d.config()).unwrap();
        assert!(
            text.contains("# how it thinks") && text.contains("# the API"),
            "{text}"
        );
        assert!(
            text.contains("[providers.claude-code]\nkind = \"claude-code\"\nmodel = \"sonnet\"\ntimeout_secs = 120\n"),
            "{text}"
        );
        assert_eq!(d.brain().as_deref(), Some("claude-code"));
        let providers = d.providers();
        let main = providers.iter().find(|p| p.name == "main").unwrap();
        assert_eq!(
            main.blocker.as_deref(),
            Some("NO_SUCH_KEY_FOR_A_TEST is not set"),
            "an API whose key is not set cannot run"
        );
        assert_eq!(main.mark(), Mark::Icon("openai"));
        let claude = providers.iter().find(|p| p.name == "claude-code").unwrap();
        assert_eq!(
            (
                claude.mark(),
                claude.title().as_str(),
                claude.label().as_str()
            ),
            (Mark::Icon("claude"), "Claude Code", "Claude Code · sonnet")
        );
    }

    #[test]
    fn every_preset_round_trips_through_the_file() {
        let mut doc: DocumentMut = "# mine\n[brain]\nthink = \"main\"\n".parse().unwrap();
        for p in brains::PRESETS.iter().filter(|p| p.id != "custom") {
            write_provider(&mut doc, None, p.id, &fields(p.id), &[]).unwrap();
        }
        let doc: DocumentMut = doc.to_string().parse().unwrap();
        let text = doc.to_string();
        assert!(text.starts_with("# mine\n"), "{text}");
        assert!(!text.contains("[providers]\n"), "{text}");
        let providers = doc["providers"].as_table().unwrap();
        for p in brains::PRESETS.iter().filter(|p| p.id != "custom") {
            let read = Provider::read(p.id, &providers[p.id]);
            assert_eq!(read.kind, p.kind, "{}", p.id);
            assert_eq!(read.model, p.model, "{}", p.id);
            assert_eq!(read.preset().map(|q| q.id), Some(p.id), "{}", p.id);
            assert_eq!(read.title(), p.name);
            if p.cli() {
                assert_eq!(read.program.as_deref(), p.program);
                assert_eq!(read.key_env, None);
            } else {
                assert_eq!(read.base_url.as_deref(), Some(p.base_url), "{}", p.id);
                assert_eq!(read.key_env.as_deref(), p.key_env, "{}", p.id);
            }
        }
        let gemini = Provider::read("gemini", &providers["gemini"]);
        let key = |var: &str| (var == "GEMINI_API_KEY").then(|| "sk-test".to_owned());
        assert_eq!(gemini.clone().check(&key, None).blocker, None);
        assert_eq!(
            gemini.check(&|_| None, None).blocker.as_deref(),
            Some("GEMINI_API_KEY is not set")
        );
        let ollama = Provider::read("ollama", &providers["ollama"]);
        assert_eq!(ollama.key_env, None, "a local API needs no key");
        assert!(
            !text.contains("_API_KEY = \"sk"),
            "no key is written: {text}"
        );
    }

    #[test]
    fn a_brain_edited_in_place_keeps_its_comments_and_other_keys() {
        let mut doc: DocumentMut = "[brain]\nthink = \"main\"\ntick = \"main\"\n\n# the API\n[providers.main]\nkind = \"openai\"\nmodel = \"deepseek-chat\" # fast\nbase_url = \"https://api.deepseek.com/v1\"\napi_key_env = \"DEEPSEEK_API_KEY\"\njson_mode = true\n\n[providers.codex]\nkind = \"codex\"\n"
            .parse()
            .unwrap();
        write_provider(
            &mut doc,
            Some("main"),
            "main",
            &[("model", "deepseek-flash".into())],
            &["base_url"],
        )
        .unwrap();
        let text = doc.to_string();
        assert!(
            text.contains("# the API\n[providers.main]\nkind = \"openai\"\nmodel = \"deepseek-flash\" # fast\napi_key_env = \"DEEPSEEK_API_KEY\"\njson_mode = true\n"),
            "{text}"
        );
        assert!(
            text.find("[providers.main]") < text.find("[providers.codex]"),
            "it stays where it was: {text}"
        );
        // Renamed, [brain] follows it.
        write_provider(&mut doc, Some("main"), "deepseek", &[], &[]).unwrap();
        let text = doc.to_string();
        assert!(
            text.contains("think = \"deepseek\"\ntick = \"deepseek\""),
            "{text}"
        );
        assert!(
            text.contains("[providers.deepseek]\nkind = \"openai\""),
            "{text}"
        );
        assert!(!text.contains("[providers.main]"), "{text}");
        // A new one may not take a name in use, nor a name TOML would split.
        assert!(write_provider(&mut doc, None, "codex", &[], &[]).is_err());
        assert!(write_provider(&mut doc, None, "a.b", &[], &[]).is_err());
        assert!(write_provider(&mut doc, Some("gone"), "x", &[], &[]).is_err());
    }

    #[test]
    fn a_brain_is_removed_unless_it_is_the_one_in_use() {
        let mut doc: DocumentMut =
            "[brain]\nthink = \"main\"\ntick = \"main\"\n\n[providers.main]\nkind = \"openai\"\n\n# signed in\n[providers.codex]\nkind = \"codex\"\n"
                .parse()
                .unwrap();
        assert!(
            drop_provider(&mut doc, "main").is_err(),
            "the brain in use stays"
        );
        drop_provider(&mut doc, "codex").unwrap();
        let text = doc.to_string();
        assert!(!text.contains("codex"), "{text}");
        assert!(
            text.contains("[providers.main]\nkind = \"openai\"\n"),
            "{text}"
        );
        assert!(drop_provider(&mut doc, "codex").is_err());
        let again: DocumentMut = text.parse().unwrap();
        assert_eq!(again.to_string(), text);
    }

    #[test]
    fn a_brain_that_cannot_run_here_says_why() {
        // A port nothing listens on: bound, then let go.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let doc: DocumentMut = format!(
            "[p]\nkind = \"openai\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\napi_key = \"ollama\"\n"
        )
        .parse()
        .unwrap();
        let p = Provider::read("p", &doc["p"]).check(&|_| None, None);
        assert_eq!(
            p.blocker,
            Some(format!("nothing answers at 127.0.0.1:{port}"))
        );
        let cli: DocumentMut = "[c]\nkind = \"codex\"\nprogram = \"/no/such/codex\"\n"
            .parse()
            .unwrap();
        let c = Provider::read("c", &cli["c"]).check(&|_| None, None);
        assert_eq!(
            c.blocker.as_deref(),
            Some("`/no/such/codex` was not found on this machine")
        );
        let key: DocumentMut = "[k]\nkind = \"anthropic\"\nmodel = \"m\"\n"
            .parse()
            .unwrap();
        let k = Provider::read("k", &key["k"]);
        assert_eq!(k.key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert!(k.clone().check(&|_| None, None).blocker.is_some());
        let k = k.check(&|_| None, Some("ANTHROPIC_API_KEY=sk-file\n"));
        assert_eq!(k.blocker, None, "a key in the core's .env is a key");
    }
}
