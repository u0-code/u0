//! `ghostd`: runs one ghost.
//!
//! `run` is the daemon: it opens her directory, starts her mind, and listens
//! for presences and consoles. The other subcommands either read her
//! directory without disturbing a running daemon (`status`, `journal`,
//! `verify`) or connect to one as a console (`watch`, `agenda`, `people`).

mod config;
mod server;
mod show;
mod wire;

use std::{
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
};
use ghost_link::Console;
use ghost_mind::{Mind, Numb, OpenMeteo, Parts, Prompts, Senses};
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
    /// Run her: verify the seal, start her mind, listen for presences.
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
    /// Verify her seal and the whole hash chain.
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
    let (cfg, cfg_path) = Config::load(config)?;
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
    let prompts = Prompts::load(cfg.mind.prompts.as_deref()).context("load prompts")?;
    let workshop = workshop(&cfg.mind.work, dir)?;
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
    let (mind, handle) = Mind::new(Parts {
        ghost,
        think,
        tick,
        body: presences.clone(),
        senses,
        prompts,
        config: cfg.mind.clone(),
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
        _ = tokio::signal::ctrl_c() => {
            handle.stop().await;
            living.await
        }
    };
    server.abort();
    outcome??;
    Ok(())
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
    println!("seal      {}", id.seal());
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
    println!("seal ok, chain ok, {} entries", peek.journal.len()?);
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

/// What she has to work with, from `[mind.work]`.
fn workshop(
    cfg: &ghost_mind::WorkConfig,
    ghost_dir: &Path,
) -> anyhow::Result<ghost_mind::Workshop> {
    use ghost_work::{Docker, Local, Machine, tools::WebConfig};
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
    })
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
