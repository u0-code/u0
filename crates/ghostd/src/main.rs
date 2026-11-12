//! `ghostd`: runs one ghost.
//!
//! v0 keeps her clock going and prints her journal. The Ghost Protocol server,
//! the brain, and the channels arrive in later steps; the shape of this binary
//! does not change: load her, verify her, tick her.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use chrono::Utc;
use clap::{Parser, Subcommand};
use ghost_core::{
    Ghost,
    journal::{Entry, Event},
    life::{Life, Phase},
};
use tracing::{info, warn};

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    match Cli::parse().cmd {
        Cmd::Birth { dir } => birth(&dir),
        Cmd::Run { dir, tick } => run(&dir, Duration::from_secs(tick)).await,
        Cmd::Status { dir } => status(&dir),
        Cmd::Journal { dir, last } => journal(&dir, last),
        Cmd::Verify { dir } => verify(&dir),
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
