//! The loop: how work ends, what reaches it while it runs, and how it is
//! picked up again.

use super::*;

#[tokio::test]
async fn work_ends_when_it_says_it_is_finished() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
        calls("shell", json!({"command": "cat hi.txt"})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "wrote it and read it back"}),
        ),
    ];
    let (worker, brain) = worker(dir.path(), script, Limits::default());
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);
    assert_eq!(report.summary, "wrote it and read it back");
    assert_eq!(report.steps, 3);
    assert!(report.usage.input >= 300);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("hi.txt")).unwrap(),
        "hi\n"
    );
    // Its word is taken: nothing was sent back, and nobody was asked to
    // read it over. Three steps, three requests.
    let seen = brain.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    // How to finish was said at the start, in the instructions.
    assert!(seen[0].system.contains("Nobody checks after you"));
    // The log has one line per step.
    assert_eq!(worker.undertaking.logged().len(), 3);
}

#[tokio::test]
async fn each_step_is_logged_with_what_was_said_called_and_returned_and_reads_back_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = calls("write", json!({"path": "hi.txt", "content": "hi\n"}));
    first.text = "writing it first:\n\n| file | lines |\n|---|---|\n| hi.txt | 1 |".into();
    let script = vec![
        first,
        calls(
            "shell",
            json!({"command": "printf 'line %s\\n' $(seq 1 400)"}),
        ),
        calls("done", json!({"outcome": "finished", "summary": "there"})),
    ];
    let (worker, _) = worker(dir.path(), script, Limits::default());
    worker.run().await;

    let db = state_db(dir.path());
    let read = crate::undertaking::steps(&db, "main", 0, 10)
        .unwrap()
        .unwrap();
    assert!(!read.more);
    let steps = read.steps;
    assert_eq!(steps.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 2, 3]);
    // What was said is kept as it was written, a table and all.
    assert_eq!(
        steps[0].said,
        "writing it first:\n\n| file | lines |\n|---|---|\n| hi.txt | 1 |"
    );
    assert!(steps[0].ms.is_some() && steps[0].at.is_some());
    let write = &steps[0].calls[0];
    assert_eq!(write["name"], "write");
    assert_eq!(write["args"]["path"], "hi.txt");
    assert_eq!(write["failed"], false);
    // A long output keeps its head, and says how long it was.
    let shell = &steps[1].calls[0];
    let out = shell["out"].as_str().unwrap();
    assert!(out.starts_with("line 1\n"), "{out}");
    assert_eq!(shell["cut"], true);
    assert!(shell["chars"].as_u64().unwrap() > out.chars().count() as u64);

    // After a step, and a step at a time.
    let rest = crate::undertaking::steps(&db, "main", 1, 1)
        .unwrap()
        .unwrap();
    assert_eq!(rest.steps.iter().map(|s| s.n).collect::<Vec<_>>(), [2]);
    assert!(rest.more);
    assert!(
        crate::undertaking::steps(&db, "nothing", 0, 10)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn something_added_mid_work_reaches_the_worker_before_its_next_step() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("shell", json!({"command": "echo first"})),
        calls("done", json!({"outcome": "gave_up", "summary": "enough"})),
    ];
    let (worker, brain) = worker(dir.path(), script, Limits::default());
    // Added by someone with their own handle on the same database.
    Undertaking::open(&state_db(dir.path()), "main", None)
        .unwrap()
        .add("also make it say hi twice")
        .unwrap();
    worker.run().await;
    let seen = brain.seen.lock().unwrap();
    assert!(matches!(seen[0].items.last(), Some(Item::User { text })
        if text.contains("also make it say hi twice") && text.contains("does not replace it")));
    assert!(
        worker
            .undertaking
            .state()
            .task_text()
            .ends_with("Added later: also make it say hi twice")
    );
}

#[test]
fn a_transcript_cut_off_mid_step_is_made_whole() {
    let call = |id: &str| Call {
        id: id.into(),
        name: "shell".into(),
        args: json!({"command": "make"}),
    };
    let mut items = vec![
        Item::user("task"),
        Item::Assistant {
            text: String::new(),
            calls: vec![call("a"), call("b"), call("c")],
            carry: None,
        },
        Item::result("a", "shell", "ok", false),
    ];
    mend(&mut items, &Prompts::shipped());
    assert_eq!(items.len(), 5);
    // The first call without a result may have begun; the one after it
    // cannot have, and is safe to run.
    assert!(
        matches!(&items[3], Item::Result { id, failed: true, output, .. }
        if id == "b" && output.contains("what it did is unknown"))
    );
    assert!(
        matches!(&items[4], Item::Result { id, failed: true, output, .. }
        if id == "c" && output.contains("was not run"))
    );
    // Nothing to do when it is already whole.
    let before = items.clone();
    mend(&mut items, &Prompts::shipped());
    assert_eq!(items, before);
}

