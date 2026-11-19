//! The actor.
//!
//! One task owns the ghost and is the only thing that writes her journal. It
//! waits on three things: a signal from outside, the outcome of a job it sent
//! off (a brain call, a send), or the next moment on its agenda. Nothing in
//! here blocks on a network call; anything slow is spawned and comes back as
//! an `Outcome`.

mod ask;
mod attend;
mod browse;
mod context;
mod decided;
mod digest;
mod give;
mod group;
mod interrupt;
mod keep;
mod living;
mod look;
mod public;
mod pursuit;
mod reach;
mod reader;
pub use reader::Reader;
mod report;
mod reread;
mod sessions;
mod sides;
mod speak;
#[cfg(test)]
mod tests;
mod thought;
mod view;
mod waiting;
mod work;
mod writer;

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration as StdDuration,
};

use chrono::{DateTime, Duration, Utc};
use fastrand::Rng;
use ghost_brain::Brain;
use ghost_core::{
    Ghost,
    journal::{Entry, Event},
    life::Life,
    narrate,
    social::{self, Actor},
    tempo::Tempo,
};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::{AbortHandle, JoinSet},
};
use tracing::{Instrument as _, info, warn};

use crate::narrate::Scene;

use self::{
    attend::{Looking, Thinking, Thought},
    living::{LivedDraft, PlanDraft, ReflectDraft},
    speak::{Bubble, Speaking},
};
use crate::{
    Act, Acted, Body, Can, Error, Incoming, MindConfig, Prompts, Result, Senses,
    actions::Deliberation,
};

/// How long the loop sleeps when nothing at all is scheduled.
const IDLE: StdDuration = StdDuration::from_secs(3600);
/// A gap in the journal longer than this is recorded as dormancy.
const DORMANT_AFTER: Duration = Duration::minutes(10);

/// Everything a mind is assembled from.
pub struct Parts {
    pub ghost: Ghost,
    /// The brain for conversation and reflection.
    pub think: Arc<dyn Brain>,
    /// The brain for routine ticks of her day.
    pub tick: Arc<dyn Brain>,
    pub body: Arc<dyn Body>,
    pub senses: Arc<dyn Senses>,
    pub prompts: Prompts,
    pub config: MindConfig,
    pub workshop: Workshop,
}

/// What she has to work with, beyond a brain.
#[derive(Clone, Default)]
pub struct Workshop {
    /// Where her shell and files are. `None` means she can look things up
    /// and think, and nothing else.
    pub machine: Option<Arc<dyn ghost_work::Machine>>,
    /// The directory her machine works in, as a full path, when it is this
    /// computer: a console is told it, to open what she makes there.
    pub local: Option<std::path::PathBuf>,
    /// How she reaches the web.
    pub web: ghost_work::tools::WebConfig,
    /// Servers of tools: those she installs from the registry, and those
    /// whoever runs her body gives her. `None` is none of either.
    pub servers: Option<Arc<ghost_work::servers::Servers>>,
    /// The skills she has and the index she finds more in. `None` is no
    /// skills at all.
    pub skills: Option<Arc<ghost_work::skills::Skills>>,
    /// Agents installed on her machine (Claude Code, Codex) that her work
    /// can hand a part to.
    pub agents: Vec<ghost_work::agents::Agent>,
    /// Where each piece of work gets a folder of its own, by the day it
    /// was taken on, when her machine is this computer. `None` is every
    /// piece in her machine's own directory.
    pub folders: Option<std::path::PathBuf>,
}

/// Something an observer can ask a running mind, of what only it knows.
/// What is read from her journal, memory and work is asked of her
/// [`Reader`] instead, off her mind.
#[derive(Debug, Clone)]
pub enum Question {
    /// What is scheduled, and when.
    Agenda,
    /// Her persona and its seal.
    Identity,
}

/// Someone who runs her body, acting on her from a console.
#[derive(Debug, Clone)]
pub struct Operator {
    /// Who her memory knows them as, `channel:id`.
    pub key: String,
    /// Their name, as she is told it.
    pub name: String,
}

/// What an operator does to her work from a console, without her. Each
/// is done the way she does it herself, and written in her journal as
/// theirs.
#[derive(Debug, Clone)]
pub enum Order {
    /// Answer what the work `work` asked, or add to what it was asked:
    /// `text` is put in its inbox, as what she passes on is.
    Answer { work: String, text: String },
    /// Stop the work `work`, as she stops work.
    Stop { work: String },
}

/// What an operator does to a session, a conversation of theirs with her
/// on the console channel, by its place key. It changes what a console
/// lists, and nothing of what was said there or of what she is doing.
#[derive(Debug, Clone)]
pub enum SessionChange {
    /// Call it `title` from now on.
    Name { place: String, title: String },
    /// Take it off the list.
    SetAside { place: String },
}

