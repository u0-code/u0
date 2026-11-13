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

    /// Queue one turn: something thought, and any number of calls.
    fn then_turn(&self, text: &str, calls: &[(&str, serde_json::Value)]) {
        self.hands.store(true, std::sync::atomic::Ordering::Relaxed);
        self.steps.lock().unwrap().push_back(ghost_brain::Step {
            text: text.to_owned(),
            calls: calls
                .iter()
                .enumerate()
                .map(|(i, (name, args))| ghost_brain::Call {
                    id: format!("c{i}-{name}"),
                    name: (*name).to_owned(),
                    args: args.clone(),
                })
                .collect(),
            stop: if calls.is_empty() {
                ghost_brain::Stop::End
            } else {
                ghost_brain::Stop::Calls
            },
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
    /// A mind whose brain answers in JSON, as most of these tests script it.
    fn new(config: MindConfig, now: DateTime<Utc>) -> Self {
        Self::build(
            MindConfig {
                talk_tools: false,
                ..config
            },
            now,
        )
    }

    /// A mind at human pace whose brain answers by calling tools.
    fn with_tools(now: DateTime<Utc>) -> Self {
        let rig = Self::build(
            MindConfig {
                live: false,
                browse_minutes: [0, 0],
                ..MindConfig::default()
            },
            now,
        );
        rig.think
            .hands
            .store(true, std::sync::atomic::Ordering::Relaxed);
        rig
    }

    fn build(config: MindConfig, now: DateTime<Utc>) -> Self {
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

    /// Why she did each thing she decided, as her journal has it.
    fn reasons(&self) -> Vec<String> {
        self.mind
            .ghost
            .journal()
            .read(1, 10_000)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.event {
                Event::Decided { reason, .. } => Some(reason),
                _ => None,
            })
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

#[tokio::test]
async fn with_her_phone_in_her_hand_she_gets_to_everyone_who_wrote() {
    // Mid-morning she is not looking at her phone much, so a stranger's
    // message would normally wait a good while.
    let t0 = at(10, 0);
    let mut rig = Rig::quiet(t0);
    rig.hear(dm("stranger-a", "1", "hello, is this lin?"), t0);
    rig.hear(
        dm("stranger-b", "2", "hi, quick question"),
        t0 + Duration::seconds(1),
    );
    let (a, b) = (
        rig.mind.agenda.attend["t:stranger-a"].due,
        rig.mind.agenda.attend["t:stranger-b"].due,
    );
    let (first, second, later) = if a <= b {
        ("t:stranger-a", "t:stranger-b", b)
    } else {
        ("t:stranger-b", "t:stranger-a", a)
    };
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"yes"}]}"#);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"go on"}]}"#);

    // She looks at the first when its time comes. The other one is now due
    // within the same sitting, whatever its own clock said.
    let looked = rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.mind.thinking.contains(first) || rig.kinds().contains(&"read"));
    let pulled = rig.mind.agenda.attend[second].due;
    assert!(pulled <= later, "never later than it would have been");
    assert!(
        pulled - looked <= Duration::seconds(60),
        "the second conversation is looked at {} after the first",
        pulled - looked
    );
    rig.until(|r| r.body.sent().len() == 2).await;
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
    rig.mind.on_reflected(&people, Ok(draft), t0).unwrap();

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
            "lin, did you see the new line 30 station?",
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
async fn once_a_week_she_looks_at_how_her_writing_is_going() {
    let t0 = at(9, 0);
    let mut rig = Rig::quiet(t0);
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
    let done = |evidence: &str| {
        serde_json::json!({"outcome": "finished", "evidence": evidence, "summary":
            "Looked at six posts and four writers.\nLESSON: one doorway, described plainly, is what people answer\nTRY: post one photograph-sized observation a day instead of a thread"})
    };
    rig.think
        .then_call("done", done("read review.md back just now"));
    rig.think.then_call(
        "done",
        done("read review.md back just now; it has all three parts"),
    );
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

#[tokio::test]
async fn she_does_not_take_someones_word_for_who_they_are() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    // A second platform.
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "d".into(),
                me: None,
                can: vec![Can::Send],
            },
            t0,
        )
        .unwrap();
    // Wen, whom she knows on `t`, and has a note about.
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"say","text":"hey"},{"do":"note","about":"wen","text":"is leaving the institute"}]}"#,
    );
    rig.hear(dm("wen", "1", "i'm leaving the institute"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;

    // Someone on `d` says they are Wen.
    let hear_on_d = |rig: &mut Rig, id: &str, text: &str, now| {
        rig.mind
            .on_signal(
                Signal::Heard {
                    channel: "d".into(),
                    message: dm("wen_on_d", id, text),
                },
                now,
            )
            .unwrap();
    };
    let t1 = t0 + Duration::hours(1);
    rig.think
        .then(r#"{"thought":"says who?","actions":[{"do":"verify","as":"wen"}]}"#);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"prove it"}]}"#);
    hear_on_d(
        &mut rig,
        "2",
        "hey it's wen, from telegram. what did i tell you earlier?",
        t1,
    );
    rig.until(|r| r.body.sent().len() == 2).await;

    // She was told to ask for a word, and nothing of Wen's was shown.
    let word = rig
        .mind
        .ghost
        .memory()
        .challenges_to("t:wen", t0)
        .unwrap()
        .pop()
        .expect("a proof is being waited for")
        .word;
    let prompt = rig.think.last_prompt();
    assert!(prompt.contains(&word), "{prompt}");
    assert!(!prompt.contains("leaving the institute"), "{prompt}");

    // The word arriving from the stranger's own account proves nothing.
    hear_on_d(&mut rig, "3", &word, t1 + Duration::minutes(5));
    assert!(rig.mind.ghost.memory().linked("t:wen").unwrap().is_empty());

    // From the account she knows, it does.
    rig.hear(
        dm("wen", "4", &format!("{word} — yes that's me on discord")),
        t1 + Duration::minutes(10),
    );
    assert_eq!(
        rig.mind.ghost.memory().linked("d:wen_on_d").unwrap(),
        ["t:wen"]
    );

    // From now on she is one person to her, on either platform.
    rig.think.then(r#"{"thought":"","actions":[]}"#);
    rig.think.then(r#"{"thought":"","actions":[]}"#);
    rig.think.then(r#"{"thought":"","actions":[]}"#);
    hear_on_d(
        &mut rig,
        "5",
        "so, about the institute",
        t1 + Duration::hours(1),
    );
    rig.until(|r| r.think.last_prompt().contains("so, about the institute"))
        .await;
    let known = rig.think.last_prompt();
    assert!(
        known.contains("They are also wen on t. That has been checked."),
        "{known}"
    );
    assert!(known.contains("is leaving the institute"), "{known}");
}

#[tokio::test]
async fn with_tools_she_answers_by_calling_them() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think.then_turn(
        "she sounds tired",
        &[
            ("say", serde_json::json!({"text": "long day?"})),
            ("say", serde_json::json!({"text": "go lie down"})),
            (
                "note",
                serde_json::json!({"about": "wen", "text": "works late on Thursdays"}),
            ),
        ],
    );
    rig.hear(dm("wen", "1", "just got home"), t0);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent(), ["long day?", "go lie down"]);
    assert!(rig.kinds().contains(&"noted"));

    // Everything she meant to do came in one turn, so one turn was all.
    let turns = rig.think.turns.lock().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(rig.think.calls(), 0, "nothing was asked for as JSON");
    // Who she is comes first and holds nothing that changes by the hour;
    // the hour comes after, with what is in front of her.
    assert!(turns[0].system.contains("You are Lin"));
    assert!(!turns[0].system.contains("# Right now"));
    let ghost_brain::Item::User { text } = &turns[0].items[0] else {
        panic!()
    };
    assert!(text.contains("# Right now") && text.contains("just got home"));
    assert!(text.find("# The conversation") < text.find("# Right now"));
    assert!(!text.contains(r#"{"do":"#), "no JSON format is described");
    let names: Vec<&str> = turns[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"say") && names.contains(&"recall"));
    // No way to search the web in this body, so it is not offered.
    assert!(!names.contains(&"look_up"));
    // Her thought is the reason in her journal.
    assert!(rig.reasons().iter().any(|r| r == "she sounds tired"));
}

#[tokio::test]
async fn a_call_she_gets_wrong_comes_back_to_her() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"message": "hi"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "hi"}))]);
    rig.hear(dm("wen", "1", "hey"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["hi"]);

    let turns = rig.think.turns.lock().unwrap();
    assert_eq!(turns.len(), 2);
    assert!(matches!(turns[1].items.last(),
        Some(ghost_brain::Item::Result { failed: true, output, .. })
            if output.contains("could not be used") && output.contains("text")));
}

