//! Taking something on: doing it beside the conversation, stopping to ask,
//! handing over what she made, and keeping what it taught her.

use super::*;

#[tokio::test]
async fn she_takes_something_on_does_it_and_comes_back_with_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"easy enough to check","actions":[
            {"do":"say","text":"let me look"},
            {"do":"undertake","brief":"Find out what time the Heming tea house in People's Park closes on weekdays."}]}"#,
    );
    // The work itself: she finds it out, and says so.
    rig.think.then_call(
        "note",
        serde_json::json!({"text": "the park's own page says 21:00"}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It closes at nine on weekdays."}),
    );
    rig.hear(
        dm("wen", "1", "do you know when the heming tea house closes?"),
        t0,
    );

    // The work runs beside the conversation: neither waits for the other.
    rig.until(|r| r.kinds().contains(&"concluded") && r.kinds().contains(&"said"))
        .await;
    let kinds = rig.kinds();
    let took = kinds.iter().position(|k| *k == "undertook").unwrap();
    let ended = kinds.iter().position(|k| *k == "concluded").unwrap();
    assert!(took < ended, "{kinds:?}");

    // The work was handed the brief, not her persona, with no shell in
    // this body.
    {
        let turns = rig.think.turns.lock().unwrap();
        assert!(!turns[0].system.contains("Weiwei"));
        assert!(
            matches!(&turns[0].items[0], ghost_brain::Item::User { text } if text.contains("Heming tea house"))
        );
        assert!(!turns[0].tools.iter().any(|t| t.name == "shell"));
    }

    // Having finished, she means to tell whoever asked, and does.
    let u = &rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0];
    assert_eq!(u.outcome.as_deref(), Some("finished"));
    assert_eq!(u.asked_by.as_deref(), Some("t:wen"));
    rig.think
        .then(r#"{"thought":"tell her","actions":[{"do":"say","text":"nine on weekdays"}]}"#);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent(), ["let me look", "nine on weekdays"]);
    assert!(
        rig.think
            .last_prompt()
            .contains("It closes at nine on weekdays.")
    );
}

#[tokio::test]
async fn she_stops_to_ask_and_goes_on_when_they_answer() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[
            {"do":"say","text":"on it"},
            {"do":"undertake","brief":"Write up when the Heming tea house is open."}]}"#,
    );
    rig.think.then_call(
        "ask",
        serde_json::json!({"question": "weekdays or weekends?"}),
    );
    rig.hear(
        dm(
            "wen",
            "1",
            "can you write up when the tea house is open? it is for my mum",
        ),
        t0,
    );
    let asked = rig.until(|r| r.kinds().contains(&"asked")).await;

    // What she works from is her own account of it, and under that what
    // was actually written to her.
    let u = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert!(
        u.brief
            .starts_with("Write up when the Heming tea house is open.")
    );
    assert!(u.brief.contains("word for word") && u.brief.contains("> can you write up"));
    assert_eq!(u.waiting.as_deref(), Some("weekdays or weekends?"));

    // She means to put the question to them, and does.
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"weekdays or weekends?"}]}"#);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent()[1], "weekdays or weekends?");
    // While it waits, she is told so whenever she looks at anything.
    assert!(
        rig.think
            .last_prompt()
            .contains("It waits for their answer")
    );
    // And the work is not touched.
    let worked = rig.think.turns.lock().unwrap().len();
    rig.pump(asked + Duration::hours(2)).await;
    assert_eq!(rig.think.turns.lock().unwrap().len(), worked);

    // They answer; she passes it on; the work picks up with it.
    let answered = asked + Duration::hours(3);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"undertake","brief":"Weekdays."}]}"#);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "blocked", "summary": "the page is down"}),
    );
    rig.hear(dm("wen", "2", "weekdays please"), answered);
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let kinds = rig.kinds();
    let added = kinds.iter().position(|k| *k == "added").unwrap();
    assert!(added < kinds.iter().position(|k| *k == "concluded").unwrap());
    let turns = rig.think.turns.lock().unwrap();
    assert!(turns.last().unwrap().items.iter().any(|item| matches!(item,
        ghost_brain::Item::User { text }
            if text.contains("added to the task") && text.contains("Weekdays.") && text.contains("> weekdays please"))));
}

