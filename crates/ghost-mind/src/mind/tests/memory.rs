//! What she remembers: searching her memory while she answers, what she knows
//! of someone and where she learned it, the note she keeps of a long
//! conversation, and the words a memory came from.

use super::*;

#[tokio::test]
async fn she_can_stop_to_remember() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.mind
        .record(Event::Canon {
            fact: "the teahouse on the east side closes in december".into(),
            source: "life".into(),
        })
        .unwrap();
    rig.think
        .then(r#"{"actions":[{"do":"recall","query":"teahouse"}]}"#);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"december"}]}"#);
    rig.hear(
        dm("haoran", "1", "when does that teahouse close again?"),
        t0,
    );
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.think.calls(), 2);
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("# What you remember"));
    assert!(prompt.contains("closes in december"));
}

#[tokio::test]
async fn what_she_knows_of_someone_is_split_by_where_she_learned_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"hey"}]}"#);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;

    // Before sleep she puts what she knows of Wen in order.
    let people = vec![rig.mind.ghost.memory().person("t:wen").unwrap().unwrap()];
    let draft = super::living::reflect_draft(&serde_json::json!({
        "reflection": "A quiet day.",
        "people": [{"who": "wen", "impression": "", "feel": 0, "account": {
            "shared": "An architect at the design institute. Dry, exact.",
            "private": "Is about to quit and has told nobody."
        }}]
    }))
    .unwrap();
    rig.mind
        .on_reflected(Some("t"), &people, None, Ok(draft), t0)
        .unwrap();

    // Alone with her, she has all of it.
    let t1 = t0 + Duration::hours(1);
    rig.hear(dm("wen", "2", "you around?"), t1);
    rig.until(|r| r.think.calls() == 2).await;
    let private = rig.think.last_prompt();
    assert!(
        private.contains("What you know of them: An architect at the design institute."),
        "{private}"
    );
    assert!(
        private.contains("What they have told you in private: Is about to quit"),
        "{private}"
    );

    // In a group with her, only the part that is for everyone, and the
    // knowledge that there is more.
    let t2 = t1 + Duration::hours(1);
    rig.hear(
        group(
            "streets",
            "wen",
            "3",
            "weiwei, did you see the new line 30 station?",
            true,
        ),
        t2,
    );
    rig.until(|r| r.think.calls() == 3).await;
    let shared = rig.think.last_prompt();
    assert!(
        shared.contains("What you know of them: An architect at the design institute."),
        "{shared}"
    );
    assert!(!shared.contains("about to quit"), "{shared}");
    assert!(
        shared.contains("They have told you more in private. None of it is for here."),
        "{shared}"
    );
}

#[tokio::test]
async fn a_long_conversation_is_kept_as_a_note_and_its_end_word_for_word() {
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
            thread_window: 4,
            ..MindConfig::default()
        },
        t0,
    );
    // Ten messages go by, each read and left unanswered.
    let mut now = t0;
    for i in 1..=10 {
        now += Duration::minutes(20);
        rig.tick
            .then("Wen has been telling you about the tea house, one line at a time.");
        rig.hear(
            dm("wen", &i.to_string(), &format!("tea house fact {i}")),
            now,
        );
        rig.until(|r| r.mind.agenda.attend.is_empty() && r.mind.jobs.is_empty())
            .await;
    }
    // Past twice what she is shown, she wrote the older part down.
    let digests: Vec<(i64, String)> = rig
        .mind
        .ghost
        .journal()
        .read(1, 10_000)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Digested { upto, text, .. } => Some((upto, text)),
            _ => None,
        })
        .collect();
    assert!(!digests.is_empty(), "{:?}", rig.kinds());
    assert!(digests[0].1.contains("tea house"));

    // The next look shows the note, then every message after it, and none
    // of the messages the note stands for.
    now += Duration::minutes(20);
    rig.hear(dm("wen", "11", "are you there?"), now);
    rig.until(|r| r.think.last_prompt().contains("are you there?"))
        .await;
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("What you kept of this conversation"));
    assert!(prompt.contains("one line at a time"));
    assert!(!prompt.contains("tea house fact 1\n"), "{prompt}");
    assert!(prompt.contains("tea house fact 10"));
    let kept_upto = rig
        .mind
        .ghost
        .memory()
        .digest("t:wen")
        .unwrap()
        .unwrap()
        .upto;
    let shown_from = rig
        .mind
        .ghost
        .memory()
        .thread_between("t:wen", kept_upto, i64::MAX, 1)
        .unwrap()[0]
        .text
        .clone();
    assert!(prompt.contains(&shown_from), "nothing falls between");
}

#[tokio::test]
async fn a_conversation_is_shown_its_own_platform_and_her_whole_memory() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.online("q", t0);
    // On the other platform: a message, and what she made of it.
    rig.think.then_turn(
        "",
        &[(
            "note",
            serde_json::json!({"about": "hao", "text": "is opening a noodle shop in March"}),
        )],
    );
    rig.hear_on("q", dm("hao", "1", "my noodle shop opens on the 3rd"), t0);
    rig.until(|r| r.kinds().contains(&"noted")).await;
    // A post of hers there.
    rig.mind
        .ghost
        .record(Event::Posted {
            channel: "q".into(),
            id: None,
            text: "the plane trees on Wenshu street".into(),
        })
        .unwrap();

    // Now a conversation on this platform, in which she searches her memory.
    let later = t0 + Duration::hours(2);
    rig.think
        .then_turn("", &[("recall", serde_json::json!({"query": "noodle"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "noodles, yes"}))]);
    rig.hear(dm("wen", "2", "know any good noodle places?"), later);
    rig.until(|r| r.body.sent().len() == 1).await;

    let turns = rig.think.turns.lock().unwrap();
    let seen = serde_json::to_string(&turns.last().unwrap().items).unwrap();
    // Her memory, whole, and where it was formed.
    assert!(seen.contains("is opening a noodle shop in March"), "{seen}");
    // The other platform's record: neither the message nor her post.
    assert!(!seen.contains("my noodle shop opens on the 3rd"), "{seen}");
    assert!(!seen.contains("the plane trees on Wenshu street"), "{seen}");
}

