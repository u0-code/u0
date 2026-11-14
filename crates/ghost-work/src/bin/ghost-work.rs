//! Run one undertaking from the command line.
//!
//! ```text
//! ghost-work --task "..." --state ./run1 --local ./workspace
//! ghost-work --task-file task.md --state ./run1 --docker my-container --workdir /app
//! ghost-work --task "..." --state ./run1 --local ./workspace --runtime claude
//! ```
//!
//! The plan, notes, transcript and log are kept in `<state>/work.db`. Run it
//! again with the same `--state` to pick up where it stopped. The report is
//! printed to stdout as JSON; progress goes to stderr.
//!
//! `--runtime claude` or `--runtime codex` has that installed CLI do the
//! stepping, with this program's tools and not its own. `--serve-mcp` is the other
//! half of that: it serves the tools on stdio and is started by the CLI.

use std::{path::PathBuf, sync::Arc, sync::atomic::AtomicBool, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use ghost_brain::{Brain, CliAgent, CliKind, Effort, anthropic::Anthropic, openai::OpenAi};
use ghost_work::{
    Docker, Limits, Local, Machine, Prompts, Undertaking, WORK_DB, Worker, claude, codex, mcp,
    prompts, tools,
    worker::{opening_text, system_prompt},
};

#[derive(Parser)]
#[command(
    version,
    about = "Do one piece of work with a model and a set of tools."
)]
#[allow(clippy::struct_excessive_bools)]
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
    /// Which undertaking in that database. One database can hold many: a
    /// ghost keeps all of hers in one, and names this when she has a
    /// command-line agent serve her tools.
    #[arg(long, default_value = "main")]
    id: String,
    /// A directory of prompts: `work.md`, `work_texts.toml` and
    /// `tools.toml`, or any of them. A `prompts/` directory inside `--state`
    /// wins over it, and what is built in is used for anything neither has.
    /// A file changed while the work runs is taken in at the next step.
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

    /// What does the stepping: `native` is this program's own loop over a
    /// model API; `claude` is the installed Claude Code CLI, given this
    /// program's tools.
    #[arg(long, default_value = "native")]
    runtime: String,
    /// The Claude Code program, for `--runtime claude`.
    #[arg(long, default_value = "claude")]
    claude_bin: String,
    /// The Codex program, for `--runtime codex`.
    #[arg(long, default_value = "codex")]
    codex_bin: String,
    /// An extra argument for the command-line agent. Repeatable. For
    /// Codex on another provider: `--agent-arg=-c --agent-arg='model_provider="x"'`.
    #[arg(long = "agent-arg", allow_hyphen_values = true)]
    agent_args: Vec<String>,

    /// For the native runtime: `openai` (any compatible server) or `anthropic`.
    #[arg(long, default_value = "openai")]
    kind: String,
    /// The model. With `--runtime claude`, leave it out for the CLI's default.
    #[arg(long)]
    model: Option<String>,
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

    /// Serve the tools over MCP on stdio instead of working. Started by a
    /// command-line agent, not by a person.
    #[arg(long)]
    serve_mcp: bool,
    /// Offer `tell`, for work someone follows as it runs and reads what it
    /// is told from the record.
    #[arg(long)]
    tell: bool,
}

