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

/// Why an installed agent cannot do parts here, and what whoever runs her
/// does about it: an exact command, the way Codex names the command that
/// fixes a server it cannot reach. It is never hers to fix: installing,
/// upgrading and logging in are done on someone's account, as them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unusable {
    pub why: String,
    pub fix: String,
}

/// What a check of an agent found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    /// It is there, but something could not be checked without running it
    /// on a part: what.
    Unchecked(String),
    Unusable(Unusable),
}

/// What is wrong with an agent that cannot work, as far as can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    Missing,
    LoggedOut,
    Outdated,
}

impl Agent {
    /// `cause`, said with the command that fixes it for this agent.
    fn unusable(&self, cause: Cause) -> Unusable {
        let program = &self.cli.program;
        let (why, fix) = match (cause, self.cli.kind) {
            (Cause::Missing, CliKind::Claude) => (
                format!("{program} is not installed"),
                "install Claude Code (`npm install -g @anthropic-ai/claude-code`), or set `program` in its provider to where it is".to_owned(),
            ),
            (Cause::Missing, CliKind::Codex) => (
                format!("{program} is not installed"),
                "install Codex (`npm install -g @openai/codex`, or `brew install codex`), or set `program` in its provider to where it is".to_owned(),
            ),
            (Cause::LoggedOut, CliKind::Claude) => (
                "Claude Code is not logged in".to_owned(),
                "run `claude` once in a terminal, as the user the daemon runs as, and log in; on a server, run `claude setup-token` and put the token in CLAUDE_CODE_OAUTH_TOKEN in the .env file".to_owned(),
            ),
            (Cause::LoggedOut, CliKind::Codex) => (
                "Codex is not logged in".to_owned(),
                "run `codex login` as the user the daemon runs as; on a server, `codex login --device-auth`, or `printenv OPENAI_API_KEY | codex login --with-api-key`".to_owned(),
            ),
            (Cause::Outdated, CliKind::Claude) => (
                "Claude Code is older than its account or model needs".to_owned(),
                "update it: `claude update`".to_owned(),
            ),
            (Cause::Outdated, CliKind::Codex) => (
                "Codex is older than its account or model needs".to_owned(),
                "update it: `npm install -g @openai/codex@latest`, or `brew upgrade codex` if it came from Homebrew".to_owned(),
            ),
        };
        Unusable { why, fix }
    }

    /// Whether it can be handed parts: installed, saying its version, and
    /// logged in where that can be seen without running it on something.
    /// Codex says so itself (`codex login status`); Claude Code does not,
    /// short of a key or token in the environment.
    pub async fn check(&self) -> Readiness {
        let says = |args: &[&str]| {
            let mut command = Command::new(&self.cli.program);
            command.args(args).stdin(Stdio::null()).kill_on_drop(true);
            for name in handed_on_not(std::env::vars()) {
                command.env_remove(name);
            }
            async move {
                tokio::time::timeout(Duration::from_secs(20), command.output())
                    .await
                    .map_err(|_| std::io::Error::other("it did not answer"))
                    .and_then(|done| done)
            }
        };
        match says(&["--version"]).await {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {}
            _ => return Readiness::Unusable(self.unusable(Cause::Missing)),
        }
        match self.cli.kind {
            CliKind::Codex => match says(&["login", "status"]).await {
                // It says so on stderr.
                Ok(out)
                    if out.status.success()
                        && [&out.stdout, &out.stderr].iter().any(|said| {
                            String::from_utf8_lossy(said)
                                .to_lowercase()
                                .contains("logged in using")
                        }) =>
                {
                    Readiness::Ready
                }
                _ => Readiness::Unusable(self.unusable(Cause::LoggedOut)),
            },
            CliKind::Claude => {
                let keyed = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
                    .iter()
                    .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()));
                if keyed {
                    Readiness::Ready
                } else {
                    Readiness::Unchecked(format!(
                        "whether it is logged in cannot be seen without running it; if a part comes back saying it is not: {}",
                        self.unusable(Cause::LoggedOut).fix
                    ))
                }
            }
        }
    }
}

