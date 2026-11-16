//! `ghostd`: runs ghosts.
//!
//! `run` is the daemon: it opens each character's directory, starts her
//! mind, and listens for presences and consoles, each of which is for one
//! of them. The other subcommands either read one directory without
//! disturbing a running daemon (`status`, `journal`, `verify`, `prompts`)
//! or connect to one character as a console (`watch`, `agenda`, `people`).

mod cast;
mod config;
mod server;
mod show;
mod wire;

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, bail};
use chrono::Utc;
use clap::{Parser, Subcommand};
use ghost_brain::Completion;
use ghost_core::{
    Ghost,
    journal::Entry,
    life::{Life as Day, Phase},
    memory::Memory,
    prompts::{
        self as words, Store,
        exported::{self, Updated},
    },
};
use ghost_link::Console;
use ghost_mind::{Mind, Numb, OpenMeteo, Parts, Prompts, Senses, prompts::Origin};
use ghost_protocol::GhostCard;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use tracing::{Instrument as _, info, info_span, warn};

use crate::{
    cast::{Cast, Character, Life, Maker, Pauses},
    config::{Config, Hers},
    server::{Presences, Shared},
};

#[derive(Parser)]
#[command(name = "ghostd", version, about = "Runs ghosts.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Give birth to the ghost described by DIR/ghost.toml. Happens once.
    Birth { dir: PathBuf },
    /// Run them: for each, verify her journal, record any revision of
    /// ghost.toml and start her mind; then listen for presences.
    Run {
        /// Ghost directories, or directories of them: every directory in one
        /// that has a ghost.toml, except `_template`. Each character is
        /// known by her directory's name.
        #[arg(required = true, num_args = 1..)]
        dirs: Vec<PathBuf>,
        /// Path to ghostd.toml.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Live only as long as whoever started this: read standard input,
        /// and stop in order when it closes. The desktop app starts its own
        /// ghost this way, so she never outlives it, not even when the app
        /// is killed rather than quit: the system closes the pipe for it.
        #[arg(long)]
        with_parent: bool,
    },
    /// Who she is and where her clock is. Reads the directory.
    Status { dir: PathBuf },
    /// Print the tail of her journal. Reads the directory.
    Journal {
        dir: PathBuf,
        #[arg(long, default_value_t = 50)]
        last: usize,
    },
    /// Verify her journal: that it begins with her birth and its whole hash
    /// chain holds.
    Verify { dir: PathBuf },
    /// List configured brains; with --probe, ask each for one word; with
    /// --discover, look for what this machine could think with instead.
    Brains {
        #[arg(long)]
        probe: bool,
        #[arg(long, conflicts_with = "probe")]
        discover: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Watch a running ghost's journal as it is written.
    Watch {
        #[command(flatten)]
        daemon: Daemon,
        /// Also print this many recent entries first.
        #[arg(long, default_value_t = 0)]
        last: u32,
    },
    /// What a running ghost is going to do without being asked, and when.
    Agenda {
        #[command(flatten)]
        daemon: Daemon,
    },
    /// Everyone a running ghost knows, and where she stands with them.
    People {
        #[command(flatten)]
        daemon: Daemon,
    },
    /// The words a brain reads, shared by every character this deployment
    /// runs: where each comes from, and a copy of the built-in ones to
    /// change in the deployment's directory of prompts.
    Prompts {
        #[command(subcommand)]
        cmd: PromptsCmd,
    },
}

/// Each prompts command works on the deployment's directory of prompts:
/// DIR if it is given, or else `[mind] prompts` in ghostd.toml.
#[derive(Subcommand)]
enum PromptsCmd {
    /// Every template, text and tool description, by its file in the tree
    /// and its layer, with where what is in use comes from (the
    /// deployment's directory, or built in) and whether it differs from the
    /// built-in.
    List {
        #[command(flatten)]
        at: PromptsDir,
    },
    /// Write the built-in prompts into the deployment's directory, each at
    /// its place in the tree, to be changed there, with a note of what each
    /// was copied from.
    Export {
        #[command(flatten)]
        at: PromptsDir,
        /// Overwrite files that are already there.
        #[arg(long)]
        force: bool,
        /// Only these files, or directories of the tree, such as
        /// `character/persona.md system/logic/texts`: the ones you mean to
        /// change. A copy you leave as it is still hides the built-in after
        /// an upgrade.
        #[arg(long, num_args = 1.., value_name = "FILE")]
        only: Vec<String>,
    },
    /// Bring the copies in the deployment's directory that nobody edited up
    /// to this build's built-ins. Edited copies are left as they are; one
    /// that has been brought up to date by hand is named with --looked-at,
    /// so that it is no longer warned about.
    Update {
        #[command(flatten)]
        at: PromptsDir,
        /// Copies you have compared with this build's built-in and kept, by
        /// their path in the tree.
        #[arg(long, num_args = 1.., value_name = "FILE")]
        looked_at: Vec<String>,
    },
}

