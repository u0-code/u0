//! A look made of tool calls: what she is offered, what she is told she can
//! do, and how a call she got wrong or a turn spent on thinking comes back to
//! her.

use super::*;

#[tokio::test]
async fn with_tools_she_answers_by_calling_them() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think.then_turn(
        "she sounds tired",
        &[
            ("say", serde_json::json!({"text": "long day?"})),
            ("say", serde_json::json!({"text": "go lie down"})),
            (
                "note",
                serde_json::json!({"about": "wen", "text": "works late on Thursdays"}),
            ),
        ],
    );
    rig.hear(dm("wen", "1", "just got home"), t0);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent(), ["long day?", "go lie down"]);
    assert!(rig.kinds().contains(&"noted"));

    // Everything she meant to do came in one turn, so one turn was all.
    let turns = rig.think.turns.lock().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(rig.think.calls(), 0, "nothing was asked for as JSON");
    // Who she is comes first and holds nothing that changes by the hour;
    // the hour comes after, with what is in front of her.
    assert!(turns[0].system.contains("You are Weiwei"));
    assert!(!turns[0].system.contains("# Right now"));
    let ghost_brain::Item::User { text } = &turns[0].items[0] else {
        panic!()
    };
    assert!(text.contains("# Right now") && text.contains("just got home"));
    assert!(text.find("# The conversation") < text.find("# Right now"));
    assert!(!text.contains(r#"{"do":"#), "no JSON format is described");
    let names: Vec<&str> = turns[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"say") && names.contains(&"recall"));
    // No way to search the web in this body, so it is not offered.
    assert!(!names.contains(&"search"));
    // Her thought is the reason in her journal.
    assert!(rig.reasons().iter().any(|r| r == "she sounds tired"));
}

#[tokio::test]
async fn a_call_she_gets_wrong_comes_back_to_her() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"message": "hi"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "hi"}))]);
    rig.hear(dm("wen", "1", "hey"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["hi"]);

    let turns = rig.think.turns.lock().unwrap();
    assert_eq!(turns.len(), 2);
    assert!(matches!(turns[1].items.last(),
        Some(ghost_brain::Item::Result { failed: true, output, .. })
            if output.contains("could not be used") && output.contains("text")));
}

#[tokio::test]
async fn she_can_search_her_memory_in_the_middle_of_answering() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think.then_turn(
        "",
        &[(
            "note",
            serde_json::json!({"about": "wen", "text": "her cat is called Dumpling"}),
        )],
    );
    rig.hear(dm("wen", "1", "my cat is called Dumpling"), t0);
    rig.until(|r| r.kinds().contains(&"noted")).await;

    let later = t0 + Duration::hours(3);
    rig.think
        .then_turn("", &[("recall", serde_json::json!({"query": "cat"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "Dumpling!"}))]);
    rig.hear(dm("wen", "2", "do you remember my cat's name?"), later);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["Dumpling!"]);

    let turns = rig.think.turns.lock().unwrap();
    let asked_again = turns.last().unwrap();
    assert!(matches!(asked_again.items.last(),
        Some(ghost_brain::Item::Result { name, output, failed: false, .. })
            if name == "recall" && output.contains("Dumpling")));
    // The turn after is the same look carried on: what she saw first is
    // still at the head of it, unchanged.
    let before = &turns[turns.len() - 2];
    assert_eq!(asked_again.items[0], before.items[0]);
}

#[tokio::test]
async fn a_thought_is_not_a_message() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    // It answers in plain text, the way it would without tools.
    rig.think.then_turn("nine, I think", &[]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "nine, I think"}))]);
    rig.hear(dm("wen", "1", "when does it close?"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["nine, I think"]);
    {
        let turns = rig.think.turns.lock().unwrap();
        assert!(matches!(turns[1].items.last(),
            Some(ghost_brain::Item::User { text }) if text.contains("nothing was sent")));
    }

    // Told once. Saying nothing a second time is taken as meant.
    let later = t0 + Duration::hours(1);
    rig.think.then_turn("not worth answering", &[]);
    rig.think.then_turn("still not", &[]);
    rig.hear(dm("wen", "2", "??"), later);
    rig.until(|r| r.reasons().iter().any(|x| x == "not worth answering"))
        .await;
    assert_eq!(rig.body.sent().len(), 1);
}

