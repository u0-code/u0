//! Starting a program, and stopping it with everything it started.
//!
//! Her shell, the command-line agents and brains, and her servers of tools
//! all start programs that start more: a dev server, a browser, a language
//! server, an agent's own shells. Killing only the program that was started
//! leaves those running for ever, holding ports and pipes. So a program is
//! started here as the leader of a process group of its own, and stopping
//! it kills the group.
//!
//! What it is given of this process's environment is decided here too: the
//! proxy rule, and nothing of a Claude Code session this process may itself
//! be running inside.

use std::{
    collections::HashSet,
    ffi::OsString,
    io,
    process::{ExitStatus, Output},
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
};

/// A program started by [`spawn`]. Dropping it while the program still
/// runs stops it, as [`Child::stop`] does.
#[derive(Debug)]
pub struct Child {
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    inner: tokio::process::Child,
}

/// Start `command` in a process group of its own, with the proxy rule and
/// without the variables of a Claude Code session. A variable the caller
/// set or removed on `command` itself stays as the caller left it.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    let said: HashSet<OsString> = command
        .as_std()
        .get_envs()
        .map(|(name, _)| name.to_owned())
        .collect();
    let names = std::env::vars_os().filter_map(|(name, _)| name.into_string().ok());
    let given = crate::current()
        .child_env()
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .chain(withheld(names).into_iter().map(|name| (name, None)));
    for (name, value) in given {
        if said.contains(&OsString::from(&name)) {
            continue;
        }
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut inner = command.kill_on_drop(true).spawn()?;
    Ok(Child {
        stdin: inner.stdin.take(),
        stdout: inner.stdout.take(),
        stderr: inner.stderr.take(),
        inner,
    })
}

impl Child {
    /// Wait for the program to exit. What it started in the background and
    /// left running is left running, as a shell leaves a job.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.inner.wait().await
    }

    /// Wait for the program to exit, reading all it printed on the pipes it
    /// was given.
    pub async fn output(mut self) -> io::Result<Output> {
        async fn read(pipe: Option<impl AsyncRead + Unpin>) -> io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                pipe.read_to_end(&mut bytes).await?;
            }
            Ok(bytes)
        }
        drop(self.stdin.take());
        let (stdout, stderr) = (self.stdout.take(), self.stderr.take());
        let (status, stdout, stderr) =
            tokio::try_join!(self.inner.wait(), read(stdout), read(stderr))?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    /// Kill the program and everything it started, at once. Once it has
    /// been waited for, its group's number may belong to another, so a
    /// program that has exited is left alone.
    pub fn stop(&self) {
        // Elsewhere there are no process groups, and the program alone is
        // killed when it is dropped.
        #[cfg(unix)]
        if let Some(group) = self
            .inner
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The names among `names`, this process's environment, that a program
/// started from it is not given. Inside a Claude Code session (`CLAUDECODE`
/// is set) they are the session's own: a `claude` that inherited them would
/// take that session, its login and the address of its host for its own,
/// and refuses to start at all inside another. A token put there on purpose
/// (`CLAUDE_CODE_OAUTH_TOKEN`) is handed on. Outside a session, everything
/// in the environment was put there on purpose, and all of it is.
fn withheld(names: impl Iterator<Item = String>) -> Vec<String> {
    let names: Vec<String> = names.collect();
    if !names.iter().any(|n| n == "CLAUDECODE") {
        return Vec::new();
    }
    names
        .into_iter()
        .filter(|n| {
            n == "CLAUDECODE"
                || (n.starts_with("CLAUDE_CODE_") && n != "CLAUDE_CODE_OAUTH_TOKEN")
                || n.starts_with("CLAUDE_AGENT_SDK_")
                || n == "ANTHROPIC_BASE_URL"
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use tokio::io::{AsyncBufReadExt, BufReader};

    use super::*;

    #[test]
    fn nothing_of_a_claude_code_session_this_process_runs_in_is_handed_on() {
        let names = |list: &[&str]| {
            list.iter()
                .map(|&n| n.to_owned())
                .collect::<Vec<_>>()
                .into_iter()
        };
        let inside = withheld(names(&[
            "CLAUDECODE",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_AGENT_SDK_VERSION",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "PATH",
        ]));
        assert_eq!(
            inside,
            [
                "CLAUDECODE",
                "CLAUDE_CODE_SESSION_ID",
                "CLAUDE_AGENT_SDK_VERSION",
                "ANTHROPIC_BASE_URL"
            ]
        );
        // Outside one, a base URL or a choice of cloud set on purpose is
        // handed on.
        let outside = names(&["ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "PATH"]);
        assert!(withheld(outside).is_empty());
    }

    /// A shell that starts a `sleep` and waits for it, and the number of
    /// that `sleep`.
    #[cfg(unix)]
    async fn with_a_child_of_its_own() -> (Child, i32) {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 300 & echo $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut child = spawn(&mut command).unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let pid = lines.next_line().await.unwrap().unwrap().parse().unwrap();
        (child, pid)
    }

    /// Whether the process `pid` is gone within five seconds. Once killed
    /// it is not ours to reap: whoever adopts it does.
    #[cfg(unix)]
    async fn gone(pid: i32) -> bool {
        let pid = rustix::process::Pid::from_raw(pid).unwrap();
        for _ in 0..50 {
            if rustix::process::test_kill_process(pid).is_err() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_a_program_kills_what_it_started_too() {
        let (mut child, grandchild) = with_a_child_of_its_own().await;
        child.stop();
        child.wait().await.unwrap();
        assert!(gone(grandchild).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_a_running_program_kills_what_it_started_too() {
        let (child, grandchild) = with_a_child_of_its_own().await;
        drop(child);
        assert!(gone(grandchild).await);
    }
}
