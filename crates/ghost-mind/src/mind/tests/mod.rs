//! Behaviour of the mind under a simulated clock.
//!
//! The actor's handlers all take `now`, so these tests drive them directly:
//! deliver a signal at one instant, jump to the next moment on the agenda,
//! and look at what she did. Brains are scripted and the body only records.
//!
//! The rig, the brains and the body, and the helpers more than one area
//! uses, are here; the tests are in a file for each area of her life they
//! cover, next to the helpers only that area needs. Each file opens with
//! `use super::*`, so that all of this is in reach as if it were one file.

mod attention;
mod day;
mod directives;
mod groups;
mod images;
mod memory;
mod prompts;
mod public;
mod reaching;
mod report;
mod sittings;
mod tools;
mod work;
mod work_in_hand;

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

use super::{Mind, Parts, Question, Signal, Workshop, living};
use crate::{Act, Acted, Body, Can, Incoming, MindConfig, Numb, Prompts};

const SEED: &str = r#"
name = "Weiwei"
pronouns = "she/her"
born = "2026-11-01"
temperament = "quiet"
voice = "short"
origin = "made"
examples = "them: you up?\nWeiwei: barely"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"

[[past]]
keys = ["teahouse", "茶馆"]
text = "Her grandmother ran a teahouse on Wenshu street until it closed."
"#;

/// Another character altogether, living by the same clock: everything that
/// makes her herself differs from [`SEED`], and nothing else does.
const OTHER: &str = r#"
name = "大肥鱼"
pronouns = "he/him"
born = "2026-11-01"
temperament = "loud, warm, easily delighted"
voice = "long run-on sentences, an exclamation mark at the end of most"
origin = "A fishmonger's son from a river town."
examples = "them: 在吗\n大肥鱼: 在在在！"
values = ["generosity"]
cares_about = ["the river"]
cannot_stand = ["stinginess"]

[home]
city = "Wuhan"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"

[[past]]
keys = ["market", "菜场"]
text = "He grew up under his father's stall at the fish market."
"#;

/// What in [`OTHER`] is his alone, as it would appear in what he is told.
const OTHERS_OWN: &[&str] = &[
    "大肥鱼",
    "he/him",
    "Wuhan",
    "loud, warm",
    "long run-on",
    "fishmonger",
    "在在在",
    "generosity",
    "the river",
    "stinginess",
];

/// A brain that says what it is told to, in order. Given steps, it can
/// also work.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<String>>,
    seen: Mutex<Vec<Completion>>,
    steps: Mutex<VecDeque<ghost_brain::Step>>,
    turns: Mutex<Vec<ghost_brain::Turn>>,
    hands: std::sync::atomic::AtomicBool,
    eyes: std::sync::atomic::AtomicBool,
    /// Steps of work held in the middle until their gate opens, by the
    /// number of the step: the sitting is running all that while.
    held: Mutex<Vec<(usize, Arc<tokio::sync::Notify>)>>,
}

impl Script {
    fn then(&self, reply: &str) {
        self.replies.lock().unwrap().push_back(reply.to_owned());
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// Give it eyes: it is shown the images in what it is sent.
    fn see(&self) {
        self.eyes.store(true, std::sync::atomic::Ordering::Relaxed);
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

    /// Hold the step of work `nth` from now (0 for the next one asked for)
    /// in the middle, after it has been asked for and before it answers,
    /// until the gate returned is opened.
    fn hold(&self, nth: usize) -> Arc<tokio::sync::Notify> {
        let gate = Arc::new(tokio::sync::Notify::new());
        let at = self.turns.lock().unwrap().len() + nth;
        self.held.lock().unwrap().push((at, gate.clone()));
        gate
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

    fn sees(&self) -> bool {
        self.eyes.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn step(&self, turn: &ghost_brain::Turn) -> ghost_brain::Result<ghost_brain::Step> {
        let n = {
            let mut turns = self.turns.lock().unwrap();
            turns.push(turn.clone());
            turns.len() - 1
        };
        let step = self
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
            });
        let gate = self
            .held
            .lock()
            .unwrap()
            .iter()
            .find(|(at, _)| *at == n)
            .map(|(_, gate)| gate.clone());
        if let Some(gate) = gate {
            gate.notified().await;
        }
        Ok(step)
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
        images: Vec::new(),
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
        Self::build_as(SEED, config, now)
    }

    /// The same, for the character `seed` describes.
    fn build_as(seed: &str, config: MindConfig, now: DateTime<Utc>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ghost.toml"), seed).unwrap();
        let ghost = Ghost::birth(dir.path()).unwrap();
        Self::assemble(ghost, dir, config, now)
    }

    /// Stop her body and start it again from her directory, with fresh
    /// brains and body: all that carries over is what is on disk.
    fn restart(self, now: DateTime<Utc>) -> Self {
        let Self {
            mind, _dir: dir, ..
        } = self;
        let config = mind.cfg.clone();
        drop(mind);
        let ghost = Ghost::open(dir.path()).unwrap();
        Self::assemble(ghost, dir, config, now)
    }

    fn assemble(
        ghost: Ghost,
        dir: tempfile::TempDir,
        config: MindConfig,
        now: DateTime<Utc>,
    ) -> Self {
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
                    id: "weiwei".into(),
                    name: "Weiwei".into(),
                    handle: Some("weiwei".into()),
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
        self.hear_on("t", message, now);
    }

    fn hear_on(&mut self, channel: &str, message: Incoming, now: DateTime<Utc>) {
        self.mind
            .on_signal(
                Signal::Heard {
                    channel: channel.into(),
                    message,
                },
                now,
            )
            .unwrap();
    }

    /// Another platform comes online, with what it can do.
    fn online(&mut self, channel: &str, now: DateTime<Utc>) {
        self.mind
            .on_signal(
                Signal::Online {
                    channel: channel.into(),
                    me: Some(Actor {
                        id: "weiwei".into(),
                        name: "Weiwei".into(),
                        handle: Some("weiwei".into()),
                        automated: true,
                    }),
                    can: vec![Can::Send, Can::Typing],
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

    /// Like `step`, without waiting on a job that is still running (a
    /// sitting held in the middle of a step): only on what finishes within
    /// a moment. Returns the moment it lived.
    async fn step_beside(&mut self) -> DateTime<Utc> {
        let now = self.mind.agenda.next_due().expect("something is scheduled");
        self.mind.on_time(now).unwrap();
        while let Ok(Some(done)) = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            self.mind.jobs.join_next(),
        )
        .await
        {
            self.mind.on_outcome(done.unwrap(), now).unwrap();
        }
        now
    }

    /// Keep stepping beside a running sitting until `done`.
    async fn beside_until(&mut self, done: impl Fn(&Self) -> bool) -> DateTime<Utc> {
        for _ in 0..200 {
            let now = self.step_beside().await;
            if done(self) {
                return now;
            }
        }
        panic!("never happened");
    }

    /// Let jobs in flight run, without the mind doing anything, until
    /// `done`: a sitting reaching a step that is held, say.
    async fn meanwhile(&self, done: impl Fn(&Self) -> bool) {
        for _ in 0..400 {
            if done(self) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("never happened");
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
        images: Vec::new(),
    }
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
                    images: Vec::new(),
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

/// The smallest PNG there is: one transparent pixel.
const PIXEL: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";

/// A server on this machine that answers every request with `body`, as
/// `kind`. Returns its address and how many requests it has had.
async fn image_host(
    kind: &'static str,
    body: &'static [u8],
) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body).await;
        }
    });
    (format!("http://{addr}/sticker"), hits)
}

fn kept(rig: &Rig) -> Vec<(i64, String, String, Option<String>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["kept"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Kept { file, note, from } => Some((e.seq, file, note, from)),
            _ => None,
        })
        .collect()
}

