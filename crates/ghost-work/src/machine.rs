//! Where work happens.
//!
//! A machine is anything that can run a bash script and hand back what it
//! printed. Everything else a worker does to its environment (reading,
//! writing and editing files) is built on that one operation, so a new kind
//! of machine is one small implementation.
//!
//! `Local` is a directory on this computer. `Docker` is a running container,
//! which is where a ghost's shell belongs on a server: what she breaks in
//! there stays in there.

use std::{collections::VecDeque, io, path::PathBuf, process::Stdio, time::Duration};

use async_trait::async_trait;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

/// How much of a command's output is kept: the beginning and the end.
const KEEP_HEAD: usize = 256 * 1024;
const KEEP_TAIL: usize = 256 * 1024;

/// What a script did.
#[derive(Debug, Clone, Default)]
pub struct Output {
    /// The exit code, if it exited on its own.
    pub code: Option<i32>,
    /// What it printed, stdout and stderr together, lossily decoded.
    pub text: String,
    /// Bytes dropped from the middle of very large output.
    pub dropped: usize,
    pub timed_out: bool,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }
}

#[async_trait]
pub trait Machine: Send + Sync {
    /// Run a bash script, feeding it `stdin` if given, for at most `timeout`.
    async fn exec(
        &self,
        script: &str,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> io::Result<Output>;

    /// A line or two for the worker about where it is.
    fn describe(&self) -> String;
}

/// Quote a string for bash.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Read a stream to its end, keeping only its head and tail.
async fn capture(mut from: impl AsyncRead + Unpin) -> io::Result<(Vec<u8>, usize)> {
    let mut head = Vec::new();
    let mut tail: VecDeque<u8> = VecDeque::new();
    let mut dropped = 0;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let mut chunk = &buf[..n];
        if head.len() < KEEP_HEAD {
            let take = chunk.len().min(KEEP_HEAD - head.len());
            head.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];
        }
        tail.extend(chunk);
        while tail.len() > KEEP_TAIL {
            tail.pop_front();
            dropped += 1;
        }
    }
    head.extend(tail);
    Ok((head, dropped))
}

/// Run a command to completion or to its deadline, whichever is first.
async fn run(
    mut command: Command,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
) -> io::Result<Output> {
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let pid = child.id();

    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            // The script may exit without reading; that is its business.
            let _ = pipe.write_all(&bytes).await;
            let _ = pipe.shutdown().await;
        });
    }
    let out = child.stdout.take().map(|s| tokio::spawn(capture(s)));
    let err = child.stderr.take().map(|s| tokio::spawn(capture(s)));

    let (code, timed_out) = if let Ok(status) = tokio::time::timeout(timeout, child.wait()).await {
        (status?.code(), false)
    } else {
        // Take the whole group down: the script's children hold the pipes
        // open, and reading would never end.
        #[cfg(unix)]
        if let Some(pid) = pid {
            let _ = Command::new("kill")
                .args(["-KILL", &format!("-{pid}")])
                .status()
                .await;
        }
        let _ = child.kill().await;
        (None, true)
    };

    let mut text = Vec::new();
    let mut dropped = 0;
    for stream in [out, err].into_iter().flatten() {
        // A grandchild that outlived the script can hold a pipe open for
        // ever. What has been read by now is what there is.
        if let Ok(Ok(Ok((bytes, lost)))) =
            tokio::time::timeout(Duration::from_secs(5), stream).await
        {
            text.extend(bytes);
            dropped += lost;
        }
    }
    Ok(Output {
        code,
        text: String::from_utf8_lossy(&text).into_owned(),
        dropped,
        timed_out,
    })
}

/// A directory on this computer. Not a sandbox: the script runs as you.
pub struct Local {
    root: PathBuf,
}

impl Local {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl Machine for Local {
    async fn exec(
        &self,
        script: &str,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> io::Result<Output> {
        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(format!("exec 2>&1\n{script}"))
            .current_dir(&self.root);
        run(command, stdin, timeout).await
    }

    fn describe(&self) -> String {
        format!(
            "You are on a {} machine, in the directory {}.",
            std::env::consts::OS,
            self.root.display()
        )
    }
}

/// A running Docker container.
pub struct Docker {
    container: String,
    workdir: Option<String>,
    user: Option<String>,
    env: Vec<String>,
}

impl Docker {
    pub fn new(container: impl Into<String>) -> Self {
        Self {
            container: container.into(),
            workdir: None,
            user: None,
            env: Vec::new(),
        }
    }

    /// Variables every command is given, each as `NAME=value`.
    #[must_use]
    pub fn env(mut self, env: Vec<String>) -> Self {
        self.env = env;
        self
    }

    #[must_use]
    pub fn workdir(mut self, dir: Option<String>) -> Self {
        self.workdir = dir;
        self
    }

    #[must_use]
    pub fn user(mut self, user: Option<String>) -> Self {
        self.user = user;
        self
    }
}

#[async_trait]
impl Machine for Docker {
    async fn exec(
        &self,
        script: &str,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> io::Result<Output> {
        // The deadline has to be enforced inside the container: killing the
        // `docker exec` client leaves the process it started running.
        let secs = timeout.as_secs().max(1);
        let guard = format!(
            "exec 2>&1\nif command -v timeout >/dev/null 2>&1; then exec timeout -k 5 {secs} bash -c \"$0\"; else exec bash -c \"$0\"; fi"
        );
        let mut command = Command::new("docker");
        command.arg("exec").arg("-i");
        if let Some(dir) = &self.workdir {
            command.args(["-w", dir]);
        }
        if let Some(user) = &self.user {
            command.args(["-u", user]);
        }
        for pair in &self.env {
            command.args(["-e", pair]);
        }
        command
            .arg(&self.container)
            .args(["bash", "-c", &guard, script]);
        let mut out = run(command, stdin, timeout + Duration::from_secs(20)).await?;
        // `timeout` exits 124 when it had to stop the script.
        if out.code == Some(124) || out.code == Some(137) && out.text.is_empty() {
            out.timed_out = true;
        }
        Ok(out)
    }

    fn describe(&self) -> String {
        format!(
            "You are inside a Linux container{}.",
            self.workdir
                .as_ref()
                .map(|d| format!(", starting in {d}"))
                .unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_quotes() {
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r"'it'\''s'");
    }

    #[tokio::test]
    async fn a_script_runs_where_it_should() {
        let dir = tempfile::tempdir().unwrap();
        let here = Local::new(dir.path());
        let out = here
            .exec("echo one; echo two >&2; pwd", None, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(out.ok());
        assert!(out.text.contains("one\ntwo\n"), "{}", out.text);
        let real = dir.path().canonicalize().unwrap();
        assert!(
            out.text.trim_end().ends_with(real.to_str().unwrap()),
            "{}",
            out.text
        );
    }

    #[tokio::test]
    async fn stdin_reaches_the_script_and_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let here = Local::new(dir.path());
        let out = here
            .exec(
                "cat > f.txt; wc -c < f.txt; exit 3",
                Some(b"hello".to_vec()),
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert_eq!(out.code, Some(3));
        assert_eq!(out.text.trim(), "5");
    }

    #[tokio::test]
    async fn a_script_that_overstays_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let here = Local::new(dir.path());
        let started = std::time::Instant::now();
        let out = here
            .exec(
                "echo before; sleep 30; echo after",
                None,
                Duration::from_millis(400),
            )
            .await
            .unwrap();
        assert!(out.timed_out);
        assert!(out.text.contains("before") && !out.text.contains("after"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
