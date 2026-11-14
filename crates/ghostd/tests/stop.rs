//! The daemon run as a service is run: started as a program, and told to
//! stop with SIGTERM the way systemd, docker and a shutting-down machine
//! tell it.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
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
        .env_remove("GHOST_PROXY")
        .env_remove("GHOST_DIRECT")
        .env_remove("GHOSTD_CONFIG");
    command
}

#[test]
fn sigterm_stops_her_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let her = dir.path().join("her");
    std::fs::create_dir(&her).unwrap();
    std::fs::write(
        her.join("ghost.toml"),
        include_str!("../../../ghosts/_template/ghost.toml"),
    )
    .unwrap();
    std::fs::write(dir.path().join("ghostd.toml"), CONFIG).unwrap();
    let born = ghostd(dir.path()).args(["birth", "her"]).output().unwrap();
    assert!(born.status.success(), "{born:?}");

    let mut daemon = ghostd(dir.path())
        .args(["run", "her", "--config", "ghostd.toml"])
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
    let mut log = Vec::new();
    let mut wait_for = |what: &str| {
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if let Ok(line) = said.recv_timeout(Duration::from_millis(200)) {
                let found = line.contains(what);
                log.push(line);
                if found {
                    return log.clone();
                }
            }
        }
        panic!("never said {what:?}:\n{}", log.join("\n"));
    };
    wait_for("listening on");

    let sent = Command::new("kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let log = wait_for("is left be");
    assert!(
        log.iter().any(|l| l.contains("terminated: stopping")),
        "{}",
        log.join("\n")
    );

    let until = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = daemon.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "still running after SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    };
    // Stopped, not killed: a process killed by the signal has no code.
    assert_eq!(status.code(), Some(0), "{}", log.join("\n"));
}