#[tokio::test]
async fn a_question_nobody_answers_does_not_hold_the_work_for_ever() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"undertake","brief":"Write up the tea house hours."}]}"#,
    );
    rig.think
        .then_call("ask", serde_json::json!({"question": "which branch?"}));
    rig.hear(dm("wen", "1", "write up the tea house hours?"), t0);
    let asked = rig.until(|r| r.kinds().contains(&"asked")).await;

    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "blocked", "summary": "assumed the park branch; its page is down"}),
    );
    // She has it down to come back to when a day has gone by with no
    // answer. (Entries are stamped by the real clock, so that day is
    // counted from the entry and not from the time this test pretends.)
    rig.pump(asked + Duration::hours(1)).await;
    rig.pump(asked + Duration::hours(2)).await;
    let give_up = rig.mind.agenda.work.expect("she means to come back to it");
    let u = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert_eq!(give_up, u.asked_at.unwrap() + Duration::hours(24));
    rig.pump(give_up).await;
    rig.pump(give_up + Duration::minutes(30)).await;
    assert!(rig.kinds().contains(&"concluded"), "{:?}", rig.kinds());
    let turns = rig.think.turns.lock().unwrap();
    assert!(turns.last().unwrap().items.iter().any(|item| matches!(item,
        ghost_brain::Item::User { text } if text.contains("No answer came"))));
}

