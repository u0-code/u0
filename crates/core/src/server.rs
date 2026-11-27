//! The agents, behind one door: each one started is held here by its
//! registry id, and what is asked of it (installing and starting it,
//! sessions opened, listed, forked, deleted; a turn, a mode, an option;
//! signing in and out) runs on the ACP runtime, each answer sent back as an
//! [`Event`]. What an agent says on its own is sent the same way, in order
//! with the answers. The desk names an agent by its id and never holds its
//! connection: what it asks comes in as a [`Call`], to [`Server::handle`].
//!
//! The server is also the one writer of what u0 keeps of sessions
//! (`store`): the index, changed by the metas the desk sends, and each
//! session's record (`records`): what an agent says in a session open with
//! it is written here and sent on as [`Event::Appended`].

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use agent_client_protocol::{
    JsonRpcRequest,
    schema::ProtocolVersion,
    v1::schema::{
        AuthenticateRequest, CancelNotification, ClientCapabilities, ClientSessionCapabilities,
        CloseSessionRequest, CompactionCapabilities, ContentBlock, ContentChunk,
        DeleteSessionRequest, ElicitationCapabilities, ElicitationFormCapabilities,
        ElicitationUrlCapabilities, ForkSessionRequest, Implementation, InitializeRequest,
        InitializeResponse, ListSessionsRequest, LoadSessionRequest, LogoutRequest, MessageId,
        NewSessionRequest, NoticeCapabilities, PromptRequest, ResumeSessionRequest,
        SessionConfigId, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigOptionValue, SessionConfigValueId, SessionId, SessionModeId, SessionUpdate,
        SetSessionConfigOptionRequest, SetSessionModeRequest, SubagentCapabilities,
    },
};

use crate::{
    access::Access,
    acp::{self, Agent, Error},
    git,
    link::Outbox,
    proto::{Call, Event, Here, Opened},
    records::Records,
    registry::{self, Entry},
    repo,
    session::Session,
    share::Sharing,
    store::{self, Index, Job, Meta},
    terminals::Terminals,
    tidy, writer,
};

mod door;
mod flows;
mod happened;
mod scheduled;
mod tools;

/// Where what the server says goes.
type Send = Arc<dyn Fn(Event) + std::marker::Send + Sync>;

/// The agents running, by id.
type Running = Arc<Mutex<BTreeMap<String, Arc<Agent>>>>;

/// The sessions a writer writes in, each with what it has said so far:
/// what is said there is the answer, not a thread.
type Writing = Arc<Mutex<HashMap<SessionId, String>>>;

pub struct Server {
    shared: Shared,
    /// The switch that stops each agent's install going on.
    installs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// The switch that stops the clone going on.
    cloning: Mutex<Arc<AtomicBool>>,
    terminals: Terminals,
    /// What it said, numbered and kept for desks linked from elsewhere
    /// (`link`), and who may link.
    outbox: Arc<Outbox>,
    access: Access,
    sharing: Mutex<Sharing>,
}

/// What the server holds that work it hands off holds too: a task's run,
/// an agent starting.
#[derive(Clone)]
struct Shared {
    home: PathBuf,
    send: Send,
    agents: Running,
    /// The index of the sessions kept, or why it does not read: then
    /// nothing is saved over it.
    index: Arc<Mutex<Result<Index, String>>>,
    records: Arc<Records>,
    writing: Writing,
    /// The turns going on, and what happened in sessions (`happened`).
    happened: Arc<happened::Happenings>,
    /// The scheduled tasks, or why they do not read.
    tasks: Arc<Mutex<Result<crate::tasks::Tasks, String>>>,
    /// The workflows' runs, or why they do not read.
    runs: Arc<Mutex<Result<crate::workflows::Runs, String>>>,
    /// What each watched pull request last stood at, by its session's
    /// agent and id (`scheduled::Shared::watch_prs`).
    watched: Arc<Mutex<HashMap<(String, SessionId), String>>>,
    /// Who each key lets in to u0's tools (`tools`).
    callers: door::Callers,
    /// What is counted, and sent as far as you let it.
    telemetry: Arc<crate::telemetry::Telemetry>,
}

impl Shared {
    /// Change the index by `change` and write it, unless it did not read.
    fn change_index(&self, change: impl FnOnce(&mut Index)) {
        let mut index = self.index.lock().expect("index");
        if let Ok(index) = &mut *index {
            change(index);
            store::queue(Job::Write(
                store::index_path(&self.home),
                store::bytes(index),
            ));
        }
    }

    /// The agent `id`, while it runs.
    fn running(&self, id: &str) -> Option<Arc<Agent>> {
        self.agents.lock().expect("agents").get(id).cloned()
    }

