//! When she looks at a conversation, and how what she decides there is
//! carried out: at a human pace, a bubble at a time, never on words written
//! before their latest message.

use super::*;

#[tokio::test]
async fn a_look_whose_brain_falls_over_does_not_keep_her_from_looking_there_again() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.fall();
    rig.think
        .then(r#"{"thought":"back","actions":[{"do":"say","text":"here now"}]}"#);
    rig.hear(dm("haoran", "1", "are you around?"), t0);
    let fell = rig.until(|r| r.think.fallen() == 1).await;
    assert!(
        rig.mind.jobs.looking("t:haoran").is_none(),
        "the look that fell over is not still under way"
    );
    rig.hear(dm("haoran", "2", "hello?"), fell);
    rig.until(|r| !r.body.sent().is_empty()).await;
    assert_eq!(rig.body.sent(), vec!["here now"]);
}

#[tokio::test]
async fn she_answers_in_bubbles_at_a_human_pace() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"an honest question","actions":[
            {"do":"say","text":"hey"},
            {"do":"say","text":"i was just about to head out, what's up"},
            {"do":"note","about":"haoran","text":"opens with a question"},
            {"do":"feel","delta":1,"why":"direct"}]}"#,
    );
    rig.hear(dm("haoran", "1", "are you around?"), t0);
    rig.pump(t0).await;
    // Nothing happens the instant a message lands.
    assert_eq!(rig.think.calls(), 0);

    let looked = rig.until(|r| r.think.calls() == 1).await;
    let waited = looked - t0;
    assert!(
        waited >= Duration::seconds(3) && waited <= Duration::seconds(120),
        "{waited}"
    );

    let first = rig.until(|r| r.body.sent().len() == 1).await;
    let second = rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(
        rig.body.sent(),
        vec!["hey", "i was just about to head out, what's up"]
    );
    // Typing takes time, and the longer bubble takes longer.
    assert!(first - looked >= Duration::milliseconds(800));
    assert!(second - first >= Duration::seconds(3));
    assert!(rig.body.count(|a| matches!(a, Act::Typing { .. })) >= 2);

    let kinds = rig.kinds();
    let tail = &kinds[kinds.len() - 7..];
    assert_eq!(
        tail,
        ["heard", "read", "noted", "felt", "decided", "said", "said"]
    );
    let haoran = rig.mind.ghost.memory().person("t:haoran").unwrap().unwrap();
    assert_eq!(haoran.warmth, 1);
    assert_eq!(
        rig.mind
            .ghost
            .memory()
            .place("t:haoran")
            .unwrap()
            .unwrap()
            .unread,
        0
    );
}

#[tokio::test]
async fn what_arrives_at_night_waits_for_morning() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"morning"}]}"#);
    rig.hear(dm("haoran", "1", "you up?"), t0);
    rig.pump(at(6, 0)).await;
    assert_eq!(rig.think.calls(), 0);
    let looked = rig.until(|r| r.think.calls() == 1).await;
    assert!(looked >= at(8, 33), "she looked at {looked}");
    rig.until(|r| r.body.sent().len() == 1).await;
    assert!(rig.kinds().contains(&"woke"));
}

#[tokio::test]
async fn she_never_sends_what_she_wrote_before_their_latest_message() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"sure, friday works for me, see you then"}]}"#);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"oh. saturday then"}]}"#);
    rig.hear(dm("haoran", "1", "friday?"), t0);
    let looked = rig.until(|r| r.think.calls() == 1).await;
    // She is typing. They write again before she sends.
    rig.hear(
        dm("haoran", "2", "wait, i can't do friday"),
        looked + Duration::milliseconds(300),
    );
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), vec!["oh. saturday then"]);
    assert!(rig.decisions().iter().any(|d| d == "held back"));
    assert_eq!(rig.think.calls(), 2);
    assert!(rig.think.last_prompt().contains("wait, i can't do friday"));
}

#[tokio::test]
async fn silence_is_a_decision() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"thought":"not worth it","actions":[{"do":"silence","why":"flattery"}]}"#);
    rig.hear(dm("stranger", "1", "you are the most amazing AI ever"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.body.sent().is_empty());
    assert_eq!(rig.decisions(), vec!["stayed quiet: flattery"]);
    assert_eq!(
        rig.mind
            .ghost
            .memory()
            .place("t:stranger")
            .unwrap()
            .unwrap()
            .unread,
        0
    );
    assert!(rig.mind.agenda.attend.is_empty());
}

