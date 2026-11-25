//! One agent, spoken to over the Agent Client Protocol on the standard input
//! and output of a program the desk starts.
//!
//! The protocol is the `agent-client-protocol` SDK's: its connection frames
//! the messages, pairs each answer with its request and refuses what the
//! desk does not offer (a file system, a terminal: every agent works with its
//! own tools, as in its own CLI). This file starts the program and turns what
//! the agent says on its own into an [`Event`]: an update to a session, or a
//! question only a person can answer, which the desk answers later with
//! [`Agent::respond`].
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
        atomic::{AtomicU64, Ordering},
    },
};

use agent_client_protocol::{
    ByteStreams, Client, JsonRpcRequest, Responder, UntypedMessage, V1, on_receive_notification,
    on_receive_request,
    v1::{
        ConnectionTo,
        schema::{
            CancelNotification, RequestPermissionRequest, RequestPermissionResponse,
            SessionNotification,
        },
    },
};
pub use agent_client_protocol::{Error, ErrorCode};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::Command,
    runtime::Runtime,
    sync::oneshot,
};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tracing::debug;

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
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// Something happened in one of its sessions.
    Update(SessionNotification),
    /// It wants a person to choose before it goes on. Answer with
    /// [`Agent::respond`] under `id`.
    Permission {
        id: u64,
        request: RequestPermissionRequest,
    },
    /// The process `pid` stopped, with the last of what it wrote to its
    /// standard error.
    Exited { pid: Option<u32>, why: String },
    /// A task it set going in the background in the session `session` (a
    /// command left running), as the `JetBrains` extension of ACP says it:
    /// not in the schema.
    Task { session: String, task: Task },
}

/// What the `JetBrains` extension of ACP (`air`, `asyncTasks`) says of a
/// task going on in the background: begun, under way, or over.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "sessionUpdate")]
pub enum Task {
    #[serde(rename = "async_task_spawned")]
    Spawned {
        #[serde(rename = "asyncTaskId")]
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default, rename = "canStop")]
        can_stop: bool,
    },
    #[serde(rename = "async_task_progress")]
    Progress {
        #[serde(rename = "asyncTaskId")]
        id: String,
        #[serde(default)]
        description: Option<String>,
    },
    #[serde(rename = "async_task_state_update")]
    Ended {
        #[serde(rename = "asyncTaskId")]
        id: String,
        /// completed, failed, stopped; anything else is still going.
        state: String,
    },
}

/// A task's update and the session it is in, as the notification has them.
#[derive(serde::Deserialize)]
struct TaskNote {
    #[serde(rename = "sessionId")]
    session: String,
    update: Task,
}

/// How many lines of an agent's standard error are kept to say why it
/// stopped.
const STDERR_KEPT: usize = 20;

/// Questions the agent asked that wait for a person, by the number the desk
/// knows each by.
type Asked = Arc<Mutex<HashMap<u64, Responder<RequestPermissionResponse, V1>>>>;

/// A running agent.
pub struct Agent {
    connection: ConnectionTo<agent_client_protocol::Agent>,
    asked: Asked,
    pid: Option<u32>,
}

