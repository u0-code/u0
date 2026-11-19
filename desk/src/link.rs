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
//! keeps the threads apart, and her core keeps the list of them: the
//! solver's link asks for it (`sessions.list`) with everything else, again
//! whenever her journal says a session was begun, named or set aside, and
//! carries a new name or a deletion (`sessions.rename`, `sessions.delete`). What it reads of her journal is added to what
//! the desk holds, never put in its place: the tail once, then the stream,
//! and after a daemon was out of reach what was written meanwhile. A [`Listing`] asks a daemon who it runs, now
//! and then, and what each of them is waiting on you for, so that the desk
//! can show a daemon's cast and its Attention without opening any of them;
//! and it carries an answer given from Attention on a short connection of
//! its own. While a link to that daemon is up, the listing asks on the
//! link's own connection what it can (who the daemon runs, and what she
//! is waiting on you for) instead of opening one: an [`Asker`] is the way
//! to it.
//!
//! All of it runs on one small tokio runtime and hands what it learns to
//! the UI as [`Update`]s over a plain channel; the UI drains it on its own
//! clock. Dropping either stops it. Nothing here touches a ghost's files.
//!
//! The brains of the core the desk runs are asked about and changed with
//! [`brains`], and her files fetched by `media`, the same way: on the
//! link's connection while it is up, else on a short connection of their
//! own; what waits on a brain's provider always on one of its own. The
//! brains are the core's, and the desk reads none of its files for them.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use anyhow::anyhow;
use ghost_link::{
    Console,
    protocol::{BrainAsk, BrainList, BrainSave},
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
/// connection per character a minute, but for the one a link is up to.
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
    /// Whether to follow her sessions: the solver's are the desk's down
    /// the left; a character's are not shown.
    pub sessions: bool,
}

/// Something whoever sits at the desk does to her, as an operator of hers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    /// Say `text` in the place `place` on the console channel, by its id.
    Say { place: String, text: String },
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
    /// Call the session `place`, by its id, `title`.
    Rename { place: String, title: String },
    /// Set the session `place`, by its id, aside: off the list, with
    /// everything said in it still in her journal.
    SetAside { place: String },
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
            Self::Rename { .. } => "rename that session",
            Self::SetAside { .. } => "delete that session",
        }
    }

    /// The session it was about, by id, if it was about one's name or
    /// its place on the list.
    pub fn session(&self) -> Option<&str> {
        match self {
            Self::Rename { place, .. } | Self::SetAside { place } => Some(place),
            _ => None,
        }
    }
}

/// Carry out `order` on a console that said who is at it. Says whether
/// anything was stopped, for a stop in a place; otherwise that it was done.
async fn carry_out(console: &mut Console, order: &Order) -> anyhow::Result<bool> {
    match order {
        Order::Say { place, text } => {
            console.say(Some(place.clone()), text).await?;
        }
        Order::Answer { work, text } => console.answer(work, text).await?,
        Order::Stop { work } => console.stop(work).await?,
        Order::StopPlace { place } => return console.stop_place(Some(place.clone())).await,
        Order::Rewind { place, to } => {
            console.rewind_place(Some(place.clone()), *to).await?;
        }
        Order::Rename { place, title } => console.rename_session(place, title).await?,
        Order::SetAside { place } => console.delete_session(place).await?,
    }
    Ok(true)
}

type Orders = tokio::sync::mpsc::UnboundedReceiver<Order>;
type Reads = tokio::sync::mpsc::UnboundedReceiver<Read>;

/// A call made on a daemon's connection, borrowing it while it runs.
pub type Call<'c, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'c>>;

/// A call asked on a link's connection, with where its answer goes.
type Job = Box<dyn for<'c> FnOnce(&'c mut Console) -> Call<'c, ()> + Send>;
type Jobs = tokio::sync::mpsc::UnboundedReceiver<Job>;

