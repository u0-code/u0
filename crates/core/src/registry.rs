//! The agents there are, as the ACP registry lists them, and how each is
//! put on this machine and started.
//!
//! The registry (`cdn.agentclientprotocol.com/registry/v1/latest`) is the
//! one list: for each agent its name, version, and how it is distributed,
//! as an archive for each platform with the command in it, or as a package
//! `npx` or `uvx` runs. The desk keeps the last copy it read in its home,
//! and reads a new one when asked or when it has none.
//!
//! An archive is downloaded and unpacked under `<home>/agents/<id>/<version>/`; a newer version goes
//! beside it, so an agent is never half replaced, and the one before runs
//! until the newer is in place, which then removes it. A package is
//! installed there the same way, with npm or uv, so that what it depends on
//! (an agent's own CLI, such as Codex's) is the newest it allows as it is
//! installed, not what a runner's cache once fetched. What is installed
//! runs; the registry's newest is installed only when an agent is not
//! here, or fails as an older version (`Desk::install_due`). The
//! downloading and unpacking is done by `curl`, `unzip` and `tar`, which
//! macOS and Linux have.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{acp::Launch, whole};

/// Where the newest registry is read from.
pub const URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub agents: Vec<Entry>,
    /// The agents whose logo the home holds, by id: read with the
    /// registry, not from it.
    #[serde(default)]
    pub logos: std::collections::HashSet<String>,
}

impl Registry {
    /// Note which of its agents' logos `home` holds.
    fn with_logos(mut self, home: &Path) -> Self {
        self.logos = (self.agents.iter())
            .filter(|e| icon_path(home, &e.id).is_file())
            .map(|e| e.id.clone())
            .collect();
        self
    }
}

/// One agent the registry lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    /// Its logo: a one-colour SVG drawn in `currentColor`.
    #[serde(default)]
    pub icon: Option<String>,
    pub distribution: Distribution,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Distribution {
    /// An archive for each platform, keyed like `darwin-aarch64`.
    #[serde(default)]
    pub binary: BTreeMap<String, Binary>,
    #[serde(default)]
    pub npx: Option<Package>,
    #[serde(default)]
    pub uvx: Option<Package>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binary {
    pub archive: String,
    /// The program inside the archive, from its top.
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(
    clippy::struct_field_names,
    reason = "the registry's own name for the field"
)]
pub struct Package {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// This machine, the way the registry names platforms.
pub fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// How an agent is put on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way<'a> {
    /// An archive to download for this platform.
    Archive,
    /// A package of the runner `npx` or `uvx`, installed here with npm or
    /// uv, which must be here.
    Runner {
        runner: &'static str,
        package: &'a str,
    },
    /// Nothing for this platform.
    None,
}

/// Why an agent cannot be started yet, and what would let it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Missing {
    /// It is not installed here yet: installing it is one click.
    Download,
    /// It runs through `runner`, which is not installed.
    Runner(String),
    /// Your command for it names a program that is not here.
    Command(String),
    /// The registry has nothing for this platform.
    Unsupported,
}

impl Missing {
    /// What the agent's page says about it.
    pub fn say(&self, name: &str) -> String {
        match self {
            Self::Download => format!("{name} is not installed yet."),
            Self::Runner(runner) if runner == "npx" => {
                format!(
                    "{name} runs with npx, which comes with Node.js. Install Node.js (nodejs.org, or `brew install node`), then try again."
                )
            }
            Self::Runner(runner) => {
                format!(
                    "{name} runs with {runner}, which comes with uv. Install uv (`brew install uv`), then try again."
                )
            }
            Self::Unsupported => format!("{name} has no build for {}.", platform()),
            Self::Command(program) => {
                format!("{name} is started with your command, and `{program}` is not on this Mac.")
            }
        }
    }
}

