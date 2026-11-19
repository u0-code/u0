//! Being stopped from a console: what she is thinking, about to say and
//! working on in a place is called off, and she remembers it was.

use super::*;
use crate::CONSOLE;

/// Fox, who runs her body, at a console.
fn fox() -> crate::Operator {
    crate::Operator {
        key: "console:fox".into(),
        name: "Fox".into(),
    }
}

/// Fox says `text` to her from the console, in the place named after him.
fn from_fox(id: &str, text: &str) -> Incoming {
    let mut message = dm("fox", id, text);
    message.from.name = "Fox".into();
    message
}

/// Every time she was stopped: by whom, whether in the middle of a look,
/// what went unsaid and what work with it.
fn stops(rig: &Rig) -> Vec<(String, bool, Vec<String>, Vec<String>)> {
    stops_in(rig, "console:fox")
}

/// The same, where every stop is to have been in `at`.
fn stops_in(rig: &Rig, at: &str) -> Vec<(String, bool, Vec<String>, Vec<String>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["stopped"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Stopped {
                place,
                by,
                looking,
                unsaid,
                works,
            } => {
                assert_eq!(place, at);
                Some((by, looking, unsaid, works))
            }
            _ => None,
        })
        .collect()
}

fn unread(rig: &Rig) -> i64 {
    rig.mind
        .ghost
        .memory()
        .place("console:fox")
        .unwrap()
        .unwrap()
        .unread
}

#[tokio::test]
async fn stopped_while_the_brain_is_thinking_she_says_nothing_even_when_it_answers_after() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.online(CONSOLE, t0);
    let gate = rig.think.hold(0);
    rig.think
        .then_call("say", serde_json::json!({"text": "half past nine"}));
    rig.hear_on(
        CONSOLE,
        from_fox("1", "what time does the tea house close"),
        t0,
    );
    let now = rig.mind.agenda.next_due().unwrap();
    rig.mind.on_time(now).unwrap();
    rig.meanwhile(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    assert!(rig.mind.jobs.looking("console:fox").is_some());

    assert!(rig.mind.interrupt("console:fox", &fox(), now).unwrap());
    // The brain answers anyway; it is too late to be heard.
    gate.notify_one();
    rig.pump(now).await;
    rig.pump(now + Duration::hours(1)).await;

    assert!(rig.body.sent().is_empty(), "{:?}", rig.body.sent());
    let kinds = rig.kinds();
    assert!(!kinds.contains(&"said") && !kinds.contains(&"decided"));
    assert_eq!(
        stops(&rig),
        [("console:fox".to_owned(), true, vec![], vec![])]
    );
    // What he said is read, so she does not look at it again.
    assert_eq!(unread(&rig), 0);
    assert_eq!(rig.think.turns.lock().unwrap().len(), 1);
    assert!(rig.mind.agenda.attend.is_empty());
    // With nothing under way, stopping her stops nothing, and is not
    // written down.
    assert!(
        !rig.mind
            .interrupt("console:fox", &fox(), now + Duration::hours(1))
            .unwrap()
    );
    assert_eq!(stops(&rig).len(), 1);
}

#[tokio::test]
async fn a_line_taken_back_is_gone_from_the_conversation_with_everything_said_after_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.online(CONSOLE, t0);
    rig.think
        .then_call("say", serde_json::json!({"text": "half past nine"}));
    rig.hear_on(
        CONSOLE,
        from_fox("1", "what time does the tea house close"),
        t0,
    );
    let now = rig.mind.agenda.next_due().unwrap();
    rig.mind.on_time(now).unwrap();
    rig.until(|r| r.body.sent().len() == 1).await;
    let thread = rig.mind.ghost.memory().thread("console:fox", 10).unwrap();
    assert_eq!(thread.len(), 2);
    let line = thread[0].seq;

    // Her line is not his to take back, nor one that was never said.
    assert_eq!(
        rig.mind
            .rewind("console:fox", thread[1].seq, &fox(), now)
            .unwrap(),
        None
    );
    assert_eq!(
        rig.mind.rewind("console:fox", 9999, &fox(), now).unwrap(),
        None
    );
    assert!(stops(&rig).is_empty());

    assert_eq!(
        rig.mind.rewind("console:fox", line, &fox(), now).unwrap(),
        Some(2)
    );
    assert!(
        rig.mind
            .ghost
            .memory()
            .thread("console:fox", 10)
            .unwrap()
            .is_empty()
    );
    assert!(rig.kinds().contains(&"rewound"));
    // Taking it back is not a stop: nothing was under way.
    assert!(stops(&rig).is_empty());

    // What he says next is looked at with no trace of it.
    rig.think
        .then_call("say", serde_json::json!({"text": "at seven"}));
    rig.hear_on(CONSOLE, from_fox("2", "when does it open"), now);
    let later = rig.mind.agenda.next_due().unwrap();
    rig.mind.on_time(later).unwrap();
    rig.until(|r| r.body.sent().len() == 2).await;
    let turn = rig.think.turns.lock().unwrap()[1].clone();
    let given = format!("{:?}", turn.items);
    assert!(given.contains("when does it open"), "{given}");
    assert!(
        !given.contains("tea house close") && !given.contains("half past nine"),
        "{given}"
    );
}

