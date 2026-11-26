//! The scheduled tasks (`tasks`), run by the server: each due one is said
//! to its agent, started here if it is not running, in a session of its
//! own or the next turn of the one it keeps. What it says is recorded as
//! any session's is, and the desk is told as each turn begins and ends.

use std::{sync::Arc, time::Duration};

use agent_client_protocol::v1::schema::{
    NewSessionRequest, PromptRequest, ResumeSessionRequest, SessionId, SessionModeId,
    SetSessionModeRequest, StopReason,
};

use super::{Server, Shared, choose_model};
use crate::{
    acp::{self, Agent},
    git,
    proto::Event,
    registry,
    session::Session,
    settings::Settings,
    store::{self, Job, Meta},
    tasks::{self, Run, Task, Tasks},
};

/// How often the server looks for tasks due.
const TICK: Duration = Duration::from_secs(20);

/// How many ticks between looks at the pull requests watched: two minutes.
const PR_TICKS: u32 = 6;

impl Server {
    pub(super) fn drop_task(&self, id: u64) {
        self.shared
            .change_tasks(|all| all.tasks.retain(|t| t.id != id));
    }

    /// Run the task `id` now, unless a run of it goes on.
    pub(super) fn run_task(&self, id: u64) {
        self.shared.run(id);
    }

    /// The tasks as they are, to the desk.
    pub(super) fn say_tasks(&self) {
        self.shared.say_tasks();
    }
}

impl Shared {
    /// Keep `task` as it is now: a new one (id 0) is given the next id. Its
    /// next run is worked out again from its schedule.
    pub(super) fn set_task(&self, mut task: Task) {
        let now = store::secs(std::time::SystemTime::now());
        task.next = task.schedule.next(now);
        self.change_tasks(|all| {
            if let Some(t) = (all.tasks.iter_mut()).find(|t| t.id == task.id && task.id != 0) {
                // What only its runs know is the server's.
                (task.last, task.runs) = (t.last.take(), t.runs);
                if task.session.is_none() {
                    task.session.clone_from(&t.session);
                }
                *t = task;
            } else {
                task.id = all.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                all.tasks.push(task);
            }
        });
    }

    /// Look for tasks due every [`TICK`], from now on.
    pub(super) fn schedule(&self) {
        let this = self.clone();
        acp::runtime().spawn(async move {
            let mut ticks = 0u32;
            loop {
                tokio::time::sleep(TICK).await;
                this.run_due();
                ticks = (ticks + 1) % PR_TICKS;
                if ticks == 0 {
                    this.watch_prs();
                }
            }
        });
    }

    fn say_tasks(&self) {
        let tasks = self.tasks.lock().expect("tasks").clone();
        (self.send)(Event::Tasks(tasks.map(|t| t.tasks)));
    }

    /// Change the tasks by `change`, write them, and say them to the desk;
    /// unless they did not read.
    fn change_tasks(&self, change: impl FnOnce(&mut Tasks)) {
        {
            let mut tasks = self.tasks.lock().expect("tasks");
            let Ok(tasks) = &mut *tasks else { return };
            change(tasks);
            store::queue(Job::Write(tasks::path(&self.home), tasks::bytes(tasks)));
        }
        self.say_tasks();
    }

    /// Run each task due that is not paused, nor going, nor waiting on a
    /// turn of its session.
    fn run_due(&self) {
        let now = store::secs(std::time::SystemTime::now());
        let due: Vec<u64> = match &*self.tasks.lock().expect("tasks") {
            Ok(all) => (all.tasks.iter())
                .filter(|t| !t.paused && t.next.is_some_and(|at| at <= now))
                .map(|t| t.id)
                .collect(),
            Err(_) => Vec::new(),
        };
        for id in due {
            self.run(id);
        }
    }