impl Entry {
    /// How it is put on this machine. An archive is preferred: it needs
    /// nothing else installed.
    pub fn way(&self) -> Way<'_> {
        let d = &self.distribution;
        if d.binary.contains_key(&platform()) {
            Way::Archive
        } else if let Some(p) = &d.npx {
            Way::Runner {
                runner: "npx",
                package: &p.package,
            }
        } else if let Some(p) = &d.uvx {
            Way::Runner {
                runner: "uvx",
                package: &p.package,
            }
        } else {
            Way::None
        }
    }

    /// The registry's id of the agent it is: an account of an agent beside
    /// the first is that agent, run apart (see [`instance`]).
    pub fn base(&self) -> &str {
        instance(&self.id).0
    }

    /// Where its archive is unpacked.
    pub fn dir(&self, home: &Path) -> PathBuf {
        self.dir_of(home, &self.version)
    }

    fn dir_of(&self, home: &Path, version: &str) -> PathBuf {
        home.join("agents").join(self.base()).join(version)
    }

    /// The version of its archive on this machine: the registry's when it
    /// is there, else the last one installed, which runs until it is
    /// updated.
    pub fn installed(&self, home: &Path) -> Option<String> {
        if self.program_in(&self.dir(home)).is_some() {
            return Some(self.version.clone());
        }
        std::fs::read_dir(home.join("agents").join(self.base()))
            .ok()?
            .flatten()
            .filter(|e| e.path().extension().is_none_or(|x| x != "partial"))
            .filter(|e| self.program_in(&e.path()).is_some())
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
    }

    /// The program installed in `dir`: an archive's command, or the one a
    /// package installed there names in its `program` file.
    fn program_in(&self, dir: &Path) -> Option<PathBuf> {
        let program = match self.distribution.binary.get(&platform()) {
            Some(b) => dir.join(&b.cmd),
            None => dir.join(std::fs::read_to_string(dir.join(PROGRAM)).ok()?.trim()),
        };
        program.is_file().then_some(program)
    }

    /// Your CLI on the PATH its adapter drives, and the variable that
    /// points the adapter at it.
    pub fn your_cli(&self) -> Option<(&'static str, PathBuf)> {
        let (name, var) = known(&self.id)?.1?;
        which(name).map(|p| (var, p))
    }

    /// The CLI it is signed in through: yours on the PATH where its
    /// adapter drives it, else a program unpacked beside its command (an
    /// archive's own `claude`).
    pub fn own_cli(&self, home: &Path) -> Option<PathBuf> {
        if let Some((_, yours)) = self.your_cli() {
            return Some(yours);
        }
        let b = self.distribution.binary.get(&platform())?;
        let dir = self.dir_of(home, &self.installed(home)?);
        let cmd = dir.join(&b.cmd);
        let cmd = cmd.canonicalize().unwrap_or(cmd);
        std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.canonicalize().map_or(true, |c| c != cmd))
            .find(|p| {
                use std::os::unix::fs::PermissionsExt as _;
                p.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }

    /// Take its archives off this machine.
    pub fn uninstall(&self, home: &Path) -> Result<(), String> {
        let dir = home.join("agents").join(self.base());
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{} could not be removed: {e}", dir.display()))
            }
            _ => Ok(()),
        }
    }

    /// Whether it is installed on this machine.
    pub fn downloaded(&self, home: &Path) -> bool {
        self.installed(home).is_some()
    }

    /// How to start it, or why it cannot be: pointed at your CLI where
    /// it drives one and you have it.
    pub fn launch(&self, home: &Path) -> Result<Launch, Missing> {
        let mut launch = self.launch_as_shipped(home)?;
        if let Some((var, path)) = self.your_cli() {
            launch
                .env
                .push((var.to_owned(), path.to_string_lossy().into_owned()));
        }
        Ok(launch)
    }

    fn launch_as_shipped(&self, home: &Path) -> Result<Launch, Missing> {
        let d = &self.distribution;
        match self.way() {
            Way::Archive => {
                let b = &d.binary[&platform()];
                let version = self.installed(home).ok_or(Missing::Download)?;
                let program = self.dir_of(home, &version).join(&b.cmd);
                Ok(Launch {
                    program,
                    args: b.args.clone(),
                    env: b.env.clone().into_iter().collect(),
                    dir: None,
                })
            }
            Way::Runner { runner, .. } => {
                let p = if runner == "npx" {
                    d.npx.as_ref()
                } else {
                    d.uvx.as_ref()
                }
                .expect("the package the way names");
                // Installed in Ghost's folder, it starts from there.
                let installed = self
                    .installed(home)
                    .and_then(|v| self.program_in(&self.dir_of(home, &v)));
                match installed {
                    Some(program) => Ok(Launch {
                        program,
                        args: p.args.clone(),
                        env: p.env.clone().into_iter().collect(),
                        dir: None,
                    }),
                    None if which(installer(runner)).is_some() => Err(Missing::Download),
                    None => Err(Missing::Runner(runner.to_owned())),
                }
            }
            Way::None => Err(Missing::Unsupported),
        }
    }
}

