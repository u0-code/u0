//! `ghostd`: runs one ghost.
//!
//! v0 keeps her clock going, prints her journal, and lets you say one thing to
//! her from the terminal. The Ghost Protocol server and the channels arrive in
//! later steps; the shape of this binary does not change: load her, verify
//! her, tick her.

mod config;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context;
use chrono::Utc;
use clap::{Parser, Subcommand};
use ghost_brain::{Completion, Message};
use ghost_core::{
    Ghost,
    journal::{Entry, Event},
    life::{Life, Phase},
    narrate::{Scene, Speaker, thread},
};
use tracing::{debug, info, warn};

use crate::config::Config;

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
    /// Run her: verify the seal, then keep her clock going until Ctrl-C.
    Run {
        dir: PathBuf,
        /// Seconds between ticks of her life engine.
        #[arg(long, default_value_t = 60)]
        tick: u64,
    },
    /// Who she is and where her clock is.
    Status { dir: PathBuf },
    /// Print the tail of her journal.
    Journal {
        dir: PathBuf,
        #[arg(long, default_value_t = 50)]
        last: usize,
    },
    /// Verify her seal and the whole hash chain.
    Verify { dir: PathBuf },
    /// Say something to her from the terminal. Both sides go in her journal.
    Say {
        dir: PathBuf,
        text: String,
        /// Who is speaking.
        #[arg(long = "as", default_value = "someone")]
        from: String,
        /// Path to ghostd.toml.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// List configured brains; with --probe, ask each for one word.
    Brains {
        #[arg(long)]
        probe: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
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
        Cmd::Run { dir, tick } => run(&dir, Duration::from_secs(tick)).await,
        Cmd::Status { dir } => status(&dir),
        Cmd::Journal { dir, last } => journal(&dir, last),
        Cmd::Verify { dir } => verify(&dir),
        Cmd::Say {
            dir,
            text,
            from,
            config,
        } => say(&dir, &from, &text, config.as_deref()).await,
        Cmd::Brains { probe, config } => brains(probe, config.as_deref()).await,
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

async fn run(dir: &Path, every: Duration) -> anyhow::Result<()> {
    let mut ghost = Ghost::open(dir).with_context(|| format!("open {}", dir.display()))?;
    let life = Life::new(ghost.identity());
    let name = ghost.identity().name().to_owned();
    let city = ghost.identity().seed().home.city.clone();
    info!(
        "{name} is in {city}. seal {}",
        ghost.identity().seal().short()
    );

    let mut interval = tokio::time::interval(every);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                match life.tick(ghost.journal_mut(), Utc::now()) {
                    Ok(Some(entry)) => info!("{name} {}", describe(&entry)),
                    Ok(None) => {}
                    Err(e) => warn!("tick failed: {e}"),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("leaving {name} be");
                return Ok(());
            }
        }
    }
}

fn status(dir: &Path) -> anyhow::Result<()> {
    let ghost = Ghost::open(dir)?;
    let id = ghost.identity();
    let life = Life::new(id);
    let now = Utc::now();
    let local = now.with_timezone(&life.clock().tz());
    let phase = life.clock().phase_at(now);
    let recorded = Life::recorded_phase(ghost.journal())?;
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
        "phase     {}{}",
        phase_word(phase),
        match recorded {
            Some(r) if r != phase => " (journal lags; run her to catch up)",
            None => " (clock has never ticked)",
            _ => "",
        }
    );
    println!("journal   {} entries", ghost.journal().len()?);
    if let Some(last) = ghost.journal().last()? {
        println!(
            "last      #{} {} {}",
            last.seq,
            last.at.format("%Y-%m-%d %H:%M:%S"),
            describe(&last)
        );
    }
    Ok(())
}