    /// Run the task `id`: its run marked going, its next one set, and the
    /// turn said; then how it went kept.
    fn run(&self, id: u64) {
        let now = store::secs(std::time::SystemTime::now());
        let turning = self.turning.lock().expect("turning").clone();
        let mut task = None;
        self.change_tasks(|all| {
            let Some(t) = all.tasks.iter_mut().find(|t| t.id == id) else {
                return;
            };
            let busy = t
                .session
                .as_ref()
                .is_some_and(|s| t.same && turning.contains(&(t.agent.clone(), s.clone())));
            if t.last.as_ref().is_some_and(|r| r.going) || busy {
                return;
            }
            t.next = t.schedule.next(now);
            t.runs += 1;
            t.last = Some(Run {
                at: now,
                session: None,
                going: true,
                error: None,
            });
            task = Some(t.clone());
        });
        let Some(task) = task else { return };
        let this = self.clone();
        acp::runtime().spawn(async move {
            let result = this.turn_with(&task, |_| {}).await;
            this.change_tasks(|all| {
                let Some(t) = all.tasks.iter_mut().find(|t| t.id == task.id) else {
                    return;
                };
                let (session, error) = match result {
                    Ok(id) => (Some(id), None),
                    Err((id, why)) => (id, Some(why)),
                };
                if t.same && t.session.is_none() {
                    t.session.clone_from(&session);
                }
                let done = error.is_none() && matches!(t.schedule, tasks::Schedule::Once(_));
                t.last = Some(Run {
                    at: now,
                    session,
                    going: false,
                    error,
                });
                // One said once is done with once it was said.
                if done {
                    all.tasks.retain(|t| t.id != task.id);
                }
            });
        });
    }

    /// Look at each pull request a session watches: where its checks
    /// finished, or a review came in, since it was last looked at, say so
    /// in the session, as a turn; one merged or closed is watched no more.
    fn watch_prs(&self) {
        let watching: Vec<Meta> = match &*self.index.lock().expect("index") {
            Ok(index) => (index.sessions.iter())
                .filter(|m| m.watch.is_some())
                .cloned()
                .collect(),
            Err(_) => return,
        };
        for meta in watching {
            let this = self.clone();
            acp::runtime().spawn(async move {
                let folder = meta.folder.clone();
                let Ok(Some(pr)) = tokio::task::spawn_blocking(move || git::read_pr(&folder)).await
                else {
                    return;
                };
                let Some(id) = meta.id.clone() else { return };
                if Some(pr.number) != meta.watch || pr.state != "open" {
                    this.change_index(|index| {
                        if let Some(m) = index.sessions.iter_mut().find(|m| super::same(m, &meta)) {
                            m.watch = None;
                        }
                    });
                    return;
                }
                let now = watched(&pr);
                let at = (meta.agent.clone(), id.clone());
                let was = this
                    .watched
                    .lock()
                    .expect("watched")
                    .insert(at.clone(), now.clone());
                let Some(said) = was.and_then(|was| woken(&was, &now, &pr)) else {
                    return;
                };
                if this.turning.lock().expect("turning").contains(&at) {
                    // Said once the turn going on is over: looked at again.
                    this.watched
                        .lock()
                        .expect("watched")
                        .insert(at, was_of(&now));
                    return;
                }
                let task = Task {
                    id: 0,
                    title: meta.title.clone(),
                    prompt: said,
                    agent: meta.agent.clone(),
                    model: None,
                    mode: None,
                    project: meta.project.clone(),
                    schedule: tasks::Schedule::Every(0),
                    paused: false,
                    same: true,
                    worktree: false,
                    session: Some(id),
                    next: None,
                    last: None,
                    runs: 0,
                };
                let _ = this.turn_with(&task, |_| {}).await;
            });
        }
    }

    /// The agent `id`, started here if it is not running: as the desk
    /// starts it, by the registry kept and your settings.
    async fn agent(&self, id: &str) -> Result<Arc<Agent>, String> {
        if let Some(agent) = self.running(id) {
            return Ok(agent);
        }
        let entry = registry::entry_of(&registry::kept(&self.home).unwrap_or_default(), id)
            .ok_or_else(|| format!("{id} is not in the registry here"))?;
        let settings = Settings::load(&self.home).0;
        let launch = registry::launch_with(
            &entry,
            &self.home,
            settings.commands.get(id).map(String::as_str),
        )
        .map_err(|m| m.say(&entry.name))?;
        let env = settings.env.get(id).cloned().unwrap_or_default();
        let started = self.launch(id, launch, env).await;
        let (agent, result) = match started {
            Ok((agent, said)) => (Ok(agent), Ok(said)),
            Err(f) => (Err(f.message.clone()), Err(f)),
        };
        (self.send)(Event::Started {
            agent: id.to_owned(),
            result,
        });
        agent
    }

