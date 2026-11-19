//! `shell`: run a bash command on the machine.

use std::{sync::Arc, sync::atomic::Ordering, time::Duration};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use super::{Desk, clip};
use crate::{
    machine::quote,
    prompts::{self, Loop, Store, TEXTS},
    tool::{Outcome, Tool, text},
};

const DEFAULT_TIMEOUT: u64 = 120;
const MAX_TIMEOUT: u64 = 3600;
/// Marks the line a command's final directory is reported on.
const WHERE: &str = "\u{1e}GHOST_CWD:";
/// What a look at a job that was never started exits with.
const NO_JOB: i32 = 3;

pub struct Shell {
    desk: Arc<Desk>,
}

impl Shell {
    pub fn new(desk: Arc<Desk>) -> Self {
        Self { desk }
    }
}

impl Shell {
    /// Wait for a background job, up to `secs`, and report in `words` what
    /// it printed since the last look.
    async fn look_at(&self, words: &Store, job: u64, secs: u64) -> Outcome {
        let dir = format!("/tmp/ghost-{}-job-{job}", self.desk.tag);
        let number = || ("job", job.to_string());
        // One script does the waiting, so a slow job costs one call, not a
        // loop of them. `seen` remembers how far the log has been read.
        let script = format!(
            "d={dir}; [ -f $d/log ] || exit {NO_JOB}\n\
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
            Err(e) => {
                return Outcome::fail(words.fill(
                    TEXTS,
                    "job_unreachable",
                    &[number(), ("why", e.to_string())],
                ));
            }
        };
        if out.code == Some(NO_JOB) {
            return Outcome::fail(words.fill(TEXTS, "no_job", &[number()]));
        }
        let Some(at) = out.text.rfind(WHERE) else {
            return Outcome::fail(out.text.trim().to_owned());
        };
        let state = out.text[at + WHERE.len()..].trim();
        let printed = out.text[..at].trim_end();
        let mut output = clip(words, printed, 5000, 9000);
        if output.is_empty() {
            output.push_str(words.text(TEXTS, "nothing_new"));
        }
        let (said, failed) = match state.strip_prefix("done:") {
            Some("0") => (words.fill(TEXTS, "job_finished", &[number()]), false),
            Some(code) => (
                words.fill(TEXTS, "job_failed", &[number(), ("code", code.to_owned())]),
                true,
            ),
            None => (
                words.fill(
                    TEXTS,
                    "job_running",
                    &[number(), ("seconds", secs.to_string())],
                ),
                false,
            ),
        };
        output.push_str(&said);
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

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
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

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let secs = args["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(1, MAX_TIMEOUT);
        if let Some(job) = args["job"].as_u64() {
            return self.look_at(words, job, secs).await;
        }
        let command = match text(words, args, "command") {
            Ok(c) => c,
            Err(refusal) => return refusal,
        };

        if args["background"].as_bool() == Some(true) {
            let n = self.desk.jobs.fetch_add(1, Ordering::Relaxed) + 1;
            let dir = format!("/tmp/ghost-{}-job-{n}", self.desk.tag);
            // When the job ends, however it ends, its exit status is left
            // beside its log, so a later look can tell done from running.
            let script = format!(
                "rm -rf {dir}; mkdir -p {dir}\nnohup bash -c {} > {dir}/log 2>&1 < /dev/null &\necho $!",
                quote(&format!("trap 'echo $? > {dir}/status' EXIT\n{command}"))
            );
            return match self.desk.exec(&script, None, Duration::from_secs(30)).await {
                Ok(out) if out.ok() => {
                    // The last line is its process; anything before it is
                    // what the shell said on the way.
                    let printed = out.text.trim();
                    let (before, pid) = printed.rsplit_once('\n').unwrap_or(("", printed));
                    let started = words.fill(
                        TEXTS,
                        "job_started",
                        &[
                            ("job", n.to_string()),
                            ("pid", pid.trim().to_owned()),
                            ("log", format!("{dir}/log")),
                        ],
                    );
                    Outcome::ok(if before.is_empty() {
                        started
                    } else {
                        format!("{before}\n{started}")
                    })
                }
                Ok(out) => Outcome::fail(out.text.trim()),
                Err(e) => Outcome::fail(words.fill(
                    TEXTS,
                    "not_started_command",
                    &[("why", e.to_string())],
                )),
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
            Err(e) => {
                return Outcome::fail(words.fill(
                    TEXTS,
                    "not_run_command",
                    &[("why", e.to_string())],
                ));
            }
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

        let mut output = clip(words, printed.trim_end(), 5000, 9000);
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
                output.push_str(&words.fill(
                    TEXTS,
                    "output_kept",
                    &[
                        ("lines", printed.lines().count().to_string()),
                        ("path", path),
                    ],
                ));
            }
        }
        if out.dropped > 0 {
            output.push_str(&words.fill(
                TEXTS,
                "output_dropped",
                &[("bytes", out.dropped.to_string())],
            ));
        }
        if output.is_empty() {
            output.push_str(words.text(TEXTS, "no_output"));
        }
        if out.timed_out {
            output.push_str(&words.fill(TEXTS, "timed_out", &[("seconds", secs.to_string())]));
        } else if out.code != Some(0) {
            output.push_str(&match out.code {
                Some(code) => words.fill(TEXTS, "exited", &[("code", code.to_string())]),
                None => words.text(TEXTS, "killed").to_owned(),
            });
        }
        if let Some(dir) = moved {
            output.push_str(&words.fill(TEXTS, "moved", &[("dir", dir)]));
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
            .call(
                &json!({"command": "mkdir sub && cd sub && echo hi"}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(!first.failed);
        assert!(first.output.starts_with("hi"), "{}", first.output);
        let second = sh
            .call(
                &json!({"command": "basename \"$PWD\"; false"}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(second.failed);
        assert_eq!(second.output, "sub\n[exit 1]");
    }

    #[tokio::test]
    async fn an_overlong_command_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let out = shell(dir.path())
            .call(
                &json!({"command": "sleep 20", "timeout": 1}),
                &crate::Prompts::shipped(),
            )
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
            .call(
                &json!({"command": "echo one; sleep 1.5; echo two; exit 4", "background": true}),
                &crate::Prompts::shipped(),
            )
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
        let early = sh
            .call(&json!({"job": n, "timeout": 1}), &crate::Prompts::shipped())
            .await;
        assert!(early.output.starts_with("one"), "{}", early.output);
        assert!(early.output.contains("still running"), "{}", early.output);

        // A longer look waits for it to end and shows only what is new.
        let late = sh
            .call(
                &json!({"job": n, "timeout": 20}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(late.failed);
        assert!(late.output.starts_with("two"), "{}", late.output);
        assert!(
            late.output
                .ends_with(&format!("[job {n} finished with exit 4]")),
            "{}",
            late.output
        );

        let none = sh
            .call(&json!({"job": 999}), &crate::Prompts::shipped())
            .await;
        assert!(
            none.failed && none.output.contains("there is no job 999"),
            "{}",
            none.output
        );
    }
}
