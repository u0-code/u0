//! Work in hand: what it tells her while it runs, steering it, stopping it,
//! and how it shows in her context and to a console.

use super::*;

fn reported(rig: &Rig) -> Vec<(String, String, Vec<String>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["reported"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Reported { id, text, files } => Some((id, text, files)),
            _ => None,
        })
        .collect()
}

/// The moment she next glances at the sitting she is at.
fn glance(rig: &Rig) -> DateTime<Utc> {
    assert!(!rig.mind.working.is_empty(), "she is at a sitting");
    rig.mind.agenda.work.expect("she means to glance at it")
}

#[tokio::test]
async fn something_the_work_tells_her_reaches_her_journal_while_it_is_still_running() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.think.then_call(
        "tell",
        serde_json::json!({"text": "The park's own page says nine on weekdays."}),
    );
    let gate = rig.think.hold(1);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Nine on weekdays."}),
    );
    // She sits down to it, and it gets as far as its second step.
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;
    assert!(reported(&rig).is_empty(), "nothing until she glances at it");

    rig.mind.on_time(glance(&rig)).unwrap();
    let id = rig.mind.working.keys().next().unwrap().clone();
    assert_eq!(
        reported(&rig),
        [(
            id.clone(),
            "The park's own page says nine on weekdays.".to_owned(),
            vec![]
        )]
    );
    assert!(!rig.kinds().contains(&"concluded"), "it is still running");
    // Glancing again finds nothing new.
    rig.mind.on_time(glance(&rig)).unwrap();
    assert_eq!(reported(&rig).len(), 1);

    gate.notify_one();
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    assert_eq!(reported(&rig).len(), 1, "and nothing twice when it ends");
    let kinds = rig.kinds();
    let told = kinds.iter().position(|k| *k == "reported").unwrap();
    assert!(told < kinds.iter().position(|k| *k == "concluded").unwrap());
}

/// What she answers a console asking after her work, read as a console
/// reads it.
fn work_seen(rig: &Rig, now: DateTime<Utc>) -> Vec<ghost_protocol::Work> {
    let answer = rig.mind.answer(&super::Question::Work, now).unwrap();
    serde_json::from_value(answer).expect("it reads as the protocol's work")
}

#[tokio::test]
async fn a_console_sees_the_work_in_hand_with_the_part_it_handed_out_and_nothing_is_written() {
    use ghost_protocol::WorkState;
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let seq = at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.think.then_call(
        "delegate",
        serde_json::json!({"task": "Read the tea house's own page."}),
    );
    // The work and its part each ask for a step after that, side by side;
    // both are held in the middle of it.
    let first = rig.think.hold(1);
    let second = rig.think.hold(2);
    rig.think.then_turn("Looking.", &[]);
    rig.think.then_turn("Looking.", &[]);
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 3)
        .await;

    let written = rig.mind.ghost.journal().last_seq().unwrap();
    let seen = work_seen(&rig, t0);
    assert_eq!(rig.mind.ghost.journal().last_seq().unwrap(), written);
    assert_eq!(seen.len(), 1);
    let work = &seen[0];
    assert_eq!(work.seq, Some(seq));
    assert!(
        work.brief
            .starts_with("Find out when the tea house closes.")
    );
    assert_eq!(work.state, WorkState::Running);
    assert_eq!(work.steps, 1, "the step that handed the part out");
    assert!(work.started.is_some() && work.last.is_some());
    assert_eq!(work.parts.len(), 1);
    let part = &work.parts[0];
    assert_eq!(part.id, format!("{}/1", work.id));
    assert_eq!(part.brief, "Read the tea house's own page.");
    assert_eq!(part.state, WorkState::Running);
    assert_eq!((part.steps, part.seq), (0, None));
    assert!(part.started.is_none(), "it is still in its first step");
    assert!(part.parts.is_empty());

    // Whichever of them comes to it first ends the work as blocked.
    for _ in 0..2 {
        rig.think.then_call(
            "done",
            serde_json::json!({"outcome": "blocked", "summary": "The page is down."}),
        );
    }
    first.notify_one();
    second.notify_one();
    rig.until(|r| r.mind.working.is_empty()).await;
    let seen = work_seen(&rig, t0);
    assert_eq!(seen.len(), 1, "work that ended is still shown");
    assert_eq!(seen[0].state, WorkState::Blocked);
    assert_eq!(seen[0].summary.as_deref(), Some("The page is down."));
    assert_eq!(seen[0].parts.len(), 1);
    assert_ne!(seen[0].parts[0].state, WorkState::Running);
}

