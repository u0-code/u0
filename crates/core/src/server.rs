//! The agents, behind one door: each one started is held here by its
//! registry id, and what is asked of it (installing and starting it,
//! sessions opened, listed, forked, deleted; a turn, a mode, an option;
//! signing in and out) runs on the ACP runtime, each answer sent back as an
//! [`Event`]. What an agent says on its own is sent the same way, in order
//! with the answers. The desk names an agent by its id and never holds its
//! connection: what it asks comes in as a [`Call`], to [`Server::handle`].

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use agent_client_protocol::{
    JsonRpcRequest,
    schema::ProtocolVersion,
    v1::schema::{
        AuthenticateRequest, CancelNotification, ClientCapabilities, CloseSessionRequest,
        ContentBlock, DeleteSessionRequest, ForkSessionRequest, Implementation, InitializeRequest,
        ListSessionsRequest, LoadSessionRequest, LogoutRequest, MessageId, NewSessionRequest,
        PromptRequest, RequestPermissionOutcome, RequestPermissionResponse, ResumeSessionRequest,
        SessionConfigId, SessionConfigOptionValue, SessionConfigValueId, SessionId, SessionModeId,
        SetSessionConfigOptionRequest, SetSessionModeRequest, SubagentCapabilities,
    },
};

use crate::{
    acp::{self, Agent, Error},
    git,
    proto::{Call, Event, Here, Opened},
    registry::{self, Entry},
};

/// Where what the server says goes.
type Send = Arc<dyn Fn(Event) + std::marker::Send + Sync>;

/// The agents running, by id.
type Running = Arc<Mutex<BTreeMap<String, Arc<Agent>>>>;

pub struct Server {
    home: PathBuf,
    send: Send,
    agents: Running,
    /// The switch that stops each agent's install going on.
    installs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// How many pages of a project's sessions are read from an agent at most:
/// some page through all they keep, the project's few among them.
const PAGES: usize = 50;

impl Server {
    /// A server for Ghost's home `home`, saying what it has to say to
    /// `send`.
    pub fn new(home: PathBuf, send: impl Fn(Event) + std::marker::Send + Sync + 'static) -> Self {
        Self {
            home,
            send: Arc::new(send),
            agents: Running::default(),
            installs: Mutex::default(),
        }
    }

    /// Do what `call` asks.
    pub fn handle(&self, call: Call) {
        match call {
            Call::Fetch => self.fetch(),
            Call::Install { entry } => self.install(entry),
            Call::CancelInstall { agent } => self.cancel_install(&agent),
            Call::Start { entry, command } => self.start(&entry, command.as_deref()),
            Call::Stop { agent } => self.stop(&agent),
            Call::Kill { agent } => self.kill(&agent),
            Call::Open {
                agent,
                key,
                folder,
                also,
            } => self.open(&agent, key, folder, also),
            Call::List { agent, places } => self.list(&agent, places),
            Call::Reopen {
                agent,
                key,
                session,
                folder,
                also,
                resumed,
            } => self.reopen(&agent, key, session, folder, also, resumed),
            Call::Probe {
                agent,
                dir,
                delete,
                close,
            } => self.probe(&agent, dir, delete, close),
            Call::Fork {
                agent,
                from,
                session,
                folder,
                upto,
            } => self.fork(&agent, from, session, folder, upto),
            Call::Delete {
                agent,
                key,
                session,
            } => self.delete(&agent, key, session),
            Call::Prompt {
                agent,
                key,
                session,
                content,
            } => self.prompt(&agent, key, session, content),
            Call::StopTask {
                agent,
                session,
                task,
            } => self.stop_task(&agent, session, task),
            Call::Cancel { agent, session } => self.cancel(&agent, session),
            Call::Answer {
                agent,
                request,
                outcome,
            } => self.answer(&agent, request, outcome),
            Call::SetMode {
                agent,
                key,
                session,
                mode,
            } => self.set_mode(&agent, key, session, mode),
            Call::SetOption {
                agent,
                key,
                session,
                config,
                value,
            } => self.set_option(&agent, key, session, config, value),
            Call::SignIn { agent, method } => self.authenticate(&agent, method),
            Call::SignOut { agent } => self.logout(&agent),
            Call::Weigh => self.weigh(),
            Call::Here { agents } => self.here(agents),
            Call::CliVersion { cli } => self.cli_version(cli),
        }
    }

