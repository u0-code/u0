//! `ghostd run`, the daemon: it listens, makes each of the characters it
//! is given in full, answers, and keeps them alive until she is told to
//! stop.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, bail};
use ghost_core::Ghost;
use ghost_mind::{Mind, Numb, OpenMeteo, Parts, Prompts, Senses};
use ghost_protocol::GhostCard;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use tracing::{Instrument as _, info, info_span, warn};

use crate::{
    brains::served::Held,
    cast::{self, Cast, Character, Life, Maker, Pauses},
    config::{Config, Hers},
    prompts::{prompts_for, say_overrides, say_stale},
    server::{self, Presences, Shared},
    stop::told_to_stop,
    workshop::workshop,
};

/// How the characters are run, besides which they are.
pub(crate) struct How<'a> {
    /// Path to ghostd.toml, if not where it is looked for.
    pub config: Option<&'a Path>,
    /// Live only as long as whoever started this.
    pub with_parent: bool,
    /// Where to listen, in place of what ghostd.toml says.
    pub listen: Option<String>,
    /// Prompts built in for them in place of the binary's own, by their
    /// file in the tree: a home's are the desk's.
    pub prompts: &'static [(&'static str, &'static str)],
    /// The home it runs, whose brains and keys an operator may change
    /// through the protocol.
    pub home: Option<&'a Path>,
}

pub(crate) async fn run(paths: &[PathBuf], how: How<'_>) -> anyhow::Result<()> {
    let How {
        config,
        with_parent,
        listen,
        prompts: over,
        home,
    } = how;
    let (mut cfg, cfg_path) = Config::settle(config).await?;
    if let Some(addr) = listen {
        cfg.listen.addr = addr;
    }
    let token = cfg.listen.token()?;
    if let Some(path) = &cfg_path {
        info!("config {}", path.display());
    }
    let found = cast::find(paths)?;
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    cfg.check_ids(&ids)?;
    let hers: Vec<Hers> = ids
        .iter()
        .map(|id| cfg.hers(id))
        .collect::<anyhow::Result<_>>()?;
    no_shared_machine(&ids, &hers)?;
    // Before any of them is started: a core that does not know which it
    // is stops here.
    let core = crate::host::core(&crate::host::keeps_in(
        home,
        found
            .first()
            .map_or(Path::new("."), |(_, dir)| dir.as_path()),
    ))?;

    // The prompts are the deployment's, read by every character's mind for
    // itself; what there is to say about them is said once.
    let deployment = cfg.prompts()?;
    let prompts = Prompts::load_over(deployment.as_deref(), over);
    if !over.is_empty() {
        let files: Vec<&str> = over.iter().map(|(file, _)| *file).collect();
        info!(
            "built in for them in place of a character's: {}",
            files.join(", ")
        );
    }
    say_overrides(&prompts);
    say_stale(&prompts, over);

    // Listened on before they are made, which can take seconds when her
    // memory is rebuilt from her journal: whoever connects meanwhile waits
    // to be answered rather than being turned away, and the desk does not
    // take an address that is slow to answer for one nothing listens on.
    // Told to stop before anything is said about listening, so that a
    // SIGTERM sent the moment she is up still stops her in order.
    let told_to_stop = told_to_stop(with_parent);
    let listener = TcpListener::bind(&cfg.listen.addr)
        .await
        .with_context(|| format!("listen on {}", cfg.listen.addr))?;
    info!(
        "listening on ws://{}{}; whoever connects is answered once each of them is up",
        cfg.listen.addr,
        if token.is_some() {
            " (token required)"
        } else {
            ""
        }
    );

    // Each is made in full before anyone is answered: one that cannot
    // be started stops the daemon here, as a single ghost always has.
    let agents = Arc::new(crate::brains::ready_agents(&cfg).await);
    // What each mind is made with is asked of this each time one is made,
    // so that a change an operator makes to the brains reaches the minds
    // started again after it.
    let held = Arc::new(Held::new(
        Arc::new(cfg),
        cfg_path,
        home.map(Path::to_path_buf),
    ));
    let cfg = held.now();
    let mut cast = Vec::new();
    let mut lives = Vec::new();
    for ((id, dir), hers) in found.into_iter().zip(hers) {
        let span = info_span!("ghost", id = %id);
        let hers = Arc::new(hers);
        let body = Arc::new(Presences::default());
        let (life, card) = span.in_scope(|| {
            begin(
                &cfg,
                &hers,
                &dir,
                (deployment.as_deref(), over),
                &body,
                &agents,
            )
        })?;
        let make: Maker = {
            let (held, hers, dir, deployment, body, agents) = (
                held.clone(),
                hers.clone(),
                dir.clone(),
                deployment.clone(),
                body.clone(),
                agents.clone(),
            );
            Box::new(move || {
                begin(
                    &held.now(),
                    &hers,
                    &dir,
                    (deployment.as_deref(), over),
                    &body,
                    &agents,
                )
                .map(|(life, _)| life)
            })
        };
        let who = Arc::new(Character::new(id, dir, card, body, life.handle.clone()));
        cast.push(who.clone());
        lives.push((who, life, make, span));
    }
    info!(
        "core {} on {}",
        core.id,
        core.host.as_deref().unwrap_or("a computer with no name")
    );

    info!("answering on ws://{}", cfg.listen.addr);
    let server = tokio::spawn(server::serve(
        listener,
        Arc::new(Shared {
            cast: Cast(cast),
            brains: held,
            token,
            origins: cfg.listen.origins.clone(),
            core,
        }),
    ));

    let (stop, stopping) = watch::channel(false);
    let mut keepers = JoinSet::new();
    for (who, life, make, span) in lives {
        let keeping = cast::keep(who, life, make, stopping.clone(), Pauses::default());
        keepers.spawn(keeping.instrument(span));
    }
    tokio::select! {
        why = told_to_stop => {
            info!("{why}: stopping");
            stop.send_replace(true);
        }
        // Every mind stopped of its own accord: there is nobody to serve.
        () = async { while keepers.join_next().await.is_some() {} } => {}
    }
    while keepers.join_next().await.is_some() {}
    server.abort();
    Ok(())
}