#[tokio::test]
async fn mail_with_kept_files_wakes_her_attention_to_the_askers_place_and_plain_mail_does_not() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find a good picture of a heron.", t0);
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.think.then_call(
        "tell",
        serde_json::json!({"text": "Two photographers worth looking at so far."}),
    );
    let first = rig.think.hold(1);
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out."}),
    );
    let second = rig.think.hold(2);
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;

    // Plain mail waits for her to look: she does not mean to do anything.
    rig.mind.on_time(glance(&rig)).unwrap();
    assert_eq!(reported(&rig).len(), 1);
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty()
    );

    first.notify_one();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 3)
        .await;
    let now = glance(&rig);
    rig.mind.on_time(now).unwrap();
    // Something she could hand over brings her back to whoever asked.
    assert_eq!(kept(&rig).len(), 1);
    let meant = rig.mind.ghost.memory().open_intentions().unwrap();
    assert_eq!(meant.len(), 1);
    assert_eq!(meant[0].place.as_deref(), Some("t:wen"));
    assert!(meant[0].owed, "she said she would come back with it");
    assert!(meant[0].what.contains("A heron, wings out."));
    assert!(rig.mind.agenda.intention.is_some_and(|due| due > now));

    second.notify_one();
    rig.until(|r| r.mind.working.is_empty()).await;
}

#[tokio::test]
async fn a_picture_kept_by_running_work_can_be_sent_in_the_conversation_before_the_work_ends() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    takes_files(&mut rig, t0);
    at_work_for_wen(&mut rig, "Find a good picture of a heron.", t0);
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out. For Wen."}),
    );
    let gate = rig.think.hold(1);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Found one."}),
    );
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;

    // She sends it while the work is still in the middle of a step.
    rig.think.then(&format!(
        r#"{{"thought":"she will like this one","actions":[{{"do":"send_file","file":"{file}","text":"first one"}}]}}"#
    ));
    rig.beside_until(|r| files_sent(r).len() == 1).await;
    assert_eq!(
        files_sent(&rig),
        [(file.clone(), PIXEL.to_vec(), Some("first one".into()))]
    );
    assert!(!rig.mind.working.is_empty(), "the sitting is still running");
    let u = rig.mind.ghost.memory().undertakings(false, 1).unwrap()[0].clone();
    assert_eq!(u.outcome, None, "and the work is not over");
    // What she had in front of her named it, by number and by file.
    let held = kept(&rig);
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains(&format!("[work #{}]", u.seq)), "{prompt}");
    assert!(
        prompt.contains(&format!(
            "You kept from it #{} ({file}): A heron, wings out.",
            held[0].0
        )),
        "{prompt}"
    );

    gate.notify_one();
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    assert_eq!(kept(&rig).len(), 1, "kept once");
}

