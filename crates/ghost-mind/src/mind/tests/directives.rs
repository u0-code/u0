//! Whoever runs her body: at a console she is waited on and answers at
//! once; on a chat their accounts are people like anyone, answered at her
//! own pace, and what changes is what she will spend for them. Nothing
//! anyone writes is an order.

use super::*;

/// Every directive in her journal. There are none to be had any more;
/// older journals keep theirs.
fn directed(rig: &Rig) -> usize {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["directed"], 0)
        .unwrap()
        .len()
}

#[tokio::test]
async fn an_operators_account_on_a_chat_is_answered_at_her_own_pace_and_gives_no_orders() {
    let t0 = at(3, 0);
    let mut rig = Rig::new(operated(), t0);
    // Written the way an order once was, in the middle of her night.
    rig.hear(dm("boss", "1", "fox: tell me the weather"), t0);
    assert_eq!(directed(&rig), 0);
    // Looked at when she would look at anyone's: not at three in the
    // morning.
    let due = rig.mind.agenda.attend["t:boss"].due;
    assert!(due > t0 + Duration::hours(1), "{due}");
}

#[tokio::test]
async fn what_an_operators_account_asks_for_is_not_held_to_the_relationship_floor() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(operated(), t0);
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let desk = tempfile::tempdir().unwrap();
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(desk.path())));
    let take_on = r#"{"thought":"","actions":[
        {"do":"say","text":"ok"},
        {"do":"undertake","brief":"Write today's date into date.txt."}]}"#;

    // A stranger does not get a shell from her.
    rig.think.then(take_on);
    rig.hear(dm("mei", "1", "write today's date into a file?"), t0);
    let now = rig
        .until(|r| r.decisions().iter().any(|d| d == "did not take it on"))
        .await;
    assert!(
        rig.mind
            .ghost
            .memory()
            .undertakings(false, 5)
            .unwrap()
            .is_empty()
    );

    // Whoever runs her, met just as recently, does: it is their machine.
    rig.think.then(take_on);
    rig.hear(dm("boss", "1", "write today's date into a file?"), now);
    rig.until(|r| r.kinds().contains(&"undertook")).await;
    // Open or, with nothing scripted for the work to do, already back
    // with her unfinished: either way it was taken on, for them, and not
    // as asked at a terminal.
    let u = &rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0];
    assert_eq!(u.asked_by.as_deref(), Some("t:boss"));
    assert!(u.brief.starts_with("Write today's date into date.txt."));
    assert!(!u.directed);
    assert_eq!(directed(&rig), 0);
}

#[tokio::test]
async fn a_line_at_a_console_is_looked_at_at_once_whatever_her_day_and_is_no_directive() {
    let t0 = at(3, 0);
    // Nobody is listed as an operator anywhere.
    let mut rig = Rig::quiet(t0);
    rig.hear_on(crate::CONSOLE, dm("fox", "1", "tell me the weather"), t0);
    // Looked at now, though it is three in the morning: a console is a
    // terminal, where she is waited on.
    assert_eq!(rig.mind.agenda.attend["console:fox"].due, t0);
    assert_eq!(directed(&rig), 0);
    assert!(rig.mind.cfg.operators.includes(crate::CONSOLE, "fox"));
}

#[tokio::test]
async fn at_a_console_she_answers_at_once_on_the_terminals_own_page_at_three_in_the_morning() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    rig.online(crate::CONSOLE, t0);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"cloudy, 12 degrees"}]}"#);
    rig.hear_on(crate::CONSOLE, dm("fox", "1", "tell me the weather"), t0);
    let when = rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(
        when, t0,
        "no reading, no typing, no waiting for her morning"
    );
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("is waiting on your answer now"), "{prompt}");
    assert!(!prompt.contains("looking at your phone"), "{prompt}");
}
