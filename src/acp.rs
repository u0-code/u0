//! One agent, spoken to over the Agent Client Protocol: JSON-RPC, a message
//! a line, on the standard input and output of a program the desk starts.
//!
//! The protocol's types are `agent-client-protocol-schema`'s; this file only
//! carries them. What the desk asks (`initialize`, `session/new`,
//! `session/prompt`, ...) is a request whose answer comes back through
//! [`Agent::request`]. What the agent says on its own comes to the desk as an
//! [`Event`]: an update to a session, or a question only a person can answer
//! (`session/request_permission`), which the desk answers later with
//! [`Agent::respond`]. Whatever else the agent asks of the desk it is told the
//! desk does not do: the desk offers it no file system and no terminal, so
//! that every agent works with its own tools, as it would in its own CLI.
//!
//! The program runs in a process group of its own, so that stopping it stops
//! whatever it started (an `npx` runs a node under it). Dropping the
//! [`Agent`] stops it.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

use agent_client_protocol_schema::v1::{RequestPermissionRequest, SessionNotification};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    process::{Child, Command},
    runtime::Runtime,
    sync::{mpsc, oneshot},
};
use tracing::{debug, warn};

/// Where every agent's pipes are served: two threads are plenty for a
/// handful of programs.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ghost-acp")
            .enable_all()
            .build()
            .expect("a tokio runtime")
    })
}

/// How to start an agent: a program, its arguments, what to add to the
/// environment, and the directory it starts in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub dir: Option<PathBuf>,
}

/// What an agent says to the desk without being asked.
#[derive(Debug)]
pub enum Event {
    /// Something happened in one of its sessions.
    Update(SessionNotification),
    /// It wants a person to choose before it goes on. Answer with
    /// [`Agent::respond`] under `id`.
    Permission {
        id: Value,
        request: RequestPermissionRequest,
    },
    /// It stopped, with the last of what it wrote to its standard error.
    Exited(String),
}

/// An error the agent answered with, or the reason there was no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The JSON-RPC code, when the agent gave one.
    pub code: Option<i64>,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// The code an agent answers with when it needs to be signed in first.
pub const AUTH_REQUIRED: i64 = -32000;

type Waiting = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, Failure>>>>>;

/// How many lines of an agent's standard error are kept to say why it
/// stopped.
const STDERR_KEPT: usize = 20;

/// A running agent.
pub struct Agent {
    out: mpsc::UnboundedSender<String>,
    waiting: Waiting,
    next: AtomicI64,
    pid: Option<u32>,
}