/// The way to a link's own asking connection, to ask on it while it is up
/// rather than open another. Cloned freely, it outlives nothing: once the
/// link is down, or gone, it asks nothing.
#[derive(Clone)]
pub struct Asker {
    jobs: tokio::sync::mpsc::UnboundedSender<Job>,
    /// The seal of whom the link speaks for, while it is up.
    up: Arc<Mutex<Option<String>>>,
}

impl Asker {
    /// The seal of the character the link speaks for, while it is up.
    fn up(&self) -> Option<String> {
        self.up.lock().ok()?.clone()
    }

    /// Ask `call` on the link's connection, after what it is asking now.
    /// An error when the link is not up, or goes down first.
    async fn ask<T, F>(&self, call: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: for<'c> FnOnce(&'c mut Console) -> Call<'c, T> + Send + 'static,
    {
        let (answer, answered) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |console| {
            Box::pin(async move {
                let _ = answer.send(call(console).await);
                Ok(())
            })
        });
        if self.up().is_none() || self.jobs.send(job).is_err() {
            return Err(anyhow!("the link to the daemon is not up"));
        }
        answered
            .await
            .unwrap_or_else(|_| Err(anyhow!("the daemon closed the connection")))
    }
}

/// Ask `call` of the character `ghost` on the daemon at `url`: on the
/// connection of `via`'s link while it is up, else on a short connection
/// of its own.
pub async fn ask<T, F>(
    via: Option<&Asker>,
    url: &str,
    token: Option<String>,
    ghost: Option<String>,
    call: F,
) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: for<'c> FnOnce(&'c mut Console) -> Call<'c, T> + Send + 'static,
{
    if let Some(via) = via.filter(|a| a.up().is_some()) {
        return via.ask(call).await;
    }
    let (mut console, _) = Console::connect(url, token, ghost).await?;
    call(&mut console).await
}

/// Ask what `read` asks, and hand the answer, or why there is none, to the
/// desk. A refusal is said, and the connection goes on.
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
            Ok(entries) => Update::Place(sorted(entries)),
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
    Models(Vec<ghost_link::protocol::BrainModel>),
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