/// Why an [`Order`] was not carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// She has no open work by that id: none, or it has ended.
    NoSuchWork(String),
    /// It is hers, and could not be done.
    Failed(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchWork(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

enum Signal {
    Heard {
        channel: String,
        message: Incoming,
    },
    Typing {
        channel: String,
        place: String,
    },
    Online {
        channel: String,
        me: Option<Actor>,
        can: Vec<Can>,
        facts: crate::ChannelFacts,
    },
    Offline {
        channel: String,
    },
    Ask {
        question: Question,
        answer: oneshot::Sender<Value>,
    },
    Status {
        answer: oneshot::Sender<ghost_protocol::GhostStatus>,
    },
    Order {
        order: Order,
        by: Operator,
        answer: oneshot::Sender<std::result::Result<(), Refused>>,
    },
    Interrupt {
        place: String,
        by: Operator,
        answer: oneshot::Sender<bool>,
    },
    Rewind {
        place: String,
        to: i64,
        by: Operator,
        answer: oneshot::Sender<Option<i64>>,
    },
    Session {
        change: SessionChange,
        by: Operator,
        answer: oneshot::Sender<bool>,
    },
    Stop,
}

/// The outside world's handle on a running mind. Cheap to clone.
#[derive(Clone)]
pub struct MindHandle {
    tx: mpsc::Sender<Signal>,
    entries: broadcast::Sender<Entry>,
    reader: Arc<std::sync::Mutex<Reader>>,
}

impl MindHandle {
    /// A message reached her on `channel`.
    pub async fn heard(&self, channel: &str, message: Incoming) -> Result<()> {
        self.send(Signal::Heard {
            channel: channel.to_owned(),
            message,
        })
        .await
    }

    /// Someone is typing in a place. She waits a little longer before looking.
    pub async fn typing(&self, channel: &str, place: &str) -> Result<()> {
        self.send(Signal::Typing {
            channel: channel.to_owned(),
            place: place.to_owned(),
        })
        .await
    }

    /// A channel became reachable. `me` is who she is there, and `facts`
    /// what it is like.
    pub async fn online(
        &self,
        channel: &str,
        me: Option<Actor>,
        can: Vec<Can>,
        facts: crate::ChannelFacts,
    ) -> Result<()> {
        self.send(Signal::Online {
            channel: channel.to_owned(),
            me,
            can,
            facts,
        })
        .await
    }

    pub async fn offline(&self, channel: &str) -> Result<()> {
        self.send(Signal::Offline {
            channel: channel.to_owned(),
        })
        .await
    }

    pub async fn ask(&self, question: Question) -> Result<Value> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Ask { question, answer }).await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// Who she is and what she is in the middle of, this moment.
    pub async fn status(&self) -> Result<ghost_protocol::GhostStatus> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Status { answer }).await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// Read something of her with `read`, off her mind and off the
    /// runtime's threads: on her [`Reader`], one question at a time.
    pub async fn read<T: Send + 'static>(
        &self,
        read: impl FnOnce(&mut Reader) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let reader = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            let mut reader = reader.lock().map_err(|_| Error::Stopped)?;
            read(&mut reader)
        })
        .await
        .map_err(|_| Error::Stopped)?
    }

    /// An operator, `by`, does `order` to her work from a console.
    pub async fn order(
        &self,
        order: Order,
        by: Operator,
    ) -> Result<std::result::Result<(), Refused>> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Order { order, by, answer }).await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// An operator, `by`, stops her in `place`, a place key, from a
    /// console: what she is thinking and about to say there is called off,
    /// and the work she took on there is stopped. Answers whether anything
    /// was.
    pub async fn interrupt(&self, place: &str, by: Operator) -> Result<bool> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Interrupt {
            place: place.to_owned(),
            by,
            answer,
        })
        .await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// An operator takes back their line `to` (the seq of its `heard`
    /// entry) in `place`, to put it another way: she is stopped there as
    /// [`Self::interrupt`] stops her, and that line and everything said
    /// there after it leave the conversation in front of her. Answers how
    /// many lines did, or `None` when `to` is not a line of theirs there.
    pub async fn rewind(&self, place: &str, to: i64, by: Operator) -> Result<Option<i64>> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Rewind {
            place: place.to_owned(),
            to,
            by,
            answer,
        })
        .await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// An operator, `by`, names a session or sets it aside, from a
    /// console. Answers whether it was done: a session is set aside only
    /// if it is listed.
    pub async fn session(&self, change: SessionChange, by: Operator) -> Result<bool> {
        let (answer, rx) = oneshot::channel();
        self.send(Signal::Session { change, by, answer }).await?;
        rx.await.map_err(|_| Error::Stopped)
    }

    /// Every journal entry, as it is written.
    pub fn watch(&self) -> broadcast::Receiver<Entry> {
        self.entries.subscribe()
    }

    pub async fn stop(&self) {
        let _ = self.tx.send(Signal::Stop).await;
    }

    async fn send(&self, signal: Signal) -> Result<()> {
        self.tx.send(signal).await.map_err(|_| Error::Stopped)
    }
}

