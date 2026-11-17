//! Parts of a piece of work done by an agent installed on this machine:
//! Claude Code or Codex, with its own loop and its own tools.
//!
//! A worker hands a part to one of them the way it hands one to a worker
//! of its own (`delegate`, with `agent`), and hears from it the same way:
//! when the part ends, its report reaches the work's inbox. What differs
//! is who steps it. The agent is started in the directory of her machine,
//! with the part's task, and runs until it answers; what it did on the way
//! (each command it ran, each file it changed) is written to the part's log
//! as steps, so that whoever follows the work sees them. Its session is
//! kept, so that what is passed on to the part while it runs (`steer`) is
//! given to it when it next stops, in the same session, and a part that
//! outlives a sitting is resumed in the next.
//!
//! The agent asks nobody anything and keeps nothing of hers: it does what
//! its task says and reports. It runs as whoever runs her, with no approval
//! asked for, as her own shell on this machine does.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use chrono::Utc;
use ghost_brain::{CliAgent, CliKind, Usage};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{
    undertaking::{Finish, MailKind, Outcome, Undertaking},
    worker::{Ended, preview},
};

/// An installed agent that can be handed a part of the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// The name a worker hands it a part by: the provider it is configured
    /// as, such as `claude-code` or `codex`.
    pub name: String,
    pub cli: CliAgent,
    /// The directory of her machine, where it works.
    pub dir: PathBuf,
}

/// How one run of the agent came out.
enum Run {
    /// It answered, with this.
    Answered(String),
    /// It could not be started, or said it failed.
    Failed(String),
    /// The sitting ended, or the part was stopped, while it ran.
    Halted,
    /// The sitting's time ran out while it ran.
    OutOfTime,
}

/// Do `part` with `agent` until it ends, the sitting halts, or `deadline`.
/// The first run is given `task`; a later one, what was passed on to the
/// part since, or `go_on` if nothing was.
pub(crate) async fn run(
    agent: &Agent,
    part: &Undertaking,
    task: &str,
    go_on: &str,
    halt: &AtomicBool,
    deadline: Instant,
) -> Ended {
    let mut prompt = if part.state().session.is_none() {
        task.to_owned()
    } else {
        added(part).unwrap_or_else(|| go_on.to_owned())
    };
    loop {
        match once(agent, part, &prompt, halt, deadline).await {
            Run::Answered(answer) => {
                // What was passed on while it ran is given to it now, in the
                // same session, before the part is called done.
                if let Some(more) = added(part) {
                    prompt = more;
                    continue;
                }
                part.update(|state| {
                    if state.finish.is_none() {
                        state.finish = Some(Finish {
                            outcome: Outcome::Finished,
                            summary: answer.trim().to_owned(),
                            files: Vec::new(),
                            lesson: None,
                            at: Utc::now(),
                        });
                    }
                });
                return part
                    .state()
                    .finish
                    .map_or(Ended::Finished, |f| f.outcome.into());
            }
            Run::Failed(why) => return Ended::Failed(why),
            Run::Halted => {
                return part
                    .state()
                    .finish
                    .map_or(Ended::Paused, |f| f.outcome.into());
            }
            Run::OutOfTime => return Ended::OutOfTime,
        }
    }
}

/// What was passed on to the part since it last ran, if anything.
fn added(part: &Undertaking) -> Option<String> {
    let added: Vec<String> = part
        .take_inbox()
        .into_iter()
        .filter(|mail| mail.kind == MailKind::Added)
        .map(|mail| mail.text)
        .collect();
    (!added.is_empty()).then(|| added.join("\n\n"))
}

