//! The desk's connections to daemons, on a runtime of their own.
//!
//! Two kinds. A [`Link`] follows one ghost on one daemon as a console: it
//! asks about her, streams her journal, and carries what whoever sits at
//! the desk says to her and does to her work, as an operator of hers, with
//! `console.say`, `work.answer` and `work.stop`; and it reads what the
//! thread asks for: her steps, what she called while she looked and what
//! her work did (`look.steps`, `work.steps`), and the entries of a place
//! older than what the desk holds (`journal.place`). Each session with the
//! solver is a place of its own on the console channel, so her journal
//! keeps the threads apart. What it reads of her journal is added to what
//! the desk holds, never put in its place: the tail once, then the stream,
//! and after a daemon was out of reach what was written meanwhile. A [`Listing`] asks a daemon who it runs, now
//! and then, and what each of them is waiting on you for, so that the desk
//! can show a daemon's cast and its Attention without opening any of them;
//! and it carries an answer given from Attention on a short connection of
//! its own.
//!
//! All of it runs on one small tokio runtime and hands what it learns to
//! the UI as [`Update`]s over a plain channel; the UI drains it on its own
//! clock. Dropping either stops it. Nothing here touches a ghost's files.
//!
//! The brains of the core the desk runs are asked about and changed with
//! [`brains`], on a short connection of its own each time: they are the
//! core's, and the desk reads none of its files for them.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use ghost_link::{
    Console,
    protocol::{BrainAsk, BrainList, BrainSave, SayMode},
};
use tokio::{runtime::Runtime, task::JoinHandle};
use tracing::{debug, info};

use crate::{
    follow::Read,
    model::{Entry, LinkState, Status, Update},
};

/// How many journal entries the desk reads when it first connects.
pub const TAIL: u32 = 400;
/// How many entries of a place are read when its thread is first shown.
const PLACE_READ: u32 = 200;
/// How often it asks after her, and after who a daemon runs, again.
const REFRESH: Duration = Duration::from_secs(20);
/// The first wait before trying a lost daemon again; it doubles on each
/// failure up to `RETRY_MOST`, so a daemon that is down for the night is
/// not knocked on every three seconds. Asking to retry now skips the wait.
const RETRY: Duration = Duration::from_secs(3);
const RETRY_MOST: Duration = Duration::from_secs(30);
/// How often each ghost a daemon runs is asked what is waiting on you. A
/// minute is soon enough for what waits on a person, and costs one short
/// connection per character a minute.
const ATTENTION: Duration = Duration::from_secs(60);

/// Wait out `wait`, or less if someone asks to try again now; then the next
/// wait, doubled up to the most. Asked to try now, the wait starts over.
async fn back_off(wait: &mut Duration, wake: &tokio::sync::Notify) {
    tokio::select! {
        () = tokio::time::sleep(*wait) => *wait = (*wait * 2).min(RETRY_MOST),
        () = wake.notified() => *wait = RETRY,
    }
}

/// Where every link runs: two threads are plenty for a handful of sockets.
pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ghost-link")
            .enable_all()
            .build()
            .expect("a tokio runtime")
    })
}

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub token: Option<String>,
    /// Which of the daemon's characters, by her id: `None` when it runs
    /// only one.
    pub ghost: Option<String>,
    /// Who you are to her: the console says so in its hello, and she knows
    /// what it says as yours, an operator's.
    pub who: String,
}

/// Something whoever sits at the desk does to her, as an operator of hers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    /// Say `text` in the place `place` on the console channel, by its id.
    Say {
        place: String,
        text: String,
        mode: SayMode,
    },
    /// Answer what the work `work` asked, or add to what it was asked.
    Answer { work: String, text: String },
    /// Stop the work `work`.
    Stop { work: String },
    /// Stop what she is doing about the place `place` on the console
    /// channel: her look there, and the work directed from there.
    StopPlace { place: String },
    /// Take back your line `to` (the seq of its `heard` entry) in the
    /// place `place` on the console channel, and everything said there
    /// after it, to put it another way.
    Rewind { place: String, to: i64 },
}