/// What a spawned job brings back.
enum Outcome {
    Thought {
        job: Box<Thinking>,
        result: Result<Thought>,
    },
    Sent {
        place: String,
        bubble: Bubble,
        result: std::result::Result<Acted, String>,
    },
    Planned {
        day: chrono::NaiveDate,
        weather: Option<String>,
        result: Result<PlanDraft>,
    },
    Lived {
        block: u32,
        result: Result<LivedDraft>,
    },
    Digested {
        place: String,
        upto: i64,
        sources: ghost_core::journal::Sources,
        result: Result<String>,
    },
    Reflected {
        /// The platform it looked back on, or none for her own day.
        channel: Option<String>,
        people: Vec<ghost_core::memory::Person>,
        sources: ghost_core::journal::Sources,
        result: Result<ReflectDraft>,
    },
    Browsed {
        channel: String,
        items: Vec<Incoming>,
        result: Result<Deliberation>,
    },
    Posted {
        channel: String,
        text: String,
        result: std::result::Result<Acted, String>,
    },
    /// A channel said how her posts are doing.
    Measured {
        channel: String,
        result: std::result::Result<Acted, String>,
    },
    /// A sitting of work ended.
    Worked { id: String, sat: Box<work::Sat> },
    /// A sitting of work did not start: see [`work::Unseated`].
    Unseated { id: String, why: work::Unseated },
    /// `text` was put in the inbox of the work `id`, from `by` if it was
    /// not she who passed it on, or could not be: `answer` is told which,
    /// if it is a console's order.
    Added {
        id: String,
        text: String,
        by: Option<String>,
        result: std::io::Result<()>,
        answer: Option<oneshot::Sender<std::result::Result<(), Refused>>>,
    },
    /// The record of the work `id`, which no sitting was at, was stopped,
    /// by the operator named `by` if not by her: as it is now, if it could
    /// be opened.
    Stopped {
        id: String,
        by: Option<String>,
        state: Option<Box<ghost_work::undertaking::State>>,
    },
    /// The record of the work `id` was ended as blocked, with `summary`.
    Ended {
        id: String,
        summary: String,
        files: Vec<String>,
    },
    /// She looked at the images in message `message`: for each, what she
    /// saw, or why she could not.
    Viewed {
        message: i64,
        seen: Vec<(u32, std::result::Result<view::Sight, String>)>,
    },
    /// A fire-and-forget act finished.
    Done,
}

struct Channel {
    me: Option<Actor>,
    can: Vec<Can>,
    facts: crate::ChannelFacts,
}

struct Attend {
    due: DateTime<Utc>,
    /// When what she has not yet looked at first brought this on. A burst
    /// pushes the look out only so far from it.
    since: DateTime<Utc>,
    /// Scheduled because something was meant for her.
    addressed: bool,
    /// How much her wait was stretched by her silence there when this was
    /// scheduled. 1 is not at all.
    backoff: f32,
}

/// Everything that will happen without anyone asking.
#[derive(Default)]
struct Agenda {
    /// Places she will look at, and when.
    attend: HashMap<String, Attend>,
    /// Places where she is in the middle of saying something.
    speaking: HashMap<String, Speaking>,
    /// The next time her day needs attention.
    life: Option<DateTime<Utc>>,
    /// The next time an intention comes due.
    intention: Option<DateTime<Utc>>,
    /// The next time she picks up her phone to scroll.
    phone: Option<DateTime<Utc>>,
    /// The next time she sits down to something she has taken on.
    work: Option<DateTime<Utc>>,
    /// The next time she comes to mean to tell her operators about her day.
    report: Option<DateTime<Utc>>,
}

impl Agenda {
    fn next_due(&self) -> Option<DateTime<Utc>> {
        self.attend
            .values()
            .map(|a| a.due)
            .chain(self.speaking.values().filter_map(Speaking::due))
            .chain(self.life)
            .chain(self.intention)
            .chain(self.phone)
            .chain(self.work)
            .chain(self.report)
            .min()
    }
}

/// What a job she sent off is for, while it runs. This is the one record
/// of what she is in the middle of: whatever way a job ends, its coming
/// back frees what it was for, so a job that falls over cannot leave her
/// thinking she is still at it.
enum Purpose {
    /// A look at a place, by its key.
    Look { place: String, looking: Looking },
    /// Planning the rest of her day.
    Plan,
    /// Writing up a block of today's plan.
    Live,
    /// Looking back on her day, or on one platform's part of it.
    Reflect,
    /// Writing down the earlier conversation of a place, by its key.
    Digest(String),
    /// Scrolling a channel.
    Browse(String),
    /// Asking a channel how her posts are doing.
    Measure(String),
    /// A sitting at the work with this id.
    Work { id: String, working: work::Working },
    /// A change to the record of the work with this id, on its way to her
    /// work database.
    Record(String),
}

/// A job that came back: what it was for, unless it was called off, and
/// what it brought back or how it fell over.
struct Done {
    purpose: Option<Purpose>,
    result: std::result::Result<Outcome, tokio::task::JoinError>,
}

/// The jobs she has sent off, and what each is for. Each runs in the span
/// she was in when she sent it, so that whatever it logs says whose it is
/// when one daemon runs several characters.
#[derive(Default)]
struct Jobs {
    set: JoinSet<Outcome>,
    purposes: HashMap<tokio::task::Id, (Purpose, AbortHandle)>,
}

