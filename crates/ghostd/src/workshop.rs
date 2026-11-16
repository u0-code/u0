//! What a character has to work with, made from her `[mind.work]`: the
//! machine her shell runs on, the web, her servers of tools and her skills.

use std::{path::Path, sync::Arc};

use anyhow::{Context, bail};
use tracing::warn;

use crate::config::Hers;

/// What she has to work with, from her `[mind.work]`. Everything in it that
/// keeps anything is under her own directory.
pub(crate) fn workshop(hers: &Hers, ghost_dir: &Path) -> anyhow::Result<ghost_mind::Workshop> {
    use ghost_work::{Docker, Local, Machine, tools::WebConfig};
    let cfg = &hers.mind.work;
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
        servers: servers(hers, ghost_dir)?,
        // Kept under `skills/` in her directory. Always there, so that the
        // skills she has stay hers when she may no longer install more.
        skills: Some(ghost_work::skills::Skills::new(
            hers.skills.clone(),
            ghost_dir.join("skills"),
        )),
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