    /// Say the task's prompt: in the session it keeps, opened again if it
    /// is not open, or in a new one. The session it was said in, or why it
    /// was not (with the session, where it got that far).
    /// `opened` is told the session as soon as it has one.
    pub(super) async fn turn_with(
        &self,
        task: &Task,
        opened: impl FnOnce(&SessionId),
    ) -> Result<SessionId, (Option<SessionId>, String)> {
        let agent = self.agent(&task.agent).await.map_err(|e| (None, e))?;
        let kept = task.session.clone().filter(|_| task.same);
        let (meta, fresh) = match kept {
            Some(id) => (self.reopen(&agent, task, id).await?, false),
            None => (self.open(&agent, task).await.map_err(|e| (None, e))?, true),
        };
        let id = meta.id.clone().expect("a session opened has an id");
        opened(&id);
        let failed = |why: String| (Some(id.clone()), why);
        // The desk is told, with what was said in it so far, read after all
        // that is being written to it; then what you said is recorded.
        let record = if fresh {
            None
        } else {
            let path = self.records.path(&task.agent, &id);
            let (tx, rx) = tokio::sync::oneshot::channel();
            store::queue(Job::Then(Box::new(move || {
                let _ = tx.send(store::read(&path));
            })));
            rx.await.ok().flatten()
        };
        (self.send)(Event::TaskTurn {
            meta: Box::new(meta.clone()),
            record,
            ended: None,
        });
        let now = std::time::SystemTime::now();
        self.append(
            &task.agent,
            &id,
            vec![
                store::Event::At(store::secs(now)),
                store::Event::You(task.prompt.clone()),
            ],
        );
        let on = (task.agent.clone(), id.clone());
        self.turning.lock().expect("turning").insert(on.clone());
        let said = agent
            .request(PromptRequest::new(
                id.clone(),
                vec![task.prompt.clone().into()],
            ))
            .await;
        self.turning.lock().expect("turning").remove(&on);
        let secs = now.elapsed().map_or(0, |d| d.as_secs());
        let tokens = said
            .as_ref()
            .map(crate::usage::Tokens::of)
            .unwrap_or_default();
        let ended: Result<StopReason, String> = said.map(|r| r.stop_reason).map_err(|f| f.message);
        self.append(
            &task.agent,
            &id,
            vec![match &ended {
                Ok(reason) => store::Event::Ended {
                    reason: *reason,
                    secs,
                    tokens,
                },
                Err(why) => store::Event::Note {
                    text: why.clone(),
                    trouble: true,
                },
            }],
        );
        let mut meta = meta;
        meta.updated = std::time::SystemTime::now();
        self.change_index(|index| {
            if let Some(m) = index.sessions.iter_mut().find(|m| super::same(m, &meta)) {
                m.updated = meta.updated;
            }
        });
        let error = ended.as_ref().err().cloned();
        (self.send)(Event::TaskTurn {
            meta: Box::new(meta),
            record: None,
            ended: Some(ended),
        });
        error.map_or(Ok(id.clone()), |why| Err(failed(why)))
    }

    /// `events`, what Ghost says in the session `id` (what you said, where
    /// a turn ended), written in its record and sent to the desk.
    fn append(&self, agent: &str, id: &SessionId, events: Vec<store::Event>) {
        self.records.append(agent, id, events.clone());
        (self.send)(Event::Appended {
            agent: agent.to_owned(),
            id: id.clone(),
            events,
        });
    }

