//! Ghost's tools: the one MCP server Ghost gives agents (rmcp's), over
//! what the server already does for the desk. It listens on 127.0.0.1
//! (`mcp`); each session Ghost opens or opens again is given it, through
//! ACP's `mcpServers`, with a key of its own that says which session
//! calls; an agent outside Ghost comes in with the key in `mcp.json`.
//!
//! What only reads (the sessions, what was said in one, a diff) any of them
//! may call. What acts (a session handed a task, a pull request opened, a
//! task scheduled) and what asks you is for a session of Ghost's alone:
//! each asks you first on the same card an agent's own question is, in that
//! session's thread, unless you allowed it always there.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use agent_client_protocol::v1::schema::{
    CreateElicitationRequest, ElicitationAction, ElicitationContentValue, ElicitationFormMode,
    ElicitationSchema, ElicitationSessionScope, EnvVariable, McpServer, McpServerStdio,
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, StringPropertySchema, ToolCallUpdate, ToolCallUpdateFields,
};
use rmcp::{
    ServerHandler, ServiceExt as _, handler::server::wrapper::Parameters, schemars, tool,
    tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader};

use super::Shared;
use crate::{
    acp::{self, Answer},
    git, mcp,
    proto::Event,
    repo::{self, GitAction},
    settings::Settings,
    store::Meta,
    tasks::{Schedule, Task},
};

/// Who each key lets in: an agent outside Ghost, or one in a session of
/// Ghost's (its agent, and its id once it has one).
pub(super) type Callers = Arc<Mutex<HashMap<String, Option<(String, Option<SessionId>)>>>>;

impl Shared {
    /// Listen for agents calling Ghost's tools, from now on: the door
    /// written in the home, each one let in by its key served on a task
    /// of its own.
    pub(super) fn serve_tools(&self) {
        let this = self.clone();
        acp::runtime().spawn(async move {
            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("Ghost's tools are not served: {e}");
                    return;
                }
            };
            let door = mcp::Door {
                port: listener.local_addr().map_or(0, |a| a.port()),
                key: crate::access::random::<32>(),
            };
            if let Err(e) = mcp::open(&this.home, &door) {
                tracing::warn!("Ghost's tools are not served: {e}");
                return;
            }
            (this.callers.lock().expect("callers")).insert(door.key, None);
            while let Ok((stream, _)) = listener.accept().await {
                let this = this.clone();
                tokio::spawn(async move {
                    let mut stream = BufReader::new(stream);
                    let mut key = String::new();
                    // A key is a line, and short.
                    let read = (&mut stream).take(256).read_line(&mut key).await;
                    let key = key.trim_end().to_owned();
                    if read.is_err() || !this.callers.lock().expect("callers").contains_key(&key) {
                        return;
                    }
                    let tools = Tools {
                        shared: this,
                        key,
                        always: Arc::default(),
                    };
                    if let Ok(serving) = tools.serve(stream).await {
                        let _ = serving.waiting().await;
                    }
                });
            }
        });
    }

    /// Ghost's tools, for a session of `agent` (its id where it has one):
    /// the key it calls with, and the server to give it as it is opened.
    pub(super) fn tools(&self, agent: &str, id: Option<SessionId>) -> (String, Vec<McpServer>) {
        let key = crate::access::random::<32>();
        (self.callers.lock().expect("callers")).insert(key.clone(), Some((agent.to_owned(), id)));
        let program = std::env::current_exe().unwrap_or_else(|_| "ghost".into());
        let server = McpServerStdio::new("ghost", program)
            .args(vec!["--mcp".into()])
            .env(vec![
                EnvVariable::new("GHOST_HOME", self.home.display().to_string()),
                EnvVariable::new(mcp::KEY, key.clone()),
            ]);
        (key, vec![McpServer::Stdio(server)])
    }

    /// The session opened with the key `key` is `id`.
    pub(super) fn bind(&self, key: &str, id: SessionId) {
        if let Some(Some((_, at))) = self.callers.lock().expect("callers").get_mut(key) {
            *at = Some(id);
        }
    }

    /// Ask you `ask` (made with the number it is asked under) in the
    /// session of `agent` it names, as the agent's own questions are; your
    /// answer, or none where the agent stopped first.
    async fn ask_you(&self, agent: &str, ask: impl FnOnce(u64) -> acp::Event) -> Option<Answer> {
        let (n, answer) = self.running(agent)?.question();
        (self.send)(Event::Said {
            agent: agent.to_owned(),
            event: Box::new(ask(n)),
        });
        answer.await.ok()
    }
}

