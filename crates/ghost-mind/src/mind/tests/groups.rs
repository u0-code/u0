//! In a group, when nobody is talking to her: how far apart her looks are,
//! the facts she is shown, and the floors that only ever make her quieter.

use super::*;

#[tokio::test]
async fn unaddressed_group_chatter_goes_to_the_cheap_brain_and_can_be_ignored() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let mut message = dm("mei", "1", "anyone seen the new ramen place");
    message.place = Place {
        id: "room".into(),
        kind: PlaceKind::Group,
        name: Some("neighbours".into()),
    };
    message.addressed = false;
    rig.hear(message, t0);
    let looked = rig.until(|r| r.tick.calls() == 1).await;
    assert!(
        looked - t0 >= Duration::minutes(2),
        "skimmed after {}",
        looked - t0
    );
    assert_eq!(rig.think.calls(), 0);
    assert!(
        rig.tick
            .last_prompt()
            .contains("Nobody here said anything to you")
    );
    assert!(rig.body.sent().is_empty());
    assert_eq!(rig.decisions(), vec!["nothing"]);
}

fn place_of(rig: &Rig, key: &str) -> ghost_core::memory::PlaceState {
    rig.mind.ghost.memory().place(key).unwrap().unwrap()
}

/// That her next look at `key` would be stretched `times` over.
#[allow(
    clippy::float_cmp,
    reason = "the stretch is a power of two, which a float holds exactly"
)]
fn assert_stretched(rig: &Rig, key: &str, now: DateTime<Utc>, times: f32) {
    let stretched = rig.mind.backoff(&place_of(rig, key), now).unwrap();
    assert_eq!(stretched, times, "stretched {stretched} times, not {times}");
}

/// Something heard in group `room`, as her journal keeps it.
fn heard_in(room: &str, from: &str, id: &str, text: &str, addressed: bool) -> Event {
    let m = group(room, from, id, text, addressed);
    Event::Heard {
        channel: "t".into(),
        place: m.place,
        from: m.from,
        id: m.id,
        text: m.text,
        reply_to: None,
        addressed,
        images: Vec::new(),
    }
}

fn said_in(room: &str, id: &str, text: &str) -> Event {
    Event::Said {
        channel: "t".into(),
        place: room.into(),
        id: Some(id.into()),
        text: text.into(),
        reply_to: None,
        file: None,
    }
}

/// Put what happened in a group into her past, each at its time, and
/// mark all of it read.
fn lived_in_group(rig: &mut Rig, room: &str, events: Vec<(DateTime<Utc>, Event)>) {
    let mut last = None;
    for (when, event) in events {
        rig.mind.ghost.record_at(when, event).unwrap();
        last = Some(when);
    }
    let upto = place_of(rig, &format!("t:{room}")).last_in_seq;
    rig.mind
        .ghost
        .record_at(
            last.unwrap(),
            Event::Read {
                channel: "t".into(),
                place: room.into(),
                upto,
            },
        )
        .unwrap();
}

/// Chatter in a group nobody addresses to her, which she looks at and lets
/// pass `times` times over, starting at `now`. Returns when she last looked.
async fn lets_pass(rig: &mut Rig, times: usize, now: DateTime<Utc>) -> DateTime<Utc> {
    let mut now = now;
    let start = rig.tick.calls();
    for n in 1..=times {
        rig.hear(
            group(
                "room",
                "mei",
                &format!("pass-{start}-{n}"),
                "chatter",
                false,
            ),
            now,
        );
        now = rig.until(|r| r.tick.calls() == start + n).await;
    }
    now
}

#[tokio::test]
async fn each_silent_look_at_a_group_stretches_the_next_up_to_eight_times_and_she_still_comes_back()
{
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let mut now = t0;
    let mut stretched = Vec::new();
    let mut waited = Vec::new();
    for n in 1..=5 {
        rig.hear(group("room", "mei", &n.to_string(), "chatter", false), now);
        stretched.push(rig.mind.backoff(&place_of(&rig, "t:room"), now).unwrap());
        let looked = rig.until(|r| r.tick.calls() == n).await;
        waited.push(looked - now);
        now = looked;
    }
    assert_eq!(stretched, [1.0, 2.0, 4.0, 8.0, 8.0]);
    // Free, she looks after two to fifteen minutes; stretched eight times,
    // after sixteen at the least, and never more than two hours.
    assert!(waited[0] >= Duration::minutes(2) && waited[0] <= Duration::minutes(15));
    for wait in &waited[3..] {
        assert!(
            *wait >= Duration::minutes(16) && *wait <= Duration::hours(2),
            "{wait}"
        );
    }
    assert!(rig.body.sent().is_empty());
}

