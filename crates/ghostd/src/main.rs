//! `ghostd`: runs one ghost.
//!
//! `run` is the daemon: it opens her directory, starts her mind, and listens
//! for presences and consoles. The other subcommands either read her
//! directory without disturbing a running daemon (`status`, `journal`,
//! `verify`, `prompts`) or connect to one as a console (`watch`, `agenda`,
//! `people`).

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
    life::{Life, Phase},
    memory::Memory,
    prompts::{
        Store,
        exported::{self, Updated},
    },
};
use ghost_link::Console;
use ghost_mind::{Mind, Numb, OpenMeteo, Parts, Prompts, Senses, prompts::Origin};
use ghost_protocol::GhostCard;
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::{
    config::Config,
    server::{Presences, Shared},
};

#[derive(Parser)]
#[command(name = "ghostd", version, about = "Runs one ghost.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Give birth to the ghost described by DIR/ghost.toml. Happens once.
    Birth { dir: PathBuf },
    /// Run her: verify her journal, record any revision of ghost.toml, start
    /// her mind, listen for presences.
    Run {
        dir: PathBuf,
        /// Path to ghostd.toml.
        #[arg(long)]
        config: Option<PathBuf>,
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
    /// The words her brain reads: where each comes from, and a copy of the
    /// built-in ones to change.
    Prompts {
        #[command(subcommand)]
        cmd: PromptsCmd,
    },
}

#[derive(Subcommand)]
enum PromptsCmd {
    /// Every template, text and tool description, with where what is in use
    /// comes from (DIR/prompts, the config's directory, or built in) and
    /// whether it differs from the built-in.
    List {
        dir: PathBuf,
        /// Path to ghostd.toml, for its `[mind] prompts`.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Write the built-in prompts into DIR/prompts, to be changed there,
    /// with a note of what each was copied from.
    Export {
        dir: PathBuf,
        /// Overwrite files that are already there.
        #[arg(long)]
        force: bool,
        /// Only these files, such as `self.md texts.toml`: the ones you
        /// mean to change. A copy you leave as it is still hides the
        /// built-in after an upgrade.
        #[arg(long, num_args = 1.., value_name = "FILE")]
        only: Vec<String>,
    },
    /// Bring the copies in DIR/prompts that nobody edited up to this
    /// build's built-ins. Edited copies are left as they are; one that has
    /// been brought up to date by hand is named with --looked-at, so that
    /// it is no longer warned about.
    Update {
        dir: PathBuf,
        /// Copies you have compared with this build's built-in and kept.
        #[arg(long, num_args = 1.., value_name = "FILE")]
        looked_at: Vec<String>,
    },
}

/// How to reach a running daemon.
#[derive(clap::Args)]
struct Daemon {
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

impl Daemon {
    async fn console(self) -> anyhow::Result<Console> {
        let (console, _) = Console::connect(&self.url, self.token).await?;
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
        Cmd::Run { dir, config } => run(&dir, config.as_deref()).await,
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
            cmd: PromptsCmd::List { dir, config },
        } => list_prompts(&dir, config.as_deref()),
        Cmd::Prompts {
            cmd: PromptsCmd::Export { dir, force, only },
        } => export_prompts(&dir, force, &only),
        Cmd::Prompts {
            cmd: PromptsCmd::Update { dir, looked_at },
        } => update_prompts(&dir, &looked_at),
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

async fn run(dir: &Path, config: Option<&Path>) -> anyhow::Result<()> {
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
    let ghost = Ghost::open(dir).with_context(|| format!("open {}", dir.display()))?;
    let seed = ghost.identity().seed();
    let card = GhostCard {
        name: seed.name.clone(),
        pronouns: seed.pronouns.clone(),
        city: seed.home.city.clone(),
        country: seed.home.country.clone(),
        seal: ghost.identity().seal().to_string(),
    };
    let (think, tick) = (cfg.think()?, cfg.tick()?);
    info!(
        "{} is in {}. seal {}. config {}",
        card.name,
        card.city,
        ghost.identity().seal().short(),
        cfg_path.display()
    );
    info!(
        "think on {}, tick on {}, tempo {}",
        think.label(),
        tick.label(),
        cfg.mind.tempo
    );

    let presences = Arc::new(Presences::default());
    let senses: Arc<dyn Senses> = if cfg.mind.weather {
        Arc::new(OpenMeteo::new())
    } else {
        Arc::new(Numb)
    };
    let prompts = Prompts::load(dir, cfg.mind.prompts.as_deref());
    say_overrides(&prompts);
    say_stale(&prompts);
    let workshop = workshop(&cfg, dir)?;
    if cfg.mind.work.enabled {
        info!(
            "work: {}, {}",
            cfg.mind.work.machine.as_deref().unwrap_or("no machine"),
            if workshop.web.search.is_some() {
                "can search the web"
            } else {
                "cannot search the web"
            }
        );
    }
    if !cfg.operators.accounts.is_empty() {
        info!(
            "directives begin with {:?}, from operators on {}",
            cfg.operators.prefix,
            cfg.operators
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
        body: presences.clone(),
        senses,
        prompts,
        config: cfg.mind(),
        workshop,
    });

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
            mind: handle.clone(),
            presences,
            token,
            card,
            origins: cfg.listen.origins.clone(),
        }),
    ));

    let mut living = tokio::spawn(mind.run());
    let outcome = tokio::select! {
        ended = &mut living => ended,
        why = told_to_stop() => {
            info!("{why}: stopping");
            handle.stop().await;
            living.await
        }
    };
    server.abort();
    outcome??;
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
/// is how systemd, docker and a shutting-down machine ask. Either way she
/// is stopped in order, so that nothing is cut off halfway through being
/// written. Says which it was.
async fn told_to_stop() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = tokio::signal::ctrl_c() => "interrupted",
                _ = term.recv() => "terminated",
            },
            Err(e) => {
                warn!("cannot listen for SIGTERM ({e}); only Ctrl-C stops her in order");
                let _ = tokio::signal::ctrl_c().await;
                "interrupted"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "interrupted"
    }
}