#[tokio::test]
async fn what_she_made_is_handed_over_as_a_file() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    let desk = tempfile::tempdir().unwrap();
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(desk.path())));
    befriend(&mut rig, "wen", t0);
    // This channel can take a file.
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: Some(Actor {
                    id: "weiwei".into(),
                    name: "Weiwei".into(),
                    handle: Some("weiwei".into()),
                    automated: true,
                }),
                can: vec![Can::Send, Can::Typing, Can::Files],
                facts: crate::ChannelFacts::default(),
            },
            t0,
        )
        .unwrap();

    rig.think.then_turn(
        "",
        &[
            ("say", serde_json::json!({"text": "give me a bit"})),
            (
                "undertake",
                serde_json::json!({"brief": "Write the tea house hours into hours.md."}),
            ),
        ],
    );
    let made = desk.path().join("hours.md");
    rig.think.then_call(
        "write",
        serde_json::json!({"path": "hours.md", "content": "09:00-21:00\n"}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({
            "outcome": "finished", "summary": "It is written up.",
            "files": [made.to_str().unwrap()],
        }),
    );
    rig.hear(
        dm("wen", "1", "could you write up the tea house hours?"),
        t0,
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;

    // Telling them, she is offered a way to hand it over, and uses it.
    rig.think.then_turn(
        "",
        &[(
            "send_file",
            serde_json::json!({"file": "hours.md", "text": "here you go"}),
        )],
    );
    rig.until(|r| r.body.count(|a| matches!(a, Act::File { .. })) == 1)
        .await;
    {
        let turns = rig.think.turns.lock().unwrap();
        let telling = turns.last().unwrap();
        assert!(telling.tools.iter().any(|t| t.name == "send_file"));
        assert!(matches!(&telling.items[0],
            ghost_brain::Item::User { text } if text.contains("hours.md") && text.contains("send_file")));
    }
    let acts = rig.body.acts.lock().unwrap();
    let sent = acts
        .iter()
        .find_map(|(_, act)| match act {
            Act::File {
                name, data, text, ..
            } => Some((name.clone(), data.clone(), text.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(sent.0, "hours.md");
    assert_eq!(sent.1, b"09:00-21:00\n");
    assert_eq!(sent.2.as_deref(), Some("here you go"));
    drop(acts);
    // And it is in the conversation as something she sent.
    let thread = rig.mind.ghost.memory().thread("t:wen", 3).unwrap();
    assert!(
        thread
            .iter()
            .any(|l| l.text == "[file: hours.md] here you go")
    );
}

#[tokio::test]
async fn she_takes_something_on_toward_what_she_is_after_and_keeps_what_it_taught_her() {
    let t0 = at(23, 30);
    let mut rig = Rig::quiet(t0);
    // A brain that can work.
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let night = |json: serde_json::Value| super::living::reflect_draft(&json).unwrap();

    // With nothing she is after, she is not asked what to do about it,
    // and anything said anyway comes to nothing.
    assert!(rig.mind.own_work_line(t0).unwrap().contains("empty"));
    rig.mind
        .on_reflected(
            None,
            &[],
            None,
            Ok(night(serde_json::json!({
                "reflection": "A quiet day.",
                "undertake": "Find out which tea houses are closing.",
            }))),
            t0,
        )
        .unwrap();
    assert!(!rig.kinds().contains(&"undertook"));

    // One night she finds she is after something, and means to start.
    rig.mind
        .on_reflected(
            None,
            &[],
            None,
            Ok(night(serde_json::json!({
                "reflection": "I keep going back to the old tea houses.",
                "pursuits": [{"what": "draw the tea houses that are closing, before they do"}],
                "undertake": "Find out which tea houses in the city are closing this year, with dates.",
            }))),
            t0,
        )
        .unwrap();
    let kinds = rig.kinds();
    assert!(
        kinds.contains(&"pursued") && kinds.contains(&"undertook"),
        "{kinds:?}"
    );
    let own = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert_eq!((own.asked_by, own.place), (None, None));
    // It is in front of her the next night, numbered, to change or let go.
    assert!(
        rig.mind
            .pursuit_block()
            .unwrap()
            .contains("draw the tea houses")
    );
    // One thing of her own at a time.
    rig.mind
        .on_reflected(
            None,
            &[],
            None,
            Ok(night(serde_json::json!({
                "reflection": "More of the same.",
                "undertake": "Also list the ones that have already gone.",
            }))),
            t0,
        )
        .unwrap();
    assert_eq!(rig.kinds().iter().filter(|k| **k == "undertook").count(), 1);

    // She does it, in her own time, and what it taught her about working
    // is kept and is there at the top of the next piece of work.
    rig.think.then_call(
        "done",
        serde_json::json!({
            "outcome": "gave_up", "summary": "The listings are all a year old.",
            "lesson": "Look at when a page was last changed before trusting a date on it.",
        }),
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    assert!(rig.kinds().contains(&"practised"));
    assert!(
        rig.mind
            .practice_block()
            .unwrap()
            .contains("when a page was last changed")
    );
    // Nobody asked, so there is nobody to tell.
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty()
    );

    // She can change her mind about what she is after, or let it go.
    let seq = rig.mind.ghost.memory().pursuits(3).unwrap()[0].seq;
    rig.mind
        .on_pursuits(vec![(String::new(), Some(seq))])
        .unwrap();
    assert!(rig.mind.ghost.memory().pursuits(3).unwrap().is_empty());
}

#[tokio::test]
async fn a_work_sittings_kept_files_become_kept_entries_once_from_that_undertaking() {
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
                sitting_steps: 2,
                ..crate::WorkConfig::default()
            },
            ..MindConfig::default()
        },
        t0,
    );
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // An image a tool showed her at work, kept beside her work database.
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.mind
        .undertake("Find a good picture of a heron.", None, None, None, t0)
        .unwrap();
    // The first sitting keeps it and runs out of steps; the second finishes.
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out. For when someone is about to leave."}),
    );
    rig.think
        .then_call("note", serde_json::json!({"text": "found one"}));
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Found one."}),
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let kinds = rig.kinds();
    assert!(kinds.contains(&"worked"), "two sittings: {kinds:?}");

    let id = rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0]
        .id
        .clone();
    let held = kept(&rig);
    assert_eq!(held.len(), 1, "kept once, not once a sitting: {held:?}");
    let (_, kept_file, note, from) = &held[0];
    assert_eq!(kept_file, &file);
    assert!(note.starts_with("A heron, wings out."));
    assert_eq!(from.as_deref(), Some(id.as_str()));
}