/// How to start `entry`: `command` (a line of yours: a program, a path or
/// a name on the PATH, and its arguments) where there is one, else the
/// registry's way.
pub fn launch_with(entry: &Entry, home: &Path, command: Option<&str>) -> Result<Launch, Missing> {
    let Some(line) = command.map(str::trim).filter(|c| !c.is_empty()) else {
        return entry.launch(home);
    };
    let mut words = shell_words::split(line)
        .map_err(|_| Missing::Command(line.to_owned()))?
        .into_iter();
    let first = words.next().unwrap_or_default();
    let program = if first.contains('/') {
        Some(PathBuf::from(&first)).filter(|p| p.is_file())
    } else {
        which(&first)
    }
    .ok_or(Missing::Command(first))?;
    Ok(Launch {
        program,
        args: words.collect(),
        env: Vec::new(),
        dir: None,
    })
}

/// A program by name on the PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// The copy of the registry kept in `home`, if there is one.
pub fn kept(home: &Path) -> Option<Registry> {
    let text = std::fs::read_to_string(home.join("registry.json")).ok()?;
    serde_json::from_str::<Registry>(&text)
        .ok()
        .map(|r| r.with_logos(home))
}

/// Where an agent's logo is kept, once read.
pub fn icon_path(home: &Path, id: &str) -> PathBuf {
    home.join("icons").join(format!("{id}.svg"))
}

/// Read the newest registry and keep it in `home`, with every agent's
/// logo. Blocking. A logo that cannot be read is left out: the agent is
/// drawn with its initial.
pub fn fetch(home: &Path) -> Result<Registry, String> {
    let out = run(Command::new("curl").args(["-fsSL", "--max-time", "30", URL]))?;
    let registry: Registry =
        serde_json::from_slice(&out).map_err(|e| format!("the registry could not be read: {e}"))?;
    whole::write(&home.join("registry.json"), &out)
        .map_err(|e| format!("the registry could not be kept: {e}"))?;
    let _ = std::fs::create_dir_all(home.join("icons"));
    let mut curl = Command::new("curl");
    curl.args(["-sSL", "--max-time", "60", "--parallel"]);
    for e in &registry.agents {
        if let Some(url) = &e.icon {
            curl.arg(url).arg("-o").arg(icon_path(home, &e.id));
        }
    }
    let _ = curl.output();
    Ok(registry.with_logos(home))
}

/// Download, check and unpack `entry`'s archive for this platform. Blocking.
/// `say` is told each stage as it starts.
/// The file in a package's install folder that names its program.
const PROGRAM: &str = "program";