    /// Start the agent `agent` by `launch`, with the variables `env` (as
    /// in the settings' `env`), and say hello to it; it is held as running
    /// from then on. What it says on its own comes back as
    /// [`Event::Said`], in order with the answers.
    async fn launch(
        &self,
        agent: &str,
        mut launch: acp::Launch,
        env: BTreeMap<String, Option<String>>,
    ) -> Result<(Arc<Agent>, Started), Error> {
        crate::policy::allows(crate::policy::Limit::Agent(agent))
            .map_err(|why| Error::internal_error().data(why))?;
        let of = agent.to_owned();
        // Read from the Keychain, or 1Password, which may ask you first.
        let vars = tokio::task::spawn_blocking(move || crate::secrets::env(&of, &env)).await;
        launch.env.extend(
            vars.expect("the variables read")
                .map_err(|why| Error::internal_error().data(why))?,
        );
        let (said, id, this) = (self.send.clone(), agent.to_owned(), self.clone());
        let running = Agent::start(&launch, move |event| {
            if let acp::Event::Permission { id: asked, request } = &event {
                let asks = happened::Happened::Asks {
                    title: (request.tool_call.fields.title.clone()).unwrap_or_default(),
                    request: *asked,
                    options: request.options.clone(),
                };
                this.happened.happened(&id, &request.session_id, asks);
            }
            if let Some(event) = heard(&this.records, &this.agents, &this.writing, &id, event) {
                said(event);
            }
        })
        .await?;
        let hello = InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(
                ClientCapabilities::new()
                    .subagents(SubagentCapabilities::new())
                    // What it asks for is a form filled in over the
                    // composer, or a page opened.
                    .elicitation(
                        ElicitationCapabilities::new()
                            .form(ElicitationFormCapabilities::new())
                            .url(ElicitationUrlCapabilities::new()),
                    )
                    // Notices, and the context compacted, are said in the
                    // thread.
                    .session(
                        ClientSessionCapabilities::new()
                            .notices(NoticeCapabilities::new())
                            .compaction(CompactionCapabilities::new()),
                    )
                    // Tasks left going in the background (a command), as
                    // the JetBrains extension says them, are shown.
                    .meta(serde_json::Map::from_iter([(
                        "jetbrains".into(),
                        serde_json::json!({"air": {"version": 1, "capabilities": ["asyncTasks"]}}),
                    )])),
            )
            .client_info(Implementation::new("u0", env!("CARGO_PKG_VERSION")));
        let init = running.request(hello).await?;
        let pid = running.pid();
        let running = Arc::new(running);
        (self.agents.lock().expect("agents")).insert(agent.to_owned(), running.clone());
        Ok((running, (pid, Box::new(init))))
    }
}

/// What an agent started says of itself: its process, and its hello.
type Started = (Option<u32>, Box<InitializeResponse>);

/// Choose the model `model` in the session `id` of `agent`, by the option
/// of its `options` that chooses one; with none, or no such option, its
/// own stays.
async fn choose_model(
    agent: &Agent,
    id: &SessionId,
    options: Option<Vec<SessionConfigOption>>,
    model: Option<String>,
) -> Result<(), Error> {
    let option = (options.unwrap_or_default().into_iter())
        .find(|o| o.category == Some(SessionConfigOptionCategory::Model));
    match model.zip(option) {
        Some((model, option)) => agent
            .request(SetSessionConfigOptionRequest::new(
                id.clone(),
                option.id,
                SessionConfigOptionValue::value_id(SessionConfigValueId::new(model)),
            ))
            .await
            .map(|_| ()),
        None => Ok(()),
    }
}

/// How many pages of a project's sessions are read from an agent at most:
/// some page through all they keep, the project's few among them.
const PAGES: usize = 50;

impl Server {
    /// A server for u0's home `home`, saying what it has to say to
    /// `send`, shared as it was left.
    pub fn new(
        home: PathBuf,
        send: impl Fn(Event) + std::marker::Send + Sync + 'static,
    ) -> Arc<Self> {
        let index = store::load(&home);
        if let Err(why) = &index {
            tracing::warn!("{why}");
        }
        let tasks = crate::tasks::load(&home);
        if let Err(why) = &tasks {
            tracing::warn!("{why}");
        }
        let runs = crate::workflows::load_runs(&home);
        if let Err(why) = &runs {
            tracing::warn!("{why}");
        }
        let outbox = Arc::new(Outbox::default());
        let kept = outbox.clone();
        // The tests send nowhere, and nor does a build made by hand.
        let endpoint = crate::telemetry::endpoint(crate::update::BUILT_AS).filter(|_| !cfg!(test));
        let telemetry = crate::telemetry::Telemetry::new(&home, endpoint);
        let counted = telemetry.clone();
        let server = Arc::new(Self {
            sharing: Mutex::default(),
            access: Access::new(&home),
            outbox,
            shared: Shared {
                records: Arc::new(Records::new(home.clone())),
                writing: Writing::default(),
                home,
                send: Arc::new(move |event| {
                    kept.push(&event);
                    counted.event(&event);
                    send(event);
                }),
                agents: Running::default(),
                index: Arc::new(Mutex::new(index)),
                happened: Arc::default(),
                tasks: Arc::new(Mutex::new(tasks)),
                runs: Arc::new(Mutex::new(runs)),
                watched: Arc::default(),
                callers: door::Callers::default(),
                telemetry,
            },
            installs: Mutex::default(),
            cloning: Mutex::default(),
            terminals: Terminals::default(),
        });
        server.share_as_kept(false);
        server.shared.schedule();
        server.shared.go_on();
        server.shared.serve_tools();
        server.shared.telemetry.start();
        server
    }