#[tokio::test]
async fn an_answer_already_back_when_she_was_stopped_is_let_go_and_the_next_look_knows_why() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.online(CONSOLE, t0);
    rig.think
        .then_call("say", serde_json::json!({"text": "half past nine"}));
    rig.hear_on(
        CONSOLE,
        from_fox("1", "what time does the tea house close"),
        t0,
    );
    let now = rig.mind.agenda.next_due().unwrap();
    rig.mind.on_time(now).unwrap();
    // The brain has answered, and she has not heard it yet.
    rig.meanwhile(|r| r.mind.jobs.finished("console:fox")).await;
    assert!(rig.mind.interrupt("console:fox", &fox(), now).unwrap());

    // He says something else before it comes in: that look is hers.
    rig.think
        .then_call("say", serde_json::json!({"text": "ok, never mind"}));
    rig.hear_on(CONSOLE, from_fox("2", "actually, never mind"), now);
    let later = rig.until(|r| !r.body.sent().is_empty()).await;
    rig.pump(later + Duration::hours(1)).await;
    assert_eq!(rig.body.sent(), ["ok, never mind"]);

    // The look after the stop was told she had been stopped, and that
    // what was called off stays off.
    let turns = rig.think.turns.lock().unwrap();
    let ghost_brain::Item::User { text } = &turns[1].items[0] else {
        panic!("a look starts with what she sees");
    };
    let told = rig.mind.prompts.fill(
        "stopped_here",
        &[
            ("when", "Just now".to_owned()),
            ("name", "Fox".to_owned()),
            (
                "what",
                rig.mind.prompts.text("stopped_answering").to_owned(),
            ),
        ],
    );
    assert!(text.contains(told.trim()), "{text}");
}

#[tokio::test]
async fn stopped_while_she_is_typing_nothing_she_meant_to_say_is_sent() {
    let t0 = at(14, 0);
    // In a chat, where she types at her own pace: at a terminal she
    // answers at once, and there is no typing to be stopped in.
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"say","text":"half past nine"},{"do":"say","text":"I think"}]}"#,
    );
    rig.hear(dm("wen", "1", "what time does the tea house close"), t0);
    let now = rig
        .until(|r| r.mind.agenda.speaking.contains_key("t:wen"))
        .await;
    assert!(rig.body.sent().is_empty());

    assert!(rig.mind.interrupt("t:wen", &fox(), now).unwrap());
    assert!(rig.mind.agenda.speaking.is_empty());
    rig.pump(now + Duration::hours(1)).await;
    assert!(rig.body.sent().is_empty(), "{:?}", rig.body.sent());
    assert_eq!(
        stops_in(&rig, "t:wen"),
        [(
            "console:fox".to_owned(),
            false,
            vec!["half past nine".to_owned(), "I think".to_owned()],
            vec![]
        )]
    );
}

#[tokio::test]
async fn work_directed_where_she_was_stopped_goes_on_and_is_stopped_only_from_its_own_card() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.online(CONSOLE, t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.hear_on(CONSOLE, from_fox("1", "draw the east gate"), t0);
    let (person, place) = {
        let memory = rig.mind.ghost.memory();
        (
            memory.person("console:fox").unwrap().unwrap(),
            memory.place("console:fox").unwrap().unwrap(),
        )
    };
    assert_eq!(
        rig.mind
            .undertake("Draw the east gate.", Some(&person), Some(&place), None, t0)
            .unwrap(),
        None
    );
    let open = rig.mind.ghost.memory().undertakings(true, 5).unwrap();
    let gate = open
        .iter()
        .find(|u| u.place.as_deref() == Some("console:fox"))
        .unwrap()
        .id
        .clone();
    assert!(open.iter().any(|u| u.directed && u.id == gate));

    assert!(rig.mind.interrupt("console:fox", &fox(), t0).unwrap());
    assert_eq!(
        stops(&rig),
        [("console:fox".to_owned(), true, vec![], vec![])]
    );
    // The work she took on there is untouched: a stop is of the look, as
    // a coding agent's stop leaves its background tasks running.
    let going = rig.mind.ghost.memory().undertaking(&gate).unwrap().unwrap();
    assert!(going.outcome.is_none(), "{going:?}");
    let still = rig.mind.ghost.memory().undertakings(true, 5).unwrap();
    assert_eq!(still.len(), 2);
    // Stopped from its own card, it ends.
    rig.mind.stop_open(going, Some("Fox".into()), t0).unwrap();
    let ended = rig.mind.ghost.memory().undertaking(&gate).unwrap().unwrap();
    assert_eq!(ended.outcome.as_deref(), Some("dropped"));
    let still = rig.mind.ghost.memory().undertakings(true, 5).unwrap();
    assert_eq!(still.len(), 1);
    assert_eq!(still[0].place.as_deref(), Some("t:wen"));
    // What she remembers of it, from the console.
    let remembered = rig
        .mind
        .ghost
        .memory()
        .search(CONSOLE, "stopped", 5)
        .unwrap()
        .into_iter()
        .filter(|hit| hit.kind == "stopped")
        .collect::<Vec<_>>();
    assert_eq!(remembered.len(), 1);
    assert!(
        remembered[0]
            .text
            .starts_with("Fox stopped you in the middle of answering"),
        "{}",
        remembered[0].text
    );
    assert!(
        !remembered[0]
            .text
            .contains("the work you had taken on there"),
        "{}",
        remembered[0].text
    );
}