#[tokio::test]
async fn steer_reaches_the_running_worker_at_its_next_step() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Write up the tea house hours.", t0);
    rig.think
        .then_call("note", serde_json::json!({"text": "two sources"}));
    let gate = rig.think.hold(0);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Written up."}),
    );
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;

    // In the middle of that step, she means to tell Wen something, and in
    // the same look passes on what she has just thought of.
    rig.mind
        .intend(
            t0 + Duration::minutes(2),
            "say the hours are coming".into(),
            Some("t:wen".into()),
            Some("t:wen".into()),
            true,
        )
        .unwrap();
    rig.mind.refresh_intentions().unwrap();
    rig.think.then(&format!(
        r#"{{"thought":"","actions":[{{"do":"say","text":"nearly there"}},{{"do":"steer","work":{work},"text":"Only the weekday hours, after all."}}]}}"#
    ));
    rig.beside_until(|r| r.kinds().contains(&"added")).await;
    assert!(!rig.mind.working.is_empty(), "the sitting is still running");
    let offered = rig.think.seen.lock().unwrap().last().unwrap().messages[0]
        .text
        .clone();
    assert!(offered.contains(r#"{"do":"steer""#), "it was offered");

    gate.notify_one();
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let turns = rig.think.turns.lock().unwrap();
    let has = |t: &ghost_brain::Turn| {
        t.items.iter().any(|item| {
            matches!(item,
            ghost_brain::Item::User { text } if text.contains("Only the weekday hours, after all."))
        })
    };
    assert!(!has(&turns[0]));
    assert!(has(&turns[1]), "it was in front of the very next step");
    assert_eq!(turns.len(), 2, "in the same sitting, not a new one");
}

#[tokio::test]
async fn stop_ends_the_work_as_dropped_and_keeps_what_it_kept() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Find a good picture of a heron.", t0);
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out."}),
    );
    rig.think
        .then_call("note", serde_json::json!({"text": "looking for more"}));
    let gate = rig.think.hold(1);
    rig.think.then_call(
        "note",
        serde_json::json!({"text": "this step is never asked for"}),
    );
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;

    // Wen no longer wants it; she stops it in the middle of a step.
    rig.mind
        .intend(
            t0 + Duration::minutes(2),
            "Wen found one herself".into(),
            Some("t:wen".into()),
            Some("t:wen".into()),
            true,
        )
        .unwrap();
    rig.mind.refresh_intentions().unwrap();
    rig.think.then(&format!(
        r#"{{"thought":"no need now","actions":[{{"do":"stop","work":{work}}}]}}"#
    ));
    rig.beside_until(|r| r.decisions().iter().any(|d| d.contains("stopped")))
        .await;
    assert!(
        !rig.kinds().contains(&"concluded"),
        "not until its step ends"
    );

    gate.notify_one();
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let ended = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["concluded"], 0)
        .unwrap();
    assert!(matches!(&ended[0].event,
        Event::Concluded { outcome, summary, .. }
            if outcome == "dropped" && summary.contains("You stopped it")));
    assert_eq!(rig.think.turns.lock().unwrap().len(), 2, "no step after it");
    // What it kept is hers, once.
    let held = kept(&rig);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].1, file);
    assert!(rig.mind.ghost.memory().holding(&file).unwrap().is_some());
    // And it is not picked up again.
    assert!(
        rig.mind
            .ghost
            .memory()
            .undertakings(true, 5)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.mind.stop_work(work, t0).unwrap().as_deref(),
        Some(format!("Work #{work} has already ended.").as_str())
    );
}

#[tokio::test]
async fn work_she_is_not_at_stops_at_once() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Draw the east gate.", t0);
    assert_eq!(rig.mind.stop_work(work, t0).unwrap(), None);
    let u = rig.mind.ghost.memory().undertakings(false, 1).unwrap()[0].clone();
    assert_eq!(u.outcome.as_deref(), Some("dropped"));
    // There is nothing left to sit down to.
    rig.pump(t0 + Duration::minutes(5)).await;
    assert!(rig.think.turns.lock().unwrap().is_empty());
    assert!(
        rig.mind
            .stop_work(work + 1000, t0)
            .unwrap()
            .is_some_and(|why| why.contains("no work numbered"))
    );
}

