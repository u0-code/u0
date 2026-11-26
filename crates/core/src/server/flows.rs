//! Workflows (`workflows`) run by the server: each run goes step by step,
//! each step's result kept as it ends, so that a run stopped by a restart
//! goes on from the step it was at. An agent step is a turn said as a
//! scheduled task's is; a question waits for your answer; a command that
//! fails, or an agent that does, pauses the run until you resume it.

use agent_client_protocol::v1::schema::{CancelNotification, SessionId};

use super::{Server, Shared};
use crate::{
    acp,
    proto::Event,
    store::{self, Job},
    tasks::{Schedule, Task},
    workflows::{self, Done, Run, Runs, State, Step, Workflow},
};

impl Server {
    pub(super) fn say_flows(&self) {
        self.shared.say_flows();
    }

    /// Begin a run of the workflow `workflow` with `input`.
    pub(super) fn start_flow(&self, workflow: String, input: String) {
        let name = format!("Couldn't start {workflow}");
        if let Err(why) = self.shared.start_flow(workflow, input) {
            (self.shared.send)(Event::Notice(Err((name, why))));
        }
    }

    /// Answer the question the run `id` waits on with `choice`.
    pub(super) fn answer_flow(&self, id: u64, choice: String) {
        let home = self.shared.home.clone();
        let mut going = false;
        self.shared.change_runs(|all| {
            let Some(run) = all
                .runs
                .iter_mut()
                .find(|r| r.id == id && r.state == State::Asking)
            else {
                return;
            };
            let Ok(flow) = workflows::find(&home, &run.workflow) else {
                return;
            };
            let Some(Step::Ask { name, stop, .. }) = flow.steps.get(run.done.len()) else {
                return;
            };
            run.state = if stop.contains(&choice) {
                State::Stopped(format!("{choice}, at “{name}”"))
            } else {
                going = true;
                State::Going
            };
            run.done.push(Done {
                name: name.clone(),
                result: choice,
                session: None,
            });
        });
        if going {
            self.shared.advance(id);
        }
    }

    /// Take the paused run `id` up again, at the step it stopped at.
    pub(super) fn resume_flow(&self, id: u64) {
        let mut going = false;
        self.shared.change_runs(|all| {
            if let Some(run) =
                (all.runs.iter_mut()).find(|r| r.id == id && matches!(r.state, State::Paused(_)))
            {
                run.state = State::Going;
                going = true;
            }
        });
        if going {
            self.shared.advance(id);
        }
    }

    /// Cancel the run `id`: the turn it has going is stopped.
    pub(super) fn cancel_flow(&self, id: u64) {
        let mut turn = None;
        self.shared.change_runs(|all| {
            if let Some(run) = all.runs.iter_mut().find(|r| r.id == id && !r.over()) {
                run.state = State::Cancelled;
                turn = run.turn.take();
            }
        });
        if let Some((agent, session)) = turn
            && let Some(agent) = self.shared.running(&agent)
        {
            agent.cancel(CancelNotification::new(session));
        }
    }
}

impl Shared {
    /// Begin a run of the workflow `workflow` with `input`: its number, or
    /// why not.
    pub(super) fn start_flow(&self, workflow: String, input: String) -> Result<u64, String> {
        workflows::find(&self.home, &workflow)?;
        let mut id = 0;
        self.change_runs(|all| {
            id = all.runs.iter().map(|r| r.id).max().unwrap_or(0) + 1;
            all.runs.push(Run {
                id,
                workflow,
                input,
                began: store::secs(std::time::SystemTime::now()),
                state: State::Going,
                done: Vec::new(),
                turn: None,
            });
        });
        if id == 0 {
            return Err("The runs kept do not read".into());
        }
        self.advance(id);
        Ok(id)
    }

    /// The workflows and the runs, to the desk.
    fn say_flows(&self) {
        let runs = self.runs.lock().expect("runs").clone();
        (self.send)(Event::Workflows {
            flows: workflows::load(&self.home),
            runs: runs.map(|r| r.runs),
        });
    }

    /// Change the runs by `change`, write them, and say them; unless they
    /// did not read.
    fn change_runs(&self, change: impl FnOnce(&mut Runs)) {
        {
            let mut runs = self.runs.lock().expect("runs");
            let Ok(runs) = &mut *runs else { return };
            change(runs);
            store::queue(Job::Write(
                workflows::runs_path(&self.home),
                workflows::bytes(runs),
            ));
        }
        self.say_flows();
    }

    /// Go on with each run that was going as the server last stopped.
    pub(super) fn go_on(&self) {
        let going: Vec<u64> = match &*self.runs.lock().expect("runs") {
            Ok(all) => (all.runs.iter())
                .filter(|r| r.state == State::Going)
                .map(|r| r.id)
                .collect(),
            Err(_) => Vec::new(),
        };
        for id in going {
            self.advance(id);
        }
    }

