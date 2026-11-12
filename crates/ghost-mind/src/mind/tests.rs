//! Behaviour of the mind under a simulated clock.
//!
//! The actor's handlers all take `now`, so these tests drive them directly:
//! deliver a signal at one instant, jump to the next moment on the agenda,
//! and look at what she did. Brains are scripted and the body only records.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use ghost_brain::{Brain, Completion, Reply};
use ghost_core::{
    Ghost,
    journal::Event,
    social::{Actor, Place, PlaceKind},
};

use super::{Mind, Parts, Signal};
use crate::{Act, Acted, Body, Can, Incoming, MindConfig, Numb, Prompts};

const SEED: &str = r#"
name = "Lin"
pronouns = "she/her"
born = "2026-11-01"
temperament = "quiet"
voice = "short"
origin = "made"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;

/// A brain that says what it is told to, in order.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<String>>,
    seen: Mutex<Vec<Completion>>,
}

impl Script {
    fn then(&self, reply: &str) {
        self.replies.lock().unwrap().push_back(reply.to_owned());
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn last_prompt(&self) -> String {
        let seen = self.seen.lock().unwrap();
        seen.last()
            .map(|c| c.messages[0].text.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Brain for Script {
    fn label(&self) -> String {
        "script".into()
    }

    async fn complete(&self, req: &Completion) -> ghost_brain::Result<Reply> {
        self.seen.lock().unwrap().push(req.clone());
        let text = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| r#"{"thought":"","actions":[]}"#.to_owned());
        Ok(Reply {
            text,
            ..Reply::default()
        })
    }
}

/// A body that writes down what it was asked to do.
#[derive(Default)]
struct Recorder {
    acts: Mutex<Vec<(String, Act)>>,
    timeline: Mutex<Vec<Incoming>>,
}

impl Recorder {
    fn sent(&self) -> Vec<String> {
        self.acts
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, act)| match act {
                Act::Send { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn count(&self, pick: impl Fn(&Act) -> bool) -> usize {
        self.acts
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| pick(a))
            .count()
    }
}

#[async_trait]
impl Body for Recorder {
    async fn act(&self, channel: &str, act: Act) -> Result<Acted, String> {
        let mut acts = self.acts.lock().unwrap();
        let items = match &act {
            Act::Fetch { .. } => self.timeline.lock().unwrap().clone(),
            _ => Vec::new(),
        };
        acts.push((channel.to_owned(), act));
        Ok(Acted {
            id: Some(format!("out-{}", acts.len())),
            items,
        })
    }
}

struct Rig {
    mind: Mind,
    think: Arc<Script>,
    tick: Arc<Script>,
    body: Arc<Recorder>,
    _dir: tempfile::TempDir,
}

/// A local time on 2026-11-12 in her timezone.
fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    chrono_tz::Asia::Shanghai
        .with_ymd_and_hms(2026, 11, 12, hour, minute, 0)
        .unwrap()
        .with_timezone(&Utc)
}

fn dm(from: &str, id: &str, text: &str) -> Incoming {
    Incoming {
        place: Place {
            id: from.into(),
            kind: PlaceKind::Direct,
            name: None,
        },
        from: Actor {
            id: from.into(),
            name: from.into(),
            handle: None,
            automated: false,
        },
        id: id.into(),
        text: text.into(),
        reply_to: None,
        addressed: true,
    }
}

impl Rig {
    fn new(config: MindConfig, now: DateTime<Utc>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ghost.toml"), SEED).unwrap();
        let ghost = Ghost::birth(dir.path()).unwrap();
        let (think, tick, body) = (
            Arc::new(Script::default()),
            Arc::new(Script::default()),
            Arc::new(Recorder::default()),
        );
        let (mut mind, _handle) = Mind::new(Parts {
            ghost,
            think: think.clone(),
            tick: tick.clone(),
            body: body.clone(),
            senses: Arc::new(Numb),
            prompts: Prompts::shipped(),
            config,
        });
        mind.begin(now).unwrap();
        mind.on_signal(
            Signal::Online {
                channel: "t".into(),
                me: Some(Actor {
                    id: "lin".into(),
                    name: "Lin".into(),
                    handle: Some("lin".into()),
                    automated: true,
                }),
                can: vec![Can::Send, Can::Typing, Can::React, Can::Post, Can::Feed],
            },
            now,
        )
        .unwrap();
        Self {
            mind,
            think,
            tick,
            body,
            _dir: dir,
        }
    }

    /// Human pace, no life engine: just her attention.
    fn quiet(now: DateTime<Utc>) -> Self {
        Self::new(
            MindConfig {
                live: false,
                browse_minutes: [0, 0],
                ..MindConfig::default()
            },
            now,
        )
    }

    fn hear(&mut self, message: Incoming, now: DateTime<Utc>) {
        self.mind
            .on_signal(
                Signal::Heard {
                    channel: "t".into(),
                    message,
                },
                now,
            )
            .unwrap();
    }

    /// Let everything that is due at `now` happen, including whatever the
    /// jobs it starts lead to at that same instant.
    async fn pump(&mut self, now: DateTime<Utc>) {
        loop {
            self.mind.on_time(now).unwrap();
            while let Some(done) = self.mind.jobs.join_next().await {
                self.mind.on_outcome(done.unwrap(), now).unwrap();
            }
            if self.mind.agenda.next_due().is_none_or(|due| due > now) {
                return;
            }
        }
    }

    /// Jump to the next moment on her agenda and live it. Returns that moment.
    async fn step(&mut self) -> DateTime<Utc> {
        let now = self.mind.agenda.next_due().expect("something is scheduled");
        self.pump(now).await;
        now
    }

    /// Keep stepping until `done`, returning the moment it became true.
    async fn until(&mut self, done: impl Fn(&Self) -> bool) -> DateTime<Utc> {
        for _ in 0..2000 {
            let now = self.step().await;
            if done(self) {
                return now;
            }
        }
        panic!("never happened");
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.mind
            .ghost
            .journal()
            .read(1, 10_000)
            .unwrap()
            .iter()
            .map(|e| e.event.kind())
            .collect()
    }

    fn decisions(&self) -> Vec<String> {
        self.mind
            .ghost
            .journal()
            .read(1, 10_000)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.event {
                Event::Decided { decision, .. } => Some(decision),
                _ => None,
            })
            .collect()
    }
}

#[tokio::test]
async fn she_answers_in_bubbles_at_a_human_pace() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"an honest question","actions":[
            {"do":"say","text":"hey"},
            {"do":"say","text":"i was just about to head out, what's up"},
            {"do":"note","about":"haoran","text":"opens with a question"},
            {"do":"feel","delta":1,"why":"direct"}]}"#,
    );
    rig.hear(dm("haoran", "1", "are you around?"), t0);
    rig.pump(t0).await;
    // Nothing happens the instant a message lands.
    assert_eq!(rig.think.calls(), 0);