/// Open her directory and make her mind from it, living as `hers` says,
/// with `body` as her body and her prompts from `prompts`: the
/// deployment's directory, over what is built in for her over the
/// binary's own. Says what she runs on. Done once at start and again each
/// time her mind is started after it failed or after her brains were
/// changed, so that all that carries over is what is on disk and the
/// brains the daemon holds now.
fn begin(
    cfg: &Config,
    hers: &Hers,
    dir: &Path,
    (deployment, over): (Option<&Path>, &[(&str, &'static str)]),
    body: &Arc<Presences>,
    agents: &[(String, ghost_brain::CliAgent)],
) -> anyhow::Result<(Life, GhostCard)> {
    let ghost = Ghost::open(dir).with_context(|| format!("open {}", dir.display()))?;
    let seed = ghost.identity().seed();
    let card = GhostCard {
        name: seed.name.clone(),
        pronouns: seed.pronouns.clone(),
        city: seed.home.city.clone(),
        country: seed.home.country.clone(),
        seal: ghost.identity().seal().to_string(),
        bio: seed.bio.clone(),
    };
    let (think, tick) = (cfg.think()?, cfg.tick()?);
    let mind = hers.mind();
    info!(
        "{} is in {}. seal {}. {}",
        card.name,
        card.city,
        ghost.identity().seal().short(),
        dir.display()
    );
    info!(
        "think on {}, tick on {}, tempo {}, at most {} calls an hour",
        think.label(),
        tick.label(),
        mind.tempo,
        mind.calls_per_hour
    );
    let senses: Arc<dyn Senses> = if mind.weather {
        Arc::new(OpenMeteo::new())
    } else {
        Arc::new(Numb)
    };
    let workshop = workshop(hers, dir, agents.to_vec())?;
    if mind.work.enabled {
        info!(
            "work: {}, {}",
            mind.work.machine.as_deref().unwrap_or("no machine"),
            if workshop.web.search.is_some() {
                "can search the web"
            } else {
                "cannot search the web"
            }
        );
    }
    if !mind.operators.accounts.is_empty() {
        info!(
            "run by accounts on {}",
            mind.operators
                .accounts
                .iter()
                .map(|(channel, ids)| format!("{channel} ({})", ids.len()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let (mind, handle) = Mind::new(Parts {
        ghost,
        think,
        tick,
        body: body.clone(),
        senses,
        prompts: prompts_for(deployment, over),
        config: mind,
        workshop,
    });
    Ok((
        Life {
            run: Box::pin(mind.run()),
            handle,
        },
        card,
    ))
}

/// Refuse two characters whose machine is the same directory of the same
/// container: each one's work would be in the other's way. A `local:`
/// machine is always under her own directory, and so always hers.
fn no_shared_machine(ids: &[&str], hers: &[Hers]) -> anyhow::Result<()> {
    let mut seen: Vec<(&str, Option<&str>, &str)> = Vec::new();
    for (id, hers) in ids.iter().zip(hers) {
        let work = &hers.mind.work;
        let Some(("docker", container)) = work
            .machine
            .as_deref()
            .and_then(|m| m.trim().split_once(':'))
        else {
            continue;
        };
        let workdir = work.workdir.as_deref();
        if let Some((_, _, other)) = seen
            .iter()
            .find(|(c, w, _)| *c == container && *w == workdir)
        {
            bail!(
                "{other} and {id} would work in the same directory of the container {container}; \
                 give one of them a workdir of her own under [ghosts.{id}.mind.work]"
            );
        }
        seen.push((container, workdir, id));
    }
    Ok(())
}

/// Make `[network]`, under whatever the environment sets, the way every
/// request from this process and the programs it starts leaves the machine.
pub(crate) fn network(setting: &ghost_net::Setting) -> anyhow::Result<()> {
    let network = ghost_net::Network::from_env(setting).context("[network]")?;
    info!("network: {network}");
    if !ghost_net::install(network) {
        warn!("the network setting came after the first request, which went the default way");
    }
    Ok(())
}