impl Jobs {
    /// Send off `job`, which is for nothing she has to know is under way.
    fn spawn(&mut self, job: impl Future<Output = Outcome> + Send + 'static) {
        self.set.spawn(job.in_current_span());
    }

    /// Send off `job`, for `purpose`.
    fn spawn_for(&mut self, purpose: Purpose, job: impl Future<Output = Outcome> + Send + 'static) {
        let abort = self.set.spawn(job.in_current_span());
        self.purposes.insert(abort.id(), (purpose, abort));
    }

    async fn join_next(&mut self) -> Option<Done> {
        let (id, result) = match self.set.join_next_with_id().await? {
            Ok((id, outcome)) => (id, Ok(outcome)),
            Err(e) => (e.id(), Err(e)),
        };
        let purpose = self.purposes.remove(&id).map(|(purpose, _)| purpose);
        Some(Done { purpose, result })
    }

    fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Whether a job is under way for something `is` picks out.
    fn any(&self, is: impl Fn(&Purpose) -> bool) -> bool {
        self.purposes.values().any(|(purpose, _)| is(purpose))
    }

    /// The look under way at the place `key`, if there is one.
    fn looking(&self, key: &str) -> Option<&Looking> {
        self.purposes
            .values()
            .find_map(|(purpose, _)| match purpose {
                Purpose::Look { place, looking } if place == key => Some(looking),
                _ => None,
            })
    }

    /// Every look under way, by the key of its place.
    fn looks(&self) -> impl Iterator<Item = (&String, &Looking)> {
        self.purposes
            .values()
            .filter_map(|(purpose, _)| match purpose {
                Purpose::Look { place, looking } => Some((place, looking)),
                _ => None,
            })
    }

    /// Whether a look under way pursues the intention `seq`.
    fn pursuing(&self, seq: i64) -> bool {
        self.looks()
            .any(|(_, looking)| looking.intention == Some(seq))
    }

    /// The sittings in progress, by the id of their work, in its order.
    fn sittings(&self) -> Vec<(&String, &work::Working)> {
        let mut sittings: Vec<_> = self
            .purposes
            .values()
            .filter_map(|(purpose, _)| match purpose {
                Purpose::Work { id, working } => Some((id, working)),
                _ => None,
            })
            .collect();
        sittings.sort_by_key(|(id, _)| *id);
        sittings
    }

    /// Whether a sitting at the work `id` is in progress.
    fn sitting(&self, id: &str) -> bool {
        self.any(|p| matches!(p, Purpose::Work { id: at, .. } if at == id))
    }

    /// Whether the work `id` is held: a sitting is at it, or a change to
    /// its record is on its way.
    fn busy(&self, id: &str) -> bool {
        self.any(|p| match p {
            Purpose::Work { id: at, .. } | Purpose::Record(at) => at == id,
            _ => false,
        })
    }

    /// The sitting at the work `id`, to change how it goes on.
    fn sitting_mut(&mut self, id: &str) -> Option<&mut work::Working> {
        self.purposes
            .values_mut()
            .find_map(|(purpose, _)| match purpose {
                Purpose::Work { id: at, working } if at == id => Some(working),
                _ => None,
            })
    }

    /// Whether the job of the look under way at the place `key` has
    /// finished, and waits to be joined.
    #[cfg(test)]
    fn finished(&self, key: &str) -> bool {
        self.purposes.values().any(|(purpose, abort)| {
            matches!(purpose, Purpose::Look { place, .. } if place == key) && abort.is_finished()
        })
    }

    /// Call off the look under way at the place `key`, if there is one:
    /// it is no longer hers, and whatever it brings back is not acted on.
    fn call_off(&mut self, key: &str) -> Option<Looking> {
        let id = self.purposes.iter().find_map(|(id, (purpose, _))| {
            matches!(purpose, Purpose::Look { place, .. } if place == key).then_some(*id)
        })?;
        let (purpose, abort) = self.purposes.remove(&id)?;
        abort.abort();
        match purpose {
            Purpose::Look { looking, .. } => Some(looking),
            _ => None,
        }
    }
}

/// A ceiling on brain calls per hour.
struct Budget {
    cap: u32,
    spent: VecDeque<DateTime<Utc>>,
}

impl Budget {
    fn allow(&mut self, now: DateTime<Utc>) -> bool {
        while self
            .spent
            .front()
            .is_some_and(|at| now - *at > Duration::hours(1))
        {
            self.spent.pop_front();
        }
        self.cap == 0 || self.spent.len() < self.cap as usize
    }

    fn spend(&mut self, now: DateTime<Utc>) {
        self.spent.push_back(now);
    }

    /// Spend `calls` if there is room for one more call this hour, and say
    /// whether there was: what is about to ask a brain asks this, once.
    fn take(&mut self, now: DateTime<Utc>, calls: usize) -> bool {
        let room = self.allow(now);
        if room {
            self.spent.extend(std::iter::repeat_n(now, calls));
        }
        room
    }
}

