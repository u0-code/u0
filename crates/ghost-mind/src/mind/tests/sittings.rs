//! When work runs: beside her day, how many sittings at once, in what
//! order, and within what budget.

use super::*;

/// A place on the console, where whoever runs her is waited on.
fn at_the_console() -> ghost_core::memory::PlaceState {
    ghost_core::memory::PlaceState {
        key: "console:fox".into(),
        channel: crate::CONSOLE.into(),
        id: "fox".into(),
        kind: PlaceKind::Direct,
        name: None,
        read_upto: 0,
        last_in_seq: 0,
        last_in_at: None,
        last_out_seq: 0,
        last_out_at: None,
        unread: 0,
        unread_addressed: 0,
        unread_directed: 0,
        quiet_until: None,
    }
}

/// Work she has taken on, for herself or as asked at a terminal, by its
/// id. Returns the id.
fn take_on(rig: &mut Rig, brief: &str, at_terminal: bool, now: DateTime<Utc>) -> String {
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let console = at_the_console();
    let place = at_terminal.then_some(&console);
    assert_eq!(
        rig.mind.undertake(brief, None, place, None, now).unwrap(),
        None
    );
    let mut open = rig.mind.ghost.memory().undertakings(true, 20).unwrap();
    open.sort_by_key(|u| u.seq);
    open.last().unwrap().id.clone()
}

#[tokio::test]
async fn a_sitting_starts_while_she_is_asleep() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    let gate = rig.think.hold(0);
    let id = take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        false,
        t0,
    );
    rig.mind.on_time(t0).unwrap();
    assert_eq!(running(&rig), [id]);
    gate.notify_one();
}

#[tokio::test]
async fn looking_at_her_phone_does_not_stop_a_running_sitting() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then_call("note", serde_json::json!({"text": "first thing"}));
    let gate = rig.think.hold(0);
    let id = take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        false,
        t0,
    );
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;

    // A message comes in while the sitting is in the middle of a step: she
    // looks at it when she would anyway, and the sitting goes on.
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"here"}]}"#);
    rig.hear(dm("wen", "1", "are you there"), t0);
    let due = rig.mind.agenda.attend["t:wen"].due;
    rig.mind.on_time(due).unwrap();
    assert!(
        rig.mind.thinking.contains_key("t:wen"),
        "she is looking at it"
    );
    assert_eq!(running(&rig), std::slice::from_ref(&id));
    assert!(
        !rig.mind.working[&id]
            .pause
            .load(std::sync::atomic::Ordering::Relaxed),
        "the sitting was not asked to stop"
    );
    // And she is told, in the rest of her day, that it is running.
    rig.meanwhile(|r| r.think.calls() == 1).await;
    assert!(
        rig.think
            .last_prompt()
            .contains(rig.mind.prompts.text("work_running")),
        "{}",
        rig.think.last_prompt()
    );
    gate.notify_one();
    rig.until(|r| r.body.sent().len() == 1).await;
}

#[tokio::test]
async fn two_pieces_of_work_run_at_once_and_a_third_waits() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    assert_eq!(rig.mind.cfg.work.at_once, 2);
    let first = rig.think.hold(0);
    let second = rig.think.hold(1);
    let a = take_on(&mut rig, "Draw the east gate.", false, t0);
    let b = take_on(&mut rig, "Draw the west gate.", false, t0);
    let c = take_on(&mut rig, "Draw the north gate.", false, t0);
    rig.mind.on_time(t0).unwrap();
    let mut both = [a.clone(), b.clone()];
    both.sort();
    assert_eq!(running(&rig), both, "the two that waited longest");
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;

    // The first finishes its sitting; the third takes its place, while the
    // second is still in the middle of a step.
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Drawn."}),
    );
    // The third is held at its first step, so that it is seen running.
    let third = rig.think.hold(1);
    first.notify_one();
    rig.beside_until(|r| r.mind.working.contains_key(&c)).await;
    assert!(!rig.mind.working.contains_key(&a));
    assert!(rig.mind.working.contains_key(&b));
    second.notify_one();
    third.notify_one();
}

