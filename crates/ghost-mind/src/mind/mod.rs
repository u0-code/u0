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
mod digest;
mod give;
mod group;
mod keep;
mod living;
mod public;
mod pursuit;
mod reach;
mod report;
mod reread;
mod speak;
#[cfg(test)]
mod tests;
mod view;
mod work;

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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
    narrate::{self, Scene},
    social::{self, Actor},
    tempo::Tempo,
};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinSet,
};
use tracing::{info, warn};

use self::{
    attend::{Thinking, Thought},
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
    /// How she reaches the web.
    pub web: ghost_work::tools::WebConfig,
    /// The `ghost-work` program, for a brain that is a command-line agent:
    /// it is what serves her tools to that agent.
    pub runner: Option<std::path::PathBuf>,
    /// The arguments that tell `ghost-work` about the same machine and web
    /// access as above.
    pub runner_args: Vec<String>,
    /// Servers of tools: those she installs from the registry, and those
    /// whoever runs her body gives her. `None` is none of either.
    pub servers: Option<Arc<ghost_work::servers::Servers>>,
}

/// Something an observer can ask a running mind.
#[derive(Debug, Clone)]
pub enum Question {
    /// Who she is and what she is in the middle of.
    Status,
    /// What is scheduled, and when.
    Agenda,
    /// Everyone she knows and where she stands with them.
    People,
    /// Her persona and its seal.
    Identity,
    /// What is true about her life, oldest first.
    Canon,
    /// Every place she is in, most recently active first.
    Places,
    /// The journal entries that belong to one place, oldest first.
    Thread { place: String, limit: usize },
    /// Journal entries from seq `from` on. Zero or less counts back from the
    /// newest entry.
    Journal { from: i64, limit: usize },
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
    },
    Offline {
        channel: String,
    },
    Ask {
        question: Question,
        answer: oneshot::Sender<Value>,
    },
    Stop,
}

/// The outside world's handle on a running mind. Cheap to clone.
#[derive(Clone)]
pub struct MindHandle {
    tx: mpsc::Sender<Signal>,
    entries: broadcast::Sender<Entry>,
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