/// Where the deployment's prompts are.
#[derive(clap::Args)]
struct PromptsDir {
    /// The directory of prompts. Without it, `[mind] prompts` in ghostd.toml.
    dir: Option<PathBuf>,
    /// Path to ghostd.toml, for its `[mind] prompts`.
    #[arg(long)]
    config: Option<PathBuf>,
}

impl PromptsDir {
    /// The directory, if one is given or configured.
    fn found(&self) -> anyhow::Result<Option<PathBuf>> {
        if let Some(dir) = &self.dir {
            return Ok(Some(dir.clone()));
        }
        if !Config::exists(self.config.as_deref()) {
            return Ok(None);
        }
        Config::load(self.config.as_deref())?.0.prompts()
    }

    /// The directory, which has to be given or configured.
    fn needed(&self) -> anyhow::Result<PathBuf> {
        self.found()?.context(
            "no directory of prompts: name one, or set `prompts` under [mind] in ghostd.toml",
        )
    }
}

/// How to reach a character in a running daemon.
#[derive(clap::Args)]
struct Daemon {
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// Which character: the name of her directory. Needed when the daemon
    /// runs more than one.
    #[arg(long, env = "GHOST_NAME")]
    ghost: Option<String>,
}

impl Daemon {
    async fn console(self) -> anyhow::Result<Console> {
        let (console, _) = Console::connect(&self.url, self.token, self.ghost).await?;
        Ok(console)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Keys and settings may sit in an ignored `.env` file in the directory
    // this runs from or one above it. Anything already set in the
    // environment wins.
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().cmd {
        Cmd::Birth { dir } => birth(&dir),
        Cmd::Run {
            dirs,
            config,
            with_parent,
        } => run(&dirs, config.as_deref(), with_parent).await,
        Cmd::Status { dir } => status(&dir),
        Cmd::Journal { dir, last } => journal(&dir, last),
        Cmd::Verify { dir } => verify(&dir),
        Cmd::Brains { discover: true, .. } => discover().await,
        Cmd::Brains { probe, config, .. } => brains(probe, config.as_deref()).await,
        Cmd::Watch { daemon, last } => watch(daemon, last).await,
        Cmd::Agenda { daemon } => {
            let agenda = daemon.console().await?.agenda().await?;
            println!("{}", serde_json::to_string_pretty(&agenda)?);
            Ok(())
        }
        Cmd::People { daemon } => {
            show::people(&daemon.console().await?.people().await?);
            Ok(())
        }
        Cmd::Prompts {
            cmd: PromptsCmd::List { at },
        } => {
            list_prompts(at.found()?.as_deref());
            Ok(())
        }
        Cmd::Prompts {
            cmd: PromptsCmd::Export { at, force, only },
        } => export_prompts(&at.needed()?, force, &only),
        Cmd::Prompts {
            cmd: PromptsCmd::Update { at, looked_at },
        } => update_prompts(&at.needed()?, &looked_at),
    }
}

fn birth(dir: &Path) -> anyhow::Result<()> {
    let ghost = Ghost::birth(dir).with_context(|| format!("birth in {}", dir.display()))?;
    let id = ghost.identity();
    println!(
        "{} is born in {}, {}. seal {}",
        id.name(),
        id.seed().home.city,
        id.seed().home.country,
        id.seal().short()
    );
    Ok(())
}

async fn run(paths: &[PathBuf], config: Option<&Path>, with_parent: bool) -> anyhow::Result<()> {
    let loaded = if Config::exists(config) {
        Some(Config::load(config)?)
    } else {
        None
    };
    // Before anything makes a request, looking for brains included.
    network(
        &loaded
            .as_ref()
            .map(|(cfg, _)| cfg.network.clone())
            .unwrap_or_default(),
    )?;
    let (cfg, cfg_path) = if let Some(loaded) = loaded {
        loaded
    } else {
        // Nothing was written down for this machine. Use what is on it.
        let (cfg, what) = Config::from_found(&ghost_brain::discover::discover().await)?;
        info!("no ghostd.toml: {what}");
        (cfg, PathBuf::from("(none)"))
    };
    let token = cfg.listen.token()?;
    info!("config {}", cfg_path.display());
    let found = cast::find(paths)?;
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    cfg.check_ids(&ids)?;
    let hers: Vec<Hers> = ids
        .iter()
        .map(|id| cfg.hers(id))
        .collect::<anyhow::Result<_>>()?;
    no_shared_machine(&ids, &hers)?;

    // The prompts are the deployment's, read by every character's mind for
    // itself; what there is to say about them is said once.
    let deployment = cfg.prompts()?;
    if let Some(said) = deployment.as_deref().and_then(retired) {
        warn!("{said}");
    }
    let prompts = Prompts::load(deployment.as_deref());
    say_overrides(&prompts);
    say_stale(&prompts);

    // Each is made in full before anyone is listened to: one that cannot
    // be started stops the daemon here, as a single ghost always has.
    let cfg = Arc::new(cfg);
    let mut cast = Vec::new();
    let mut lives = Vec::new();
    for ((id, dir), hers) in found.into_iter().zip(hers) {
        let span = info_span!("ghost", id = %id);
        let hers = Arc::new(hers);
        let body = Arc::new(Presences::default());
        let (life, card) =
            span.in_scope(|| begin(&cfg, &hers, &dir, deployment.as_deref(), &body))?;
        let make: Maker = {
            let (cfg, hers, dir, deployment, body) = (
                cfg.clone(),
                hers.clone(),
                dir.clone(),
                deployment.clone(),
                body.clone(),
            );
            Box::new(move || {
                begin(&cfg, &hers, &dir, deployment.as_deref(), &body).map(|(life, _)| life)
            })
        };
        let who = Arc::new(Character::new(id, dir, card, body, life.handle.clone()));
        cast.push(who.clone());
        lives.push((who, life, make, span));
    }

    // Listened for before anything is said about listening, so that a
    // SIGTERM sent the moment she is up still stops her in order.
    let told_to_stop = told_to_stop(with_parent);
    let listener = TcpListener::bind(&cfg.listen.addr)
        .await
        .with_context(|| format!("listen on {}", cfg.listen.addr))?;
    info!(
        "listening on ws://{}{}",
        cfg.listen.addr,
        if token.is_some() {
            " (token required)"
        } else {
            ""
        }
    );
    let server = tokio::spawn(server::serve(
        listener,
        Arc::new(Shared {
            cast: Cast(cast),
            token,
            origins: cfg.listen.origins.clone(),
        }),
    ));

    let (stop, stopping) = watch::channel(false);
    let mut keepers = JoinSet::new();
    for (who, life, make, span) in lives {
        let keeping = cast::keep(who, life, make, stopping.clone(), Pauses::default());
        keepers.spawn(keeping.instrument(span));
    }
    tokio::select! {
        why = told_to_stop => {
            info!("{why}: stopping");
            stop.send_replace(true);
        }
        // Every mind stopped of its own accord: there is nobody to serve.
        () = async { while keepers.join_next().await.is_some() {} } => {}
    }
    while keepers.join_next().await.is_some() {}
    server.abort();
    Ok(())
}

/// Open her directory and make her mind from it, living as `hers` says,
/// with `body` as her body. Says what she runs on. Done once at start and
/// again each time her mind is started after it failed, so that all that
/// carries over is what is on disk.
fn begin(
    cfg: &Config,
    hers: &Hers,
    dir: &Path,
    deployment: Option<&Path>,
    body: &Arc<Presences>,
) -> anyhow::Result<(Life, GhostCard)> {
    let ghost = Ghost::open(dir).with_context(|| format!("open {}", dir.display()))?;
    let seed = ghost.identity().seed();
    let card = GhostCard {
        name: seed.name.clone(),
        pronouns: seed.pronouns.clone(),
        city: seed.home.city.clone(),
        country: seed.home.country.clone(),
        seal: ghost.identity().seal().to_string(),
        bio: seed.bio.clone(),
    };
    let (think, tick) = (cfg.think()?, cfg.tick()?);
    let mind = hers.mind();
    info!(
        "{} is in {}. seal {}. {}",
        card.name,
        card.city,
        ghost.identity().seal().short(),
        dir.display()
    );
    info!(
        "think on {}, tick on {}, tempo {}, at most {} calls an hour",
        think.label(),
        tick.label(),
        mind.tempo,
        mind.calls_per_hour
    );
    let senses: Arc<dyn Senses> = if mind.weather {
        Arc::new(OpenMeteo::new())
    } else {
        Arc::new(Numb)
    };
    let workshop = workshop(hers, dir)?;
    if mind.work.enabled {
        info!(
            "work: {}, {}",
            mind.work.machine.as_deref().unwrap_or("no machine"),
            if workshop.web.search.is_some() {
                "can search the web"
            } else {
                "cannot search the web"
            }
        );
    }
    if !mind.operators.accounts.is_empty() {
        info!(
            "directives begin with {:?}, from operators on {}",
            mind.operators.prefix,
            mind.operators
                .accounts
                .iter()
                .map(|(channel, ids)| format!("{channel} ({})", ids.len()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let (mind, handle) = Mind::new(Parts {
        ghost,
        think,
        tick,
        body: body.clone(),
        senses,
        prompts: prompts_for(dir, deployment),
        config: mind,
        workshop,
    });
    Ok((
        Life {
            run: Box::pin(mind.run()),
            handle,
        },
        card,
    ))
}

/// Refuse two characters whose machine is the same directory of the same
/// container: each one's work would be in the other's way. A `local:`
/// machine is always under her own directory, and so always hers.
fn no_shared_machine(ids: &[&str], hers: &[Hers]) -> anyhow::Result<()> {
    let mut seen: Vec<(&str, Option<&str>, &str)> = Vec::new();
    for (id, hers) in ids.iter().zip(hers) {
        let work = &hers.mind.work;
        let Some(("docker", container)) = work
            .machine
            .as_deref()
            .and_then(|m| m.trim().split_once(':'))
        else {
            continue;
        };
        let workdir = work.workdir.as_deref();
        if let Some((_, _, other)) = seen
            .iter()
            .find(|(c, w, _)| *c == container && *w == workdir)
        {
            bail!(
                "{other} and {id} would work in the same directory of the container {container}; \
                 give one of them a workdir of her own under [ghosts.{id}.mind.work]"
            );
        }
        seen.push((container, workdir, id));
    }
    Ok(())
}

/// Make `[network]`, under whatever the environment sets, the way every
/// request from this process and the programs it starts leaves the machine.
fn network(setting: &ghost_net::Setting) -> anyhow::Result<()> {
    let network = ghost_net::Network::from_env(setting).context("[network]")?;
    info!("network: {network}");
    if !ghost_net::install(network) {
        warn!("the network setting came after the first request, which went the default way");
    }
    Ok(())
}

/// Resolves when she should stop: Ctrl-C at a terminal, or SIGTERM, which
/// is how systemd, docker and a shutting-down machine ask, or, `with_parent`,
/// standard input closing because whoever started her is gone. Every way
/// she is stopped in order, so that nothing is cut off halfway through
/// being written. Says which it was.
fn told_to_stop(with_parent: bool) -> impl std::future::Future<Output = &'static str> {
    let orphaned = with_parent.then(parent_gone);
    // The handlers are set up here, when this is called, not when the
    // future is first polled: a signal that comes in between is kept.
    #[cfg(unix)]
    let term = {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate())
            .inspect_err(|e| {
                warn!("cannot listen for SIGTERM ({e}); only Ctrl-C stops her in order");
            })
            .ok()
    };
    let ctrl_c = tokio::signal::ctrl_c();
    async move {
        let orphaned = async {
            match orphaned {
                Some(gone) => {
                    let _ = gone.await;
                }
                None => std::future::pending().await,
            }
        };
        #[cfg(unix)]
        if let Some(mut term) = term {
            return tokio::select! {
                _ = ctrl_c => "interrupted",
                _ = term.recv() => "terminated",
                () = orphaned => "the program that started her is gone",
            };
        }
        tokio::select! {
            _ = ctrl_c => "interrupted",
            () = orphaned => "the program that started her is gone",
        }
    }
}

/// Resolves once standard input has closed: read on a thread of its own,
/// since a read that waits for ever must not hold up anything else.
fn parent_gone() -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 256];
        while matches!(stdin.read(&mut buf), Ok(n) if n > 0) {}
        let _ = tx.send(());
    });
    rx
}