#[tokio::test]
async fn work_that_ends_while_she_still_means_to_show_what_it_found_is_still_told_as_ended() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find a good picture of a heron.", t0);
    // The work keeps something worth handing over, and then is done, in
    // the one sitting: she means to show Wen what it found before she has
    // had a moment to.
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.think.then_call(
        "keep",
        serde_json::json!({"file": file, "note": "A heron, wings out."}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Found one by the lake."}),
    );
    rig.mind.on_time(t0).unwrap();
    rig.until(|r| r.kinds().contains(&"concluded")).await;

    let open: Vec<String> = rig
        .mind
        .ghost
        .memory()
        .open_intentions()
        .unwrap()
        .into_iter()
        .filter(|i| i.about.as_deref() == Some("t:wen"))
        .map(|i| i.what)
        .collect();
    assert!(
        open.iter().any(|w| w.contains("A heron, wings out.")),
        "{open:?}"
    );
    // That it is over, and what it came to, is not taken for the same
    // thing as what it had found so far.
    assert!(
        open.iter().any(|w| w.contains("Found one by the lake.")
            && w.contains(rig.mind.prompts.text("how_finished"))),
        "{open:?}"
    );
}

/// What she means to do about the work that came back to her for Wen.
fn meant_for_wen(rig: &Rig) -> String {
    let open = rig.mind.ghost.memory().open_intentions().unwrap();
    assert_eq!(open.len(), 1, "{open:?}");
    open[0].what.clone()
}

#[tokio::test]
async fn work_that_runs_out_of_sittings_comes_back_unfinished_and_she_means_to_tell_whoever_asked()
{
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
                max_sittings: 1,
                ..crate::WorkConfig::default()
            },
            ..MindConfig::default()
        },
        t0,
    );
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.think
        .then_call("note", serde_json::json!({"text": "two sources disagree"}));
    rig.mind.on_time(t0).unwrap();
    rig.until(|r| r.kinds().contains(&"concluded")).await;

    let u = &rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0];
    assert_eq!(u.outcome.as_deref(), Some("blocked"));
    let summary = u.summary.clone().unwrap_or_default();
    assert!(summary.contains("two sources disagree"), "{summary}");
    let meant = meant_for_wen(&rig);
    assert!(
        meant.contains(rig.mind.prompts.text("how_blocked")),
        "{meant}"
    );
    assert!(
        meant.contains(rig.mind.prompts.text("unfinished_next").trim()),
        "{meant}"
    );
}

/// Her attention, with every piece of work given one sitting.
fn one_sitting() -> MindConfig {
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
            max_sittings: 1,
            ..crate::WorkConfig::default()
        },
        ..MindConfig::default()
    }
}