fn journal(dir: &Path, last: usize) -> anyhow::Result<()> {
    let ghost = Ghost::open(dir)?;
    for e in ghost.journal().tail(last)? {
        println!(
            "#{:<6} {}  {:<9} {}",
            e.seq,
            e.at.format("%Y-%m-%d %H:%M:%S"),
            e.event.kind(),
            describe(&e)
        );
    }
    Ok(())
}

fn verify(dir: &Path) -> anyhow::Result<()> {
    let ghost = Ghost::open(dir)?;
    let n = ghost.journal().verify()?;
    println!("seal ok, chain ok, {n} entries");
    Ok(())
}

/// One exchange over the `cli` channel. What they said is journaled before
/// the brain is asked, so a failed call still leaves a trace of being spoken to.
async fn say(dir: &Path, from: &str, text: &str, config: Option<&Path>) -> anyhow::Result<()> {
    const CHANNEL: &str = "cli";
    let (cfg, cfg_path) = Config::load(config)?;
    let brain = cfg.think()?;
    debug!("config {} brain {}", cfg_path.display(), brain.label());
    let template = cfg.self_template()?;

    let mut ghost = Ghost::open(dir)?;
    let life = Life::new(ghost.identity());
    let now = Utc::now();
    life.tick(ghost.journal_mut(), now)?;
    ghost.journal_mut().append_at(
        now,
        Event::Heard {
            channel: CHANNEL.into(),
            from: from.into(),
            text: text.into(),
        },
    )?;

    let recent = ghost
        .journal()
        .last_n_of(&["lived", "planned", "reflected", "woke", "slept"], 12)?;
    let exchange = ghost.journal().last_n_of(&["heard", "said"], 60)?;
    let scene = Scene {
        identity: ghost.identity(),
        clock: life.clock(),
        now,
        person: from,
        channel: CHANNEL,
        recent: &recent,
    };
    let system = scene.system_prompt(&template);
    let messages = thread(&exchange, from)
        .into_iter()
        .map(|t| match t.speaker {
            Speaker::Them => Message::user(t.text),
            Speaker::Her => Message::assistant(t.text),
        })
        .collect();
    let req = Completion {
        system,
        messages,
        max_tokens: cfg.brain.max_tokens,
        temperature: None,
    };

    let started = Instant::now();
    let reply = brain
        .complete(&req)
        .await
        .with_context(|| format!("brain {}", brain.label()))?;
    debug!(
        "{} replied in {:.1}s ({:?} in, {:?} out)",
        reply.model,
        started.elapsed().as_secs_f32(),
        reply.input_tokens,
        reply.output_tokens
    );
    ghost.journal_mut().append(Event::Said {
        channel: CHANNEL.into(),
        to: from.into(),
        text: reply.text.clone(),
    })?;
    println!("{}", reply.text);
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
        let req = Completion {
            system: "Reply with exactly one word.".into(),
            messages: vec![Message::user("Say hello.")],
            max_tokens: 16,
            temperature: None,
        };
        let started = Instant::now();
        match brain.complete(&req).await {
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

fn phase_word(p: Phase) -> &'static str {
    match p {
        Phase::Awake => "awake",
        Phase::Asleep => "asleep",
    }
}

fn describe(e: &Entry) -> String {
    match &e.event {
        Event::Born { name, seal } => format!("{name} was born (seal {})", &seal.as_str()[..8]),
        Event::Woke => "woke up".into(),
        Event::Slept => "went to sleep".into(),
        Event::Planned { plan } => format!("planned: {plan}"),
        Event::Lived { text } => text.clone(),
        Event::Heard {
            channel,
            from,
            text,
        } => format!("[{channel}] {from}: {text}"),
        Event::Said { channel, to, text } => format!("[{channel}] to {to}: {text}"),
        Event::Decided {
            about,
            decision,
            reason,
        } => format!("{about}: {decision} ({reason})"),
        Event::Reflected { text } => format!("reflected: {text}"),
        Event::Canon { fact, source } => format!("canon: {fact} [{source}]"),
    }
}
