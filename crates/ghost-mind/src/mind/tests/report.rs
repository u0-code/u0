//! Her daily report to the operators she talks with in private: when it is
//! due, what it lists, and what it leaves out.

use super::*;

/// Her attention, with `boss` and `carol` on `t` as operators of hers, who
/// are told about her day at the time it is told by default.
fn reporting() -> MindConfig {
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
        operators: crate::Operators {
            accounts: [("t".to_owned(), vec!["boss".to_owned(), "carol".to_owned()])].into(),
            ..crate::Operators::default()
        },
        ..MindConfig::default()
    }
}

/// Every report she has meant to make: to whom, where, and when it was due.
fn reports(rig: &Rig) -> Vec<(String, String, DateTime<Utc>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["intended"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Intended {
                due,
                about: Some(about),
                place: Some(place),
                owed: true,
                report: true,
                ..
            } => Some((about, place, due)),
            _ => None,
        })
        .collect()
}

/// The prompts she was shown when she looked at a report she meant to make.
fn report_prompts(rig: &Rig) -> Vec<String> {
    let what = rig
        .mind
        .prompts
        .fill("report_what", &[("name", "boss".to_owned())]);
    rig.think
        .seen
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.messages[0].text.clone())
        .filter(|p| p.contains(&what))
        .collect()
}

/// `boss` writes to her in private, and she looks at it.
async fn boss_writes(rig: &mut Rig, now: DateTime<Utc>) {
    rig.hear(dm("boss", "1", "how's your week going"), now);
    rig.until(|r| r.think.calls() == 1).await;
}

fn looked_up(query: &str, place: Option<&str>) -> Event {
    Event::LookedUp {
        query: query.to_owned(),
        found: format!("what came back for {query}"),
        place: place.map(str::to_owned),
    }
}

fn opened_page(url: &str) -> Event {
    Event::Opened {
        url: url.to_owned(),
        text: "a page".to_owned(),
        file: None,
        place: None,
    }
}

#[tokio::test]
async fn at_report_time_she_means_to_report_to_an_operator_she_talks_with_in_private_and_not_to_one_she_does_not()
 {
    let t0 = at(14, 0);
    let mut rig = Rig::new(reporting(), t0);
    boss_writes(&mut rig, t0).await;
    rig.pump(at(20, 59)).await;
    assert!(reports(&rig).is_empty());

    rig.pump(at(21, 0)).await;
    // Carol is an operator too, but has never written to her in private:
    // there is nowhere she could tell her.
    assert_eq!(
        reports(&rig),
        [("t:boss".to_owned(), "t:boss".to_owned(), at(21, 0))]
    );
    // She looked at it then, as she would at anything she meant to do,
    // with how to tell it in front of her.
    let looked = report_prompts(&rig);
    assert_eq!(looked.len(), 1);
    let how = rig
        .mind
        .prompts
        .report
        .lines()
        .find(|l| l.starts_with("How to tell them"))
        .unwrap();
    assert!(looked[0].contains(how), "{}", looked[0]);
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty(),
        "she settled it"
    );
}

#[tokio::test]
async fn the_report_lists_what_she_looked_up_opened_and_kept_since_her_last_report_and_not_before()
{
    let t0 = at(14, 0);
    let mut rig = Rig::new(reporting(), t0);
    boss_writes(&mut rig, t0).await;
    rig.mind
        .record(looked_up("how paper lanterns are made", None))
        .unwrap();
    rig.mind
        .record(opened_page("https://lanterns.example/history"))
        .unwrap();
    rig.pump(at(21, 0)).await;
    assert!(report_prompts(&rig)[0].contains("how paper lanterns are made"));

    // The next day.
    rig.mind
        .record(looked_up("the Wenshu temple fair", None))
        .unwrap();
    rig.mind
        .record(opened_page("https://fair.example/programme"))
        .unwrap();
    let note = "a lantern shaped like a rabbit, for when someone needs cheering up";
    let kept = rig
        .mind
        .record(Event::Kept {
            file: "rabbit.png".to_owned(),
            note: note.to_owned(),
            from: Some("https://fair.example/rabbit.png".to_owned()),
        })
        .unwrap();
    rig.pump(at(21, 0) + Duration::days(1)).await;
    assert_eq!(reports(&rig).len(), 2);
    let second = report_prompts(&rig).pop().unwrap();
    let p = &rig.mind.prompts;
    for shown in [
        p.fill(
            "report_looked_up",
            &[("query", "the Wenshu temple fair".to_owned())],
        ),
        p.fill(
            "report_opened",
            &[("url", "https://fair.example/programme".to_owned())],
        ),
        // By the number she can hand it over with.
        p.fill(
            "report_kept_line",
            &[("thing", kept.seq.to_string()), ("note", note.to_owned())],
        ),
    ] {
        assert!(second.contains(&shown), "{shown} is missing:\n{second}");
    }
    for told_before in ["how paper lanterns are made", "lanterns.example/history"] {
        assert!(!second.contains(told_before), "{second}");
    }
}