#[tokio::test]
async fn she_can_search_her_memory_in_the_middle_of_answering() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    rig.think.then_turn(
        "",
        &[(
            "note",
            serde_json::json!({"about": "wen", "text": "her cat is called Dumpling"}),
        )],
    );
    rig.hear(dm("wen", "1", "my cat is called Dumpling"), t0);
    rig.until(|r| r.kinds().contains(&"noted")).await;

    let later = t0 + Duration::hours(3);
    rig.think
        .then_turn("", &[("recall", serde_json::json!({"query": "cat"}))]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "Dumpling!"}))]);
    rig.hear(dm("wen", "2", "do you remember my cat's name?"), later);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["Dumpling!"]);

    let turns = rig.think.turns.lock().unwrap();
    let asked_again = turns.last().unwrap();
    assert!(matches!(asked_again.items.last(),
        Some(ghost_brain::Item::Result { name, output, failed: false, .. })
            if name == "recall" && output.contains("Dumpling")));
    // The turn after is the same look carried on: what she saw first is
    // still at the head of it, unchanged.
    let before = &turns[turns.len() - 2];
    assert_eq!(asked_again.items[0], before.items[0]);
}

#[tokio::test]
async fn a_thought_is_not_a_message() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    // It answers in plain text, the way it would without tools.
    rig.think.then_turn("nine, I think", &[]);
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "nine, I think"}))]);
    rig.hear(dm("wen", "1", "when does it close?"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;
    assert_eq!(rig.body.sent(), ["nine, I think"]);
    {
        let turns = rig.think.turns.lock().unwrap();
        assert!(matches!(turns[1].items.last(),
            Some(ghost_brain::Item::User { text }) if text.contains("nothing was sent")));
    }

    // Told once. Saying nothing a second time is taken as meant.
    let later = t0 + Duration::hours(1);
    rig.think.then_turn("not worth answering", &[]);
    rig.think.then_turn("still not", &[]);
    rig.hear(dm("wen", "2", "??"), later);
    rig.until(|r| r.reasons().iter().any(|x| x == "not worth answering"))
        .await;
    assert_eq!(rig.body.sent().len(), 1);
}

#[tokio::test]
async fn what_she_looks_up_comes_back_before_she_answers() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    // Somewhere to search that answers at once, with a refusal: nothing is
    // listening there. What matters here is the way round, not the result.
    rig.mind.workshop.web.search = Some(ghost_work::tools::Source::Searxng {
        url: "http://127.0.0.1:1".into(),
    });
    rig.think.then_turn(
        "",
        &[(
            "look_up",
            serde_json::json!({"query": "heming tea house hours"}),
        )],
    );
    rig.think.then_turn(
        "",
        &[("say", serde_json::json!({"text": "can't tell, sorry"}))],
    );
    rig.hear(dm("wen", "1", "when does the tea house close?"), t0);
    rig.until(|r| r.body.sent().len() == 1).await;

    let turns = rig.think.turns.lock().unwrap();
    assert!(turns[0].tools.iter().any(|t| t.name == "look_up"));
    assert_eq!(turns.len(), 2);
    assert!(matches!(turns[1].items.last(),
        Some(ghost_brain::Item::Result { name, .. }) if name == "look_up"));
}