pub struct Mind {
    ghost: Ghost,
    life: Life,
    cfg: MindConfig,
    tempo: Tempo,
    prompts: Prompts,
    think: Arc<dyn Brain>,
    tick: Arc<dyn Brain>,
    body: Arc<dyn Body>,
    senses: Arc<dyn Senses>,
    rx: mpsc::Receiver<Signal>,
    entries: broadcast::Sender<Entry>,
    /// What she is in the middle of, for her [`Reader`].
    live: tokio::sync::watch::Sender<reader::Live>,
    channels: HashMap<String, Channel>,
    agenda: Agenda,
    jobs: Jobs,
    rng: Rng,
    budget: Budget,
    /// The client every image she is sent is fetched with.
    images: reqwest::Client,
    workshop: Workshop,
    /// Work that is not to be sat down to again before the time given: it
    /// is resting after a sitting.
    resting: HashMap<String, DateTime<Utc>>,
    /// When this body started running.
    started: DateTime<Utc>,
    /// What writes her work database, off her mind: the rounds of her
    /// looks, and every change she makes to the records of her work.
    writer: writer::Writer,
    /// Her work database opened to read, the first time it is there to be
    /// read: what she reads of her work as she thinks is read on this one
    /// connection, so that looking never writes, never makes a record,
    /// never waits on the work, and does not open the file again each
    /// time. A console's questions are her [`Reader`]'s.
    records: std::cell::OnceCell<ghost_work::undertaking::Records>,
    /// Consecutive failed deliberations per place.
    retries: HashMap<String, u8>,
    /// Intentions put off until a later moment.
    held: HashMap<i64, DateTime<Utc>>,
    plan_failures: u8,
    /// Posts she has already scrolled past, per channel. Bounded.
    scrolled: HashMap<String, VecDeque<String>>,
}

impl Mind {
    pub fn new(parts: Parts) -> (Self, MindHandle) {
        let (tx, rx) = mpsc::channel(1024);
        let (entries, _) = broadcast::channel(1024);
        let life = Life::new(parts.ghost.identity());
        let writer = writer::Writer::new(parts.ghost.dir().join(ghost_work::WORK_DB));
        let mind = Self {
            life,
            tempo: Tempo::new(parts.config.tempo),
            budget: Budget {
                cap: parts.config.calls_per_hour,
                spent: VecDeque::new(),
            },
            images: view::client(),
            ghost: parts.ghost,
            cfg: parts.config,
            prompts: parts.prompts,
            think: parts.think,
            tick: parts.tick,
            body: parts.body,
            senses: parts.senses,
            rx,
            entries: entries.clone(),
            live: tokio::sync::watch::Sender::new(reader::Live::default()),
            channels: HashMap::new(),
            agenda: Agenda::default(),
            jobs: Jobs::default(),
            workshop: parts.workshop,
            resting: HashMap::new(),
            rng: Rng::new(),
            started: Utc::now(),
            writer,
            records: std::cell::OnceCell::new(),
            retries: HashMap::new(),
            held: HashMap::new(),
            plan_failures: 0,
            scrolled: HashMap::new(),
        };
        let reader = Reader::new(
            mind.ghost.dir().to_path_buf(),
            mind.cfg.operators.clone(),
            mind.live.subscribe(),
        );
        let handle = MindHandle {
            tx,
            entries,
            reader: Arc::new(std::sync::Mutex::new(reader)),
        };
        (mind, handle)
    }

    /// Live until told to stop. Returns only on `stop` or a storage failure.
    pub async fn run(mut self) -> Result<()> {
        self.begin(Utc::now())?;
        loop {
            let wait = self.agenda.next_due().map_or(IDLE, |due| {
                (due - Utc::now()).to_std().unwrap_or_default().min(IDLE)
            });
            tokio::select! {
                signal = self.rx.recv() => match signal {
                    None | Some(Signal::Stop) => break,
                    Some(signal) => self.on_signal(signal, Utc::now())?,
                },
                Some(done) = self.jobs.join_next(), if !self.jobs.is_empty() => {
                    self.on_done(done, Utc::now())?;
                }
                () = tokio::time::sleep(wait) => self.on_time(Utc::now())?,
            }
        }
        info!("{} is left be", self.ghost.identity().name());
        Ok(())
    }

    /// Write down the brain she thinks with, if it is not the one her
    /// journal last named: her body is started with a brain and keeps it,
    /// so a change of brain happens here and nowhere else.
    fn note_brain(&mut self) -> Result<()> {
        let think = self.think.label();
        let was = match self.ghost.journal().last_of(&["brain_changed"])? {
            Some(last) => match last.event {
                Event::BrainChanged { think, .. } => Some(think),
                _ => None,
            },
            None => None,
        };
        if was.as_deref() != Some(think.as_str()) {
            self.record(Event::BrainChanged { think, was })?;
        }
        Ok(())
    }

