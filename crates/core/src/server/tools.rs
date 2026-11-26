//! Ghost's tools: the one MCP server Ghost gives agents (rmcp's), over
//! what the server already does for the desk, reached as `door` says.
//!
//! What only reads (the sessions, what was said in one, a diff, what
//! happened) any caller may call. What acts, or asks you, is for an agent
//! in a session of Ghost's, asked of you first in its thread; one that
//! acts on a session it names (says something in it, stops its turn) is
//! also for an agent outside Ghost where your settings let it, or on a
//! device paired of full scope, asked of you first in that session. Each
//! is asked on the same card an agent's own question is, unless you
//! allowed it always.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use agent_client_protocol::v1::schema::{
    CancelNotification, CreateElicitationRequest, ElicitationAction, ElicitationContentValue,
    ElicitationFormMode, ElicitationSchema, ElicitationSessionScope, PermissionOption,
    PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest, SessionId,
    StringPropertySchema, ToolCallUpdate, ToolCallUpdateFields,
};
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

mod devices;

use super::{Shared, door::Caller};
use crate::{
    access::Scope,
    acp::{self, Answer},
    git,
    proto::Event,
    repo::{self, GitAction},
    settings::Settings,
    store::Meta,
    tasks::{Schedule, Task},
};

impl Shared {
    /// The session `id` of `agent`, as the index keeps it.
    pub(super) fn meta(&self, agent: &str, id: &SessionId) -> Option<Meta> {
        (self.index.lock().expect("index").as_ref().ok())?
            .sessions
            .iter()
            .find(|m| m.agent == agent && m.id.as_ref() == Some(id))
            .cloned()
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
pub(super) struct Tools {
    shared: Shared,
    /// The key it came in with.
    key: String,
    /// The tools you allowed always to it.
    always: Arc<Mutex<HashSet<&'static str>>>,
}

/// Why a tool that acts, or asks you, is not for an agent outside Ghost.
const OUTSIDE: &str = "Only an agent in a session of Ghost's can do this: Ghost asks you first there. Start one in Ghost.";

/// Why a tool that acts is not for this caller: it may only read.
const READS: &str = "This agent may only read here: let agents outside Ghost act on a session in Settings › Agents, or start one in Ghost.";

impl Tools {
    /// The tools for the caller `key` lets in; none where it lets no one.
    pub(super) fn of(shared: &Shared, key: &str) -> Option<Self> {
        let always = (shared.callers.lock().expect("callers"))
            .get(key)?
            .always
            .clone();
        Some(Self {
            shared: shared.clone(),
            key: key.to_owned(),
            always,
        })
    }

    /// The session it calls from, and how the index keeps it; none for an
    /// agent outside Ghost.
    fn session(&self) -> Option<(SessionId, Meta)> {
        let caller = self.caller()?;
        let Caller::Session(agent, Some(id)) = caller else {
            return None;
        };
        let meta = self.shared.meta(&agent, &id)?;
        Some((id, meta))
    }

    /// Who it is, while its key lets it in.
    fn caller(&self) -> Option<Caller> {
        (self.shared.callers.lock().expect("callers"))
            .get(&self.key)
            .map(|c| c.who.clone())
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
        self.ask(tool, title, input, &meta.agent, id).await?;
        Ok(meta)
    }

    /// Ask you whether `tool` may do what `title` says to the session
    /// `on` of `agent`: in the session it calls from, for one in a
    /// session of Ghost's; for one outside Ghost where your settings let
    /// it act, in the session it acts on. That session, as kept.
    async fn allowed_on(
        &self,
        tool: &'static str,
        title: String,
        input: &impl Serialize,
        agent: &str,
        on: &SessionId,
    ) -> Result<Meta, String> {
        let meta = (self.shared.meta(agent, on)).ok_or("Ghost keeps no such session")?;
        match self.caller() {
            Some(Caller::Session(..)) => self.allowed(tool, title, input).await.map(|_| meta),
            Some(Caller::Outside) if Settings::load(&self.shared.home).0.outside_acts => {
                let title = format!("An agent outside Ghost: {title}");
                self.ask(tool, title, input, agent, on.clone()).await?;
                Ok(meta)
            }
            Some(Caller::Device(Scope::Full)) => {
                let title = format!("An agent on another device: {title}");
                self.ask(tool, title, input, agent, on.clone()).await?;
                Ok(meta)
            }
            _ => Err(READS.into()),
        }
    }

    /// Ask you, on a permission card in the session `id` of `agent`,
    /// whether `tool` may do what `title` says with `input`.
    async fn ask(
        &self,
        tool: &'static str,
        title: String,
        input: &impl Serialize,
        agent: &str,
        id: SessionId,
    ) -> Result<(), String> {
        if self.always.lock().expect("always").contains(tool) {
            return Ok(());
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
            .ask_you(agent, |n| acp::Event::Permission {
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
                Ok(())
            }
            "once" => Ok(()),
            _ => Err("You did not allow it".into()),
        }
    }

    /// Refuse a session's own: it would wait on the turn it calls from.
    fn not_mine(&self, agent: &str, id: &SessionId) -> Result<(), String> {
        match self.caller() {
            Some(Caller::Session(a, Some(own))) if a == agent && own == *id => {
                Err("That is this session: name another".into())
            }
            _ => Ok(()),
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

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct SendTo {
    /// The session's agent, by its registry id (as `list_sessions` says).
    agent: String,
    /// The session's id.
    session: String,
    /// What to say in it, as its next turn: one going on is let end
    /// first, never stopped.
    prompt: String,
    /// Wait for its answer, and give it back.
    #[serde(default)]
    wait: bool,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Which {
    /// The session's agent, by its registry id.
    agent: String,
    /// The session's id.
    session: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct WaitFor {
    agent: String,
    session: String,
    /// The most to wait, in seconds (an hour at most); none, ten minutes.
    seconds: Option<u64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Since {
    /// What happened after this number, as the last call said; none, all
    /// that is kept.
    #[serde(default)]
    after: u64,
    /// How long to wait, in seconds (an hour at most), for something to
    /// happen; none, not at all.
    #[serde(default)]
    seconds: u64,
}

/// The longest a tool waits.
const MOST: std::time::Duration = std::time::Duration::from_hours(1);

/// `seconds`, or ten minutes, within [`MOST`].
fn waited(seconds: Option<u64>) -> std::time::Duration {
    std::time::Duration::from_secs(seconds.unwrap_or(600)).min(MOST)
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
        let diffs = tokio::task::spawn_blocking(move || git::uncommitted(&folder, None, false))
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
            workflow: None,
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
            workflow: None,
            session: None,
            next: None,
            last: None,
            runs: 0,
        });
        Ok(format!("“{}” is scheduled.", ask.title))
    }

    /// Say something in a session Ghost keeps, as its next turn (one going
    /// on is let end first, never stopped); with `wait`, give back what
    /// its agent answered. You are asked first.
    #[tool]
    async fn send_to_session(&self, Parameters(ask): Parameters<SendTo>) -> Result<String, String> {
        let id = SessionId::new(ask.session.clone());
        self.not_mine(&ask.agent, &id)?;
        let shown: String = ask.prompt.chars().take(80).collect();
        let meta = self
            .allowed_on(
                "send_to_session",
                format!("Say “{shown}”"),
                &ask,
                &ask.agent,
                &id,
            )
            .await?;
        let task = Task {
            id: 0,
            title: meta.title,
            prompt: ask.prompt,
            agent: ask.agent.clone(),
            model: None,
            mode: None,
            project: meta.project,
            schedule: Schedule::Every(0),
            paused: false,
            same: true,
            worktree: false,
            workflow: None,
            session: Some(id.clone()),
            next: None,
            last: None,
            runs: 0,
        };
        let shared = self.shared.clone();
        let turn = async move {
            if !(shared.happened).idle(&task.agent, &id, MOST).await {
                return Err("Its turn went on for an hour: nothing was said".to_owned());
            }
            shared
                .turn_with(&task, |_| {})
                .await
                .map_err(|(_, why)| why)?;
            Ok(shared.said_in(&task.agent, &id, true).await)
        };
        if ask.wait {
            turn.await
        } else {
            acp::runtime().spawn(turn);
            Ok("Said, as its next turn: wait_for_turn gives its answer.".into())
        }
    }

    /// Wait until the turn going on in a session ends (at once where none
    /// does), and give back what its agent said in its last turn.
    #[tool]
    async fn wait_for_turn(&self, Parameters(ask): Parameters<WaitFor>) -> Result<String, String> {
        let id = SessionId::new(ask.session);
        self.not_mine(&ask.agent, &id)?;
        if !(self.shared.happened)
            .idle(&ask.agent, &id, waited(ask.seconds))
            .await
        {
            return Err("Its turn still goes on".into());
        }
        Ok(self.shared.said_in(&ask.agent, &id, true).await)
    }

    /// Stop the turn going on in a session. You are asked first.
    #[tool]
    async fn interrupt_session(
        &self,
        Parameters(ask): Parameters<Which>,
    ) -> Result<String, String> {
        let id = SessionId::new(ask.session.clone());
        self.not_mine(&ask.agent, &id)?;
        let meta = self
            .allowed_on(
                "interrupt_session",
                "Stop its turn".into(),
                &ask,
                &ask.agent,
                &id,
            )
            .await?;
        match self.shared.running(&ask.agent) {
            Some(agent) if self.shared.happened.busy(&ask.agent, &id) => {
                agent.cancel(CancelNotification::new(id));
                Ok(format!("The turn in “{}” is stopped.", meta.title))
            }
            _ => Ok(format!("No turn goes on in “{}”.", meta.title)),
        }
    }

    /// What happened in the sessions Ghost runs after a number (a turn
    /// ended, an agent asks the user), the latest last, each numbered:
    /// call again with the last number to follow on; with `seconds`,
    /// waiting that long for something to. `missed` says some of what
    /// happened since is no longer kept.
    #[tool]
    async fn events(&self, Parameters(ask): Parameters<Since>) -> Result<String, String> {
        let most = std::time::Duration::from_secs(ask.seconds).min(MOST);
        let (happened, missed) = self.shared.happened.after(ask.after, most).await;
        let said = serde_json::json!({"happened": happened, "missed": missed});
        serde_json::to_string_pretty(&said).map_err(|e| e.to_string())
    }

    /// Ask the user a question in this session's thread, and wait for the
    /// answer.
    #[tool]
    async fn ask_user(&self, Parameters(ask): Parameters<AskUser>) -> Result<String, String> {
        let (id, meta) = self.session().ok_or(OUTSIDE)?;
        let field = StringPropertySchema::new()
            .title("Your answer".to_owned())
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
    router = Self::every(),
    instructions = "Ghost, the app this agent may be running in: the sessions it keeps of your agents, their diffs and pull requests, its scheduled tasks, the simulators and emulators where it runs; and the user, asked in this session's thread."
)]
impl ServerHandler for Tools {
    /// Each call counted by its tool (`telemetry`), then made.
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        (self.shared.telemetry).count(|c| crate::telemetry::tally(&mut c.tools, &request.name));
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        Self::every().call(call).await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, Write as _},
        time::Duration,
    };

    use agent_client_protocol::v1::schema::SelectedPermissionOutcome;
    use serde_json::{Value, json};

    use super::*;
    use crate::{mcp, proto::Call, registry::Entry, server::tests::server};

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

    /// What the tools answer over HTTP at the door of `home` to `message`,
    /// with `key` as the bearer token: the status line and the body.
    fn over_http(home: &std::path::Path, key: &str, message: &Value) -> (String, String) {
        use std::io::Read as _;
        let door = door(home);
        let mut s = std::net::TcpStream::connect(("127.0.0.1", door.http)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let body = message.to_string();
        write!(
            s,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-06-18\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            door.http,
            body.len()
        )
        .unwrap();
        let mut said = String::new();
        let _ = s.read_to_string(&mut said);
        let (head, body) = said.split_once("\r\n\r\n").unwrap_or((&said, ""));
        (
            head.lines().next().unwrap_or_default().to_owned(),
            body.to_owned(),
        )
    }

    #[test]
    fn the_tools_answer_streamable_http_on_this_mac_by_a_key_alone() {
        let home = tempfile::tempdir().unwrap();
        let (server, _said) = server(home.path());
        server.handle(Call::Keep(vec![meta(home.path(), "Fix the build")]));
        let key = door(home.path()).key;
        let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "stand-in", "version": "1"}}});
        let (status, body) = over_http(home.path(), &key, &hello);
        assert!(status.contains("200"), "{status} {body}");
        assert!(body.contains("\"ghost\""), "{body}");
        let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "list_sessions", "arguments": {}}});
        let (_, body) = over_http(home.path(), &key, &list);
        assert!(body.contains("Fix the build"), "{body}");
        let send = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "send_to_session",
            "arguments": {"agent": "a", "session": "s1", "prompt": "Go"}}});
        let (_, body) = over_http(home.path(), &key, &send);
        assert!(body.contains("may only read"), "{body}");
        let (status, _) = over_http(home.path(), "not a key", &list);
        assert!(status.contains("401"), "{status}");
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
                "device_screen",
                "events",
                "interrupt_session",
                "list_devices",
                "list_sessions",
                "open_pr",
                "read_diff",
                "read_session",
                "schedule_task",
                "send_to_session",
                "use_device",
                "wait_for_turn"
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
        // An agent that opens `s1`, `s2`, …, writing down what it was asked
        // to open each with, and answers a prompt in `s2`.
        let agent = home.path().join("agent.sh");
        let heard = home.path().join("heard");
        let opened = |n: usize| -> Value {
            let text = std::fs::read_to_string(&heard).unwrap();
            serde_json::from_str(text.lines().nth(n).unwrap()).unwrap()
        };
        let script = format!(
            r#"id() {{ printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }}
n=0
while read line; do case "$line" in
*'"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1}}}}\n' "$(id)" ;;
*'"session/new"'*) n=$((n+1)); echo "$line" >> {heard}
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
        let asked: Value = opened(0);
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
        // Switched off for the agent, its sessions are given nothing.
        std::fs::write(
            home.path().join("settings.json"),
            json!({"enabled": [], "folders": [], "no_tools": ["a"]}).to_string(),
        )
        .unwrap();
        server.handle(Call::Open {
            agent: "a".into(),
            key: 2,
            folder: home.path().to_path_buf(),
            also: Vec::new(),
        });
        while !matches!(next(), Event::Opened { key: 2, .. }) {}
        assert_eq!(opened(2)["params"]["mcpServers"], json!([]));
    }

    #[test]
    fn an_agent_outside_ghost_acts_on_a_session_only_where_let_and_as_you_allow_it_there() {
        let home = tempfile::tempdir().unwrap();
        // An agent that opens s1 and answers each prompt in the session it
        // was said in.
        let agent = home.path().join("agent.sh");
        let script = r#"id() { printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }
while read line; do case "$line" in
*'"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$(id)" ;;
*'"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1"}}\n' "$(id)" ;;
*'"session/prompt"'*) printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Carried on."}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)" ;;
esac; done"#;
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
        server.handle(Call::Keep(vec![meta(home.path(), "Build")]));
        let send = json!({"agent": "a", "session": "s1", "prompt": "Go on", "wait": true});
        let mut client = Client::new(home.path(), None);
        // Not let: it reads only, and nothing is asked of you.
        client.call("send_to_session", &send);
        assert_eq!(client.result(), (READS.to_owned(), true));
        client.call("interrupt_session", &json!({"agent": "a", "session": "s1"}));
        assert_eq!(client.result(), (READS.to_owned(), true));
        // Let: asked of you in the session it acts on, and turned down.
        let settings = json!({"enabled": [], "folders": [], "outside_acts": true});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let card = |next: &dyn Fn() -> Event| loop {
            if let Event::Said { event, .. } = next()
                && let acp::Event::Permission { id, request } = *event
            {
                break (id, request);
            }
        };
        let answer = |request, chosen: &str| Call::Answer {
            agent: "a".into(),
            request,
            answer: Answer::Permission(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new(chosen.to_owned()),
            )),
        };
        client.call("send_to_session", &send);
        let (request, asked) = card(&next);
        assert_eq!(asked.session_id, SessionId::new("s1"));
        let title = asked.tool_call.fields.title.unwrap_or_default();
        assert!(
            title.starts_with("An agent outside Ghost: Say “Go on”"),
            "{title}"
        );
        server.handle(answer(request, "no"));
        assert_eq!(client.result(), ("You did not allow it".to_owned(), true));
        // Allowed, it is said as the session's next turn, and answered.
        client.call("send_to_session", &send);
        let (request, _) = card(&next);
        server.handle(answer(request, "once"));
        assert_eq!(client.result(), ("Carried on.".to_owned(), false));
        client.call(
            "wait_for_turn",
            &json!({"agent": "a", "session": "s1", "seconds": 5}),
        );
        assert_eq!(client.result(), ("Carried on.".to_owned(), false));
        client.call("events", &json!({"after": 0}));
        let (happened, failed) = client.result();
        let happened: Value = serde_json::from_str(&happened).unwrap();
        assert!(!failed);
        let ends = happened["happened"].as_array().unwrap();
        assert!(
            ends.iter()
                .any(|m| m["what"] == json!({"turn_ended": "end_turn"})
                    && m["session"] == json!("s1"))
        );
    }
}
