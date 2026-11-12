//! Run one undertaking from the command line.
//!
//! ```text
//! ghost-work --task "..." --state ./run1 --local ./workspace
//! ghost-work --task-file task.md --state ./run1 --docker my-container --workdir /app
//! ```
//!
//! Run it again with the same `--state` to pick up where it stopped. The
//! report is printed to stdout as JSON; progress goes to stderr.

use std::{path::PathBuf, sync::Arc, sync::atomic::AtomicBool, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use ghost_brain::{Brain, anthropic::Anthropic, openai::OpenAi};
use ghost_work::{Docker, Limits, Local, Machine, Undertaking, Worker, tools};

#[derive(Parser)]
#[command(
    version,
    about = "Do one piece of work with a model and a set of tools."
)]
struct Args {
    /// The task, as text.
    #[arg(long, conflicts_with = "task_file")]
    task: Option<String>,
    /// A file holding the task.
    #[arg(long)]
    task_file: Option<PathBuf>,
    /// Where this undertaking's plan, notes, transcript and log are kept.
    #[arg(long)]
    state: PathBuf,

    /// Work in this directory on this computer.
    #[arg(long, conflicts_with = "docker")]
    local: Option<PathBuf>,
    /// Work inside this running Docker container.
    #[arg(long)]
    docker: Option<String>,
    /// Starting directory inside the container.
    #[arg(long, requires = "docker")]
    workdir: Option<String>,
    /// User to run as inside the container.
    #[arg(long, requires = "docker")]
    user: Option<String>,
    /// A variable to set for every command in the container, as NAME=value.
    #[arg(long = "docker-env", requires = "docker")]
    docker_env: Vec<String>,

    /// `openai` (any compatible server) or `anthropic`.
    #[arg(long, default_value = "openai")]
    kind: String,
    #[arg(long, default_value = "deepseek-flash")]
    model: String,
    #[arg(long, default_value = "https://api.deepseek.com")]
    base_url: String,
    /// The environment variable holding the API key.
    #[arg(long, default_value = "DEEPSEEK_API_KEY")]
    api_key_env: String,

    /// Give the worker `search` and `fetch`. Search uses `DEEPSEEK_API_KEY`,
    /// `BRAVE_API_KEY` or `SEARXNG_URL`, whichever is set first.
    #[arg(long)]
    web: bool,

    #[arg(long, default_value_t = 200)]
    max_steps: u32,
    #[arg(long, default_value_t = 60)]
    max_minutes: u64,
    /// Fold the transcript when it is about to pass this many tokens.
    #[arg(long, default_value_t = 60_000)]
    context_tokens: u64,
    #[arg(long, default_value_t = 8192)]
    max_output: u32,
    #[arg(long)]
    temperature: Option<f32>,
    /// Accept the worker's own word that it is finished, without a review.
    #[arg(long)]
    no_audit: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
    let args = Args::parse();

    let brief = match (&args.task, &args.task_file) {
        (Some(text), _) => Some(text.clone()),
        (None, Some(path)) => Some(
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        ),
        (None, None) => None,
    };
    let undertaking = Arc::new(
        Undertaking::open(&args.state, brief.as_deref())
            .with_context(|| format!("open {}", args.state.display()))?,
    );

    let machine: Arc<dyn Machine> = match (&args.local, &args.docker) {
        (_, Some(container)) => Arc::new(
            Docker::new(container.clone())
                .workdir(args.workdir.clone())
                .user(args.user.clone())
                .env(args.docker_env.clone()),
        ),
        (Some(dir), None) => {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
            Arc::new(Local::new(dir.canonicalize()?))
        }
        (None, None) => bail!("say where to work: --local <dir> or --docker <container>"),
    };

    let key = std::env::var(&args.api_key_env)
        .ok()
        .filter(|k| !k.trim().is_empty())
        .with_context(|| format!("set {} to the API key", args.api_key_env))?;
    let brain: Arc<dyn Brain> = match args.kind.as_str() {
        "openai" => Arc::new(OpenAi::new(
            key,
            args.model.clone(),
            Some(args.base_url.clone()),
            None,
            false,
        )),
        "anthropic" => Arc::new(Anthropic::new(
            key,
            args.model.clone(),
            Some(args.base_url.clone()),
        )),
        other => bail!("unknown kind {other:?}: use openai or anthropic"),
    };

    // A first look around, so the first steps are not spent finding out
    // which system this is and what is in the directory.
    let mut place = machine.describe();
    let look = "uname -srm 2>/dev/null; echo \"user: $(whoami 2>/dev/null)\"; echo \"directory: $PWD\"; ls -la 2>/dev/null | head -40";
    match machine.exec(look, None, Duration::from_secs(30)).await {
        Ok(out) if !out.text.trim().is_empty() => {
            place.push_str("\n\nWhen this run started it looked like this:\n\n```\n");
            place.push_str(out.text.trim());
            place.push_str("\n```");
        }
        Ok(_) => {}
        Err(e) => bail!("the machine cannot be reached: {e}"),
    }

    let worker = Worker {
        brain,
        tools: if args.web {
            tools::with_web(
                tools::standard(machine.clone(), &undertaking),
                &tools::WebConfig::from_env(),
            )
        } else {
            tools::standard(machine.clone(), &undertaking)
        },
        undertaking,
        limits: Limits {
            max_steps: args.max_steps,
            max_seconds: args.max_minutes * 60,
            context_tokens: args.context_tokens,
            max_output: args.max_output,
            temperature: args.temperature,
            audit: !args.no_audit,
        },
        who: String::new(),
        machine: place,
        pause: Arc::new(AtomicBool::new(false)),
    };
    let report = worker.run().await;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