    /// Pick her life back up: note any gap, then rebuild the agenda from what
    /// the journal says is unread and undone.
    fn begin(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.started = now;
        if let Some(last) = self.ghost.journal().last()?
            && now - last.at > DORMANT_AFTER
            && !matches!(last.event, Event::Born { .. })
        {
            self.record(Event::Dormant { since: last.at })?;
        }
        self.note_brain()?;
        // Settle whether she is awake before anything else can happen.
        self.live(now)?;
        // Whatever servers of tools she installed are still hers. None is
        // started until she loads it.
        if let Some(servers) = &self.workshop.servers {
            servers.know(self.ghost.memory().installed()?);
        }
        // So are her skills, unless their folders changed while her body
        // was not running: those are put aside, and she is told why at
        // once.
        if let Some(skills) = &self.workshop.skills {
            skills.know(self.prompts.store(), self.ghost.memory().skills()?);
            self.installs_changed()?;
        }
        // Whatever she had taken on is still hers.
        if self.can_work() && !self.ghost.memory().undertakings(true, 1)?.is_empty() {
            self.agenda.work = Some(now + Duration::minutes(1));
        }
        // Today's report, if it has not been made: at its time, or at once
        // if that has passed while her body was not running.
        self.agenda.report = self.report_time(now, 0);
        self.refresh_intentions()
    }

    /// Put in her journal what happened to her servers of tools and her
    /// skills since the last look: what she installed at work, and what is
    /// gone and why. They happen in whatever loop used them; only she
    /// writes them down, at every moment of her day and as each job comes
    /// back, so a sitting that installs something has it on the record
    /// within one glance.
    fn installs_changed(&mut self) -> Result<()> {
        use ghost_work::{servers, skills};
        let mut changes = Vec::new();
        if let Some(servers) = &self.workshop.servers {
            changes.extend(servers.take_changes().into_iter().map(|c| match c {
                servers::Change::Installed {
                    name,
                    version,
                    description,
                    source,
                    owner,
                    how,
                    tools,
                    hash,
                } => Event::Installed {
                    name,
                    version,
                    description,
                    source,
                    owner,
                    how,
                    tools,
                    hash,
                },
                servers::Change::Uninstalled { name, why } => Event::Uninstalled { name, why },
            }));
        }
        if let Some(skills) = &self.workshop.skills {
            changes.extend(skills.take_changes().into_iter().map(|c| match c {
                skills::Change::Installed {
                    name,
                    source,
                    path,
                    sha,
                    hash,
                    description,
                } => Event::SkillInstalled {
                    name,
                    source,
                    path,
                    sha,
                    hash,
                    description,
                },
                skills::Change::Uninstalled { name, why } => Event::SkillUninstalled { name, why },
            }));
        }
        for event in changes {
            info!("tools: {}", event.kind());
            self.record(event)?;
        }
        Ok(())
    }

    fn on_signal(&mut self, signal: Signal, now: DateTime<Utc>) -> Result<()> {
        self.prompts.refresh(now);
        match signal {
            Signal::Heard { channel, message } => self.hear(&channel, message, now)?,
            Signal::Typing { channel, place } => {
                let floor = after(now, self.tempo.settle() * 2);
                if let Some(attend) = self.agenda.attend.get_mut(&social::key(&channel, &place)) {
                    attend.due = attend.due.max(floor);
                }
            }
            Signal::Online {
                channel,
                me,
                can,
                facts,
            } => {
                info!(
                    "{channel} is reachable{}",
                    me.as_ref()
                        .map(|m| format!(" as {}", m.handle.as_deref().unwrap_or(&m.name)))
                        .unwrap_or_default()
                );
                self.channels
                    .insert(channel.clone(), Channel { me, can, facts });
                for place in self.ghost.memory().places_unread()? {
                    if place.channel == channel {
                        self.schedule_attend(&place.key, now)?;
                    }
                }
                if self.agenda.phone.is_none() {
                    self.agenda.phone = self.next_phone(now);
                }
                self.refresh_intentions()?;
            }
            Signal::Offline { channel } => {
                info!("{channel} is out of reach");
                self.channels.remove(&channel);
            }
            Signal::Ask { question, answer } => {
                let _ = answer.send(self.answer(&question));
            }
            Signal::Status { answer } => {
                let _ = answer.send(self.status(now)?);
            }
            Signal::Order { order, by, answer } => self.on_order(order, &by, answer)?,
            Signal::Interrupt { place, by, answer } => {
                let _ = answer.send(self.interrupt(&place, &by, now)?);
            }
            Signal::Rewind {
                place,
                to,
                by,
                answer,
            } => {
                let _ = answer.send(self.rewind(&place, to, &by, now)?);
            }
            Signal::Session { change, by, answer } => {
                let _ = answer.send(self.on_session(change, &by)?);
            }
            Signal::Stop => {}
        }
        self.publish();
        Ok(())
    }