#[tokio::test]
async fn a_group_that_never_pauses_still_gets_her_look() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    // Someone speaks every two seconds, for an hour: never long enough for
    // a burst to settle.
    let mut now = t0;
    let mut looked = None;
    for n in 0..1800 {
        rig.hear(
            group("room", "mei", &format!("c{n}"), "chatter", false),
            now,
        );
        rig.pump(now).await;
        if rig.tick.calls() > 0 {
            looked = Some(now);
            break;
        }
        now += Duration::seconds(2);
    }
    let looked = looked.expect("she looked while they were still talking");
    // Free, she looks at a group after two to fifteen minutes; the chatter
    // does not hold her off past that.
    assert!(looked - t0 <= Duration::minutes(16), "{}", looked - t0);
}

#[tokio::test]
async fn a_burst_or_her_name_brings_her_back_at_the_usual_pace() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let now = lets_pass(&mut rig, 3, t0).await;
    rig.hear(group("room", "mei", "b0", "chatter", false), now);
    assert_stretched(&rig, "t:room", now, 8.0);
    let due = rig.mind.agenda.attend["t:room"].due;
    // Five more make a burst of six, and she does not wait as long.
    for n in 1..6 {
        rig.hear(group("room", "lao", &format!("b{n}"), "what?!", false), now);
    }
    assert_stretched(&rig, "t:room", now, 1.0);
    assert!(rig.mind.agenda.attend["t:room"].due <= due);
    let now = rig.until(|r| r.tick.calls() == 4).await;

    let now = lets_pass(&mut rig, 3, now).await;
    // Her name, without an @, is not said to her, and still catches her eye.
    rig.hear(
        group("room", "mei", "n1", "ask weiwei, she would know", false),
        now,
    );
    let place = place_of(&rig, "t:room");
    assert_eq!(place.unread_addressed, 0);
    assert_stretched(&rig, &place.key, now, 1.0);
}

#[tokio::test]
async fn a_friend_writing_in_a_group_brings_her_back_at_the_usual_pace() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    befriend(&mut rig, "lan", t0);
    let now = lets_pass(&mut rig, 3, t0).await;
    rig.hear(group("room", "mei", "s1", "more chatter", false), now);
    assert_stretched(&rig, "t:room", now, 8.0);
    rig.hear(
        group("room", "lan", "f1", "back from the coast", false),
        now,
    );
    assert_stretched(&rig, "t:room", now, 1.0);
}

#[tokio::test]
async fn the_group_facts_she_sees_include_her_share_her_last_unprompted_line_and_whether_it_was_answered()
 {
    let t0 = at(14, 0);
    let history = |answered: bool| {
        let mut events = vec![
            (
                t0 - Duration::hours(3),
                heard_in("room", "mei", "1", "anyone been to the night market", false),
            ),
            (
                t0 - Duration::hours(3) + Duration::minutes(1),
                said_in("room", "s1", "the one by the river? went last week"),
            ),
        ];
        if answered {
            events.push((
                t0 - Duration::hours(3) + Duration::minutes(4),
                heard_in("room", "mei", "2", "@weiwei was it any good", true),
            ));
        }
        events.push((
            t0 - Duration::hours(2),
            heard_in("room", "lao", "3", "rain again", false),
        ));
        events
    };

    let mut rig = Rig::quiet(t0);
    lived_in_group(&mut rig, "room", history(false));
    rig.hear(
        group("room", "lao", "4", "anyone up for noodles", false),
        t0,
    );
    rig.until(|r| r.tick.calls() == 1).await;
    let prompt = rig.tick.last_prompt();
    assert!(prompt.contains("Nobody here said anything to you"));
    assert!(prompt.contains("  - 1 of the last 4 messages here are yours."));
    assert!(prompt.contains("and 2 message(s) from others have come since."));
    assert!(prompt.contains(
        "(\"the one by the river? went last week\"), nobody answered you or turned to you within 10 minutes."
    ));

    let mut rig = Rig::quiet(t0);
    lived_in_group(&mut rig, "room", history(true));
    rig.hear(
        group("room", "lao", "4", "anyone up for noodles", false),
        t0,
    );
    rig.until(|r| r.tick.calls() == 1).await;
    let prompt = rig.tick.last_prompt();
    assert!(prompt.contains("  - 1 of the last 5 messages here are yours."));
    assert!(prompt.contains("someone answered you or turned to you within 10 minutes."));
}