#[tokio::test]
async fn talking_without_acting_is_nudged_then_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        says("I would write the file."),
        says("Then check it."),
        says("That is the plan."),
    ];
    let (worker, brain) = worker(dir.path(), script, Limits::default());
    assert_eq!(worker.run().await.ended, Ended::Stalled);
    let seen = brain.seen.lock().unwrap();
    assert!(matches!(&seen[1].items[2], Item::User { text } if text.contains("called no tool")));
}

#[tokio::test]
async fn a_question_stops_the_work_until_it_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("ask", json!({"question": "plain text or markdown?"})),
        // After the answer.
        calls("write", json!({"path": "out.md", "content": "# hi\n"})),
        calls(
            "done",
            json!({"outcome": "blocked", "summary": "enough", "files": ["/x/out.md"]}),
        ),
    ];
    let (mut first, brain) = worker(dir.path(), script, Limits::default());
    first.tools = first.tools.with(tools::Ask::new(first.undertaking.clone()));
    let report = first.run().await;
    assert_eq!(report.ended, Ended::Waiting);
    assert_eq!(report.asking.as_deref(), Some("plain text or markdown?"));
    assert_eq!(report.steps, 1);
    // Picked up with no answer, it is still waiting, and asks the
    // model for nothing.
    assert_eq!(first.run().await.ended, Ended::Waiting);
    assert_eq!(brain.seen.lock().unwrap().len(), 1);

    first.undertaking.add("markdown").unwrap();
    let report = first.run().await;
    assert_eq!(report.ended, Ended::Blocked);
    assert_eq!(report.asking, None);
    assert_eq!(report.files, ["/x/out.md"]);
    // The answer was in front of it when it went on.
    let seen = brain.seen.lock().unwrap();
    assert!(seen[1].items.iter().any(|item| matches!(item,
        Item::User { text } if text.contains("markdown") && text.contains("added to the task"))));
}

#[tokio::test]
async fn a_question_keeps_what_answering_it_takes_until_it_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        // A choice with nothing to choose from is sent back.
        calls(
            "ask",
            json!({"question": "which colour?", "kind": "choose"}),
        ),
        // Options alone are a choice.
        calls(
            "ask",
            json!({"question": "which colour?", "options": ["red", " blue ", ""]}),
        ),
        calls("done", json!({"outcome": "finished", "summary": "blue"})),
    ];
    let (mut first, brain) = worker(dir.path(), script, Limits::default());
    first.tools = first.tools.with(tools::Ask::new(first.undertaking.clone()));
    let report = first.run().await;
    assert_eq!(report.ended, Ended::Waiting);
    assert_eq!(report.steps, 2);
    let state = first.undertaking.state();
    assert_eq!(state.waiting.as_deref(), Some("which colour?"));
    assert_eq!(
        state.waiting_kind,
        Some(crate::undertaking::AskKind::Choose)
    );
    assert_eq!(state.waiting_options, ["red", "blue"]);
    assert!(
        brain.seen.lock().unwrap()[1]
            .items
            .iter()
            .any(|item| matches!(item,
        Item::Result { output, failed: true, .. } if output.contains("at least two")))
    );

    first.undertaking.add("blue").unwrap();
    assert_eq!(first.run().await.ended, Ended::Finished);
    let state = first.undertaking.state();
    assert_eq!((state.waiting, state.waiting_kind), (None, None));
    assert!(state.waiting_options.is_empty());
}

#[tokio::test]
async fn a_question_says_only_a_kind_there_is_and_options_only_for_a_choice() {
    let dir = tempfile::tempdir().unwrap();
    let (worker, _) = worker(dir.path(), Vec::new(), Limits::default());
    let ask = tools::Ask::new(worker.undertaking.clone());
    let unknown = ask.call(&json!({"question": "q", "kind": "guess"})).await;
    assert!(unknown.failed);
    let stray = ask
        .call(&json!({"question": "q", "kind": "file", "options": ["a", "b"]}))
        .await;
    assert!(stray.failed);
    assert_eq!(worker.undertaking.state().waiting, None);
    let file = ask
        .call(&json!({"question": "send me the contract", "kind": "file"}))
        .await;
    assert!(!file.failed);
    assert_eq!(
        worker.undertaking.state().waiting_kind,
        Some(crate::undertaking::AskKind::File)
    );
}

#[tokio::test]
async fn a_paused_run_picks_up_where_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![calls("shell", json!({"command": "echo one"}))];
    let (first, _) = worker(
        dir.path(),
        script,
        Limits {
            max_steps: 1,
            ..Limits::default()
        },
    );
    assert_eq!(first.run().await.ended, Ended::OutOfSteps);

    let script = vec![calls(
        "done",
        json!({"outcome": "gave_up", "summary": "no"}),
    )];
    let (second, brain) = worker(dir.path(), script, Limits::default());
    let report = second.run().await;
    assert_eq!(report.ended, Ended::GaveUp);
    assert_eq!(report.steps, 2, "totals run across both");
    // It was shown the transcript it left, not a blank page.
    assert_eq!(brain.seen.lock().unwrap()[0].items.len(), 3);
}