/// What Ghost knows of an agent that the registry does not say, by its id;
/// the one list of agents in code. Each row is:
///
/// - the CLI its adapter drives, and the variable that points the adapter
///   at one: yours on the PATH is the one it runs, so that its version,
///   sign-in and settings are those of your terminal; without one it runs
///   its own;
/// - whether it is switched on when the desk is first opened;
/// - the colour its logo is drawn in, on the dark ground and the light:
///   GPUI draws an icon in one colour, so each gets its brand's own,
///   deepened on the light ground to keep 3:1 there; a mark that is black
///   has none and is drawn in the text colour;
/// - the variable that names the folder its CLI keeps its sign-in and
///   settings in: another account of it is the agent run with its own.
type Known = (
    &'static str,
    Option<(&'static str, &'static str)>,
    bool,
    Option<(u32, u32)>,
    Option<&'static str>,
);
pub const KNOWN: [Known; 6] = [
    (
        "claude-acp",
        Some(("claude", "CLAUDE_CODE_EXECUTABLE")),
        true,
        Some((0xD977_57, 0xD977_57)),
        Some("CLAUDE_CONFIG_DIR"),
    ),
    (
        "codex-acp",
        Some(("codex", "CODEX_PATH")),
        true,
        None,
        Some("CODEX_HOME"),
    ),
    (
        "antigravity-acp",
        None,
        true,
        Some((0x8FBD_FF, 0x3C7B_E0)),
        None,
    ),
    ("gemini", None, false, Some((0xA99B_FA, 0x6B5B_D6)), None),
    ("opencode", None, false, Some((0xB4B4_B4, 0x6E6E_6E)), None),
    (
        "github-copilot-cli",
        None,
        false,
        Some((0xC4C4_C4, 0x6E6E_6E)),
        None,
    ),
];

/// What Ghost knows of the agent with this id (or an account of it), if
/// anything.
pub fn known(id: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.0 == instance(id).0)
}

/// An agent's id, and the account of it it names, if not the first: an
/// account beside the first is `<id>+<account>`, the agent run with the
/// variable [`KNOWN`] names pointed at a folder of its own.
pub fn instance(id: &str) -> (&str, Option<&str>) {
    match id.split_once('+') {
        Some((agent, account)) => (agent, Some(account)),
        None => (id, None),
    }
}

/// The id of the account `account` of the agent `agent`: its name made a
/// word of letters, digits and dashes.
pub fn account_id(agent: &str, account: &str) -> Option<String> {
    let word: String = (account.trim().to_lowercase().chars())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let word = word.trim_matches('-');
    (!word.is_empty()).then(|| format!("{}+{word}", instance(agent).0))
}

/// The entry of the agent `id` in `registry`: for an account beside the
/// first, its agent's, under its own id and named for it.
pub fn entry_of(registry: &Registry, id: &str) -> Option<Entry> {
    let (agent, account) = instance(id);
    let mut entry = registry.agents.iter().find(|e| e.id == agent)?.clone();
    if let Some(account) = account {
        id.clone_into(&mut entry.id);
        entry.name = format!("{} ({account})", entry.name);
    }
    Some(entry)
}

/// What installs a runner's package: npm for npx's, uv for uvx's.
fn installer(runner: &str) -> &'static str {
    if runner == "npx" { "npm" } else { "uv" }
}