/// What a failure an agent reported says is wrong with it, where that is
/// something whoever runs her fixes.
fn cause_of(text: &str) -> Option<Cause> {
    let text = text.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if any(&[
        "requires a newer version",
        "please upgrade",
        "update to the latest",
        "upgrade to the latest",
    ]) {
        Some(Cause::Outdated)
    } else if any(&[
        "please run /login",
        "not logged in",
        "authentication_error",
        "oauth access token",
        "invalid api key",
        "401 ",
        "403 ",
        "codex login",
    ]) {
        Some(Cause::LoggedOut)
    } else {
        None
    }
}

/// The names in `vars`, this process's environment, that belong to a
/// Claude Code session the daemon was itself started from, and are not
/// handed on: an agent that inherited them would take that session, and
/// its login, for its own. A token put in the environment on purpose
/// (`CLAUDE_CODE_OAUTH_TOKEN`) is handed on.
fn handed_on_not(vars: impl Iterator<Item = (String, String)>) -> Vec<String> {
    let names: Vec<String> = vars.map(|(name, _)| name).collect();
    let inside = names.iter().any(|n| n == "CLAUDECODE");
    names
        .into_iter()
        .filter(|n| {
            n == "CLAUDECODE"
                || (n.starts_with("CLAUDE_CODE_") && n != "CLAUDE_CODE_OAUTH_TOKEN")
                || n.starts_with("CLAUDE_AGENT_SDK_")
                // Inside a session, the address of the session's own host.
                || (inside && n == "ANTHROPIC_BASE_URL")
        })
        .collect()
}

/// How one run of the agent came out.
enum Run {
    /// It answered, with this.
    Answered(String),
    /// It could not be started, or said it failed.
    Failed(String),
    /// It cannot work here until whoever runs her fixes something.
    Unusable(Unusable),
    /// The sitting ended, or the part was stopped, while it ran.
    Halted,
    /// The sitting's time ran out while it ran.
    OutOfTime,
}

