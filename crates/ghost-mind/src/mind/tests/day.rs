//! Her day: planned and lived without anyone asking, carried from one place
//! to the next, and looked back on at night.

use super::*;

#[tokio::test]
async fn her_day_is_planned_and_lived_without_anyone_asking() {
    let t0 = at(9, 0);
    let mut rig = Rig::new(
        MindConfig {
            weather: false,
            browse_minutes: [0, 0],
            ..MindConfig::default()
        },
        t0,
    );
    rig.tick.then(
        r#"{"blocks":[
            {"from":"09:30","to":"11:00","what":"sketching the teahouse on the east side","reach":"busy"},
            {"from":"13:00","to":"14:00","what":"lunch","reach":"free"}],
            "canon":["has started sketching the old teahouse"]}"#,
    );
    rig.tick
        .then(r#"{"text":"Got the roofline down. The light was flat.","canon":[]}"#);
    rig.pump(t0).await;
    assert!(rig.kinds().ends_with(&["woke", "planned", "canon"]));

    let written = rig.until(|r| r.kinds().contains(&"lived")).await;
    assert!(
        written >= at(11, 0) && written < at(11, 31),
        "written at {written}"
    );
    // What she lived is now part of who a brain is told she is.
    let system = rig.mind.self_prompt(written).unwrap();
    assert!(system.contains("Got the roofline down."));
    assert!(system.contains("has started sketching the old teahouse"));
    // And while sketching she was slow to look at her phone.
    let moment = rig
        .mind
        .life
        .moment(rig.mind.ghost.journal(), at(10, 0))
        .unwrap();
    assert_eq!(moment.reach, ghost_core::life::Reach::Busy);
}

#[tokio::test]
async fn she_carries_her_day_between_places_but_not_what_was_private() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[
            {"do":"say","text":"that is a lot to carry"},
            {"do":"note","about":"wen","text":"her sister is moving to Kunming and she has told nobody"}]}"#,
    );
    rig.hear(
        dm(
            "wen",
            "1",
            "my sister is moving to kunming. i haven't told anyone",
        ),
        t0,
    );
    rig.until(|r| r.body.sent().len() == 1).await;

    // Later, in a group where Wen also is.
    let t1 = t0 + Duration::minutes(30);
    rig.hear(
        group(
            "streets",
            "wen",
            "2",
            "anyone going to the market on sunday",
            false,
        ),
        t1,
    );
    rig.hear(
        group("streets", "bo", "3", "weiwei, are you coming?", true),
        t1,
    );
    rig.until(|r| r.think.calls() == 2).await;
    let prompt = rig.think.last_prompt();

    assert!(prompt.contains("# The rest of your day"), "{prompt}");
    assert!(
        prompt.contains("A private chat with wen on t: 1 from them, 1 from you"),
        "{prompt}"
    );
    // The note is there, because she knows it, and marked, because of where
    // she learned it.
    assert!(
        prompt.contains("her sister is moving to Kunming and she has told nobody (they told you this in private)"),
        "{prompt}"
    );

    // Back in the private chat, the same note carries no such mark.
    let t2 = t1 + Duration::hours(1);
    rig.hear(dm("wen", "4", "thanks for earlier"), t2);
    rig.until(|r| r.think.calls() == 3).await;
    let private = rig.think.last_prompt();
    assert!(private.contains("she has told nobody"));
    assert!(
        !private.contains("(they told you this in private)"),
        "{private}"
    );
    assert!(
        private.contains("The group \"old streets\" on t"),
        "{private}"
    );
}

#[tokio::test]
async fn she_looks_back_on_her_own_day_and_on_each_platform_apart() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.online("q", t0);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"oh no"}]}"#);
    rig.hear(dm("wen", "1", "the institute is a mess this week"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"I'll come"}]}"#);
    rig.hear_on(
        "q",
        dm("hao", "2", "my noodle shop opens on the 3rd"),
        t0 + Duration::hours(1),
    );
    rig.until(|r| r.body.sent().len() == 2).await;

    // Every look back gets the same answer, naming both of them: each can
    // only keep what it says about the people in front of it.
    let answer = r#"{"reflection": "A day.", "people": [
        {"who": "wen", "impression": "worn down by the institute"},
        {"who": "hao", "impression": "about to open a noodle shop"}]}"#;
    for _ in 0..3 {
        rig.think.then(answer);
    }
    let night = t0 + Duration::hours(10);
    let before = rig.think.calls();
    rig.mind.cfg.live = true;
    rig.mind.reflect(night).unwrap();
    while let Some(done) = rig.mind.jobs.join_next().await {
        rig.mind.on_outcome(done.unwrap(), night).unwrap();
    }

    // One look at her own day, and one at each platform.
    let asked: Vec<String> = rig.think.seen.lock().unwrap()[before..]
        .iter()
        .map(|c| format!("{:?}", c.messages))
        .collect();
    assert_eq!(asked.len(), 3);
    let on = |channel: &str| {
        asked
            .iter()
            .find(|p| p.contains(&format!("you look back on {channel}:")))
            .unwrap_or_else(|| panic!("no look back on {channel}"))
    };
    assert!(on("t").contains("institute") && !on("t").contains("noodle"));
    assert!(on("q").contains("noodle") && !on("q").contains("institute"));
    let own = asked
        .iter()
        .find(|p| p.contains("You also talked with people today, on q, t."))
        .expect("a look back on her own day");
    assert!(!own.contains("noodle") && !own.contains("institute"));

    // Each is kept with what it is about and what it was made from.
    let mut looked: Vec<Option<String>> = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["reflected"], 0)
        .unwrap()
        .into_iter()
        .map(|e| match e.event {
            Event::Reflected {
                channel, sources, ..
            } => {
                let sources = sources.expect("what it was made from");
                assert!(sources.since <= t0 && sources.until == night);
                channel
            }
            _ => unreachable!(),
        })
        .collect();
    looked.sort();
    assert_eq!(looked, [None, Some("q".into()), Some("t".into())]);
    let memory = rig.mind.ghost.memory();
    let of = |key: &str| -> Vec<String> {
        memory
            .notes(key, 10)
            .unwrap()
            .into_iter()
            .map(|n| n.text)
            .collect()
    };
    assert_eq!(of("t:wen"), ["worn down by the institute"]);
    assert_eq!(of("q:hao"), ["about to open a noodle shop"]);

    // With Wen, she can go back to what that impression was made from:
    // what Wen said to her, and nothing from the other platform.
    let note = memory.notes("t:wen", 10).unwrap()[0].seq;
    let direct = memory.person("t:wen").unwrap().unwrap().direct.unwrap();
    let here = memory.place(&direct).unwrap().unwrap();
    let words = rig.mind.reread(note, &here, night).unwrap();
    assert!(
        words.contains("the institute is a mess this week"),
        "{words}"
    );
    assert!(!words.contains("noodle"), "{words}");
}