/// The command that runs `agent` once on `prompt`, resuming `session`.
fn command(agent: &Agent, prompt: &str, session: Option<&str>) -> Command {
    let cli = &agent.cli;
    let mut command = Command::new(&cli.program);
    match cli.kind {
        CliKind::Claude => {
            command
                .arg("-p")
                .arg(prompt)
                .args(["--output-format", "stream-json", "--verbose"])
                .args(["--permission-mode", "bypassPermissions"]);
            if let Some(model) = &cli.model {
                command.args(["--model", model]);
            }
            command.args(&cli.args);
            if let Some(session) = session {
                command.args(["--resume", session]);
            }
            // It refuses to start inside another session of itself.
            command
                .env_remove("CLAUDECODE")
                .env_remove("CLAUDE_CODE_ENTRYPOINT");
        }
        CliKind::Codex => {
            command
                .args(["exec", "--json", "--skip-git-repo-check"])
                .arg("-C")
                .arg(&agent.dir)
                .arg("--dangerously-bypass-approvals-and-sandbox");
            if let Some(model) = &cli.model {
                command.args(["-m", model]);
            }
            command.args(&cli.args);
            match session {
                Some(session) => command.args(["resume", session]).arg(prompt),
                None => command.arg(prompt),
            };
        }
    }
    command
        .current_dir(&agent.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // It reaches the web as she does.
    ghost_net::pass_on(&mut command);
    command
}

/// What one line the agent printed says, as far as the part goes.
#[derive(Default)]
struct Heard {
    session: Option<String>,
    /// A step: what it said, and the call it made.
    step: Option<(String, Value)>,
    usage: Option<Usage>,
    answer: Option<String>,
    failed: Option<String>,
}

/// Read one line of Claude Code's `stream-json`.
fn claude_line(event: &Value) -> Heard {
    let mut heard = Heard::default();
    match event["type"].as_str() {
        Some("system") if event["subtype"] == "init" => {
            heard.session = event["session_id"].as_str().map(str::to_owned);
        }
        Some("assistant") => {
            let blocks = event["message"]["content"].as_array();
            let said: String = blocks
                .into_iter()
                .flatten()
                .filter_map(|b| (b["type"] == "text").then(|| b["text"].as_str()).flatten())
                .collect::<Vec<_>>()
                .join(" ");
            let calls: Vec<Value> = blocks
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "tool_use")
                .map(|b| json!({"name": b["name"], "args": b["input"]}))
                .collect();
            if !calls.is_empty() || !said.is_empty() {
                heard.step = Some((said, Value::Array(calls)));
            }
        }
        Some("result") => {
            let u = &event["usage"];
            let n = |key: &str| u[key].as_u64().unwrap_or(0);
            let cached = n("cache_read_input_tokens");
            heard.usage = Some(Usage {
                input: n("input_tokens") + n("cache_creation_input_tokens") + cached,
                cached,
                output: n("output_tokens"),
                thought: 0,
            });
            let text = event["result"].as_str().unwrap_or_default().to_owned();
            if event["is_error"].as_bool() == Some(true) {
                heard.failed = Some(text);
            } else {
                heard.answer = Some(text);
            }
        }
        _ => {}
    }
    heard
}

/// Read one line of `codex exec --json`.
fn codex_line(event: &Value, last_said: &mut String) -> Heard {
    let mut heard = Heard::default();
    match event["type"].as_str() {
        Some("thread.started") => {
            heard.session = event["thread_id"].as_str().map(str::to_owned);
        }
        Some("item.completed") => {
            let item = &event["item"];
            match item["type"].as_str() {
                Some("agent_message") => {
                    item["text"]
                        .as_str()
                        .unwrap_or_default()
                        .clone_into(last_said);
                }
                Some("command_execution") => {
                    let call = json!({
                        "name": "shell",
                        "args": {"command": item["command"]},
                        "out": preview(item["aggregated_output"].as_str().unwrap_or_default(), 400),
                        "failed": item["exit_code"].as_i64().is_some_and(|c| c != 0),
                    });
                    heard.step = Some((String::new(), Value::Array(vec![call])));
                }
                Some(kind @ ("file_change" | "mcp_tool_call" | "web_search")) => {
                    let call = json!({"name": kind, "args": item});
                    heard.step = Some((String::new(), Value::Array(vec![call])));
                }
                _ => {}
            }
        }
        Some("turn.completed") => {
            let u = &event["usage"];
            let n = |key: &str| u[key].as_u64().unwrap_or(0);
            heard.usage = Some(Usage {
                input: n("input_tokens"),
                cached: n("cached_input_tokens"),
                output: n("output_tokens"),
                thought: n("reasoning_output_tokens"),
            });
            heard.answer = Some(last_said.clone());
        }
        Some("turn.failed" | "error") => {
            heard.failed = Some(
                event["error"]["message"]
                    .as_str()
                    .or_else(|| event["message"].as_str())
                    .unwrap_or("it reported an error")
                    .to_owned(),
            );
        }
        _ => {}
    }
    heard
}