    /// The agent `id`, while it runs.
    fn running(&self, id: &str) -> Option<Arc<Agent>> {
        self.agents.lock().expect("agents").get(id).cloned()
    }

    /// Do `work` off the window's thread, and send what it comes to back.
    fn off(&self, work: impl FnOnce() -> Event + std::marker::Send + 'static) {
        let send = self.send.clone();
        acp::runtime().spawn_blocking(move || send(work()));
    }

    /// Ask the agent `id` `request`, and send what `then` makes of the
    /// answer back; one not running answers that it is not.
    fn ask<R>(
        &self,
        id: &str,
        request: R,
        then: impl FnOnce(Result<R::Response, Error>) -> Event + std::marker::Send + 'static,
    ) where
        R: JsonRpcRequest + std::marker::Send + 'static,
        R::Response: std::marker::Send,
    {
        let (agent, send) = (self.running(id), self.send.clone());
        acp::runtime().spawn(async move {
            let result = match agent {
                Some(agent) => agent.request(request).await,
                None => Err(Error::internal_error().data("The agent is not running")),
            };
            send(then(result));
        });
    }

    /// Read the newest registry.
    fn fetch(&self) {
        let home = self.home.clone();
        self.off(move || Event::Registry(registry::fetch(&home)));
    }

    /// Install `entry` from its archive, until it is cancelled.
    fn install(&self, entry: Entry) {
        let stop = Arc::new(AtomicBool::new(false));
        (self.installs.lock().expect("installs")).insert(entry.id.clone(), stop.clone());
        let (home, send) = (self.home.clone(), self.send.clone());
        self.off(move || {
            let agent = entry.id.clone();
            let say = |what: &str, share: Option<f32>| {
                send(Event::Installing {
                    agent: agent.clone(),
                    what: what.to_owned(),
                    share,
                });
            };
            let result = registry::install(&entry, &home, &say, &stop);
            Event::Installed { agent, result }
        });
    }

