//! What a character has to work with, made from her `[mind.work]`: the
//! machine her shell runs on, the web, her servers of tools and her skills.

use std::{path::Path, sync::Arc};

use anyhow::{Context, bail};
use tracing::warn;

use crate::config::Hers;

/// What she has to work with, from her `[mind.work]`. Everything in it that
/// keeps anything is under her own directory.
pub(crate) fn workshop(
    hers: &Hers,
    ghost_dir: &Path,
    agents: Vec<(String, ghost_brain::CliAgent)>,
) -> anyhow::Result<ghost_mind::Workshop> {
    use ghost_work::{Docker, Local, Machine, tools::WebConfig};
    let cfg = &hers.mind.work;
    let mut local = None;
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
                let dir = dir.canonicalize()?;
                local = Some(dir.clone());
                Some(Arc::new(Local::new(dir)))
            }
            _ => bail!(
                "work.machine is {spec:?}: use \"docker:<container>\" or \"local:<directory>\""
            ),
        },
    };
    // An installed agent works in her machine's directory, so only where
    // that is a directory on this computer.
    let agents = match &local {
        Some(dir) => agents
            .into_iter()
            .map(|(name, cli)| ghost_work::agents::Agent {
                name,
                cli,
                dir: dir.clone(),
            })
            .collect(),
        None => Vec::new(),
    };
    Ok(ghost_mind::Workshop {
        machine,
        web: if cfg.web {
            WebConfig::from_env()
        } else {
            WebConfig::default()
        },
        servers: servers(hers, ghost_dir)?,
        // Kept under `skills/` in her directory. Always there, so that the
        // skills she has stay hers when she may no longer install more.
        skills: Some(ghost_work::skills::Skills::new(
            hers.skills.clone(),
            ghost_dir.join("skills"),
        )),
        agents,
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