impl Order {
    /// What it was, in a few words, for a line saying it could not be done.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Say { .. } => "say that",
            Self::Answer { .. } => "answer that",
            Self::Stop { .. } => "stop that work",
            Self::StopPlace { .. } => "stop her",
            Self::Rewind { .. } => "take that back",
        }
    }
}

/// Carry out `order` on a console that said who is at it. Says whether
/// anything was stopped, for a stop in a place; otherwise that it was done.
async fn carry_out(console: &mut Console, order: &Order) -> anyhow::Result<bool> {
    match order {
        Order::Say { place, text, mode } => {
            console.say(Some(place.clone()), text, *mode).await?;
        }
        Order::Answer { work, text } => console.answer(work, text).await?,
        Order::Stop { work } => console.stop(work).await?,
        Order::StopPlace { place } => return console.stop_place(Some(place.clone())).await,
        Order::Rewind { place, to } => {
            console.rewind_place(Some(place.clone()), *to).await?;
        }
    }
    Ok(true)
}

type Orders = tokio::sync::mpsc::UnboundedReceiver<Order>;
type Reads = tokio::sync::mpsc::UnboundedReceiver<Read>;

/// Ask what `read` asks, and hand the answer, or why there is none, to the
/// desk. A daemon from before steps were kept refuses it; that is said,
/// and the connection goes on.
async fn read_steps(console: &mut Console, read: Read, up: &mpsc::Sender<Update>) {
    let update = match read {
        Read::Looks { place, after } => Update::Looks {
            looks: console
                .look_steps(&place, after)
                .await
                .map_err(|e| format!("{e:#}")),
            place,
        },
        Read::Steps { work, after } => Update::Steps {
            log: console
                .work_steps(&work, after, None)
                .await
                .map_err(|e| format!("{e:#}")),
            work,
        },
        // Asked after every step there could be, the answer holds none of
        // them, and what the newest one was given.
        Read::Context { work } => Update::Context {
            context: console
                .work_steps(&work, Some(u64::MAX), Some(1))
                .await
                .map(|log| log.context)
                .map_err(|e| format!("{e:#}")),
            work,
        },
        // A place that cannot be read shows what the desk holds of it; a
        // daemon that refuses says so in the log, not on the desk.
        Read::Place { place } => match console.thread(&place, PLACE_READ).await {
            Ok(entries) => Update::Place(sorted(&entries)),
            Err(e) => {
                debug!("could not read the place {place}: {e:#}");
                return;
            }
        },
    };
    let _ = up.send(update);
}

/// Something asked of a core about its brains, which are its own: the desk
/// reads and changes them only this way, never in the core's files.
#[derive(Debug, Clone)]
pub enum BrainCall {
    List,
    Save(BrainSave),
    Remove(String),
    /// Think with the brain `name`, on `model` if given.
    Use {
        name: String,
        model: Option<String>,
    },
    Models(BrainAsk),
    Check(BrainAsk),
}

/// What the core answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrainAnswer {
    /// The brains, as they are now.
    List(BrainList),
    Models(Vec<String>),
    /// It thought, when asked to.
    Checked,
}

/// Why the core did not do what it was asked: what it said, and its error
/// code when the core answered at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub code: Option<i64>,
    pub why: String,
}

impl Refused {
    /// The core's own words for why, or the desk's when it could not be
    /// reached.
    fn from(e: &anyhow::Error) -> Self {
        match e.downcast_ref::<ghost_link::protocol::RpcError>() {
            Some(rpc) => Self {
                code: Some(rpc.code),
                why: rpc.message.clone(),
            },
            None => Self {
                code: None,
                why: format!("{e:#}"),
            },
        }
    }
}