#[tokio::test]
async fn a_message_to_someone_else_is_shown_as_theirs() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(group("room", "mei", "1", "@lao 你的梯子还在吗", false), t0);
    rig.until(|r| r.tick.calls() == 1).await;
    let prompt = rig.tick.last_prompt();
    assert!(prompt.contains("The newest message, from mei, is to lao: theirs, not yours."));
    assert!(prompt.contains("You have never said anything here."));
}

fn bot_in(room: &str, id: &str, text: &str) -> Incoming {
    let mut m = group(room, "minutes", id, text, true);
    m.from.automated = true;
    m
}

#[tokio::test]
async fn after_two_rounds_with_bots_and_no_person_she_stops_answering_them_until_a_person_writes() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    for n in 1..=4 {
        rig.think.then(&format!(
            r#"{{"actions":[{{"do":"say","text":"pong {n}"}}]}}"#
        ));
    }
    rig.hear(bot_in("room", "1", "@weiwei ping"), t0);
    let now = rig.until(|r| r.body.sent().len() == 1).await;
    rig.hear(bot_in("room", "2", "@weiwei ping"), now);
    let now = rig.until(|r| r.body.sent().len() == 2).await;

    // A third time, with no person in between: it is left read, and no
    // brain is asked.
    rig.hear(bot_in("room", "3", "@weiwei ping"), now);
    let now = rig
        .until(|r| r.decisions().last().map(String::as_str) == Some("stayed out of it"))
        .await;
    assert_eq!(rig.think.calls(), 2);
    assert_eq!(place_of(&rig, "t:room").unread, 0);

    // A person writes, and she answers again.
    rig.hear(
        group("room", "mei", "4", "@weiwei are you a bot too", true),
        now,
    );
    let now = rig.until(|r| r.body.sent().len() == 3).await;
    // With a person just now in between, the bot is answered once more.
    rig.hear(bot_in("room", "5", "@weiwei ping"), now);
    rig.until(|r| r.body.sent().len() == 4).await;
    assert_eq!(rig.think.calls(), 4);
}

#[tokio::test]
async fn quiet_stops_unaddressed_looks_until_it_runs_out_and_she_still_answers_when_spoken_to() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"好，我安静会儿"},{"do":"quiet","hours":2}]}"#);
    rig.hear(group("room", "mei", "1", "@weiwei 你能安静点吗", true), t0);
    let now = rig.until(|r| r.body.sent().len() == 1).await;
    assert!(rig.kinds().contains(&"quieted"));
    assert!(place_of(&rig, "t:room").quiet_at(now));

    // Her body stops and starts again: she is still quiet, from her journal.
    let mut rig = rig.restart(now);
    rig.hear(group("room", "lao", "2", "她终于安静了", false), now);
    rig.hear(group("room", "mei", "3", "哈哈", false), now);
    assert!(!rig.mind.agenda.attend.contains_key("t:room"));
    rig.pump(now + Duration::minutes(90)).await;
    assert_eq!(rig.tick.calls() + rig.think.calls(), 0);

    // Spoken to, she answers.
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"在呢"}]}"#);
    let later = now + Duration::minutes(90);
    rig.hear(group("room", "lao", "4", "@weiwei 你还在吗", true), later);
    let later = rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.tick.calls(), 0);

    // Once it runs out, the chatter is looked at again.
    let after = now + Duration::hours(2) + Duration::minutes(1);
    assert!(after > later);
    rig.hear(group("room", "mei", "5", "晚上吃什么", false), after);
    rig.until(|r| r.tick.calls() == 1).await;
}