#[tokio::test]
async fn work_that_ended_on_its_record_before_she_wrote_it_down_ends_as_its_record_says() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(one_sitting(), t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    let u = rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].clone();
    // Its one sitting finished it, and she stopped before she heard: the
    // record says so, her journal has its sitting and has it open.
    let record = rig.mind.undertaking(&u.id, &u.brief).unwrap();
    record
        .update(|s| {
            s.told.push(ghost_work::undertaking::Told {
                text: "The park's page says nine.".into(),
                files: vec![],
                at: t0,
            });
            s.finish = Some(ghost_work::undertaking::Finish {
                outcome: ghost_work::undertaking::Outcome::Finished,
                summary: "It closes at nine on weekdays.".into(),
                files: vec!["hours.md".into()],
                lesson: None,
                at: t0,
            });
        })
        .unwrap();
    rig.mind
        .record(Event::Worked {
            id: u.id.clone(),
            summary: "Started; nothing written down yet.".into(),
            steps: 3,
        })
        .unwrap();

    rig.mind.on_time(t0).unwrap();

    let ended = rig.mind.ghost.memory().undertaking(&u.id).unwrap().unwrap();
    assert_eq!(
        ended.outcome.as_deref(),
        Some("finished"),
        "as the record says, not blocked for want of sittings"
    );
    assert_eq!(
        ended.summary.as_deref(),
        Some("It closes at nine on weekdays.")
    );
    assert_eq!(count(&rig, "reported"), 1, "what it told her comes first");
    assert!(running(&rig).is_empty(), "nothing sat down to it again");
    let meant = meant_for_wen(&rig);
    assert!(
        meant.contains(rig.mind.prompts.text("how_finished")),
        "{meant}"
    );
}

#[tokio::test]
async fn work_she_ends_ends_on_its_record_too() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(one_sitting(), t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    let u = rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].clone();
    rig.mind
        .record(Event::Worked {
            id: u.id.clone(),
            summary: "Two sources disagree.".into(),
            steps: 3,
        })
        .unwrap();

    // Out of sittings, it comes back to her unfinished.
    rig.mind.on_time(t0).unwrap();

    let ended = rig.mind.ghost.memory().undertaking(&u.id).unwrap().unwrap();
    assert_eq!(ended.outcome.as_deref(), Some("blocked"));
    let looked = ghost_work::undertaking::look(
        &rig.mind.ghost.dir().join(ghost_work::WORK_DB),
        std::slice::from_ref(&u.id),
    )
    .unwrap();
    let finish = looked[0].state.finish.clone().unwrap();
    assert_eq!(
        finish.outcome,
        ghost_work::undertaking::Outcome::Blocked,
        "a tool server still holding it refuses from now on"
    );
    assert_eq!(Some(finish.summary), ended.summary);
}

#[tokio::test]
async fn looking_at_work_she_has_ended_makes_no_record_of_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(rig.mind.ghost.dir())));
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: None,
                can: vec![Can::Send, Can::Files],
                facts: crate::ChannelFacts::default(),
            },
            t0,
        )
        .unwrap();
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.mind.agenda.work = None;
    let u = rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].clone();
    rig.mind
        .record(Event::Concluded {
            id: u.id.clone(),
            outcome: "finished".into(),
            summary: "It closes at nine.".into(),
        })
        .unwrap();
    // Her work database is there, with other work in it.
    let db = rig.mind.ghost.dir().join(ghost_work::WORK_DB);
    ghost_work::Undertaking::open(&db, "other", Some("something else")).unwrap();

    let place = rig.mind.ghost.memory().place("t:wen").unwrap().unwrap();
    assert!(rig.mind.deliverables(&place).unwrap().is_empty());
    rig.mind.elsewhere(&place, &[], t0).unwrap();

    assert!(
        ghost_work::undertaking::look(&db, std::slice::from_ref(&u.id))
            .unwrap()
            .is_empty(),
        "no record was made by looking"
    );
}

#[tokio::test]
async fn work_that_stops_getting_anywhere_comes_back_unfinished_after_one_sitting() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    // Nothing scripted: every step of the work only talks.
    rig.mind.on_time(t0).unwrap();
    rig.until(|r| r.kinds().contains(&"concluded")).await;

    assert_eq!(
        count(&rig, "worked"),
        0,
        "no second sitting from the same place"
    );
    let u = &rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0];
    assert_eq!(u.outcome.as_deref(), Some("blocked"));
    let meant = meant_for_wen(&rig);
    assert!(meant.contains("stopped getting anywhere"), "{meant}");
    assert!(
        meant.contains(rig.mind.prompts.text("unfinished_next").trim()),
        "{meant}"
    );
}