    /// Run the run `id`'s steps from the one it is at, while it goes.
    fn advance(&self, id: u64) {
        let this = self.clone();
        acp::runtime().spawn(async move {
            while let Some(run) = this.going(id) {
                let flow = match workflows::find(&this.home, &run.workflow) {
                    Ok(flow) => flow,
                    Err(why) => return this.end(id, State::Paused(why)),
                };
                let Some(step) = flow.steps.get(run.done.len()).cloned() else {
                    return this.end(id, State::Done);
                };
                let done = match &step {
                    Step::Ask { .. } => return this.end(id, State::Asking),
                    Step::Run { command, dir, .. } => {
                        this.command(&run, &flow, command, dir.as_ref()).await
                    }
                    Step::Agent { .. } => this.agent_step(&run, &flow, &step).await,
                };
                match done {
                    Ok(done) => this.change_runs(|all| {
                        if let Some(r) = all
                            .runs
                            .iter_mut()
                            .find(|r| r.id == id && r.state == State::Going)
                        {
                            r.done.push(done);
                        }
                    }),
                    Err(why) => return this.end(id, State::Paused(why)),
                }
            }
        });
    }

    /// The run `id`, while it goes.
    fn going(&self, id: u64) -> Option<Run> {
        let runs = self.runs.lock().expect("runs");
        (runs.as_ref().ok()?.runs.iter())
            .find(|r| r.id == id && r.state == State::Going)
            .cloned()
    }

    /// The run `id` stands at `state` now, unless it was cancelled.
    fn end(&self, id: u64, state: State) {
        self.change_runs(|all| {
            if let Some(r) = all
                .runs
                .iter_mut()
                .find(|r| r.id == id && r.state == State::Going)
            {
                r.state = state;
            }
        });
    }

