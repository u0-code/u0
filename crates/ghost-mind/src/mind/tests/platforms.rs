//! What a platform allows her shapes what she does there: QQ's bot platform
//! lets her answer only for a while after something is said, and seldom
//! lets her speak first.

use super::*;

/// QQ's bot platform as its presence says it: half an hour to answer in
/// private, five minutes in a group, and no speaking first.
fn qq_bot() -> crate::ChannelFacts {
    crate::ChannelFacts {
        answer_direct: Some(std::time::Duration::from_secs(30 * 60)),
        answer_group: Some(std::time::Duration::from_secs(5 * 60)),
        writes_first: false,
        ..crate::ChannelFacts::default()
    }
}

/// `channel` comes online as `facts` say it is.
fn online_as(rig: &mut Rig, channel: &str, facts: crate::ChannelFacts, now: DateTime<Utc>) {
    rig.mind
        .on_signal(
            Signal::Online {
                channel: channel.into(),
                me: None,
                can: vec![Can::Send, Can::Files],
                facts,
            },
            now,
        )
        .unwrap();
}

/// A mind slow enough that, left to her own pace, she would look at a
/// message long after a platform's limit, and let a burst settle for
/// longer than the limit too.
fn slow() -> MindConfig {
    MindConfig {
        live: false,
        tempo: 600.0,
        ..MindConfig::default()
    }
}

#[tokio::test]
async fn where_she_can_answer_only_for_a_while_she_looks_well_within_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(slow(), t0);
    online_as(&mut rig, "qq", qq_bot(), t0);
    online_as(&mut rig, "tg", crate::ChannelFacts::default(), t0);
    rig.hear_on("qq", dm("wen", "1", "are you there"), t0);
    rig.hear_on("tg", dm("wen", "1", "are you there"), t0);
    let qq = rig.mind.agenda.attend["qq:wen"].due;
    let tg = rig.mind.agenda.attend["tg:wen"].due;
    assert!(qq <= t0 + Duration::minutes(15), "{qq}");
    assert!(tg > t0 + Duration::minutes(30), "at her own pace: {tg}");
}

#[tokio::test]
async fn where_she_cannot_speak_first_she_is_not_offered_coming_back_later() {
    let t0 = at(14, 0);
    let offered = |facts: crate::ChannelFacts| async move {
        let mut rig = Rig::with_tools(t0);
        online_as(&mut rig, "qq", facts, t0);
        rig.think
            .then_turn("", &[("say", serde_json::json!({"text": "here"}))]);
        rig.hear_on("qq", dm("wen", "1", "are you there"), t0);
        rig.until(|r| r.body.sent().len() == 1).await;
        rig.think.turns.lock().unwrap()[0]
            .tools
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
    };
    let bot = offered(qq_bot()).await;
    assert!(
        !bot.iter().any(|n| n == "later" || n == "intend"),
        "{bot:?}"
    );
    let anyone = offered(crate::ChannelFacts::default()).await;
    assert!(anyone.iter().any(|n| n == "later") && anyone.iter().any(|n| n == "intend"));
}

#[tokio::test]
async fn where_she_cannot_speak_first_what_she_meant_to_say_later_is_let_go() {
    let t0 = at(14, 0);
    // The test's own channel has a feed; she does not scroll it here.
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            channels: [(
                "t".to_owned(),
                crate::ChannelConfig {
                    browse_minutes: [0, 0],
                    ..crate::ChannelConfig::default()
                },
            )]
            .into(),
            ..MindConfig::default()
        },
        t0,
    );
    online_as(&mut rig, "qq", qq_bot(), t0);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"good luck"}]}"#);
    rig.hear_on("qq", dm("wen", "1", "interview tomorrow"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    let calls = rig.think.calls();
    // Meant for the next day, long after QQ would let her answer.
    rig.mind
        .intend(
            t0 + Duration::hours(20),
            "ask how the interview went".into(),
            Some("qq:wen".into()),
            Some("qq:wen".into()),
            false,
        )
        .unwrap();
    rig.mind.refresh_intentions().unwrap();
    rig.pump(t0 + Duration::hours(21)).await;
    assert_eq!(rig.think.calls(), calls, "no look to say it");
    assert_eq!(rig.body.sent().len(), 1);
    assert!(rig.kinds().contains(&"settled"), "the intention is let go");
}