fn status(dir: &Path) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    let id = &peek.identity;
    let life = Day::new(id);
    let now = Utc::now();
    let local = now.with_timezone(&life.clock().tz());
    let moment = life.moment(&peek.journal, now)?;
    println!("name      {} ({})", id.name(), id.seed().pronouns);
    println!(
        "home      {}, {} [{}]",
        id.seed().home.city,
        id.seed().home.country,
        id.seed().home.timezone
    );
    println!("born      {}", id.seed().born);
    println!(
        "seal      {}{}",
        id.seal(),
        if peek.revised {
            " (ghost.toml revised; recorded and in effect at the next run)"
        } else {
            ""
        }
    );
    println!("local     {}", local.format("%Y-%m-%d %H:%M %Z"));
    println!(
        "now       {}",
        match (&moment.phase, &moment.doing) {
            (Phase::Asleep, _) => "asleep".to_owned(),
            (Phase::Awake, Some(doing)) =>
                format!("{} ({:?})", doing.what, doing.reach).to_lowercase(),
            (Phase::Awake, None) => "awake, nothing planned right now".to_owned(),
        }
    );
    println!("journal   {} entries", peek.journal.len()?);
    if let Some(last) = peek.journal.last()? {
        println!("last      {}", show::entry(&last, life.clock().tz()));
    }
    // What a running body keeps on disk, read without disturbing it. What
    // only the live process knows, such as which sitting is running this
    // second, is not here; the time of each piece of work's last step is
    // the nearest thing.
    let Some(memory) = Memory::glance(dir)? else {
        println!("memory    not built yet, or by another version; it is built when she runs");
        return Ok(());
    };
    let unread = memory.places_unread()?;
    if unread.is_empty() {
        println!("unread    nothing");
    }
    for (i, place) in unread.iter().enumerate() {
        let mut detail = format!("{} unread", place.unread);
        if place.unread_addressed > 0 {
            let _ = write!(detail, ", {} to her", place.unread_addressed);
        }
        if place.unread_directed > 0 {
            let _ = write!(detail, ", {} from an operator", place.unread_directed);
        }
        let name = place
            .name
            .as_ref()
            .map_or_else(|| place.key.clone(), |n| format!("{n} ({})", place.key));
        println!("{:<9} {name}: {detail}", if i == 0 { "unread" } else { "" });
    }
    let open = memory.undertakings(true, 50)?;
    let db = dir.join(ghost_work::WORK_DB);
    let seen = if db.exists() {
        ghost_work::undertaking::glance(&db).with_context(|| format!("read {}", db.display()))?
    } else {
        Vec::new()
    };
    println!("work      {} open", open.len());
    for work in &open {
        let brief: String = work.brief.split_whitespace().collect::<Vec<_>>().join(" ");
        let brief: String = brief.chars().take(70).collect();
        println!("          #{} {brief}", work.seq);
        let mut detail = format!("{} sittings", work.sittings);
        if let Some(seen) = seen.iter().find(|s| s.id == work.id) {
            let _ = write!(detail, ", {} steps", seen.steps);
            if let Some(at) = seen.last_step {
                let at = at.with_timezone(&life.clock().tz());
                let _ = write!(detail, ", last step {}", at.format("%m-%d %H:%M"));
            }
            if seen.parts_open > 0 {
                let _ = write!(detail, ", {} parts unfinished", seen.parts_open);
            }
        }
        if let Some(question) = &work.waiting {
            let question: String = question.chars().take(60).collect();
            let _ = write!(detail, ", waiting to hear: {question}");
        }
        println!("            {detail}");
    }
    Ok(())
}