/// Install `entry`'s package at its version into Ghost's folder, with npm
/// (an npx package) or uv (a uvx one): it then starts from there, with
/// nothing fetched each time, and is updated as an archive is.
fn install_package(entry: &Entry, home: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
    let d = &entry.distribution;
    let dir = entry.dir(home);
    let staging = dir.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let mut command = if let Some(p) = &d.npx {
        say(&format!("Installing {} with npm", p.package), None);
        let npm = which(installer("npx"))
            .ok_or_else(|| Missing::Runner("npx".into()).say(&entry.name))?;
        let mut c = Command::new(npm);
        c.args([
            "install",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
            "--prefix",
        ])
        .arg(&staging)
        .arg(&p.package);
        c
    } else if let Some(p) = &d.uvx {
        say(&format!("Installing {} with uv", p.package), None);
        let uv = which(installer("uvx"))
            .ok_or_else(|| Missing::Runner("uvx".into()).say(&entry.name))?;
        let mut c = Command::new(uv);
        c.args(["tool", "install", "--force", &p.package])
            .env("UV_TOOL_DIR", staging.join("tools"))
            .env("UV_TOOL_BIN_DIR", staging.join("bin"));
        c
    } else {
        return Err(Missing::Unsupported.say(&entry.name));
    };
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    // Read as it comes: an installer that says more than the pipe holds
    // would otherwise wait on it, and never exit.
    let mut err = child.stderr.take().expect("piped stderr");
    let said = std::thread::spawn(move || {
        let mut said = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut said);
        String::from_utf8_lossy(&said).into_owned()
    });
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&staging);
            return Err(CANCELLED.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                let said = said.join().unwrap_or_default();
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!("the install failed: {}", said.trim()));
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let program = if let Some(p) = &d.npx {
        npm_bin(&staging, &p.package)
    } else {
        first_file(&staging.join("bin"))
            .and_then(|p| p.strip_prefix(&staging).ok().map(Path::to_path_buf))
    }
    .ok_or("the package installed no program")?;
    std::fs::write(staging.join(PROGRAM), program.to_string_lossy().as_bytes())
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir).map_err(|e| e.to_string())?;
    if let Ok(all) = std::fs::read_dir(home.join("agents").join(&entry.id)) {
        for old in all.flatten().filter(|e| e.path() != dir) {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    Ok(())
}

/// The program an npm package installed under `prefix` gives, from its
/// `package.json`'s `bin`: its path from `prefix`.
fn npm_bin(prefix: &Path, package: &str) -> Option<PathBuf> {
    // "@scope/name@1.2.3" or "name@1.2.3": the name is before the last @
    // that is not the first character.
    let name = match package[1..].rfind('@') {
        Some(at) => &package[..=at],
        None => package,
    };
    let root = prefix.join("node_modules").join(name);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).ok()?).ok()?;
    let bin = match &json["bin"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(m) => m.values().next()?.as_str()?.to_owned(),
        _ => return None,
    };
    let program = root.join(bin);
    program
        .is_file()
        .then(|| program.strip_prefix(prefix).ok().map(Path::to_path_buf))
        .flatten()
}

fn first_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file())
}

/// What an install says it is doing, and how far it is, from 0 to 1, when
/// that is known.
pub type Say<'a> = &'a dyn Fn(&str, Option<f32>);

/// What an install that was stopped returns.
pub const CANCELLED: &str = "cancelled";