#[tokio::test]
async fn she_can_go_back_to_the_words_a_memory_came_from() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.online("q", t0);
    // Told something in private, she keeps a note of it.
    rig.think.then_turn(
        "",
        &[
            (
                "note",
                serde_json::json!({"about": "wen", "text": "has an interview on Thursday"}),
            ),
            ("say", serde_json::json!({"text": "good luck"})),
        ],
    );
    rig.hear(
        dm(
            "wen",
            "1",
            "interview thursday at 3. the panel is three people",
        ),
        t0,
    );
    rig.until(|r| r.body.sent().len() == 1).await;
    let noted = rig
        .mind
        .ghost
        .journal()
        .last_of(&["noted"])
        .unwrap()
        .unwrap()
        .seq;

    // Later, with her: recall gives the note its number, and going back to
    // it gives the words.
    let later = t0 + Duration::days(2);
    rig.think
        .then_turn("", &[("recall", serde_json::json!({"query": "interview"}))]);
    rig.think
        .then_turn("", &[("reread", serde_json::json!({"memory": noted}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "three people"}))]);
    rig.hear(dm("wen", "2", "how many were on the panel again?"), later);
    rig.until(|r| r.body.sent().len() == 2).await;
    let result = |rig: &Rig, step: usize| -> String {
        let turns = rig.think.turns.lock().unwrap();
        let n = turns.len();
        match turns[n - step].items.last() {
            Some(ghost_brain::Item::Result { output, .. }) => output.clone(),
            other => panic!("no result: {other:?}"),
        }
    };
    assert!(result(&rig, 2).contains(&format!("[#{noted}]")));
    let words = result(&rig, 1);
    assert!(words.contains("the panel is three people"), "{words}");

    // In a group, what Wen said to her alone stays where it was said.
    rig.think
        .then_turn("", &[("reread", serde_json::json!({"memory": noted}))]);
    rig.think.then_turn("", &[]);
    rig.hear(
        group("streets", "hao", "3", "weiwei, how's wen doing?", true),
        later + Duration::hours(1),
    );
    rig.until(|r| r.think.steps.lock().unwrap().is_empty())
        .await;
    let withheld = result(&rig, 1);
    assert!(withheld.contains("none of it is for here"), "{withheld}");
    assert!(!withheld.contains("three people"), "{withheld}");

    // On another platform, the record of this one is not in front of her.
    rig.think
        .then_turn("", &[("reread", serde_json::json!({"memory": noted}))]);
    rig.think.then_turn("", &[]);
    rig.hear_on("q", dm("hao", "4", "hey"), later + Duration::hours(2));
    rig.until(|r| r.think.steps.lock().unwrap().is_empty())
        .await;
    let elsewhere = result(&rig, 1);
    assert!(elsewhere.contains("That was on t."), "{elsewhere}");

    // A message she remembers comes back with what was around it.
    let asked = rig
        .mind
        .ghost
        .journal()
        .of_kinds_after(&["heard"], 0)
        .unwrap()[1]
        .seq;
    rig.think
        .then_turn("", &[("reread", serde_json::json!({"memory": asked}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "it was three"}))]);
    rig.hear(dm("wen", "5", "?"), later + Duration::hours(3));
    rig.until(|r| r.body.sent().len() == 3).await;
    let around = result(&rig, 1);
    assert!(
        around.contains("panel is three people") && around.contains("how many were on the panel"),
        "{around}"
    );
}

#[tokio::test]
async fn her_past_comes_up_only_when_the_conversation_touches_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let grandmother = "grandmother ran a teahouse";

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"hey"}]}"#);
    rig.hear(dm("wen", "1", "hi, how was your day"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    {
        let seen = rig.think.seen.lock().unwrap();
        let first = seen.last().unwrap();
        // Her voice is shown in who she is, every time.
        assert!(first.system.contains("you up?"), "{}", first.system);
        // Nothing here touches her past, so it is not in front of her.
        assert!(!format!("{:?}", first.messages).contains(grandmother));
        assert!(!first.system.contains(grandmother));
    }

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"long story"}]}"#);
    rig.hear(
        dm("wen", "2", "do you know any old 茶馆 around here?"),
        t0 + Duration::hours(1),
    );
    rig.until(|r| r.body.sent().len() == 2).await;
    let seen = rig.think.seen.lock().unwrap();
    assert!(format!("{:?}", seen.last().unwrap().messages).contains(grandmother));
}

#[tokio::test]
async fn what_was_told_in_someone_elses_private_chat_is_marked_private_like_their_own() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("bo", "1", "between us"), t0);
    rig.hear(group("g", "ann", "2", "hello all", false), t0);
    let note = |seq, place: &str| ghost_core::memory::Note {
        seq,
        at: t0,
        text: format!("note {seq}"),
        place: Some(place.to_owned()),
        sources: None,
    };
    // About ann: two learned from bo in private, one in the group.
    let notes = vec![note(1, "t:bo"), note(2, "t:g"), note(3, "t:bo")];
    let marked: Vec<String> = rig
        .mind
        .with_provenance(notes, "t:g", &[])
        .unwrap()
        .into_iter()
        .map(|n| n.text)
        .collect();
    assert_eq!(
        marked,
        [
            "note 1 (they told you this in private)",
            "note 2",
            "note 3 (they told you this in private)",
        ]
    );
}
