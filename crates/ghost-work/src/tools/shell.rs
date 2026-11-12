//! `shell`: run a bash command on the machine.

use std::{fmt::Write as _, sync::Arc, sync::atomic::Ordering, time::Duration};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use super::{Desk, clip};
use crate::{
    machine::quote,
    tool::{Outcome, Tool, spec, text},
};

const DEFAULT_TIMEOUT: u64 = 120;
const MAX_TIMEOUT: u64 = 3600;
/// Marks the line a command's final directory is reported on.
const WHERE: &str = "\u{1e}GHOST_CWD:";

pub struct Shell {
    desk: Arc<Desk>,
}

impl Shell {
    pub fn new(desk: Arc<Desk>) -> Self {
        Self { desk }
    }
}

#[async_trait]
impl Tool for Shell {
    fn spec(&self) -> ToolSpec {
        spec(
            "shell",
            "Run a bash command and get its output (stdout and stderr together) and exit status. \
             Not interactive: there is no terminal and nothing can be typed at a prompt. \
             The working directory carries over to the next call; exported variables do not. \
             Long output is cut in the middle, so prefer commands that print what you need.",
            json!({
                "command": {"type": "string", "description": "The bash command or script to run."},
                "timeout": {"type": "integer", "description": "Seconds before the command is stopped. Default 120, at most 3600."},
                "background": {"type": "boolean", "description": "Start the command and return at once, for servers and anything long-running. Its output goes to a log file whose path is returned."},
            }),
            &["command"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let command = match text(args, "command") {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };
        let secs = args["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(1, MAX_TIMEOUT);

        if args["background"].as_bool() == Some(true) {
            let n = self.desk.jobs.fetch_add(1, Ordering::Relaxed) + 1;
            let log = format!("/tmp/ghost-job-{n}.log");
            let script = format!(
                "nohup bash -c {} > {log} 2>&1 &\necho \"started in the background: pid $!, output in {log}\"",
                quote(command)
            );
            return match self.desk.exec(&script, None, Duration::from_secs(30)).await {
                Ok(out) => Outcome {
                    output: out.text.trim().to_owned(),
                    failed: !out.ok(),
                },
                Err(e) => Outcome::fail(format!("could not start the command: {e}")),
            };
        }

        // Report where the command ended up, so the next one starts there.
        let script = format!(
            "{command}\n__ghost_status=$?\nprintf '\\n{WHERE}%s\\n' \"$PWD\"\nexit $__ghost_status"
        );
        let out = match self
            .desk
            .exec(&script, None, Duration::from_secs(secs))
            .await
        {
            Ok(out) => out,
            Err(e) => return Outcome::fail(format!("could not run the command: {e}")),
        };

        let mut printed = out.text.as_str();
        let mut moved = None;
        if let Some(at) = printed.rfind(WHERE) {
            let dir = printed[at + WHERE.len()..]
                .lines()
                .next()
                .unwrap_or_default()
                .trim();
            if !dir.is_empty() && self.desk.moved_to(dir.to_owned()) {
                moved = Some(dir.to_owned());
            }
            printed = printed[..at].strip_suffix('\n').unwrap_or(&printed[..at]);
        }

        let mut output = clip(printed.trim_end(), 5000, 9000);
        if out.dropped > 0 {
            let _ = write!(output, "\n[{} bytes of output were dropped]", out.dropped);
        }
        if output.is_empty() {
            output.push_str("(no output)");
        }
        if out.timed_out {
            let _ = write!(
                output,
                "\n[stopped: still running after {secs}s. Raise `timeout`, or use `background` and poll.]"
            );
        } else if out.code != Some(0) {
            let _ = match out.code {
                Some(code) => write!(output, "\n[exit {code}]"),
                None => write!(output, "\n[killed by a signal]"),
            };
        }
        if let Some(dir) = moved {
            let _ = write!(output, "\n[now in {dir}]");
        }
        Outcome {
            output,
            failed: !out.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Local;

    fn shell(dir: &std::path::Path) -> Shell {
        Shell::new(Desk::new(Arc::new(Local::new(dir))))
    }

    #[tokio::test]
    async fn the_directory_carries_over_and_status_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let sh = shell(dir.path());
        let first = sh
            .call(&json!({"command": "mkdir sub && cd sub && echo hi"}))
            .await;
        assert!(!first.failed);
        assert!(first.output.starts_with("hi"), "{}", first.output);
        let second = sh
            .call(&json!({"command": "basename \"$PWD\"; false"}))
            .await;
        assert!(second.failed);
        assert_eq!(second.output, "sub\n[exit 1]");
    }

    #[tokio::test]
    async fn an_overlong_command_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let out = shell(dir.path())
            .call(&json!({"command": "sleep 20", "timeout": 1}))
            .await;
        assert!(out.failed);
        assert!(
            out.output.contains("still running after 1s"),
            "{}",
            out.output
        );
    }

    #[tokio::test]
    async fn a_background_job_keeps_running() {
        let dir = tempfile::tempdir().unwrap();
        let sh = shell(dir.path());
        let started = sh
            .call(&json!({"command": "sleep 1; echo finished > marker", "background": true}))
            .await;
        assert!(
            started.output.contains("started in the background"),
            "{}",
            started.output
        );
        tokio::time::sleep(Duration::from_millis(1800)).await;
        let seen = sh.call(&json!({"command": "cat marker"})).await;
        assert_eq!(seen.output, "finished");
    }
}
