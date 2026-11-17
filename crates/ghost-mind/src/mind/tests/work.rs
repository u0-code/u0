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
                kind: crate::Kind::Chat,
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
            live: false,
            browse_minutes: [0, 0],
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
        .undertake("Find a good picture of a heron.", None, None, t0)
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
            live: false,
            browse_minutes: [0, 0],
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
