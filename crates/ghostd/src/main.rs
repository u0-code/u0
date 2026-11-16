//! `ghostd`: runs ghosts.
//!
//! `run` is the daemon: it opens each character's directory, starts her
//! mind, and listens for presences and consoles, each of which is for one
//! of them. The other subcommands either read one directory without
//! disturbing a running daemon (`status`, `journal`, `verify`, `prompts`)
//! or connect to one character as a console (`watch`, `agenda`, `people`).

mod brains;
mod cast;
mod config;
mod console;
mod directory;
mod operator;
mod prompts;
mod run;
mod server;
mod show;
mod stop;
mod wire;
mod workshop;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ghost_link::Console;

use crate::config::Config;

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
        Cmd::Birth { dir } => directory::birth(&dir),
        Cmd::Run {
            dirs,
            config,
            with_parent,
        } => run::run(&dirs, config.as_deref(), with_parent).await,
        Cmd::Status { dir } => directory::status(&dir),
        Cmd::Journal { dir, last } => directory::journal(&dir, last),
        Cmd::Verify { dir } => directory::verify(&dir),
        Cmd::Brains { discover: true, .. } => brains::discover().await,
        Cmd::Brains { probe, config, .. } => brains::brains(probe, config.as_deref()).await,
        Cmd::Watch { daemon, last } => console::watch(daemon, last).await,
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
            prompts::list_prompts(at.found()?.as_deref());
            Ok(())
        }
        Cmd::Prompts {
            cmd: PromptsCmd::Export { at, force, only },
        } => prompts::export_prompts(&at.needed()?, force, &only),
        Cmd::Prompts {
            cmd: PromptsCmd::Update { at, looked_at },
        } => prompts::update_prompts(&at.needed()?, &looked_at),
    }
}