fn status(dir: &Path) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    let id = &peek.identity;
    let life = Life::new(id);
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
    let (mut console, welcome) = Console::connect(&daemon.url, daemon.token).await?;
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
        // The commands take the directory a `prompts/` is in.
        if let Some(parent) = dir
            .parent()
            .filter(|_| dir.ends_with(ghost_mind::prompts::DIR))
        {
            warn!(
                "if you never edited them, `ghostd prompts update {0}` brings them up to date. \
                 If you did, write this build's next to them with `ghostd prompts export <somewhere> --only {names}`, \
                 carry over what you want, then `ghostd prompts update {0} --looked-at {names}`",
                parent.display()
            );
        } else {
            warn!(
                "compare them with this build's (`ghostd prompts export <somewhere> --only {names}`) and carry over what you want"
            );
        }
    }
}

/// `ghostd prompts list`.
fn list_prompts(dir: &Path, config: Option<&Path>) -> anyhow::Result<()> {
    let configured = if Config::exists(config) {
        Config::load(config)?.0.mind.prompts
    } else {
        None
    };
    let prompts = Prompts::load(dir, configured.as_deref());
    // What hides something newer in this build, by where it is.
    let files = ghost_mind::prompts::all_files();
    let behind: Vec<(PathBuf, Option<String>)> = prompts
        .store()
        .layers()
        .iter()
        .flat_map(|(_, dir)| exported::stale(&files, dir))
        .map(|s| (s.path, s.key))
        .collect();
    for listed in prompts.listing() {
        let name = match &listed.key {
            Some(key) => format!("{} {key}", listed.file),
            None => listed.file.to_owned(),
        };
        let from = match (&listed.origin, &listed.path) {
            (Origin::BuiltIn, _) | (_, None) => listed.origin.to_string(),
            (origin, Some(path)) => format!("{origin:<8} {}", path.display()),
        };
        let is_behind = listed.path.as_ref().is_some_and(|path| {
            behind
                .iter()
                .any(|(p, key)| p == path && (key.is_none() || *key == listed.key))
        });
        println!(
            "{name:<36} {:<8} {from}",
            if is_behind {
                "behind"
            } else if listed.differs {
                "changed"
            } else {
                ""
            }
        );
    }
    Ok(())
}

/// The prompt files called `names`, or every one if there are none.
fn prompt_files(names: &[String]) -> anyhow::Result<Vec<ghost_mind::prompts::File>> {
    let files = ghost_mind::prompts::all_files();
    if let Some(unknown) = names.iter().find(|n| !files.iter().any(|f| f.name == *n)) {
        let known: Vec<&str> = files.iter().map(|f| f.name).collect();
        bail!(
            "there is no prompt file {unknown:?}; there are {}",
            known.join(", ")
        );
    }
    Ok(files
        .into_iter()
        .filter(|f| names.is_empty() || names.iter().any(|n| n == f.name))
        .collect())
}

/// `ghostd prompts export`.
fn export_prompts(dir: &Path, force: bool, only: &[String]) -> anyhow::Result<()> {
    let target = dir.join(ghost_mind::prompts::DIR);
    let done = Store::export(&prompt_files(only)?, &target, force)
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
fn update_prompts(dir: &Path, looked_at: &[String]) -> anyhow::Result<()> {
    let target = dir.join(ghost_mind::prompts::DIR);
    prompt_files(looked_at)?;
    let looked_at: Vec<&str> = looked_at.iter().map(String::as_str).collect();
    let done = exported::update(&ghost_mind::prompts::all_files(), &target, &looked_at)
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

/// What she has to work with, from `[mind.work]`.
fn workshop(config: &Config, ghost_dir: &Path) -> anyhow::Result<ghost_mind::Workshop> {
    use ghost_work::{Docker, Local, Machine, tools::WebConfig};
    let cfg = &config.mind.work;
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
        servers: servers(config, ghost_dir)?,
    })
}

/// Her servers of tools, kept under `tools/` in her directory, if she may
/// install any or has been given any.
fn servers(
    config: &Config,
    ghost_dir: &Path,
) -> anyhow::Result<Option<Arc<ghost_work::servers::Servers>>> {
    if !config.tools.enabled && config.mcp.is_empty() {
        return Ok(None);
    }
    // Absolute: each server runs with its own folder as its home.
    let dir = ghost_dir.canonicalize()?.join("tools");
    Ok(Some(ghost_work::servers::Servers::new(
        config.tools.clone(),
        dir,
        config.mcp.clone(),
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
