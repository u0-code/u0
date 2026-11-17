//! In public: scrolling, posting, the read-over a post gets before it goes
//! out, and how she learns from the way her posts did.

use super::*;

#[tokio::test]
async fn scrolling_mostly_ends_in_nothing_and_sometimes_in_a_post() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [10, 10],
            posts_per_day: 1,
            public_likes: true,
            public_replies: true,
            gate: false,
            ..MindConfig::default()
        },
        t0,
    );
    let mut post = dm(
        "mei",
        "tweet-1",
        "the plane trees on renmin road are finally turning",
    );
    post.place = Place {
        id: "tweet-1".into(),
        kind: PlaceKind::Public,
        name: None,
    };
    post.addressed = false;
    rig.body.timeline.lock().unwrap().push(post);
    rig.think.then(
        r#"{"thought":"autumn is late this year","actions":[
            {"do":"react","to":"i1","emoji":"❤"},
            {"do":"say","reply_to":1,"text":"saw them this morning"},
            {"do":"post","text":"flat light all day. good for roofs, bad for everything else"},
            {"do":"post","text":"a second post that is over the daily limit"}]}"#,
    );
    let scrolled = rig.until(|r| r.kinds().contains(&"saw")).await;
    assert_eq!(scrolled - t0, Duration::minutes(10));
    rig.until(|r| r.kinds().contains(&"said") && r.kinds().contains(&"posted"))
        .await;
    assert_eq!(
        rig.body
            .count(|a| matches!(a, Act::React { to, .. } if to == "tweet-1")),
        1
    );
    assert_eq!(rig.body.count(|a| matches!(a, Act::Post { .. })), 1);
    assert_eq!(
        rig.body
            .count(|a| matches!(a, Act::Send { reply_to: Some(to), .. } if to == "tweet-1")),
        1
    );
    // Only what she engaged with entered her journal.
    assert_eq!(rig.kinds().iter().filter(|k| **k == "heard").count(), 1);

    // The next time she picks up her phone the same posts are still there.
    // She has seen them and has posted enough: no brain call, no second like.
    let fetches = |r: &Rig| r.body.count(|a| matches!(a, Act::Fetch { .. }));
    rig.until(|r| fetches(r) == 2).await;
    assert_eq!(rig.think.calls(), 1);
    assert_eq!(rig.body.count(|a| matches!(a, Act::React { .. })), 1);
}

#[tokio::test]
async fn in_public_her_messages_hang_off_each_other() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"actions":[{"do":"say","text":"i walk past it most mornings"},{"do":"say","text":"where did you hear that"}]}"#,
    );
    let mut mention = dm("mei", "tweet-9", "@weiwei is the teahouse really closing?");
    mention.place = Place {
        id: "thread-9".into(),
        kind: PlaceKind::Public,
        name: None,
    };
    rig.hear(mention, t0);
    rig.until(|r| r.body.sent().len() == 2).await;
    let replies: Vec<Option<String>> = rig
        .body
        .acts
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, act)| match act {
            Act::Send { reply_to, .. } => Some(reply_to.clone()),
            _ => None,
        })
        .collect();
    // The first answers their post; the second continues her own.
    assert_eq!(replies[0].as_deref(), Some("tweet-9"));
    assert!(
        replies[1]
            .as_deref()
            .is_some_and(|id| id.starts_with("out-"))
    );
}

fn public_post(id: &str, from: &str, text: &str) -> Incoming {
    let mut post = dm(from, id, text);
    post.place = Place {
        id: id.into(),
        kind: PlaceKind::Public,
        name: None,
    };
    post.addressed = false;
    post
}

#[tokio::test]
async fn by_default_she_neither_likes_nor_replies_to_strangers_posts() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [10, 10],
            gate: false,
            ..MindConfig::default()
        },
        t0,
    );
    rig.body.timeline.lock().unwrap().push(public_post(
        "tweet-1",
        "mei",
        "the plane trees are turning",
    ));
    // Even if the brain asks for them, the platform's rules hold.
    rig.think.then(
        r#"{"thought":"nice","actions":[
            {"do":"react","to":1,"emoji":"❤"},
            {"do":"say","reply_to":1,"text":"saw them too"},
            {"do":"post","text":"flat light all day"}]}"#,
    );
    rig.until(|r| r.kinds().contains(&"posted")).await;
    assert_eq!(rig.body.count(|a| matches!(a, Act::React { .. })), 0);
    assert_eq!(rig.body.count(|a| matches!(a, Act::Send { .. })), 0);
    // And they are not offered in the first place.
    let prompt = rig.think.last_prompt();
    assert!(!prompt.contains("\"do\":\"react\""), "{prompt}");
    assert!(prompt.contains("\"do\":\"post\""));
}

#[tokio::test]
async fn a_post_is_read_over_and_held_if_it_carries_something_private() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [10, 10],
            ..MindConfig::default()
        },
        t0,
    );
    // Something told to her in private, which she wrote down.
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"note","about":"wen","text":"is quitting her job at the institute next month"}]}"#,
    );
    rig.hear(
        dm(
            "wen",
            "1",
            "i'm quitting the institute next month. nobody knows yet",
        ),
        t0,
    );
    rig.until(|r| r.kinds().contains(&"noted")).await;

    // Scrolling later, she drafts a post that leans on it.
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"post","text":"a friend is about to walk out of a design institute and i think she is right"}]}"#,
    );
    rig.tick
        .then(r#"{"verdict":"drop","why":"this is what Wen told me in private"}"#);
    rig.until(|r| r.decisions().iter().any(|d| d == "did not post"))
        .await;
    assert_eq!(rig.body.count(|a| matches!(a, Act::Post { .. })), 0);
    assert!(!rig.kinds().contains(&"posted"));

    // The read-over was shown the private note and the draft.
    let gate = rig.tick.last_prompt();
    assert!(
        gate.contains("about wen: is quitting her job at the institute next month"),
        "{gate}"
    );
    assert!(gate.contains("walk out of a design institute"));

    // A draft that is fine goes out, revised if the read-over says so.
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"post","text":"flat light all day, good for roofs"}]}"#,
    );
    rig.tick.then(
        r#"{"verdict":"revise","text":"flat light all day. good for roofs.","why":"tighter"}"#,
    );
    rig.until(|r| r.kinds().contains(&"posted")).await;
    assert_eq!(
        rig.body.count(
            |a| matches!(a, Act::Post { text } if text == "flat light all day. good for roofs.")
        ),
        1
    );
}

