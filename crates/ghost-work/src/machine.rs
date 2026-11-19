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

use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use crate::prompts::{Store, TEXTS};
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

    /// A line or two for the worker about where it is, in `words`.
    fn describe(&self, words: &Store) -> String;

    /// The same machine, working in the folder `dir` on it instead: where
    /// one piece of work is done. `None` for a machine that cannot be
    /// given a folder of its own this way, which works where it is.
    fn at(&self, _dir: &Path) -> Option<Arc<dyn Machine>> {
        None
    }

    /// The bytes of a file on the machine, for handing it to someone. A
    /// file larger than `limit` is refused rather than cut.
    async fn read(&self, _path: &str, _limit: u64) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "files cannot be taken off this machine",
        ))
    }
}

fn too_large(path: &str, limit: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{path} is larger than {limit} bytes"),
    )
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
        .stderr(Stdio::piped());
    let mut child = ghost_net::spawn(&mut command)?;

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
        child.stop();
        let _ = child.wait().await;
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

    fn at(&self, dir: &Path) -> Option<Arc<dyn Machine>> {
        Some(Arc::new(Local::new(self.root.join(dir))))
    }

    async fn read(&self, path: &str, limit: u64) -> io::Result<Vec<u8>> {
        let full = self.root.join(path);
        if tokio::fs::metadata(&full).await?.len() > limit {
            return Err(too_large(path, limit));
        }
        tokio::fs::read(full).await
    }

    fn describe(&self, words: &Store) -> String {
        words.fill(
            TEXTS,
            "machine_local",
            &[
                ("os", std::env::consts::OS.to_owned()),
                ("dir", self.root.display().to_string()),
            ],
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

    async fn read(&self, path: &str, limit: u64) -> io::Result<Vec<u8>> {
        // One byte more than is allowed, to tell a file at the limit from
        // one over it without bringing all of a large one across.
        let mut command = Command::new("docker");
        command.arg("exec");
        if let Some(dir) = &self.workdir {
            command.args(["-w", dir]);
        }
        if let Some(user) = &self.user {
            command.args(["-u", user]);
        }
        let out = command
            .arg(&self.container)
            .args(["head", "-c", &(limit + 1).to_string(), "--", path])
            .output()
            .await?;
        if !out.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }
        if out.stdout.len() as u64 > limit {
            return Err(too_large(path, limit));
        }
        Ok(out.stdout)
    }

    fn describe(&self, words: &Store) -> String {
        match &self.workdir {
            Some(dir) => words.fill(TEXTS, "machine_container_in", &[("dir", dir.clone())]),
            None => words.text(TEXTS, "machine_container").to_owned(),
        }
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
    async fn a_file_can_be_taken_off_the_machine_unless_it_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "the result").unwrap();
        let local = Local::new(dir.path());
        assert_eq!(local.read("out.txt", 100).await.unwrap(), b"the result");
        let absolute = dir.path().join("out.txt");
        assert_eq!(
            local.read(absolute.to_str().unwrap(), 100).await.unwrap(),
            b"the result"
        );
        assert!(local.read("out.txt", 5).await.is_err());
        assert!(local.read("nothing.txt", 100).await.is_err());
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

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[tokio::test]
    async fn a_local_machine_given_a_folder_runs_there_and_a_container_stays_where_it_is() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("work")).unwrap();
        let machine = Local::new(root.path());
        let there = machine.at(Path::new("work")).unwrap();
        let out = there
            .exec("pwd", None, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            Path::new(out.text.trim()).canonicalize().unwrap(),
            root.path().join("work").canonicalize().unwrap()
        );
        assert!(Docker::new("c").at(Path::new("/w")).is_none());
    }
}