    /// A new session for a run of `task`: in its project, or a worktree
    /// of it made for this run; its model and mode chosen; kept in the
    /// index, above the rest.
    async fn open(&self, agent: &Agent, task: &Task) -> Result<Meta, String> {
        let mut session = Session::new(0, &task.agent, task.project.clone());
        if task.worktree {
            let (project, about, home) =
                (task.project.clone(), task.title.clone(), self.home.clone());
            let made = tokio::task::spawn_blocking(move || {
                let settings = Settings::load(&home).0;
                let under = settings.worktree_folders(&home).remove(0);
                let from = git::fresh_base(&project);
                let wt = git::worktree(
                    &project,
                    &about,
                    &under,
                    &settings.branch_prefix,
                    &from,
                    None,
                )?;
                git::submodules(&wt.path, settings.of(&project).submodules);
                Ok::<_, String>(wt)
            })
            .await
            .map_err(|e| e.to_string())??;
            session.meta.folder.clone_from(&made.path);
            session.meta.worktree = Some(made);
        }
        let (tools, given) = self.tools(&task.agent, None);
        let request = NewSessionRequest::new(session.meta.folder.clone()).mcp_servers(given);
        let opened = agent.request(request).await.map_err(|f| f.message)?;
        let id = opened.session_id.clone();
        self.bind(&tools, id.clone());
        self.records.open(&task.agent, id.clone());
        choose_model(agent, &id, opened.config_options, task.model.clone())
            .await
            .map_err(|f| f.message)?;
        if let Some(mode) = &task.mode {
            agent
                .request(SetSessionModeRequest::new(
                    id.clone(),
                    SessionModeId::new(mode.clone()),
                ))
                .await
                .map_err(|f| f.message)?;
        }
        let mut meta = session.meta;
        meta.id = Some(id);
        meta.title.clone_from(&task.title);
        meta.renamed = true;
        let kept = meta.clone();
        self.change_index(|index| index.sessions.push(kept));
        Ok(meta)
    }

    /// The session `id` a task keeps, as the index has it: opened again
    /// with its agent (resumed, as it has a record) unless it is open.
    async fn reopen(
        &self,
        agent: &Agent,
        task: &Task,
        id: SessionId,
    ) -> Result<Meta, (Option<SessionId>, String)> {
        let failed = |why: String| (Some(id.clone()), why);
        let meta = (self.index.lock().expect("index").as_ref().ok())
            .and_then(|i| {
                (i.sessions.iter())
                    .find(|m| m.agent == task.agent && m.id.as_ref() == Some(&id))
                    .cloned()
            })
            .ok_or_else(|| failed("Its session is no longer kept".into()))?;
        if !self.records.is_open(&task.agent, &id) {
            self.records.open(&task.agent, id.clone());
            let (_, given) = self.tools(&task.agent, Some(id.clone()));
            let request = ResumeSessionRequest::new(id.clone(), meta.folder.clone())
                .additional_directories(meta.also.clone())
                .mcp_servers(given);
            if let Err(f) = agent.request(request).await {
                return Err(failed(f.message));
            }
        }
        Ok(meta)
    }
}

/// Where a pull request stands, as its watch compares it: its checks
/// (going, or how many passed and failed) and the review decided.
fn watched(pr: &git::Pr) -> String {
    let checks = if pr.going > 0 {
        "going".to_owned()
    } else {
        format!("{}/{}", pr.passed, pr.failed)
    };
    format!("{checks}|{}", pr.review)
}

/// A standing that will be woken on again: its checks taken as going.
fn was_of(now: &str) -> String {
    format!("going|{}", now.split_once('|').map_or("", |(_, r)| r))
}