    pub(crate) fn sharing(&self) -> std::sync::MutexGuard<'_, Sharing> {
        self.sharing.lock().expect("sharing")
    }

    pub(crate) fn say(&self, event: Event) {
        (self.shared.send)(event);
    }

    pub fn home(&self) -> &std::path::Path {
        &self.shared.home
    }

    pub fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    /// The settings in this server's home, or why they do not read.
    pub fn settings(&self) -> Result<Box<crate::settings::Settings>, String> {
        match crate::settings::Settings::load(&self.shared.home) {
            (settings, None) => Ok(Box::new(settings)),
            (_, Some(why)) => Err(why),
        }
    }

    /// Wait until what was asked to be written is written: as u0 quits.
    pub fn settle() {
        store::settle();
    }

    /// Do what `call` asks.
    pub fn handle(self: &Arc<Self>, call: Call) {
        self.shared.telemetry.call(&call);
        match call {
            Call::Fetch => self.fetch(),
            Call::Install { entry } => self.install(entry),
            Call::CancelInstall { agent } => self.cancel_install(&agent),
            Call::Start {
                entry,
                command,
                env,
            } => self.start(&entry, command.as_deref(), &env),
            Call::Secret { account, value } => self.off(move || {
                Event::Notice(match crate::secrets::set(&account, value.as_deref()) {
                    Ok(()) if value.is_some() => Ok("Kept in the Keychain".into()),
                    Ok(()) => Ok("Taken out of the Keychain".into()),
                    Err(e) => Err(("The Keychain refused it".into(), e)),
                })
            }),
            Call::Stop { agent } => self.stop(&agent),
            Call::Kill { agent } => self.kill(&agent),
            Call::Open {
                agent,
                key,
                folder,
                also,
            } => self.open(&agent, key, folder, also),
            Call::List {
                agent,
                places,
                under,
            } => self.list(&agent, places, under),
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
            Call::Write(order) => self.write(order),
            Call::StopTask {
                agent,
                session,
                task,
            } => self.stop_task(&agent, session, task),
            Call::Cancel { agent, session } => self.cancel(&agent, session),
            Call::Answer {
                agent,
                request,
                answer,
            } => self.answer(&agent, request, answer),
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
            Call::Telemetry(ask) => self.shared.telemetry.ask(ask, &*self.shared.send),
            Call::Signal { pid, signal } => self.off(move || {
                Event::Notice(
                    crate::diagnostics::signal(std::process::id(), pid, &signal)
                        .map_err(|e| (format!("Couldn't stop {pid}"), e)),
                )
            }),
            Call::Troubles => {
                let home = self.shared.home.clone();
                self.off(move || Event::Troubles(crate::diagnostics::troubles(&home, 200)));
            }
            Call::Here { agents, commands } => self.here(agents, commands),
            Call::Uninstall { entry } => {
                let (home, said) = (self.shared.home.clone(), self.shared.send.clone());
                self.off(move || match entry.uninstall(&home) {
                    Ok(()) => Event::Here([(entry.id.clone(), here(&entry, &home, None))].into()),
                    Err(why) => {
                        said(Event::Here(
                            [(entry.id.clone(), here(&entry, &home, None))].into(),
                        ));
                        Event::Notice(Err((format!("Couldn't uninstall {}", entry.name), why)))
                    }
                });
            }
            Call::CliVersion { cli } => self.cli_version(cli),
            Call::Watch => {
                let index = self.shared.index.lock().expect("index").clone();
                (self.shared.send)(Event::Index(index));
                self.say_tasks();
            }
            Call::SaveSettings(settings) => settings.save(&self.shared.home),
            Call::Share(share) => self.share(share),
            Call::SetTask(task) => self.shared.set_task(task),
            Call::DropTask(id) => self.drop_task(id),
            Call::RunTask(id) => self.run_task(id),
            Call::RotateHook(id) => self.rotate_hook(id),
            Call::Say {
                tag,
                agent,
                session,
                project,
                prompt,
            } => self.shared.say_in(tag, agent, session, project, prompt),
            Call::Workflows => self.say_flows(),
            Call::StartFlow { workflow, input } => self.start_flow(workflow, input),
            Call::AnswerFlow { run, choice } => self.answer_flow(run, choice),
            Call::ResumeFlow(run) => self.resume_flow(run),
            Call::CancelFlow(run) => self.cancel_flow(run),
            Call::NewToolsKey => self.shared.new_outside_key(),
            Call::Keep(metas) => self.shared.change_index(|index| {
                for meta in metas {
                    match index.sessions.iter_mut().find(|m| same(m, &meta)) {
                        Some(m) => *m = meta,
                        None => index.sessions.push(meta),
                    }
                }
            }),
            Call::Forget {
                agent,
                id,
                keep_out,
            } => {
                self.shared.change_index(|index| {
                    index
                        .sessions
                        .retain(|m| m.agent != agent || m.id.as_ref() != Some(&id));
                    if keep_out {
                        index.gone.push((agent.clone(), id.0.to_string()));
                    }
                });
                self.shared.records.forget(&agent, &id);
            }
            Call::Stash(stashed) => self.shared.change_index(|index| index.stashed = stashed),
            Call::Append { agent, id, events } => self.shared.records.append(&agent, &id, events),
            Call::DropRecord { agent, id } => {
                store::queue(Job::Remove(self.shared.records.path(&agent, &id)));
            }
            Call::Read { key, agent, id } => {
                let (path, send) = (
                    self.shared.records.path(&agent, &id),
                    self.shared.send.clone(),
                );
                store::queue(Job::Then(Box::new(move || {
                    send(Event::Record {
                        key,
                        events: store::read(&path),
                    });
                })));
            }
            Call::CopyRecord {
                agent,
                from,
                to,
                yours,
            } => {
                let (from, to) = (
                    self.shared.records.path(&agent, &from),
                    self.shared.records.path(&agent, &to),
                );
                // After what is being written to it; and before anything
                // else is written to the branch.
                store::queue(Job::Then(Box::new(move || {
                    let events =
                        Session::record_before(store::read(&from).unwrap_or_default(), yours);
                    if let Err(e) = store::append(&to, &events) {
                        tracing::warn!("a branch's record could not be written: {e}");
                    }
                })));
            }
            Call::Used { records } => {
                let home = self.shared.home.clone();
                self.off(move || {
                    Event::Used(
                        (records.into_iter())
                            .map(|(key, agent, id)| {
                                let events = store::read(&store::record_path(&home, &agent, &id.0));
                                (key, crate::usage::used(&events.unwrap_or_default()))
                            })
                            .filter(|(_, used)| !used.is_empty())
                            .collect(),
                    )
                });
            }
            Call::Search { query, records } => {
                let home = self.shared.home.clone();
                self.off(move || Event::Found {
                    keys: (records.into_iter())
                        .filter(|(_, agent, id)| store::holds(&home, agent, &id.0, &query))
                        .map(|(key, ..)| key)
                        .collect(),
                    query,
                });
            }
            Call::Look { key, folder } => self.off(move || repo::look(key, folder)),
            Call::SaveScript {
                dir,
                at,
                script,
                key,
            } => self.off(move || repo::save_script(dir, at, script, key)),
            Call::ReadProject { folder } => self.off(move || repo::read_project(folder)),
            Call::Uncommitted {
                dir,
                key,
                since,
                no_spacing,
            } => self.off(move || Event::Uncommitted {
                key,
                result: git::uncommitted(&dir, since.as_deref(), no_spacing),
            }),
            Call::ScopeDiff {
                dir,
                key,
                scope,
                base,
                no_spacing,
            } => self.off(move || repo::scope_diff(&dir, key, scope, base.as_deref(), no_spacing)),
            Call::Commits { dir, key } => self.off(move || Event::Commits {
                key,
                commits: git::log(&dir, 12),
                stashes: git::stashes(&dir),
                branches: git::branches(&dir),
                upstream: git::upstream(&dir),
            }),
            Call::ReadPr { dir, key } => self.off(move || Event::Pr {
                key,
                pr: git::read_pr(&dir).map(Box::new),
            }),
            Call::PrText { dir, number } => self.off(move || Event::PrText {
                text: git::pull_request_text(&dir, number),
                number,
            }),
            Call::ListPrs { folder } => self.off(move || Event::Prs {
                prs: git::open_on_github(&folder, "pr"),
                issues: git::open_on_github(&folder, "issue"),
                folder,
            }),
            Call::Folders { dir } => self.off(move || Event::Folders {
                names: repo::folders(&dir),
                dir,
            }),
            Call::MakeFolder { dir } => {
                let send = self.shared.send.clone();
                acp::runtime().spawn_blocking(move || {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        let what = format!("Couldn't make the folder {}", dir.display());
                        send(Event::Notice(Err((what, e.to_string()))));
                    }
                });
            }
            Call::ListFiles { folder } => self.off(move || Event::Files {
                files: git::files(&folder),
                folder,
            }),
            Call::ReadFile { path } => self.off(move || repo::read_file(path)),
            Call::Forge {
                folder,
                kind,
                ask,
                linear,
            } => self.off(move || Event::Forge {
                answer: crate::forge::answer(&folder, kind, ask, linear.as_deref()),

                folder,
                kind,
            }),
            Call::Grep { folder, words, how } => self.off(move || Event::Grepped {
                found: git::grep(&folder, &words, how),
                words,
            }),
            Call::Create { dir, name, publish } => self.off(move || Event::Cloned {
                result: git::create(&dir, &name).and_then(|()| {
                    if publish {
                        git::publish(&dir, false).map(|_| ())
                    } else {
                        Ok(())
                    }
                }),
                dir,
            }),
            Call::Clone { url, dir } => {
                let stop = Arc::new(AtomicBool::new(false));
                *self.cloning.lock().expect("cloning") = stop.clone();
                let send = self.shared.send.clone();
                self.off(move || repo::clone(&url, dir, &stop, &*send));
            }
            Call::CancelClone => {
                let stop = self.cloning.lock().expect("cloning");
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Call::Refresh { projects } => {
                acp::runtime().spawn_blocking(move || {
                    for (dir, pull) in projects {
                        if let Err(e) = git::refresh(&dir, pull) {
                            tracing::warn!("{}: {e}", dir.display());
                        }
                    }
                });
            }
            Call::MakeWorktree {
                dir,
                about,
                under,
                prefix,
                from,
                submodules,
                key,
            } => self.off(move || {
                repo::make_worktree(&dir, &about, &under, &prefix, from, submodules, key)
            }),
            Call::Checkpoint {
                dir,
                name,
                key,
                turn,
                after,
                before,
                first,
                then,
                no_spacing,
            } => self.off(move || {
                repo::checkpoint(
                    &dir,
                    &name,
                    key,
                    turn,
                    after,
                    (before, first),
                    then,
                    no_spacing,
                )
            }),
            Call::Restore {
                dir,
                target,
                name,
                first,
                key,
                turn,
                undo,
                no_spacing,
            } => self.off(move || {
                repo::restore(&dir, &target, &name, first, key, turn, undo, no_spacing)
            }),
            Call::Git {
                dir,
                action,
                prefix,
                left_out,
                key,
            } => {
                let send = self.shared.send.clone();
                self.off(move || repo::git_action(&dir, action, &prefix, &left_out, key, &*send));
            }
            Call::DropCheckpoints { dir, uid } => {
                acp::runtime().spawn_blocking(move || git::drop_checkpoints(&dir, &uid));
            }
            Call::Place { key, calls } => self.off(move || repo::place(key, calls)),
            Call::Sizes { worktrees } => {
                let home = self.shared.home.clone();
                self.off(move || Event::Sizes(tidy::sizes(&home, &worktrees)));
            }
            Call::Tidy { what, kept } => {
                let (home, send) = (self.shared.home.clone(), self.shared.send.clone());
                self.off(move || {
                    send(Event::Tidied(tidy::tidy(&home, what, &kept)));
                    Event::Sizes(tidy::sizes(&home, &kept.under))
                });
            }
            Call::Forges => self.off(|| Event::Forges(crate::forge::seen())),
            Call::Devices => self.off(crate::devices::listed),
            Call::Device { device, act } => {
                let send = self.shared.send.clone();
                self.off(move || {
                    if let Err(why) = crate::devices::act(&device, &act) {
                        send(Event::Notice(Err((
                            format!("{} did not do that", device.name),
                            why,
                        ))));
                    }
                    crate::devices::listed()
                });
            }
            Call::InstallAxe => {
                let send = self.shared.send.clone();
                self.off(move || {
                    let done = which::which("brew")
                        .map_err(|_| "Homebrew is not installed: see brew.sh".to_owned())
                        .and_then(|brew| crate::devices::install_axe(&brew));
                    send(Event::Notice(match done {
                        Ok(()) => Ok("AXe is installed".into()),
                        Err(why) => Err(("AXe was not installed".into(), why)),
                    }));
                    crate::devices::listed()
                });
            }
            Call::Screen { device } => self.off(move || Event::Screen {
                picture: crate::devices::screen(&device),
                id: device.id,
            }),
            Call::TermOpen {
                id,
                dir,
                shell,
                first,
                size,
            } => (self.terminals).open(id, (dir, shell), first, size, self.shared.send.clone()),
            Call::TermInput { id, bytes } => self.terminals.input(id, &bytes),
            Call::TermResize { id, size } => self.terminals.resize(id, size),
            Call::TermClose { id, force } => {
                let busy = self.terminals.close(id, force);
                (self.shared.send)(Event::TermClosed { id, busy });
            }
        }
    }

    /// The agent `id`, while it runs.
    fn running(&self, id: &str) -> Option<Arc<Agent>> {
        self.shared.running(id)
    }

    /// Do `work` off the window's thread, and send what it comes to back.
    fn off(&self, work: impl FnOnce() -> Event + std::marker::Send + 'static) {
        let send = self.shared.send.clone();
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
        self.ask_after(id, || {}, request, then);
    }

    /// As `ask`, once `before` is done, off the window's thread.
    fn ask_after<R>(
        &self,
        id: &str,
        before: impl FnOnce() + std::marker::Send + 'static,
        request: R,
        then: impl FnOnce(Result<R::Response, Error>) -> Event + std::marker::Send + 'static,
    ) where
        R: JsonRpcRequest + std::marker::Send + 'static,
        R::Response: std::marker::Send,
    {
        let (agent, send) = (self.running(id), self.shared.send.clone());
        acp::runtime().spawn(async move {
            let _ = tokio::task::spawn_blocking(before).await;
            let result = match agent {
                Some(agent) => agent.request(request).await,
                None => Err(Error::internal_error().data("The agent is not running")),
            };
            send(then(result));
        });
    }

    /// Read the newest registry.
    fn fetch(&self) {
        let home = self.shared.home.clone();
        self.off(move || Event::Registry(registry::fetch(&home)));
    }

    /// Install `entry` from its archive, until it is cancelled.
    fn install(&self, entry: Entry) {
        let stop = Arc::new(AtomicBool::new(false));
        (self.installs.lock().expect("installs")).insert(entry.id.clone(), stop.clone());
        let (home, send) = (self.shared.home.clone(), self.shared.send.clone());
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
    fn start(&self, entry: &Entry, command: Option<&str>, env: &BTreeMap<String, Option<String>>) {
        let agent = entry.id.clone();
        let launch = match registry::launch_with(entry, &self.shared.home, command) {
            Ok(l) => l,
            Err(m) => {
                (self.shared.send)(Event::Started {
                    agent,
                    result: Err(Error::internal_error().data(m.say(&entry.name))),
                });
                return;
            }
        };
        let shared = self.shared.clone();
        let env = env.clone();
        acp::runtime().spawn(async move {
            let result = shared
                .launch(&agent, launch, env)
                .await
                .map(|(_, said)| said);
            (shared.send)(Event::Started { agent, result });
        });
    }

    /// Let go of the agent `id`: its process stops once nothing more is
    /// asked of it.
    fn stop(&self, id: &str) {
        self.shared.agents.lock().expect("agents").remove(id);
        self.shared.records.close(id);
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
        let (tools, given) = self.shared.tools(id, None);
        let request = NewSessionRequest::new(folder)
            .additional_directories(also)
            .mcp_servers(given);
        let (shared, agent) = (self.shared.clone(), id.to_owned());
        self.ask(id, request, move |r| {
            if let Ok(r) = &r {
                shared.records.open(&agent, r.session_id.clone());
                shared.bind(&tools, r.session_id.clone());
            }
            Event::Opened {
                key,
                result: r.map(Box::new),
            }
        });
    }

    /// The sessions the agent `id` keeps in each of the projects `places`
    /// and in u0's worktrees of them, those in `under`, each with the
    /// project it belongs to.
    fn list(&self, id: &str, places: Vec<PathBuf>, under: Vec<PathBuf>) {
        let Some(agent) = self.running(id) else {
            return;
        };
        let (name, home, send) = (
            id.to_owned(),
            self.shared.home.clone(),
            self.shared.send.clone(),
        );
        acp::runtime().spawn(async move {
            let mut all: Vec<(PathBuf, PathBuf)> = Vec::new();
            // Each project's folder, and u0's worktrees of it.
            for project in places {
                all.push((project.clone(), project.clone()));
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
            // u0's own), each with how many: what could be added.
            let known: Vec<PathBuf> = found.iter().map(|(_, s)| s.cwd.clone()).collect();
            let mut elsewhere = BTreeMap::<PathBuf, usize>::default();
            let mut cursor = None;
            for _ in 0..PAGES / 5 {
                let request = ListSessionsRequest::new().cursor(cursor);
                let Ok(page) = agent.request(request).await else {
                    break;
                };
                for s in page.sessions {
                    let ours =
                        s.cwd.starts_with(&home) || under.iter().any(|u| s.cwd.starts_with(u));
                    if !known.contains(&s.cwd) && !ours {
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
        // What it says from now on is written: on a load, all that was
        // said in it.
        self.shared.records.open(id, session.clone());
        let (_, given) = self.shared.tools(id, Some(session.clone()));
        let loaded = move |result| Event::Loaded {
            key,
            resumed,
            result,
        };
        // A worktree tidied away while the session was archived is
        // checked out again first.
        let gone = match &*self.shared.index.lock().expect("index") {
            Ok(index) => (index.sessions.iter())
                .find(|m| m.agent == id && m.id.as_ref() == Some(&session))
                .and_then(|m| m.worktree.clone())
                .filter(|w| !w.path.exists()),
            Err(_) => None,
        };
        let before = move || {
            if let Some(wt) = gone
                && let Err(why) = git::restore_worktree(&wt)
            {
                tracing::warn!("{} was not checked out again: {why}", wt.path.display());
            }
        };
        if resumed {
            let request = ResumeSessionRequest::new(session, folder)
                .additional_directories(also)
                .mcp_servers(given);
            self.ask_after(id, before, request, move |r| {
                loaded(r.map(|r| Opened {
                    modes: r.modes,
                    options: r.config_options,
                }))
            });
        } else {
            let request = LoadSessionRequest::new(session, folder)
                .additional_directories(also)
                .mcp_servers(given);
            self.ask_after(id, before, request, move |r| {
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
        let (name, send) = (id.to_owned(), self.shared.send.clone());
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
        let (tools, given) = self.shared.tools(id, None);
        let mut request = ForkSessionRequest::new(session, folder).mcp_servers(given);
        if let Some((_, last)) = &upto {
            let point = serde_json::json!({"air": {"fork": {"version": 1, "messageId": last}}});
            request = request.meta(serde_json::Map::from_iter([("jetbrains".into(), point)]));
        }
        let at = upto.map(|(at, _)| at);
        let (shared, agent) = (self.shared.clone(), id.to_owned());
        self.ask(id, request, move |r| {
            if let Ok(r) = &r {
                shared.records.open(&agent, r.session_id.clone());
                shared.bind(&tools, r.session_id.clone());
            }
            Event::Forked {
                from,
                at,
                result: r.map(Box::new),
            }
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
        let (happened, agent, on) = (self.shared.happened.clone(), id.to_owned(), session.clone());
        happened.begin(&agent, &on, false);
        self.ask(id, PromptRequest::new(session, content), move |r| {
            happened.end(
                &agent,
                &on,
                happened::ended(r.as_ref().map(|r| r.stop_reason)),
            );
            Event::Ended {
                key,
                tokens: r.as_ref().map(crate::usage::Tokens::of).unwrap_or_default(),
                result: r.map(|r| r.stop_reason),
            }
        });
    }

    /// Have a writer write what `order` asks: a session opened for that
    /// alone, its model chosen, the prompt said, and what it says in answer
    /// kept until the turn ends; then the session is let go.
    fn write(&self, order: writer::Order) {
        let writer::Order {
            agent,
            model,
            key,
            ask,
            yours,
            dir,
            delete,
            close,
        } = order;
        let (running, send, writing) = (
            self.running(&agent),
            self.shared.send.clone(),
            self.shared.writing.clone(),
        );
        acp::runtime().spawn(async move {
            let made = ask.clone();
            let result = async {
                let agent = running.ok_or("the agent is not running")?;
                let prompt = tokio::task::spawn_blocking(move || made.prompt(&yours))
                    .await
                    .map_err(|e| e.to_string())??;
                let opened = agent
                    .request(NewSessionRequest::new(dir))
                    .await
                    .map_err(|f| f.message)?;
                let id = opened.session_id;
                writing
                    .lock()
                    .expect("writing")
                    .insert(id.clone(), String::new());
                let chosen = choose_model(&agent, &id, opened.config_options, model).await;
                let turn = match chosen {
                    Ok(()) => agent
                        .request(PromptRequest::new(id.clone(), vec![prompt.into()]))
                        .await
                        .map(|_| ()),
                    Err(f) => Err(f),
                };
                let said = writing
                    .lock()
                    .expect("writing")
                    .remove(&id)
                    .unwrap_or_default();
                if delete {
                    let _ = agent.request(DeleteSessionRequest::new(id)).await;
                } else if close {
                    let _ = agent.request(CloseSessionRequest::new(id)).await;
                }
                turn.map_err(|f| f.message)?;
                Ok::<_, String>(ask.take(&said))
            }
            .await;
            send(Event::Written { key, ask, result });
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
    fn answer(&self, id: &str, request: u64, answer: acp::Answer) {
        if let Some(agent) = self.running(id) {
            agent.respond(request, answer);
        }
    }

    /// Put the session in another mode.
    fn set_mode(&self, id: &str, key: u64, session: SessionId, mode: String) {
        let Some(agent) = self.running(id) else {
            return;
        };
        let send = self.shared.send.clone();
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
            SessionConfigId::new(config.clone()),
            SessionConfigOptionValue::value_id(SessionConfigValueId::new(value.clone())),
        );
        let telemetry = self.shared.telemetry.clone();
        self.ask(id, request, move |r| match r {
            Ok(r) => {
                let model = (r.config_options.iter()).any(|o| {
                    o.id.0.as_ref() == config
                        && o.category == Some(SessionConfigOptionCategory::Model)
                });
                if model {
                    telemetry.count(|c| crate::telemetry::tally(&mut c.models, &value));
                }
                Event::Options {
                    key,
                    options: r.config_options,
                }
            }
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

    /// What u0 runs (`diagnostics`), and which are the agents running:
    /// each one's id and process id.
    fn weigh(&self) {
        let running: Vec<(String, u32)> = (self.shared.agents.lock().expect("agents").iter())
            .filter_map(|(id, a)| Some((id.clone(), a.pid()?)))
            .collect();
        self.off(move || Event::Weighed {
            agents: running,
            processes: crate::diagnostics::processes(std::process::id()),
        });
    }

    /// Look at what of each of `agents` is here, in the home and on the
    /// PATH.
    fn here(&self, agents: Vec<Entry>, commands: BTreeMap<String, String>) {
        let home = self.shared.home.clone();
        self.off(move || {
            Event::Here(
                (agents.iter())
                    .map(|e| (e.id.clone(), here(e, &home, commands.get(&e.id))))
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

/// What the agent `agent` said, as the server says it: what it said in a
/// session open with it is written in its record, and sent as the lines
/// that makes; the rest as it is. Its process stopping closes its sessions,
/// unless it is one let go before (another may run in its place).
fn heard(
    records: &Records,
    running: &Running,
    writing: &Writing,
    agent: &str,
    event: acp::Event,
) -> Option<Event> {
    let event = match event {
        // What a writer says is its answer, kept until it ends; what it
        // would ask of you is refused: it was told it needs nothing.
        acp::Event::Update(n) if writing.lock().expect("writing").contains_key(&n.session_id) => {
            if let SessionUpdate::AgentMessageChunk(ContentChunk {
                content: ContentBlock::Text(t),
                ..
            }) = n.update
                && let Some(said) = writing.lock().expect("writing").get_mut(&n.session_id)
            {
                said.push_str(&t.text);
            }
            return None;
        }
        acp::Event::Permission { id, request }
            if writing
                .lock()
                .expect("writing")
                .contains_key(&request.session_id) =>
        {
            if let Some(a) = running.lock().expect("agents").get(agent) {
                a.respond(id, acp::Answer::cancelled(false));
            }
            return None;
        }
        acp::Event::Update(n) => match records.said(agent, n) {
            Ok((id, events)) => {
                return Some(Event::Appended {
                    agent: agent.to_owned(),
                    id,
                    events,
                });
            }
            Err(n) => acp::Event::Update(*n),
        },
        acp::Event::Exited { pid, why } => {
            let this = running.lock().expect("agents").get(agent).map(|a| a.pid());
            if this == Some(pid) {
                records.close(agent);
            }
            acp::Event::Exited { pid, why }
        }
        other => other,
    };
    Some(Event::Said {
        agent: agent.to_owned(),
        event: Box::new(event),
    })
}

/// What of `entry` is in `home`, started by `command` where you gave one.
fn here(entry: &Entry, home: &std::path::Path, command: Option<&String>) -> Here {
    Here {
        missing: registry::launch_with(entry, home, command.map(String::as_str)).err(),
        installed: entry.installed(home),
        cli: entry.your_cli().map(|(_, cli)| cli),
    }
}

/// Whether `a` and `b` are the same session: of one agent, by one id.
fn same(a: &Meta, b: &Meta) -> bool {
    a.agent == b.agent && a.id == b.id
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use agent_client_protocol::v1::schema::StopReason;

    use super::*;

    /// A server for `home`, and what it says.
    pub(super) fn server(home: &std::path::Path) -> (Arc<Server>, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let server = Server::new(home.to_path_buf(), move |e| {
            let _ = tx.send(e);
        });
        (server, rx)
    }

    fn meta(id: &str, title: &str) -> Meta {
        serde_json::from_value(serde_json::json!({
            "agent": "a", "id": id, "project": "/p", "folder": "/p",
            "title": title, "begun": 5, "updated": 7,
        }))
        .unwrap()
    }

    /// The record of the session `id` of `a`, as the server reads it back.
    fn read(
        server: &Arc<Server>,
        rx: &mpsc::Receiver<Event>,
        id: &str,
    ) -> Option<Vec<store::Event>> {
        server.handle(Call::Read {
            key: 9,
            agent: "a".into(),
            id: SessionId::new(id),
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            Event::Record { key: 9, events } => events,
            other => panic!("not a record: {other:?}"),
        }
    }

    #[test]
    fn the_server_writes_the_index_by_the_metas_sent_and_each_record() {
        let home = tempfile::tempdir().unwrap();
        let (server, rx) = server(home.path());
        server.handle(Call::Watch);
        assert!(matches!(rx.recv().unwrap(), Event::Index(Ok(i)) if i.sessions.is_empty()));
        assert!(matches!(rx.recv().unwrap(), Event::Tasks(Ok(t)) if t.is_empty()));
        server.handle(Call::Keep(vec![meta("s1", "first"), meta("s2", "other")]));
        server.handle(Call::Keep(vec![meta("s1", "renamed")]));
        server.handle(Call::Stash(vec!["later".into()]));
        let ended = store::Event::Ended {
            reason: StopReason::EndTurn,
            secs: 1,
            tokens: Vec::new(),
        };
        server.handle(Call::Append {
            agent: "a".into(),
            id: SessionId::new("s1"),
            events: vec![store::Event::You("go".into()), ended],
        });
        server.handle(Call::Append {
            agent: "a".into(),
            id: SessionId::new("s1"),
            events: vec![store::Event::You("again".into())],
        });
        // A branch before the second message has only the first.
        server.handle(Call::CopyRecord {
            agent: "a".into(),
            from: SessionId::new("s1"),
            to: SessionId::new("s3"),
            yours: 1,
        });
        assert_eq!(read(&server, &rx, "s1").unwrap().len(), 3);
        let branch = read(&server, &rx, "s3").unwrap();
        assert!(matches!(&branch[..], [store::Event::You(t), _] if t == "go"));
        server.handle(Call::Forget {
            agent: "a".into(),
            id: SessionId::new("s1"),
            keep_out: true,
        });
        assert!(read(&server, &rx, "s1").is_none());
        let index = store::load(home.path()).unwrap();
        let titles: Vec<&str> = index.sessions.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["other"]);
        assert_eq!(index.gone, [("a".to_owned(), "s1".to_owned())]);
        assert_eq!(index.stashed, ["later"]);
    }

    #[test]
    fn an_index_that_does_not_read_is_said_so_and_never_written_over() {
        let home = tempfile::tempdir().unwrap();
        let path = store::index_path(home.path());
        std::fs::write(&path, r#"{"threads":[]}"#).unwrap();
        let (server, rx) = server(home.path());
        server.handle(Call::Watch);
        let said = rx.recv().unwrap();
        assert!(matches!(said, Event::Index(Err(why)) if why.contains("Move it away")));
        server.handle(Call::Keep(vec![meta("s1", "first")]));
        Server::settle();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"threads":[]}"#);
    }

    #[test]
    fn what_an_agent_says_in_a_session_it_loads_is_written_and_sent_as_its_record() {
        let home = tempfile::tempdir().unwrap();
        // An agent that says hello, then, asked to load `s1`, says again
        // what was said in it, and then stops.
        let agent = home.path().join("agent.sh");
        let script = r#"id() { printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }
read line
printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$(id)"
read line
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"probe","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x"}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id)"
read line"#;
        std::fs::write(&agent, script).unwrap();
        let (server, rx) = server(home.path());
        let next = || rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "id": "a", "name": "A", "version": "1", "distribution": {},
        }))
        .unwrap();
        server.handle(Call::Start {
            entry,
            command: Some(format!("/bin/sh {}", agent.display())),
            env: BTreeMap::new(),
        });
        assert!(matches!(next(), Event::Started { result: Ok(_), .. }));
        let s1 = SessionId::new("s1");
        server.handle(Call::Reopen {
            agent: "a".into(),
            key: 1,
            session: s1.clone(),
            folder: home.path().to_path_buf(),
            also: Vec::new(),
            resumed: false,
        });
        let Event::Appended { id, events, .. } = next() else {
            panic!("not the thread's record");
        };
        assert_eq!(id, s1);
        assert!(matches!(
            &events[..],
            [store::Event::At(_), store::Event::Update(_)]
        ));
        // One said in a session not open here comes as it was said.
        assert!(
            matches!(next(), Event::Said { event, .. } if matches!(*event, acp::Event::Update(_)))
        );
        assert!(matches!(
            next(),
            Event::Loaded {
                key: 1,
                result: Ok(_),
                ..
            }
        ));
        Server::settle();
        let record = store::read(&store::record_path(home.path(), "a", "s1")).unwrap();
        assert_eq!(record.len(), 2);
        assert!(store::read(&store::record_path(home.path(), "a", "probe")).is_none());
    }

    #[test]
    fn a_writer_answers_in_a_session_of_its_own_which_is_deleted_and_never_recorded() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        // An agent that opens `w1`, answers the prompt there in a code
        // block, and writes each line it is sent to a file.
        let agent = home.path().join("agent.sh");
        let heard = home.path().join("heard");
        let script = format!(
            r#"id() {{ printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }}
read line
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1}}}}\n' "$(id)"
read line; echo "$line" >> {heard}
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"w1"}}}}\n' "$(id)"
read line; echo "$line" >> {heard}
printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"w1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"```\nAdd a.txt"}}}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"w1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"\n```"}}}}}}}}'
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$(id)"
read line; echo "$line" >> {heard}
printf '{{"jsonrpc":"2.0","id":%s,"result":{{}}}}\n' "$(id)"
read line"#,
            heard = heard.display()
        );
        std::fs::write(&agent, script).unwrap();
        let (server, rx) = server(home.path());
        let next = || rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "id": "a", "name": "A", "version": "1", "distribution": {},
        }))
        .unwrap();
        server.handle(Call::Start {
            entry,
            command: Some(format!("/bin/sh {}", agent.display())),
            env: BTreeMap::new(),
        });
        assert!(matches!(next(), Event::Started { result: Ok(_), .. }));
        let ask = writer::Ask::Commit { dir: repo };
        server.handle(Call::Write(writer::Order {
            agent: "a".into(),
            model: None,
            key: 3,
            ask: ask.clone(),
            yours: "Say it in French.".into(),
            dir: home.path().to_path_buf(),
            delete: true,
            close: false,
        }));
        // What it said in the session is the answer, not an update.
        let Event::Written {
            key: 3,
            ask: a,
            result,
        } = next()
        else {
            panic!("not what was written");
        };
        assert_eq!((a, result), (ask, Ok("Add a.txt".to_owned())));
        let heard = std::fs::read_to_string(&heard).unwrap();
        let lines: Vec<&str> = heard.lines().collect();
        assert!(lines[0].contains("session/new"));
        assert!(lines[1].contains("session/prompt") && lines[1].contains("a.txt"));
        assert!(lines[1].contains("Say it in French."));
        assert!(lines[2].contains("session/delete") && lines[2].contains("w1"));
        Server::settle();
        assert!(store::read(&store::record_path(home.path(), "a", "w1")).is_none());
    }
}