    fn on_time(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.prompts.refresh(now);
        self.installs_changed()?;
        let due = |at: Option<DateTime<Utc>>| at.is_some_and(|at| at <= now);
        if due(self.agenda.life) {
            self.live(now)?;
        }
        let places: Vec<String> = self
            .agenda
            .attend
            .iter()
            .filter(|(_, a)| a.due <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for place in places {
            self.attend(&place, now)?;
        }
        let speaking: Vec<String> = self
            .agenda
            .speaking
            .iter()
            .filter(|(_, s)| s.due().is_some_and(|at| at <= now))
            .map(|(key, _)| key.clone())
            .collect();
        for place in speaking {
            self.speak(&place, now)?;
        }
        // Before intentions: a report meant now is acted on in this moment.
        if due(self.agenda.report) {
            self.report(now)?;
        }
        if due(self.agenda.intention) {
            self.pursue(now)?;
        }
        if due(self.agenda.phone) {
            self.browse(now)?;
        }
        if due(self.agenda.work) {
            self.work(now)?;
        }
        self.publish();
        Ok(())
    }

    /// A job came back, with what it brought or having fallen over. What
    /// it was for is no longer under way either way.
    fn on_done(&mut self, done: Done, now: DateTime<Utc>) -> Result<()> {
        let handled = match done.result {
            Ok(outcome) => self.on_outcome(outcome, done.purpose, now),
            // Called off: she was stopped in the middle of it.
            Err(e) if e.is_cancelled() => Ok(()),
            Err(e) => {
                warn!("a job fell over: {e}");
                self.on_fell(done.purpose, now)
            }
        };
        self.publish();
        handled
    }

    /// Say what she is in the middle of, for her reader, if it changed.
    /// Called once each thing she handles is handled: what is under way
    /// changes only there.
    fn publish(&self) {
        let live = reader::Live {
            sittings: self
                .jobs
                .sittings()
                .into_iter()
                .map(|(id, _)| id.clone())
                .collect(),
            looks: self
                .jobs
                .looks()
                .map(|(place, looking)| (place.clone(), looking.kept))
                .collect(),
            online: self.channels.keys().cloned().collect(),
        };
        self.live.send_if_modified(|was| {
            let changed = *was != live;
            *was = live;
            changed
        });
    }

    /// A job for `purpose` fell over before it brought anything back. What
    /// it was for is free to be done again, and is put back on her agenda
    /// where nothing else would bring it round: her day, her phone and her
    /// posts come round on their own.
    fn on_fell(&mut self, purpose: Option<Purpose>, now: DateTime<Utc>) -> Result<()> {
        match purpose {
            Some(Purpose::Look { place, looking }) => match looking.intention {
                // Tried again later rather than lost, as when she could
                // not think.
                Some(intention) => {
                    self.held.insert(intention, now + Duration::minutes(10));
                    self.refresh_intentions()
                }
                None => self.schedule_attend(&place, now),
            },
            Some(Purpose::Work { id, .. }) => {
                self.resting.insert(id, now + Duration::minutes(10));
                self.work_at(now);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn on_outcome(
        &mut self,
        outcome: Outcome,
        purpose: Option<Purpose>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.prompts.refresh(now);
        self.installs_changed()?;
        match outcome {
            Outcome::Thought { job, result } => {
                // Without its purpose, the look was called off before it
                // came back: what it came to is not hers to act on.
                if purpose.is_none() {
                    tracing::debug!("{}: a look she was stopped in came back", job.place);
                    return Ok(());
                }
                self.on_thought(*job, result, now)
            }
            Outcome::Sent {
                place,
                bubble,
                result,
            } => self.on_sent(&place, bubble, result, now),
            Outcome::Planned {
                day,
                weather,
                result,
            } => self.on_planned(day, weather, result, now),
            Outcome::Lived { block, result } => self.on_lived(block, result, now),
            Outcome::Reflected {
                channel,
                people,
                sources,
                result,
            } => self.on_reflected(channel.as_deref(), &people, Some(sources), result, now),
            Outcome::Digested {
                place,
                upto,
                sources,
                result,
            } => self.on_digested(&place, upto, sources, result),
            Outcome::Browsed {
                channel,
                items,
                result,
            } => self.on_browsed(&channel, &items, result, now),
            Outcome::Posted {
                channel,
                text,
                result,
            } => self.on_posted(channel, text, result),
            Outcome::Measured { channel, result } => self.on_measured(&channel, result),
            Outcome::Worked { id, sat } => {
                // A sitting is never called off: it is stopped, and comes
                // back, so what it was for is always there.
                let Some(Purpose::Work { working, .. }) = purpose else {
                    warn!("a sitting at {id} came back that was not under way");
                    return Ok(());
                };
                self.on_worked(&id, *sat, &working, now)
            }
            Outcome::Unseated { id, why } => self.on_unseated(id, why, now),
            Outcome::Added {
                id,
                text,
                by,
                result,
                answer,
            } => self.on_added(id, text, by, result, answer, now),
            Outcome::Stopped { id, by, state } => self.on_stopped(&id, by.as_deref(), state, now),
            Outcome::Ended { id, summary, files } => self.on_ended(&id, &summary, &files, now),
            Outcome::Viewed { message, seen } => self.on_viewed(message, seen),
            Outcome::Done => Ok(()),
        }
    }

    /// Write into her journal and tell anyone watching.
    fn record(&mut self, event: Event) -> Result<Entry> {
        let entry = self.ghost.record(event)?;
        let _ = self.entries.send(entry.clone());
        Ok(entry)
    }

    /// Do something on a channel without waiting to hear how it went.
    fn fire(&mut self, channel: &str, act: Act) {
        let body = self.body.clone();
        let channel = channel.to_owned();
        self.jobs.spawn(async move {
            if let Err(e) = body.act(&channel, act).await {
                warn!("{channel}: {e}");
            }
            Outcome::Done
        });
    }

    /// What `channel` is like, as it said when it came online. Out of
    /// reach, the console is still the console, and anything else is taken
    /// for a chat that allows anything.
    pub(super) fn facts(&self, channel: &str) -> crate::ChannelFacts {
        match self.channels.get(channel) {
            Some(c) => c.facts,
            None if channel == crate::CONSOLE => crate::ChannelFacts::console(),
            None => crate::ChannelFacts::default(),
        }
    }

    /// The pace she keeps at `place_key`: none at a terminal, where she is
    /// waited on, and her own everywhere else.
    pub(super) fn pace(&self, place_key: &str) -> Tempo {
        match social::split_key(place_key) {
            Some((channel, _)) if self.facts(channel).waited_on => Tempo::new(0.0),
            _ => self.tempo,
        }
    }

    fn can(&self, channel: &str, what: Can) -> bool {
        self.channels
            .get(channel)
            .is_some_and(|c| c.can.contains(&what))
    }

    /// Who she is, and what moment this is. The first is the system prompt
    /// and changes only when a fact is added to her life; the second
    /// changes by the hour and goes late in the request, so that a provider
    /// which caches what it has read before can reuse everything ahead of it.
    fn portrait(&self, now: DateTime<Utc>) -> Result<(String, String)> {
        self.portrait_on(now, None, "")
    }

    /// The same, for a moment spent on platform `on`. What she posted is
    /// that platform's record, so only her posts there are among what she
    /// has been doing lately; with no platform, none are. `abilities` is
    /// what she can do through the phone she is on, if that is said.
    fn portrait_on(
        &self,
        now: DateTime<Utc>,
        on: Option<&str>,
        abilities: &str,
    ) -> Result<(String, String)> {
        let journal = self.ghost.journal();
        let moment = self.life.moment(journal, now)?;
        let day = self.life.today(journal, now)?;
        let mut recent: Vec<Entry> = journal
            .last_n_of(&["lived", "reflected", "posted", "dormant"], 30)?
            .into_iter()
            .filter(|e| match &e.event {
                Event::Posted { channel, .. } => on == Some(channel.as_str()),
                _ => true,
            })
            .collect();
        let extra = recent.len().saturating_sub(10);
        recent.drain(..extra);
        let canon = self.ghost.memory().canon(40)?;
        let scene = Scene {
            identity: self.ghost.identity(),
            clock: self.life.clock(),
            now,
            day: day.as_ref(),
            recent: &recent,
            canon: &canon,
        };
        let mut pairs = scene.pairs(&self.prompts);
        // The exchanges in her persona are introduced as how she sounds, and
        // the introduction goes with them when she has none.
        for (key, value) in &mut pairs {
            if *key == "examples" {
                *value = self.prompts.section("examples", "examples", value);
            }
        }
        pairs.push(("state", self.state_words(&moment)));
        pairs.push(("facets", self.facets_words()));
        pairs.push(("abilities", abilities.to_owned()));
        Ok((
            self.prompts.system(&pairs),
            narrate::fill(&self.prompts.now, &pairs),
        ))
    }

    /// What she is doing as she reads, in the words of her prompt files.
    fn state_words(&self, moment: &ghost_core::life::Moment) -> String {
        if moment.phase == ghost_core::life::Phase::Asleep {
            return self.prompts.text("state_asleep").to_owned();
        }
        let Some(doing) = &moment.doing else {
            return self.prompts.text("state_idle").to_owned();
        };
        let reach = match doing.reach {
            ghost_core::life::Reach::Free => "reach_free",
            ghost_core::life::Reach::Glance => "reach_glance",
            ghost_core::life::Reach::Busy => "reach_busy",
            ghost_core::life::Reach::Away => "reach_away",
        };
        let tz = self.life.clock().tz();
        self.prompts.fill(
            "state_doing",
            &[
                ("what", doing.what.clone()),
                (
                    "until",
                    doing.to.with_timezone(&tz).format("%H:%M").to_string(),
                ),
                ("reach", self.prompts.text(reach).to_owned()),
            ],
        )
    }

    /// Who she is: the system prompt.
    fn who(&self, now: DateTime<Utc>) -> Result<String> {
        Ok(self.portrait(now)?.0)
    }

    /// A request about `prompt`, as her, at this moment.
    fn framed(&self, now: DateTime<Utc>, prompt: &str) -> Result<ghost_brain::Completion> {
        let (who, right_now) = self.portrait(now)?;
        Ok(ghost_brain::Completion::ask(
            who,
            format!("{right_now}\n{prompt}"),
            self.cfg.max_tokens,
        ))
    }

    /// Everything a brain is told about her, in one piece.
    #[cfg(test)]
    fn self_prompt(&self, now: DateTime<Utc>) -> Result<String> {
        let (who, right_now) = self.portrait(now)?;
        Ok(format!("{who}\n{right_now}"))
    }
}

/// `now` plus a std duration, without overflowing.
fn after(now: DateTime<Utc>, wait: StdDuration) -> DateTime<Utc> {
    now + Duration::from_std(wait).unwrap_or_else(|_| Duration::days(365))
}
