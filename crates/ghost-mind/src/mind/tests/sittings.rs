//! When work runs: sittings she sits down to or that run beside her day, how
//! many at once, in what order, and within what budget.

use super::*;

/// Her attention, with work that is something she sits down to.
fn sat_down_to() -> MindConfig {
    MindConfig {
        live: false,
        browse_minutes: [0, 0],
        work: crate::WorkConfig {
            runs: crate::Runs::Awake,
            ..crate::WorkConfig::default()
        },
        ..MindConfig::default()
    }
}

#[tokio::test]
async fn with_work_she_sits_down_to_a_sitting_is_put_down_when_she_looks_at_her_phone() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(sat_down_to(), t0);
    rig.think
        .then_call("note", serde_json::json!({"text": "first thing"}));
    rig.mind
        .undertake(
            "Read up on the history of Kuixinglou street.",
            None,
            None,
            &[],
            t0,
        )
        .unwrap();
    // She sits down to it.
    rig.mind.on_time(t0).unwrap();
    assert!(!rig.mind.working.is_empty());

    // A message is due while the sitting is still running: looking at it
    // waits, and the sitting is told to stop.
    rig.hear(dm("wen", "1", "are you there"), t0);
    let due = rig.mind.agenda.attend["t:wen"].due;
    rig.mind.on_time(due).unwrap();
    assert_eq!(rig.think.calls(), 0, "she has not looked yet");
    assert!(rig.mind.agenda.attend["t:wen"].due > due);

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"here"}]}"#);
    rig.until(|r| r.body.sent().len() == 1).await;
    let kinds = rig.kinds();
    let worked = kinds.iter().position(|k| *k == "worked").unwrap();
    let read = kinds.iter().position(|k| *k == "read").unwrap();
    assert!(
        worked < read,
        "the sitting ended before she read: {kinds:?}"
    );
    // The work is still hers, and still open.
    assert_eq!(
        rig.mind.ghost.memory().undertakings(true, 5).unwrap().len(),
        1
    );
    assert!(rig.mind.agenda.work.is_some());
}

/// Work she has taken on for herself, by its id. Returns the id.
fn take_on(rig: &mut Rig, brief: &str, directed: &[&str], now: DateTime<Utc>) -> String {
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let directed: Vec<String> = directed.iter().map(|d| (*d).to_owned()).collect();
    assert_eq!(
        rig.mind
            .undertake(brief, None, None, &directed, now)
            .unwrap(),
        None
    );
    let mut open = rig.mind.ghost.memory().undertakings(true, 20).unwrap();
    open.sort_by_key(|u| u.seq);
    open.last().unwrap().id.clone()
}

#[tokio::test]
async fn with_work_that_runs_always_a_sitting_starts_while_she_is_asleep() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    let gate = rig.think.hold(0);
    let id = take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        &[],
        t0,
    );
    rig.mind.on_time(t0).unwrap();
    assert_eq!(running(&rig), [id]);
    gate.notify_one();
}

#[tokio::test]
async fn with_work_she_sits_down_to_no_sitting_starts_while_she_is_asleep() {
    let t0 = at(3, 0);
    let mut rig = Rig::new(sat_down_to(), t0);
    take_on(
        &mut rig,
        "Read up on the history of Kuixinglou street.",
        &[],
        t0,
    );
    rig.mind.on_time(t0).unwrap();
    assert!(running(&rig).is_empty());
    let next = rig.mind.agenda.work.expect("she means to come back to it");
    assert!(next >= at(8, 30), "not before her morning: {next}");
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
        &[],
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
    let a = take_on(&mut rig, "Draw the east gate.", &[], t0);
    let b = take_on(&mut rig, "Draw the west gate.", &[], t0);
    let c = take_on(&mut rig, "Draw the north gate.", &[], t0);
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
async fn directed_work_starts_before_older_work() {
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
    let older = take_on(&mut rig, "Draw the east gate.", &[], t0);
    let directed = take_on(
        &mut rig,
        "Write today's date down.",
        &["write the date"],
        t0,
    );
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
            live: false,
            browse_minutes: [0, 0],
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
        &[],
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
            live: false,
            browse_minutes: [0, 0],
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
        &[],
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
async fn what_directed_work_comes_back_with_is_taken_to_them_in_her_sleep() {
    let t0 = at(3, 0);
    let mut rig = Rig::new(operated(), t0);
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // Boss wrote earlier without a directive: that waits for her morning.
    rig.hear(dm("boss", "1", "hello"), t0);
    let memory = rig.mind.ghost.memory();
    let boss = memory.person("t:boss").unwrap().unwrap();
    let place = memory.place("t:boss").unwrap().unwrap();
    assert_eq!(
        rig.mind
            .undertake(
                "Write today's date down.",
                Some(&boss),
                Some(&place),
                &["write the date down".to_owned()],
                t0,
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
    // them then, not in the morning.
    let told = rig.until(|r| r.body.sent().len() == 1).await;
    assert!(told < at(8, 30), "she told them at {told}");
    assert_eq!(rig.body.sent(), ["the twelfth"]);
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
}