#[tokio::test]
async fn a_word_she_put_in_unasked_and_nobody_took_up_leaves_the_chatter_alone_for_a_while() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.tick
        .then(r#"{"actions":[{"do":"say","text":"湿毛巾擦脖子"}]}"#);
    rig.hear(group("room", "lao", "1", "热死了", false), t0);
    let now = rig.until(|r| r.body.sent().len() == 1).await;

    // The others go on without a word to her: no look, and none due before
    // the pause is over.
    rig.hear(group("room", "mei", "2", "空调也坏了", false), now);
    let due = rig.mind.agenda.attend["t:room"].due;
    assert!(due >= now + Duration::minutes(29), "{due} vs {now}");
    rig.pump(now + Duration::minutes(20)).await;
    assert_eq!(rig.tick.calls(), 1);

    // Spoken to, she answers at once.
    rig.think.then(r#"{"actions":[{"do":"say","text":"热"}]}"#);
    let later = now + Duration::minutes(20);
    rig.hear(group("room", "lao", "3", "@weiwei 你那边热吗", true), later);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.tick.calls(), 1);
}

#[tokio::test]
async fn her_name_ends_the_pause_after_an_unasked_word_even_without_an_at() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let name = rig.mind.ghost.identity().name().to_owned();
    rig.tick
        .then(r#"{"actions":[{"do":"say","text":"湿毛巾擦脖子"}]}"#);
    rig.hear(group("room", "lao", "1", "热死了", false), t0);
    let now = rig.until(|r| r.body.sent().len() == 1).await;

    // Called by name, not @-ed: that is someone turning to her.
    rig.tick.then(r#"{"actions":[{"do":"say","text":"还行"}]}"#);
    let text = format!("{name}你那边热不热");
    rig.hear(group("room", "mei", "2", &text, false), now);
    let due = rig.mind.agenda.attend["t:room"].due;
    assert!(due < now + Duration::minutes(29), "{due} vs {now}");
    rig.until(|r| r.body.sent().len() == 2).await;
}

#[tokio::test]
async fn quiet_is_offered_only_in_groups() {
    let t0 = at(14, 0);
    let offered = |rig: &Rig| -> Vec<String> {
        rig.think.turns.lock().unwrap()[0]
            .tools
            .iter()
            .map(|t| t.name.clone())
            .collect()
    };
    let mut rig = Rig::with_tools(t0);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    assert!(!offered(&rig).contains(&"quiet".to_owned()));

    let mut rig = Rig::with_tools(t0);
    rig.hear(group("room", "wen", "1", "@weiwei hi", true), t0);
    rig.until(|r| r.think.turns.lock().unwrap().len() == 1)
        .await;
    assert!(offered(&rig).contains(&"quiet".to_owned()));
}

#[tokio::test]
async fn an_operator_can_keep_her_to_being_spoken_to_in_a_place_and_tell_her_about_another() {
    let t0 = at(14, 0);
    let mut config = MindConfig {
        public: crate::PublicConfig {
            browse_minutes: [0, 0],
            ..crate::PublicConfig::default()
        },
        live: false,
        ..MindConfig::default()
    };
    config.places.insert(
        "t:work".into(),
        crate::PlaceConfig {
            addressed_only: true,
            note: None,
        },
    );
    config.places.insert(
        "t:club".into(),
        crate::PlaceConfig {
            addressed_only: false,
            note: Some("this group prefers you keep it light".into()),
        },
    );
    let mut rig = Rig::new(config, t0);
    rig.hear(group("work", "mei", "1", "standup in 5", false), t0);
    rig.pump(t0 + Duration::hours(3)).await;
    assert_eq!(rig.tick.calls(), 0);

    let now = t0 + Duration::hours(3);
    rig.hear(group("work", "mei", "2", "@weiwei can you join", true), now);
    rig.until(|r| r.think.calls() == 1).await;

    let now = now + Duration::minutes(10);
    rig.hear(
        group("club", "lao", "3", "who's around tonight", false),
        now,
    );
    rig.until(|r| r.tick.calls() == 1).await;
    assert!(rig.tick.last_prompt().contains(
        "  - The people who run your body say of this place: this group prefers you keep it light"
    ));
}

#[tokio::test]
async fn in_a_private_chat_silence_neither_stretches_her_next_look_nor_shows_group_facts() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let mut now = t0;
    for n in 1..=3 {
        rig.hear(dm("haoran", &n.to_string(), "hm"), now);
        now = rig.until(|r| r.think.calls() == n).await;
    }
    rig.hear(dm("haoran", "4", "hm"), now);
    let place = place_of(&rig, "t:haoran");
    assert_stretched(&rig, &place.key, now, 1.0);
    assert!(!rig.mind.holds_back(&place, now));
    assert!(!rig.mind.in_a_loop_with_bots(&place).unwrap());
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains("This is a private conversation."));
    assert!(!prompt.contains("What is so here"));
}
