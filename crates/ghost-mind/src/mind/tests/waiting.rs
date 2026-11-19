//! What a console is shown as waiting on the people who run her.

use serde_json::json;

use super::*;

/// Write `event` into her journal at `at`, as her mind would have.
fn lived(rig: &mut Rig, at: DateTime<Utc>, event: Event) -> i64 {
    rig.mind.ghost.record_at(at, event).unwrap().seq
}

fn took_on(rig: &mut Rig, at: DateTime<Utc>, id: &str, asked_by: Option<&str>, directed: bool) {
    lived(
        rig,
        at,
        Event::Undertook {
            id: id.into(),
            brief: format!("the brief of {id}"),
            asked_by: asked_by.map(Into::into),
            place: None,
            directed,
            dir: None,
        },
    );
}

fn said(rig: &mut Rig, at: DateTime<Utc>, place: &str, text: &str) {
    lived(
        rig,
        at,
        Event::Said {
            channel: "t".into(),
            place: place.into(),
            id: None,
            text: text.into(),
            reply_to: None,
            file: None,
        },
    );
}

#[tokio::test]
async fn a_console_is_shown_what_waits_on_her_operators_newest_first_and_nothing_older() {
    let t0 = at(14, 0);
    let minutes = |m: i64| t0 + Duration::minutes(m);
    let mut rig = Rig::new(operated(), t0);

    // Long ago: work that gave up, and work that is still waiting.
    took_on(&mut rig, t0 - Duration::days(5), "old", None, false);
    let long_ago = lived(
        &mut rig,
        t0 - Duration::days(5),
        Event::Concluded {
            id: "old".into(),
            outcome: "gave_up".into(),
            summary: "nobody knew".into(),
        },
    );
    took_on(&mut rig, t0 - Duration::days(4), "rota", None, false);
    let asked = lived(
        &mut rig,
        t0 - Duration::days(4),
        Event::Asked {
            id: "rota".into(),
            question: "which week?".into(),
        },
    );
    rig.mind
        .undertaking("rota", "the brief of rota")
        .unwrap()
        .update(|s| {
            s.ask(
                "which week?",
                Some(ghost_work::undertaking::AskKind::Choose),
                vec!["this".into(), "next".into()],
            );
        })
        .unwrap();

    // Lately: work that was blocked, and work for the boss that told her
    // something, beside work for someone else that did.
    took_on(&mut rig, minutes(1), "gate", None, false);
    let blocked = lived(
        &mut rig,
        minutes(2),
        Event::Concluded {
            id: "gate".into(),
            outcome: "blocked".into(),
            summary: "the site wants a login".into(),
        },
    );
    took_on(&mut rig, minutes(3), "boss-work", Some("t:boss"), false);
    took_on(&mut rig, minutes(3), "mei-work", Some("t:mei"), false);
    let told = lived(
        &mut rig,
        minutes(4),
        Event::Reported {
            id: "boss-work".into(),
            text: "halfway there".into(),
            files: vec!["/work/draft.md".into()],
        },
    );
    lived(
        &mut rig,
        minutes(4),
        Event::Reported {
            id: "mei-work".into(),
            text: "for mei only".into(),
            files: Vec::new(),
        },
    );

    // Her report to the boss, what she said in it, and the talk after.
    let meant = lived(
        &mut rig,
        minutes(5),
        Event::Intended {
            due: minutes(5),
            what: "tell boss about the day".into(),
            about: Some("t:boss".into()),
            place: Some("t:boss".into()),
            owed: true,
            directed: false,
            report: true,
        },
    );
    lived(
        &mut rig,
        minutes(6),
        Event::Decided {
            about: "t:boss".into(),
            decision: "said 2 things".into(),
            reason: String::new(),
        },
    );
    lived(
        &mut rig,
        minutes(6),
        Event::Settled {
            intention: meant,
            outcome: "said 2 things".into(),
        },
    );
    said(&mut rig, minutes(7), "boss", "a quiet day at the clinic");
    said(&mut rig, minutes(7), "boss", "two foals, both fine");
    let mut answer = dm("boss", "9", "good to hear");
    answer.addressed = true;
    rig.hear(answer, minutes(8));
    said(&mut rig, minutes(9), "boss", "and you?");

    let shown = serde_json::to_value(rig.reader().waiting(minutes(10)).unwrap()).unwrap();
    let ids: Vec<&str> = shown
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            format!("report-{meant}"),
            format!("told-{told}"),
            format!("ended-{blocked}"),
            format!("asked-{asked}"),
        ]
    );
    assert!(!shown.to_string().contains(&format!("ended-{long_ago}")));
    assert!(!shown.to_string().contains("for mei only"));

    let report = &shown[0];
    assert_eq!(report["place"], "t:boss");
    assert_eq!(
        report["text"],
        "a quiet day at the clinic\ntwo foals, both fine"
    );
    let told = &shown[1];
    assert_eq!(
        (&told["kind"], &told["work"], &told["text"]),
        (&json!("told"), &json!("boss-work"), &json!("halfway there"))
    );
    assert_eq!(told["files"], json!(["/work/draft.md"]));
    let ended = &shown[2];
    assert_eq!(
        (&ended["outcome"], &ended["text"], &ended["brief"]),
        (
            &json!("blocked"),
            &json!("the site wants a login"),
            &json!("the brief of gate")
        )
    );
    // A question stays for as long as it waits, with what answering takes.
    let question = &shown[3];
    assert_eq!(question["kind"], "question");
    assert_eq!(question["seq"], asked);
    assert_eq!(question["text"], "which week?");
    assert_eq!(question["ask"], "choose");
    assert_eq!(question["options"], json!(["this", "next"]));

    // Once answered, it is no longer waiting on anyone.
    lived(
        &mut rig,
        minutes(11),
        Event::Added {
            id: "rota".into(),
            text: "next".into(),
            by: Some("console:fox".into()),
        },
    );
    let shown = serde_json::to_value(rig.reader().waiting(minutes(12)).unwrap()).unwrap();
    assert!(!shown.to_string().contains("asked-"), "{shown}");
    // Nothing was written to be shown any of it.
    assert_eq!(
        rig.mind.ghost.journal().last_seq().unwrap(),
        rig.mind
            .ghost
            .journal()
            .last_of(&["added"])
            .unwrap()
            .unwrap()
            .seq
    );
}
