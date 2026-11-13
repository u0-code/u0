//! A sitting stepped by the Codex CLI.
//!
//! The same arrangement as [`crate::claude`]: `codex exec` is started with
//! its own shell turned off and our tools attached through MCP, with our
//! system prompt as its instructions. Codex talks to whatever it is set up
//! to talk to: the account it is signed in to, or a provider named in the
//! agent's extra arguments.
//!
//! One difference from Claude Code: Codex 0.133 cannot be stripped down to
//! our tools alone. Its plan tool and a few others stay. They do nothing to
//! the machine, so the rule that only our tools act still holds.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use ghost_brain::Usage;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{
    claude::Seat,
    undertaking::{Outcome, Undertaking},
    worker::{Ended, Report},
};

/// A string as a TOML value, for `-c key=value`.
fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Run one sitting. Returns when Codex exits or its time is up.
pub async fn sit(seat: &Seat) -> Report {
    let started = Instant::now();
    let dir: PathBuf = seat
        .undertaking
        .dir()
        .canonicalize()
        .unwrap_or_else(|_| seat.undertaking.dir().to_path_buf());
    let fail = |why: String| report(&seat.undertaking, Ended::Failed(why));

    let system_file = dir.join("codex-system.md");
    if let Err(e) = std::fs::write(&system_file, &seat.system) {
        return fail(format!("could not write its instructions: {e}"));
    }
    let Some((server, server_args)) = seat.tool_server.split_first() else {
        return fail("no tool server to give it".into());
    };
    let args_toml = format!(
        "[{}]",
        server_args
            .iter()
            .map(|a| toml_string(a))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut command = Command::new(&seat.agent.program);
    command
        .args(["exec", "--json", "--skip-git-repo-check"])
        .arg("-C")
        .arg(&dir)
        // Its own sandbox stays shut: everything it does goes through us.
        .args(["-s", "read-only"])
        .args(["--disable", "shell_tool"])
        .args(["-c", "web_search=\"disabled\""])
        .args(["-c", "project_doc_max_bytes=0"])
        .args([
            "-c",
            &format!(
                "model_instructions_file={}",
                toml_string(&system_file.display().to_string())
            ),
        ])
        .args([
            "-c",
            &format!("mcp_servers.ghost.command={}", toml_string(server)),
        ])
        .args(["-c", &format!("mcp_servers.ghost.args={args_toml}")])
        .args(["-c", "mcp_servers.ghost.required=true"])
        .args(["-c", "mcp_servers.ghost.tool_timeout_sec=3600"])
        // Without this an MCP call waits for an approval nobody can give.
        .args([
            "-c",
            "mcp_servers.ghost.default_tools_approval_mode=\"approve\"",
        ])
        // The keys our web tools read, and nothing else of the environment.
        .args([
            "-c",
            "mcp_servers.ghost.env_vars=[\"DEEPSEEK_API_KEY\", \"BRAVE_API_KEY\", \"SEARXNG_URL\"]",
        ]);
    if let Some(model) = &seat.agent.model {
        command.args(["-m", model]);
    }
    command.args(&seat.agent.args);
    match seat.undertaking.state().session {
        Some(session) => command.args(["resume", &session]).arg(&seat.opening),
        None => command.arg(&seat.opening),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return fail(format!("could not start {}: {e}", seat.agent.program)),
    };
    let Some(stdout) = child.stdout.take() else {
        return fail("it gave no output to read".into());
    };
    let errors = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut text = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut text).await;
            text
        })
    });

    let mut usage = Usage::default();
    let mut steps = 0u64;
    let mut failure: Option<String> = None;
    let mut turned = false;
    let mut lines = BufReader::new(stdout).lines();
    let deadline = Duration::from_secs(seat.max_seconds);
    let mut stopped = false;
    let timed_out = loop {
        if seat.stop.load(Ordering::Relaxed) {
            stopped = true;
            break false;
        }
        let Some(left) = deadline.checked_sub(started.elapsed()) else {
            break true;
        };
        let line =
            match tokio::time::timeout(left.min(Duration::from_secs(1)), lines.next_line()).await {
                Err(_) => continue,
                Ok(Ok(Some(line))) => line,
                Ok(_) => break false,
            };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("thread.started") => {
                if let Some(session) = event["thread_id"].as_str() {
                    seat.undertaking
                        .update(|s| s.session = Some(session.to_owned()));
                }
            }
            Some("item.completed") => {
                let item = &event["item"];
                match item["type"].as_str() {
                    Some("mcp_tool_call") => {
                        steps += 1;
                        tracing::info!(
                            "step {steps}: {}({})",
                            item["tool"].as_str().unwrap_or_default(),
                            item["arguments"]
                                .to_string()
                                .chars()
                                .take(100)
                                .collect::<String>()
                        );
                    }
                    Some("agent_message") => {
                        if let Some(text) = item["text"].as_str() {
                            tracing::info!("said: {}", text.chars().take(160).collect::<String>());
                        }
                    }
                    _ => {}
                }
            }
            Some("turn.completed") => {
                turned = true;
                let u = &event["usage"];
                let n = |key: &str| u[key].as_u64().unwrap_or(0);
                usage.add(Usage {
                    input: n("input_tokens"),
                    cached: n("cached_input_tokens"),
                    output: n("output_tokens"),
                    thought: n("reasoning_output_tokens"),
                });
            }
            Some("turn.failed" | "error") => {
                failure = Some(
                    event["error"]["message"]
                        .as_str()
                        .or_else(|| event["message"].as_str())
                        .unwrap_or("it reported an error")
                        .chars()
                        .take(400)
                        .collect(),
                );
            }
            _ => {}
        }
    };
    if timed_out || stopped {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    let complaint = match errors {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };

    // The tool server was another process writing the same undertaking.
    let on_disk = Undertaking::open(&dir, None).ok().map(|u| u.state());
    seat.undertaking.update(|state| {
        if let Some(disk) = on_disk {
            let session = state.session.take();
            *state = disk;
            state.session = session.or(state.session.take());
        }
        state.steps += steps;
        state.usage.add(usage);
        state.seconds += started.elapsed().as_secs_f64();
    });

    let ended = match seat.undertaking.state().finish.map(|f| f.outcome) {
        Some(Outcome::Finished) => Ended::Finished,
        Some(Outcome::Blocked) => Ended::Blocked,
        Some(Outcome::GaveUp) => Ended::GaveUp,
        None if stopped => Ended::Paused,
        None if timed_out => Ended::OutOfTime,
        None => match failure {
            Some(why) => Ended::Failed(why),
            // It ended its turn without ending the work.
            None if turned => Ended::Stalled,
            None => Ended::Failed(format!(
                "it exited without finishing a turn. {}",
                complaint
                    .trim()
                    .chars()
                    .rev()
                    .take(400)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )),
        },
    };
    report(&seat.undertaking, ended)
}

fn report(undertaking: &Undertaking, ended: Ended) -> Report {
    let state = undertaking.state();
    Report {
        ended,
        asking: state.waiting,
        lesson: state.finish.as_ref().and_then(|f| f.lesson.clone()),
        files: state
            .finish
            .as_ref()
            .map(|f| f.files.clone())
            .unwrap_or_default(),
        summary: state.finish.map(|f| f.summary).unwrap_or_default(),
        steps: state.steps,
        sittings: state.sittings + 1,
        usage: state.usage,
        seconds: state.seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_survive_being_written_as_toml() {
        assert_eq!(toml_string("/a b/c"), "\"/a b/c\"");
        assert_eq!(toml_string(r#"say "hi" \ now"#), r#""say \"hi\" \\ now""#);
    }
}