#[tokio::test]
async fn work_is_done_in_a_folder_of_its_own_made_for_it_unless_one_was_named() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let computer = tempfile::tempdir().unwrap();
    let hers = computer.path().join("Ghost").join("solver");
    rig.mind.workshop.folders = Some(hers.clone());
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(computer.path())));
    let theirs = computer.path().join("their-project");
    std::fs::create_dir(&theirs).unwrap();

    rig.mind
        .undertake(
            "Count the lines of every crate.\nThen make a table.",
            None,
            None,
            None,
            t0,
        )
        .unwrap();
    let dir_of = |rig: &Rig, start: &str| {
        rig.mind
            .ghost
            .memory()
            .undertakings(false, 5)
            .unwrap()
            .into_iter()
            .find(|u| u.brief.starts_with(start))
            .and_then(|u| u.dir)
            .unwrap()
    };
    // Its own, by the day it was taken on and what it is.
    let own = std::path::PathBuf::from(dir_of(&rig, "Count"));
    assert_eq!(
        own.parent().unwrap(),
        hers.join(t0.format("%Y-%m-%d").to_string())
    );
    let name = own.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("count-the-lines-of-every"), "{name}");

    // It is made before the first step, as a repository, and the work's
    // commands run in it.
    rig.think
        .then_call("shell", serde_json::json!({"command": "pwd > where.txt"}));
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "counted"}),
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    assert!(own.join(".git").is_dir(), "made as a repository");
    let ran_in = std::fs::read_to_string(own.join("where.txt")).unwrap();
    assert_eq!(
        std::path::Path::new(ran_in.trim()).canonicalize().unwrap(),
        own.canonicalize().unwrap()
    );

    // Work whose asker named a folder is done there.
    rig.mind
        .undertake(
            "Tidy their project.",
            None,
            None,
            Some(&theirs.display().to_string()),
            t0,
        )
        .unwrap();
    assert_eq!(dir_of(&rig, "Tidy"), theirs.display().to_string());
}

#[test]
fn a_work_folder_is_named_from_the_first_line_of_its_brief_in_any_script() {
    assert_eq!(
        super::super::work::folder_name("Count the lines: of every crate!\nmore"),
        "count-the-lines-of-every-crat"
    );
    assert_eq!(
        super::super::work::folder_name("统计 crates 的行数"),
        "统计-crates-的行数"
    );
    assert_eq!(super::super::work::folder_name("  \n/// ?"), "work");
}

#[test]
fn her_status_says_what_she_can_do_as_her_daemon_runs_her() {
    use ghost_protocol::{Abilities, MachineInfo};
    let t0 = at(14, 0);
    let abilities = |rig: &Rig| -> Abilities {
        let status = rig.mind.answer(&Question::Status, t0).unwrap();
        serde_json::from_value(status["abilities"].clone()).unwrap()
    };

    // A brain that cannot run tools takes nothing on, whatever is set.
    let rig = Rig::quiet(t0);
    assert_eq!(
        abilities(&rig),
        Abilities {
            work: false,
            web: true,
            ..Abilities::default()
        }
    );

    // With one, and a shell on this computer, she works here, and says
    // in which directory.
    let mut rig = Rig::with_tools(t0);
    assert!(abilities(&rig).work && abilities(&rig).machine.is_none());
    let desk = tempfile::tempdir().unwrap();
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(desk.path())));
    rig.mind.workshop.local = Some(desk.path().to_path_buf());
    rig.mind.cfg.work.machine = Some(" local: ".into());
    let said = abilities(&rig);
    assert_eq!(
        said.machine,
        Some(MachineInfo {
            kind: "local".into(),
            spec: "local:".into(),
            dir: Some(desk.path().display().to_string()),
        })
    );
    assert!(said.local());

    // Turned off, nothing is offered, though the machine is still there.
    rig.mind.cfg.work.enabled = false;
    rig.mind.cfg.work.web = false;
    let off = abilities(&rig);
    assert!(!off.work && !off.web && !off.local());
}