    let looked = rig.until(|r| r.think.calls() == 1).await;
    let waited = looked - t0;
    assert!(
        waited >= Duration::seconds(3) && waited <= Duration::seconds(120),
        "{waited}"
    );

    let first = rig.until(|r| r.body.sent().len() == 1).await;
    let second = rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(
        rig.body.sent(),
        vec!["hey", "i was just about to head out, what's up"]
    );
    // Typing takes time, and the longer bubble takes longer.
    assert!(first - looked >= Duration::milliseconds(800));
    assert!(second - first >= Duration::seconds(3));
    assert!(rig.body.count(|a| matches!(a, Act::Typing { .. })) >= 2);

    let kinds = rig.kinds();
    let tail = &kinds[kinds.len() - 7..];
    assert_eq!(
        tail,
        ["heard", "read", "noted", "felt", "decided", "said", "said"]
    );
    let haoran = rig.mind.ghost.memory().person("t:haoran").unwrap().unwrap();
    assert_eq!(haoran.warmth, 1);
    assert_eq!(
        rig.mind
            .ghost
            .memory()
            .place("t:haoran")
            .unwrap()
            .unwrap()
            .unread,
        0
    );
}

#[tokio::test]
async fn what_arrives_at_night_waits_for_morning() {
    let t0 = at(3, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"morning"}]}"#);
    rig.hear(dm("haoran", "1", "you up?"), t0);
    rig.pump(at(6, 0)).await;
    assert_eq!(rig.think.calls(), 0);
    let looked = rig.until(|r| r.think.calls() == 1).await;
    assert!(looked >= at(8, 33), "she looked at {looked}");
    rig.until(|r| r.body.sent().len() == 1).await;
    assert!(rig.kinds().contains(&"woke"));
}