impl Agent {
    /// Start the program and connect to it; what it says on its own goes to
    /// `said`, in the order it says it, before the answer that follows it.
    pub async fn start(
        launch: &Launch,
        said: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self, Error> {
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
        let mut child = command.spawn().map_err(|e| {
            Error::internal_error().data(format!(
                "{} could not be started: {e}",
                launch.program.display()
            ))
        })?;
        let pid = child.id();
        let stdin = child.stdin.take().expect("piped stdin").compat_write();
        let stdout = child.stdout.take().expect("piped stdout").compat();

        // The last of what it writes to its standard error, handed over
        // when that closes: it says why the agent stopped. Read as bytes, so
        // that a line that is not UTF-8 does not stop the reading and leave
        // the agent blocked on a full pipe.
        let stderr = child.stderr.take().expect("piped stderr");
        let tail = runtime().spawn(async move {
            let mut kept = VecDeque::new();
            let mut reader = BufReader::new(stderr);
            let mut line = Vec::new();
            while reader
                .read_until(b'\n', &mut line)
                .await
                .is_ok_and(|n| n > 0)
            {
                let text = String::from_utf8_lossy(&line).trim_end().to_owned();
                debug!(target: "ghost_desk::agent", "{text}");
                if kept.len() == STDERR_KEPT {
                    kept.pop_front();
                }
                kept.push_back(text);
                line.clear();
            }
            kept
        });

        let said = Arc::new(said);
        let asked = Asked::default();
        let (connected, connection) = oneshot::channel();
        let (updates, questions, waiting) = (said.clone(), said.clone(), asked.clone());
        runtime().spawn(async move {
            let ticket = AtomicU64::new(1);
            let _ = Client
                .v1()
                .name("ghost")
                .on_receive_notification(
                    async move |n: UntypedMessage, _| {
                        if n.method == "session/update" {
                            update(n.params, &*updates);
                        }
                        Ok(())
                    },
                    on_receive_notification!(),
                )
                .on_receive_request(
                    async move |request: RequestPermissionRequest, responder, _| {
                        let id = ticket.fetch_add(1, Ordering::Relaxed);
                        waiting.lock().expect("asked").insert(id, responder);
                        questions(Event::Permission { id, request });
                        Ok(())
                    },
                    on_receive_request!(),
                )
                .connect_with(ByteStreams::new(stdin, stdout), async move |cx| {
                    let _ = connected.send(cx.clone());
                    // Its output closing ends it, as does the connection
                    // failing some other way.
                    tokio::select! {
                        () = cx.incoming_closed() => {}
                        _ = cx.terminated() => {}
                    }
                    Ok(())
                })
                .await;
            let status = child.wait().await.ok();
            // A program it left behind may hold its standard error open: the
            // reason is waited for a moment, not for ever.
            let tail = tokio::time::timeout(std::time::Duration::from_secs(2), tail).await;
            let mut why = tail
                .ok()
                .and_then(Result::ok)
                .map(Vec::from)
                .unwrap_or_default()
                .join("\n");
            if why.trim().is_empty() {
                why = match status.and_then(|s| s.code()) {
                    Some(code) => format!("it exited with status {code}"),
                    None => "it was stopped".into(),
                };
            }
            said(Event::Exited { pid, why });
        });
        let connection = connection
            .await
            .map_err(|_| Error::internal_error().data("the agent stopped as it started"))?;
        Ok(Self {
            connection,
            asked,
            pid,
        })
    }

    /// Ask `request`, and wait for the answer.
    pub async fn request<R: JsonRpcRequest>(&self, request: R) -> Result<R::Response, Error> {
        self.connection.send_request(request).block_task().await
    }

    /// Stop the turn going on in a session.
    pub fn cancel(&self, cancel: CancelNotification) {
        let _ = self.connection.send_notification(cancel);
    }

    /// Answer the question the agent asked under `id`.
    pub fn respond(&self, id: u64, response: RequestPermissionResponse) {
        if let Some(responder) = self.asked.lock().expect("asked").remove(&id) {
            let _ = responder.respond(response);
        }
    }

    /// Its process's id, where it has one.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Stop it at once, and all it started: when it would not stop a turn.
    pub fn kill(&self) {
        signal(self.pid, libc::SIGKILL);
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // The whole group: an npx and the node it started.
        signal(self.pid, libc::SIGTERM);
    }
}

/// Send `signal` to the process group `pid` leads.
fn signal(pid: Option<u32>, signal: i32) {
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) {
        // SAFETY: kill(2) on a process group the desk started; it touches no
        // memory.
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

/// A `session/update`: one the schema has, or a task in the background,
/// which it does not; any other kind the desk does not know yet is skipped,
/// and the session goes on.
fn update(params: serde_json::Value, said: &dyn Fn(Event)) {
    match serde_json::from_value(params.clone()) {
        Ok(update) => said(Event::Update(update)),
        Err(e) => {
            if let Ok(n) = serde_json::from_value::<TaskNote>(params) {
                said(Event::Task {
                    session: n.session,
                    task: n.update,
                });
            } else {
                debug!("skipped an update the desk does not read: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::v1::schema::NewSessionRequest;
    use serde_json::{Value, json};
    use tokio::sync::mpsc;

    /// A shell script that speaks to the desk the way an agent would; `id`
    /// reads the id of the request on `$line`.
    fn script(body: &str) -> Launch {
        let id = r#"id() { printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }"#;
        Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), format!("{id}\n{body}")],
            env: vec![],
            dir: None,
        }
    }

    fn start(body: &str) -> (Agent, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let agent = runtime()
            .block_on(Agent::start(&script(body), move |e| {
                let _ = tx.send(e);
            }))
            .unwrap();
        (agent, rx)
    }

    #[test]
    fn a_task_in_the_background_is_read_as_the_jetbrains_extension_says_it() {
        let note = |update: Value| -> TaskNote {
            serde_json::from_value(json!({"sessionId": "s1", "update": update})).unwrap()
        };
        let begun = note(
            json!({"sessionUpdate": "async_task_spawned", "asyncTaskId": "b1", "name": "Bash", "description": "make all", "taskType": "local_bash", "showInTranscript": true, "canStop": true, "toolCallId": "t1"}),
        );
        assert_eq!(begun.session, "s1");
        assert_eq!(
            begun.update,
            Task::Spawned {
                id: "b1".into(),
                name: Some("Bash".into()),
                description: Some("make all".into()),
                can_stop: true
            }
        );
        let over = note(
            json!({"sessionUpdate": "async_task_state_update", "asyncTaskId": "b1", "state": "completed", "summary": "done"}),
        );
        assert_eq!(
            over.update,
            Task::Ended {
                id: "b1".into(),
                state: "completed".into()
            }
        );
    }

    #[test]
    fn an_update_said_before_an_answer_arrives_before_it() {
        let (agent, mut rx) = start(
            r#"read line
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1"}}\n' "$(id)"
read line"#,
        );
        let answer = runtime()
            .block_on(agent.request(NewSessionRequest::new("/")))
            .unwrap();
        assert_eq!(&*answer.session_id.0, "s1");
        let event = rx.try_recv().expect("the update, already there");
        assert!(matches!(event, Event::Update(u) if &*u.session_id.0 == "s1"));
    }

    #[test]
    fn a_question_is_answered_later_under_its_id() {
        let (agent, mut rx) = start(
            r#"printf '%s\n' '{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t"},"options":[]}}'
read line
echo "$line" >&2"#,
        );
        let Event::Permission { id, .. } = runtime().block_on(rx.recv()).unwrap() else {
            panic!("expected a question");
        };
        agent.respond(
            id,
            RequestPermissionResponse::new(
                agent_client_protocol::v1::schema::RequestPermissionOutcome::Cancelled,
            ),
        );
        let Event::Exited { why, .. } = runtime().block_on(rx.recv()).unwrap() else {
            panic!("expected the agent to exit");
        };
        assert!(
            why.contains(r#""id":9"#) && why.contains("cancelled"),
            "{why}"
        );
    }

    #[test]
    fn a_request_the_desk_does_not_offer_is_refused_and_an_exit_says_why() {
        let (_agent, mut rx) = start(
            r#"printf '%s\n' '{"jsonrpc":"2.0","id":7,"method":"fs/read_text_file","params":{"sessionId":"s1","path":"/x"}}'
read line
echo "$line" >&2
exit 3"#,
        );
        let Event::Exited { why, .. } = runtime().block_on(rx.recv()).unwrap() else {
            panic!("expected the agent to exit");
        };
        assert!(why.contains("-32601"), "{why}");
    }
}