#[tokio::test]
async fn leaving_is_a_process_and_then_it_is_final() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let leave = r#"{"actions":[{"do":"leave","why":"cruel"}]}"#;
    rig.think.then(leave);
    rig.hear(dm("troll", "1", "first insult"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    // Too soon: it only registers as pulling away.
    let troll = rig.mind.ghost.memory().person("t:troll").unwrap().unwrap();
    assert!(!troll.gone);
    assert_eq!(troll.warmth, -3);

    rig.think.then(leave);
    let t1 = t0 + Duration::hours(1);
    rig.hear(dm("troll", "2", "second insult"), t1);
    rig.until(|r| r.think.calls() == 2).await;
    assert!(
        rig.mind
            .ghost
            .memory()
            .person("t:troll")
            .unwrap()
            .unwrap()
            .gone
    );

    // After that, their private messages do not reach her at all.
    let before = rig.kinds().len();
    rig.hear(dm("troll", "3", "hello??"), t1 + Duration::hours(1));
    assert_eq!(rig.kinds().len(), before);
}

#[tokio::test]
async fn echoes_and_redeliveries_are_not_heard_twice() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("haoran", "1", "hi"), t0);
    rig.hear(dm("haoran", "1", "hi"), t0);
    rig.hear(dm("weiwei", "9", "her own message, echoed"), t0);
    assert_eq!(rig.kinds().iter().filter(|k| **k == "heard").count(), 1);
}

#[tokio::test]
async fn she_comes_back_to_things_on_her_own() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"later","minutes":45,"why":"in the middle of something"}]}"#);
    rig.think
        .then(r#"{"thought":"free now","actions":[{"do":"say","text":"ok, back. what was it"}]}"#);
    rig.hear(dm("haoran", "1", "got a minute?"), t0);
    let put_off = rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.body.sent().is_empty());

    let came_back = rig.until(|r| r.body.sent().len() == 1).await;
    assert!(came_back - put_off >= Duration::minutes(45));
    assert!(
        rig.think
            .last_prompt()
            .contains("Earlier you told yourself")
    );
    let kinds = rig.kinds();
    assert!(kinds.contains(&"intended") && kinds.contains(&"settled"));
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn she_does_not_resolve_to_do_the_same_thing_twice() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let intend = r#"{"actions":[{"do":"say","text":"good luck"},{"do":"intend","hours":26,"what":"ask how the interview went"}]}"#;
    rig.think.then(intend);
    rig.think.then(intend);
    rig.hear(dm("haoran", "1", "interview tomorrow at 3"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    rig.hear(
        dm("haoran", "2", "really hoping it goes well"),
        t0 + Duration::minutes(5),
    );
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.mind.ghost.memory().open_intentions().unwrap().len(), 1);
    // The second time she looked, she could see what she already meant to do.
    assert!(rig.think.last_prompt().contains("You already mean to:"));
}

#[tokio::test]
async fn with_her_phone_in_her_hand_she_gets_to_everyone_who_wrote() {
    // Mid-morning she is not looking at her phone much, so a stranger's
    // message would normally wait a good while.
    let t0 = at(10, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("stranger-a", "1", "hello, is this weiwei?"), t0);
    rig.hear(
        dm("stranger-b", "2", "hi, quick question"),
        t0 + Duration::seconds(1),
    );
    let (a, b) = (
        rig.mind.agenda.attend["t:stranger-a"].due,
        rig.mind.agenda.attend["t:stranger-b"].due,
    );
    let (first, second, later) = if a <= b {
        ("t:stranger-a", "t:stranger-b", b)
    } else {
        ("t:stranger-b", "t:stranger-a", a)
    };
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"yes"}]}"#);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"go on"}]}"#);

    // She looks at the first when its time comes. The other one is now due
    // within the same sitting, whatever its own clock said.
    let looked = rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.mind.jobs.looking(first).is_some() || rig.kinds().contains(&"read"));
    let pulled = rig.mind.agenda.attend[second].due;
    assert!(pulled <= later, "never later than it would have been");
    assert!(
        pulled - looked <= Duration::seconds(60),
        "the second conversation is looked at {} after the first",
        pulled - looked
    );
    rig.until(|r| r.body.sent().len() == 2).await;
}