/// Do `part` with `agent` until it ends, the sitting halts, or `deadline`.
/// The first run is given `task`; a later one, what was passed on to the
/// part since, or `go_on` if nothing was.
///
/// An agent that cannot work here at all comes back as why and what fixes
/// it, for the work to say to whoever runs her and not to try itself.
pub(crate) async fn run(
    agent: &Agent,
    part: &Undertaking,
    task: &str,
    go_on: &str,
    halt: &AtomicBool,
    deadline: Instant,
) -> Result<Ended, Unusable> {
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
                return Ok(part
                    .state()
                    .finish
                    .map_or(Ended::Finished, |f| f.outcome.into()));
            }
            Run::Failed(why) => return Ok(Ended::Failed(why)),
            Run::Unusable(unusable) => return Err(unusable),
            Run::Halted => {
                return Ok(part
                    .state()
                    .finish
                    .map_or(Ended::Paused, |f| f.outcome.into()));
            }
            Run::OutOfTime => return Ok(Ended::OutOfTime),
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
    // Nothing of a Claude Code session the daemon runs inside.
    for name in handed_on_not(std::env::vars()) {
        command.env_remove(name);
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
    /// Something it said of itself that is not a failure on its own.
    note: Option<String>,
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
        Some(kind @ ("turn.failed" | "error")) => {
            let said = event["error"]["message"]
                .as_str()
                .or_else(|| event["message"].as_str())
                .unwrap_or("it reported an error")
                .to_owned();
            // A dropped stream it is reconnecting to is not the end of the
            // turn: kept, in case the turn never ends, but not a failure.
            if kind == "error" && said.trim_start().to_lowercase().starts_with("reconnecting") {
                heard.note = Some(said);
            } else {
                heard.failed = Some(said);
            }
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Run::Unusable(agent.unusable(Cause::Missing));
        }
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
    let mut noted = String::new();
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
        if let Some(note) = heard.note {
            noted = note;
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
    let why = match (failed, answer) {
        (Some(why), _) => why,
        (None, Some(answer)) => return Run::Answered(answer),
        (None, None) => format!("ended without an answer. {noted} {}", complaint.trim()),
    };
    // What it said, and what it complained of, can say what is wrong with
    // it rather than with the part.
    match cause_of(&format!("{why}\n{complaint}")) {
        Some(cause) => Run::Unusable(agent.unusable(cause)),
        None => Run::Failed(preview(&why, 400)),
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
        // A stream it reconnects to is noted, not a failure.
        let dropped = codex_line(
            &json!({"type": "error", "message": "Reconnecting... 2/5 (stream disconnected)"}),
            &mut last,
        );
        assert!(dropped.failed.is_none() && dropped.note.is_some());
        let failed = codex_line(
            &json!({"type": "turn.failed", "error": {"message": "stream disconnected"}}),
            &mut last,
        );
        assert_eq!(failed.failed.as_deref(), Some("stream disconnected"));
    }

    #[test]
    fn what_an_agent_failed_with_says_whether_it_needs_a_login_or_an_upgrade() {
        assert_eq!(
            cause_of("API Error: 401 {\"error\":{\"type\":\"authentication_error\"}}"),
            Some(Cause::LoggedOut)
        );
        assert_eq!(
            cause_of("API Error: 403 forbidden · Please run /login"),
            Some(Cause::LoggedOut)
        );
        assert_eq!(
            cause_of("The 'gpt-6-astra' model requires a newer version of Codex. Please upgrade"),
            Some(Cause::Outdated)
        );
        assert_eq!(cause_of("cargo test failed: 3 tests"), None);
    }

    #[test]
    fn nothing_of_a_claude_code_session_the_daemon_runs_in_is_handed_on() {
        let vars = |pairs: &[&str]| {
            pairs
                .iter()
                .map(|n| ((*n).to_owned(), "x".to_owned()))
                .collect::<Vec<_>>()
                .into_iter()
        };
        let inside = handed_on_not(vars(&[
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
        // Outside one, a base URL set on purpose is handed on.
        assert!(handed_on_not(vars(&["ANTHROPIC_BASE_URL", "PATH"])).is_empty());
    }

    fn fake(dir: &std::path::Path, kind: CliKind, body: &str) -> Agent {
        use std::os::unix::fs::PermissionsExt;
        let program = dir.join("cli");
        std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        Agent {
            name: "it".into(),
            cli: CliAgent {
                kind,
                program: program.display().to_string(),
                model: None,
                args: Vec::new(),
            },
            dir: dir.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn a_check_finds_an_agent_missing_or_logged_out_and_says_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let mut missing = fake(dir.path(), CliKind::Claude, "exit 0");
        missing.cli.program = dir.path().join("nowhere").display().to_string();
        let Readiness::Unusable(u) = missing.check().await else {
            panic!("a program that is not there")
        };
        assert!(
            u.why.ends_with("is not installed") && u.fix.contains("npm install"),
            "{u:?}"
        );

        let codex = fake(
            dir.path(),
            CliKind::Codex,
            r#"case "$*" in --version) echo "codex-cli 0.169.0";; "login status") echo "Not logged in"; exit 1;; esac"#,
        );
        let Readiness::Unusable(u) = codex.check().await else {
            panic!("codex says it is not logged in")
        };
        assert_eq!(u.why, "Codex is not logged in");
        assert!(u.fix.contains("codex login"), "{u:?}");

        let logged_in = fake(
            dir.path(),
            CliKind::Codex,
            r#"case "$*" in --version) echo "codex-cli 0.169.0";; "login status") echo "Logged in using ChatGPT" >&2;; esac"#,
        );
        assert_eq!(logged_in.check().await, Readiness::Ready);
    }
}
