//! A sitting stepped by the Claude Code CLI instead of by our own loop.
//!
//! The installed `claude` is started headless with every built-in tool
//! turned off and one MCP server attached: ours (see [`crate::mcp`]). The
//! system prompt is ours, the tools are ours, the undertaking on disk is
//! ours. What the CLI contributes is the model behind whatever it is signed
//! in to, and the stepping between tool calls.
//!
//! Nothing here reads the CLI's stored credentials.
//!
//! What is given up compared with [`crate::Worker`]: the context is managed
//! by the CLI (its compaction, not our handoff), and a claim of being
//! finished gets the task read back by the `done` tool but no separate
//! review.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ghost_brain::{CliAgent, Usage};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{
    undertaking::{Outcome, Undertaking},
    worker::{Ended, Report},
};

/// What one sitting through the CLI needs.
pub struct Seat {
    pub agent: CliAgent,
    pub undertaking: Arc<Undertaking>,
    /// The whole system prompt.
    pub system: String,
    /// What the sitting opens with: the task, and what was left behind.
    pub opening: String,
    /// The command that serves our tools over MCP on stdio, program first.
    pub tool_server: Vec<String>,
    pub max_turns: u32,
    pub max_seconds: u64,
    /// Set from outside to stop the sitting. The CLI is killed; its session
    /// and everything our tools wrote stay, and the next sitting resumes.
    pub stop: Arc<AtomicBool>,
}

/// `2.1.322 (Claude Code)` as `(2, 1, 322)`.
fn version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

async fn installed(program: &str) -> Option<(u32, u32, u32)> {
    let out = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    version(&String::from_utf8_lossy(&out.stdout))
}

fn usage_of(message: &Value) -> Usage {
    let u = &message["usage"];
    let n = |key: &str| u[key].as_u64().unwrap_or(0);
    let cached = n("cache_read_input_tokens");
    Usage {
        input: n("input_tokens") + n("cache_creation_input_tokens") + cached,
        cached,
        output: n("output_tokens"),
        thought: 0,
    }
}

/// Run one sitting. Returns when the CLI exits or its time is up.
pub async fn sit(seat: &Seat) -> Report {
    let started = Instant::now();
    // Absolute, because the CLI is started from inside it.
    let dir: PathBuf = seat
        .undertaking
        .dir()
        .canonicalize()
        .unwrap_or_else(|_| seat.undertaking.dir().to_path_buf());
    let fail = |why: String| report(&seat.undertaking, Ended::Failed(why));

    let Some(found) = installed(&seat.agent.program).await else {
        return fail(format!(
            "{} is not installed or did not say its version",
            seat.agent.program
        ));
    };
    let (system_file, mcp_file) = (dir.join("claude-system.md"), dir.join("claude-mcp.json"));
    let Some((server, server_args)) = seat.tool_server.split_first() else {
        return fail("no tool server to give it".into());
    };
    let config = json!({"mcpServers": {"ghost": {"command": server, "args": server_args}}});
    if let Err(e) = std::fs::write(&system_file, &seat.system)
        .and_then(|()| std::fs::write(&mcp_file, config.to_string()))
    {
        return fail(format!("could not write its configuration: {e}"));
    }

    let mut command = Command::new(&seat.agent.program);
    command
        .arg("-p")
        .arg(&seat.opening)
        .args(["--output-format", "stream-json", "--verbose"])
        .arg("--system-prompt-file")
        .arg(&system_file)
        // No tools of its own: only what our server offers.
        .args(["--tools", ""])
        .arg("--mcp-config")
        .arg(&mcp_file)
        .args(["--strict-mcp-config", "--allowedTools", "mcp__ghost__*"])
        .args(["--permission-mode", "dontAsk"])
        .args(["--max-turns", &seat.max_turns.to_string()])
        .arg("--disable-slash-commands");
    if let Some(model) = &seat.agent.model {
        command.args(["--model", model]);
    }
    command.args(&seat.agent.args);
    if let Some(session) = seat.undertaking.state().session {
        command.args(["--resume", &session]);
    }
    // `--bare` keeps the operator's own memory, hooks and plugins out. It
    // also skips the stored login, so it is only usable with a key.
    let keyed = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()));
    if found >= (2, 1, 81) && keyed {
        command.arg("--bare");
    }
    command
        .current_dir(&dir)
        // It refuses to start inside another session of itself.
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1")
        // Show the model our tools outright instead of behind a search.
        .env("ENABLE_TOOL_SEARCH", "false")
        // Older versions wait on stdin for ever unless it is closed.
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
    let mut last: Option<Value> = None;
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
        // Wake once a second to see whether we have been asked to stop.
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
            Some("system") if event["subtype"] == "init" => {
                if let Some(session) = event["session_id"].as_str() {
                    seat.undertaking
                        .update(|s| s.session = Some(session.to_owned()));
                }
                let offered = event["tools"].as_array().map_or(0, Vec::len);
                tracing::info!("{} started with {offered} tools", seat.agent.program);
            }
            Some("assistant") => {
                steps += 1;
                usage.add(usage_of(&event["message"]));
                let said: Vec<String> = event["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|block| match block["type"].as_str() {
                        Some("tool_use") => Some(format!(
                            "{}({})",
                            block["name"]
                                .as_str()
                                .unwrap_or_default()
                                .trim_start_matches("mcp__ghost__"),
                            block["input"]
                                .to_string()
                                .chars()
                                .take(100)
                                .collect::<String>()
                        )),
                        Some("text") => block["text"]
                            .as_str()
                            .map(|t| t.chars().take(120).collect()),
                        _ => None,
                    })
                    .collect();
                if !said.is_empty() {
                    tracing::info!("step {steps}: {}", said.join(" | "));
                }
            }
            Some("result") => last = Some(event),
            _ => {}
        }
    };
    if timed_out || stopped {
        let _ = child.kill().await;
    }
    // The CLI's own account of the whole run is better than a sum over
    // messages, which repeats the same usage for each part of one reply.
    if let Some(result) = &last {
        if result["usage"].is_object() {
            usage = usage_of(result);
        }
        if let Some(turns) = result["num_turns"].as_u64() {
            steps = turns;
        }
    }
    let _ = child.wait().await;
    let complaint = match errors {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };

    // The tool server was another process writing the same undertaking.
    // What it wrote is on disk, not in this process's copy.
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
        None => match &last {
            Some(r) if r["subtype"] == "error_max_turns" => Ended::OutOfSteps,
            Some(r) if r["is_error"].as_bool() == Some(true) => Ended::Failed(
                r["result"]
                    .as_str()
                    .unwrap_or("it reported an error")
                    .chars()
                    .take(400)
                    .collect(),
            ),
            // It stopped talking without ending the work.
            Some(_) => Ended::Stalled,
            None => Ended::Failed(format!(
                "it exited without a result. {}",
                complaint.trim().chars().take(400).collect::<String>()
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
    fn versions_are_read_and_compared() {
        assert_eq!(version("2.1.322 (Claude Code)"), Some((2, 1, 322)));
        assert_eq!(version("2.1.20 (Claude Code)"), Some((2, 1, 20)));
        assert!(version("2.1.20 (Claude Code)").unwrap() < (2, 1, 81));
        assert_eq!(version("nonsense"), None);
    }

    #[test]
    fn usage_counts_cached_input_once() {
        let message = json!({"usage": {"input_tokens": 10, "cache_creation_input_tokens": 5, "cache_read_input_tokens": 100, "output_tokens": 7}});
        assert_eq!(
            usage_of(&message),
            Usage {
                input: 115,
                cached: 100,
                output: 7,
                thought: 0,
            }
        );
    }
}