#[tokio::test]
async fn work_asked_for_at_a_terminal_starts_before_older_work() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            work: crate::WorkConfig {
                at_once: 1,
                ..crate::WorkConfig::default()
            },
            ..operated()
        },
        t0,
    );
    let gate = rig.think.hold(0);
    let older = take_on(&mut rig, "Draw the east gate.", false, t0);
    let directed = take_on(&mut rig, "Write today's date down.", true, t0);
    assert!(
        rig.mind
            .ghost
            .memory()
            .undertaking(&directed)
            .unwrap()
            .unwrap()
            .directed
    );
    rig.mind.on_time(t0).unwrap();
    assert_eq!(running(&rig), [directed]);
    assert!(!rig.mind.working.contains_key(&older));
    gate.notify_one();
}

#[tokio::test]
async fn an_unfinished_work_starts_its_next_sitting_right_after_the_last_one_ends() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            channels: [(
                "t".to_owned(),
                crate::ChannelConfig {
                    browse_minutes: [0, 0],
                    ..crate::ChannelConfig::default()
                },
            )]
            .into(),
            live: false,
            work: crate::WorkConfig {
                sitting_steps: 1,
                ..crate::WorkConfig::default()
            },
            ..MindConfig::default()
        },
        t0,
    );
    take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        false,
        t0,
    );
    rig.think
        .then_call("note", serde_json::json!({"text": "the street is old"}));
    rig.think
        .then_call("note", serde_json::json!({"text": "older than the wall"}));
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It is older than the wall."}),
    );
    // Three sittings of one step each, one after another, with no time
    // between them: none of them waited for a rest.
    rig.pump(t0).await;
    assert_eq!(count(&rig, "worked"), 2, "{:?}", rig.kinds());
    assert!(rig.kinds().contains(&"concluded"));
}

#[tokio::test]
async fn the_hourly_budget_bounds_how_many_sittings_start() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            channels: [(
                "t".to_owned(),
                crate::ChannelConfig {
                    browse_minutes: [0, 0],
                    ..crate::ChannelConfig::default()
                },
            )]
            .into(),
            live: false,
            calls_per_hour: 2,
            work: crate::WorkConfig {
                sitting_steps: 1,
                ..crate::WorkConfig::default()
            },
            ..MindConfig::default()
        },
        t0,
    );
    take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        false,
        t0,
    );
    for n in 0..6 {
        rig.think
            .then_call("note", serde_json::json!({"text": format!("note {n}")}));
    }
    // Work that would go straight on stops where the hour's calls run out.
    rig.pump(t0).await;
    assert_eq!(count(&rig, "worked"), 2, "{:?}", rig.kinds());
    assert!(running(&rig).is_empty());
    assert!(!rig.kinds().contains(&"concluded"));
    let next = rig.mind.agenda.work.expect("she means to come back to it");
    assert!(next > t0);
    // Within the hour it still waits; after it, it goes on.
    rig.pump(t0 + Duration::minutes(30)).await;
    assert_eq!(count(&rig, "worked"), 2);
    rig.pump(t0 + Duration::minutes(61)).await;
    assert_eq!(count(&rig, "worked"), 4);
}

#[tokio::test]
async fn what_work_asked_for_at_a_terminal_comes_back_with_is_taken_there_in_her_sleep() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    rig.online(crate::CONSOLE, t0);
    // Asked at the console in the middle of her night, and answered then.
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"on it"}]}"#);
    rig.hear_on(crate::CONSOLE, dm("fox", "1", "write the date down"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let memory = rig.mind.ghost.memory();
    let fox = memory.person("console:fox").unwrap().unwrap();
    let place = memory.place("console:fox").unwrap().unwrap();
    assert_eq!(
        rig.mind
            .undertake(
                "Write today's date down.",
                Some(&fox),
                Some(&place),
                None,
                t0
            )
            .unwrap(),
        None
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It is the twelfth."}),
    );
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"the twelfth"}]}"#);
    // The work runs in her sleep, and what it came back with is taken to
    // the console then, not in the morning.
    let done = rig.until(|r| r.kinds().contains(&"concluded")).await;
    let told = rig.until(|r| r.body.sent().len() == 2).await;
    assert!(told < at(8, 30), "she told them at {told}");
    assert_eq!(rig.body.sent(), ["on it", "the twelfth"]);
    let meant = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["intended"], 0)
        .unwrap();
    assert!(matches!(
        &meant[0].event,
        Event::Intended { directed: true, .. }
    ));
    // Someone waits at a terminal, so it is meant for the moment the work
    // is done, not a minute after.
    let Event::Intended { due, .. } = &meant[0].event else {
        unreachable!()
    };
    assert!(*due <= done, "meant for {due}, done at {done}");
}