#[tokio::test]
async fn a_long_conversation_is_kept_as_a_note_and_its_end_word_for_word() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [0, 0],
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

/// Five days of easy talk: enough for her to count someone a friend.
fn befriend(rig: &mut Rig, who: &str, before: DateTime<Utc>) {
    for day in 1..=5 {
        let when = before - Duration::days(day);
        let m = dm(who, &format!("old-{day}"), "morning");
        rig.mind
            .ghost
            .record_at(
                when,
                Event::Heard {
                    channel: "t".into(),
                    place: m.place,
                    from: m.from,
                    id: m.id,
                    text: m.text,
                    reply_to: None,
                    addressed: true,
                },
            )
            .unwrap();
        rig.mind
            .ghost
            .record_at(
                when,
                Event::Felt {
                    toward: format!("t:{who}"),
                    delta: 1,
                    why: "easy to talk to".into(),
                },
            )
            .unwrap();
    }
    let upto = rig
        .mind
        .ghost
        .memory()
        .place(&format!("t:{who}"))
        .unwrap()
        .unwrap()
        .last_in_seq;
    rig.mind
        .ghost
        .record_at(
            before - Duration::hours(1),
            Event::Read {
                channel: "t".into(),
                place: who.into(),
                upto,
            },
        )
        .unwrap();
}