/// One agent's connection to Ghost's tools.
#[derive(Clone)]
struct Tools {
    shared: Shared,
    /// The key it came in with.
    key: String,
    /// The tools you allowed always in its session.
    always: Arc<Mutex<HashSet<&'static str>>>,
}

/// Why a tool that acts, or asks you, is not for an agent outside Ghost.
const OUTSIDE: &str = "Only an agent in a session of Ghost's can do this: Ghost asks you first there. Start one in Ghost.";

impl Tools {
    /// The session it calls from, and how the index keeps it; none for an
    /// agent outside Ghost.
    fn session(&self) -> Option<(SessionId, Meta)> {
        let caller = self
            .shared
            .callers
            .lock()
            .expect("callers")
            .get(&self.key)?
            .clone();
        let (agent, id) = caller?;
        let id = id?;
        let meta = (self.shared.index.lock().expect("index").as_ref().ok())?
            .sessions
            .iter()
            .find(|m| m.agent == agent && m.id.as_ref() == Some(&id))?
            .clone();
        Some((id, meta))
    }

    /// Ask you whether `tool` may do what `title` says, with `input`, in
    /// the session it calls from: yes where you allowed it always there.
    async fn allowed(
        &self,
        tool: &'static str,
        title: String,
        input: &impl Serialize,
    ) -> Result<Meta, String> {
        let (id, meta) = self.session().ok_or(OUTSIDE)?;
        if self.always.lock().expect("always").contains(tool) {
            return Ok(meta);
        }
        let options = vec![
            PermissionOption::new("once", "Allow", PermissionOptionKind::AllowOnce),
            PermissionOption::new("always", "Always allow", PermissionOptionKind::AllowAlways),
            PermissionOption::new("no", "Reject", PermissionOptionKind::RejectOnce),
        ];
        let call = ToolCallUpdate::new(
            format!("ghost-{tool}-{}", crate::access::number()),
            ToolCallUpdateFields::new()
                .title(title)
                .raw_input(serde_json::to_value(input).ok()),
        );
        let answer = self
            .shared
            .ask_you(&meta.agent, |n| acp::Event::Permission {
                id: n,
                request: RequestPermissionRequest::new(id, call, options),
            })
            .await;
        let Some(Answer::Permission(RequestPermissionOutcome::Selected(chosen))) = answer else {
            return Err("You did not allow it".into());
        };
        match &*chosen.option_id.0 {
            "always" => {
                self.always.lock().expect("always").insert(tool);
                Ok(meta)
            }
            "once" => Ok(meta),
            _ => Err("You did not allow it".into()),
        }
    }

