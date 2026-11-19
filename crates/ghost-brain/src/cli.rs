//! Relays through the official `claude` and `codex` command-line tools.
//!
//! Both are driven as subprocesses in their documented non-interactive modes
//! (`claude -p`, `codex exec`). Nothing here touches either tool's stored
//! credentials; whatever account the CLI is logged into is what answers.
//!
//! These exist so you can experiment on a subscription you already pay for.
//! A ghost other people talk to should run on an API key.

use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::{Brain, CliAgent, Completion, Error, Reply, Result};

/// How long one answer may take when the configuration does not say.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// `claude -p`, JSON output, one turn.
pub(crate) struct ClaudeCode {
    agent: CliAgent,
    timeout: Duration,
    window: Option<u64>,
}

impl ClaudeCode {
    pub(crate) fn new(agent: CliAgent, timeout: Option<Duration>) -> Self {
        Self {
            window: crate::cli_models::window(&agent),
            agent,
            timeout: timeout.unwrap_or(DEFAULT_TIMEOUT),
        }
    }
}

#[async_trait]
impl Brain for ClaudeCode {
    fn label(&self) -> String {
        format!(
            "claude-code/{}",
            self.agent.model.as_deref().unwrap_or("default")
        )
    }

    fn window(&self) -> Option<u64> {
        self.window
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        let mut args: Vec<OsString> = vec![
            "-p".into(),
            "--output-format".into(),
            "json".into(),
            "--max-turns".into(),
            "1".into(),
            "--system-prompt".into(),
            req.system.clone().into(),
            // She is asked for words, not work. With its own tools left on
            // the CLI sends thousands of tokens of tool definitions with
            // every call, and may reach for one instead of answering.
            "--tools".into(),
            "".into(),
            "--strict-mcp-config".into(),
            "--disable-slash-commands".into(),
        ];
        args.extend(self.agent.args.iter().map(OsString::from));
        if let Some(m) = &self.agent.model {
            args.push("--model".into());
            args.push(m.clone().into());
        }
        // The system prompt already went in by flag, so leave it out here.
        let prompt = Completion {
            system: String::new(),
            ..req.clone()
        }
        .transcript("them", "you");
        args.push(prompt.into());

        let out = run(&self.agent.program, &args, None, self.timeout).await?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        // `claude` reports API failures as a JSON result with `is_error`, and
        // may exit non-zero while doing so. Read the JSON first.
        let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) else {
            return Err(failed(&self.agent.program, &out));
        };
        if v["is_error"].as_bool() == Some(true) {
            return Err(Error::Malformed {
                provider: "claude-code".into(),
                detail: v["result"].as_str().unwrap_or("is_error").to_owned(),
            });
        }
        let text = v["result"].as_str().unwrap_or_default().trim().to_owned();
        if text.is_empty() {
            return Err(Error::Malformed {
                provider: "claude-code".into(),
                detail: format!("empty result in {stdout}"),
            });
        }
        Ok(Reply {
            text,
            model: self
                .agent
                .model
                .clone()
                .unwrap_or_else(|| "claude-code".into()),
            input_tokens: v["usage"]["input_tokens"].as_u64(),
            output_tokens: v["usage"]["output_tokens"].as_u64(),
        })
    }
}

/// `codex exec`, read-only sandbox, final message captured to a file.
pub(crate) struct Codex {
    agent: CliAgent,
    timeout: Duration,
    /// Read from what Codex keeps on this machine when it is made, not
    /// whenever it is asked: asking must not wait on the disk.
    window: Option<u64>,
}

impl Codex {
    pub(crate) fn new(agent: CliAgent, timeout: Option<Duration>) -> Self {
        Self {
            window: crate::cli_models::window(&agent),
            agent,
            timeout: timeout.unwrap_or(DEFAULT_TIMEOUT),
        }
    }
}

#[async_trait]
impl Brain for Codex {
    fn label(&self) -> String {
        format!("codex/{}", self.agent.model.as_deref().unwrap_or("default"))
    }

    fn window(&self) -> Option<u64> {
        self.window
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        // An empty working directory, so codex has nothing to read and no repo
        // to complain about, and a file for its last message.
        let scratch = tempfile::tempdir()?;
        let last = scratch.path().join("last.md");
        let mut args: Vec<OsString> = vec![
            "exec".into(),
            "--skip-git-repo-check".into(),
            "-s".into(),
            "read-only".into(),
            "-o".into(),
            last.clone().into(),
        ];
        if let Some(m) = &self.agent.model {
            args.push("-m".into());
            args.push(m.clone().into());
        }
        // She is asked for words: no shell, no web, no project files.
        for flag in [
            "--disable",
            "shell_tool",
            "-c",
            "web_search=\"disabled\"",
            "-c",
            "project_doc_max_bytes=0",
        ] {
            args.push(flag.into());
        }
        args.extend(self.agent.args.iter().map(OsString::from));
        args.push(req.transcript("them", "you").into());

        run(
            &self.agent.program,
            &args,
            Some(scratch.path()),
            self.timeout,
        )
        .await?;
        let text = tokio::fs::read_to_string(&last)
            .await
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            return Err(Error::Malformed {
                provider: "codex".into(),
                detail: "no last message written".into(),
            });
        }
        Ok(Reply {
            text,
            model: self.agent.model.clone().unwrap_or_else(|| "codex".into()),
            ..Reply::default()
        })
    }
}

async fn run(
    program: &str,
    args: &[OsString],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Whoever runs her keeps notes and instructions for their own sessions.
    // Those are theirs, not hers, and must not leak into who she is.
    cmd.env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let child = ghost_net::spawn(&mut cmd).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotInstalled(program.to_owned())
        } else {
            Error::Io(e)
        }
    })?;
    let out = tokio::time::timeout(timeout, child.output())
        .await
        .map_err(|_| Error::Timeout(timeout))??;
    Ok(out)
}

fn failed(program: &str, out: &std::process::Output) -> Error {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Error::Cli {
        program: program.to_owned(),
        status: out.status.to_string(),
        stderr: if stderr.is_empty() { stdout } else { stderr },
    }
}