#[tokio::test]
async fn reports_appear_in_her_context_and_the_works_tool_output_does_not() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [0, 0],
            // Sat down to, so that it rests after its sitting runs out of
            // steps and is still under way when she looks.
            work: crate::WorkConfig {
                sitting_steps: 2,
                runs: crate::Runs::Awake,
                ..crate::WorkConfig::default()
            },
            ..MindConfig::default()
        },
        t0,
    );
    let work = at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    let (page, _) = image_host("text/plain", b"opening hours table, row 7f3e: 09:00-21:00").await;
    rig.think
        .then_call("fetch", serde_json::json!({"url": page}));
    rig.think.then_call(
        "tell",
        serde_json::json!({"text": "Two sources agree: nine on weekdays.", "files": ["/w/hours.md"]}),
    );
    // One sitting, out of steps: still under way.
    rig.mind.on_time(t0).unwrap();
    rig.until(|r| r.kinds().contains(&"worked")).await;
    {
        // The work itself saw the page.
        let turns = rig.think.turns.lock().unwrap();
        assert!(turns[1].items.iter().any(|item| matches!(item,
            ghost_brain::Item::Result { output, .. } if output.contains("7f3e"))));
    }

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"soon"}]}"#);
    rig.hear(dm("wen", "2", "any luck?"), t0 + Duration::minutes(1));
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    assert!(
        prompt.contains(&format!(
            "[work #{work}] You have taken something on for wen"
        )),
        "{prompt}"
    );
    // (When it told her is stamped by the real clock, so it is not checked.)
    let told = prompt
        .lines()
        .find(|l| l.starts_with("  It told you, "))
        .expect("what the work told her is under it");
    assert!(
        told.ends_with(": Two sources agree: nine on weekdays. (ready to hand over: /w/hours.md)"),
        "{told}"
    );
    assert!(!prompt.contains("7f3e"), "{prompt}");
    assert!(!prompt.contains("opening hours table"), "{prompt}");
}

#[tokio::test]
async fn the_parts_of_her_work_show_under_it_a_line_each() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Survey the tea houses.", t0);
    // Not sitting down to it here: only how it is shown is looked at.
    rig.mind.agenda.work = None;
    let u = rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].clone();
    let record = rig.mind.undertaking(&u.id, &u.brief).unwrap();
    let first = record
        .open_part("Read the reviews of the east one")
        .unwrap();
    first.update(|s| {
        s.finish = Some(ghost_work::undertaking::Finish {
            outcome: ghost_work::undertaking::Outcome::Finished,
            summary: "Mostly about the tea, which they love.".into(),
            files: vec![],
            lesson: None,
            at: t0,
        });
    });
    let second = record.open_part("Find their opening hours").unwrap();
    second.update(|s| {
        s.told.push(ghost_work::undertaking::Told {
            text: "Nine on weekdays.".into(),
            files: vec![],
            at: t0,
        });
    });

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"soon"}]}"#);
    rig.hear(dm("wen", "2", "any luck?"), t0 + Duration::minutes(1));
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    let at = prompt
        .find(&format!("[work #{work}]"))
        .expect("the work is in the rest of her day");
    let under: Vec<&str> = prompt[at..]
        .lines()
        .filter(|l| l.starts_with("  Part "))
        .collect();
    assert_eq!(
        under,
        [
            "  Part 1 of it (finished), given \"Read the reviews of the east one\": Mostly about the tea, which they love.",
            "  Part 2 of it (paused until you sit down to the work again), given \"Find their opening hours\": Nine on weekdays.",
        ]
    );
}

#[tokio::test]
async fn her_open_work_shows_how_far_its_plan_has_got_in_one_line() {
    use ghost_work::undertaking::{PlanStep, Progress};
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Survey the tea houses.", t0);
    rig.mind.agenda.work = None;
    let u = rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].clone();
    let record = rig.mind.undertaking(&u.id, &u.brief).unwrap();
    let step = |text: &str, status| PlanStep {
        text: text.into(),
        status,
        ..PlanStep::default()
    };
    record.update(|s| {
        s.plan = vec![
            step("List the tea houses", Progress::Done),
            step("Read the reviews", Progress::Done),
            step("Find the opening hours", Progress::Doing),
            step("Visit the east one", Progress::Dropped),
            step("Write it up", Progress::Todo),
        ];
    });

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"soon"}]}"#);
    rig.hear(dm("wen", "2", "any luck?"), t0 + Duration::minutes(1));
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    let at = prompt
        .find(&format!("[work #{work}]"))
        .expect("the work is in the rest of her day");
    let under: Vec<&str> = prompt[at..]
        .lines()
        .filter(|l| l.starts_with("  Its plan"))
        .collect();
    // The dropped step is not counted among those still to be done.
    assert_eq!(
        under,
        ["  Its plan: 2 of 4 steps done; on now: \"Find the opening hours\"."]
    );
}