impl Args {
    /// The arguments that say where to work, for handing to a child.
    fn place(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(dir) = &self.local {
            out.extend(["--local".to_owned(), dir.display().to_string()]);
        }
        if let Some(container) = &self.docker {
            out.extend(["--docker".to_owned(), container.clone()]);
        }
        if let Some(dir) = &self.workdir {
            out.extend(["--workdir".to_owned(), dir.clone()]);
        }
        if let Some(user) = &self.user {
            out.extend(["--user".to_owned(), user.clone()]);
        }
        for pair in &self.docker_env {
            out.extend(["--docker-env".to_owned(), pair.clone()]);
        }
        if self.no_machine {
            out.push("--no-machine".to_owned());
        }
        if self.web {
            out.push("--web".to_owned());
        }
        out
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
        Undertaking::open(&db, &args.id, brief.as_deref())
            .with_context(|| format!("open {} in {}", args.id, db.display()))?,
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
    let words = Prompts::load(&args.state, args.prompts.as_deref()).shared();
    let equip: ghost_work::worker::ToolsFor = {
        let (machine, words) = (machine.clone(), words.clone());
        Arc::new(move |undertaking| {
            let toolbox = if no_machine {
                tools::desk(undertaking, &words)
            } else {
                tools::standard(machine.clone(), undertaking, &words)
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
    let toolbox = if args.tell {
        toolbox.with(tools::Tell::new(undertaking.clone()))
    } else {
        toolbox
    };

    if args.serve_mcp {
        let input = tokio::io::BufReader::new(tokio::io::stdin());
        return Ok(mcp::serve(toolbox, words, undertaking, input, tokio::io::stdout()).await?);
    }

    // A first look around, so the first steps are not spent finding out
    // which system this is and what is in the directory.
    let mut place = if args.no_machine {
        prompts::hold(&words).text("no_machine").to_owned()
    } else {
        machine.describe()
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

    let report = match args.runtime.as_str() {
        "claude" | "codex" => {
            let me = std::env::current_exe().context("find this program")?;
            let state = args
                .state
                .canonicalize()
                .with_context(|| format!("resolve {}", args.state.display()))?;
            let mut tool_server = vec![
                me.display().to_string(),
                "--serve-mcp".to_owned(),
                "--state".to_owned(),
                state.display().to_string(),
                "--id".to_owned(),
                args.id.clone(),
            ];
            // A relative directory would mean something else to the child.
            let mut place_args = args.place();
            if let Some(dir) = &args.local
                && let Some(at) = place_args.iter().position(|a| a == "--local")
            {
                place_args[at + 1] = dir.canonicalize()?.display().to_string();
            }
            tool_server.extend(place_args);
            if let Some(dir) = &args.prompts {
                tool_server.extend([
                    "--prompts".to_owned(),
                    dir.canonicalize()?.display().to_string(),
                ]);
            }
            let by_codex = args.runtime == "codex";
            let seat = claude::Seat {
                agent: CliAgent {
                    kind: if by_codex {
                        CliKind::Codex
                    } else {
                        CliKind::Claude
                    },
                    program: if by_codex {
                        args.codex_bin.clone()
                    } else {
                        args.claude_bin.clone()
                    },
                    model: args.model.clone(),
                    args: args.agent_args.clone(),
                },
                system: system_prompt(&prompts::hold(&words), "", &place, &limits),
                opening: opening_text(&prompts::hold(&words), &undertaking.state()),
                undertaking,
                tool_server,
                max_turns: args.max_steps,
                max_seconds: args.max_minutes * 60,
                stop: Arc::new(AtomicBool::new(false)),
            };
            if by_codex {
                codex::sit(&seat).await
            } else {
                claude::sit(&seat).await
            }
        }
        "native" => {
            let key = std::env::var(&args.api_key_env)
                .ok()
                .filter(|k| !k.trim().is_empty())
                .with_context(|| format!("set {} to the API key", args.api_key_env))?;
            let model = args
                .model
                .clone()
                .unwrap_or_else(|| "deepseek-flash".to_owned());
            let brain: Arc<dyn Brain> = match args.kind.as_str() {
                "openai" => Arc::new(
                    OpenAi::new(key, model, Some(args.base_url.clone()), None, false)
                        .with_window(args.context_window),
                ),
                "anthropic" => Arc::new(
                    Anthropic::new(key, model, Some(args.base_url.clone()))
                        .with_window(args.context_window),
                ),
                other => bail!("unknown kind {other:?}: use openai or anthropic"),
            };
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
            }
            .run()
            .await
        }
        other => bail!("unknown runtime {other:?}: use native, claude or codex"),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
