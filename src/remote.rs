//! Projects on another machine, reached over SSH, as t3code reaches its
//! remote environments: a project `ssh://host/path`, and for each agent a
//! slot of its own there (`<agent>@<host>`) whose program is the agent
//! started on that machine through `ssh`, speaking ACP over its standard
//! input and output. The agent must be one the registry publishes as an
//! npm or uv package, run there by `npx` or `uvx` in a login shell; the
//! host must let you in without a password (a key). Git and the files are
//! the other machine's, so the desk does not read them.

use std::path::{Path, PathBuf};

use crate::registry::Entry;

/// The project `path` on `host`.
pub fn project(host: &str, path: &str) -> PathBuf {
    PathBuf::from(format!(
        "ssh://{host}{}",
        if path.starts_with('/') { "" } else { "/" }
    ))
    .join(path.trim_start_matches('/'))
}

/// The host and the path on it of a project on another machine; `None`
/// for one here.
pub fn parse(project: &Path) -> Option<(String, PathBuf)> {
    let text = project.to_str()?.strip_prefix("ssh:/")?;
    let text = text.strip_prefix('/').unwrap_or(text);
    let (host, path) = text.split_once('/')?;
    (!host.is_empty()).then(|| (host.to_owned(), PathBuf::from(format!("/{path}"))))
}

/// The machines named in `~/.ssh/config` (no patterns), to pick from.
pub fn hosts() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(Path::new(&home).join(".ssh/config")).unwrap_or_default();
    named(&text)
}

/// The hosts a `ssh_config` text names, in order, once each.
fn named(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("host")) {
            continue;
        }
        for host in words {
            if !host.contains(['*', '?', '!']) && !out.iter().any(|h| h == host) {
                out.push(host.to_owned());
            }
        }
    }
    out
}

/// The slot of `agent` on `host`.
pub fn agent_on(agent: &str, host: &str) -> String {
    format!("{agent}@{host}")
}

/// The agent of a slot, and the host it is on if not this machine.
pub fn split(id: &str) -> (&str, Option<&str>) {
    match id.split_once('@') {
        Some((agent, host)) => (agent, Some(host)),
        None => (id, None),
    }
}

/// The command line that starts `entry` on `host` over SSH; `None` where
/// the registry publishes it only as an archive, which is not here to send.
pub fn command(entry: &Entry, host: &str) -> Option<String> {
    let d = &entry.distribution;
    let (runner, p) = match (&d.npx, &d.uvx) {
        (Some(p), _) => ("npx -y", p),
        (None, Some(p)) => ("uvx", p),
        (None, None) => return None,
    };
    let mut inner = format!("{runner} {}", p.package);
    for a in &p.args {
        inner.push(' ');
        inner.push_str(a);
    }
    Some(over(host, &inner))
}

/// A shell line that runs `then` in `dir`, whatever `dir` is named.
pub fn shell_in(dir: &Path, then: &str) -> String {
    format!(
        "cd {} && {then}",
        shell_words::quote(&dir.to_string_lossy())
    )
}

/// `command` run on `host` in a login shell, so that what is on the PATH
/// there is found.
fn over(host: &str, command: &str) -> String {
    format!(
        "ssh -T {host} {}",
        shell_words::quote(&format!("bash -lc {}", shell_words::quote(command)))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosts_of_an_ssh_config_are_named_without_patterns() {
        let text =
            "Host devbox gpu-1\n  HostName 10.0.0.2\nHost *\n  User me\nhost staging devbox\n";
        assert_eq!(named(text), ["devbox", "gpu-1", "staging"]);
    }

    #[test]
    fn a_project_on_another_machine_is_read_back_and_its_agent_started_over_ssh() {
        let p = project("devbox", "/home/me/app");
        assert_eq!(p, PathBuf::from("ssh://devbox/home/me/app"));
        assert_eq!(
            parse(&p),
            Some(("devbox".into(), PathBuf::from("/home/me/app")))
        );
        assert_eq!(parse(Path::new("/Users/me/app")), None);
        assert_eq!(split("codex-acp@devbox"), ("codex-acp", Some("devbox")));
        assert_eq!(split("codex-acp"), ("codex-acp", None));
        let e: Entry = serde_json::from_str(
            r#"{"id":"c","name":"C","version":"1","distribution":{"npx":{"package":"@x/c@1","args":["--acp"]}}}"#,
        )
        .unwrap();
        let line = command(&e, "devbox").unwrap();
        let launch = crate::registry::launch_with(&e, Path::new("/"), Some(&line));
        if let Ok(l) = launch {
            assert_eq!(l.args, ["-T", "devbox", "bash -lc 'npx -y @x/c@1 --acp'"]);
        }
        // A folder is gone into whatever it is named, a quote and all.
        let dir = std::env::temp_dir().join(format!("ghost it's {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = std::process::Command::new("sh")
            .args(["-c", &shell_in(&dir, "pwd")])
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            Path::new(shown.trim()).canonicalize().unwrap(),
            dir.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir(&dir);
    }
}
