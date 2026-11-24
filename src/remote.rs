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

/// The command line that runs `command` on `host` in `dir` there.
pub fn run_in(host: &str, dir: &Path, command: &str) -> String {
    over(
        host,
        &format!("cd {} && {command}", quoted(&dir.to_string_lossy())),
    )
}

/// `command` run on `host` in a login shell, so that what is on the PATH
/// there is found.
fn over(host: &str, command: &str) -> String {
    format!(
        "ssh -T {host} {}",
        quoted(&format!("bash -lc {}", quoted(command)))
    )
}

/// `text` as one word to a POSIX shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // What the other machine's shell is given runs in the folder there,
        // a quote in its name and all.
        let dir = std::env::temp_dir().join(format!("ghost it's {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let line = run_in("devbox", &dir, "pwd");
        if let Ok(l) = crate::registry::launch_with(&e, Path::new("/"), Some(&line)) {
            let out = std::process::Command::new("sh")
                .args(["-c", &l.args[2]])
                .env("HOME", &dir)
                .output()
                .unwrap();
            let shown = String::from_utf8_lossy(&out.stdout);
            assert_eq!(
                Path::new(shown.trim()).canonicalize().unwrap(),
                dir.canonicalize().unwrap()
            );
        }
        let _ = std::fs::remove_dir(&dir);
    }
}