/// A tool that takes a while, during which something happens outside
/// the work: what the closure does.
struct Meanwhile(Box<dyn Fn() + Send + Sync>);

#[async_trait]
impl Tool for Meanwhile {
    fn name(&self) -> &'static str {
        "meanwhile"
    }
    fn spec(&self, _: &Store, _: Loop) -> ToolSpec {
        tool::spec("meanwhile", "Wait a moment.", json!({}), &[])
    }
    async fn call(&self, _: &Value) -> tool::Outcome {
        (self.0)();
        tool::Outcome::ok("waited")
    }
}

/// What happens in `meanwhile` happens from another handle on the
/// same record, while the worker's first step is running.
fn while_it_runs(
    dir: &std::path::Path,
    meanwhile: impl Fn(&Undertaking) + Send + Sync + 'static,
) -> Meanwhile {
    let db = state_db(dir);
    Meanwhile(Box::new(move || {
        meanwhile(&Undertaking::open(&db, "main", None).unwrap());
    }))
}

#[tokio::test]
async fn words_added_while_a_step_runs_reach_the_worker_at_its_next_step() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("meanwhile", json!({})),
        calls("done", json!({"outcome": "gave_up", "summary": "enough"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    worker.tools = worker.tools.with(while_it_runs(dir.path(), |outside| {
        outside.add("only the weekday hours, after all").unwrap();
    }));
    worker.run().await;
    let seen = brain.seen.lock().unwrap();
    assert!(!seen[0].items.iter().any(|item| matches!(item,
        Item::User { text } if text.contains("weekday hours"))));
    assert!(matches!(seen[1].items.last(), Some(Item::User { text })
        if text.contains("only the weekday hours, after all")));
}

#[tokio::test]
async fn work_stopped_from_outside_ends_as_dropped_after_its_step_and_keeps_what_it_kept() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        Step {
            calls: vec![
                Call {
                    id: "k".into(),
                    name: "keep".into(),
                    args: json!({"file": "PLACEHOLDER", "note": "a pixel"}),
                },
                Call {
                    id: "w".into(),
                    name: "meanwhile".into(),
                    args: json!({}),
                },
            ],
            stop: Stop::Calls,
            ..says("")
        },
        calls("write", json!({"path": "late.txt", "content": "too late"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    let file = worker.undertaking.keep(&[ghost_brain::Picture {
        mime: "image/png".into(),
        bytes: tools::testing::PIXEL.to_vec(),
    }])[0]
        .clone();
    brain.steps.lock().unwrap()[0].calls[0].args["file"] = json!(file);
    worker.tools = worker
        .tools
        .with(tools::Keep::new(worker.undertaking.clone()))
        .with(while_it_runs(dir.path(), |outside| {
            assert!(outside.stop(), "it was still going");
        }));
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Dropped);
    assert_eq!(report.steps, 1, "nothing after the step it was on");
    assert_eq!(brain.seen.lock().unwrap().len(), 1);
    assert!(!dir.path().join("late.txt").exists());
    let state = worker.undertaking.state();
    assert_eq!(state.finish.unwrap().outcome, Outcome::Dropped);
    assert_eq!(state.kept.len(), 1, "what it kept is still on record");
    // Picked up again, it does not start over.
    assert_eq!(worker.run().await.ended, Ended::Dropped);
    assert!(!worker.undertaking.stop(), "it had already ended");
}

#[tokio::test]
async fn work_with_a_folder_writes_there_and_elsewhere_only_in_a_file_whoever_asked_named() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder");
    let repo = dir.path().join("their-repo");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    let named = repo.join("notes").join("wanted.md");
    let script = vec![
        // Beside what it was asked to look at: refused.
        calls(
            "write",
            json!({"path": repo.join("counts.md").display().to_string(), "content": "x"}),
        ),
        // The file they named: written.
        calls(
            "write",
            json!({"path": named.display().to_string(), "content": "wanted\n"}),
        ),
        // In its folder: written.
        calls(
            "write",
            json!({"path": "counts.md", "content": "| a | 1 |\n"}),
        ),
        calls("done", json!({"outcome": "finished", "summary": "counted"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    worker
        .undertaking
        .update(|s| s.dir = Some(folder.display().to_string()));
    let asked = format!(
        "Count the files.\n\nWhat Fox wrote, word for word:\n> count the files in {} and put the note in {}",
        repo.display(),
        named.display()
    );
    worker.undertaking.update(|s| s.additions.push(asked));
    let machine: Arc<dyn crate::Machine> =
        crate::Machine::at(&Local::new(dir.path()), &folder).unwrap();
    worker.tools = tools::standard(machine, &worker.undertaking, &worker.prompts);
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let turns = brain.turns_of(PARENT);
    assert!(result_of(&turns[1], "write").contains("outside the work's folder"));
    assert!(!repo.join("counts.md").exists());
    assert_eq!(std::fs::read_to_string(&named).unwrap(), "wanted\n");
    assert_eq!(
        std::fs::read_to_string(folder.join("counts.md")).unwrap(),
        "| a | 1 |\n"
    );
}