#[tokio::test]
async fn she_sees_how_her_posts_did_and_keeps_what_she_learns() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [10, 10],
            gate: false,
            ..MindConfig::default()
        },
        t0,
    );
    // This channel can say how her posts are doing.
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: None,
                can: vec![Can::Post, Can::Feed, Can::Metrics],
                kind: crate::Kind::Chat,
            },
            t0,
        )
        .unwrap();
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"post","text":"the door on kuixinglou street"}]}"#);
    rig.until(|r| r.kinds().contains(&"posted")).await;
    let id = rig
        .mind
        .ghost
        .journal()
        .read(1, 10_000)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.event {
            Event::Posted { id, .. } => id,
            _ => None,
        })
        .unwrap();
    *rig.body.standing.lock().unwrap() = Some(crate::Measure {
        followers: Some(412),
        posts: vec![ghost_core::journal::PostStats {
            id,
            likes: 31,
            replies: 4,
            reposts: 7,
            views: 2900,
        }],
    });

    // The next time she scrolls the channel is asked, and from then on the
    // numbers are in front of her when she scrolls.
    let next = rig.mind.agenda.phone.unwrap();
    rig.pump(next).await;
    assert!(rig.kinds().contains(&"measured"));
    let next = rig.mind.agenda.phone.unwrap();
    rig.pump(next).await;
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("412 people follow you on t"), "{prompt}");
    assert!(
        prompt.contains(
            "\"the door on kuixinglou street\": 31 likes, 4 replies, 7 reposts, seen 2900 times"
        ),
        "{prompt}"
    );

    // What she works out from it is kept, and a later lesson can replace it.
    rig.mind
        .learn(vec![(
            "one street, one detail does better than a thread".into(),
            None,
        )])
        .unwrap();
    let first = rig.mind.ghost.memory().craft(5).unwrap()[0].seq;
    rig.mind
        .learn(vec![(
            "a single door or window, said plainly, is what people answer".into(),
            Some(first),
        )])
        .unwrap();
    let held = rig.mind.ghost.memory().craft(5).unwrap();
    assert_eq!(held.len(), 1);
    assert!(held[0].lesson.starts_with("a single door"));
    let next = rig.mind.agenda.phone.unwrap();
    rig.pump(next).await;
    assert!(
        rig.think
            .last_prompt()
            .contains("a single door or window, said plainly")
    );
}

#[tokio::test]
async fn once_a_week_she_looks_at_how_her_writing_is_going() {
    let t0 = at(9, 0);
    let mut rig = Rig::quiet(t0);
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: None,
                can: vec![Can::Post, Can::Feed, Can::Metrics],
                kind: crate::Kind::Chat,
            },
            t0,
        )
        .unwrap();
    // A brain that can work, with nothing queued yet.
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // Nothing to look at yet: she does not sit down to it.
    rig.mind.consider_review(t0).unwrap();
    assert!(!rig.kinds().contains(&"undertook"));

    // Six posts and how they did.
    for i in 0..6u64 {
        rig.mind
            .record(Event::Posted {
                channel: "t".into(),
                id: Some(format!("p{i}")),
                text: format!("post number {i} about a doorway"),
            })
            .unwrap();
    }
    rig.mind
        .record(Event::Measured {
            channel: "t".into(),
            followers: Some(300),
            posts: (0..6u64)
                .map(|i| ghost_core::journal::PostStats {
                    id: format!("p{i}"),
                    likes: i * 5,
                    replies: i,
                    ..Default::default()
                })
                .collect(),
        })
        .unwrap();
    rig.mind.consider_review(t0).unwrap();
    let review = &rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0];
    assert!(review.id.starts_with("review-"));
    assert!(review.asked_by.is_none());
    assert!(
        review
            .brief
            .contains("\"post number 5 about a doorway\": 25 likes, 5 replies"),
        "{}",
        review.brief
    );
    // And not again the same week.
    rig.mind.consider_review(t0 + Duration::days(2)).unwrap();
    assert_eq!(
        rig.mind
            .ghost
            .memory()
            .undertakings(false, 5)
            .unwrap()
            .len(),
        1
    );

    // What it comes to is kept as craft.
    let done = || {
        serde_json::json!({"outcome": "finished", "summary":
            "Looked at six posts and four writers.\nLESSON: one doorway, described plainly, is what people answer\nTRY: post one photograph-sized observation a day instead of a thread"})
    };
    rig.think.then_call("done", done());
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let craft: Vec<String> = rig
        .mind
        .ghost
        .memory()
        .craft(5)
        .unwrap()
        .into_iter()
        .map(|l| l.lesson)
        .collect();
    assert_eq!(
        craft,
        [
            "one doorway, described plainly, is what people answer",
            "This week, try: post one photograph-sized observation a day instead of a thread"
        ]
    );
}