fn journal(dir: &Path, last: usize) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    let tz = peek.identity.tz();
    for entry in peek.journal.tail(last)? {
        println!("{}", show::entry(&entry, tz));
    }
    Ok(())
}

fn verify(dir: &Path) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    println!("chain ok, {} entries", peek.journal.len()?);
    Ok(())
}

async fn watch(daemon: Daemon, last: u32) -> anyhow::Result<()> {
    let (mut console, welcome) = Console::connect(&daemon.url, daemon.token, daemon.ghost).await?;
    let status = console.status().await?;
    let tz: chrono_tz::Tz = status["timezone"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    eprintln!(
        "watching {} in {} ({})",
        welcome.ghost.name,
        welcome.ghost.city,
        status["local_time"].as_str().unwrap_or_default()
    );
    let print = |value: serde_json::Value| match serde_json::from_value::<Entry>(value) {
        Ok(entry) => println!("{}", show::entry(&entry, tz)),
        Err(e) => eprintln!("(an entry this build cannot read: {e})"),
    };
    if last > 0 {
        for value in console.journal(1 - i64::from(last), last).await? {
            print(value);
        }
    }
    console.subscribe().await?;
    while let Some(value) = console.next_entry().await? {
        print(value);
    }
    eprintln!("the daemon went away");
    Ok(())
}

/// The prompts for the ghost in `ghost_dir`, from the deployment's
/// directory `deployment` if there is one: her own copy, which her mind
/// takes changes into by itself. Prompts in her own directory are warned
/// about and not used.
fn prompts_for(ghost_dir: &Path, deployment: Option<&Path>) -> Prompts {
    if let Some(said) = misplaced(ghost_dir, deployment) {
        warn!("{said}");
    }
    Prompts::load(deployment)
}

/// What there is to say about prompts in the ghost directory `ghost_dir`,
/// if it has any. They are not read: how she works is shared by every
/// character, and only her persona is hers.
fn misplaced(ghost_dir: &Path, deployment: Option<&Path>) -> Option<String> {
    let mut found = words::misplaced(&ghost_mind::prompts::all_files(), ghost_dir);
    found.extend(
        retired(&ghost_dir.join(words::DIR)).map(|_| ghost_dir.join(words::DIR).join(RETIRED)),
    );
    if found.is_empty() {
        return None;
    }
    let hers = ghost_dir.join(words::DIR);
    let names: Vec<String> = found
        .iter()
        .filter_map(|p| p.strip_prefix(&hers).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let to = deployment.map_or_else(
        || "a directory of prompts named by `prompts` under [mind] in ghostd.toml".to_owned(),
        |dir| dir.display().to_string(),
    );
    Some(format!(
        "{} holds {}, which are not used: how she works is shared by every character this deployment runs, \
         and what is hers alone is in her ghost.toml. Move them to their place in the tree in {to} to use them for everyone",
        ghost_dir.join(words::DIR).display(),
        names.join(", ")
    ))
}

/// The prompt that was split into the persona template and her being.
const RETIRED: &str = "self.md";

/// What there is to say about a copy of `self.md` in `dir`, if there is
/// one: nothing reads it any more.
fn retired(dir: &Path) -> Option<String> {
    let path = dir.join(RETIRED);
    path.exists().then(|| {
        format!(
            "{} is no longer read: it is now character/persona.md, her own fields arranged, and system/being.md, \
             what she is and the ground rules. `ghostd prompts export --only character/persona.md system/being.md` \
             writes this build's, to carry your changes into",
            path.display()
        )
    })
}

/// Say which prompts are not the built-in ones, and where they come from.
fn say_overrides(prompts: &Prompts) {
    let mut from: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for listed in prompts.listing() {
        let Some(path) = listed.path else { continue };
        let what = listed.key.unwrap_or_else(|| listed.file.to_owned());
        match from.iter_mut().find(|(p, _)| *p == path) {
            Some((_, names)) => names.push(what),
            None => from.push((path, vec![what])),
        }
    }
    if from.is_empty() {
        info!("prompts: all built in");
    }
    for (path, names) in from {
        info!("prompts from {}: {}", path.display(), names.join(", "));
    }
}

/// Warn about each copy of a prompt that may hide something newer in this
/// build, and say once per directory what to do about it.
fn say_stale(prompts: &Prompts) {
    let files = ghost_mind::prompts::all_files();
    for (_, dir) in prompts.store().layers() {
        let stale = exported::stale(&files, dir);
        if stale.is_empty() {
            continue;
        }
        for one in &stale {
            let what = match &one.key {
                Some(key) => format!("{key} in {}", one.file),
                None => one.file.to_owned(),
            };
            let lacks = if one.missing.is_empty() {
                String::new()
            } else {
                let names: Vec<String> =
                    one.missing.iter().map(|m| format!("{{{{{m}}}}}")).collect();
                format!(
                    "; it lacks {}, which the built-in fills in, so that is left out of what her brain reads",
                    names.join(", ")
                )
            };
            if one.changed {
                warn!(
                    "{}: the built-in {what} has changed since this copy was exported, and the copy hides the newer one{lacks}",
                    one.path.display()
                );
            } else {
                warn!(
                    "{}: there is no record of when this copy of {what} was exported{lacks}",
                    one.path.display()
                );
            }
        }
        let mut names: Vec<&str> = stale.iter().map(|s| s.file).collect();
        names.dedup();
        let names = names.join(" ");
        warn!(
            "if you never edited them, `ghostd prompts update {0}` brings them up to date. \
             If you did, write this build's next to them with `ghostd prompts export <somewhere> --only {names}`, \
             carry over what you want, then `ghostd prompts update {0} --looked-at {names}`",
            dir.display()
        );
    }
}

/// `ghostd prompts list`.
fn list_prompts(dir: Option<&Path>) {
    if let Some(said) = dir.and_then(retired) {
        warn!("{said}");
    }
    for line in listing(&Prompts::load(dir)) {
        println!("{line}");
    }
}

/// A line for each template, text and tool: its layer, its file in the
/// tree and its key there, whether it is not the built-in or hides a newer
/// one, and where it comes from.
fn listing(prompts: &Prompts) -> Vec<String> {
    // What hides something newer in this build, by where it is.
    let files = ghost_mind::prompts::all_files();
    let behind: Vec<(PathBuf, Option<String>)> = prompts
        .store()
        .layers()
        .iter()
        .flat_map(|(_, dir)| exported::stale(&files, dir))
        .map(|s| (s.path, s.key))
        .collect();
    prompts
        .listing()
        .into_iter()
        .map(|listed| {
            let name = match &listed.key {
                Some(key) => format!("{} {key}", listed.file),
                None => listed.file.to_owned(),
            };
            let from = match (&listed.origin, &listed.path) {
                (Origin::BuiltIn, _) | (_, None) => listed.origin.to_string(),
                (origin, Some(path)) => format!("{origin:<10} {}", path.display()),
            };
            let is_behind = listed.path.as_ref().is_some_and(|path| {
                behind
                    .iter()
                    .any(|(p, key)| p == path && (key.is_none() || *key == listed.key))
            });
            let state = if is_behind {
                "behind"
            } else if listed.differs {
                "changed"
            } else {
                ""
            };
            format!("{:<8} {name:<56} {state:<8} {from}", listed.layer)
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The prompt files `names` pick, by their path in the tree or a directory
/// of it, or every one if there are none.
fn prompt_files(names: &[String]) -> anyhow::Result<Vec<ghost_mind::prompts::File>> {
    let files = ghost_mind::prompts::all_files();
    words::pick(&files, names).map_err(|unknown| {
        let known: Vec<&str> = files.iter().map(|f| f.path).collect();
        anyhow::anyhow!(
            "there is no prompt file or directory {unknown:?} in the tree; there are {}",
            known.join(", ")
        )
    })
}

/// `ghostd prompts export`.
fn export_prompts(target: &Path, force: bool, only: &[String]) -> anyhow::Result<()> {
    words::say_flat(&ghost_mind::prompts::all_files(), target);
    let done = Store::export(&prompt_files(only)?, target, force)
        .with_context(|| format!("write into {}", target.display()))?;
    let skipped = done.iter().filter(|(_, written)| !written).count();
    for (path, written) in &done {
        if *written {
            println!("wrote    {}", path.display());
        } else {
            println!("skipped  {} (already there)", path.display());
        }
    }
    if skipped > 0 {
        println!("{skipped} left as they were; --force overwrites them");
    }
    Ok(())
}

/// `ghostd prompts update`.
fn update_prompts(target: &Path, looked_at: &[String]) -> anyhow::Result<()> {
    words::say_flat(&ghost_mind::prompts::all_files(), target);
    let looked_at: Vec<&str> = if looked_at.is_empty() {
        Vec::new()
    } else {
        prompt_files(looked_at)?.iter().map(|f| f.path).collect()
    };
    let done = exported::update(&ghost_mind::prompts::all_files(), target, &looked_at)
        .with_context(|| format!("update {}", target.display()))?;
    if done.is_empty() {
        println!("no copies in {}", target.display());
    }
    for (path, what) in &done {
        let said = match what {
            Updated::Replaced => "updated ",
            Updated::Current => "current ",
            Updated::Marked => "kept    ",
            Updated::LeftAlone => "edited  ",
        };
        println!("{said} {}", path.display());
    }
    if done.iter().any(|(_, what)| *what == Updated::LeftAlone) {
        println!(
            "edited copies were left as they are; `ghostd run` warns about any that hide something newer"
        );
    }
    Ok(())
}

/// What she has to work with, from her `[mind.work]`. Everything in it that
/// keeps anything is under her own directory.
fn workshop(hers: &Hers, ghost_dir: &Path) -> anyhow::Result<ghost_mind::Workshop> {
    use ghost_work::{Docker, Local, Machine, tools::WebConfig};
    let cfg = &hers.mind.work;
    let machine: Option<Arc<dyn Machine>> = match cfg.machine.as_deref().map(str::trim) {
        None | Some("" | "none") => None,
        Some(spec) => match spec.split_once(':') {
            Some(("docker", container)) if !container.is_empty() => Some(Arc::new(
                Docker::new(container).workdir(cfg.workdir.clone()),
            )),
            Some(("local", dir)) => {
                // Relative to her own directory, and `local:` alone is a
                // `home` inside it.
                let dir = if dir.is_empty() {
                    ghost_dir.join("home")
                } else {
                    ghost_dir.join(dir)
                };
                std::fs::create_dir_all(&dir)
                    .with_context(|| format!("create {}", dir.display()))?;
                warn!(
                    "her shell runs on this computer, in {}, as you. That is not a sandbox.",
                    dir.display()
                );
                Some(Arc::new(Local::new(dir.canonicalize()?)))
            }
            _ => bail!(
                "work.machine is {spec:?}: use \"docker:<container>\" or \"local:<directory>\""
            ),
        },
    };
    // What the same choices look like as arguments to `ghost-work`, which
    // serves her tools to a brain that is a command-line agent.
    let mut runner_args = match cfg.machine.as_deref().map(str::trim) {
        None | Some("" | "none") => vec!["--no-machine".to_owned()],
        Some(spec) => match spec.split_once(':') {
            Some(("docker", container)) => {
                let mut args = vec!["--docker".to_owned(), container.to_owned()];
                if let Some(dir) = &cfg.workdir {
                    args.extend(["--workdir".to_owned(), dir.clone()]);
                }
                args
            }
            Some((_, dir)) => {
                let dir = ghost_dir.join(if dir.is_empty() { "home" } else { dir });
                vec![
                    "--local".to_owned(),
                    dir.canonicalize()?.display().to_string(),
                ]
            }
            None => Vec::new(),
        },
    };
    if cfg.web {
        runner_args.push("--web".to_owned());
    }
    // Looked for where it was told to be, then beside this program.
    let runner = std::env::var_os("GHOST_WORK_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let beside = std::env::current_exe().ok()?.with_file_name("ghost-work");
            beside.exists().then_some(beside)
        });
    Ok(ghost_mind::Workshop {
        machine,
        web: if cfg.web {
            WebConfig::from_env()
        } else {
            WebConfig::default()
        },
        runner,
        runner_args,
        servers: servers(hers, ghost_dir)?,
        // Kept under `skills/` in her directory. Always there, so that the
        // skills she has stay hers when she may no longer install more.
        skills: Some(ghost_work::skills::Skills::new(
            hers.skills.clone(),
            ghost_dir.join("skills"),
        )),
    })
}

/// Her servers of tools, kept under `tools/` in her directory, if she may
/// install any or has been given any.
fn servers(
    hers: &Hers,
    ghost_dir: &Path,
) -> anyhow::Result<Option<Arc<ghost_work::servers::Servers>>> {
    if !hers.tools.enabled && hers.mcp.is_empty() {
        return Ok(None);
    }
    // Absolute: each server runs with its own folder as its home.
    let dir = ghost_dir.canonicalize()?.join("tools");
    Ok(Some(ghost_work::servers::Servers::new(
        hers.tools.clone(),
        dir,
        hers.mcp.clone(),
    )))
}

/// Say what this machine could think with, as configuration to paste.
async fn discover() -> anyhow::Result<()> {
    let found = ghost_brain::discover::discover().await;
    if found.is_empty() {
        println!("Nothing found. Set an API key in the environment (DEEPSEEK_API_KEY,");
        println!("ANTHROPIC_API_KEY, OPENAI_API_KEY), install the claude or codex CLI,");
        println!("or start Ollama.");
        return Ok(());
    }
    for f in &found {
        println!("# {}", f.what);
        if !f.works {
            println!("# It can answer, but cannot run tools yet: use it to talk, not to work.");
        }
        println!("{}", f.toml());
    }
    Ok(())
}

async fn brains(probe: bool, config: Option<&Path>) -> anyhow::Result<()> {
    let (cfg, path) = Config::load(config)?;
    network(&cfg.network)?;
    println!("config    {}", path.display());
    println!("think     {}", cfg.brain.think);
    println!("tick      {}", cfg.brain.tick);
    println!();
    for (name, pc) in &cfg.providers {
        let built = pc.build();
        let label = match &built {
            Ok(b) => b.label(),
            Err(e) => format!("unavailable: {e}"),
        };
        println!("{name:<14} {:<12} {label}", pc.kind());
        if !probe {
            continue;
        }
        let Ok(brain) = built else { continue };
        let request = Completion::ask("Reply with exactly one word.", "Say hello.", 16);
        let started = Instant::now();
        match brain.complete(&request).await {
            Ok(r) => println!(
                "{:<14} ok  {:.1}s  {:?}",
                "",
                started.elapsed().as_secs_f32(),
                r.text
            ),
            Err(e) => println!("{:<14} err {e}", ""),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_shared_prompt_in_a_ghosts_own_directory_is_warned_about_and_the_deployments_is_used() {
        let (ghost, deployment) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let hers = ghost.path().join(words::DIR);
        fs::create_dir_all(hers.join("character")).unwrap();
        // One laid out as the tree, one the old flat way: neither is read.
        fs::write(hers.join("plan.md"), "hers").unwrap();
        fs::write(hers.join("character/persona.md"), "hers too").unwrap();

        let said = misplaced(ghost.path(), Some(deployment.path())).unwrap();
        assert!(said.contains("character/persona.md, plan.md"), "{said}");
        assert!(
            said.contains(&deployment.path().display().to_string()),
            "{said}"
        );
        // Not read, with or without a deployment's directory.
        assert_eq!(
            prompts_for(ghost.path(), None).plan,
            Prompts::shipped().plan
        );
        assert_eq!(
            prompts_for(ghost.path(), None).persona,
            Prompts::shipped().persona
        );
        fs::create_dir_all(deployment.path().join("system/logic")).unwrap();
        fs::write(
            deployment.path().join("system/logic/plan.md"),
            "the deployment's",
        )
        .unwrap();
        assert_eq!(
            prompts_for(ghost.path(), Some(deployment.path())).plan,
            "the deployment's"
        );
        // A ghost with no prompts of her own is not warned about.
        assert_eq!(misplaced(deployment.path(), None), None);
    }

    #[test]
    fn a_copy_of_the_retired_self_md_is_named_wherever_it_is() {
        let (ghost, deployment) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::create_dir_all(ghost.path().join(words::DIR)).unwrap();
        fs::write(ghost.path().join(words::DIR).join(RETIRED), "old").unwrap();
        fs::write(deployment.path().join(RETIRED), "old").unwrap();
        assert!(misplaced(ghost.path(), None).unwrap().contains("self.md"));
        let said = retired(deployment.path()).unwrap();
        assert!(
            said.contains("character/persona.md") && said.contains("system/being.md"),
            "{said}"
        );
    }

    #[test]
    fn prompts_list_shows_each_prompt_by_its_file_in_the_tree_with_its_layer() {
        let deployment = tempfile::tempdir().unwrap();
        fs::create_dir_all(deployment.path().join("system")).unwrap();
        fs::write(
            deployment.path().join("system/being.md"),
            "Being.\n\n{{abilities}}{{canon}}\n",
        )
        .unwrap();
        let lines = listing(&Prompts::load(Some(deployment.path())));
        let line = |file: &str, key: Option<&str>| {
            lines
                .iter()
                .find(|l| {
                    let mut words = l.split_whitespace().skip(1);
                    words.next() == Some(file) && (key.is_none() || words.next() == key)
                })
                .unwrap_or_else(|| panic!("no {file} {key:?} in {lines:#?}"))
                .clone()
        };
        assert!(line("character/persona.md", None).starts_with("persona "));
        assert!(line("character/texts.toml", Some("facet")).starts_with("persona "));
        let being = line("system/being.md", None);
        assert!(being.starts_with("being "));
        assert!(being.contains("changed"));
        assert!(being.contains(&format!(
            "deployment {}",
            deployment.path().join("system/being.md").display()
        )));
        assert!(line("system/logic/attend.md", None).starts_with("logic "));
        assert!(line("system/logic/texts/addressed.toml", Some("fact_note")).starts_with("logic "));
        assert!(line("system/work/working.md", None).starts_with("work "));
        assert!(line("system/work/work.md", None).starts_with("work "));
        assert!(line("system/work/texts.toml", Some("budget")).starts_with("work "));
        assert!(line("system/tools.toml", Some("say")).starts_with("work "));
        // Every line is of the tree, in one half of it or the other.
        for l in &lines {
            let file = l.split_whitespace().nth(1).unwrap();
            assert!(
                file.starts_with("system/") || file.starts_with("character/"),
                "{l}"
            );
        }
    }
}