#[tokio::test]
async fn nothing_kept_is_recorded_twice_when_the_sitting_ends() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find a good picture of a heron.", t0);
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out."}),
    );
    let gate = rig.think.hold(1);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Found one."}),
    );
    rig.mind.on_time(t0).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;
    rig.mind.on_time(glance(&rig)).unwrap();
    rig.mind.on_time(glance(&rig)).unwrap();
    assert_eq!(count(&rig, "kept"), 1, "kept as soon as she glanced");

    gate.notify_one();
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    // The sitting's record still lists it at the end; it is hers once.
    assert_eq!(count(&rig, "kept"), 1);
}

/// Fox, who runs her body, at a console.
fn fox() -> crate::Operator {
    crate::Operator {
        key: "console:fox".into(),
        name: "Fox".into(),
    }
}

#[tokio::test]
async fn an_operator_answers_what_her_work_asked_and_her_journal_says_it_was_theirs() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Write up when the tea house is open.", t0);
    rig.think.then_call(
        "ask",
        serde_json::json!({"question": "weekdays or weekends?", "options": ["weekdays", "weekends"]}),
    );
    rig.mind.on_time(t0).unwrap();
    let asked = rig.until(|r| r.kinds().contains(&"asked")).await;
    let u = rig
        .mind
        .ghost
        .memory()
        .undertaking_numbered(work)
        .unwrap()
        .unwrap();

    // A console is shown what answering takes, and she means to give Wen
    // the choices along with the question.
    let shown = rig.mind.answer(&Question::Work, asked).unwrap();
    assert_eq!(shown[0]["question"], "weekdays or weekends?");
    assert_eq!(shown[0]["kind"], "choose");
    assert_eq!(
        shown[0]["options"],
        serde_json::json!(["weekdays", "weekends"])
    );
    let meant = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["intended"], 0)
        .unwrap();
    assert!(matches!(&meant.last().unwrap().event,
        Event::Intended { what, .. } if what.contains("weekdays / weekends")));

    // Fox answers before Wen does.
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "weekdays, nine to five"}),
    );
    let order = crate::Order::Answer {
        work: u.id.clone(),
        text: "Weekdays.".into(),
    };
    assert_eq!(rig.mind.on_order(order, &fox(), asked).unwrap(), Ok(()));
    let added = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["added"], 0)
        .unwrap();
    assert!(matches!(&added[0].event,
        Event::Added { id, text, by } if *id == u.id && text == "Weekdays."
            && by.as_deref() == Some("console:fox")));
    let waiting = rig.mind.ghost.memory().undertaking(&u.id).unwrap().unwrap();
    assert_eq!(waiting.waiting, None);
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let turns = rig.think.turns.lock().unwrap();
    assert!(turns.last().unwrap().items.iter().any(|item| matches!(item,
        ghost_brain::Item::User { text } if text.contains("added to the task") && text.contains("Weekdays."))));
}

#[tokio::test]
async fn an_operator_stops_her_work_and_it_ends_as_their_doing_not_hers() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let work = at_work_for_wen(&mut rig, "Draw the east gate.", t0);
    let id = rig
        .mind
        .ghost
        .memory()
        .undertaking_numbered(work)
        .unwrap()
        .unwrap()
        .id;
    let stop = || crate::Order::Stop { work: id.clone() };
    assert_eq!(rig.mind.on_order(stop(), &fox(), t0).unwrap(), Ok(()));
    let ended = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["concluded"], 0)
        .unwrap();
    assert!(matches!(&ended[0].event,
        Event::Concluded { outcome, summary, .. }
            if outcome == "dropped" && summary.starts_with("Fox, who runs you, stopped it")));

    // What is over, and what was never hers, cannot be answered or stopped.
    assert!(matches!(
        rig.mind.on_order(stop(), &fox(), t0).unwrap(),
        Err(crate::Refused::NoSuchWork(_))
    ));
    let part = crate::Order::Answer {
        work: format!("{id}/1"),
        text: "yes".into(),
    };
    assert!(matches!(
        rig.mind.on_order(part, &fox(), t0).unwrap(),
        Err(crate::Refused::NoSuchWork(_))
    ));
    assert_eq!(count(&rig, "added"), 0);
}