/// Ask the core at `url` about its brains, on a connection of its own.
pub async fn brains(
    url: String,
    token: Option<String>,
    call: BrainCall,
) -> Result<BrainAnswer, Refused> {
    let asked = async {
        let (mut console, _) = Console::connect(&url, token, None).await?;
        anyhow::Ok(match call {
            BrainCall::List => BrainAnswer::List(console.brains().await?),
            BrainCall::Save(save) => BrainAnswer::List(console.save_brain(&save).await?),
            BrainCall::Remove(name) => BrainAnswer::List(console.remove_brain(&name).await?),
            BrainCall::Use { name, model } => {
                BrainAnswer::List(console.use_brain(&name, model).await?)
            }
            BrainCall::Models(ask) => BrainAnswer::Models(console.brain_models(&ask).await?),
            BrainCall::Check(ask) => {
                console.check_brain(&ask).await?;
                BrainAnswer::Checked
            }
        })
    };
    asked.await.map_err(|e| Refused::from(&e))
}

/// Entries as the daemon serves them, oldest first.
fn sorted(values: &[serde_json::Value]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = values.iter().filter_map(Entry::from_value).collect();
    entries.sort_by_key(|e| e.seq);
    entries
}

/// One ghost the desk follows.
pub struct Link {
    /// Where what the link learns arrives.
    pub rx: mpsc::Receiver<Update>,
    /// Where what you do to her goes. It waits there while her daemon
    /// cannot be reached, and goes when it can.
    orders: tokio::sync::mpsc::UnboundedSender<Order>,
    /// Where what the thread wants to read of her steps goes.
    reads: tokio::sync::mpsc::UnboundedSender<Read>,
    task: JoinHandle<()>,
    /// Rung to try a lost daemon again at once.
    wake: Arc<tokio::sync::Notify>,
}

impl Link {
    /// Follow the ghost `cfg` names: ask about her, follow her journal from
    /// the entry after `newest`, the newest the desk holds of it (zero for
    /// none), and carry what you do to her.
    pub fn watch(cfg: Config, newest: i64) -> Self {
        let (up, rx) = mpsc::channel();
        let (orders, taken) = tokio::sync::mpsc::unbounded_channel();
        let (reads, to_read) = tokio::sync::mpsc::unbounded_channel();
        let wake = Arc::new(tokio::sync::Notify::new());
        let task = runtime().spawn(console(cfg, newest, up, wake.clone(), taken, to_read));
        Self {
            rx,
            orders,
            reads,
            task,
            wake,
        }
    }

    /// Read some of her steps, once her daemon can be reached. The answer
    /// comes back as [`Update::Looks`] or [`Update::Steps`].
    pub fn read(&self, read: Read) {
        let _ = self.reads.send(read);
    }

    /// Try the daemon again now, if it is lost, instead of after the wait.
    pub fn retry(&self) {
        self.wake.notify_one();
    }

