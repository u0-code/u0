//! `ghostd brains`: the brains a deployment is configured with, or the ones
//! this machine could think with.

use std::{path::Path, time::Instant};

use ghost_brain::Completion;

use crate::{config::Config, run::network};

/// Say what this machine could think with, as configuration to paste.
pub(crate) async fn discover() -> anyhow::Result<()> {
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

pub(crate) async fn brains(probe: bool, config: Option<&Path>) -> anyhow::Result<()> {
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