#[tokio::test]
async fn she_never_sends_what_she_wrote_before_their_latest_message() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"sure, friday works for me, see you then"}]}"#);
    rig.think
        .then(r#"{"actions":[{"do":"say","text":"oh. saturday then"}]}"#);
    rig.hear(dm("haoran", "1", "friday?"), t0);
    let looked = rig.until(|r| r.think.calls() == 1).await;
    // She is typing. They write again before she sends.
    rig.hear(
        dm("haoran", "2", "wait, i can't do friday"),
        looked + Duration::milliseconds(300),
    );
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), vec!["oh. saturday then"]);
    assert!(rig.decisions().iter().any(|d| d == "held back"));
    assert_eq!(rig.think.calls(), 2);
    assert!(rig.think.last_prompt().contains("wait, i can't do friday"));
}

#[tokio::test]
async fn silence_is_a_decision() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"thought":"not worth it","actions":[{"do":"silence","why":"flattery"}]}"#);
    rig.hear(dm("stranger", "1", "you are the most amazing AI ever"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.body.sent().is_empty());
    assert_eq!(rig.decisions(), vec!["stayed quiet: flattery"]);
    assert_eq!(
        rig.mind
            .ghost
            .memory()
            .place("t:stranger")
            .unwrap()
            .unwrap()
            .unread,
        0
    );
    assert!(rig.mind.agenda.attend.is_empty());
}

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
async fn leaving_is_a_process_and_then_it_is_final() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let leave = r#"{"actions":[{"do":"leave","why":"cruel"}]}"#;
    rig.think.then(leave);
    rig.hear(dm("troll", "1", "first insult"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    // Too soon: it only registers as pulling away.
    let troll = rig.mind.ghost.memory().person("t:troll").unwrap().unwrap();
    assert!(!troll.gone);
    assert_eq!(troll.warmth, -3);

    rig.think.then(leave);
    let t1 = t0 + Duration::hours(1);
    rig.hear(dm("troll", "2", "second insult"), t1);
    rig.until(|r| r.think.calls() == 2).await;
    assert!(
        rig.mind
            .ghost
            .memory()
            .person("t:troll")
            .unwrap()
            .unwrap()
            .gone
    );

    // After that, their private messages do not reach her at all.
    let before = rig.kinds().len();
    rig.hear(dm("troll", "3", "hello??"), t1 + Duration::hours(1));
    assert_eq!(rig.kinds().len(), before);
}

#[tokio::test]
async fn echoes_and_redeliveries_are_not_heard_twice() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("haoran", "1", "hi"), t0);
    rig.hear(dm("haoran", "1", "hi"), t0);
    rig.hear(dm("lin", "9", "her own message, echoed"), t0);
    assert_eq!(rig.kinds().iter().filter(|k| **k == "heard").count(), 1);
}

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
    assert!(rig.tick.last_prompt().contains("Nobody addressed you"));
    assert!(rig.body.sent().is_empty());
    assert_eq!(rig.decisions(), vec!["nothing"]);
}

#[tokio::test]
async fn she_comes_back_to_things_on_her_own() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then(r#"{"actions":[{"do":"later","minutes":45,"why":"in the middle of something"}]}"#);
    rig.think
        .then(r#"{"thought":"free now","actions":[{"do":"say","text":"ok, back. what was it"}]}"#);
    rig.hear(dm("haoran", "1", "got a minute?"), t0);
    let put_off = rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.body.sent().is_empty());

    let came_back = rig.until(|r| r.body.sent().len() == 1).await;
    assert!(came_back - put_off >= Duration::minutes(45));
    assert!(
        rig.think
            .last_prompt()
            .contains("Earlier you told yourself")
    );
    let kinds = rig.kinds();
    assert!(kinds.contains(&"intended") && kinds.contains(&"settled"));
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty()
    );
}

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
async fn scrolling_mostly_ends_in_nothing_and_sometimes_in_a_post() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [10, 10],
            posts_per_day: 1,
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
    let mut mention = dm("mei", "tweet-9", "@lin is the teahouse really closing?");
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
