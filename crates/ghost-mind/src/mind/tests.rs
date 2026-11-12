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

use super::{Mind, Parts, Signal, Workshop};
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

/// A brain that says what it is told to, in order. Given steps, it can
/// also work.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<String>>,
    seen: Mutex<Vec<Completion>>,
    steps: Mutex<VecDeque<ghost_brain::Step>>,
    turns: Mutex<Vec<ghost_brain::Turn>>,
    hands: std::sync::atomic::AtomicBool,
}

impl Script {
    fn then(&self, reply: &str) {
        self.replies.lock().unwrap().push_back(reply.to_owned());
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// Queue one step of work: a call to a tool.
    fn then_call(&self, name: &str, args: serde_json::Value) {
        self.hands.store(true, std::sync::atomic::Ordering::Relaxed);
        self.steps.lock().unwrap().push_back(ghost_brain::Step {
            text: String::new(),
            calls: vec![ghost_brain::Call {
                id: format!("c-{name}"),
                name: name.into(),
                args,
            }],
            stop: ghost_brain::Stop::Calls,
            usage: ghost_brain::Usage::default(),
            model: "script".into(),
            carry: None,
        });
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

    fn works(&self) -> bool {
        self.hands.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn step(&self, turn: &ghost_brain::Turn) -> ghost_brain::Result<ghost_brain::Step> {
        self.turns.lock().unwrap().push(turn.clone());
        Ok(self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ghost_brain::Step {
                text: String::new(),
                calls: vec![],
                stop: ghost_brain::Stop::End,
                usage: ghost_brain::Usage::default(),
                model: "script".into(),
                carry: None,
            }))
    }
}

/// A body that writes down what it was asked to do.
#[derive(Default)]
struct Recorder {
    acts: Mutex<Vec<(String, Act)>>,
    timeline: Mutex<Vec<Incoming>>,
    /// What the channel reports when asked how her posts are doing.
    standing: Mutex<Option<crate::Measure>>,
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
        let measure = matches!(act, Act::Measure)
            .then(|| self.standing.lock().unwrap().clone())
            .flatten();
        acts.push((channel.to_owned(), act));
        Ok(Acted {
            id: Some(format!("out-{}", acts.len())),
            items,
            measure,
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
            workshop: Workshop::default(),
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
async fn she_does_not_resolve_to_do_the_same_thing_twice() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let intend = r#"{"actions":[{"do":"say","text":"good luck"},{"do":"intend","hours":26,"what":"ask how the interview went"}]}"#;
    rig.think.then(intend);
    rig.think.then(intend);
    rig.hear(dm("haoran", "1", "interview tomorrow at 3"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    rig.hear(
        dm("haoran", "2", "really hoping it goes well"),
        t0 + Duration::minutes(5),
    );
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.mind.ghost.memory().open_intentions().unwrap().len(), 1);
    // The second time she looked, she could see what she already meant to do.
    assert!(rig.think.last_prompt().contains("You already mean to:"));
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

fn group(place: &str, from: &str, id: &str, text: &str, addressed: bool) -> Incoming {
    Incoming {
        place: Place {
            id: place.into(),
            kind: PlaceKind::Group,
            name: Some("old streets".into()),
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
        addressed,
    }
}

#[tokio::test]
async fn she_takes_something_on_does_it_and_comes_back_with_it() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"easy enough to check","actions":[
            {"do":"say","text":"let me look"},
            {"do":"undertake","brief":"Find out what time the Heming tea house in People's Park closes on weekdays."}]}"#,
    );
    // The work itself: she is asked to check once, then it is recorded.
    rig.think.then_call(
        "note",
        serde_json::json!({"text": "the park's own page says 21:00"}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It closes at nine."}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It closes at nine on weekdays.", "evidence": "the park's own page lists 21:00 for weekdays"}),
    );
    rig.hear(
        dm("wen", "1", "do you know when the heming tea house closes?"),
        t0,
    );

    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let kinds = rig.kinds();
    let took = kinds.iter().position(|k| *k == "undertook").unwrap();
    let said = kinds.iter().position(|k| *k == "said").unwrap();
    let ended = kinds.iter().position(|k| *k == "concluded").unwrap();
    assert!(took < ended && said < ended, "{kinds:?}");

    // She worked as herself, on the brief, with no shell in this body.
    {
        let turns = rig.think.turns.lock().unwrap();
        assert!(turns[0].system.contains("Lin"));
        assert!(
            matches!(&turns[0].items[0], ghost_brain::Item::User { text } if text.contains("Heming tea house"))
        );
        assert!(!turns[0].tools.iter().any(|t| t.name == "shell"));
    }

    // Having finished, she means to tell whoever asked, and does.
    let u = &rig.mind.ghost.memory().undertakings(false, 5).unwrap()[0];
    assert_eq!(u.outcome.as_deref(), Some("finished"));
    assert_eq!(u.asked_by.as_deref(), Some("t:wen"));
    rig.think
        .then(r#"{"thought":"tell her","actions":[{"do":"say","text":"nine on weekdays"}]}"#);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent(), ["let me look", "nine on weekdays"]);
    assert!(
        rig.think
            .last_prompt()
            .contains("It closes at nine on weekdays.")
    );
}

#[tokio::test]
async fn a_sitting_is_put_down_when_she_looks_at_her_phone() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think
        .then_call("note", serde_json::json!({"text": "first thing"}));
    rig.mind
        .undertake(
            "Read up on the history of Kuixinglou street.",
            None,
            None,
            t0,
        )
        .unwrap();
    // She sits down to it.
    rig.mind.on_time(t0).unwrap();
    assert!(rig.mind.working.is_some());

    // A message is due while the sitting is still running: looking at it
    // waits, and the sitting is told to stop.
    rig.hear(dm("wen", "1", "are you there"), t0);
    let due = rig.mind.agenda.attend["t:wen"].due;
    rig.mind.on_time(due).unwrap();
    assert_eq!(rig.think.calls(), 0, "she has not looked yet");
    assert!(rig.mind.agenda.attend["t:wen"].due > due);

    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"here"}]}"#);
    rig.until(|r| r.body.sent().len() == 1).await;
    let kinds = rig.kinds();
    let worked = kinds.iter().position(|k| *k == "worked").unwrap();
    let read = kinds.iter().position(|k| *k == "read").unwrap();
    assert!(
        worked < read,
        "the sitting ended before she read: {kinds:?}"
    );
    // The work is still hers, and still open.
    assert_eq!(
        rig.mind.ghost.memory().undertakings(true, 5).unwrap().len(),
        1
    );
    assert!(rig.mind.agenda.work.is_some());
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
        group("streets", "bo", "3", "lin, are you coming?", true),
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
