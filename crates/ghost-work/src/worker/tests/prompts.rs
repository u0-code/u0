//! What the worker is told comes from the prompt files, and with nothing
//! overridden is word for word what it was told before.

use super::*;

/// Compare `got` with what was recorded under `name`, or record it when
/// `GHOST_RECORD` is set.
fn compare(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/requests")
        .join(format!("{name}.txt"));
    if std::env::var_os("GHOST_RECORD").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    if want != got {
        let at = want
            .lines()
            .zip(got.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(want.lines().count().min(got.lines().count()));
        panic!(
            "{name} differs from what was recorded, first at line {}:\nrecorded: {:?}\nnow:      {:?}",
            at + 1,
            want.lines().nth(at),
            got.lines().nth(at)
        );
    }
}

#[tokio::test]
async fn a_tool_described_anew_in_tools_toml_reaches_work_already_running() {
    let dir = tempfile::tempdir().unwrap();
    // The work writes the file itself, between its first step and its
    // second, as a person editing it would.
    let script = vec![
        calls(
            "write",
            json!({
                "path": "prompts/system/tools.toml",
                "content": "[shell]\ndescription = \"Run it.\"\n\n[shell.parameters]\ncommand = \"What to run.\"\n",
            }),
        ),
        calls("done", json!({"outcome": "finished", "summary": "ok"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    worker.prompts = Prompts::load(Some(&dir.path().join("prompts"))).shared();
    worker.run().await;
    let seen = brain.seen.lock().unwrap();
    let shell = |turn: &Turn| {
        turn.tools
            .iter()
            .find(|t| t.name == "shell")
            .cloned()
            .unwrap()
    };
    let shipped = Prompts::shipped();
    assert_eq!(
        shell(&seen[0]).description,
        shipped.store().tool("shell", Loop::Work)
    );
    let now = shell(&seen[1]);
    assert_eq!(now.description, "Run it.");
    assert_eq!(
        now.parameters["properties"]["command"]["description"],
        "What to run."
    );
    // What the file does not mention keeps its built-in words.
    assert_eq!(
        now.parameters["properties"]["timeout"]["description"],
        shipped
            .store()
            .parameter("shell", "timeout", Loop::Work)
            .unwrap()
    );
}

#[tokio::test]
async fn what_a_worker_is_told_comes_from_the_files_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let prompts = dir.path().join("elsewhere");
    std::fs::create_dir_all(prompts.join("system/work/texts")).unwrap();
    std::fs::write(
        prompts.join("system/work/work.md"),
        "{{who}}Work here: {{machine}}. {{budget}}",
    )
    .unwrap();
    std::fs::write(
        prompts.join("system/work/texts/sitting.toml"),
        "no_call = \"Call a tool.\"\nbudget = \"{{steps}} steps.\"\n",
    )
    .unwrap();
    let script = vec![
        says("I would."),
        calls("done", json!({"outcome": "finished", "summary": "ok"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    worker.prompts = Prompts::load(Some(&prompts)).shared();
    worker.run().await;
    let seen = brain.seen.lock().unwrap();
    assert_eq!(seen[0].system, "Work here: a test directory. 200 steps.");
    assert!(matches!(seen[1].items.last(), Some(Item::User { text }) if text == "Call a tool."));
}

#[tokio::test]
async fn with_nothing_overridden_a_worker_asks_exactly_what_it_asked_before() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        says("I will write it."),
        calls("write", json!({"path": "hi.txt", "content": "hi\n"})),
        calls("shell", json!({"command": "cat hi.txt"})),
        calls("delegate", json!({"task": "say what is in hi.txt"})),
        calls("wait", json!({"seconds": 10})),
        calls("note", json!({"text": "hi.txt holds hi"})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "wrote it"}),
        ),
    ];
    let limits = Limits {
        max_steps: 6,
        ..Limits::default()
    };
    let (mut worker, brain) = worker(dir.path(), script, limits);
    brain.part(
        "say what is in hi.txt",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "it says hi"}),
        )],
    );
    with_parts(&mut worker, dir.path(), None);
    let u = worker.undertaking.clone();
    worker.tools = tools::with_web(worker.tools.clone(), &tools::WebConfig::default())
        .with(tools::Keep::new(u.clone()))
        .with(tools::Tell::new(u.clone()))
        .with(tools::Ask::new(u.clone()));
    worker.run().await;
    let mut out = String::new();
    for turn in brain.seen.lock().unwrap().iter() {
        out.push_str(&format!(
            "system: {}\nitems: {:#?}\ntools: {:#?}\n\n",
            turn.system, turn.items, turn.tools
        ));
    }
    let words = Prompts::shipped();
    let mut state = u.state();
    state.steps = 3;
    out.push_str(&format!(
        "opening: {}\n\ncleared: {}\n\n",
        opening_with(&words, &state, false),
        opening_with(&words, &state, true)
    ));
    let call = |id: &str| Call {
        id: id.into(),
        name: "shell".into(),
        args: json!({"command": "make"}),
    };
    let mut items = vec![Item::Assistant {
        text: String::new(),
        calls: vec![call("a"), call("b")],
        carry: None,
    }];
    mend(&mut items, &words);
    out.push_str(&format!("mended: {items:#?}\n\n"));
    for said in ["no_call", "cut_off", "thought_out", "repeating", "handoff"] {
        out.push_str(&format!("nudge: {}\n", words.text(said)));
    }
    compare("worker", &out);
}