#[tokio::test]
async fn a_tool_described_anew_in_the_deployments_prompts_reaches_the_brain_without_a_restart() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    // The deployment's prompts, as a daemon given `[mind] prompts` reads.
    let deployment = tempfile::tempdir().unwrap();
    rig.mind.prompts = Prompts::load(Some(deployment.path()));
    let say = |rig: &Rig, turn: usize| {
        rig.think.turns.lock().unwrap()[turn]
            .tools
            .iter()
            .find(|t| t.name == "say")
            .cloned()
            .unwrap()
    };
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let shipped = Prompts::shipped();
    assert_eq!(say(&rig, 0).description, shipped.tool("say"));

    // While she runs, someone describes `say` differently.
    std::fs::create_dir_all(deployment.path().join("system")).unwrap();
    std::fs::write(
        deployment.path().join("system/tools.toml"),
        "[say]\ndescription = \"Say one thing.\"\n\n[say.parameters]\ntext = \"What you say.\"\n",
    )
    .unwrap();
    rig.hear(dm("wen", "2", "you there?"), t0 + Duration::minutes(30));
    rig.until(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;
    let now = say(&rig, 1);
    assert_eq!(now.description, "Say one thing.");
    let described = |argument: &str| now.parameters["properties"][argument]["description"].clone();
    assert_eq!(described("text"), "What you say.");
    // What the file does not mention keeps its built-in words.
    assert_eq!(
        described("reply_to"),
        shipped.parameter("say", "reply_to").unwrap()
    );
}

#[tokio::test]
async fn what_she_is_told_she_can_do_here_is_what_she_is_offered_here() {
    let t0 = at(14, 0);
    let section = "# What you can do here";
    let ability =
        |rig: &Rig, name: &str| rig.mind.prompts.store().ability(name).unwrap().to_owned();

    // Nothing beyond talking: nothing is said.
    let mut rig = Rig::with_tools(t0);
    rig.mind.cfg.work.web = false;
    rig.mind.cfg.work.enabled = false;
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let system = rig.think.turns.lock().unwrap()[0].system.clone();
    assert!(!system.contains(section), "{system}");
    assert!(system.contains("You cannot stand:") && system.contains("# Ground rules"));

    // The web, and a place that takes files: both, and nothing else.
    let mut rig = Rig::with_tools(t0);
    rig.mind.cfg.work.enabled = false;
    rig.mind.workshop.web.search = Some(ghost_work::tools::Source::Searxng {
        url: "http://127.0.0.1:1".into(),
    });
    takes_files(&mut rig, t0);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let system = rig.think.turns.lock().unwrap()[0].system.clone();
    assert!(system.contains(section), "{system}");
    for name in ["search", "fetch", "send_file"] {
        assert!(system.contains(&ability(&rig, name)), "{name} is not said");
    }
    for name in ["undertake", "shell"] {
        assert!(!system.contains(&ability(&rig, name)), "{name} is said");
    }
}