#[tokio::test]
async fn what_someone_told_her_in_private_is_not_in_her_report() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(reporting(), t0);
    boss_writes(&mut rig, t0).await;
    rig.hear(
        dm("mei", "1", "my sister is pregnant, nobody knows yet"),
        t0 + Duration::minutes(5),
    );
    rig.until(|r| r.think.calls() == 2).await;
    // What she looked up for Mei in private is Mei's; what she looked up
    // on her own is hers to tell.
    rig.mind
        .record(looked_up(
            "how early can you tell you are pregnant",
            Some("t:mei"),
        ))
        .unwrap();
    rig.mind
        .record(looked_up("lantern festival dates", None))
        .unwrap();
    rig.pump(at(21, 0)).await;

    let report = report_prompts(&rig).pop().unwrap();
    assert!(!report.contains("pregnant"), "{report}");
    assert!(!report.contains("nobody knows"), "{report}");
    assert!(report.contains("lantern festival dates"), "{report}");
    let p = &rig.mind.prompts;
    let mei = rig.mind.ghost.memory().place("t:mei").unwrap().unwrap();
    // That she talked with Mei, and how much, is there.
    let talked = p.fill(
        "report_talked_line",
        &[
            ("place", rig.mind.place_words(&mei, &[], false).unwrap()),
            ("heard", "1".to_owned()),
            ("said", "0".to_owned()),
        ],
    );
    assert!(report.contains(&talked), "{report}");
    // And the search is counted, not shown.
    let counted = p.fill("report_private", &[("count", "1".to_owned())]);
    assert!(report.contains(&counted), "{report}");
}

#[tokio::test]
async fn what_she_did_in_an_operators_own_private_conversation_is_in_their_report() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(reporting(), t0);
    boss_writes(&mut rig, t0).await;
    // The report is told in that same conversation, to the person who was
    // in it: the rule that keeps a private conversation in its place does
    // not keep it from itself.
    rig.mind
        .record(looked_up("quarterly figures for boss", Some("t:boss")))
        .unwrap();
    rig.pump(at(21, 0)).await;

    let report = report_prompts(&rig).pop().unwrap();
    assert!(report.contains("quarterly figures for boss"), "{report}");
    let counted = rig
        .mind
        .prompts
        .fill("report_private", &[("count", "1".to_owned())]);
    assert!(!report.contains(&counted), "{report}");
}

#[tokio::test]
async fn she_reports_once_a_day_and_a_restart_does_not_make_it_twice() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(reporting(), t0);
    boss_writes(&mut rig, t0).await;
    // Her body is not running at nine: started again at ten, she makes the
    // report she missed, at once.
    let mut rig = rig.restart(at(22, 0));
    rig.pump(at(22, 0)).await;
    assert_eq!(
        reports(&rig),
        [("t:boss".to_owned(), "t:boss".to_owned(), at(21, 0))]
    );
    assert_eq!(report_prompts(&rig).len(), 1);

    // Later that evening, and after another restart, it is still made.
    rig.pump(at(23, 0)).await;
    let mut rig = rig.restart(at(23, 30));
    rig.pump(at(23, 30)).await;
    assert_eq!(reports(&rig).len(), 1);
    assert_eq!(
        rig.think.calls(),
        0,
        "the new body did not look at it again"
    );

    // The next day is another report.
    rig.pump(at(21, 0) + Duration::days(1)).await;
    assert_eq!(reports(&rig).len(), 2);
    assert_eq!(reports(&rig)[1].2, at(21, 0) + Duration::days(1));
}

#[tokio::test]
async fn an_empty_report_time_means_she_never_reports() {
    let t0 = at(14, 0);
    let mut config = reporting();
    config.operators.report = None;
    let mut rig = Rig::new(config, t0);
    boss_writes(&mut rig, t0).await;
    assert_eq!(rig.mind.agenda.report, None);
    rig.pump(at(21, 0)).await;
    rig.pump(at(21, 0) + Duration::days(1)).await;
    assert!(reports(&rig).is_empty());
}

#[tokio::test]
async fn a_report_that_falls_due_while_she_sleeps_waits_for_her_to_wake() {
    let t0 = at(1, 0);
    let mut config = reporting();
    config.operators.report = chrono::NaiveTime::from_hms_opt(2, 0, 0);
    let mut rig = Rig::new(config, t0);
    // Boss wrote to her in the night: she has a private conversation with
    // them, though she has not looked at it yet.
    rig.hear(dm("boss", "1", "you still up?"), t0);
    rig.pump(at(2, 0)).await;
    assert_eq!(
        reports(&rig),
        [("t:boss".to_owned(), "t:boss".to_owned(), at(2, 0))]
    );
    // Meant, and not acted on while she sleeps.
    rig.pump(at(8, 0)).await;
    assert!(report_prompts(&rig).is_empty());
    let looked = rig.until(|r| !report_prompts(r).is_empty()).await;
    assert!(looked >= at(8, 30), "she reported at {looked}");
}