/// Ask the core at `url` about its brains: on the connection of `via`'s
/// link to it while that is up, else on a connection of its own. What
/// waits on a provider, its models or a test, always has one of its own:
/// on the link's it would hold her journal and what you say to her back
/// until the provider answered.
pub async fn brains(
    via: Asker,
    url: String,
    token: Option<String>,
    call: BrainCall,
) -> Result<BrainAnswer, Refused> {
    let provider = matches!(call, BrainCall::Models(_) | BrainCall::Check(_));
    let via = (!provider).then_some(&via);
    let asked = ask(via, &url, token, None, move |console| {
        Box::pin(async move {
            Ok(match call {
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
        })
    });
    asked.await.map_err(|e| Refused::from(&e))
}

/// Entries as the daemon serves them, oldest first.
fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
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
    /// The way to ask on its connection.
    asker: Asker,
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
        let (jobs, to_do) = tokio::sync::mpsc::unbounded_channel();
        let asker = Asker {
            jobs,
            up: Arc::default(),
        };
        let wake = Arc::new(tokio::sync::Notify::new());
        let channels = Channels {
            orders: taken,
            reads: to_read,
            jobs: to_do,
            up: asker.up.clone(),
        };
        let task = runtime().spawn(console(cfg, newest, up, wake.clone(), channels));
        Self {
            rx,
            orders,
            reads,
            asker,
            task,
            wake,
        }
    }

    /// The way to ask on its connection while it is up.
    pub fn asker(&self) -> Asker {
        self.asker.clone()
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
        if let Ok(mut up) = self.asker.up.lock() {
            *up = None;
        }
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
    /// what is waiting on you. `asker` is a link's to that daemon, where
    /// the desk has one, to ask on while it is up.
    pub fn follow(url: String, token: Option<String>, asker: Option<Asker>) -> Self {
        let (up, rx) = mpsc::channel();
        let wake = Arc::new(tokio::sync::Notify::new());
        let attend = Arc::new(AtomicBool::new(true));
        let task = runtime().spawn(listing(
            url.clone(),
            token.clone(),
            asker,
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
    via: Option<Asker>,
    up: mpsc::Sender<Update>,
    wake: Arc<tokio::sync::Notify>,
    attend: Arc<AtomicBool>,
) {
    let mut wait = RETRY;
    let mut asked: Option<Instant> = None;
    loop {
        let listed = match via.as_ref().filter(|a| a.up().is_some()) {
            Some(via) => via.ask(|console| Box::pin(console.list())).await,
            None => ghost_link::list(&url, token.clone()).await,
        };
        match listed {
            Ok(roster) => {
                wait = RETRY;
                let _ = up.send(Update::Link(LinkState::Connected));
                let due = asked.is_none_or(|at| at.elapsed() >= ATTENTION);
                if attend.swap(false, Ordering::Relaxed) || due {
                    for g in &roster.ghosts {
                        // Hers on her link's connection, if it is hers.
                        let on = via
                            .as_ref()
                            .filter(|a| a.up().as_deref() == Some(g.card.seal.as_str()));
                        let ghost = Some(g.id.clone());
                        let items = ask(on, &url, token.clone(), ghost, |console| {
                            Box::pin(console.attention())
                        });
                        match items.await {
                            Ok(items) => {
                                let _ = up.send(Update::Attention {
                                    ghost: g.id.clone(),
                                    items,
                                });
                            }
                            // Without an answer the desk shows nothing of
                            // hers, and asks again next time.
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

/// What a link's task is handed from the desk, and where it says whom its
/// connection speaks for while it is up.
struct Channels {
    orders: Orders,
    reads: Reads,
    jobs: Jobs,
    up: Arc<Mutex<Option<String>>>,
}

impl Channels {
    /// Say whom the connection speaks for, or that it is down; either way
    /// what was asked of the one before is dropped, and its askers told.
    fn speak_for(&mut self, seal: Option<String>) {
        if let Ok(mut up) = self.up.lock() {
            *up = seal;
        }
        while self.jobs.try_recv().is_ok() {}
    }
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
    mut channels: Channels,
) {
    let mut wait = RETRY;
    loop {
        let _ = up.send(Update::Link(LinkState::Connecting));
        let ended = session(&cfg, &mut newest, &up, &mut channels).await;
        channels.speak_for(None);
        match ended {
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
    channels: &mut Channels,
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
    channels.speak_for(Some(welcome.ghost.seal.clone()));
    let _ = up.send(Update::Welcome(welcome.ghost));
    let _ = up.send(Update::Link(LinkState::Connected));

    // Subscribed before anything is read, so that an entry written while
    // the questions are answered is streamed rather than lost; one the
    // read brings as well is held once.
    stream.subscribe().await?;
    ask_all(&mut asker, up).await?;
    if cfg.sessions {
        ask_sessions(&mut asker, up).await;
    }
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
                    Some(entry) => {
                        *newest = entry.seq.max(*newest);
                        let listed = cfg.sessions && changes_sessions(&entry);
                        let worked = changes_work(&entry);
                        let _ = up.send(Update::Entry(entry));
                        // Begun, named or set aside, by this desk or
                        // another: what the list is now.
                        if listed {
                            ask_sessions(&mut asker, up).await;
                        }
                        // Where her work stands is her core's to say: asked
                        // as soon as her journal says it moved, so that a
                        // card shows it with the entry, not at the next
                        // refresh.
                        if worked {
                            ask_work(&mut asker, up).await;
                        }
                    }
                    None => return Ok(()),
                }
            }
            Some(order) = channels.orders.recv() => {
                match carry_out(&mut asker, &order).await {
                    Ok(_) if matches!(order, Order::Say { .. }) => {}
                    // What became of her work shows at once, not at the
                    // next refresh.
                    Ok(stopped) => {
                        let listed = cfg.sessions && order.session().is_some();
                        if let Order::StopPlace { place } = order {
                            let _ = up.send(Update::Stopped { place, stopped });
                        }
                        ask_all(&mut asker, up).await?;
                        if listed {
                            ask_sessions(&mut asker, up).await;
                        }
                    }

                    Err(e) => {
                        let why = format!("Could not {}: {e:#}", order.what());
                        let _ = up.send(Update::Refused { order, why });
                    }
                }
            }
            Some(read) = channels.reads.recv() => read_steps(&mut asker, read, up).await,
            // What is asked on this connection rather than one of its own.
            Some(job) = channels.jobs.recv() => {
                let _ = job(&mut asker).await;
            }
            _ = refresh.tick() => {
                ask_all(&mut asker, up).await?;
                if cfg.sessions {
                    ask_sessions(&mut asker, up).await;
                }
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
        let gap = sorted(asker.journal(newest + 1, MOST).await?);
        if gap.len() < MOST as usize {
            return Ok(gap);
        }
    }
    Ok(sorted(asker.journal(-i64::from(TAIL), TAIL).await?))
}

/// Whether `entry` may change the list of her sessions: one begun by its
/// first line, named or set aside.
fn changes_sessions(entry: &Entry) -> bool {
    match entry.kind.as_str() {
        "session_named" | "session_set_aside" => true,
        "heard" => entry.data["channel"] == ghost_link::protocol::CONSOLE,
        _ => false,
    }
}

/// Whether `entry` may move a piece of her work: taken on, asking,
/// answered or over.
fn changes_work(entry: &Entry) -> bool {
    matches!(
        entry.kind.as_str(),
        "undertook" | "asked" | "added" | "concluded"
    )
}

/// Her work, as her core lists it. Without an answer the desk shows what
/// it had, and the rest of her status stands.
async fn ask_work(asker: &mut Console, up: &mpsc::Sender<Update>) {
    if let Ok(work) = asker.work().await {
        let _ = up.send(Update::Work(work));
    }
}

/// Her sessions, as her core lists them. Without an answer the desk keeps
/// what it has.
async fn ask_sessions(asker: &mut Console, up: &mpsc::Sender<Update>) {
    match asker.sessions().await {
        Ok(sessions) => {
            let _ = up.send(Update::Sessions(sessions));
        }
        Err(e) => debug!("her core lists no sessions: {e:#}"),
    }
}

async fn ask_all(asker: &mut Console, up: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let status = asker.status().await?;
    let _ = up.send(Update::Status(Box::new(Status::read(status))));
    ask_work(asker, up).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_asked_of_a_link_that_goes_down_is_refused_rather_than_kept() {
        let (jobs, to_do) = tokio::sync::mpsc::unbounded_channel();
        let up = Arc::new(Mutex::new(Some("seal".to_owned())));
        let asker = Asker {
            jobs,
            up: up.clone(),
        };
        let mut channels = Channels {
            orders: tokio::sync::mpsc::unbounded_channel().1,
            reads: tokio::sync::mpsc::unbounded_channel().1,
            jobs: to_do,
            up,
        };
        let waiting = asker.clone();
        let pending = runtime().spawn(async move {
            waiting
                .ask(|console| Box::pin(console.list()))
                .await
                .is_err()
        });
        while channels.jobs.is_empty() {
            std::thread::yield_now();
        }
        // The link goes down with it asked and not yet answered.
        channels.speak_for(None);
        assert!(
            runtime().block_on(pending).unwrap(),
            "told, not kept waiting"
        );
        // Down, it takes nothing more: whoever asks opens a connection.
        let later = runtime().block_on(asker.ask(|console| Box::pin(console.list())));
        assert!(later.is_err());
        assert!(channels.jobs.is_empty());
    }
}