#[tokio::test]
async fn she_is_told_she_can_find_tools_and_is_offered_to_load_only_once_she_has_some() {
    let t0 = at(14, 0);
    let ability =
        |rig: &Rig, name: &str| rig.mind.prompts.store().ability(name).unwrap().to_owned();
    let folder = tempfile::tempdir().unwrap();
    let servers = |configured| {
        ghost_work::servers::Servers::new(
            ghost_work::servers::Config {
                registry: "http://127.0.0.1:1".into(),
                ..ghost_work::servers::Config::default()
            },
            folder.path().to_path_buf(),
            configured,
        )
    };
    let first_turn = |rig: &Rig| rig.think.turns.lock().unwrap()[0].clone();

    // None yet: finding them is work she can take on, and nothing is
    // there to load.
    let mut rig = Rig::with_tools(t0);
    rig.mind.workshop.servers = Some(servers(Vec::new()));
    rig.hear(dm("wen", "1", "can you look at a page for me?"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let turn = first_turn(&rig);
    assert!(
        turn.system.contains(&ability(&rig, "find_tools")),
        "{}",
        turn.system
    );
    assert!(!turn.tools.iter().any(|t| t.name == "load_tools"));
    // Installing belongs to work, not to a conversation.
    assert!(!turn.tools.iter().any(|t| t.name == "install"));

    // With one, it is listed in a line under `load_tools`.
    let mut rig = Rig::with_tools(t0);
    rig.mind.workshop.servers = Some(servers(vec![ghost_work::servers::Configured {
        name: "notes".into(),
        description: "Her notebook.".into(),
        command: Some("true".into()),
        args: Vec::new(),
        url: None,
        env: Vec::new(),
    }]));
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let turn = first_turn(&rig);
    let load = turn
        .tools
        .iter()
        .find(|t| t.name == "load_tools")
        .expect("load_tools is offered");
    assert!(
        load.description.contains("- notes: Her notebook."),
        "{}",
        load.description
    );
    assert!(turn.system.contains(&ability(&rig, "load_tools")));
}

#[tokio::test]
async fn what_happens_to_her_servers_of_tools_is_written_in_her_journal_by_her() {
    let t0 = at(14, 0);
    let folder = tempfile::tempdir().unwrap();
    let servers = ghost_work::servers::Servers::new(
        ghost_work::servers::Config::default(),
        folder.path().to_path_buf(),
        Vec::new(),
    );
    servers.know(vec![ghost_core::memory::Installed {
        seq: 1,
        at: t0,
        name: "io.github.someone/notes".into(),
        version: "1.0.0".into(),
        description: "A notebook.".into(),
        source: "npm:notes@1.0.0".into(),
        owner: Some("github.com/someone".into()),
        how: ghost_core::journal::Launch::default(),
        tools: vec!["write".into()],
        hash: "h".into(),
    }]);
    let mut rig = Rig::new(MindConfig::default(), t0);
    rig.mind.workshop.servers = Some(servers.clone());
    // As a sitting of work would, off to the side of her.
    servers
        .uninstall("notes", "it never kept anything")
        .unwrap();
    assert!(!rig.kinds().contains(&"uninstalled"));
    rig.mind.on_time(t0).unwrap();
    assert!(rig.kinds().contains(&"uninstalled"));
    // Once.
    rig.mind.on_time(t0).unwrap();
    assert_eq!(
        rig.kinds().iter().filter(|k| **k == "uninstalled").count(),
        1
    );
}

#[tokio::test]
async fn she_is_told_she_can_add_skills_and_sees_each_one_she_has_as_a_line() {
    let t0 = at(14, 0);
    let folder = tempfile::tempdir().unwrap();
    let skills = || {
        ghost_work::skills::Skills::new(
            ghost_work::skills::Config {
                search: "http://127.0.0.1:1/search".into(),
                ..ghost_work::skills::Config::default()
            },
            folder.path().to_path_buf(),
        )
    };
    let first_turn = |rig: &Rig| rig.think.turns.lock().unwrap()[0].clone();

    // None yet: adding one is work she can take on, and there is nothing
    // to read.
    let mut rig = Rig::with_tools(t0);
    rig.mind.workshop.skills = Some(skills());
    rig.hear(dm("wen", "1", "can you make me a slide deck?"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let turn = first_turn(&rig);
    let ability = rig.mind.prompts.store().ability("find_skills").unwrap();
    assert!(turn.system.contains(ability), "{}", turn.system);
    assert!(!turn.tools.iter().any(|t| t.name == "use_skill"));
    // Installing belongs to work, not to a conversation.
    assert!(!turn.tools.iter().any(|t| t.name == "install_skill"));

    // With one, it is a line under `use_skill`.
    std::fs::create_dir_all(folder.path().join("slides")).unwrap();
    std::fs::write(
        folder.path().join("slides/SKILL.md"),
        "---\nname: slides\ndescription: Build a slide deck.\n---\nStart with the outline.\n",
    )
    .unwrap();
    let have = skills();
    have.know(vec![ghost_core::memory::Skill {
        seq: 1,
        at: t0,
        name: "slides".into(),
        source: "someone/skills".into(),
        path: "skills/slides".into(),
        sha: "a".repeat(40),
        hash: ghost_work::skills::guard::hash(&folder.path().join("slides")).unwrap(),
        description: "Build a slide deck.".into(),
    }]);
    let mut rig = Rig::with_tools(t0);
    rig.mind.workshop.skills = Some(have);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    let turn = first_turn(&rig);
    let using = turn
        .tools
        .iter()
        .find(|t| t.name == "use_skill")
        .expect("use_skill is offered");
    assert!(
        using.description.contains("- slides: Build a slide deck."),
        "{}",
        using.description
    );
}

#[tokio::test]
async fn a_look_that_spends_its_turns_on_looking_things_up_still_ends_in_an_answer() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    let steps = usize::from(rig.mind.cfg.talk_steps);
    // Every turn but the last goes to searching her memory, which always
    // asks for another turn.
    for _ in 0..steps - 1 {
        rig.think
            .then_turn("", &[("recall", serde_json::json!({"query": "tea"}))]);
    }
    rig.think.then_turn(
        "",
        &[("say", serde_json::json!({"text": "found nothing, sorry"}))],
    );
    rig.hear(dm("wen", "1", "what was that teahouse called?"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;

    let turns = rig.think.turns.lock().unwrap();
    assert_eq!(turns.len(), steps);
    let last = turns.last().unwrap();
    // On the last turn nothing that brings something back is offered, and
    // she is told why.
    assert!(last.tools.iter().all(|t| t.name != "recall"));
    assert!(last.tools.iter().any(|t| t.name == "say"));
    assert!(matches!(last.items.last(),
        Some(ghost_brain::Item::User { text }) if text.contains("last turn")));
    // Earlier turns still had it.
    assert!(turns[0].tools.iter().any(|t| t.name == "recall"));
}
