//! Ghost's tools for what runs on its own: the workflows and their runs
//! (`Call::Workflows`, `Call::StartFlow`), and the scheduled tasks
//! (`Event::Tasks` as a device that may only read hears it; `SetTask`,
//! `RunTask`, `DropTask`).

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{Tools, said};
use crate::{proto::Event, workflows};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct StartWorkflow {
    /// The workflow, by its name as `list_workflows` gives it.
    workflow: String,
    /// What its steps are given as `{{input}}`.
    #[serde(default)]
    input: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Change {
    /// It runs no more on its schedule until resumed.
    Pause,
    Resume,
    /// It runs now, whatever its schedule says.
    Run,
    /// It is taken away.
    Remove,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ChangeTask {
    /// The task, by its id as `list_tasks` gives it.
    id: u64,
    change: Change,
}

#[tool_router(router = flow_router, vis = "pub(super)")]
impl Tools {
    /// The workflows (your files in Ghost's `workflows` folder): each
    /// one's name, what it is for and its steps, or why its file does
    /// not read; and their runs, each one's id, input, state and the
    /// steps it has done.
    #[tool]
    async fn list_workflows(&self) -> Result<String, String> {
        let flows: Vec<_> = (workflows::load(&self.shared.home).into_iter())
            .map(|flow| match flow {
                Ok(f) => serde_json::json!({"name": f.name, "about": f.about,
                    "agent": f.agent, "project": f.project, "steps": f.steps}),
                Err((name, why)) => serde_json::json!({"name": name, "does_not_read": why}),
            })
            .collect();
        let runs = self.shared.runs.lock().expect("runs").clone();
        said(&serde_json::json!({"workflows": flows, "runs": runs.map(|r| r.runs)}))
    }

    /// Begin a run of a workflow with `input`: its id, whose state
    /// `list_workflows` gives. You are asked first.
    #[tool]
    async fn start_workflow(
        &self,
        Parameters(ask): Parameters<StartWorkflow>,
    ) -> Result<String, String> {
        let title = format!("Run the workflow “{}”", ask.workflow);
        self.allowed("start_workflow", title, &ask).await?;
        let run = self.shared.start_flow(ask.workflow, ask.input)?;
        Ok(format!("Run {run} began."))
    }

    /// The scheduled tasks: each one's id, title, prompt, agent, project,
    /// schedule, whether it is paused, and its last run. A webhook's
    /// address is not given.
    #[tool]
    async fn list_tasks(&self) -> Result<String, String> {
        let tasks = self.shared.tasks.lock().expect("tasks").clone()?;
        match Event::Tasks(Ok(tasks.tasks)).for_reader() {
            Some(Event::Tasks(Ok(tasks))) => said(&tasks),
            _ => Err("The tasks could not be told".into()),
        }
    }

    /// Pause a scheduled task, resume it, run it now or take it away. You
    /// are asked first.
    #[tool]
    async fn change_task(&self, Parameters(ask): Parameters<ChangeTask>) -> Result<String, String> {
        let task = (self.shared.tasks.lock().expect("tasks").as_ref())
            .map_err(Clone::clone)?
            .tasks
            .iter()
            .find(|t| t.id == ask.id)
            .cloned()
            .ok_or("No task has that id: list_tasks says which do")?;
        let doing = match ask.change {
            Change::Pause => "Pause",
            Change::Resume => "Resume",
            Change::Run => "Run",
            Change::Remove => "Remove",
        };
        let title = format!("{doing} the task “{}”", task.title);
        self.allowed("change_task", title, &ask).await?;
        match ask.change {
            Change::Pause | Change::Resume => {
                let paused = matches!(ask.change, Change::Pause);
                self.shared.set_task(crate::tasks::Task { paused, ..task });
            }
            Change::Run => self.shared.run(ask.id, str::to_owned),
            Change::Remove => self.shared.drop_task(ask.id),
        }
        Ok("Done.".into())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::super::{
        OUTSIDE,
        tests::{Client, answer_card, in_session},
    };

    /// Your tasks: a webhook's, and one each day.
    fn tasks(home: &std::path::Path) {
        let task = |id: u64, title: &str, schedule| {
            json!({"id": id, "title": title, "prompt": "Look", "agent": "a",
                "project": home, "schedule": schedule})
        };
        let tasks = json!({"tasks": [
            task(1, "On push", json!({"hook": {"token": "s3cret-t0ken", "signed": true}})),
            task(2, "Each morning", json!({"daily": {"minute": 540, "days": []}})),
        ]});
        std::fs::write(home.join("tasks.json"), tasks.to_string()).unwrap();
    }

    #[test]
    fn the_tasks_are_listed_without_a_webhooks_address_and_changed_as_you_allow() {
        let home = tempfile::tempdir().unwrap();
        tasks(home.path());
        let (server, said, mut client) = in_session(home.path(), home.path());
        client.call("list_tasks", &json!({}));
        let (listed, _) = client.result();
        assert!(listed.contains("Each morning") && listed.contains("On push"));
        assert!(!listed.contains("s3cret-t0ken"), "{listed}");
        client.call("change_task", &json!({"id": 2, "change": "pause"}));
        let title = answer_card(&server, &said, "once");
        assert_eq!(title, "Pause the task “Each morning”");
        assert_eq!(client.result(), ("Done.".to_owned(), false));
        let paused = (server
            .shared
            .tasks
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .tasks
            .iter())
        .any(|t| t.id == 2 && t.paused);
        assert!(paused);
        client.call("change_task", &json!({"id": 1, "change": "remove"}));
        answer_card(&server, &said, "no");
        assert!(client.result().1, "turned down, it stays");
        client.call("change_task", &json!({"id": 9, "change": "run"}));
        assert!(client.result().0.contains("No task has that id"));
        let mut outside = Client::new(home.path(), None);
        outside.call("change_task", &json!({"id": 2, "change": "resume"}));
        assert_eq!(outside.result(), (OUTSIDE.to_owned(), true));
    }

    #[test]
    fn a_workflow_is_listed_and_begun_as_you_allow_and_its_run_told() {
        let home = tempfile::tempdir().unwrap();
        let flows = crate::workflows::folder(home.path());
        std::fs::create_dir(&flows).unwrap();
        let ask = json!({"about": "Ship it", "steps": [
            {"ask": {"name": "go", "title": "Go?", "choices": ["Yes"]}}]});
        std::fs::write(flows.join("release.json"), ask.to_string()).unwrap();
        std::fs::write(flows.join("broken.json"), "{").unwrap();
        let (server, said, mut client) = in_session(home.path(), home.path());
        client.call("list_workflows", &json!({}));
        let (listed, _) = client.result();
        let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
        let names: Vec<_> = (listed["workflows"].as_array().unwrap().iter())
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["broken", "release"]);
        client.call(
            "start_workflow",
            &json!({"workflow": "release", "input": "v2"}),
        );
        assert_eq!(
            answer_card(&server, &said, "once"),
            "Run the workflow “release”"
        );
        assert_eq!(client.result(), ("Run 1 began.".to_owned(), false));
        for _ in 0..250 {
            client.call("list_workflows", &json!({}));
            if client.result().0.contains("\"asking\"") {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the run never waited on its question");
    }
}