    /// A channel became reachable. `me` is who she is there.
    pub async fn online(&self, channel: &str, me: Option<Actor>, can: Vec<Can>) -> Result<()> {
        self.send(Signal::Online {
            channel: channel.to_owned(),
            me,
            can,
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
    /// A post was read over and not sent.
    Held {
        channel: String,
        text: String,
        why: String,
    },
    /// A sitting of work ended.
    Worked {
        id: String,
        /// Steps the undertaking had taken before this sitting.
        before: u64,
        report: ghost_work::Report,
        progress: String,
        /// Its record, with everything it kept and told in it.
        record: Arc<ghost_work::Undertaking>,
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
}

struct Attend {
    due: DateTime<Utc>,
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
    channels: HashMap<String, Channel>,
    agenda: Agenda,
    jobs: JoinSet<Outcome>,
    rng: Rng,
    budget: Budget,
    workshop: Workshop,
    /// The sittings in progress, by the work they are on.
    working: BTreeMap<String, work::Working>,
    /// Work that is not to be sat down to again before the time given: it
    /// is resting after a sitting.
    resting: HashMap<String, DateTime<Utc>>,
    /// Channels that have been asked how her posts are doing and have not
    /// answered yet.
    measuring: HashSet<String>,
    /// When this body started running.
    started: DateTime<Utc>,
    /// Places with a deliberation in flight.
    thinking: HashSet<String>,
    /// Consecutive failed deliberations per place.
    retries: HashMap<String, u8>,
    /// Intentions with a deliberation in flight.
    pursuing: HashSet<i64>,
    /// Intentions put off until a later moment.
    held: HashMap<i64, DateTime<Utc>>,
    planning: bool,
    plan_failures: u8,
    /// The block whose `Lived` entry is being written.
    living: Option<u32>,
    /// How many looks back on the day are still being thought.
    reflecting: usize,
    /// Places whose earlier conversation she is writing down right now.
    digesting: HashSet<String>,
    /// Channels with a scroll in flight.
    browsing: HashSet<String>,
    /// Posts she has already scrolled past, per channel. Bounded.
    scrolled: HashMap<String, VecDeque<String>>,
}

impl Mind {
    pub fn new(parts: Parts) -> (Self, MindHandle) {
        let (tx, rx) = mpsc::channel(1024);
        let (entries, _) = broadcast::channel(1024);
        let life = Life::new(parts.ghost.identity());
        let mind = Self {
            life,
            tempo: Tempo::new(parts.config.tempo),
            budget: Budget {
                cap: parts.config.calls_per_hour,
                spent: VecDeque::new(),
            },
            ghost: parts.ghost,
            cfg: parts.config,
            prompts: parts.prompts,
            think: parts.think,
            tick: parts.tick,
            body: parts.body,
            senses: parts.senses,
            rx,
            entries: entries.clone(),
            channels: HashMap::new(),
            agenda: Agenda::default(),
            jobs: JoinSet::new(),
            workshop: parts.workshop,
            working: BTreeMap::new(),
            resting: HashMap::new(),
            measuring: HashSet::new(),
            rng: Rng::new(),
            started: Utc::now(),
            thinking: HashSet::new(),
            retries: HashMap::new(),
            pursuing: HashSet::new(),
            held: HashMap::new(),
            planning: false,
            plan_failures: 0,
            living: None,
            reflecting: 0,
            digesting: HashSet::new(),
            browsing: HashSet::new(),
            scrolled: HashMap::new(),
        };
        (mind, MindHandle { tx, entries })
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
                Some(done) = self.jobs.join_next(), if !self.jobs.is_empty() => match done {
                    Ok(outcome) => self.on_outcome(outcome, Utc::now())?,
                    Err(e) => warn!("a job fell over: {e}"),
                },
                () = tokio::time::sleep(wait) => self.on_time(Utc::now())?,
            }
        }
        info!("{} is left be", self.ghost.identity().name());
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
        // Settle whether she is awake before anything else can happen.
        self.live(now)?;
        // Whatever servers of tools she installed are still hers. None is
        // started until she loads it.
        if let Some(servers) = &self.workshop.servers {
            servers.know(self.ghost.memory().installed()?);
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

    /// Put in her journal what happened to her servers of tools since the
    /// last look: what she installed at work, and what is gone and why.
    /// They happen in whatever loop used them; only she writes them down,
    /// at every moment of her day and as each job comes back, so a sitting
    /// that installs something has it on the record within one glance.
    fn servers_changed(&mut self) -> Result<()> {
        let changes = match &self.workshop.servers {
            Some(servers) => servers.take_changes(),
            None => return Ok(()),
        };
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
            Signal::Online { channel, me, can } => {
                info!(
                    "{channel} is reachable{}",
                    me.as_ref()
                        .map(|m| format!(" as {}", m.handle.as_deref().unwrap_or(&m.name)))
                        .unwrap_or_default()
                );
                self.channels.insert(channel.clone(), Channel { me, can });
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
                let _ = answer.send(self.answer(&question, now)?);
            }
            Signal::Stop => {}
        }
        Ok(())
    }

    fn on_time(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.prompts.refresh(now);
        self.servers_changed()?;
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
        Ok(())
    }

    fn on_outcome(&mut self, outcome: Outcome, now: DateTime<Utc>) -> Result<()> {
        self.prompts.refresh(now);
        self.servers_changed()?;
        match outcome {
            Outcome::Thought { job, result } => self.on_thought(*job, result, now),
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
            Outcome::Held { channel, text, why } => self.on_held(&channel, &text, &why),
            Outcome::Worked {
                id,
                before,
                report,
                progress,
                record,
            } => self.on_worked(&id, before, &report, progress, &record, now),
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
            moment: &moment,
            day: day.as_ref(),
            recent: &recent,
            canon: &canon,
        };
        let mut pairs = scene.pairs();
        // The exchanges in her persona are introduced as how she sounds, and
        // the introduction goes with them when she has none.
        for (key, value) in &mut pairs {
            if *key == "examples" {
                *value = self.prompts.section("examples", "examples", value);
            }
            if *key == "state" {
                *value = self.state_words(&moment);
            }
        }
        pairs.push(("abilities", abilities.to_owned()));
        Ok((
            narrate::fill(&self.prompts.this, &pairs),
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
