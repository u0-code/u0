//! Run one undertaking from the command line.
//!
//! ```text
//! ghost-work --task "..." --state ./run1 --local ./workspace
//! ghost-work --task-file task.md --state ./run1 --docker my-container --workdir /app
//! ghost-work --task "..." --state ./run1 --local ./workspace --agent claude
//! ```
//!
//! The plan, notes, transcript and log are kept in `<state>/work.db`. Run it
//! again with the same `--state` to pick up where it stopped. The report is
//! printed to stdout as JSON; progress goes to stderr.
//!
//! `--agent claude` or `--agent codex` lets the work hand parts of itself to
//! that installed CLI, which does them in the `--local` directory with its
//! own tools.

use std::{path::PathBuf, sync::Arc, sync::atomic::AtomicBool, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use ghost_brain::{CliAgent, CliKind, Effort, config::ProviderConfig};
use ghost_work::{
    Docker, Limits, Local, Machine, Prompts, Undertaking, WORK_DB, Worker, prompts, tools,
};

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
    /// The directory whose `work.db` keeps this undertaking's plan, notes,
    /// transcript and log. With `--no-machine` it is also where the tools
    /// would work.
    #[arg(long)]
    state: PathBuf,
    /// A directory of prompts laid out as `prompts/` is:
    /// `system/work/work.md`, the files of `system/work/texts/` and
    /// `system/tools.toml`, or any of them. What is built in is used for anything
    /// it does not have. A file changed while the work runs is taken in at
    /// the next step.
    #[arg(long)]
    prompts: Option<PathBuf>,

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
    /// Nowhere to run anything: only the record of the work and, with
    /// `--web`, the web.
    #[arg(long, conflicts_with_all = ["local", "docker"])]
    no_machine: bool,

    /// An installed agent the work may hand parts to: `claude` or `codex`.
    /// Repeatable. Needs `--local`, the directory it works in.
    #[arg(long = "agent", requires = "local")]
    agents: Vec<String>,
    /// The Claude Code program, for `--agent claude`.
    #[arg(long, default_value = "claude")]
    claude_bin: String,
    /// The Codex program, for `--agent codex`.
    #[arg(long, default_value = "codex")]
    codex_bin: String,
    /// An extra argument for the command-line agent. Repeatable. For
    /// Codex on another provider: `--agent-arg=-c --agent-arg='model_provider="x"'`.
    #[arg(long = "agent-arg", allow_hyphen_values = true)]
    agent_args: Vec<String>,

    /// The model API: `openai` (any compatible server) or `anthropic`.
    #[arg(long, default_value = "openai")]
    kind: String,
    /// The model.
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
    /// Default: most of the model's context window.
    #[arg(long)]
    context_tokens: Option<u64>,
    /// The model's context window in tokens, for a model whose window is
    /// not known here.
    #[arg(long)]
    context_window: Option<u64>,
    /// Output tokens per step, thinking included. Default: what the model
    /// is known to manage, up to 32,000.
    #[arg(long)]
    max_output: Option<u32>,
    /// How hard the model thinks at each step: off, low, high or max.
    /// Default: whatever the provider does.
    #[arg(long)]
    effort: Option<String>,
    #[arg(long)]
    temperature: Option<f32>,
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
    let db = args.state.join(WORK_DB);
    let undertaking = Arc::new(
        Undertaking::open(&db, "main", brief.as_deref())
            .with_context(|| format!("open {}", db.display()))?,
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
        (None, None) if args.no_machine => Arc::new(Local::new(&args.state)),
        (None, None) => {
            bail!("say where to work: --local <dir>, --docker <container> or --no-machine")
        }
    };
    // The tools for a piece of work with its own record: this one, and any
    // helper it hands part of itself to.
    let (no_machine, web) = (args.no_machine, args.web);
    let words = Prompts::load(args.prompts.as_deref()).shared();
    let equip: ghost_work::worker::ToolsFor = {
        let machine = machine.clone();
        Arc::new(move |undertaking| {
            let toolbox = if no_machine {
                tools::without_machine(undertaking)
            } else {
                tools::standard(machine.clone(), undertaking)
            };
            if web {
                tools::with_web(toolbox, &tools::WebConfig::from_env())
            } else {
                toolbox
            }
        })
    };
    // What it keeps stays in this undertaking's record. A helper is not
    // given `keep`: its record is its own, and nobody reads what it kept.
    let toolbox = equip(&undertaking).with(tools::Keep::new(undertaking.clone()));

    // A first look around, so the first steps are not spent finding out
    // which system this is and what is in the directory.
    let mut place = if args.no_machine {
        prompts::hold(&words).text("no_machine").to_owned()
    } else {
        machine.describe(&prompts::hold(&words))
    };
    let look = "uname -srm 2>/dev/null; echo \"user: $(whoami 2>/dev/null)\"; echo \"directory: $PWD\"; ls -la 2>/dev/null | head -40";
    let looked = if args.no_machine {
        Ok(ghost_work::machine::Output::default())
    } else {
        machine.exec(look, None, Duration::from_secs(30)).await
    };
    match looked {
        Ok(out) if !out.text.trim().is_empty() => {
            place.push_str("\n\n");
            place.push_str(
                &prompts::hold(&words)
                    .fill("looked_around", &[("look", out.text.trim().to_owned())]),
            );
        }
        Ok(_) => {}
        Err(e) => bail!("the machine cannot be reached: {e}"),
    }
    let limits = Limits {
        max_steps: args.max_steps,
        max_seconds: args.max_minutes * 60,
        context_tokens: args.context_tokens,
        max_output: args.max_output,
        effort: match args.effort.as_deref() {
            None => None,
            Some("off") => Some(Effort::Off),
            Some("low") => Some(Effort::Low),
            Some("high") => Some(Effort::High),
            Some("max") => Some(Effort::Max),
            Some(other) => bail!("unknown effort {other:?}: use off, low, high or max"),
        },
        temperature: args.temperature,
        ..Limits::default()
    };

    let report = {
        // A provider as ghostd.toml would give it, read the same way.
        let brain = serde_json::from_value::<ProviderConfig>(serde_json::json!({
            "kind": args.kind,
            "model": args.model,
            "base_url": args.base_url,
            "api_key_env": args.api_key_env,
            "context_window": args.context_window,
        }))?
        .build()?;
        Worker {
            brain,
            tools: toolbox,
            undertaking,
            limits,
            who: String::new(),
            machine: place,
            pause: Arc::new(AtomicBool::new(false)),
            helpers: Some(equip),
            prompts: words,
            agents: agents(&args)?,
        }
        .run()
        .await
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// The installed agents `--agent` names, working in `--local`.
fn agents(args: &Args) -> anyhow::Result<Vec<ghost_work::agents::Agent>> {
    let Some(dir) = &args.local else {
        return Ok(Vec::new());
    };
    let dir = dir.canonicalize()?;
    args.agents
        .iter()
        .map(|name| {
            let (kind, program, name) = match name.as_str() {
                "claude" | "claude-code" => (CliKind::Claude, &args.claude_bin, "claude-code"),
                "codex" => (CliKind::Codex, &args.codex_bin, "codex"),
                other => bail!("unknown agent {other:?}: use claude or codex"),
            };
            Ok(ghost_work::agents::Agent {
                name: name.to_owned(),
                cli: CliAgent {
                    kind,
                    program: program.clone(),
                    model: None,
                    args: args.agent_args.clone(),
                },
                dir: dir.clone(),
            })
        })
        .collect()
}