    /// Run `command` for `run` in a shell, where `dir` or the workflow's
    /// project is; what it printed is its result.
    async fn command(
        &self,
        run: &Run,
        flow: &Workflow,
        command: &str,
        dir: Option<&std::path::PathBuf>,
    ) -> Result<Done, String> {
        let dir = dir
            .or(flow.project.as_ref())
            .cloned()
            .ok_or("Name a project or a dir for it")?;
        let (line, values) = run.command(command);
        let name = flow.steps[run.done.len()].name().to_owned();
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new("/bin/sh")
                .args(["-c", &line, "ghost"])
                .args(values)
                .current_dir(dir)
                .stdin(std::process::Stdio::null())
                .output()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("“{name}” failed ({}): {}", out.status, err.trim()));
        }
        Ok(Done {
            name,
            result: said,
            session: None,
        })
    }

    /// Say an agent step's prompt, as a scheduled task's is said; its
    /// result is what the agent answered.
    async fn agent_step(&self, run: &Run, flow: &Workflow, step: &Step) -> Result<Done, String> {
        let Step::Agent {
            name,
            prompt,
            agent,
            model,
            project,
            worktree,
            after,
        } = step
        else {
            unreachable!("an agent step")
        };
        let agent = agent
            .clone()
            .or_else(|| flow.agent.clone())
            .ok_or("Name an agent for it")?;
        let project = project
            .clone()
            .or_else(|| flow.project.clone())
            .ok_or("Name a project for it")?;
        let session = match after {
            Some(earlier) => Some(
                (run.done.iter())
                    .find(|d| &d.name == earlier)
                    .and_then(|d| d.session.clone())
                    .map(|(_, id)| id)
                    .ok_or_else(|| format!("“{earlier}” has no session to go on in"))?,
            ),
            None => None,
        };
        let task = Task {
            id: 0,
            title: format!("{} · {name}", flow.name),
            prompt: run.fill(prompt),
            agent: agent.clone(),
            model: model.clone(),
            mode: None,
            project,
            schedule: Schedule::Every(0),
            paused: false,
            same: session.is_some(),
            worktree: *worktree,
            workflow: None,
            session,
            next: None,
            last: None,
            runs: 0,
        };
        let (on, id) = (agent.clone(), run.id);
        let at = self.turn_with(&task, |session| {
            self.change_runs(|all| {
                if let Some(r) = all.runs.iter_mut().find(|r| r.id == id) {
                    r.turn = Some((on.clone(), session.clone()));
                }
            });
        });
        let said = at.await.map_err(|(_, why)| why)?;
        self.change_runs(|all| {
            if let Some(r) = all.runs.iter_mut().find(|r| r.id == id) {
                r.turn = None;
            }
        });
        Ok(Done {
            name: name.clone(),
            result: self.said_in(&agent, &said, true).await,
            session: Some((agent, said)),
        })
    }

    /// What was said in the session `id`, as its record has it: what the
    /// agent said in its last turn (`last`), or all of it, with what you
    /// said led by "You:".
    pub(super) async fn said_in(&self, agent: &str, id: &SessionId, last: bool) -> String {
        let path = self.records.path(agent, id);
        let (tx, rx) = tokio::sync::oneshot::channel();
        store::queue(Job::Then(Box::new(move || {
            let _ = tx.send(store::read(&path));
        })));
        crate::session::said(rx.await.ok().flatten().unwrap_or_default(), last)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;
    use crate::proto::Call;

    /// The runs, once one stands as `stands` says.
    fn until(rx: &mpsc::Receiver<Event>, stands: impl Fn(&Run) -> bool) -> Vec<Run> {
        loop {
            if let Event::Workflows { runs: Ok(runs), .. } =
                rx.recv_timeout(Duration::from_secs(10)).unwrap()
                && runs.iter().any(&stands)
            {
                return runs;
            }
        }
    }

    #[test]
    fn a_turn_of_yours_ending_begins_the_workflow_of_a_task_on_it_and_one_of_ghosts_does_not() {
        let home = tempfile::tempdir().unwrap();
        let flows = workflows::folder(home.path());
        std::fs::create_dir(&flows).unwrap();
        let ask = serde_json::json!({"steps": [
            {"ask": {"name": "go", "title": "Go?", "choices": ["Yes"]}}]});
        std::fs::write(flows.join("review.json"), ask.to_string()).unwrap();
        let task = serde_json::json!({"id": 1, "title": "Review each turn", "agent": "a",
            "prompt": "Look at {{event}}", "project": home.path(), "workflow": "review",
            "schedule": {"on": {"moment": "turn_ended", "here": false}}});
        let tasks = serde_json::json!({"tasks": [task]});
        std::fs::write(home.path().join("tasks.json"), tasks.to_string()).unwrap();
        let (tx, rx) = mpsc::channel();
        let server = Server::new(home.path().to_path_buf(), move |e| {
            let _ = tx.send(e);
        });
        let (mine, ghosts) = (SessionId::new("s1"), SessionId::new("s2"));
        server.shared.happened.by_ghost("a", &ghosts);
        server.shared.happened.end("a", &ghosts, "end_turn".into());
        server.shared.happened.end("a", &mine, "end_turn".into());
        let runs = until(&rx, |r| r.state == State::Asking);
        assert_eq!(runs.len(), 1, "one run, for the turn of yours");
        assert_eq!(runs[0].workflow, "review");
        assert!(runs[0].input.starts_with("Look at <event untrusted>"));
        assert!(
            runs[0]
                .input
                .contains("a session s1): its turn ended (end_turn)"),
            "{}",
            runs[0].input
        );
    }

    #[test]
    fn a_run_goes_step_by_step_waits_on_your_answer_and_pauses_where_a_step_fails() {
        let home = tempfile::tempdir().unwrap();
        let flows = workflows::folder(home.path());
        std::fs::create_dir(&flows).unwrap();
        std::fs::write(
            flows.join("release.json"),
            serde_json::json!({
                "project": home.path(),
                "steps": [
                    {"run": {"name": "notes", "command": "echo notes for {{input}}"}},
                    {"ask": {"name": "ship", "title": "Ship?", "choices": ["Ship", "Hold"], "stop": ["Hold"]}},
                    {"run": {"name": "tag", "command": "echo {{notes}} > tagged; exit 3"}},
                ],
            })
            .to_string(),
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let server = Server::new(home.path().to_path_buf(), move |e| {
            let _ = tx.send(e);
        });
        server.handle(Call::StartFlow {
            workflow: "release".into(),
            input: "v1.4".into(),
        });
        let runs = until(&rx, |r| r.state == State::Asking);
        assert_eq!(runs[0].done[0].result, "notes for v1.4\n");
        server.handle(Call::AnswerFlow {
            run: 1,
            choice: "Ship".into(),
        });
        let runs = until(&rx, |r| matches!(r.state, State::Paused(_)));
        assert!(matches!(&runs[0].state, State::Paused(why) if why.contains("“tag” failed")));
        assert_eq!(
            std::fs::read_to_string(home.path().join("tagged")).unwrap(),
            "notes for v1.4\n"
        );
        // Kept, as it stands, for the server after this one.
        Server::settle();
        let kept = workflows::load_runs(home.path()).unwrap().runs;
        assert_eq!(
            (kept[0].done.len(), &kept[0].done[1].result),
            (2, &"Ship".to_owned())
        );
        // A second run held back is stopped at the question.
        server.handle(Call::StartFlow {
            workflow: "release".into(),
            input: "v2".into(),
        });
        until(&rx, |r| r.id == 2 && r.state == State::Asking);
        server.handle(Call::AnswerFlow {
            run: 2,
            choice: "Hold".into(),
        });
        until(&rx, |r| r.id == 2 && matches!(r.state, State::Stopped(_)));
    }
}