    /// Do `order` to her, as soon as her daemon can be reached.
    pub fn order(&self, order: Order) {
        let _ = self.orders.send(order);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Who one daemon runs, and what each of them waits on you for, asked now
/// and then.
pub struct Listing {
    pub rx: mpsc::Receiver<Update>,
    up: mpsc::Sender<Update>,
    url: String,
    token: Option<String>,
    task: JoinHandle<()>,
    wake: Arc<tokio::sync::Notify>,
    /// Set, with `wake` rung, to ask what waits on you now rather than when
    /// the minute is up.
    attend: Arc<AtomicBool>,
}

impl Listing {
    /// Ask the daemon at `url` who it runs, and again every little while,
    /// connecting again when it goes away; and each of them, every minute,
    /// what is waiting on you.
    pub fn follow(url: String, token: Option<String>) -> Self {
        let (up, rx) = mpsc::channel();
        let wake = Arc::new(tokio::sync::Notify::new());
        let attend = Arc::new(AtomicBool::new(true));
        let task = runtime().spawn(listing(
            url.clone(),
            token.clone(),
            up.clone(),
            wake.clone(),
            attend.clone(),
        ));
        Self {
            rx,
            up,
            url,
            token,
            task,
            wake,
            attend,
        }
    }

    /// Ask the daemon again now, instead of after the wait.
    pub fn retry(&self) {
        self.wake.notify_one();
    }

    /// Ask everyone it runs what is waiting on you, now.
    pub fn attend(&self) {
        self.attend.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }

    /// Do `order` to its character `ghost`, as `who`, on a connection of
    /// its own; then ask again what waits, so that what was answered goes.
    /// Whether it could be done comes back as [`Update::Ordered`].
    pub fn order(&self, ghost: String, who: String, order: Order) {
        let (url, token, up) = (self.url.clone(), self.token.clone(), self.up.clone());
        let (attend, wake) = (self.attend.clone(), self.wake.clone());
        runtime().spawn(async move {
            let done = async {
                let (mut console, _) =
                    Console::connect_as(&url, token, Some(ghost.clone()), Some(who)).await?;
                carry_out(&mut console, &order).await
            }
            .await
            .map(|_| ());
            let _ = up.send(Update::Ordered {
                why: done
                    .err()
                    .map(|e| format!("Could not {}: {e:#}", order.what())),
            });
            attend.store(true, Ordering::Relaxed);
            wake.notify_one();
        });
    }
}

impl Drop for Listing {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Ask who the daemon runs every little while, and each of them what
/// waits on you when it is time, or sooner when asked to.
async fn listing(
    url: String,
    token: Option<String>,
    up: mpsc::Sender<Update>,
    wake: Arc<tokio::sync::Notify>,
    attend: Arc<AtomicBool>,
) {
    let mut wait = RETRY;
    let mut asked: Option<Instant> = None;
    loop {
        match ghost_link::list(&url, token.clone()).await {
            Ok(roster) => {
                wait = RETRY;
                let _ = up.send(Update::Link(LinkState::Connected));
                let due = asked.is_none_or(|at| at.elapsed() >= ATTENTION);
                if attend.swap(false, Ordering::Relaxed) || due {
                    for g in &roster.ghosts {
                        match waiting(&url, token.clone(), &g.id).await {
                            Ok(items) => {
                                let _ = up.send(Update::Attention {
                                    ghost: g.id.clone(),
                                    items,
                                });
                            }
                            // A daemon from before attention was listed has
                            // no answer; the desk then shows nothing of hers.
                            Err(e) => debug!("what waits on {} at {url}: {e:#}", g.id),
                        }
                    }
                    asked = Some(Instant::now());
                }
                let _ = up.send(Update::Roster(roster.ghosts));
                tokio::select! {
                    () = tokio::time::sleep(REFRESH) => {}
                    () = wake.notified() => {}
                }
            }
            Err(e) => {
                let _ = up.send(Update::Link(LinkState::Lost(format!("{e:#}"))));
                back_off(&mut wait, &wake).await;
            }
        }
    }
}

/// What the character `ghost` is waiting on you for, asked on a short
/// connection of its own.
async fn waiting(
    url: &str,
    token: Option<String>,
    ghost: &str,
) -> anyhow::Result<Vec<ghost_link::protocol::Attention>> {
    let (mut console, _) = Console::connect(url, token, Some(ghost.to_owned())).await?;
    console.attention().await
}

/// The console role: connect, ask everything once, then follow her journal
/// as it is written, ask again now and then, and carry out what you do to
/// her. Reconnects when the daemon goes away, and reads what was written
/// meanwhile; `newest` is the newest entry handed to the desk so far.
async fn console(
    cfg: Config,
    mut newest: i64,
    up: mpsc::Sender<Update>,
    wake: Arc<tokio::sync::Notify>,
    mut orders: Orders,
    mut reads: Reads,
) {
    let mut wait = RETRY;
    loop {
        let _ = up.send(Update::Link(LinkState::Connecting));
        match session(&cfg, &mut newest, &up, &mut orders, &mut reads).await {
            Ok(()) => {
                let _ = up.send(Update::Link(LinkState::Lost(
                    "the daemon closed the connection".into(),
                )));
            }
            Err(e) => {
                let _ = up.send(Update::Link(LinkState::Lost(format!("{e:#}"))));
            }
        }
        back_off(&mut wait, &wake).await;
    }
}

async fn session(
    cfg: &Config,
    newest: &mut i64,
    up: &mpsc::Sender<Update>,
    orders: &mut Orders,
    reads: &mut Reads,
) -> anyhow::Result<()> {
    // One connection streams her journal; a second asks questions and
    // carries what you do, since a call on a subscribed connection would
    // drop entries that arrive while it waits for its answer. The second
    // says who is at the desk: what it says to her is theirs.
    let (mut asker, welcome) = Console::connect_as(
        &cfg.url,
        cfg.token.clone(),
        cfg.ghost.clone(),
        Some(cfg.who.clone()),
    )
    .await?;
    let (mut stream, _) = Console::connect(&cfg.url, cfg.token.clone(), cfg.ghost.clone()).await?;
    info!(
        "connected to {} as a console for {}",
        cfg.url, welcome.ghost.name
    );
    let _ = up.send(Update::Welcome(welcome.ghost));
    let _ = up.send(Update::Link(LinkState::Connected));

    // Subscribed before anything is read, so that an entry written while
    // the questions are answered is streamed rather than lost; one the
    // read brings as well is held once.
    stream.subscribe().await?;
    ask_all(&mut asker, up).await?;
    let stretch = since(&mut asker, *newest).await?;
    if let Some(last) = stretch.last() {
        *newest = last.seq.max(*newest);
    }
    let _ = up.send(Update::Journal(stretch));

    let mut refresh = tokio::time::interval(REFRESH);
    refresh.tick().await;
    loop {
        tokio::select! {
            entry = stream.next_entry() => {
                match entry? {
                    Some(v) => {
                        if let Some(entry) = Entry::from_value(&v) {
                            *newest = entry.seq.max(*newest);
                            let _ = up.send(Update::Entry(entry));
                        }
                    }
                    None => return Ok(()),
                }
            }
            Some(order) = orders.recv() => {
                match carry_out(&mut asker, &order).await {
                    Ok(_) if matches!(order, Order::Say { .. }) => {}
                    // What became of her work shows at once, not at the
                    // next refresh.
                    Ok(stopped) => {
                        if let Order::StopPlace { place } = order {
                            let _ = up.send(Update::Stopped { place, stopped });
                        }
                        ask_all(&mut asker, up).await?;
                    }

                    Err(e) => {
                        let why = format!("Could not {}: {e:#}", order.what());
                        let _ = up.send(Update::Refused { order, why });
                    }
                }
            }
            Some(read) = reads.recv() => read_steps(&mut asker, read, up).await,
            _ = refresh.tick() => {
                ask_all(&mut asker, up).await?;
            }
        }
    }
}

/// What the desk is missing of her journal: everything after `newest`, or
/// the tail when nothing is held yet or so much was written meanwhile that
/// the tail is what there is to show. Oldest first.
async fn since(asker: &mut Console, newest: i64) -> anyhow::Result<Vec<Entry>> {
    const MOST: u32 = TAIL * 4;
    if newest > 0 {
        let gap = sorted(&asker.journal(newest + 1, MOST).await?);
        if gap.len() < MOST as usize {
            return Ok(gap);
        }
    }
    Ok(sorted(&asker.journal(-i64::from(TAIL), TAIL).await?))
}

async fn ask_all(asker: &mut Console, up: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let status = asker.status().await?;
    let _ = up.send(Update::Status(Box::new(Status::from_value(&status))));
    // A daemon from before work could be listed has no answer; the desk
    // then shows none.
    if let Ok(work) = asker.work().await {
        let _ = up.send(Update::Work(work));
    }
    Ok(())
}
