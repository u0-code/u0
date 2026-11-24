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
//! beside it, so an agent is never half replaced. A package needs only
//! `npx` or `uvx`, which fetches and caches it on the first start. The
//! downloading, checking and unpacking is done by `curl`, `shasum`,
//! `unzip` and `tar`, which macOS and Linux have.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
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
        home.join("agents").join(&self.id).join(&self.version)
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
                let program = self.dir(home).join(&b.cmd);
                if !program.is_file() {
                    return Err(Missing::Download);
                }
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
pub fn install(entry: &Entry, home: &Path, say: &dyn Fn(&str)) -> Result<(), String> {
    let Some(b) = entry.distribution.binary.get(&platform()) else {
        return Err(Missing::Unsupported.say(&entry.name));
    };
    let dir = entry.dir(home);
    let staging = dir.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let name = b.archive.rsplit('/').next().unwrap_or("archive");
    let archive = staging.join(name);

    say(&format!("Downloading {name}"));
    run(Command::new("curl")
        .args(["-fsSL", "--max-time", "900", "-o"])
        .arg(&archive)
        .arg(&b.archive))?;

    if let Some(want) = &b.sha256 {
        say("Checking the download");
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

    say("Unpacking");
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
    Ok(())
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
}