    /// Stop the install of `agent` going on; nothing of it is kept.
    fn cancel_install(&self, agent: &str) {
        if let Some(stop) = self.installs.lock().expect("installs").get(agent) {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Start `entry`'s program (by `command`, a command line of yours,
    /// where you gave one) and say hello to it. What it says on its own
    /// comes back as [`Event::Said`], in order with the answers.
    fn start(&self, entry: &Entry, command: Option<&str>) {
        let agent = entry.id.clone();
        let launch = match registry::launch_with(entry, &self.home, command) {
            Ok(l) => l,
            Err(m) => {
                (self.send)(Event::Started {
                    agent,
                    result: Err(Error::internal_error().data(m.say(&entry.name))),
                });
                return;
            }
        };
        let (send, agents) = (self.send.clone(), self.agents.clone());
        acp::runtime().spawn(async move {
            let (said, id) = (send.clone(), agent.clone());
            let result = match Agent::start(&launch, move |event| {
                said(Event::Said {
                    agent: id.clone(),
                    event: Box::new(event),
                });
            })
            .await
            {
                Ok(running) => {
                    let hello = InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(
                            ClientCapabilities::new()
                                .subagents(SubagentCapabilities::new())
                                // Tasks left going in the background (a
                                // command), as the JetBrains extension says
                                // them, are shown.
                                .meta(serde_json::Map::from_iter([(
                                    "jetbrains".into(),
                                    serde_json::json!({"air": {"version": 1, "capabilities": ["asyncTasks"]}}),
                                )])),
                        )
                        .client_info(Implementation::new("ghost", env!("CARGO_PKG_VERSION")));
                    running.request(hello).await.map(|init| {
                        let pid = running.pid();
                        let running = Arc::new(running);
                        (agents.lock().expect("agents")).insert(agent.clone(), running);
                        (pid, Box::new(init))
                    })
                }
                Err(e) => Err(e),
            };
            send(Event::Started { agent, result });
        });
    }

    /// Let go of the agent `id`: its process stops once nothing more is
    /// asked of it.
    fn stop(&self, id: &str) {
        self.agents.lock().expect("agents").remove(id);
    }

    /// Stop the agent `id` at once, and all it started: when it would not
    /// stop a turn.
    fn kill(&self, id: &str) {
        if let Some(agent) = self.running(id) {
            agent.kill();
        }
    }

    /// Open a new session of the agent `id` in `folder`, with the folders
    /// `also` beside it.
    fn open(&self, id: &str, key: u64, folder: PathBuf, also: Vec<PathBuf>) {
        let request = NewSessionRequest::new(folder).additional_directories(also);
        self.ask(id, request, move |r| Event::Opened {
            key,
            result: r.map(Box::new),
        });
    }

    /// The sessions the agent `id` keeps in each of `places` (where it
    /// works, and the project that is) and in Ghost's worktrees of them,
    /// each with the project it belongs to.
    fn list(&self, id: &str, places: Vec<(PathBuf, PathBuf)>) {
        let Some(agent) = self.running(id) else {
            return;
        };
        let (name, home, send) = (id.to_owned(), self.home.clone(), self.send.clone());
        acp::runtime().spawn(async move {
            let under = home.join("worktrees");
            let mut all: Vec<(PathBuf, PathBuf)> = Vec::new();
            // Each project's folder (on another machine, its path there), and
            // here, Ghost's worktrees of it.
            for (cwd, project) in places {
                all.push((cwd.clone(), project.clone()));
                if cwd != project {
                    continue;
                }
                let p = project.clone();
                let u = under.clone();
                let worktrees = tokio::task::spawn_blocking(move || git::worktrees(&p, &u))
                    .await
                    .unwrap_or_default();
                all.extend(worktrees.into_iter().map(|w| (w.path, project.clone())));
            }
            let mut found = Vec::new();
            for (cwd, project) in all {
                let mut cursor = None;
                for _ in 0..PAGES {
                    let request = ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor);
                    match agent.request(request).await {
                        Ok(page) => {
                            found.extend(page.sessions.into_iter().map(|s| (project.clone(), s)));
                            cursor = page.next_cursor;
                            if cursor.is_none() {
                                break;
                            }
                        }
                        Err(f) => {
                            send(Event::Listed {
                                agent: name,
                                result: Err(f.message),
                            });
                            return;
                        }
                    }
                }
            }
            // The folders it has sessions in that are not projects (nor
            // Ghost's own), each with how many: what could be added.
            let known: Vec<PathBuf> = found.iter().map(|(_, s)| s.cwd.clone()).collect();
            let mut elsewhere = BTreeMap::<PathBuf, usize>::default();
            let mut cursor = None;
            for _ in 0..PAGES / 5 {
                let request = ListSessionsRequest::new().cursor(cursor);
                let Ok(page) = agent.request(request).await else {
                    break;
                };
                for s in page.sessions {
                    if !known.contains(&s.cwd) && !s.cwd.starts_with(&home) {
                        *elsewhere.entry(s.cwd).or_default() += 1;
                    }
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            send(Event::Listed {
                agent: name.clone(),
                result: Ok(found),
            });
            send(Event::Elsewhere {
                agent: name,
                folders: elsewhere.into_iter().collect(),
            });
        });
    }

    /// Open again the session `session` the agent `id` keeps, in `folder`
    /// (and `also`): resumed, without what was said in it, which the desk
    /// has already; or loaded, and the agent says again all that was said
    /// in it, as updates.
    fn reopen(
        &self,
        id: &str,
        key: u64,
        session: SessionId,
        folder: PathBuf,
        also: Vec<PathBuf>,
        resumed: bool,
    ) {
        let loaded = move |result| Event::Loaded {
            key,
            resumed,
            result,
        };
        if resumed {
            let request = ResumeSessionRequest::new(session, folder).additional_directories(also);
            self.ask(id, request, move |r| {
                loaded(r.map(|r| Opened {
                    modes: r.modes,
                    options: r.config_options,
                }))
            });
        } else {
            let request = LoadSessionRequest::new(session, folder).additional_directories(also);
            self.ask(id, request, move |r| {
                loaded(r.map(|r| Opened {
                    modes: r.modes,
                    options: r.config_options,
                }))
            });
        }
    }

    /// Open a session of the agent `id` in `dir` to read what a session
    /// offers, then delete it (or close it) again, so that nothing of it is
    /// kept.
    fn probe(&self, id: &str, dir: PathBuf, delete: bool, close: bool) {
        let Some(agent) = self.running(id) else {
            return;
        };
        let (name, send) = (id.to_owned(), self.send.clone());
        acp::runtime().spawn(async move {
            let result = agent.request(NewSessionRequest::new(dir)).await;
            if let Ok(r) = &result {
                let id = r.session_id.clone();
                if delete {
                    let _ = agent.request(DeleteSessionRequest::new(id)).await;
                } else if close {
                    let _ = agent.request(CloseSessionRequest::new(id)).await;
                }
            }
            send(Event::Probed {
                agent: name,
                result: result.map(Box::new),
            });
        });
    }

    /// Branch the session `session` off into a new one, in `folder`: with
    /// all that was said in it, or, given `upto` (an item of its thread and
    /// the last message the fork keeps), with what was said before that
    /// item.
    ///
    /// ACP's own fork point is a cursor, which agents announce only on v2.
    /// The adapters of Claude Code and Codex read one from the request's
    /// `_meta` on v1 as well: `jetbrains.air.fork`, the id of the last
    /// message to keep.
    fn fork(
        &self,
        id: &str,
        from: u64,
        session: SessionId,
        folder: PathBuf,
        upto: Option<(usize, MessageId)>,
    ) {
        let mut request = ForkSessionRequest::new(session, folder);
        if let Some((_, last)) = &upto {
            let point = serde_json::json!({"air": {"fork": {"version": 1, "messageId": last}}});
            request = request.meta(serde_json::Map::from_iter([("jetbrains".into(), point)]));
        }
        let at = upto.map(|(at, _)| at);
        self.ask(id, request, move |r| Event::Forked {
            from,
            at,
            result: r.map(Box::new),
        });
    }

    /// Delete the session `session` from the agent `id`.
    fn delete(&self, id: &str, key: u64, session: SessionId) {
        self.ask(id, DeleteSessionRequest::new(session), move |r| {
            Event::Deleted {
                key,
                result: r.map(|_| ()).map_err(|f| f.message),
            }
        });
    }

    /// Say `content` in the session `session`, and wait for the turn to
    /// end.
    fn prompt(&self, id: &str, key: u64, session: SessionId, content: Vec<ContentBlock>) {
        self.ask(id, PromptRequest::new(session, content), move |r| {
            Event::Ended {
                key,
                result: r.map(|r| r.stop_reason),
            }
        });
    }

    /// Stop the task `task` going on in the background in the session
    /// `session`, as the `JetBrains` extension of ACP asks it.
    fn stop_task(&self, id: &str, session: SessionId, task: String) {
        let Some(agent) = self.running(id) else {
            return;
        };
        acp::runtime().spawn(async move {
            let _ = agent
                .request(
                    agent_client_protocol::UntypedMessage::new(
                        "_session/async_task/stop",
                        serde_json::json!({"sessionId": session.0, "asyncTaskId": task}),
                    )
                    .expect("a JSON object"),
                )
                .await;
        });
    }

    /// Stop the turn going on in `session`.
    fn cancel(&self, id: &str, session: SessionId) {
        if let Some(agent) = self.running(id) {
            agent.cancel(CancelNotification::new(session));
        }
    }

    /// Answer the question the agent `id` asked under `request`.
    fn answer(&self, id: &str, request: u64, outcome: RequestPermissionOutcome) {
        if let Some(agent) = self.running(id) {
            agent.respond(request, RequestPermissionResponse::new(outcome));
        }
    }

    /// Put the session in another mode.
    fn set_mode(&self, id: &str, key: u64, session: SessionId, mode: String) {
        let Some(agent) = self.running(id) else {
            return;
        };
        let send = self.send.clone();
        acp::runtime().spawn(async move {
            let request = SetSessionModeRequest::new(session, SessionModeId::new(mode));
            if let Err(f) = agent.request(request).await {
                send(Event::Refused {
                    key,
                    why: f.message,
                });
            }
        });
    }

    /// Choose `value` for the option `config` (a model, a level of
    /// thought).
    fn set_option(&self, id: &str, key: u64, session: SessionId, config: String, value: String) {
        let request = SetSessionConfigOptionRequest::new(
            session,
            SessionConfigId::new(config),
            SessionConfigOptionValue::value_id(SessionConfigValueId::new(value)),
        );
        self.ask(id, request, move |r| match r {
            Ok(r) => Event::Options {
                key,
                options: r.config_options,
            },
            Err(f) => Event::Refused {
                key,
                why: f.message,
            },
        });
    }

    /// Sign in to the agent `id` with its method `method`: the agent itself
    /// does it, often in your browser.
    fn authenticate(&self, id: &str, method: String) {
        let agent = id.to_owned();
        self.ask(id, AuthenticateRequest::new(method), move |r| {
            Event::Signed {
                agent,
                result: r.map(|_| ()).map_err(|f| f.message),
            }
        });
    }

    /// Sign the agent `id` out.
    fn logout(&self, id: &str) {
        let agent = id.to_owned();
        self.ask(id, LogoutRequest::new(), move |r| Event::LoggedOut {
            agent,
            result: r.map(|_| ()).map_err(|f| f.message),
        });
    }

    /// How much memory each agent running holds with what it started (ps),
    /// by its id and process id: in kilobytes.
    fn weigh(&self) {
        let running: Vec<(String, u32)> = (self.agents.lock().expect("agents").iter())
            .filter_map(|(id, a)| Some((id.clone(), a.pid()?)))
            .collect();
        self.off(move || {
            Event::Weighed(
                running
                    .into_iter()
                    .filter_map(|(id, pid)| {
                        let out = std::process::Command::new("ps")
                            .args(["-o", "rss=", "-g", &pid.to_string()])
                            .output()
                            .ok()?;
                        let kb: u64 = String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .filter_map(|l| l.trim().parse::<u64>().ok())
                            .sum();
                        Some((id, pid, kb))
                    })
                    .collect(),
            )
        });
    }

    /// Look at what of each of `agents` is here, in the home and on the
    /// PATH.
    fn here(&self, agents: Vec<Entry>) {
        let home = self.home.clone();
        self.off(move || {
            Event::Here(
                agents
                    .into_iter()
                    .map(|e| {
                        let here = Here {
                            launches: e.launch(&home).is_ok(),
                            installed: e.installed(&home),
                            cli: e.your_cli().map(|(_, cli)| cli),
                        };
                        (e.id, here)
                    })
                    .collect(),
            )
        });
    }

    /// Ask `cli` its version (`--version`, its first line); none, if it
    /// does not say.
    fn cli_version(&self, cli: PathBuf) {
        self.off(move || {
            let version = std::process::Command::new(&cli)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| {
                    let said = String::from_utf8_lossy(&out.stdout);
                    said.lines().next().unwrap_or_default().trim().to_owned()
                })
                .unwrap_or_default();
            Event::CliVersion { cli, version }
        });
    }
}