#[tokio::test]
async fn she_stops_to_ask_and_goes_on_when_they_answer() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[
            {"do":"say","text":"on it"},
            {"do":"undertake","brief":"Write up when the Heming tea house is open."}]}"#,
    );
    rig.think.then_call(
        "ask",
        serde_json::json!({"question": "weekdays or weekends?"}),
    );
    rig.hear(
        dm(
            "wen",
            "1",
            "can you write up when the tea house is open? it is for my mum",
        ),
        t0,
    );
    let asked = rig.until(|r| r.kinds().contains(&"asked")).await;

    // What she works from is her own account of it, and under that what
    // was actually written to her.
    let u = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert!(
        u.brief
            .starts_with("Write up when the Heming tea house is open.")
    );
    assert!(u.brief.contains("word for word") && u.brief.contains("> can you write up"));
    assert_eq!(u.waiting.as_deref(), Some("weekdays or weekends?"));

    // She means to put the question to them, and does.
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"weekdays or weekends?"}]}"#);
    rig.until(|r| r.body.sent().len() == 2).await;
    assert_eq!(rig.body.sent()[1], "weekdays or weekends?");
    // While it waits, she is told so whenever she looks at anything.
    assert!(
        rig.think
            .last_prompt()
            .contains("It waits for their answer")
    );
    // And the work is not touched.
    let worked = rig.think.turns.lock().unwrap().len();
    rig.pump(asked + Duration::hours(2)).await;
    assert_eq!(rig.think.turns.lock().unwrap().len(), worked);

    // They answer; she passes it on; the work picks up with it.
    let answered = asked + Duration::hours(3);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"undertake","brief":"Weekdays."}]}"#);
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "blocked", "summary": "the page is down"}),
    );
    rig.hear(dm("wen", "2", "weekdays please"), answered);
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    let kinds = rig.kinds();
    let added = kinds.iter().position(|k| *k == "added").unwrap();
    assert!(added < kinds.iter().position(|k| *k == "concluded").unwrap());
    let turns = rig.think.turns.lock().unwrap();
    assert!(turns.last().unwrap().items.iter().any(|item| matches!(item,
        ghost_brain::Item::User { text }
            if text.contains("added to the task") && text.contains("Weekdays.") && text.contains("> weekdays please"))));
}

#[tokio::test]
async fn a_question_nobody_answers_does_not_hold_the_work_for_ever() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.think.then(
        r#"{"thought":"","actions":[{"do":"undertake","brief":"Write up the tea house hours."}]}"#,
    );
    rig.think
        .then_call("ask", serde_json::json!({"question": "which branch?"}));
    rig.hear(dm("wen", "1", "write up the tea house hours?"), t0);
    let asked = rig.until(|r| r.kinds().contains(&"asked")).await;

    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "blocked", "summary": "assumed the park branch; its page is down"}),
    );
    // She has it down to come back to when a day has gone by with no
    // answer. (Entries are stamped by the real clock, so that day is
    // counted from the entry and not from the time this test pretends.)
    rig.pump(asked + Duration::hours(1)).await;
    rig.pump(asked + Duration::hours(2)).await;
    let give_up = rig.mind.agenda.work.expect("she means to come back to it");
    let u = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert_eq!(give_up, u.asked_at.unwrap() + Duration::hours(24));
    rig.pump(give_up).await;
    rig.pump(give_up + Duration::minutes(30)).await;
    assert!(rig.kinds().contains(&"concluded"), "{:?}", rig.kinds());
    let turns = rig.think.turns.lock().unwrap();
    assert!(turns.last().unwrap().items.iter().any(|item| matches!(item,
        ghost_brain::Item::User { text } if text.contains("No answer came"))));
}