/// What to say in a session that watches `pr`, which stood at `was` and
/// stands at `now`: its checks finished, or a review came in; nothing
/// where neither happened.
fn woken(was: &str, now: &str, pr: &git::Pr) -> Option<String> {
    let (checks_was, review_was) = was.split_once('|')?;
    let (checks, review) = now.split_once('|')?;
    let mut said = Vec::new();
    if checks_was == "going" && checks != "going" {
        said.push(format!(
            "The checks on pull request #{} finished: {} passed, {} failed.",
            pr.number, pr.passed, pr.failed
        ));
    }
    if review != review_was && !review.is_empty() {
        said.push(format!(
            "A review came in on pull request #{}: {}.",
            pr.number,
            review.to_lowercase()
        ));
    }
    (!said.is_empty()).then(|| {
        format!(
            "{} Look at it (gh pr view {n} --comments, gh pr checks {n}) and carry on from there.",
            said.join(" "),
            n = pr.number
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_watched_pull_request_wakes_its_session_when_checks_finish_or_a_review_comes_in() {
        let pr = |going, passed, failed, review: &str| git::Pr {
            number: 7,
            title: "t".into(),
            url: String::new(),
            state: "open".into(),
            draft: false,
            conflicts: false,
            passed,
            failed,
            going,
            review: review.into(),
        };
        let going = watched(&pr(2, 1, 0, ""));
        let done = pr(0, 2, 1, "");
        let said = woken(&going, &watched(&done), &done).unwrap();
        assert!(said.starts_with("The checks on pull request #7 finished: 2 passed, 1 failed."));
        // Nothing new, nothing said.
        assert_eq!(woken(&watched(&done), &watched(&done), &done), None);
        let reviewed = pr(0, 2, 1, "CHANGES_REQUESTED");
        let said = woken(&watched(&done), &watched(&reviewed), &reviewed).unwrap();
        assert!(said.starts_with("A review came in on pull request #7: changes_requested."));
        // One woken on while a turn went on is woken on again after.
        assert!(woken(&was_of(&watched(&done)), &watched(&done), &done).is_some());
    }

    use std::sync::mpsc;

    use super::*;
    use crate::{proto::Call, tasks::Schedule};

    #[test]
    fn a_task_run_starts_its_agent_and_says_its_prompt_in_a_session_it_keeps() {
        let home = tempfile::tempdir().unwrap();
        // An agent that opens `t1` and answers each prompt in it, writing
        // each line it is sent to a file.
        let agent = home.path().join("agent.sh");
        let heard = home.path().join("heard");
        let script = format!(
            r#"id() {{ printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }}
read line
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1}}}}\n' "$(id)"
read line; echo "$line" >> {heard}
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"t1"}}}}\n' "$(id)"
while read line; do echo "$line" >> {heard}
printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"t1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"done"}}}}}}}}'
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$(id)"
done"#,
            heard = heard.display()
        );
        std::fs::write(&agent, script).unwrap();
        std::fs::write(
            home.path().join("registry.json"),
            r#"{"agents":[{"id":"a","name":"A","version":"1","distribution":{}}]}"#,
        )
        .unwrap();
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::json!({
                "enabled": [], "folders": [],
                "commands": {"a": format!("/bin/sh {}", agent.display())},
            })
            .to_string(),
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let server = Server::new(home.path().to_path_buf(), move |e| {
            let _ = tx.send(e);
        });
        let task = Task {
            id: 0,
            title: "Nightly".into(),
            prompt: "Look over the day".into(),
            agent: "a".into(),
            model: None,
            mode: None,
            project: home.path().to_path_buf(),
            schedule: Schedule::Every(60),
            paused: false,
            same: true,
            worktree: false,
            session: None,
            next: None,
            last: None,
            runs: 0,
        };
        server.handle(Call::SetTask(task));
        // The task's run ended, as the list says it.
        let settled = |rx: &mpsc::Receiver<Event>| loop {
            if let Event::Tasks(Ok(t)) = rx.recv_timeout(Duration::from_secs(10)).unwrap()
                && t[0].last.as_ref().is_some_and(|r| !r.going)
            {
                break t;
            }
        };
        let Event::Tasks(Ok(set)) = rx.recv().unwrap() else {
            panic!("not the tasks");
        };
        assert_eq!(set[0].id, 1);
        let now = store::secs(std::time::SystemTime::now());
        assert!(set[0].next.is_some_and(|at| at > now));
        server.handle(Call::RunTask(1));
        let ran = settled(&rx);
        let t1 = SessionId::new("t1");
        assert_eq!(ran[0].session.as_ref(), Some(&t1));
        assert_eq!(ran[0].runs, 1);
        assert_eq!(ran[0].last.as_ref().unwrap().error, None);
        // Again, in the same session: no new one is opened.
        server.handle(Call::RunTask(1));
        assert_eq!(settled(&rx)[0].runs, 2);
        Server::settle();
        let record = store::read(&store::record_path(home.path(), "a", "t1")).unwrap();
        let yours = (record.iter())
            .filter(|e| matches!(e, store::Event::You(t) if t == "Look over the day"))
            .count();
        let ended = (record.iter())
            .filter(|e| matches!(e, store::Event::Ended { .. }))
            .count();
        assert_eq!((yours, ended), (2, 2));
        let index = store::load(home.path()).unwrap();
        assert_eq!(index.sessions[0].title, "Nightly");
        let heard = std::fs::read_to_string(&heard).unwrap();
        assert_eq!(heard.matches("session/new").count(), 1);
        assert_eq!(heard.matches("session/prompt").count(), 2);
        assert_eq!(tasks::load(home.path()).unwrap().tasks[0].runs, 2);
    }
}
