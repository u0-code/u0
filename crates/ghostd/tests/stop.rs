//! The daemon run as a service is run: started as a program, and told to
//! stop with SIGTERM the way systemd, docker and a shutting-down machine
//! tell it.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Thinks with a model that is not there: nothing in this test asks it
/// anything that has to be answered, and nothing leaves this machine.
const CONFIG: &str = r#"
[brain]
think = "nowhere"
tick = "nowhere"

[providers.nowhere]
kind = "openai"
model = "none"
base_url = "http://127.0.0.1:9/v1"
api_key = "not-a-key"

[listen]
addr = "127.0.0.1:0"

[network]
proxy = "none"

[mind]
live = false
weather = false

[mind.work]
enabled = false
"#;

fn ghostd(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ghostd"));
    // Run from the test's own directory so that no `.env` above the
    // checkout is read, and with nothing from the environment to say how
    // to reach the network.
    command
        .current_dir(dir)
        .env("RUST_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("GHOST_PROXY")
        .env_remove("GHOST_DIRECT")
        .env_remove("GHOSTD_CONFIG");
    command
}

/// Write a persona into `dir`, and give birth to her.
fn born(root: &Path, dir: &str) {
    let her = root.join(dir);
    std::fs::create_dir_all(&her).unwrap();
    std::fs::write(
        her.join("ghost.toml"),
        include_str!("../../../ghosts/_template/ghost.toml"),
    )
    .unwrap();
    let born = ghostd(root).args(["birth", dir]).output().unwrap();
    assert!(born.status.success(), "{born:?}");
}

/// A running daemon and what it has said on stderr so far.
struct Running {
    daemon: Child,
    said: mpsc::Receiver<String>,
    log: Vec<String>,
}

impl Running {
    fn start(root: &Path, args: &[&str]) -> Self {
        let mut daemon = ghostd(root)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (lines, said) = mpsc::channel();
        let stderr = daemon.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = lines.send(line);
            }
        });
        Self {
            daemon,
            said,
            log: Vec::new(),
        }
    }

    /// Wait until a line has every one of `words` in it; return the log.
    fn wait_for(&mut self, words: &[&str]) -> &[String] {
        let until = Instant::now() + Duration::from_secs(30);
        if self.log.iter().any(|l| words.iter().all(|w| l.contains(w))) {
            return &self.log;
        }
        while Instant::now() < until {
            if let Ok(line) = self.said.recv_timeout(Duration::from_millis(200)) {
                let found = words.iter().all(|w| line.contains(w));
                self.log.push(line);
                if found {
                    return &self.log;
                }
            }
        }
        panic!("never said {words:?}:\n{}", self.log.join("\n"));
    }

    /// Send SIGTERM, and wait for the daemon to exit with a code of its own.
    fn terminate(&mut self) -> Option<i32> {
        let sent = Command::new("kill")
            .args(["-TERM", &self.daemon.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.daemon.try_wait().unwrap() {
                // Whatever it said on the way out.
                while let Ok(line) = self.said.recv_timeout(Duration::from_millis(200)) {
                    self.log.push(line);
                }
                return status.code();
            }
            assert!(Instant::now() < until, "still running after SIGTERM");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn sigterm_stops_her_in_order() {
    let dir = tempfile::tempdir().unwrap();
    born(dir.path(), "her");
    std::fs::write(dir.path().join("ghostd.toml"), CONFIG).unwrap();

    let mut running = Running::start(dir.path(), &["run", "her", "--config", "ghostd.toml"]);
    running.wait_for(&["listening on"]);
    let code = running.terminate();
    let log = running.wait_for(&["is left be"]).join("\n");
    assert!(log.contains("terminated: stopping"), "{log}");
    // Stopped, not killed: a process killed by the signal has no code.
    assert_eq!(code, Some(0), "{log}");
}

#[test]
fn a_directory_of_characters_runs_each_with_her_own_settings_and_name_in_the_log() {
    let dir = tempfile::tempdir().unwrap();
    born(dir.path(), "ghosts/lin");
    born(dir.path(), "ghosts/mei");
    // Copied, not born: it is never run.
    std::fs::create_dir(dir.path().join("ghosts/_template")).unwrap();
    std::fs::write(
        dir.path().join("ghosts/_template/ghost.toml"),
        include_str!("../../../ghosts/_template/ghost.toml"),
    )
    .unwrap();
    let config = format!("{CONFIG}\n[ghosts.mei.mind]\ncalls_per_hour = 7\n");
    std::fs::write(dir.path().join("ghostd.toml"), config).unwrap();

    let mut running = Running::start(dir.path(), &["run", "ghosts", "--config", "ghostd.toml"]);
    running.wait_for(&["listening on"]);
    running.wait_for(&["ghost{id=lin}", "at most 120 calls an hour"]);
    running.wait_for(&["ghost{id=mei}", "at most 7 calls an hour"]);
    let code = running.terminate();
    running.wait_for(&["ghost{id=lin}", "is left be"]);
    let log = running
        .wait_for(&["ghost{id=mei}", "is left be"])
        .join("\n");
    assert!(!log.contains("_template"), "{log}");
    assert_eq!(code, Some(0), "{log}");

    // A misspelled character is not run past.
    std::fs::write(
        dir.path().join("ghostd.toml"),
        format!("{CONFIG}\n[ghosts.mai.mind]\ntempo = 0.5\n"),
    )
    .unwrap();
    let refused = ghostd(dir.path())
        .args(["run", "ghosts", "--config", "ghostd.toml"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("[ghosts.mai]"),
        "{refused:?}"
    );
}