/// Run the agent once on `prompt`, writing what it does to the part.
async fn once(
    agent: &Agent,
    part: &Undertaking,
    prompt: &str,
    halt: &AtomicBool,
    deadline: Instant,
) -> Run {
    let started = Instant::now();
    let session = part.state().session;
    let mut child = match command(agent, prompt, session.as_deref()).spawn() {
        Ok(child) => child,
        Err(e) => return Run::Failed(format!("could not start {}: {e}", agent.cli.program)),
    };
    let Some(stdout) = child.stdout.take() else {
        return Run::Failed("it gave no output to read".into());
    };
    let errors = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut text = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut text).await;
            text
        })
    });
    let mut lines = BufReader::new(stdout).lines();
    let (mut answer, mut failed, mut usage) = (None, None, Usage::default());
    let mut last_said = String::new();
    let mut steps = 0u64;
    let ended = loop {
        if halt.load(Ordering::Relaxed) || part.state().finish.is_some() {
            break Some(Run::Halted);
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break Some(Run::OutOfTime);
        };
        // Wake once a second to see whether it has been stopped.
        let line =
            match tokio::time::timeout(left.min(Duration::from_secs(1)), lines.next_line()).await {
                Err(_) => continue,
                Ok(Ok(Some(line))) => line,
                Ok(_) => break None,
            };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let heard = match agent.cli.kind {
            CliKind::Claude => claude_line(&event),
            CliKind::Codex => codex_line(&event, &mut last_said),
        };
        if let Some(session) = heard.session {
            part.update(|state| state.session = Some(session));
        }
        if let Some((said, calls)) = heard.step {
            steps += 1;
            part.log(&json!({
                "at": Utc::now().to_rfc3339(),
                "step": steps,
                "said": preview(&said, 400),
                "calls": calls,
            }));
        }
        if let Some(u) = heard.usage {
            usage.add(u);
        }
        if heard.answer.is_some() {
            answer = heard.answer;
        }
        if heard.failed.is_some() {
            failed = heard.failed;
        }
    };
    if ended.is_some() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    let complaint = match errors {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    part.update(|state| {
        state.steps += steps;
        state.usage.add(usage);
        state.seconds += started.elapsed().as_secs_f64();
    });
    if let Some(ended) = ended {
        return ended;
    }
    match (failed, answer) {
        (Some(why), _) => Run::Failed(preview(&why, 400)),
        (None, Some(answer)) => Run::Answered(answer),
        (None, None) => Run::Failed(format!(
            "{} ended without an answer. {}",
            agent.name,
            preview(complaint.trim(), 400)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_codes_lines_give_its_session_its_steps_and_its_answer() {
        let init = claude_line(&json!({"type": "system", "subtype": "init", "session_id": "s1"}));
        assert_eq!(init.session.as_deref(), Some("s1"));
        let step = claude_line(&json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "Reading it."},
            {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}
        ]}}));
        let (said, calls) = step.step.unwrap();
        assert_eq!(said, "Reading it.");
        assert_eq!(calls[0]["name"], "Bash");
        let done = claude_line(
            &json!({"type": "result", "result": "Done: 3 files.", "is_error": false,
            "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5}}),
        );
        assert_eq!(done.answer.as_deref(), Some("Done: 3 files."));
        assert_eq!(done.usage.unwrap().input, 100);
        let failed = claude_line(&json!({"type": "result", "result": "no", "is_error": true}));
        assert_eq!(failed.failed.as_deref(), Some("no"));
    }

    #[test]
    fn codexs_lines_give_its_session_its_commands_and_its_last_word_as_the_answer() {
        let mut last = String::new();
        let started = codex_line(
            &json!({"type": "thread.started", "thread_id": "t1"}),
            &mut last,
        );
        assert_eq!(started.session.as_deref(), Some("t1"));
        let ran = codex_line(
            &json!({"type": "item.completed", "item": {"type": "command_execution",
                "command": "cargo test", "aggregated_output": "ok", "exit_code": 1}}),
            &mut last,
        );
        let (_, calls) = ran.step.unwrap();
        assert_eq!(calls[0]["args"]["command"], "cargo test");
        assert_eq!(calls[0]["failed"], true);
        codex_line(
            &json!({"type": "item.completed", "item": {"type": "agent_message", "text": "Fixed it."}}),
            &mut last,
        );
        let done = codex_line(
            &json!({"type": "turn.completed", "usage": {"input_tokens": 7, "output_tokens": 2}}),
            &mut last,
        );
        assert_eq!(done.answer.as_deref(), Some("Fixed it."));
    }
}
