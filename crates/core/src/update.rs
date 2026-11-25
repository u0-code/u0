//! Ghost's own updates: the builds on the repository's GitHub releases,
//! as the Homebrew casks install them. Stable is the newest release tagged
//! `v<version>`; Nightly is the `nightly` prerelease, rebuilt from each
//! push to main. An update downloads the release's zip, unpacks its
//! Ghost.app, and puts it in place of the one running, which is opened
//! again once Ghost has quit.
//!
//! The updater crates were weighed: `self_update` replaces a lone binary,
//! not an app bundle, and `cargo-packager-updater` needs a signed manifest
//! per channel that the releases do not publish; both bring an HTTP client
//! beside the `curl` Ghost already downloads agents with.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::registry::{Say, download, run};

/// The repository's releases, newest first.
const RELEASES: &str = "https://api.github.com/repos/u0-code/u0/releases";

/// What the macOS build is called among a release's files.
const ASSET: &str = "macos-arm64.zip";

/// This build's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit this build was made from, where `bundle.sh` said.
pub const COMMIT: Option<&str> = option_env!("GHOST_COMMIT");

/// Which builds Ghost updates to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Releases, as the `ghost` cask installs them.
    #[default]
    Stable,
    /// What main is now, as the `ghost@nightly` cask installs it.
    Nightly,
}

/// A build newer than this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its version, or `nightly` and its commit.
    pub name: String,
    /// Where its zip is.
    pub url: String,
    /// What its release says of it.
    pub notes: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    target_commitish: String,
    #[serde(default)]
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// The newest build on `channel`, when it is not this one. Reads GitHub;
/// blocking.
pub fn check(channel: Channel) -> Result<Option<Found>, String> {
    let out = run(Command::new("curl").args([
        "-fsSL",
        "--max-time",
        "30",
        "-H",
        "Accept: application/vnd.github+json",
        RELEASES,
    ]))?;
    let releases: Vec<Release> =
        serde_json::from_slice(&out).map_err(|e| format!("GitHub's answer did not read: {e}"))?;
    Ok(newer(&releases, channel, VERSION, COMMIT))
}

/// Among `releases`, the build on `channel` newer than `version` (built
/// from `commit`, where known).
fn newer(
    releases: &[Release],
    channel: Channel,
    version: &str,
    commit: Option<&str>,
) -> Option<Found> {
    let release = match channel {
        Channel::Stable => {
            let now = semver::Version::parse(version).ok()?;
            let (version, release) = releases
                .iter()
                .filter(|r| !r.draft)
                .filter_map(|r| {
                    let v = semver::Version::parse(r.tag_name.strip_prefix('v')?).ok()?;
                    Some((v, r))
                })
                .filter(|(v, _)| *v > now)
                .max_by(|a, b| a.0.cmp(&b.0))?;
            return found(release, version.to_string());
        }
        Channel::Nightly => releases.iter().find(|r| r.tag_name == "nightly")?,
    };
    let built = &release.target_commitish;
    if commit
        .is_some_and(|c| !c.is_empty() && (built.starts_with(c) || c.starts_with(built.as_str())))
    {
        return None;
    }
    found(release, format!("nightly {}", &built[..built.len().min(7)]))
}

fn found(release: &Release, name: String) -> Option<Found> {
    let asset = release.assets.iter().find(|a| a.name.ends_with(ASSET))?;
    Some(Found {
        name,
        url: asset.browser_download_url.clone(),
        notes: release.body.clone().unwrap_or_default(),
    })
}

/// The Ghost.app this process runs from; none when it runs outside one
/// (from cargo).
pub fn running_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

/// Download `found` into `home` and put its Ghost.app in place of `app`;
/// the app that was there is put back if the new one cannot be. Blocking.
pub fn install(
    found: &Found,
    app: &Path,
    home: &Path,
    say: Say,
    stop: &AtomicBool,
) -> Result<(), String> {
    let dir = home.join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let zip = dir.join("Ghost.zip");
    download(&found.url, &zip, say, stop)?;
    say("Unpacking", None);
    run(Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(&zip)
        .arg(&dir))?;
    let new = dir.join("Ghost.app");
    if !new.join("Contents/MacOS/ghost-desk").is_file() {
        return Err("the download holds no Ghost.app".to_owned());
    }
    let old = dir.join("Ghost before.app");
    std::fs::rename(app, &old).map_err(|e| format!("{} could not be moved: {e}", app.display()))?;
    if let Err(e) = std::fs::rename(&new, app) {
        let _ = std::fs::rename(&old, app);
        return Err(format!("the new Ghost.app could not be put in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn releases() -> Vec<Release> {
        serde_json::from_str(
            r#"[
              {"tag_name":"nightly","target_commitish":"b086a33aaaa","body":"Built from main.",
               "assets":[{"name":"Ghost-macos-arm64.zip","browser_download_url":"https://n/zip"}]},
              {"tag_name":"v0.2.0","target_commitish":"main","body":"Two.",
               "assets":[{"name":"Ghost-0.2.0-macos-arm64.zip","browser_download_url":"https://s/2"}]},
              {"tag_name":"v0.3.0","target_commitish":"main","draft":true,
               "assets":[{"name":"Ghost-0.3.0-macos-arm64.zip","browser_download_url":"https://s/3"}]},
              {"tag_name":"v0.1.0-pre.1","target_commitish":"main",
               "assets":[{"name":"Ghost-0.1.0-pre.1-macos-arm64.zip","browser_download_url":"https://s/1"}]}
            ]"#,
        )
        .expect("the fixture reads")
    }

    #[test]
    fn stable_offers_the_newest_release_above_this_version_and_never_a_draft() {
        let found = newer(&releases(), Channel::Stable, "0.1.0-pre.1", None).expect("one");
        assert_eq!(found.name, "0.2.0");
        assert_eq!(found.url, "https://s/2");
        assert_eq!(found.notes, "Two.");
        assert_eq!(newer(&releases(), Channel::Stable, "0.2.0", None), None);
    }

    #[test]
    fn nightly_is_offered_unless_this_build_is_its_commit() {
        let found = newer(
            &releases(),
            Channel::Nightly,
            "0.1.0-pre.1",
            Some("1234567"),
        );
        assert_eq!(found.map(|f| f.name), Some("nightly b086a33".to_owned()));
        assert_eq!(
            newer(
                &releases(),
                Channel::Nightly,
                "0.1.0-pre.1",
                Some("b086a33aaaa")
            ),
            None
        );
    }
}