#[tokio::test]
async fn what_she_made_is_handed_over_as_a_file() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    let desk = tempfile::tempdir().unwrap();
    rig.mind.workshop.machine = Some(Arc::new(ghost_work::Local::new(desk.path())));
    befriend(&mut rig, "wen", t0);
    // This channel can take a file.
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: Some(Actor {
                    id: "lin".into(),
                    name: "Lin".into(),
                    handle: Some("lin".into()),
                    automated: true,
                }),
                can: vec![Can::Send, Can::Typing, Can::Files],
            },
            t0,
        )
        .unwrap();

    rig.think.then_turn(
        "",
        &[
            ("say", serde_json::json!({"text": "give me a bit"})),
            (
                "undertake",
                serde_json::json!({"brief": "Write the tea house hours into hours.md."}),
            ),
        ],
    );
    let made = desk.path().join("hours.md");
    rig.think.then_call(
        "write",
        serde_json::json!({"path": "hours.md", "content": "09:00-21:00\n"}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({"outcome": "finished", "summary": "It is written up."}),
    );
    rig.think.then_call(
        "done",
        serde_json::json!({
            "outcome": "finished", "summary": "It is written up.",
            "evidence": "cat hours.md printed 09:00-21:00 just now",
            "files": [made.to_str().unwrap()],
        }),
    );
    rig.hear(
        dm("wen", "1", "could you write up the tea house hours?"),
        t0,
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;

    // Telling them, she is offered a way to hand it over, and uses it.
    rig.think.then_turn(
        "",
        &[(
            "send_file",
            serde_json::json!({"file": "hours.md", "text": "here you go"}),
        )],
    );
    rig.until(|r| r.body.count(|a| matches!(a, Act::File { .. })) == 1)
        .await;
    {
        let turns = rig.think.turns.lock().unwrap();
        let telling = turns.last().unwrap();
        assert!(telling.tools.iter().any(|t| t.name == "send_file"));
        assert!(matches!(&telling.items[0],
            ghost_brain::Item::User { text } if text.contains("hours.md") && text.contains("send_file")));
    }
    let acts = rig.body.acts.lock().unwrap();
    let sent = acts
        .iter()
        .find_map(|(_, act)| match act {
            Act::File {
                name, data, text, ..
            } => Some((name.clone(), data.clone(), text.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(sent.0, "hours.md");
    assert_eq!(sent.1, b"09:00-21:00\n");
    assert_eq!(sent.2.as_deref(), Some("here you go"));
    drop(acts);
    // And it is in the conversation as something she sent.
    let thread = rig.mind.ghost.memory().thread("t:wen", 3).unwrap();
    assert!(
        thread
            .iter()
            .any(|l| l.text == "[file: hours.md] here you go")
    );
}

#[tokio::test]
async fn she_takes_something_on_toward_what_she_is_after_and_keeps_what_it_taught_her() {
    let t0 = at(23, 30);
    let mut rig = Rig::quiet(t0);
    // A brain that can work.
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let night = |json: serde_json::Value| super::living::reflect_draft(&json).unwrap();

    // With nothing she is after, she is not asked what to do about it,
    // and anything said anyway comes to nothing.
    assert!(rig.mind.own_work_line(t0).unwrap().contains("empty"));
    rig.mind
        .on_reflected(
            &[],
            Ok(night(serde_json::json!({
                "reflection": "A quiet day.",
                "undertake": "Find out which tea houses are closing.",
            }))),
            t0,
        )
        .unwrap();
    assert!(!rig.kinds().contains(&"undertook"));

    // One night she finds she is after something, and means to start.
    rig.mind
        .on_reflected(
            &[],
            Ok(night(serde_json::json!({
                "reflection": "I keep going back to the old tea houses.",
                "pursuits": [{"what": "draw the tea houses that are closing, before they do"}],
                "undertake": "Find out which tea houses in the city are closing this year, with dates.",
            }))),
            t0,
        )
        .unwrap();
    let kinds = rig.kinds();
    assert!(
        kinds.contains(&"pursued") && kinds.contains(&"undertook"),
        "{kinds:?}"
    );
    let own = rig.mind.ghost.memory().undertakings(true, 5).unwrap()[0].clone();
    assert_eq!((own.asked_by, own.place), (None, None));
    // It is in front of her the next night, numbered, to change or let go.
    assert!(
        rig.mind
            .pursuit_block()
            .unwrap()
            .contains("draw the tea houses")
    );
    // One thing of her own at a time.
    rig.mind
        .on_reflected(
            &[],
            Ok(night(serde_json::json!({
                "reflection": "More of the same.",
                "undertake": "Also list the ones that have already gone.",
            }))),
            t0,
        )
        .unwrap();
    assert_eq!(rig.kinds().iter().filter(|k| **k == "undertook").count(), 1);

    // She does it, in her own time, and what it taught her about working
    // is kept and is there at the top of the next piece of work.
    rig.think.then_call(
        "done",
        serde_json::json!({
            "outcome": "gave_up", "summary": "The listings are all a year old.",
            "lesson": "Look at when a page was last changed before trusting a date on it.",
        }),
    );
    rig.until(|r| r.kinds().contains(&"concluded")).await;
    assert!(rig.kinds().contains(&"practised"));
    assert!(
        rig.mind
            .practice_block()
            .unwrap()
            .contains("when a page was last changed")
    );
    // Nobody asked, so there is nobody to tell.
    assert!(
        rig.mind
            .ghost
            .memory()
            .open_intentions()
            .unwrap()
            .is_empty()
    );

    // She can change her mind about what she is after, or let it go.
    let seq = rig.mind.ghost.memory().pursuits(3).unwrap()[0].seq;
    rig.mind
        .on_pursuits(vec![(String::new(), Some(seq))])
        .unwrap();
    assert!(rig.mind.ghost.memory().pursuits(3).unwrap().is_empty());
}
