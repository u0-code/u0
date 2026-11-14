//! `shell`: run a bash command on the machine.

use std::{fmt::Write as _, sync::Arc, sync::atomic::Ordering, time::Duration};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use super::{Desk, clip};
use crate::{
    machine::quote,
    prompts::{Loop, Prompts},
    tool::{Outcome, Tool, text},
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

impl Shell {
    /// Wait for a background job, up to `secs`, and report what it printed
    /// since the last look.
    async fn look_at(&self, job: u64, secs: u64) -> Outcome {
        let dir = format!("/tmp/ghost-{}-job-{job}", self.desk.tag);
        // One script does the waiting, so a slow job costs one call, not a
        // loop of them. `seen` remembers how far the log has been read.
        let script = format!(
            "d={dir}; [ -f $d/log ] || {{ echo \"there is no job {job}\"; exit 3; }}\n\
             n=0; while [ ! -f $d/status ] && [ $n -lt {} ]; do sleep 0.5; n=$((n+1)); done\n\
             from=$(cat $d/seen 2>/dev/null || echo 0); size=$(wc -c < $d/log | tr -d ' ')\n\
             tail -c +$((from+1)) $d/log; echo $size > $d/seen\n\
             if [ -f $d/status ]; then printf '\\n{WHERE}done:%s\\n' \"$(cat $d/status)\"; else printf '\\n{WHERE}running\\n'; fi",
            secs * 2
        );
        let out = match self
            .desk
            .exec(&script, None, Duration::from_secs(secs + 30))
            .await
        {
            Ok(out) => out,
            Err(e) => return Outcome::fail(format!("could not look at job {job}: {e}")),
        };
        let Some(at) = out.text.rfind(WHERE) else {
            return Outcome::fail(out.text.trim().to_owned());
        };
        let state = out.text[at + WHERE.len()..].trim();
        let printed = out.text[..at].trim_end();
        let mut output = clip(printed, 5000, 9000);
        if output.is_empty() {
            output.push_str("(nothing new printed)");
        }
        let failed = match state.strip_prefix("done:") {
            Some("0") => {
                let _ = write!(output, "\n[job {job} finished]");
                false
            }
            Some(code) => {
                let _ = write!(output, "\n[job {job} finished with exit {code}]");
                true
            }
            None => {
                let _ = write!(
                    output,
                    "\n[job {job} is still running after {secs}s. Look again later, or get on with something else.]"
                );
                false
            }
        };
        Outcome {
            failed,
            ..Outcome::ok(output)
        }
    }
}

#[async_trait]
impl Tool for Shell {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn spec(&self, words: &Prompts, lp: Loop) -> ToolSpec {
        words.spec(
            lp,
            "shell",
            json!({
                "command": {"type": "string"},
                "timeout": {"type": "integer"},
                "background": {"type": "boolean"},
                "job": {"type": "integer"},
            }),
            &[],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let secs = args["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(1, MAX_TIMEOUT);
        if let Some(job) = args["job"].as_u64() {
            return self.look_at(job, secs).await;
        }
        let command = match text(args, "command") {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };

        if args["background"].as_bool() == Some(true) {
            let n = self.desk.jobs.fetch_add(1, Ordering::Relaxed) + 1;
            let dir = format!("/tmp/ghost-{}-job-{n}", self.desk.tag);
            // When the job ends, however it ends, its exit status is left
            // beside its log, so a later look can tell done from running.
            let script = format!(
                "rm -rf {dir}; mkdir -p {dir}\nnohup bash -c {} > {dir}/log 2>&1 < /dev/null &\necho \"started as job {n} (pid $!). Look at it with shell {{\\\"job\\\": {n}}}; its output is in {dir}/log\"",
                quote(&format!("trap 'echo $? > {dir}/status' EXIT\n{command}"))
            );
            return match self.desk.exec(&script, None, Duration::from_secs(30)).await {
                Ok(out) => Outcome {
                    failed: !out.ok(),
                    ..Outcome::ok(out.text.trim())
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
        if output.len() < printed.trim_end().len() {
            // Too long to show whole. Keep all of it where it can be searched.
            let n = self.desk.jobs.fetch_add(1, Ordering::Relaxed) + 1;
            let path = format!("/tmp/ghost-{}-out-{n}.txt", self.desk.tag);
            let saved = self
                .desk
                .exec(
                    &format!("cat > {path}"),
                    Some(printed.as_bytes().to_vec()),
                    Duration::from_secs(30),
                )
                .await
                .is_ok_and(|saved| saved.ok());
            if saved {
                let _ = write!(
                    output,
                    "\n[the whole output, {} lines, is in {path}: search it with grep or read part of it with sed]",
                    printed.lines().count()
                );
            }
        }
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
            failed: !out.ok(),
            ..Outcome::ok(output)
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
    async fn a_background_job_can_be_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let sh = shell(dir.path());
        let started = sh
            .call(&json!({"command": "echo one; sleep 1.5; echo two; exit 4", "background": true}))
            .await;
        assert!(!started.failed, "{}", started.output);
        let n: u64 = started
            .output
            .split("job ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .expect("the reply names the job");

        // A short look while it is still going: what it has printed so far.
        let early = sh.call(&json!({"job": n, "timeout": 1})).await;
        assert!(early.output.starts_with("one"), "{}", early.output);
        assert!(early.output.contains("still running"), "{}", early.output);

        // A longer look waits for it to end and shows only what is new.
        let late = sh.call(&json!({"job": n, "timeout": 20})).await;
        assert!(late.failed);
        assert!(late.output.starts_with("two"), "{}", late.output);
        assert!(
            late.output
                .ends_with(&format!("[job {n} finished with exit 4]")),
            "{}",
            late.output
        );

        let none = sh.call(&json!({"job": 999})).await;
        assert!(
            none.failed && none.output.contains("there is no job 999"),
            "{}",
            none.output
        );
    }
}
