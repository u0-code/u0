//! What a brain is told: the requests recorded word for word, her persona
//! before her being, and how she comes to a conversation.

use super::*;

/// Every clock time written as `HH:MM`. When a message came in is when the
/// test ran, and when she looks is a moment her tempo draws at random.
fn without_times(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if i + 5 <= b.len()
            && b[i + 2] == b':'
            && [0, 1, 3, 4].iter().all(|&k| b[i + k].is_ascii_digit())
        {
            out.extend_from_slice(b"HH:MM");
            i += 5;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// Compare `got` with what was recorded under `name`, or record it when
/// `GHOST_RECORD` is set. The ghost's directory, which changes from run to
/// run, is written as `<dir>`, and the times messages came in, which are
/// when the test ran, are left out of the comparison.
fn compare_recorded(rig: &Rig, name: &str, got: &str) {
    let got = got.replace(&rig.mind.ghost.dir().display().to_string(), "<dir>");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/requests")
        .join(format!("{name}.txt"));
    if std::env::var_os("GHOST_RECORD").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let (want, got) = (
        without_times(&std::fs::read_to_string(&path).unwrap()),
        without_times(&got),
    );
    if want != got {
        let at = want
            .lines()
            .zip(got.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(want.lines().count().min(got.lines().count()));
        panic!(
            "{name} differs from what was recorded, first at line {}:\nrecorded: {:?}\nnow:      {:?}",
            at + 1,
            want.lines().nth(at),
            got.lines().nth(at)
        );
    }
}

fn turns_text(turns: &[ghost_brain::Turn]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for t in turns {
        let _ = write!(
            out,
            "system: {}\nitems: {:#?}\ntools: {:#?}\n\n",
            t.system, t.items, t.tools
        );
    }
    out
}

#[tokio::test]
async fn with_nothing_overridden_she_asks_exactly_what_she_asked_before() {
    let t0 = at(14, 0);
    // Answering in JSON.
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("wen", "1", "is the tea house open?"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    let seen = format!("{:#?}", rig.think.seen.lock().unwrap());
    compare_recorded(&rig, "talk_json", &seen);

    // Answering with tools, with memory searched in between.
    let mut rig = Rig::with_tools(t0);
    rig.think
        .then_turn("", &[("recall", serde_json::json!({"query": "tea"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "nine, I think"}))]);
    rig.hear(dm("wen", "1", "is the tea house open?"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    let turns = turns_text(&rig.think.turns.lock().unwrap());
    compare_recorded(&rig, "talk_tools", &turns);

    // At work for a friend.
    let mut rig = Rig::quiet(t0);
    at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "Nine on weekdays."}),
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let turns = turns_text(&rig.think.turns.lock().unwrap());
    compare_recorded(&rig, "work", &turns);
}

/// Her system prompt split where being begins: her persona, then the rest.
fn persona_and_being(system: &str) -> (&str, &str) {
    let begins = Prompts::shipped().being.lines().next().unwrap().to_owned();
    let at = system.find(&begins).expect("being is in the system prompt");
    system.split_at(at)
}

#[test]
fn the_system_prompt_is_her_persona_then_her_being_then_what_is_true_of_her_life() {
    let t0 = at(14, 0);
    let quiet = MindConfig {
        live: false,
        browse_minutes: [0, 0],
        ..MindConfig::default()
    };
    let rig = Rig::build_as(OTHER, quiet, t0);
    let system = rig.mind.who(t0).unwrap();
    // The order, as a brain reads it.
    let order = [
        "You are 大肥鱼 (he/him). You live in Wuhan, CN.",
        "# Who you are",
        "# How you talk",
        "# Where you came from",
        "You value: generosity",
        "You are not an assistant and not a character being played.",
        "You keep things you like",
        "# Whose word counts",
        "# Ground rules",
        "# Things that are true about your life",
        "(nothing registered yet beyond who you are)",
    ];
    let at: Vec<usize> = order
        .iter()
        .map(|part| system.find(part).unwrap_or_else(|| panic!("no {part:?}")))
        .collect();
    assert!(at.is_sorted(), "{order:?} are at {at:?}");
    assert!(system.starts_with(order[0]));
    let (persona, being) = persona_and_being(&system);
    for own in OTHERS_OWN {
        assert!(persona.contains(own), "{own} is not in his persona");
        assert!(!being.contains(own), "{own} is in what is shared");
    }

    // With the persona template emptied, nothing of his is left in it: his
    // fields reach his brain through that template and no other.
    let deployment = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(deployment.path().join("character")).unwrap();
    std::fs::write(deployment.path().join("character/persona.md"), "").unwrap();
    let mut rig = rig;
    rig.mind.prompts = Prompts::load(Some(deployment.path()));
    let system = rig.mind.who(t0).unwrap();
    assert!(system.trim_start().starts_with(order[5]));
    for own in OTHERS_OWN {
        assert!(!system.contains(own), "{own} came in without the persona");
    }
}

#[tokio::test]
async fn two_characters_are_told_the_same_being_logic_and_work_in_their_own_persona() {
    let t0 = at(14, 0);
    let talk = |seed: &'static str| async move {
        let mut rig = Rig::build_as(
            seed,
            MindConfig {
                live: false,
                browse_minutes: [0, 0],
                ..MindConfig::default()
            },
            t0,
        );
        rig.think
            .hands
            .store(true, std::sync::atomic::Ordering::Relaxed);
        rig.think
            .then_turn("", &[("say", serde_json::json!({"text": "nine"}))]);
        rig.hear(dm("wen", "1", "is the tea house open?"), t0);
        rig.until(|r| r.body.sent().len() == 1).await;
        rig.think.turns.lock().unwrap()[0].clone()
    };
    let work = |seed: &'static str| async move {
        let mut rig = Rig::build_as(
            seed,
            MindConfig {
                live: false,
                browse_minutes: [0, 0],
                ..MindConfig::default()
            },
            t0,
        );
        at_work_for_wen(&mut rig, "Find out when the tea house closes.", t0);
        rig.think.then_call(
            "done",
            serde_json::json!({"outcome": "finished", "summary": "Nine on weekdays."}),
        );
        rig.until(|r| r.kinds().contains(&"concluded")).await;
        rig.think.turns.lock().unwrap()[0].clone()
    };
    for (hers, his) in [
        (talk(SEED).await, talk(OTHER).await),
        (work(SEED).await, work(OTHER).await),
    ] {
        let (her_persona, her_being) = persona_and_being(&hers.system);
        let (his_persona, his_being) = persona_and_being(&his.system);
        assert_ne!(her_persona, his_persona);
        assert!(his_persona.contains("大肥鱼") && !her_persona.contains("大肥鱼"));
        // Being, and for work what she is told about it after: the same.
        assert_eq!(her_being, his_being);
        // The conversation or the work itself, and the tools: the same.
        let rest = |turn: &ghost_brain::Turn| {
            without_times(&format!("{:#?}\n{:#?}", turn.items, turn.tools))
        };
        assert_eq!(rest(&hers), rest(&his));
    }
}

/// [`SEED`] with two sides to her.
fn with_sides() -> String {
    format!(
        r#"{SEED}
[[facet]]
name = "粘人"
when = "With people she likes, when they come back after a while."
examples = "them: 我回来了\nWeiwei: 你还知道回来"

[[facet]]
name = "嘴毒"
when = "When she is teased or kept waiting."
"#
    )
}

/// A mind at human pace, answering in JSON, with no life engine, for the
/// character `seed` describes.
fn quiet_as(seed: &str, now: DateTime<Utc>) -> Rig {
    Rig::build_as(
        seed,
        MindConfig {
            talk_tools: false,
            live: false,
            browse_minutes: [0, 0],
            ..MindConfig::default()
        },
        now,
    )
}

/// The message `m`, as her journal keeps it.
fn as_heard(m: Incoming) -> Event {
    Event::Heard {
        channel: "t".into(),
        place: m.place,
        from: m.from,
        id: m.id,
        text: m.text,
        reply_to: None,
        addressed: m.addressed,
        images: Vec::new(),
    }
}

/// A friend, ming, whom she wrote to last night and who got to her twice
/// after that before he said sorry; and at `t0` he writes again.
fn a_friend_who_kept_her_waiting(rig: &mut Rig, t0: DateTime<Utc>) {
    befriend(rig, "ming", t0 - Duration::hours(15));
    let feel = |delta, why: &str| Event::Felt {
        toward: "t:ming".into(),
        delta,
        why: why.into(),
    };
    for (when, event) in [
        (
            t0 - Duration::hours(14),
            Event::Said {
                channel: "t".into(),
                place: "ming".into(),
                id: Some("out-old".into()),
                text: "你吃饭了没".into(),
                reply_to: None,
                file: None,
            },
        ),
        (
            t0 - Duration::hours(13),
            feel(-1, "called me 大肥鱼 in the group"),
        ),
        (t0 - Duration::hours(12), feel(-1, "called me 大肥鱼 again")),
        (t0 - Duration::hours(11), feel(1, "said sorry, sort of")),
        (t0, as_heard(dm("ming", "new", "早"))),
    ] {
        rig.mind.ghost.record_at(when, event).unwrap();
    }
    rig.mind.schedule_attend("t:ming", t0).unwrap();
}

#[tokio::test]
async fn how_she_comes_to_a_conversation_says_the_hour_what_she_felt_and_why_and_who_kept_her_waiting()
 {
    // She wakes at 08:30.
    let t0 = at(9, 0);
    let mut rig = quiet_as(&with_sides(), t0);
    a_friend_who_kept_her_waiting(&mut rig, t0);
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();

    let at = prompt
        .find("# How you come to this")
        .expect("there is something to say");
    let block = &prompt[at..prompt.find("# What is happening").unwrap()];
    for fact in [
        "- You woke up not long ago.",
        "- Today you have been leaning a little toward your ",
        "- ming is a friend.",
        "ming got to you (what did it, as you put it then: called me 大肥鱼 again).",
        "you warmed to ming (what did it, as you put it then: said sorry, sort of).",
        "- It was not the first time: ming has got to you more than once since yesterday.",
        "(\"你吃饭了没\"); this is the first they have said since.",
    ] {
        assert!(block.contains(fact), "{fact:?} is not in:\n{block}");
    }
    // Only the latest two of what she felt are said, oldest first.
    assert!(!block.contains("in the group"), "{block}");
    assert!(block.find("got to you").unwrap() < block.find("warmed to").unwrap());
    // No labels and no numbers: facts and their causes.
    assert!(
        !block.contains("mood") && !block.contains("warmth"),
        "{block}"
    );
    // The rule for sides is said with them.
    assert!(block.contains("never their looks, their family, an illness or where they are from"));

    // It comes late: after the conversation and her moment, just before
    // what is happening.
    let conversation = prompt.find("# The conversation").unwrap();
    let moment = prompt.find("# Right now").unwrap();
    assert!(conversation < moment && moment < at, "{prompt}");
}

#[tokio::test]
async fn todays_lean_is_one_of_her_sides_and_none_is_drawn_for_a_persona_without_them() {
    let t0 = at(9, 0);
    let mut rig = quiet_as(&with_sides(), t0);
    a_friend_who_kept_her_waiting(&mut rig, t0);
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    assert!(
        ["粘人", "嘴毒"]
            .iter()
            .any(|side| prompt.contains(&format!("leaning a little toward your {side} side"))),
        "{prompt}"
    );

    // Without sides, the same morning is still said, and no lean is.
    let mut rig = quiet_as(SEED, t0);
    a_friend_who_kept_her_waiting(&mut rig, t0);
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("- You woke up not long ago."), "{prompt}");
    assert!(!prompt.contains("leaning"), "{prompt}");
}

#[tokio::test]
async fn with_nothing_to_say_about_how_she_comes_to_it_the_block_is_left_out() {
    // The middle of her afternoon, someone new, and no sides to lean.
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("wen", "1", "is the tea house open?"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    assert!(!rig.think.last_prompt().contains("# How you come to this"));
}

#[tokio::test]
async fn in_a_group_she_has_what_she_felt_toward_someone_but_not_what_was_said_in_private() {
    let t0 = at(14, 0);
    let mut rig = quiet_as(SEED, t0);
    befriend(&mut rig, "ming", t0 - Duration::hours(3));
    rig.mind
        .ghost
        .record_at(
            t0 - Duration::hours(2),
            Event::Felt {
                toward: "t:ming".into(),
                delta: -2,
                why: "laughed about what I told him about my mother".into(),
            },
        )
        .unwrap();
    rig.mind
        .ghost
        .record_at(
            t0,
            as_heard(group("room", "ming", "g1", "@weiwei 在吗", true)),
        )
        .unwrap();
    rig.mind.schedule_attend("t:room", t0).unwrap();
    rig.until(|r| r.think.calls() >= 1).await;
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("ming got to you."), "{prompt}");
    assert!(!prompt.contains("my mother"), "{prompt}");
}

#[tokio::test]
async fn her_sides_are_set_out_under_who_she_is_and_a_persona_without_them_reads_as_before() {
    let t0 = at(14, 0);
    let mut rig = quiet_as(&with_sides(), t0);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    let system = rig.think.seen.lock().unwrap()[0].system.clone();
    let sides =
        &system[system.find("# Who you are").unwrap()..system.find("# How you talk").unwrap()];
    for part in [
        "quiet\n\nThere is more than one side to you",
        "## 粘人\n\nWhen it comes out: With people she likes, when they come back after a while.",
        "How it sounds. Again, not lines to reuse:\n\nthem: 我回来了\nWeiwei: 你还知道回来\n\n## 嘴毒",
        "## 嘴毒\n\nWhen it comes out: When she is teased or kept waiting.\n\n",
    ] {
        assert!(sides.contains(part), "{part:?} is not in:\n{sides}");
    }

    let mut rig = Rig::quiet(t0);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    let system = rig.think.seen.lock().unwrap()[0].system.clone();
    assert!(
        system.contains("# Who you are\n\nquiet\n\n# How you talk"),
        "{system}"
    );
}