/// What the last call to `name` gave back, as the brain was shown it.
fn last_result(rig: &Rig, name: &str) -> (String, bool) {
    let turns = rig.think.turns.lock().unwrap();
    turns
        .iter()
        .flat_map(|t| t.items.iter())
        .filter_map(|item| match item {
            ghost_brain::Item::Result {
                name: called,
                output,
                failed,
                ..
            } if called == name => Some((output.clone(), *failed)),
            _ => None,
        })
        .next_back()
        .expect("it was called")
}

/// The channel `t` comes online again, now able to take a file.
fn takes_files(rig: &mut Rig, now: DateTime<Utc>) {
    rig.mind
        .on_signal(
            Signal::Online {
                channel: "t".into(),
                me: Some(Actor {
                    id: "weiwei".into(),
                    name: "Weiwei".into(),
                    handle: Some("weiwei".into()),
                    automated: true,
                }),
                can: vec![Can::Send, Can::Typing, Can::Files],
            },
            now,
        )
        .unwrap();
}

/// Every file that reached the body: its name, its bytes and its line.
fn files_sent(rig: &Rig) -> Vec<(String, Vec<u8>, Option<String>)> {
    rig.body
        .acts
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, act)| match act {
            Act::File {
                name, data, text, ..
            } => Some((name.clone(), data.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

/// Every `Said` in her journal: its text and the file sent with it.
fn said_with_files(rig: &Rig) -> Vec<(String, Option<String>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["said"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Said { text, file, .. } => Some((text, file)),
            _ => None,
        })
        .collect()
}

/// A mind that can work, with a friend, Wen, who has asked it for something
/// in their private chat. Returns the number she knows the work by.
fn at_work_for_wen(rig: &mut Rig, brief: &str, now: DateTime<Utc>) -> i64 {
    rig.think
        .hands
        .store(true, std::sync::atomic::Ordering::Relaxed);
    befriend(rig, "wen", now);
    let memory = rig.mind.ghost.memory();
    let wen = memory.person("t:wen").unwrap().unwrap();
    let place = memory.place("t:wen").unwrap().unwrap();
    assert_eq!(
        rig.mind
            .undertake(brief, Some(&wen), Some(&place), &[], now)
            .unwrap(),
        None
    );
    rig.mind.ghost.memory().undertakings(true, 1).unwrap()[0].seq
}

fn count(rig: &Rig, kind: &str) -> usize {
    rig.kinds().iter().filter(|k| **k == kind).count()
}

/// Her attention, with `boss` on `t` as an operator of hers.
fn operated() -> MindConfig {
    MindConfig {
        live: false,
        browse_minutes: [0, 0],
        operators: crate::Operators {
            accounts: [("t".to_owned(), vec!["boss".to_owned()])].into(),
            ..crate::Operators::default()
        },
        ..MindConfig::default()
    }
}

/// The pieces of work with a sitting running, by id.
fn running(rig: &Rig) -> Vec<String> {
    rig.mind.working.keys().cloned().collect()
}