    /// `folder`, or where the session it calls from works.
    fn folder(&self, folder: Option<PathBuf>) -> Result<PathBuf, String> {
        folder
            .or_else(|| self.session().map(|(_, m)| m.folder))
            .ok_or_else(|| "Name the folder".into())
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ListSessions {
    /// Only the sessions of this project (its folder).
    project: Option<PathBuf>,
}

#[derive(Serialize)]
struct Listed {
    agent: String,
    id: Option<SessionId>,
    title: String,
    project: PathBuf,
    folder: PathBuf,
    archived: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ReadSession {
    /// The session's agent, by its registry id (as `list_sessions` says).
    agent: String,
    /// The session's id.
    session: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ReadDiff {
    /// The folder; none, where this session works.
    folder: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Delegate {
    /// What to ask of it: all it needs to know, as it starts afresh.
    prompt: String,
    /// The agent to hand it to, by its registry id; none, this one.
    agent: Option<String>,
    /// The project it works in (its folder); none, this session's.
    project: Option<PathBuf>,
    /// It works in a worktree of its own.
    #[serde(default)]
    worktree: bool,
    /// The new session's title.
    title: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct OpenPr {
    title: String,
    body: String,
    /// The message of the commit of what is not committed yet; none, the
    /// title.
    message: Option<String>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ScheduleTask {
    /// What is said to the agent each time.
    prompt: String,
    title: String,
    /// Once, this many minutes from now.
    in_minutes: Option<u64>,
    /// Every this many minutes, from now on.
    every_minutes: Option<u32>,
    /// The agent it is said to, by its registry id; none, this one.
    agent: Option<String>,
    /// The project it works in (its folder); none, this session's.
    project: Option<PathBuf>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AskUser {
    question: String,
    /// The answers to choose from; none, the user writes one.
    #[serde(default)]
    choices: Vec<String>,
}

#[tool_router]
impl Tools {
    /// The sessions Ghost keeps, the latest first: each one's agent, id,
    /// title, project and the folder it works in.
    #[tool]
    async fn list_sessions(
        &self,
        Parameters(ask): Parameters<ListSessions>,
    ) -> Result<String, String> {
        let index = self.shared.index.lock().expect("index").clone()?;
        let mut sessions: Vec<&Meta> = (index.sessions.iter())
            .filter(|m| ask.project.as_ref().is_none_or(|p| &m.project == p))
            .collect();
        sessions.sort_by_key(|m| std::cmp::Reverse(m.updated));
        let listed: Vec<Listed> = (sessions.into_iter().take(100))
            .map(|m| Listed {
                agent: m.agent.clone(),
                id: m.id.clone(),
                title: m.title.clone(),
                project: m.project.clone(),
                folder: m.folder.clone(),
                archived: m.flags.archived,
            })
            .collect();
        serde_json::to_string_pretty(&listed).map_err(|e| e.to_string())
    }

    /// What was said in a session: what the user said (led by "You:")
    /// and what its agent answered.
    #[tool]
    async fn read_session(&self, Parameters(ask): Parameters<ReadSession>) -> String {
        let said = (self.shared)
            .said_in(&ask.agent, &SessionId::new(ask.session), false)
            .await;
        if said.is_empty() {
            "Nothing was said in it, or Ghost does not keep it.".into()
        } else {
            said
        }
    }

    /// What is not committed in a folder, as a unified diff.
    #[tool]
    async fn read_diff(&self, Parameters(ask): Parameters<ReadDiff>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let diffs = tokio::task::spawn_blocking(move || git::uncommitted(&folder, false))
            .await
            .map_err(|e| e.to_string())??;
        Ok(if diffs.is_empty() {
            "Nothing is uncommitted.".into()
        } else {
            diffs.into_iter().map(|d| d.patch).collect()
        })
    }

    /// Hand a task to an agent in a new session of Ghost's, which you can
    /// watch there, and wait for its answer: its session's id and what it
    /// said at the end.
    #[tool]
    async fn delegate_task(&self, Parameters(ask): Parameters<Delegate>) -> Result<String, String> {
        let meta = self
            .allowed("delegate_task", format!("Hand off “{}”", ask.title), &ask)
            .await?;
        let agent = ask.agent.unwrap_or(meta.agent);
        let task = Task {
            id: 0,
            title: ask.title,
            prompt: ask.prompt,
            agent: agent.clone(),
            model: None,
            mode: None,
            project: ask.project.unwrap_or(meta.project),
            schedule: Schedule::Every(0),
            paused: false,
            same: false,
            worktree: ask.worktree,
            session: None,
            next: None,
            last: None,
            runs: 0,
        };
        let id = self
            .shared
            .turn_with(&task, |_| {})
            .await
            .map_err(|(_, why)| why)?;
        let said = self.shared.said_in(&agent, &id, true).await;
        Ok(format!("Session {} of {agent} answered:\n\n{said}", id.0))
    }

    /// Commit what is not committed where this session works, push it
    /// (on a branch of its own, where it is on the default one) and open a
    /// pull request with `gh`.
    #[tool]
    async fn open_pr(&self, Parameters(ask): Parameters<OpenPr>) -> Result<String, String> {
        let meta = self
            .allowed(
                "open_pr",
                format!("Open the pull request “{}”", ask.title),
                &ask,
            )
            .await?;
        let home = self.shared.home.clone();
        let done = tokio::task::spawn_blocking(move || {
            let prefix = Settings::load(&home).0.branch_prefix;
            let action = GitAction::PullRequest {
                message: ask.message.unwrap_or_else(|| ask.title.clone()),
                title: ask.title,
                body: ask.body,
            };
            repo::git_action(&meta.folder, action, &prefix, &[], 0, &|_| {})
        })
        .await
        .map_err(|e| e.to_string())?;
        match done {
            Event::Git { result, .. } => result,
            _ => Err("No pull request was opened".into()),
        }
    }

    /// Schedule a prompt to be said to an agent, in a new session each
    /// time: once, some minutes from now, or every so many minutes. Ghost
    /// lists it with the scheduled tasks, where it can be paused or taken
    /// away.
    #[tool]
    async fn schedule_task(
        &self,
        Parameters(ask): Parameters<ScheduleTask>,
    ) -> Result<String, String> {
        let now = crate::store::secs(std::time::SystemTime::now());
        let schedule = match (ask.in_minutes, ask.every_minutes) {
            (Some(m), None) => Schedule::Once(now + m * 60),
            (None, Some(m)) if m >= crate::tasks::LEAST_MINUTES => Schedule::Every(m),
            _ => return Err("Give in_minutes or every_minutes (1 or more), not both".into()),
        };
        let meta = self
            .allowed("schedule_task", format!("Schedule “{}”", ask.title), &ask)
            .await?;
        self.shared.set_task(Task {
            id: 0,
            title: ask.title.clone(),
            prompt: ask.prompt,
            agent: ask.agent.unwrap_or(meta.agent),
            model: None,
            mode: None,
            project: ask.project.unwrap_or(meta.project),
            schedule,
            paused: false,
            same: false,
            worktree: false,
            session: None,
            next: None,
            last: None,
            runs: 0,
        });
        Ok(format!("“{}” is scheduled.", ask.title))
    }

    /// Ask the user a question in this session's thread, and wait for the
    /// answer.
    #[tool]
    async fn ask_user(&self, Parameters(ask): Parameters<AskUser>) -> Result<String, String> {
        let (id, meta) = self.session().ok_or(OUTSIDE)?;
        let field = StringPropertySchema::new()
            .enum_values((!ask.choices.is_empty()).then_some(ask.choices));
        let form = ElicitationFormMode::new(
            ElicitationSessionScope::new(id),
            ElicitationSchema::new().property("answer", field, true),
        );
        let answer = self
            .shared
            .ask_you(&meta.agent, |n| acp::Event::Elicit {
                id: n,
                request: CreateElicitationRequest::new(form, ask.question),
            })
            .await;
        match answer {
            Some(Answer::Form(r)) => match r.action {
                ElicitationAction::Accept(a) => {
                    match a.content.and_then(|mut c| c.remove("answer")) {
                        Some(ElicitationContentValue::String(said)) => Ok(said),
                        _ => Err("The user gave no answer".into()),
                    }
                }
                _ => Err("The user would not answer".into()),
            },
            _ => Err("The user was not asked".into()),
        }
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "rmcp's tool_handler writes the methods it makes async"
)]
#[tool_handler(
    name = "ghost",
    instructions = "Ghost, the app this agent may be running in: the sessions it keeps of your agents, their diffs and pull requests, its scheduled tasks; and the user, asked in this session's thread."
)]
impl ServerHandler for Tools {}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, Write as _},
        time::Duration,
    };

    use agent_client_protocol::v1::schema::SelectedPermissionOutcome;
    use serde_json::{Value, json};

    use super::*;
    use crate::{proto::Call, registry::Entry, server::tests::server};

    /// A stand-in for an MCP client: JSON-RPC lines through the door, after
    /// the key it came with.
    struct Client {
        lines: std::io::BufReader<std::net::TcpStream>,
        out: std::net::TcpStream,
        asked: u64,
    }

    /// The door of the server of `home`, once it is written.
    fn door(home: &std::path::Path) -> mcp::Door {
        for _ in 0..250 {
            if let Ok(text) = std::fs::read_to_string(mcp::path(home)) {
                return serde_json::from_str(&text).unwrap();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no door");
    }

    impl Client {
        /// In at the door of the server of `home` with `key` (none, the
        /// door's own: an agent outside Ghost), and past MCP's hello.
        fn new(home: &std::path::Path, key: Option<&str>) -> Self {
            let door = door(home);
            let mut out = std::net::TcpStream::connect(("127.0.0.1", door.port)).unwrap();
            out.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            writeln!(out, "{}", key.unwrap_or(&door.key)).unwrap();
            let lines = std::io::BufReader::new(out.try_clone().unwrap());
            let mut client = Self {
                lines,
                out,
                asked: 0,
            };
            let hello = json!({"protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "stand-in", "version": "1"}});
            client.ask("initialize", &hello);
            client.reply();
            client.say(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
            client
        }

        fn say(&mut self, message: &Value) {
            writeln!(self.out, "{message}").unwrap();
        }

        fn ask(&mut self, method: &str, params: &Value) {
            self.asked += 1;
            let id = self.asked;
            self.say(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        }

        /// The answer to what was asked last.
        fn reply(&mut self) -> Value {
            loop {
                let mut line = String::new();
                self.lines.read_line(&mut line).unwrap();
                let said: Value = serde_json::from_str(&line).unwrap();
                if said["id"] == json!(self.asked) {
                    return said;
                }
            }
        }

        fn call(&mut self, tool: &str, arguments: &Value) {
            self.ask("tools/call", &json!({"name": tool, "arguments": arguments}));
        }

        /// What the tool called last said, and whether as a failure.
        fn result(&mut self) -> (String, bool) {
            let said = self.reply();
            let result = &said["result"];
            let text = result["content"][0]["text"].as_str().unwrap_or_default();
            (text.to_owned(), result["isError"] == json!(true))
        }
    }

    fn meta(folder: &std::path::Path, title: &str) -> Meta {
        serde_json::from_value(json!({
            "agent": "a", "id": "s1", "project": folder, "folder": folder,
            "title": title, "begun": 5, "updated": 7,
        }))
        .unwrap()
    }

    #[test]
    fn an_agent_outside_ghost_reads_the_sessions_kept_but_may_not_act() {
        let home = tempfile::tempdir().unwrap();
        let (server, _said) = server(home.path());
        server.handle(Call::Keep(vec![meta(home.path(), "Fix the build")]));
        let mut client = Client::new(home.path(), None);
        client.ask("tools/list", &json!({}));
        let listed = client.reply();
        let mut names: Vec<&str> = (listed["result"]["tools"].as_array().unwrap().iter())
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "ask_user",
                "delegate_task",
                "list_sessions",
                "open_pr",
                "read_diff",
                "read_session",
                "schedule_task"
            ]
        );
        client.call("list_sessions", &json!({}));
        let (sessions, failed) = client.result();
        assert!(!failed && sessions.contains("Fix the build"), "{sessions}");
        client.call(
            "delegate_task",
            &json!({"prompt": "Do it", "title": "It", "agent": "a", "project": "/p"}),
        );
        assert_eq!(client.result(), (OUTSIDE.to_owned(), true));
        // A key it was not given lets nothing in.
        let mut stranger =
            std::net::TcpStream::connect(("127.0.0.1", door(home.path()).port)).unwrap();
        stranger
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        writeln!(stranger, "not a key").unwrap();
        let mut said = String::new();
        let read = std::io::BufReader::new(stranger).read_line(&mut said);
        assert!(matches!(read, Ok(0)), "{said}");
    }

    #[test]
    fn a_session_is_given_the_tools_with_a_key_of_its_own_and_what_acts_asks_you_first() {
        let home = tempfile::tempdir().unwrap();
        // An agent that opens `s1`, then `s2`, writing down what it was
        // asked to open the first with, and answers a prompt in `s2`.
        let agent = home.path().join("agent.sh");
        let heard = home.path().join("heard");
        let script = format!(
            r#"id() {{ printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }}
n=0
while read line; do case "$line" in
*'"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1}}}}\n' "$(id)" ;;
*'"session/new"'*) n=$((n+1)); [ $n = 1 ] && echo "$line" > {heard}
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"s%s"}}}}\n' "$(id)" $n ;;
*'"session/prompt"'*) printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s2","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"It is done."}}}}}}}}'
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$(id)" ;;
esac; done"#,
            heard = heard.display()
        );
        std::fs::write(&agent, script).unwrap();
        let (server, said) = server(home.path());
        let next = || said.recv_timeout(Duration::from_secs(10)).unwrap();
        let entry: Entry = serde_json::from_value(json!({
            "id": "a", "name": "A", "version": "1", "distribution": {},
        }))
        .unwrap();
        server.handle(Call::Start {
            entry,
            command: Some(format!("/bin/sh {}", agent.display())),
            env: std::collections::BTreeMap::new(),
        });
        assert!(matches!(next(), Event::Started { result: Ok(_), .. }));
        server.handle(Call::Open {
            agent: "a".into(),
            key: 1,
            folder: home.path().to_path_buf(),
            also: Vec::new(),
        });
        assert!(matches!(next(), Event::Opened { result: Ok(_), .. }));
        server.handle(Call::Keep(vec![meta(home.path(), "t")]));
        // It was given Ghost's tools: this program, started with --mcp and
        // a key of its own.
        let asked: Value = serde_json::from_str(&std::fs::read_to_string(&heard).unwrap()).unwrap();
        let given = &asked["params"]["mcpServers"][0];
        assert_eq!(
            (&given["name"], &given["args"]),
            (&json!("ghost"), &json!(["--mcp"]))
        );
        let key = (given["env"].as_array().unwrap().iter())
            .find(|v| v["name"] == mcp::KEY)
            .and_then(|v| v["value"].as_str())
            .unwrap()
            .to_owned();
        let mut client = Client::new(home.path(), Some(&key));
        // What acts asks first, in the session's thread, and does nothing
        // when you turn it down.
        client.call("open_pr", &json!({"title": "Fix", "body": "It is fixed."}));
        let (request, asked) = loop {
            if let Event::Said { event, .. } = next()
                && let acp::Event::Permission { id, request } = *event
            {
                break (id, request);
            }
        };
        assert_eq!(asked.session_id, SessionId::new("s1"));
        assert_eq!(
            asked.tool_call.fields.title.as_deref(),
            Some("Open the pull request “Fix”")
        );
        server.handle(Call::Answer {
            agent: "a".into(),
            request,
            answer: Answer::Permission(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("no"),
            )),
        });
        assert_eq!(client.result(), ("You did not allow it".to_owned(), true));
        // A question is asked in its thread as a form, and answered there.
        client.call(
            "ask_user",
            &json!({"question": "Which database?", "choices": ["pg", "sqlite"]}),
        );
        let request = loop {
            if let Event::Said { event, .. } = next()
                && let acp::Event::Elicit { id, request } = *event
            {
                assert_eq!(request.message, "Which database?");
                break id;
            }
        };
        let answer =
            serde_json::from_value(json!({"action": "accept", "content": {"answer": "pg"}}))
                .unwrap();
        server.handle(Call::Answer {
            agent: "a".into(),
            request,
            answer: Answer::Form(answer),
        });
        assert_eq!(client.result(), ("pg".to_owned(), false));
        // A task handed off, once allowed, is a session of its own, kept
        // with the rest; its answer comes back.
        client.call(
            "delegate_task",
            &json!({"prompt": "Tidy the tests", "title": "Tidy"}),
        );
        let request = loop {
            if let Event::Said { event, .. } = next()
                && let acp::Event::Permission { id, .. } = *event
            {
                break id;
            }
        };
        server.handle(Call::Answer {
            agent: "a".into(),
            request,
            answer: Answer::Permission(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("once"),
            )),
        });
        assert_eq!(
            client.result(),
            ("Session s2 of a answered:\n\nIt is done.".to_owned(), false)
        );
        let index = server.shared.index.lock().unwrap().clone().unwrap();
        assert!((index.sessions.iter()).any(|m| m.title == "Tidy"));
    }
}