/// Install `entry` from its archive, saying how far it is; `stop` set
/// stops it, and nothing is left of it.
pub fn install(entry: &Entry, home: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
    let Some(b) = entry.distribution.binary.get(&platform()) else {
        return install_package(entry, home, say, stop);
    };
    let dir = entry.dir(home);
    let staging = dir.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let name = b.archive.rsplit('/').next().unwrap_or("archive");
    let archive = staging.join(name);

    if let Err(e) = download(&b.archive, &archive, say, stop) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    say("Unpacking", None);
    let lower = name.to_ascii_lowercase();
    if std::path::Path::new(&lower)
        .extension()
        .is_some_and(|e| e == "zip")
    {
        run(Command::new("unzip")
            .args(["-q", "-o"])
            .arg(&archive)
            .arg("-d")
            .arg(&staging))?;
    } else if [".tar.gz", ".tgz", ".tar.bz2", ".tar.xz", ".tar"]
        .iter()
        .any(|s| lower.ends_with(s))
    {
        run(Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging))?;
    } else {
        // A bare program: the archive is the command itself.
        std::fs::rename(&archive, staging.join(&b.cmd)).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&archive);
    let program = staging.join(&b.cmd);
    if !program.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the archive has no {}", b.cmd));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir).map_err(|e| e.to_string())?;
    // The versions before it are not needed once it is in place.
    if let Ok(all) = std::fs::read_dir(home.join("agents").join(&entry.id)) {
        for old in all.flatten().filter(|e| e.path() != dir) {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    Ok(())
}

/// Download `url` to `to` with `curl`, saying how many megabytes have come
/// of how many, until it is done or `stop` is set.
pub(crate) fn download(url: &str, to: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
    let mb = |n: u64| n as f64 / 1_048_576.;
    // The size, from the headers of the last answer after redirects.
    let total = Command::new("curl")
        .args(["-sIL", "--max-time", "30", url])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<u64>().ok())?
                })
                .next_back()
        })
        .filter(|n| *n > 0);
    let mut child = Command::new("curl")
        .args(["-fsSL", "--max-time", "1800", "-o"])
        .arg(to)
        .arg(url)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl could not be started: {e}"))?;
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELLED.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut said = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut err, &mut said);
            }
            return Err(format!("the download failed: {}", said.trim()));
        }
        let got = std::fs::metadata(to).map_or(0, |m| m.len());
        match total {
            Some(total) => say(
                &format!("Downloading · {:.1} of {:.1} MB", mb(got), mb(total)),
                Some((got as f64 / total as f64).min(1.) as f32),
            ),
            None => say(&format!("Downloading · {:.1} MB", mb(got)), None),
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Run a command to its end; its output, or what it said went wrong.
pub(crate) fn run(command: &mut Command) -> Result<Vec<u8>, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let out = command
        .output()
        .map_err(|e| format!("{program} could not be started: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program} failed: {}", said.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(json: &str) -> Entry {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn another_account_is_its_agent_under_an_id_and_name_of_its_own_installed_once() {
        let registry = Registry {
            agents: vec![entry(
                r#"{"id":"claude-acp","name":"Claude","version":"1","distribution":{}}"#,
            )],
            ..Registry::default()
        };
        let id = account_id("claude-acp", " My Work! ").unwrap();
        assert_eq!(id, "claude-acp+my-work");
        assert_eq!(account_id("claude-acp", " !! "), None);
        let work = entry_of(&registry, &id).unwrap();
        assert_eq!(
            (work.name.as_str(), work.base()),
            ("Claude (my-work)", "claude-acp")
        );
        let home = Path::new("/h");
        assert_eq!(work.dir(home), registry.agents[0].dir(home));
        // Its CLI keeps its sign-in where the variable Ghost knows points.
        assert_eq!(known(&id).and_then(|k| k.4), Some("CLAUDE_CONFIG_DIR"));
        assert!(entry_of(&registry, "codex-acp+x").is_none());
    }

    #[test]
    fn the_registry_kept_knows_which_logos_the_home_holds() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        std::fs::write(
            home.join("registry.json"),
            r#"{"agents":[{"id":"a","name":"A","version":"1","distribution":{}},{"id":"b","name":"B","version":"1","distribution":{}}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(home.join("icons")).unwrap();
        std::fs::write(icon_path(home, "a"), "<svg/>").unwrap();
        let logos = kept(home).expect("kept").logos;
        assert_eq!(logos, ["a".to_owned()].into());
    }

    #[test]
    fn a_package_is_installed_here_before_it_runs() {
        let e = entry(
            r#"{"id":"gemini","name":"Gemini CLI","version":"1","distribution":{"npx":{"package":"@google/gemini-cli@1","args":["--acp"]}}}"#,
        );
        assert_eq!(
            e.way(),
            Way::Runner {
                runner: "npx",
                package: "@google/gemini-cli@1"
            }
        );
        // Not installed: never run from a runner's cache.
        let missing = e.launch(Path::new("/nowhere")).expect_err("not installed");
        assert!(matches!(missing, Missing::Download | Missing::Runner(_)));
    }

    #[test]
    fn an_archive_not_yet_unpacked_is_missing_a_download() {
        let e = entry(&format!(
            r#"{{"id":"x","name":"X","version":"2","distribution":{{"binary":{{"{}":{{"archive":"https://e/x.zip","cmd":"./x"}}}}}}}}"#,
            platform()
        ));
        assert_eq!(e.way(), Way::Archive);
        assert_eq!(e.launch(Path::new("/nowhere")), Err(Missing::Download));
        assert_eq!(e.dir(Path::new("/h")), Path::new("/h/agents/x/2"));
    }

    #[test]
    fn an_agent_with_nothing_for_this_platform_says_so() {
        let e = entry(
            r#"{"id":"y","name":"Y","version":"1","distribution":{"binary":{"plan9-mips":{"archive":"a","cmd":"b"}}}}"#,
        );
        assert_eq!(e.launch(Path::new("/h")), Err(Missing::Unsupported));
    }

    #[test]
    fn an_archive_installs_keeps_running_when_the_registry_moves_on_and_updates_in_place() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let src = home.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("agent"), "#!/bin/sh\n").unwrap();
        let archive = home.join("agent.tar.gz");
        let made = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("agent")
            .status()
            .unwrap();
        assert!(made.success());
        let at = |version: &str| {
            entry(&format!(
                r#"{{"id":"a","name":"A","version":"{version}","distribution":{{"binary":{{"{}":{{"archive":"file://{}","cmd":"./agent"}}}}}}}}"#,
                platform(),
                archive.display()
            ))
        };
        let stop = AtomicBool::new(false);
        let quiet = |_: &str, _: Option<f32>| {};

        let one = at("1");
        assert_eq!(one.launch(home), Err(Missing::Download));
        install(&one, home, &quiet, &stop).unwrap();
        assert_eq!(one.installed(home).as_deref(), Some("1"));

        // The registry names 2: 1 runs until 2 is installed, which removes it.
        let two = at("2");
        assert_eq!(two.installed(home).as_deref(), Some("1"));
        assert!(two.launch(home).is_ok());
        install(&two, home, &quiet, &stop).unwrap();
        assert_eq!(two.installed(home).as_deref(), Some("2"));
        assert!(!one.dir(home).exists());

        two.uninstall(home).unwrap();
        assert_eq!(two.launch(home), Err(Missing::Download));
    }

    #[test]
    fn an_install_stopped_before_it_starts_leaves_nothing() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let e = entry(&format!(
            r#"{{"id":"b","name":"B","version":"1","distribution":{{"binary":{{"{}":{{"archive":"file:///nowhere/agent.tar.gz","cmd":"./agent"}}}}}}}}"#,
            platform()
        ));
        let stop = AtomicBool::new(true);
        let got = install(&e, home, &|_: &str, _: Option<f32>| {}, &stop);
        assert_eq!(got, Err(CANCELLED.to_owned()));
        assert!(e.installed(home).is_none());
    }

    #[test]
    fn a_command_of_yours_is_split_as_a_shell_splits_it_and_must_be_here() {
        let e = entry(r#"{"id":"a","name":"A","version":"1","distribution":{}}"#);
        let home = Path::new("/nowhere");
        assert_eq!(
            launch_with(&e, home, Some("no-such-program-here --acp")),
            Err(Missing::Command("no-such-program-here".into()))
        );
        assert_eq!(
            launch_with(&e, home, Some("agent 'unclosed")),
            Err(Missing::Command("agent 'unclosed".into()))
        );
        let l = launch_with(&e, home, Some("sh -c 'exit 0'")).unwrap();
        assert!(l.program.ends_with("sh"));
        assert_eq!(l.args, ["-c", "exit 0"]);
    }

    #[test]
    fn an_npm_package_names_its_program_in_its_package_json() {
        let prefix = tempfile::tempdir().unwrap();
        let prefix = prefix.path();
        let root = prefix.join("node_modules/@scope/agent");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"@scope/agent","bin":{"agent":"dist/main.js"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("dist/main.js"), "").unwrap();
        assert_eq!(
            npm_bin(prefix, "@scope/agent@1.2.3"),
            Some(PathBuf::from("node_modules/@scope/agent/dist/main.js"))
        );
        assert_eq!(npm_bin(prefix, "@scope/other@1"), None);
    }
}