impl Agent {
    /// Start the program and serve its pipes; what it says on its own goes
    /// to `events`.
    pub fn start(launch: &Launch, events: mpsc::UnboundedSender<Event>) -> std::io::Result<Self> {
        let _enter = runtime().enter();
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .envs(launch.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        // A desk started from inside a Claude Code session must not make
        // the agent think it is nested in one.
        for (key, _) in std::env::vars() {
            if key.starts_with("CLAUDECODE") || key.starts_with("CLAUDE_CODE_") {
                command.env_remove(key);
            }
        }
        if let Some(dir) = &launch.dir {
            command.current_dir(dir);
        }
        let mut child = command.spawn()?;
        let pid = child.id();
        let (out, mut lines) = mpsc::unbounded_channel::<String>();
        let waiting: Waiting = Arc::default();

        let mut stdin = child.stdin.take().expect("piped stdin");
        runtime().spawn(async move {
            while let Some(line) = lines.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                    || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });

        // The last of what it writes to its standard error, handed over
        // when that closes: it says why the agent stopped.
        let stderr = child.stderr.take().expect("piped stderr");
        let tail = runtime().spawn(async move {
            let mut kept = VecDeque::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!(target: "ghost_desk::agent", "{line}");
                if kept.len() == STDERR_KEPT {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
            kept
        });

        let stdout = child.stdout.take().expect("piped stdout");
        let answers = waiting.clone();
        let replies = out.clone();
        runtime().spawn(read(child, stdout, answers, replies, events, tail));

        Ok(Self {
            out,
            waiting,
            next: AtomicI64::new(1),
            pid,
        })
    }

    /// Ask `method` with `params`, and wait for the answer.
    pub async fn request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, Failure> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().expect("waiting").insert(id, tx);
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(line.to_string()).is_err() {
            self.waiting.lock().expect("waiting").remove(&id);
            return Err(gone());
        }
        let value = rx.await.map_err(|_| gone())??;
        serde_json::from_value(value).map_err(|e| Failure {
            code: None,
            message: format!("{method} answered in a shape the desk does not read: {e}"),
        })
    }

    /// Say `method` with `params`, expecting no answer.
    pub fn notify<P: Serialize>(&self, method: &str, params: P) {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let _ = self.out.send(line.to_string());
    }

    /// Answer the agent's request `id`.
    pub fn respond<R: Serialize>(&self, id: &Value, result: R) {
        let line = json!({"jsonrpc": "2.0", "id": id, "result": result});
        let _ = self.out.send(line.to_string());
    }
}

impl Agent {
    /// Stop it at once, and all it started: when it would not stop a turn.
    pub fn kill(&self) {
        if let Some(pid) = self.pid.and_then(|p| i32::try_from(p).ok()) {
            // SAFETY: kill(2) on a process group the desk started; it
            // touches no memory.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // The whole group: an npx and the node it started.
        if let Some(pid) = self.pid.and_then(|p| i32::try_from(p).ok()) {
            // SAFETY: kill(2) on a process group the desk started; it
            // touches no memory.
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
    }
}

fn gone() -> Failure {
    Failure {
        code: None,
        message: "the agent is no longer running".into(),
    }
}

/// Read the agent's standard output to its end: answers to the desk's
/// requests go to whoever waits on them, everything else is an [`Event`].
async fn read(
    mut child: Child,
    stdout: tokio::process::ChildStdout,
    waiting: Waiting,
    out: mpsc::UnboundedSender<String>,
    events: mpsc::UnboundedSender<Event>,
    tail: tokio::task::JoinHandle<VecDeque<String>>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            warn!(
                "an agent wrote a line that is not JSON: {}",
                line.chars().take(200).collect::<String>()
            );
            continue;
        };
        match (
            message.get("method").and_then(Value::as_str),
            message.get("id"),
        ) {
            (Some(method), id) => incoming(method, id.cloned(), &message, &out, &events),
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else { continue };
                let Some(tx) = waiting.lock().expect("waiting").remove(&id) else {
                    continue;
                };
                let answer = match message.get("error") {
                    Some(error) => Err(Failure {
                        code: error.get("code").and_then(Value::as_i64),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("the agent refused")
                            .to_owned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(answer);
            }
            (None, None) => {}
        }
    }
    let status = child.wait().await.ok();
    waiting.lock().expect("waiting").clear();
    // A program it left behind may hold its standard error open: the reason
    // is waited for a moment, not for ever.
    let said: Vec<String> = tokio::time::timeout(std::time::Duration::from_secs(2), tail)
        .await
        .ok()
        .and_then(Result::ok)
        .map(Vec::from)
        .unwrap_or_default();
    let mut why = said.join("\n");
    if why.trim().is_empty() {
        why = match status.and_then(|s| s.code()) {
            Some(code) => format!("it exited with status {code}"),
            None => "it was stopped".into(),
        };
    }
    let _ = events.send(Event::Exited(why));
}

/// A message the agent began: a notification, or a request of the desk.
fn incoming(
    method: &str,
    id: Option<Value>,
    message: &Value,
    out: &mpsc::UnboundedSender<String>,
    events: &mpsc::UnboundedSender<Event>,
) {
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match (method, id) {
        ("session/update", None) => match serde_json::from_value(params) {
            Ok(update) => {
                let _ = events.send(Event::Update(update));
            }
            // A kind of update this desk does not know yet: it is skipped,
            // the session goes on.
            Err(e) => debug!("skipped an update the desk does not read: {e}"),
        },
        ("session/request_permission", Some(id)) => match serde_json::from_value(params) {
            Ok(request) => {
                let _ = events.send(Event::Permission { id, request });
            }
            Err(e) => {
                let line = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": e.to_string()}});
                let _ = out.send(line.to_string());
            }
        },
        (_, Some(id)) => {
            let line = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method} is not offered by this client")}});
            let _ = out.send(line.to_string());
        }
        (_, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell script that answers one request and says one update, the way
    /// an agent would.
    fn script(body: &str) -> Launch {
        Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), body.into()],
            env: vec![],
            dir: None,
        }
    }

    #[test]
    fn an_answer_reaches_the_request_it_answers_and_an_update_arrives_as_an_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let agent = Agent::start(
            &script(
                r#"read line
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"s1"}}'
read line"#,
            ),
            tx,
        )
        .unwrap();
        let answer: Value = runtime()
            .block_on(agent.request("session/new", json!({"cwd": "/", "mcpServers": []})))
            .unwrap();
        assert_eq!(answer["sessionId"], "s1");
        let event = runtime().block_on(rx.recv()).unwrap();
        assert!(matches!(event, Event::Update(u) if &*u.session_id.0 == "s1"));
    }

    #[test]
    fn a_request_the_desk_does_not_offer_is_refused_and_an_exit_says_why() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _agent = Agent::start(
            &script(
                r#"printf '%s\n' '{"jsonrpc":"2.0","id":7,"method":"fs/read_text_file","params":{}}'
read line
echo "$line" >&2
exit 3"#,
            ),
            tx,
        )
        .unwrap();
        let Event::Exited(why) = runtime().block_on(rx.recv()).unwrap() else {
            panic!("expected the agent to exit");
        };
        assert!(why.contains("-32601"), "{why}");
        assert!(why.contains("fs/read_text_file is not offered"), "{why}");
    }
}
