//! The agents there are, as the ACP registry lists them, and how each is
//! put on this machine and started.
//!
//! The registry (`cdn.agentclientprotocol.com/registry/v1/latest`) is the
//! one list: for each agent its name, version, and how it is distributed,
//! as an archive for each platform with the command in it, or as a package
//! `npx` or `uvx` runs. The desk keeps the last copy it read in its home,
//! and reads a new one when asked or when it has none.
//!
//! An archive is downloaded, checked against the digest the registry gives,
//! and unpacked under `<home>/agents/<id>/<version>/`; a newer version goes
//! beside it, so an agent is never half replaced, and the one before runs
//! until the newer is in place, which then removes it. A package needs only
//! `npx` or `uvx`, which fetches and caches it on the first start. The
//! downloading, checking and unpacking is done by `curl`, `shasum`,
//! `unzip` and `tar`, which macOS and Linux have.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::Deserialize;

use crate::{acp::Launch, whole};

/// Where the newest registry is read from.
pub const URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub agents: Vec<Entry>,
}

/// One agent the registry lists.
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Distribution {
    /// An archive for each platform, keyed like `darwin-aarch64`.
    #[serde(default)]
    pub binary: BTreeMap<String, Binary>,
    #[serde(default)]
    pub npx: Option<Package>,
    #[serde(default)]
    pub uvx: Option<Package>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Binary {
    pub archive: String,
    /// The program inside the archive, from its top.
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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
    /// A package a runner (`npx`, `uvx`) fetches; the runner must be here.
    Runner {
        runner: &'static str,
        package: &'a str,
    },
    /// Nothing for this platform.
    None,
}

/// Why an agent cannot be started yet, and what would let it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// Its archive is not downloaded: installing it is one click.
    Download,
    /// It runs through `runner`, which is not installed.
    Runner(&'static str),
    /// The registry has nothing for this platform.
    Unsupported,
}

impl Missing {
    /// What the agent's page says about it.
    pub fn say(&self, name: &str) -> String {
        match self {
            Self::Download => format!("{name} is not installed yet."),
            Self::Runner("npx") => {
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

    /// Where its archive is unpacked.
    pub fn dir(&self, home: &Path) -> PathBuf {
        self.dir_of(home, &self.version)
    }

    fn dir_of(&self, home: &Path, version: &str) -> PathBuf {
        home.join("agents").join(&self.id).join(version)
    }

    /// The version of its archive on this machine: the registry's when it
    /// is there, else the last one installed, which runs until it is
    /// updated.
    pub fn installed(&self, home: &Path) -> Option<String> {
        let cmd = &self.distribution.binary.get(&platform())?.cmd;
        if self.dir(home).join(cmd).is_file() {
            return Some(self.version.clone());
        }
        std::fs::read_dir(home.join("agents").join(&self.id))
            .ok()?
            .flatten()
            .filter(|e| e.path().join(cmd).is_file())
            .filter(|e| e.path().extension().is_none_or(|x| x != "partial"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
    }

    /// Take its archives off this machine.
    pub fn uninstall(&self, home: &Path) -> Result<(), String> {
        let dir = home.join("agents").join(&self.id);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{} could not be removed: {e}", dir.display()))
            }
            _ => Ok(()),
        }
    }

    /// Whether its archive is on this machine: an agent run through a
    /// runner never is, the runner fetching it each time.
    pub fn downloaded(&self, home: &Path) -> bool {
        matches!(self.way(), Way::Archive) && self.launch(home).is_ok()
    }

    /// How to start it, or why it cannot be.
    pub fn launch(&self, home: &Path) -> Result<Launch, Missing> {
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
                let program = which(runner).ok_or(Missing::Runner(runner))?;
                let mut args = if runner == "npx" {
                    vec!["-y".to_owned()]
                } else {
                    vec![]
                };
                args.push(p.package.clone());
                args.extend(p.args.iter().cloned());
                Ok(Launch {
                    program,
                    args,
                    env: p.env.clone().into_iter().collect(),
                    dir: None,
                })
            }
            Way::None => Err(Missing::Unsupported),
        }
    }
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
    serde_json::from_str(&text).ok()
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
    Ok(registry)
}

/// Download, check and unpack `entry`'s archive for this platform. Blocking.
/// `say` is told each stage as it starts.
/// What an install says it is doing, and how far it is, from 0 to 1, when
/// that is known.
pub type Say<'a> = &'a dyn Fn(&str, Option<f32>);

/// What an install that was stopped returns.
pub const CANCELLED: &str = "cancelled";

/// Install `entry` from its archive, saying how far it is; `stop` set
/// stops it, and nothing is left of it.
pub fn install(entry: &Entry, home: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
    let Some(b) = entry.distribution.binary.get(&platform()) else {
        return Err(Missing::Unsupported.say(&entry.name));
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

    if let Some(want) = &b.sha256 {
        say("Checking the download", None);
        let out = run(Command::new("shasum").args(["-a", "256"]).arg(&archive))?;
        let got = String::from_utf8_lossy(&out);
        let got = got.split_whitespace().next().unwrap_or_default();
        if !got.eq_ignore_ascii_case(want) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!(
                "the download does not match the registry's digest (got {got}); nothing was installed"
            ));
        }
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
fn download(url: &str, to: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
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
        #[allow(clippy::cast_possible_truncation, reason = "a share of 0 to 1")]
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
fn run(command: &mut Command) -> Result<Vec<u8>, String> {
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
    fn a_package_runs_through_its_runner_with_the_arguments_the_registry_gives() {
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
        if let Ok(launch) = e.launch(Path::new("/nowhere")) {
            assert_eq!(launch.args, ["-y", "@google/gemini-cli@1", "--acp"]);
        }
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
        let home = std::env::temp_dir().join(format!("ghost-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
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
        assert_eq!(one.launch(&home), Err(Missing::Download));
        install(&one, &home, &quiet, &stop).unwrap();
        assert_eq!(one.installed(&home).as_deref(), Some("1"));

        // The registry names 2: 1 runs until 2 is installed, which removes it.
        let two = at("2");
        assert_eq!(two.installed(&home).as_deref(), Some("1"));
        assert!(two.launch(&home).is_ok());
        install(&two, &home, &quiet, &stop).unwrap();
        assert_eq!(two.installed(&home).as_deref(), Some("2"));
        assert!(!one.dir(&home).exists());

        two.uninstall(&home).unwrap();
        assert_eq!(two.launch(&home), Err(Missing::Download));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_install_stopped_before_it_starts_leaves_nothing() {
        let home = std::env::temp_dir().join(format!("ghost-stop-{}", std::process::id()));
        let e = entry(&format!(
            r#"{{"id":"b","name":"B","version":"1","distribution":{{"binary":{{"{}":{{"archive":"file:///nowhere/agent.tar.gz","cmd":"./agent"}}}}}}}}"#,
            platform()
        ));
        let stop = AtomicBool::new(true);
        let got = install(&e, &home, &|_: &str, _: Option<f32>| {}, &stop);
        assert_eq!(got, Err(CANCELLED.to_owned()));
        assert!(e.installed(&home).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }
}
